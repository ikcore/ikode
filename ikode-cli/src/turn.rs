//! The agent turn: send the conversation to the model, stream the reply, run any
//! tool calls it makes (feeding results back), and loop until it settles on a
//! final answer. Also the shared Ctrl+C race ([`App::cancellable`]) every long
//! operation rides, and the post-turn re-index/enrich.
//!
//! Two callers share the streaming core ([`App::stream_once`]):
//!   - [`App::process_prompt`] — the interactive prompt path, driving the shared
//!     `self.history`.
//!   - [`App::run_turn`] / [`App::start_goal`] / [`App::resume_goal`] — the
//!     autonomous goal path, driving an isolated [`GoalTask`]'s own history, mode,
//!     and cache lane via the shared [`App::drive_goal`] loop. `run_turn(task)` is
//!     the concurrency-enabling seam: a future scheduler spawns one per task.

use std::io::{self, Write};
use std::time::Duration;

use anyhow::{anyhow, Result};
use colored::*;
use futures_util::StreamExt;
use gaise_core::contracts::{
    GaiseContent, GaiseInstructRequest, GaiseMessage, GaiseStreamAccumulator, GaiseStreamChunk,
    GaiseToolCall, OneOrMany,
};
use indicatif::{ProgressBar, ProgressStyle};
use uuid::Uuid;

use crate::app::App;
use crate::goal::{GoalMatch, GoalStatus, GoalTask};
use ikode::harness::ProjectConfig;
use ikode::settings::{Effort, Mode};
use ikode::tools;
use ikode::util::{abbrev, comma, sanitize_tool_arguments, short_id, usage_totals};

/// A side question can inspect the checkout and make several read-only tool
/// round-trips, but it must eventually yield control back to the main chat.
const SIDE_MAX_TURNS: usize = 12;

const SIDE_INSTRUCTIONS: &str = "\n\nEPHEMERAL SIDE CHAT: Answer the user's focused side question using the conversation so far as context. This detour is discarded when you finish: do not claim that it changes the main transcript. You have read-only inspection tools only, cannot modify the workspace, and cannot spawn or control subagents. Give a self-contained answer, then stop.";

/// What one model response produced, used to drive the autonomous goal loop.
enum TurnOutcome {
    /// The model ran tool calls (work in progress); call again to proceed.
    Continue,
    /// The model gave a final answer with no tool calls and no completion signal —
    /// in a goal this means "stopped without finishing", so the loop nudges it on.
    Final,
    /// The model called `task_complete` (summary).
    Complete(String),
    /// The model called `task_block` (reason).
    Blocked(String),
    /// Ctrl+C cut the turn short.
    Interrupted,
}

/// The assembled assistant message from one streamed model response, plus whether
/// the user interrupted it.
struct StreamOutcome {
    message: GaiseMessage,
    interrupted: bool,
}

struct ToolExecution {
    output: String,
    workspace_changed: bool,
}

impl App {
    /// Race an async operation against Ctrl+C. Returns `Some(result)` on completion, or
    /// `None` if the user pressed Ctrl+C — which cancels the in-flight operation and
    /// returns to the prompt instead of terminating the whole process. Rides the same
    /// `interrupt` watch channel as the agent turn (the single Ctrl+C listener spawned
    /// in `new`), so every long internal pass (enrich / embed / summarise / relationship
    /// & directory rollups / `/ask` inference) is escapable without losing the session.
    pub(crate) async fn cancellable<F: std::future::Future>(&self, fut: F) -> Option<F::Output> {
        let mut interrupt = self.interrupt.clone();
        // Ignore any Ctrl+C pressed while idle so we don't cancel before we start.
        interrupt.borrow_and_update();
        tokio::select! {
            biased;
            _ = interrupt.changed() => {
                println!("\n{} Cancelled — back to the prompt.", "⏹ ".bright_yellow());
                None
            }
            r = fut => Some(r),
        }
    }

    /// Stream one model response for `request`: drive the spinner, accumulate the
    /// reply, fold token usage into the session totals, and print the per-turn token
    /// line. Returns the assembled assistant message and whether Ctrl+C interrupted
    /// it. On interrupt the message is normalised (content defaulted, partial tool
    /// calls dropped) so history never holds a tool call without a result. Shared by
    /// the interactive and goal paths; the caller decides what to print and record.
    async fn stream_once(
        &mut self,
        request: &GaiseInstructRequest,
        interrupt: &mut tokio::sync::watch::Receiver<u64>,
    ) -> Result<StreamOutcome> {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("{spinner:.green} {msg}")?,
        );
        pb.set_message("Thinking...");
        pb.enable_steady_tick(Duration::from_millis(100));

        let mut stream = self
            .client
            .instruct_stream(request)
            .await
            .map_err(|e| anyhow!("{}", e))?;

        let mut acc = GaiseStreamAccumulator::new();
        let mut streamed_text = false;
        let mut interrupted = false;
        loop {
            tokio::select! {
                biased;
                _ = interrupt.changed() => {
                    interrupted = true;
                    break;
                }
                maybe = stream.next() => {
                    match maybe {
                        Some(item) => {
                            let resp = item.map_err(|e| anyhow!("{}", e))?;
                            if let GaiseStreamChunk::Text(text) = &resp.chunk {
                                if !streamed_text {
                                    pb.finish_and_clear();
                                    streamed_text = true;
                                }
                                print!("{}", text);
                                let _ = io::stdout().flush();
                            }
                            let is_usage = matches!(resp.chunk, GaiseStreamChunk::Usage(_));
                            acc.push(&resp);
                            // Surface token counts live in the spinner while we're
                            // still waiting (before any text streams).
                            if is_usage && !streamed_text {
                                let (i, o, c) = acc.usage.as_ref().map(usage_totals).unwrap_or((0, 0, 0));
                                let cached = if c > 0 { format!(" ({} cached)", comma(c)) } else { String::new() };
                                pb.set_message(format!("Thinking… ↑ {} tokens{} ↓ {} tokens", comma(i), cached, comma(o)));
                            }
                        }
                        None => break,
                    }
                }
            }
        }

