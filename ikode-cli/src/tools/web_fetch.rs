//! `web_fetch` — web-agent-only tool. Fetches one URL as readable text
//! (boilerplate stripped). JavaScript pages fall back to embedded-hydration
//! harvesting, then a headless render via the user's own Chrome when one was
//! discovered at startup. Internal/private hosts are refused (SSRF guard).

use super::{obj, p};
use crate::tools::ToolHost;
use anyhow::Result;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct WebFetchArgs {
    pub url: String,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "web_fetch".to_string(),
        description: Some(
            "Fetch a URL and return its main readable text (boilerplate stripped, length-capped). Each call spends one unit of this research task's fixed fetch budget — fetch only pages whose search snippet is insufficient. A 'requires JavaScript' result means the page cannot be read; try another source instead of refetching."
                .to_string(),
        ),
        parameters: Some(obj(
            vec![("url", p("string", "Absolute http(s) URL to fetch"))],
            vec!["url"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: WebFetchArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    let url = args.url.trim();
    if url.is_empty() {
        return Ok("Error: url cannot be empty.".to_string());
    }
    let Some(session) = host.web_session() else {
        return Ok("Web fetch is unavailable in this host; use web_research from the lead.".to_string());
    };
    match session.fetch(url).await {
        Ok(Some(page)) => Ok(page),
        Ok(None) => Ok(
            "Fetch budget exhausted for this research task. Answer now from the search snippets and pages already read."
                .to_string(),
        ),
        Err(error) => Ok(format!("Fetch failed: {error:#}")),
    }
}
