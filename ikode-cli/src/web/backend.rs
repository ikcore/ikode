//! Search backends for the web-research agent. One enum behind one interface —
//! the same model-agnostic discipline GAISe applies to inference providers.
//! All are plain HTTPS + JSON (via the workspace's existing `reqwest`), so no
//! new dependencies: Brave and Tavily are hosted APIs keyed via the standard
//! dotenv path; SearXNG is a self-hosted endpoint for the local-first story.

use anyhow::{anyhow, Context, Result};

use super::SearchResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchBackend {
    Brave { key: String },
    Tavily { key: String },
    SearxNG { endpoint: String },
}

impl SearchBackend {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Brave { .. } => "brave",
            Self::Tavily { .. } => "tavily",
            Self::SearxNG { .. } => "searxng",
        }
    }

    /// Resolve the configured backend from config + environment. Pure given the
    /// lookup closure (tests inject one; callers pass `std::env::var`-backed).
    /// `preferred` pins a backend (erroring when its credential is missing);
    /// otherwise the first available of Brave → Tavily → SearXNG wins. `Err`
    /// carries the human-readable reason web research is unavailable.
    pub fn resolve(
        preferred: Option<&str>,
        endpoint: Option<&str>,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, String> {
        let brave = env("BRAVE_API_KEY").filter(|value| !value.trim().is_empty());
        let tavily = env("TAVILY_API_KEY").filter(|value| !value.trim().is_empty());
        let searx = endpoint
            .map(str::to_string)
            .or_else(|| env("SEARXNG_URL"))
            .filter(|value| !value.trim().is_empty());
        match preferred.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("brave") => brave
                .map(|key| Self::Brave { key })
                .ok_or_else(|| "web_search_backend = \"brave\" but BRAVE_API_KEY is not set".into()),
            Some("tavily") => tavily
                .map(|key| Self::Tavily { key })
                .ok_or_else(|| "web_search_backend = \"tavily\" but TAVILY_API_KEY is not set".into()),
            Some("searxng") => searx
                .map(|endpoint| Self::SearxNG { endpoint })
                .ok_or_else(|| {
                    "web_search_backend = \"searxng\" but no web_search_endpoint / SEARXNG_URL is set"
                        .into()
                }),
            Some(other) => Err(format!(
                "unknown web_search_backend '{other}' (expected brave, tavily, or searxng)"
            )),
            None => brave
                .map(|key| Self::Brave { key })
                .or(tavily.map(|key| Self::Tavily { key }))
                .or(searx.map(|endpoint| Self::SearxNG { endpoint }))
                .ok_or_else(|| {
                    "no search backend: set BRAVE_API_KEY or TAVILY_API_KEY (in .ikode/.ikenv), or configure a SearXNG web_search_endpoint"
                        .into()
                }),
        }
    }

    /// Run one search and return up to `count` ranked results.
    pub async fn search(
        &self,
        client: &reqwest::Client,
        query: &str,
        count: usize,
    ) -> Result<Vec<SearchResult>> {
        let json = match self {
            Self::Brave { key } => {
                let response = client
                    .get("https://api.search.brave.com/res/v1/web/search")
                    .query(&[("q", query), ("count", &count.to_string())])
                    .header("X-Subscription-Token", key)
                    .header("Accept", "application/json")
                    .send()
                    .await
                    .context("brave search request")?;
                error_for_status(response, "brave").await?
            }
            Self::Tavily { key } => {
                let response = client
                    .post("https://api.tavily.com/search")
                    .bearer_auth(key)
                    .json(&serde_json::json!({ "query": query, "max_results": count }))
                    .send()
                    .await
                    .context("tavily search request")?;
                error_for_status(response, "tavily").await?
            }
            Self::SearxNG { endpoint } => {
                let url = format!("{}/search", endpoint.trim_end_matches('/'));
                let response = client
                    .get(url)
                    .query(&[("q", query), ("format", "json")])
                    .send()
                    .await
                    .context("searxng search request")?;
                error_for_status(response, "searxng").await?
            }
        };
        let results = parse_results(self.name(), &json, count);
        if results.is_empty() {
            return Err(anyhow!("{} returned no results for this query", self.name()));
        }
        Ok(results)
    }
}