        if streamed_text {
            println!();
        } else {
            pb.finish_and_clear();
        }

        // Fold token usage into the running session totals (shared across both paths).
        let (turn_in, turn_out, turn_cached) =
            acc.usage.as_ref().map(usage_totals).unwrap_or((0, 0, 0));
        // The prompt size of this request is the current context size — the signal for
        // token-threshold auto-compaction.
        if turn_in > 0 {
            self.last_input_tokens = turn_in;
        }
        self.session_input_tokens += turn_in;
        self.session_output_tokens += turn_out;
        self.session_cached_tokens += turn_cached;
        if !interrupted && (turn_in > 0 || turn_out > 0) {
            let session_total = self.session_input_tokens + self.session_output_tokens;
            let cached = if turn_cached > 0 {
                format!(" ({} cached)", comma(turn_cached))
            } else {
                String::new()
            };
            let session = if self.session_cached_tokens > 0 {
                format!(
                    "{}, {} cached",
                    abbrev(session_total),
                    abbrev(self.session_cached_tokens)
                )
            } else {
                abbrev(session_total)
            };
            println!(
                "{}",
                format!(
                    "  ↑ {} tokens{} · ↓ {} tokens · Σ {}   (session {})",
                    comma(turn_in),
                    cached,
                    comma(turn_out),
                    comma(turn_in + turn_out),
                    session
                )
                .dimmed()
            );
        }

