use super::obj;
use crate::harness;
use crate::tools::ToolHost;
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "graph_overview".to_string(),
        description: Some("Summarizes the in-memory code graph: total nodes/edges, label breakdowns, and WAL file/delta/compression byte statistics. Useful to confirm the codebase is indexed, linked, and storage-bounded.".to_string()),
        parameters: Some(obj(vec![], vec![])),
    }
}

pub async fn execute(host: &mut dyn ToolHost, _arguments: Option<&str>) -> Result<String> {
    println!("{} Graph overview", "🕸️ ".bright_cyan());
    if host.indexer().is_empty() {
        host.indexer().index_all();
    }
    match host.indexer().graph_stats() {
        Some(stats) => Ok(harness::render_graph_stats(&stats)),
        None => Ok("Graph persistence is disabled; no graph available.".to_string()),
    }
}