async fn error_for_status(response: reqwest::Response, backend: &str) -> Result<serde_json::Value> {
    let status = response.status();
    if !status.is_success() {
        // Bodies of failed calls can carry account details; report only the code.
        return Err(anyhow!("{backend} search failed with HTTP {status}"));
    }
    response
        .json::<serde_json::Value>()
        .await
        .with_context(|| format!("{backend} returned non-JSON"))
}

/// Pure response mapping — one place per backend's JSON shape, unit-tested with
/// fixtures so a drifting API surfaces as a parse test failure, not a mystery.
pub fn parse_results(backend: &str, json: &serde_json::Value, count: usize) -> Vec<SearchResult> {
    let items = match backend {
        "brave" => json["web"]["results"].as_array(),
        _ => json["results"].as_array(),
    };
    let snippet_key = match backend {
        "brave" => "description",
        _ => "content",
    };
    items
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let url = item["url"].as_str()?.to_string();
                    let title = item["title"].as_str().unwrap_or("(untitled)").to_string();
                    let snippet = item[snippet_key].as_str().unwrap_or("").to_string();
                    Some(SearchResult {
                        title,
                        url,
                        snippet,
                    })
                })
                .take(count)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key: &str| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.to_string())
        }
    }

    #[test]
    fn resolve_prefers_brave_then_tavily_then_searxng() {
        let all = env_of(&[("BRAVE_API_KEY", "b"), ("TAVILY_API_KEY", "t")]);
        assert_eq!(
            SearchBackend::resolve(None, Some("http://sx.example"), &all).unwrap().name(),
            "brave"
        );
        let tavily_only = env_of(&[("TAVILY_API_KEY", "t")]);
        assert_eq!(
            SearchBackend::resolve(None, None, &tavily_only).unwrap().name(),
            "tavily"
        );
        let none = env_of(&[]);
        assert_eq!(
            SearchBackend::resolve(None, Some("http://sx.example"), &none).unwrap(),
            SearchBackend::SearxNG {
                endpoint: "http://sx.example".to_string()
            }
        );
        assert!(SearchBackend::resolve(None, None, &none)
            .unwrap_err()
            .contains("BRAVE_API_KEY"));
    }

    #[test]
    fn resolve_honours_and_validates_the_pinned_backend() {
        let brave_only = env_of(&[("BRAVE_API_KEY", "b")]);
        assert!(SearchBackend::resolve(Some("tavily"), None, &brave_only)
            .unwrap_err()
            .contains("TAVILY_API_KEY"));
        assert_eq!(
            SearchBackend::resolve(Some("Brave"), None, &brave_only).unwrap().name(),
            "brave"
        );
        assert!(SearchBackend::resolve(Some("bing"), None, &brave_only)
            .unwrap_err()
            .contains("unknown web_search_backend"));
    }

    #[test]
    fn parses_brave_and_tavily_and_searxng_shapes() {
        let brave = serde_json::json!({"web": {"results": [
            {"title": "One", "url": "https://a", "description": "first"},
            {"title": "Two", "url": "https://b", "description": "second"},
            {"url": "https://no-title", "description": "kept with placeholder"}
        ]}});
        let parsed = parse_results("brave", &brave, 2);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].title, "One");
        assert_eq!(parsed[1].snippet, "second");

        let tavily = serde_json::json!({"results": [
            {"title": "T", "url": "https://t", "content": "tavily snippet"}
        ]});
        assert_eq!(parse_results("tavily", &tavily, 5)[0].snippet, "tavily snippet");

        let searx = serde_json::json!({"results": [
            {"title": "S", "url": "https://s", "content": "searx snippet"}
        ]});
        assert_eq!(parse_results("searxng", &searx, 5)[0].url, "https://s");
        assert!(parse_results("brave", &serde_json::json!({}), 3).is_empty());
    }
}