        let mut message = acc.finish();
        if interrupted {
            if message.content.is_none() {
                message.content = Some(OneOrMany::One(GaiseContent::Text {
                    text: "[interrupted]".to_string(),
                }));
            }
            // Drop any partial tool calls so we never leave a tool_call without a result.
            message.tool_calls = None;
        }
        Ok(StreamOutcome {
            message,
            interrupted,
        })
    }

    pub(crate) async fn process_prompt(&mut self, prompt: &str) -> Result<()> {
        // Assemble this turn's images from two sources: those queued via `/image`,
        // and any local image paths referenced inline in the prompt (so dragging a
        // file into the terminal — which pastes a quoted path — just works). The bytes
        // ride along in `current_turn_images` (spliced into the request by
        // `build_request_history`) but are NOT recorded: the persisted message carries
        // only a text breadcrumb, so attachments stay ephemeral — present for this
        // turn, gone on resume. Taking `pending_images` also resets any leftover from
        // a prior interrupted turn.
        let mut turn_images = std::mem::take(&mut self.pending_images);
        let (text, inline_images) =
            crate::image::extract_inline_images(prompt, &self.working_directory);
        turn_images.extend(inline_images);
        let mut turn_documents = std::mem::take(&mut self.pending_documents);
        let (mut text, inline_documents) =
            crate::document::extract_inline_documents(&text, &self.working_directory);
        turn_documents.extend(inline_documents);

        if let Some(error) = turn_documents
            .iter()
            .find_map(|document| document.support_error(&self.model))
        {
            // Keep queued and inline attachments available so the user can switch
            // model/provider and retry without reselecting files.
            self.pending_images = turn_images;
            self.pending_documents = turn_documents;
            println!("{} {}", "Warning:".bright_yellow(), error);
            return Ok(());
        }
        for img in &turn_images {
            text.push_str(&format!("\n[Attached image: {}]", img.label));
        }
        for document in &turn_documents {
            text.push_str(&format!("\n[Attached document: {}]", document.label));
        }
        self.current_turn_images = turn_images.iter().map(|i| i.content()).collect();
        self.current_turn_documents = turn_documents
            .iter()
            .map(|document| document.content())
            .collect();

        self.record(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text { text })),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
        });

        // Mark the current interrupt count as seen, so a Ctrl-C pressed while idle
        // at the prompt doesn't instantly cancel this fresh turn.
        let mut interrupt = self.interrupt.clone();
        interrupt.borrow_and_update();

        // Track whether any mutating tool ran this turn, so we can re-sync the index
        // (and optionally enrich) once the turn settles.
        let mut dirty = false;

        loop {
            let request = GaiseInstructRequest {
                input: OneOrMany::Many(self.build_request_history()),
                model: self.model.clone(),
                tools: Some(self.request_tools(self.settings.mode, false)),
                generation_config: self.generation_config(
                    &self.session_cache_key,
                    self.settings.mode,
                    self.settings.effort,
                ),
                ..Default::default()
            };

            let outcome = self.stream_once(&request, &mut interrupt).await?;

            if outcome.interrupted {
                self.record(outcome.message);
                println!(
                    "{}",
                    "⏹  Interrupted — turn cancelled. (Type /exit to quit.)".bright_yellow()
                );
                return Ok(());
            }

            self.record(outcome.message.clone());

            if let Some(tool_calls) = outcome.message.tool_calls {
                for tool_call in tool_calls {
                    // A tool failure must NOT abort the turn: feed the error back to the
                    // model as the tool result so it can adjust and retry, exactly as it
                    // would for any other tool output.
                    let result = match self.handle_tool_call(&tool_call).await {
                        Ok(execution) => {
                            dirty |= execution.workspace_changed;
                            execution.output
                        }
                        Err(e) => {
                            println!(
                                "{} Tool {} failed: {}",
                                "⚠️ ".bright_yellow(),
                                tool_call.function.name.bright_magenta(),
                                e
                            );
                            format!("Error: tool '{}' failed: {}", tool_call.function.name, e)
                        }
                    };
                    self.record(GaiseMessage {
                        role: "tool".to_string(),
                        content: Some(OneOrMany::One(GaiseContent::Text { text: result })),
                        tool_calls: None,
                        tool_call_id: Some(tool_call.id.clone()),
                        tool_name: Some(tool_call.function.name.clone()),
                    });
                }
            } else {
                // Turn settled (final answer, no more tool calls). If files changed,
                // bring the graph back in sync — and, if enabled, enrich what changed.
                if dirty {
                    self.auto_sync_after_changes().await;
                }
                // Shrink context if it has grown past the auto-compact threshold.
                self.maybe_auto_compact().await;
                return Ok(());
            }
        }
    }

    /// `/btw` (alias `/side`) — run a focused, ephemeral side turn over a clone of
    /// the current context. Side messages and tool results never pass through
    /// [`App::record`], so neither the in-memory main history nor its JSONL
    /// transcript changes. The model receives only the bounded read-only leaf
    /// toolset used by subagents, which also prevents recursive orchestration.
    pub(crate) async fn run_btw(&mut self, prompt: &str) -> Result<()> {
        // `stream_once` updates this value because it normally describes the most
        // recent main request. Preserve the main context-size signal so a large side
        // prompt cannot trigger auto-compaction after the next ordinary turn. Token
        // usage totals still include the side request because it consumed real usage.
        let main_last_input_tokens = self.last_input_tokens;
        let result = self.run_btw_inner(prompt).await;
        self.last_input_tokens = main_last_input_tokens;
        result
    }

    async fn run_btw_inner(&mut self, prompt: &str) -> Result<()> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err(anyhow!("side question cannot be empty"));
        }

        // Images are local to the side transcript too. Include both already queued
        // attachments and paths pasted inline, but never copy their bytes into the
        // persisted parent conversation.
        let mut side_images = std::mem::take(&mut self.pending_images);
        let (text, inline_images) =
            crate::image::extract_inline_images(prompt, &self.working_directory);
        side_images.extend(inline_images);
        let mut side_documents = std::mem::take(&mut self.pending_documents);
        let (mut text, inline_documents) =
            crate::document::extract_inline_documents(&text, &self.working_directory);
        side_documents.extend(inline_documents);

        if let Some(error) = side_documents
            .iter()
            .find_map(|document| document.support_error(&self.model))
        {
            self.pending_images = side_images;
            self.pending_documents = side_documents;
            println!("{} {}", "Warning:".bright_yellow(), error);
            return Ok(());
        }
        for image in &side_images {
            text.push_str(&format!("\n[Attached image: {}]", image.label));
        }
        for document in &side_documents {
            text.push_str(&format!("\n[Attached document: {}]", document.label));
        }

        let mut content = vec![GaiseContent::Text { text }];
        content.extend(side_images.iter().map(|image| image.content()));
        content.extend(side_documents.iter().map(|document| document.content()));

        let mut history = self.history.clone();
        if let Some(first) = history.first_mut() {
            if let Some(OneOrMany::One(GaiseContent::Text { text })) = first.content.as_mut() {
                text.push_str(SIDE_INSTRUCTIONS);
            }
        }
        history.push(GaiseMessage {
            role: "user".to_string(),
            content: Some(if content.len() == 1 {
                OneOrMany::One(content.remove(0))
            } else {
                OneOrMany::Many(content)
            }),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
        });

        // Ultra's provider reasoning depth is retained through Max, while removing
        // Ultra's proactive delegation policy from this structurally non-delegating
        // side lane.
        let side_effort = match self.settings.effort {
            Effort::Ultra => Effort::Max,
            effort => effort,
        };
        let side_tools = tools::get_subagent_tools_for(self.graph_enabled);
        let side_cache_key = format!("btw-{}", Uuid::new_v4());
        let mut interrupt = self.interrupt.clone();
        interrupt.borrow_and_update();

        println!(
            "{} Ephemeral read-only side chat — the main transcript will not change.",
            "↪ ".bright_cyan()
        );

        for _ in 0..SIDE_MAX_TURNS {
            let request = GaiseInstructRequest {
                input: OneOrMany::Many(self.assemble_history(
                    &history,
                    Mode::Plan,
                    side_effort,
                    None,
                )),
                model: self.model.clone(),
                tools: Some(side_tools.clone()),
                generation_config: self.generation_config(&side_cache_key, Mode::Plan, side_effort),
                ..Default::default()
            };

            let outcome = self.stream_once(&request, &mut interrupt).await?;
            if outcome.interrupted {
                println!(
                    "{} Side chat interrupted — the main transcript is unchanged.",
                    "⏹ ".bright_yellow()
                );
                return Ok(());
            }

            let tool_calls = outcome.message.tool_calls.clone();
            history.push(outcome.message);
            let Some(tool_calls) = tool_calls else {
                println!(
                    "{} Back to the main chat — side transcript discarded.",
                    "↩ ".bright_cyan()
                );
                return Ok(());
            };

            for tool_call in tool_calls {
                let tool_name = &tool_call.function.name;
                // Tool availability is checked again at dispatch time. A provider
                // must not be able to smuggle a mutating or orchestration call merely
                // by returning a tool name that was absent from the request schema.
                let result = if side_tools.iter().any(|tool| tool.name == *tool_name) {
                    match self.handle_tool_call(&tool_call).await {
                        Ok(execution) => execution.output,
                        Err(error) => {
                            println!(
                                "{} Side tool {} failed: {}",
                                "⚠️ ".bright_yellow(),
                                tool_name.bright_magenta(),
                                error
                            );
                            format!("Error: tool '{tool_name}' failed: {error}")
                        }
                    }
                } else {
                    format!("Error: tool '{tool_name}' is unavailable in the read-only side chat")
                };
                history.push(GaiseMessage {
                    role: "tool".to_string(),
                    content: Some(OneOrMany::One(GaiseContent::Text { text: result })),
                    tool_calls: None,
                    tool_call_id: Some(tool_call.id),
                    tool_name: Some(tool_call.function.name.clone()),
                });
            }
        }

        println!(
            "{} Side chat stopped after {SIDE_MAX_TURNS} turns — the main transcript is unchanged.",
            "⚠️ ".bright_yellow()
        );
        Ok(())
    }

    /// One model response in the context of a goal `task`: stream the reply, record
    /// it to the task's isolated history/session, run any tool calls, and report what
    /// happened via [`TurnOutcome`]. The goal-control tools (`task_plan` /
    /// `task_complete` / `task_block`) are intercepted here — `run_turn` owns the
    /// active task, so it applies their effects directly rather than routing through
    /// the shared `ToolHost` (which avoids aliasing `&mut self` with `&mut task`).
    ///
    /// This is the concurrency seam: it operates on a task passed by reference, never
    /// on `self.history`/`self.settings.mode`, so a scheduler can later run one
    /// `run_turn` per task. `dirty` is OR-ed into the caller's flag for the post-goal
    /// re-index.
    async fn run_turn(
        &mut self,
        task: &mut GoalTask,
        interrupt: &mut tokio::sync::watch::Receiver<u64>,
        dirty: &mut bool,
    ) -> Result<TurnOutcome> {
        let request = GaiseInstructRequest {
            input: OneOrMany::Many(self.assemble_history(
                &task.history,
                task.mode,
                task.effort,
                Some((&task.objective, &task.acceptance)),
            )),
            model: self.model.clone(),
            tools: Some(self.request_tools(task.mode, true)),
            generation_config: self.generation_config(&task.cache_key, task.mode, task.effort),
            ..Default::default()
        };

        let outcome = self.stream_once(&request, interrupt).await?;
        self.record_task(task, outcome.message.clone());

        if outcome.interrupted {
            return Ok(TurnOutcome::Interrupted);
        }

        let Some(tool_calls) = outcome.message.tool_calls else {
            // Final answer, no tool calls: in a goal this is "stopped without a stop
            // signal" — the loop will nudge it on.
            return Ok(TurnOutcome::Final);
        };

        let mut control: Option<TurnOutcome> = None;
        for tool_call in tool_calls {
            let name = tool_call.function.name.clone();
            let args = sanitize_tool_arguments(tool_call.function.arguments.as_deref());
            let result = match name.as_str() {
                "task_plan" => self.apply_task_plan(task, args.as_deref()),
                "task_complete" => {
                    let summary = extract_field(args.as_deref(), "summary").unwrap_or_default();
                    control = Some(TurnOutcome::Complete(summary.clone()));
                    format!("Acknowledged — goal marked complete: {summary}")
                }
                "task_block" => {
                    let reason = extract_field(args.as_deref(), "reason").unwrap_or_default();
                    control = Some(TurnOutcome::Blocked(reason.clone()));
                    format!("Acknowledged — goal blocked pending input: {reason}")
                }
                _ => match self.handle_tool_call(&tool_call).await {
                    Ok(execution) => {
                        *dirty |= execution.workspace_changed;
                        execution.output
                    }
                    Err(e) => {
                        println!(
                            "{} Tool {} failed: {}",
                            "⚠️ ".bright_yellow(),
                            name.bright_magenta(),
                            e
                        );
                        format!("Error: tool '{}' failed: {}", name, e)
                    }
                },
            };
            self.record_task(
                task,
                GaiseMessage {
                    role: "tool".to_string(),
                    content: Some(OneOrMany::One(GaiseContent::Text { text: result })),
                    tool_calls: None,
                    tool_call_id: Some(tool_call.id.clone()),
                    tool_name: Some(tool_call.function.name.clone()),
                },
            );
        }

        Ok(control.unwrap_or(TurnOutcome::Continue))
    }

    /// Apply a `task_plan` call: set the task's acceptance criteria and seed its
    /// steps as the shared todo list. Returns the model-facing acknowledgement.
    fn apply_task_plan(&mut self, task: &mut GoalTask, args: Option<&str>) -> String {
        let parsed: tools::TaskPlanArgs = match serde_json::from_str(args.unwrap_or("{}")) {
            Ok(p) => p,
            Err(e) => return format!("Error: could not parse task_plan arguments: {e}"),
        };
        task.acceptance = parsed.acceptance;
        // Seed the per-step tracker. The todo list is the in-task step tracker; reset
        // it to the freshly-declared plan so a re-plan doesn't accumulate stale steps.
        self.todos.clear();
        for (i, step) in parsed.steps.iter().enumerate() {
            self.todos.push(ikode::tools::Todo {
                id: i + 1,
                task: step.clone(),
                completed: false,
            });
        }
        println!(
            "{} Plan set: {} acceptance criteria, {} steps.",
            "📋 ".bright_blue(),
            task.acceptance.len(),
            parsed.steps.len()
        );
        format!(
            "Plan recorded: {} acceptance criteria, {} steps.",
            task.acceptance.len(),
            parsed.steps.len()
        )
    }

    /// Start a *new* goal: mint an isolated [`GoalTask`] (fresh id / session /
    /// cache lane), seed it with the system message + kickoff prompt, persist it,
    /// then hand off to [`drive_goal`]. The task is saved to `.ikode/goals.json`
    /// (metadata only) so it survives in the `/goals` list and is resumable.
    pub(crate) async fn start_goal(&mut self, objective: &str) -> Result<()> {
        let objective = objective.trim();
        if objective.is_empty() {
            println!("{} Usage: /goal {{objective}}", "⚠️ ".bright_yellow());
            return Ok(());
        }

        let id = short_id(&Uuid::new_v4().to_string()).to_string();
        let cache_key = Uuid::new_v4().to_string();
        let mut task = GoalTask::new(
            id.clone(),
            objective.to_string(),
            self.settings.mode,
            self.settings.effort,
            cache_key,
        );
        // Seed the system message at the head of the task's isolated history. Like
        // sessions, the system prompt is never persisted (rebuilt each launch) — so we
        // push it directly rather than via `record_task`. It gives `assemble_history`
        // a system message to append the goal/mode blocks to.
        task.history.push(self.system_message());

        self.goals.upsert_active(task.clone());
        if let Err(e) = self.goals.save(&self.working_directory) {
            eprintln!("{} could not save goals: {e}", "⚠️ ".yellow());
        }

        println!(
            "{} Goal started ({}, {} mode, {} effort): {}",
            "🎯 ".bright_cyan(),
            short_id(&id).cyan(),
            task.mode.as_str().bright_magenta(),
            task.effort.as_str().bright_magenta(),
            objective.bright_white().bold()
        );

        // Kickoff: orient the model and tell it the goal-control contract.
        self.record_task(&mut task, user_message(KICKOFF));

        self.drive_goal(task).await
    }

    /// Resume an existing goal by id: rehydrate its conversation from the session
    /// transcript, reuse its cache lane (so the warm prefix survives), mark it active,
    /// inject either the user's `guidance` (e.g. the reply a `task_block` was waiting
    /// on) or a generic resume nudge at the tail, then hand off to [`drive_goal`].
    /// Accepts an id *prefix* for convenience. Closed goals (done/abandoned) are
    /// refused.
    pub(crate) async fn resume_goal(&mut self, id: &str, guidance: Option<&str>) -> Result<()> {
        let meta = match self.goals.match_task(id) {
            GoalMatch::Unique(task) => task.clone(),
            GoalMatch::None => {
                println!(
                    "{} No goal matching {}. Run {} to list them.",
                    "⚠️ ".bright_yellow(),
                    id.cyan(),
                    "/goals".cyan()
                );
                return Ok(());
            }
            GoalMatch::Ambiguous(matches) => {
                let matches = matches
                    .iter()
                    .map(|task| task.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                println!(
                    "{} Goal prefix {} is ambiguous; matches: {}.",
                    "⚠️ ".bright_yellow(),
                    id.cyan(),
                    matches
                );
                return Ok(());
            }
        };

        if matches!(meta.status, GoalStatus::Done | GoalStatus::Abandoned) {
            println!(
                "{} Goal {} is {} — start a new one with {}.",
                "•".dimmed(),
                short_id(&meta.id).cyan(),
                meta.status.as_str().bright_magenta(),
                "/goal <objective>".cyan()
            );
            return Ok(());
        }

        // Rehydrate the conversation from the on-disk transcript, then put the (never
        // persisted) system message back at the head so `assemble_history` has
        // somewhere to append the goal/mode blocks — matching how `start_goal` seeds.
        let session_path =
            crate::session::Session::new(&self.working_directory, meta.session_id.clone()).path;
        let mut history = match crate::session::load_messages(&session_path) {
            Ok(history) => history,
            Err(error) => {
                println!(
                    "{} Could not resume goal transcript: {}",
                    "⚠️ ".bright_yellow(),
                    error
                );
                return Ok(());
            }
        };
        let restored = history.len();
        history.insert(0, self.system_message());

        let mut task = GoalTask {
            history,
            status: GoalStatus::Active,
            ..meta
        };
        let id = task.id.clone();
        self.goals.upsert_active(task.clone());
        if let Err(e) = self.goals.save(&self.working_directory) {
            eprintln!("{} could not save goals: {e}", "⚠️ ".yellow());
        }

        println!(
            "{} Goal resumed ({}, {} mode, {} effort, {} messages restored): {}",
            "🎯 ".bright_cyan(),
            short_id(&id).cyan(),
            task.mode.as_str().bright_magenta(),
            task.effort.as_str().bright_magenta(),
            restored,
            task.objective.bright_white().bold()
        );
        if restored == 0 {
            println!(
                "  {}",
                "No prior transcript found — continuing with a fresh context.".dimmed()
            );
        }

        // The user's reply / steer (if any) goes at the tail so it never disturbs the
        // warm cached prefix; otherwise a generic resume nudge.
        let nudge = match guidance {
            Some(g) if !g.trim().is_empty() => user_message(g.trim()),
            _ => user_message(RESUME),
        };
        self.record_task(&mut task, nudge);

        self.drive_goal(task).await
    }

    /// The shared autonomous loop: drive a locally-owned [`GoalTask`] through
    /// [`run_turn`] — injecting a continuation nudge when the model stops without a
    /// stop signal — until it calls `task_complete`/`task_block`, the step budget is
    /// exhausted, or Ctrl+C drops back to the prompt. Persists the task's final status
    /// and re-syncs the graph once if the run changed files.
    async fn drive_goal(&mut self, mut task: GoalTask) -> Result<()> {
        let max_steps = ProjectConfig::resolve(&self.working_directory)
            .goal_max_steps
            .unwrap_or(25);

        println!(
            "  {}",
            format!("Working autonomously (≤{max_steps} steps) — press Ctrl+C to pause and return to the prompt.").dimmed()
        );

        let mut interrupt = self.interrupt.clone();
        interrupt.borrow_and_update();
        let mut dirty = false;
        let mut steps = 0usize;

        let final_status = loop {
            if steps >= max_steps {
                println!(
                    "{} Step budget ({}) reached — pausing. The goal stays active; run {} to continue, or close it with {}.",
                    "⏳ ".bright_yellow(),
                    max_steps,
                    "/goal".cyan(),
                    "/goal done".cyan()
                );
                break GoalStatus::Active;
            }
            // Surface any finished web research into the task's context before
            // the next turn, so the goal stays aware of delegated research even
            // when it never called wait_agent.
            for context in self.drain_web_result_contexts() {
                self.record_task(&mut task, user_message(&context));
            }
            // Shrink the task's context before the next call if it has grown past the
            // auto-compact threshold (no-op on the first iteration).
            self.maybe_auto_compact_task(&mut task).await;
            steps += 1;

            match self.run_turn(&mut task, &mut interrupt, &mut dirty).await? {
                TurnOutcome::Continue => continue,
                TurnOutcome::Final => {
                    // The model produced a final answer but did not signal completion;
                    // nudge it to finish, complete, or block. The nudge sits at the tail
                    // so it never disturbs the warm cached prefix.
                    self.record_task(&mut task, user_message(NUDGE));
                    continue;
                }
                TurnOutcome::Complete(summary) => {
                    println!(
                        "{} Goal complete: {}",
                        "✅ ".bright_green(),
                        if summary.is_empty() {
                            "(no summary)".to_string()
                        } else {
                            summary
                        }
                    );
                    break GoalStatus::Done;
                }
                TurnOutcome::Blocked(reason) => {
                    println!(
                        "{} Goal blocked — needs your input: {}",
                        "⏸ ".bright_yellow(),
                        if reason.is_empty() {
                            "(no reason given)".to_string()
                        } else {
                            reason
                        }
                    );
                    break GoalStatus::Blocked;
                }
                TurnOutcome::Interrupted => {
                    println!(
                        "{} Goal paused — still active. Run {} to see it or {} to resume work.",
                        "⏹ ".bright_yellow(),
                        "/goals".cyan(),
                        "/goal".cyan()
                    );
                    break GoalStatus::Active;
                }
            }
        };

        task.status = final_status;
        self.goals.upsert_active(task);
        if let Err(e) = self.goals.save(&self.working_directory) {
            eprintln!("{} could not save goals: {e}", "⚠️ ".yellow());
        }

        // If the goal changed files, bring the graph back in sync once at the end
        // (hash-gated; only does real work when auto_enrich is enabled).
        if dirty {
            self.auto_sync_after_changes().await;
        }

        Ok(())
    }

    /// After an agent turn that touched files, re-index (structural — catches
    /// creates, edits, deletes, and any command-driven changes) and, when
    /// `auto_enrich` is set, summarise + embed the affected chunks. Both passes are
    /// hash-gated, so only new/changed chunks cost model calls, and the enrich pass
    /// is Ctrl+C-escapable. No-op (beyond a cheap re-index) when `auto_enrich` is off.
    pub(crate) async fn auto_sync_after_changes(&mut self) {
        if !self.graph_enabled {
            return;
        }
        let auto_enrich = ProjectConfig::resolve(&self.working_directory)
            .auto_enrich
            .unwrap_or(false);
        println!(
            "{} Changes detected — re-indexing{}...",
            "🔄 ".bright_cyan(),
            if auto_enrich { " and enriching" } else { "" }
        );
        self.run_index();
        if auto_enrich {
            self.run_enrich(0).await;
        }
    }

    async fn handle_tool_call(&mut self, tool_call: &GaiseToolCall) -> Result<ToolExecution> {
        let name = &tool_call.function.name;
        println!(
            "{} Calling tool: {}",
            "🛠️ ".bright_yellow(),
            name.bright_magenta().bold()
        );
        let args = sanitize_tool_arguments(tool_call.function.arguments.as_deref());
        if tools::requires_graph(name) && !self.graph_enabled {
            return Ok(ToolExecution {
                output: format!(
                    "Error: tool '{name}' is unavailable while graph mode is off. Enable it with /graph on."
                ),
                workspace_changed: false,
            });
        }
        if self.mcp.has_tool(name) {
            let label = self
                .mcp
                .tool_label(name)
                .unwrap_or_else(|| name.to_string());
            let detail = args.as_deref().unwrap_or("{}");
            let action = format!("Run third-party MCP tool {label} with arguments {detail}");
            if !self.ask_permission(name, detail, &action)? {
                return Ok(ToolExecution {
                    output: format!("MCP tool '{label}' was not run (denied or cancelled)."),
                    workspace_changed: false,
                });
            }
            let output = self
                .mcp
                .call(name, args.as_deref())
                .await
                .unwrap_or_else(|error| format!("Error: {error:#}"));
            // MCP annotations are untrusted and an external tool may have changed
            // workspace state even when it returned an error.
            self.workspace_revision = self.workspace_revision.wrapping_add(1);
            return Ok(ToolExecution {
                output,
                workspace_changed: true,
            });
        }
        if ikode::mcp::is_exposed_tool_name(name) {
            return Ok(ToolExecution {
                output: format!(
                    "Unknown or disconnected MCP tool: {name}. Run /mcp refresh to rediscover tools."
                ),
                workspace_changed: false,
            });
        }
        let before = self.workspace_revision;
        let output = tools::dispatch(self, name, args.as_deref()).await?;
        Ok(ToolExecution {
            output,
            workspace_changed: self.workspace_revision != before,
        })
    }
}

