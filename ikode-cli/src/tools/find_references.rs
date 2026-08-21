use super::{obj, p};
use crate::tools::ToolHost;
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct FindReferencesArgs {
    pub symbol: String,
    pub depth: Option<usize>,
    pub direction: Option<String>,
    pub edge_kind: Option<String>,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "find_references".to_string(),
        description: Some("Impact / blast-radius analysis via the code graph (deterministic, no inference). Given a symbol name (or chunk_id), traverses CALLS (true invocations) + REFERENCES (looser mentions) edges to find what depends on it. direction='callers' (default) walks dependents (who would break if this changes); direction='callees' walks what it depends on. edge_kind filters which edges to follow: 'calls' (precise call graph only), 'references' (incidental mentions only — types in signatures, fields, same-named symbols), or 'all' (default). Returns each reached chunk with its path and line range. Use before editing a widely-used symbol.".to_string()),
        parameters: Some(obj(
            vec![
                ("symbol", p("string", "Symbol name (e.g. a function/struct/trait name) or a chunk_id (path::name)")),
                ("depth", p("integer", "Traversal depth (1-5). Defaults to 2.")),
                ("direction", p("string", "'callers' (dependents, default) or 'callees' (dependencies)")),
                ("edge_kind", p("string", "'calls' (true invocations only), 'references' (mentions only), or 'all' (default).")),
            ],
            vec!["symbol"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: FindReferencesArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    let callers = !matches!(args.direction.as_deref(), Some("callees"));
    let depth = args.depth.unwrap_or(2).clamp(1, 5);
    let edges = crate::index::DepEdges::parse(args.edge_kind.as_deref());
    let kind_label = match edges {
        crate::index::DepEdges::Calls => " via CALLS",
        crate::index::DepEdges::References => " via REFERENCES",
        crate::index::DepEdges::All => "",
    };
    println!(
        "{} {} of {} (depth {}{})",
        "🕸️ ".bright_cyan(),
        if callers { "callers" } else { "callees" },
        args.symbol.bright_cyan(),
        depth,
        kind_label
    );
    if host.indexer().is_empty() {
        host.indexer().index_all();
    }
    let (found, hits) = host
        .indexer()
        .impact_with(&args.symbol, depth, callers, edges);
    if !found {
        return Ok(format!(
            "Symbol '{}' was not found in the graph. Try a function/type name or a chunk_id (path::name), and ensure the codebase is indexed.",
            args.symbol
        ));
    }
    if hits.is_empty() {
        let rel = if callers { "callers" } else { "callees" };
        return Ok(format!(
            "'{}' has no {} in the graph (no CALLS/REFERENCES edges reach it).",
            args.symbol, rel
        ));
    }
    let rel = if callers {
        "Dependents (callers)"
    } else {
        "Dependencies (callees)"
    };
    let mut out = format!("{} of '{}' — {} chunk(s):\n", rel, args.symbol, hits.len());
    for h in &hits {
        out.push_str(&format!(
            "  [d{}] {} {} — {}:{}-{}\n",
            h.depth, h.kind, h.name, h.path, h.line_start, h.line_end
        ));
    }
    // In callers mode, also surface the tests guarding the symbol + its
    // dependents — the "what tests should I run?" answer, deterministically.
    if callers {
        let mut ids: Vec<String> = vec![args.symbol.clone()];
        ids.extend(hits.iter().map(|h| h.chunk_id.clone()));
        let tests = host.indexer().guarding_tests(&ids);
        if !tests.is_empty() {
            out.push_str(&format!("\nGuarding tests ({}):\n", tests.len()));
            for t in &tests {
                out.push_str(&format!(
                    "  {} {} — {}:{}-{}\n",
                    t.kind, t.name, t.path, t.line_start, t.line_end
                ));
            }
        }
    }
    Ok(out)
}
