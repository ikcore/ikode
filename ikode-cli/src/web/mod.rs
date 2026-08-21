//! Web research for iKode — the model-agnostic, zero-new-dependency web layer.
//!
//! Architecture: the lead agent never touches the network. It calls the
//! `web_research` tool, which spawns a dedicated **web agent** (a bounded
//! subagent running on the `/wmodel` model) whose only tools are `web_search`
//! and `web_fetch`. Raw pages live and die inside that worker; the lead receives
//! a short synthesised answer with source URLs. That isolates prompt injection
//! from untrusted web content (the worker has no write/shell/orchestration
//! tools) and keeps page dumps out of the lead's context and cache prefix.
//!
//! Everything here rides dependencies already in the workspace: `reqwest` (used
//! by the GAISe providers and rmcp), `serde_json`, `tokio`. HTML extraction is
//! hand-rolled pure Rust ([`extract`]), internal hosts are refused ([`guard`]),
//! and JavaScript pages fall back to hydration-payload harvesting or a headless
//! render via the user's own Chrome/Edge/Chromium ([`chrome`]).

pub mod backend;
pub mod chrome;
pub mod extract;
pub mod fetch;
pub mod guard;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;

pub use backend::SearchBackend;

/// Default per-research budgets. Deliberately small: research tasks are meant
/// to be sharp, and the web agent is told to answer as soon as it can.
pub const DEFAULT_MAX_SEARCHES: usize = 3;
pub const DEFAULT_MAX_FETCHES: usize = 4;
/// Default cap on concurrently running web agents (within the overall subagent
/// thread cap).
pub const DEFAULT_MAX_WEB_AGENTS: usize = 2;
/// Default model-turn budget for one web agent (well under the code-subagent
/// default: search → fetch → answer should be short).
pub const DEFAULT_WEB_AGENT_TURNS: usize = 8;
/// Snippet cap in formatted search results, protecting the worker's context.
const SNIPPET_CHARS: usize = 300;
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// One ranked search hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// A fetched page reduced to readable text. `via` records how the text was
/// obtained: `"static"`, `"hydration-json"`, `"chrome"`, or `"js-shell"` (the
/// page needs JavaScript and no fallback succeeded).
#[derive(Debug, Clone)]
pub struct FetchedPage {
    pub url: String,
    pub title: Option<String>,
    pub text: String,
    pub truncated: bool,
    pub via: &'static str,
}

/// Session-resolved web capability: which backend (if any), whether a browser
/// is available for JS rendering, and the per-research budgets. Resolved once
/// at startup and cloned into each web-agent launch.
///
/// `backend: None` is **fetch-only mode**: fetching needs no credential (static
/// HTTP and the headless-Chrome fallback are both free), so `web_research`
/// stays available for questions that carry their URLs — only searching is
/// withheld until a backend key/endpoint is configured.
#[derive(Debug, Clone)]
pub struct WebConfig {
    pub backend: Option<SearchBackend>,
    pub browser: Option<PathBuf>,
    pub max_searches: usize,
    pub max_fetches: usize,
}

impl WebConfig {
    /// Human summary for the spawn permission prompt and status lines, e.g.
    /// `backend: brave · budget: 3 searches / 4 fetches · JS rendering: available`.
    pub fn describe(&self) -> String {
        format!(
            "backend: {} · budget: {} searches / {} fetches · JS rendering: {}",
            match &self.backend {
                Some(backend) => backend.name(),
                None => "none — fetch-only (explicit URLs)",
            },
            if self.backend.is_some() {
                self.max_searches
            } else {
                0
            },
            self.max_fetches,
            if self.browser.is_some() {
                "available"
            } else {
                "unavailable (static fetch only)"
            }
        )
    }
}

/// The per-web-agent state behind the `web_search` / `web_fetch` tools: HTTP
/// client, resolved backend, and the **structurally enforced** budget — once a
/// counter is spent the tools return a "budget exhausted" result, so no prompt
/// wording can extend a research task.
pub struct WebSession {
    config: WebConfig,
    client: reqwest::Client,
    searches_left: usize,
    fetches_left: usize,
}

impl WebSession {
    /// `depth` scales the budget: 0 = snippets only (no fetches), 1 = the
    /// configured budget, 2 = double budget for a thorough pass. Without a
    /// search backend the search budget is zero regardless of depth
    /// (fetch-only mode).
    pub fn new(config: WebConfig, depth: usize) -> Self {
        let depth = depth.min(2);
        let (mut searches_left, fetches_left) = match depth {
            0 => (config.max_searches, 0),
            1 => (config.max_searches, config.max_fetches),
            _ => (config.max_searches * 2, config.max_fetches * 2),
        };
        if config.backend.is_none() {
            searches_left = 0;
        }
        let client = reqwest::Client::builder()
            .user_agent(format!("ikode/{} (web-research)", env!("CARGO_PKG_VERSION")))
            .timeout(FETCH_TIMEOUT)
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                // Re-apply the SSRF guard on every redirect hop, so a public
                // page cannot bounce the fetcher into an internal service.
                let blocked = attempt
                    .url()
                    .host_str()
                    .and_then(guard::blocked_host_reason);
                if let Some(reason) = blocked {
                    return attempt.error(format!("redirect to blocked host ({reason})"));
                }
                if attempt.previous().len() >= 5 {
                    return attempt.stop();
                }
                attempt.follow()
            }))
            .build()
            .expect("reqwest client construction cannot fail with static options");
        Self {
            config,
            client,
            searches_left,
            fetches_left,
        }
    }

    pub fn budget_line(&self) -> String {
        format!(
            "{} search(es) and {} fetch(es) remaining",
            self.searches_left, self.fetches_left
        )
    }

    /// Run one search against the configured backend, spending budget only when
    /// the call succeeds (a flaky network must not eat the allowance).
    pub async fn search(&mut self, query: &str, max_results: usize) -> Result<Option<String>> {
        // Defensive: in fetch-only mode the web_search tool is not offered at
        // all, but a hallucinated call must still fail informatively.
        let Some(backend) = self.config.backend.clone() else {
            anyhow::bail!(
                "no search backend configured — this research task can only fetch explicit URLs"
            );
        };
        if self.searches_left == 0 {
            return Ok(None);
        }
        let results = backend.search(&self.client, query, max_results).await?;
        self.searches_left -= 1;
        Ok(Some(format_search_results(
            query,
            backend.name(),
            &results,
            &self.budget_line(),
        )))
    }

    /// Fetch one page as readable text; `None` when the fetch budget is spent.
    pub async fn fetch(&mut self, url: &str) -> Result<Option<String>> {
        if self.fetches_left == 0 {
            return Ok(None);
        }
        let page = fetch::fetch_page(&self.client, url, self.config.browser.as_deref()).await?;
        self.fetches_left -= 1;
        Ok(Some(format_page(&page, &self.budget_line())))
    }
}

