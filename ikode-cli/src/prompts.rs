//! System prompts for iKode's internal sub-calls — the opt-in, hash-gated
//! inference passes (`/summarize`, `/relationships`, `/dir-summaries`,
//! `/architecture`). Centralised so every model call iKode makes to *itself* has
//! one authored, reviewable system prompt. They are deliberately terse to minimise
//! tokens (iKode's "minimise inference" principle).
//!
//! Retrieval — `ask_codebase`, `search_code`, `find_references` — is pure graph RAG
//! (keyword relevance fused with graph neighbourhood, plus optional cosine over the
//! per-chunk embedding property) and makes **no** model call, so it has no prompt
//! here.

use gaise_core::contracts::{GaiseContent, GaiseMessage, OneOrMany};

/// `/summarize`: one-line summary of a single code chunk.
pub const CHUNK_SUMMARY: &str = "You summarise one code chunk in a single concise sentence describing what it does. Reply with only the sentence — no preamble, no quotes.";

/// `/relationships`: one-line description of a typed edge between two chunks.
pub const RELATIONSHIP_SUMMARY: &str = "You are given two code chunks linked by a typed relationship (source -> target). In a single concise sentence, describe what the source uses the target for. Reply with only the sentence — no preamble.";

/// `/dir-summaries`: one-line rollup of what a directory is responsible for.
pub const DIRECTORY_SUMMARY: &str = "You describe, in a single concise sentence, what a directory contains and is responsible for, based only on its listed top-level elements. Reply with only the sentence — no preamble.";

/// `/architecture`: short prose overview of the whole codebase from graph stats.
pub const ARCHITECTURE: &str = "You are given the structural graph of a codebase (languages, node counts by kind, edge counts by relationship). Write a short prose overview (3-6 sentences) of how the codebase is organised and how its parts relate, based only on the provided data. Do not invent specifics.";

/// `/ask` answer pass: synthesise an answer from the small set of chunks the
/// embedding search retrieved, or signal that they are insufficient so the loop
/// fetches the next set. The `INSUFFICIENT` sentinel is matched exactly by the loop,
/// so the model must emit it alone — hence the explicit instruction.
pub const ASK_ANSWER: &str = "You answer a question about a codebase using ONLY the provided code chunks. Cite the chunks you rely on inline by their `path:line` reference. If the chunks do not contain enough information to answer the question, reply with exactly the single word INSUFFICIENT and nothing else. Be concise and do not invent code that is not shown.";

/// Build a `system` + `user` message pair for a sub-call: the authored system
/// prompt plus the data payload the model works on.
pub fn sys_user(system: &str, user_payload: String) -> OneOrMany<GaiseMessage> {
    OneOrMany::Many(vec![
        message("system", system.to_string()),
        message("user", user_payload),
    ])
}

fn message(role: &str, text: String) -> GaiseMessage {
    GaiseMessage {
        role: role.to_string(),
        content: Some(OneOrMany::One(GaiseContent::Text { text })),
        tool_calls: None,
        tool_call_id: None,
        tool_name: None,
    }
}