/// Build a `user`-role message carrying `text` (the goal kickoff / nudge prompts).
pub(crate) fn user_message(text: &str) -> GaiseMessage {
    GaiseMessage {
        role: "user".to_string(),
        content: Some(OneOrMany::One(GaiseContent::Text {
            text: text.to_string(),
        })),
        tool_calls: None,
        tool_call_id: None,
        tool_name: None,
    }
}

/// Best-effort extraction of a top-level string field from a tool call's JSON
/// arguments (used for `task_complete`/`task_block`'s single field).
fn extract_field(args: Option<&str>, key: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(args.unwrap_or("{}")).ok()?;
    value.get(key)?.as_str().map(|s| s.to_string())
}

/// First instruction the model sees when a goal starts.
const KICKOFF: &str = "You are now working autonomously toward the goal in your system prompt. \
First call task_plan with concrete, checkable acceptance criteria and an ordered list of steps. \
Then carry out the work with your tools, keeping the todo list current. \
Call task_complete with a summary when every acceptance criterion is met, or task_block if you \
genuinely need my input to proceed.";

/// Injected when the model stops talking without signalling completion.
const NUDGE: &str = "Continue toward the goal. If every acceptance criterion is now met, call \
task_complete with a summary. If you are blocked and need my input, call task_block. Otherwise \
keep working.";