/// Deterministic, token-disciplined search-result block.
pub fn format_search_results(
    query: &str,
    backend: &str,
    results: &[SearchResult],
    budget: &str,
) -> String {
    let mut out = format!("Results for \"{query}\" ({backend}):\n");
    for (index, result) in results.iter().enumerate() {
        let (snippet, _) = extract::truncate_chars(result.snippet.trim(), SNIPPET_CHARS);
        out.push_str(&format!(
            "{}. {}\n   {}\n   {}\n",
            index + 1,
            result.title.trim(),
            result.url,
            snippet.replace('\n', " ")
        ));
    }
    out.push_str(&format!(
        "\n[{budget}] Fetch a page with web_fetch only when a snippet is insufficient; cite the URLs you rely on."
    ));
    out
}

/// Deterministic page block, with explicit guidance when the page needed
/// JavaScript and could not be rendered.
pub fn format_page(page: &FetchedPage, budget: &str) -> String {
    let mut out = String::new();
    if let Some(title) = &page.title {
        out.push_str(&format!("# {title}\n"));
    }
    out.push_str(&format!("{} (via {})\n\n", page.url, page.via));
    if page.via == "js-shell" {
        out.push_str(
            "This page requires JavaScript to render and no fallback succeeded. Do not fetch it again — rely on the search snippet or try a different source.\n",
        );
    }
    out.push_str(page.text.trim());
    if page.truncated {
        out.push_str("\n\n[truncated]");
    }
    out.push_str(&format!("\n\n[{budget}]"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> WebConfig {
        WebConfig {
            backend: Some(SearchBackend::Brave {
                key: "k".to_string(),
            }),
            browser: None,
            max_searches: 3,
            max_fetches: 4,
        }
    }

    #[test]
    fn depth_scales_budgets_and_zero_disables_fetches() {
        let snippets_only = WebSession::new(config(), 0);
        assert_eq!(snippets_only.searches_left, 3);
        assert_eq!(snippets_only.fetches_left, 0);
        let default = WebSession::new(config(), 1);
        assert_eq!((default.searches_left, default.fetches_left), (3, 4));
        let thorough = WebSession::new(config(), 9); // clamped to 2
        assert_eq!((thorough.searches_left, thorough.fetches_left), (6, 8));
    }

    #[tokio::test]
    async fn fetch_only_mode_zeroes_searches_and_rejects_search_calls() {
        let fetch_only = WebConfig {
            backend: None,
            ..config()
        };
        let mut session = WebSession::new(fetch_only.clone(), 2);
        assert_eq!(session.searches_left, 0, "depth cannot buy searches back");
        assert_eq!(session.fetches_left, 8);
        let error = session.search("q", 5).await.unwrap_err();
        assert!(error.to_string().contains("no search backend"));
        assert!(fetch_only.describe().contains("fetch-only"));
    }

    #[tokio::test]
    async fn exhausted_budgets_return_none_without_touching_the_network() {
        let mut session = WebSession::new(config(), 0);
        assert!(session.fetch("https://example.com").await.unwrap().is_none());
        session.searches_left = 0;
        assert!(session.search("q", 5).await.unwrap().is_none());
    }

    #[test]
    fn formatted_results_number_rank_and_carry_budget_and_citation_nudge() {
        let results = vec![
            SearchResult {
                title: "One".to_string(),
                url: "https://a".to_string(),
                snippet: "first\nsnippet".to_string(),
            },
            SearchResult {
                title: "Two".to_string(),
                url: "https://b".to_string(),
                snippet: "x".repeat(400),
            },
        ];
        let block = format_search_results("q", "brave", &results, "2 left");
        assert!(block.contains("1. One"));
        assert!(block.contains("   https://a"));
        assert!(block.contains("first snippet"));
        assert!(block.contains("[2 left]"));
        assert!(block.contains("cite the URLs"));
        // Long snippets are capped.
        assert!(!block.contains(&"x".repeat(320)));
    }

    #[test]
    fn formatted_page_flags_js_shells_and_truncation() {
        let page = FetchedPage {
            url: "https://spa.example".to_string(),
            title: Some("Shell".to_string()),
            text: "tiny".to_string(),
            truncated: true,
            via: "js-shell",
        };
        let block = format_page(&page, "1 left");
        assert!(block.contains("# Shell"));
        assert!(block.contains("requires JavaScript"));
        assert!(block.contains("[truncated]"));
        assert!(block.contains("(via js-shell)"));
    }
}
