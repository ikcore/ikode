//! `web_research` — the lead-facing web capability. Spawns a dedicated,
//! budget-bounded web agent (its own conversation, its own `/wmodel` model,
//! only `web_search`/`web_fetch` tools) and returns immediately; the lead
//! collects the synthesised answer with `wait_agent`, or it is auto-surfaced
//! into the conversation when the worker finishes. Raw pages never enter the
//! lead's context, and web content can never reach a tool that writes.

use super::{obj, p};
use crate::tools::ToolHost;
use anyhow::Result;
use gaise_core::contracts::GaiseTool;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct WebResearchArgs {
    pub question: String,
    pub depth: Option<usize>,
}

pub fn spec() -> GaiseTool {
    GaiseTool {
        name: "web_research".to_string(),
        description: Some(
            "Delegate a question to a bounded web-research agent that searches the public web, reads the best sources, and returns a short synthesised answer with source URLs. Use for facts newer than your training, external docs/APIs/pricing, or anything not in this codebase — prefer codebase retrieval first. Runs in the background: continue other work, then collect the result with wait_agent (results are also auto-surfaced when finished)."
                .to_string(),
        ),
        parameters: Some(obj(
            vec![
                ("question", p("string", "The research question, with any context the researcher needs (it cannot see this conversation)")),
                ("depth", p("integer", "0 = search snippets only, 1 = also read the best pages (default), 2 = double budget for a thorough pass")),
            ],
            vec!["question"],
        )),
    }
}

pub async fn execute(host: &mut dyn ToolHost, arguments: Option<&str>) -> Result<String> {
    let args: WebResearchArgs = serde_json::from_str(arguments.unwrap_or("{}"))?;
    let question = args.question.trim();
    if question.is_empty() {
        return Ok("Error: research question cannot be empty.".to_string());
    }
    // Read-only but network-reaching: gated like a mutating tool (see
    // `settings::is_network`), with the question as the rule-matchable detail
    // so users can write allow/deny rules such as `web_research(*)`.
    let action = format!("Web research: {question}");
    if !host.ask_permission("web_research", question, &action)? {
        return Ok("Web research not run (denied or cancelled).".to_string());
    }
    host.spawn_web_agent(question, args.depth).await
}
