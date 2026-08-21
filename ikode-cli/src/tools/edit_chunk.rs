use super::{match_line_endings, obj, p};
use crate::harness;
use crate::tools::ToolHost;
use anyhow::Result;
use colored::Colorize;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;
use std::io::{self, Write};

#[derive(Deserialize)]
pub struct EditChunkArgs {
    pub chunk_id: String,
    pub new_text: String,
    pub impact_depth: Option<usize>,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "edit_chunk".to_string(),
        description: Some("Replaces a whole graph chunk (a function/method/struct/trait/module body) in place, addressed by chunk_id or symbol name rather than by search text — the structural counterpart to edit_file. Before applying it reports the blast radius: the dependent chunks (callers, traversed recursively up to impact_depth, hard cap 5) and the tests guarding them, so a breaking change is visible up front. new_text replaces the chunk's exact on-disk span verbatim, so supply the complete, correctly-indented replacement body. Use this to edit a chunk you located via ask_codebase/search_code; use find_references first (or in plan mode) for a read-only impact check.".to_string()),
        parameters: Some(obj(
            vec![
                ("chunk_id", p("string", "The chunk to replace: a chunk_id (path::name) or a symbol name. A bare name that matches several chunks is rejected — pass the full chunk_id.")),
                ("new_text", p("string", "The complete replacement source for the chunk, correctly indented. Replaces the chunk's current text exactly.")),
                ("impact_depth", p("integer", "How many CALLS/REFERENCES hops of dependents to report before editing (1-5). Defaults to 2.")),
            ],
            vec!["chunk_id", "new_text"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: EditChunkArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    println!(
        "{} Editing chunk: {}",
        "✂️ ".bright_yellow(),
        args.chunk_id.bold().bright_yellow()
    );

    if host.indexer().is_empty() {
        host.indexer().index_all();
    }

    let matches = host.indexer().resolve_chunks(&args.chunk_id);
    let rec = match matches.len() {
        0 => {
            return Ok(format!(
                "No chunk matches '{}'. Pass a chunk_id (path::name) or symbol name from search_code/ask_codebase, and ensure the codebase is indexed.",
                args.chunk_id
            ));
        }
        1 => matches.into_iter().next().unwrap(),
        n => {
            let ids: Vec<String> = matches.iter().map(|c| c.chunk_id.clone()).collect();
            return Ok(format!(
                "'{}' is ambiguous — {} chunks share that name. Pass the full chunk_id (path::name):\n  {}",
                args.chunk_id,
                n,
                ids.join("\n  ")
            ));
        }
    };

    // Blast radius: who depends on this chunk (recursive, depth-capped) and which
    // tests guard it — surfaced before the edit so a breaking change is visible.
    let depth = args.impact_depth.unwrap_or(2).clamp(1, 5);
    let (_, callers) = host.indexer().impact(&rec.chunk_id, depth, true);
    let mut ids: Vec<String> = vec![rec.chunk_id.clone()];
    ids.extend(callers.iter().map(|h| h.chunk_id.clone()));
    let tests = host.indexer().guarding_tests(&ids);

    let mut impact = String::new();
    if callers.is_empty() {
        impact.push_str(
            "Blast radius: no dependents reach this chunk via CALLS/REFERENCES (low risk).\n",
        );
    } else {
        impact.push_str(&format!(
            "Blast radius: {} dependent chunk(s) reference this (depth {}):\n",
            callers.len(),
            depth
        ));
        for h in &callers {
            impact.push_str(&format!(
                "  [d{}] {} {} — {}:{}-{}\n",
                h.depth, h.kind, h.name, h.path, h.line_start, h.line_end
            ));
        }
    }
    if !tests.is_empty() {
        impact.push_str(&format!("Guarding tests ({}):\n", tests.len()));
        for t in &tests {
            impact.push_str(&format!(
                "  {} {} — {}:{}-{}\n",
                t.kind, t.name, t.path, t.line_start, t.line_end
            ));
        }
    }

    let validated_path = match host.validate_path(&rec.path) {
        Ok(p) => p,
        Err(e) => return Ok(format!("Error: {}", e)),
    };
    let content = match std::fs::read_to_string(&validated_path) {
        Ok(c) => c,
        Err(e) => return Ok(format!("Error reading file: {}", e)),
    };

    // Match the file's line endings (chunk text comes from the LF-stripped index, but
    // the file on disk may be CRLF), so the match and rewrite stay consistent.
    let crlf = content.contains("\r\n");
    let old_text = match_line_endings(&rec.indented_code(), crlf);
    let new_text = match_line_endings(&args.new_text, crlf);

    let count = content.matches(&old_text).count();
    if count == 0 {
        println!(
            "{} chunk text for {} no longer matches {} — no change made.",
            "⚠️ ".yellow(),
            rec.chunk_id.yellow(),
            rec.path.yellow()
        );
        return Ok(format!(
            "Error: the indexed text for '{}' no longer matches {} on disk — the file changed since indexing. Re-run index_codebase, or use edit_file.",
            rec.chunk_id, rec.path
        ));
    }
    if count > 1 {
        println!(
            "{} chunk text for {} appears {} times in {} — no change made.",
            "⚠️ ".yellow(),
            rec.chunk_id.yellow(),
            count,
            rec.path.yellow()
        );
        return Ok(format!(
            "Error: the text of '{}' appears {} times in {}; cannot edit unambiguously by chunk. Use edit_file with more surrounding context.",
            rec.chunk_id, count, rec.path
        ));
    }

    print!(
        "{}",
        harness::render_file_diff(&rec.path, &content, &old_text, &new_text)
    );
    print!("{}", impact.dimmed());
    let _ = io::stdout().flush();

    let permission_path = host.permission_path(&validated_path);
    if !host.ask_permission(
        "edit_chunk",
        &permission_path,
        &format!("Replace chunk {}", rec.chunk_id),
    )? {
        return Ok("Chunk edit not applied (denied or cancelled). If in planning mode, switch with /mode agentic.".to_string());
    }

    let new_content = content.replacen(&old_text, &new_text, 1);
    match std::fs::write(&validated_path, &new_content) {
        Ok(_) => {
            host.reindex_path(&validated_path);
            Ok(format!(
                "Chunk '{}' replaced in {}.\n{}",
                rec.chunk_id, rec.path, impact
            ))
        }
        Err(e) => Ok(format!("Error writing file: {}", e)),
    }
}