/// Injected when a goal is resumed with no explicit user guidance (bare `/goal`).
const RESUME: &str = "Resuming this goal. Review the conversation so far to recall what is already \
done, then continue toward the acceptance criteria. Call task_complete when every criterion is met, \
or task_block if you still need my input.";

#[cfg(test)]
mod tests {
    use super::*;
    use gaise_core::contracts::{
        GaiseEmbeddingsRequest, GaiseEmbeddingsResponse, GaiseInstructResponse,
        GaiseInstructStreamResponse,
    };
    use gaise_core::GaiseClient;
    use std::collections::VecDeque;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    struct SideClient {
        requests: Mutex<Vec<GaiseInstructRequest>>,
        responses: Mutex<VecDeque<Vec<GaiseStreamChunk>>>,
        fail_start: bool,
    }

    impl Default for SideClient {
        fn default() -> Self {
            Self::scripted(vec![vec![GaiseStreamChunk::Text(
                "side answer".to_string(),
            )]])
        }
    }

    impl SideClient {
        fn scripted(responses: Vec<Vec<GaiseStreamChunk>>) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                responses: Mutex::new(responses.into()),
                fail_start: false,
            }
        }

        fn failing() -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                responses: Mutex::new(VecDeque::new()),
                fail_start: true,
            }
        }

        fn requests(&self) -> Vec<GaiseInstructRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl GaiseClient for SideClient {
        async fn instruct_stream(
            &self,
            request: &GaiseInstructRequest,
        ) -> std::result::Result<
            Pin<
                Box<
                    dyn futures_util::Stream<
                            Item = std::result::Result<
                                GaiseInstructStreamResponse,
                                Box<dyn std::error::Error + Send + Sync>,
                            >,
                        > + Send,
                >,
            >,
            Box<dyn std::error::Error + Send + Sync>,
        > {
            self.requests.lock().unwrap().push(request.clone());
            if self.fail_start {
                return Err("scripted side start failure".into());
            }
            let chunks = self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or("no scripted side response remaining")?;
            Ok(Box::pin(futures_util::stream::iter(
                chunks.into_iter().map(|chunk| {
                    Ok(GaiseInstructStreamResponse {
                        chunk,
                        external_id: None,
                    })
                }),
            )))
        }

        async fn instruct(
            &self,
            _request: &GaiseInstructRequest,
        ) -> std::result::Result<GaiseInstructResponse, Box<dyn std::error::Error + Send + Sync>>
        {
            Err("not used".into())
        }

        async fn embeddings(
            &self,
            _request: &GaiseEmbeddingsRequest,
        ) -> std::result::Result<GaiseEmbeddingsResponse, Box<dyn std::error::Error + Send + Sync>>
        {
            Err("not used".into())
        }
    }

    fn app(root: &std::path::Path, client: Arc<dyn GaiseClient>) -> App {
        let mut app = App::new(
            "openai::gpt-5.4-mini".to_string(),
            "openai::fake-embedding".to_string(),
            "openai::fake-summary".to_string(),
            false,
            None,
            100,
            3,
            root.to_path_buf(),
            true,
            ikode::settings::LocalSettings::default(),
            None,
        )
        .unwrap();
        app.client = client;
        app
    }

    fn tool_call(index: usize, id: &str, name: &str, arguments: &str) -> GaiseStreamChunk {
        GaiseStreamChunk::ToolCall {
            index,
            id: Some(id.to_string()),
            name: Some(name.to_string()),
            arguments: Some(arguments.to_string()),
            thought_signature: None,
        }
    }

    #[tokio::test]
    async fn btw_uses_read_only_context_without_changing_parent_transcript() {
        let root = tempfile::tempdir().unwrap();
        let client = Arc::new(SideClient::default());
        let mut app = app(root.path(), client.clone());
        app.record(user_message("main question"));
        app.last_input_tokens = 42;

        let history_before = serde_json::to_string(&app.history).unwrap();
        let transcript_before = std::fs::read(&app.session.path).unwrap();

        app.run_btw("explain one detail").await.unwrap();

        assert_eq!(serde_json::to_string(&app.history).unwrap(), history_before);
        assert_eq!(std::fs::read(&app.session.path).unwrap(), transcript_before);
        assert_eq!(app.last_input_tokens, 42);

        let requests = client.requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        let input = serde_json::to_string(&request.input).unwrap();
        assert!(input.contains("main question"));
        assert!(input.contains("explain one detail"));
        assert!(input.contains("EPHEMERAL SIDE CHAT"));
        let names = request
            .tools
            .as_ref()
            .unwrap()
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"read_file"));
        assert!(!names.contains(&"edit_file"));
        assert!(!names.contains(&"spawn_agent"));
    }

    #[tokio::test]
    async fn btw_executes_read_tool_with_images_and_uses_non_delegating_ultra_lane() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("evidence.txt"), "side evidence").unwrap();
        let client = Arc::new(SideClient::scripted(vec![
            vec![tool_call(
                0,
                "side-read",
                "read_file",
                r#"{"path":"evidence.txt"}"#,
            )],
            vec![GaiseStreamChunk::Text("side conclusion".to_string())],
        ]));
        let mut app = app(root.path(), client.clone());
        app.settings.effort = Effort::Ultra;
        app.record(user_message("parent context"));
        app.pending_images.push(crate::image::LoadedImage {
            label: "queued.png".to_string(),
            media_type: "image/png".to_string(),
            data: vec![1, 2, 3],
        });
        let history_before = serde_json::to_string(&app.history).unwrap();
        let transcript_before = std::fs::read(&app.session.path).unwrap();

        app.run_btw("inspect the evidence and image").await.unwrap();

        assert_eq!(serde_json::to_string(&app.history).unwrap(), history_before);
        assert_eq!(std::fs::read(&app.session.path).unwrap(), transcript_before);
        assert!(app.pending_images.is_empty());
        let requests = client.requests();
        assert_eq!(requests.len(), 2);
        let first_input = serde_json::to_string(&requests[0].input).unwrap();
        assert!(first_input.contains("queued.png"));
        assert!(first_input.contains("parent context"));
        let config = requests[0].generation_config.as_ref().unwrap();
        assert_eq!(config.thinking_effort.as_deref(), Some("xhigh"));
        assert!(config.cache_key.as_deref().unwrap().ends_with("-plan-max"));
        let second_input = serde_json::to_string(&requests[1].input).unwrap();
        assert!(second_input.contains("side evidence"));
        assert!(second_input.contains("side-read"));
    }

    #[tokio::test]
    async fn btw_rejects_unadvertised_write_call_and_preserves_workspace() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("protected.txt");
        std::fs::write(&path, "before").unwrap();
        let client = Arc::new(SideClient::scripted(vec![
            vec![tool_call(
                0,
                "side-write",
                "edit_file",
                r#"{"path":"protected.txt","old_text":"before","new_text":"after"}"#,
            )],
            vec![GaiseStreamChunk::Text("write refused".to_string())],
        ]));
        let mut app = app(root.path(), client.clone());
        app.record(user_message("parent"));
        let transcript_before = std::fs::read(&app.session.path).unwrap();

        app.run_btw("try the bad tool call").await.unwrap();

        assert_eq!(std::fs::read_to_string(path).unwrap(), "before");
        assert_eq!(std::fs::read(&app.session.path).unwrap(), transcript_before);
        let requests = client.requests();
        assert_eq!(requests.len(), 2);
        assert!(serde_json::to_string(&requests[1].input)
            .unwrap()
            .contains("unavailable in the read-only side chat"));
    }

    #[tokio::test]
    async fn btw_validation_and_model_start_failure_restore_main_context_signal() {
        let root = tempfile::tempdir().unwrap();
        let client = Arc::new(SideClient::failing());
        let mut app = app(root.path(), client);
        app.record(user_message("parent"));
        app.last_input_tokens = 77;
        let history_before = serde_json::to_string(&app.history).unwrap();
        let transcript_before = std::fs::read(&app.session.path).unwrap();

        assert!(app.run_btw("   ").await.is_err());
        let error = app.run_btw("will fail to start").await.unwrap_err();
        assert!(error.to_string().contains("scripted side start failure"));
        assert_eq!(app.last_input_tokens, 77);
        assert_eq!(serde_json::to_string(&app.history).unwrap(), history_before);
        assert_eq!(std::fs::read(&app.session.path).unwrap(), transcript_before);
    }

    #[tokio::test]
    async fn main_turn_sends_mixed_image_text_document_and_pdf_without_persisting_bytes() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("notes.txt"), "document words").unwrap();
        std::fs::write(root.path().join("report.pdf"), b"%PDF-1.7\nsecret-bytes").unwrap();
        let client = Arc::new(SideClient::default());
        let mut app = app(root.path(), client.clone());
        app.pending_images.push(crate::image::LoadedImage {
            label: "queued.png".to_string(),
            media_type: "image/png".to_string(),
            data: vec![1, 2, 3],
        });
        app.pending_documents
            .push(crate::document::load_document("notes.txt", root.path()).unwrap());
        app.pending_documents
            .push(crate::document::load_document("report.pdf", root.path()).unwrap());

        app.process_prompt("compare all attachments").await.unwrap();

        let requests = client.requests();
        assert_eq!(requests.len(), 1);
        let messages = match &requests[0].input {
            OneOrMany::Many(messages) => messages,
            OneOrMany::One(_) => panic!("expected assembled message history"),
        };
        let user = messages
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .unwrap();
        let blocks = match user.content.as_ref().unwrap() {
            OneOrMany::Many(blocks) => blocks,
            OneOrMany::One(_) => panic!("expected mixed attachment blocks"),
        };
        assert_eq!(blocks.len(), 4);
        assert!(matches!(blocks[0], GaiseContent::Text { .. }));
        assert!(matches!(blocks[1], GaiseContent::Image { .. }));
        assert!(matches!(blocks[2], GaiseContent::Text { .. }));
        assert!(matches!(blocks[3], GaiseContent::File { .. }));
        let serialized = serde_json::to_string(blocks).unwrap();
        assert!(serialized.contains("document words"));
        assert!(serialized.contains("report.pdf"));

        let transcript = std::fs::read_to_string(&app.session.path).unwrap();
        assert!(transcript.contains("Attached image: queued.png"));
        assert!(transcript.contains("Attached document: notes.txt"));
        assert!(transcript.contains("Attached document: report.pdf"));
        assert!(!transcript.contains("document words"));
        assert!(!transcript.contains("secret-bytes"));
        assert!(app.pending_images.is_empty());
        assert!(app.pending_documents.is_empty());
    }

    #[tokio::test]
    async fn unsupported_binary_document_is_retained_and_never_calls_provider() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("report.pdf"), b"%PDF-1.7\nbody").unwrap();
        let client = Arc::new(SideClient::default());
        let mut app = app(root.path(), client.clone());
        app.model = "ollama::llama3.2".to_string();
        app.pending_documents
            .push(crate::document::load_document("report.pdf", root.path()).unwrap());
        let history_before = serde_json::to_string(&app.history).unwrap();

        app.process_prompt("summarize it").await.unwrap();

        assert!(client.requests().is_empty());
        assert_eq!(app.pending_documents.len(), 1);
        assert_eq!(serde_json::to_string(&app.history).unwrap(), history_before);
        assert!(!app.session.path.exists());
    }
}
