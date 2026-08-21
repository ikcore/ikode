//! `web_search` — web-agent-only tool. Never offered to the lead: web research
//! is delegated through `web_research`, so untrusted web content only ever
//! reaches a sandboxed worker with no write/shell/orchestration tools.

use super::{obj, p};
use crate::tools::ToolHost;
use anyhow::Result;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct WebSearchArgs {
    pub query: String,
    pub max_results: Option<usize>,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "web_search".to_string(),
        description: Some(
            "Search the public web. Returns ranked results (title, URL, snippet). Each call spends one unit of this research task's fixed search budget, so make queries sharp and stop as soon as you can answer."
                .to_string(),
        ),
        parameters: Some(obj(
            vec![
                ("query", p("string", "The search query")),
                ("max_results", p("integer", "Max results to return (default 5, cap 10)")),
            ],
            vec!["query"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: WebSearchArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    let query = args.query.trim();
    if query.is_empty() {
        return Ok("Error: search query cannot be empty.".to_string());
    }
    let max_results = args.max_results.unwrap_or(5).clamp(1, 10);
    let Some(session) = host.web_session() else {
        return Ok("Web search is unavailable in this host; use web_research from the lead.".to_string());
    };
    match session.search(query, max_results).await {
        Ok(Some(results)) => Ok(results),
        Ok(None) => Ok(
            "Search budget exhausted for this research task. Answer now with what you have, citing the sources already gathered."
                .to_string(),
        ),
        Err(error) => Ok(format!("Search failed: {error:#}")),
    }
}
