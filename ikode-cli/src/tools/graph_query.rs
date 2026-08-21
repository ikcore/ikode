use super::{arr, obj, p};
use crate::index::{GraphQuerySpec, QueryRow};
use crate::tools::ToolHost;
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::{GaiseTool, GaiseToolParameter};

pub fn spec() -> GaiseTool {
    let start = obj(
        vec![
            ("symbol", p("string", "Start from nodes whose name OR chunk_id equals this.")),
            ("chunk_id", p("string", "Start from this exact chunk_id (path::name). Takes precedence over symbol.")),
            ("kind", p("string", "Restrict the start nodes to this label (Function, Method, Struct, File, Directory, ...).")),
        ],
        vec![],
    );
    let step = obj(
        vec![
            ("edge", p("string", "Edge label to follow: CALLS (invocations), REFERENCES (mentions), DEFINES, TESTS, IMPLEMENTS, EXTENDS, FOR_TYPE, CONTAINS.")),
            ("dir", p("string", "'out' (default) follows edges leaving the frontier (e.g. callees); 'in' follows edges entering it (e.g. callers).")),
            ("depth", p("integer", "Hops for this step (1-10). Defaults to 1.")),
        ],
        vec!["edge"],
    );
    let traverse = GaiseToolParameter {
        r#type: Some("array".to_string()),
        description: Some(
            "Ordered traversal steps applied to the start set; each step starts from the previous step's reached nodes. Omit to query the start set directly."
                .to_string(),
        ),
        properties: None,
        items: Some(Box::new(step)),
        required: None,
    };
    let where_ = obj(
        vec![
            ("kind", arr("string", "Keep only nodes with one of these labels (case-insensitive).")),
            ("path_prefix", p("string", "Keep only nodes whose file path starts with this (e.g. 'src/').")),
            ("visibility", p("string", "Keep only nodes with this visibility: public, private, protected, internal, crate, or default.")),
            ("name", p("string", "Keep only nodes whose name equals or contains this.")),
        ],
        vec![],
    );
    GaiseTool {
        name: "graph_query".to_string(),
        description: Some("Run a structured, deterministic traversal over the code graph (no inference). Resolve a start node set, walk labelled edges (CALLS / REFERENCES / DEFINES / TESTS / IMPLEMENTS / EXTENDS / FOR_TYPE / CONTAINS) in/out for N hops via `traverse`, narrow the result with `where`, and return matching chunks. More composable than find_references when you need filtering or multi-hop / multi-edge paths. Examples — callees of a function: start {symbol} + traverse [{edge:CALLS,dir:out,depth:2}]; callers only: dir:in; all public Functions under src/: where {kind:[Function],path_prefix:'src/',visibility:'public'} (no traverse); tests guarding a symbol: start {symbol} + traverse [{edge:TESTS,dir:in}].".to_string()),
        parameters: Some(obj(
            vec![
                ("start", start),
                ("traverse", traverse),
                ("where", where_),
                ("select", arr("string", "Columns to show: chunk_id, kind, name, path, lines. Defaults to all.")),
                ("limit", p("integer", "Max rows (1-1000). Defaults to 100.")),
            ],
            vec![],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let spec: GraphQuerySpec = serde_json::from_str(arguments.unwrap_or("{}"))?;
    println!("{} graph_query", "🕸️ ".bright_cyan());
    if host.indexer().is_empty() {
        host.indexer().index_all();
    }
    let select = spec.select.clone();
    match host.indexer().graph_query(&spec) {
        Err(e) => Ok(format!("graph_query error: {e}")),
        Ok(rows) if rows.is_empty() => Ok("graph_query: no matching nodes.".to_string()),
        Ok(rows) => Ok(render_rows(&rows, select.as_deref())),
    }
}

/// Render rows one per line, honouring an optional `select` column list. Shared
/// with the `/query` REPL command so tool and command output match.
pub fn render_rows(rows: &[QueryRow], select: Option<&[String]>) -> String {
    let default_cols = ["kind", "name", "path", "lines", "chunk_id"];
    let cols: Vec<String> = match select {
        Some(s) if !s.is_empty() => s.iter().map(|c| c.to_ascii_lowercase()).collect(),
        _ => default_cols.iter().map(|c| c.to_string()).collect(),
    };
    let mut out = format!("graph_query — {} row(s):\n", rows.len());
    for r in rows {
        let mut parts: Vec<String> = Vec::new();
        for c in &cols {
            match c.as_str() {
                "kind" => parts.push(r.kind.clone()),
                "name" => parts.push(r.name.clone()),
                "path" => parts.push(r.path.clone()),
                "lines" | "line_start" | "line_end" => {
                    parts.push(format!("{}-{}", r.line_start, r.line_end))
                }
                "chunk_id" => parts.push(format!("[{}]", r.chunk_id)),
                _ => {}
            }
        }
        out.push_str("  ");
        out.push_str(&parts.join(" "));
        out.push('\n');
    }
    out
}
