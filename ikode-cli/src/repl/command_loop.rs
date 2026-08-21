//! Interactive input loop and slash-command dispatch.

use super::*;

#[derive(Debug, PartialEq, Eq)]
enum ConversationCommand<'a> {
    Fork,
    ForkUsage,
    Side(Option<&'a str>),
}

#[derive(Debug, PartialEq, Eq)]
enum AttachmentCommand<'a> {
    Image(&'a str),
    Document(&'a str),
    Any(&'a str),
}

/// Pure, exact attachment-command routing for aliases and arguments.
fn attachment_command(input: &str) -> Option<AttachmentCommand<'_>> {
    match input {
        "/image" | "/img" => Some(AttachmentCommand::Image("")),
        value if value.starts_with("/image ") => Some(AttachmentCommand::Image(
            value.trim_start_matches("/image "),
        )),
        value if value.starts_with("/img ") => {
            Some(AttachmentCommand::Image(value.trim_start_matches("/img ")))
        }
        "/document" | "/doc" => Some(AttachmentCommand::Document("")),
        value if value.starts_with("/document ") => Some(AttachmentCommand::Document(
            value.trim_start_matches("/document "),
        )),
        value if value.starts_with("/doc ") => Some(AttachmentCommand::Document(
            value.trim_start_matches("/doc "),
        )),
        "/attach" => Some(AttachmentCommand::Any("")),
        value if value.starts_with("/attach ") => {
            Some(AttachmentCommand::Any(value.trim_start_matches("/attach ")))
        }
        _ => None,
    }
}

/// Pure slash-command recogniser for the conversation branching controls. Keeping
/// exact matching here prevents lookalikes such as `/forked` or `/btwfoo` from
/// being swallowed by the console dispatcher, and gives the routing its own unit
/// seam independent of terminal input or provider calls.
fn conversation_command(input: &str) -> Option<ConversationCommand<'_>> {
    match input {
        "/fork" => Some(ConversationCommand::Fork),
        value if value.starts_with("/fork ") => Some(ConversationCommand::ForkUsage),
        "/btw" | "/side" => Some(ConversationCommand::Side(None)),
        value if value.starts_with("/btw ") => {
            Some(ConversationCommand::Side(value.strip_prefix("/btw ")))
        }
        value if value.starts_with("/side ") => {
            Some(ConversationCommand::Side(value.strip_prefix("/side ")))
        }
        _ => None,
    }
}

fn graph_only_command(input: &str) -> bool {
    let command = input.split_whitespace().next().unwrap_or("");
    matches!(
        command,
        "/index"
            | "/scan"
            | "/rebuild"
            | "/squash"
            | "/ask"
            | "/ask-codebase"
            | "/embed"
            | "/init"
            | "/enrich"
            | "/summarize"
            | "/architecture"
            | "/arch"
            | "/relationships"
            | "/query"
            | "/dir-summaries"
            | "/visualize"
            | "/viz"
    )
}

impl App {
    pub(crate) async fn run_loop(&mut self, auto_indexed: bool) -> Result<()> {
        println!(
            "{}",
            "✨  Welcome to iKode! Your AI coding assistant.."
                .bright_cyan()
                .bold()
        );
        println!(
            "{}",
            "Type '/help' for a list of commands, or '/exit' to quit.\n".dimmed()
        );

        self.print_mode();
        self.warn_session_budget();

        // Auto-index already re-synced the graph this launch; otherwise surface any
        // drift between the persisted graph and the working tree.
        if !auto_indexed {
            self.check_startup_sync().await;
        }

        let term = console::Term::stdout();
        loop {
            let mode_before = self.settings.mode;
            // Live subagent annotation for the prompt frame's rule (bottom
            // right): re-evaluated on every editor redraw via this closure, so
            // it tracks workers finishing while the user types.
            let agents = self.agents.clone();
            let agent_status = move || {
                let (running, running_web, pending_results) = agents.activity();
                if running == 0 && pending_results == 0 {
                    return None;
                }
                let mut parts = Vec::new();
                if running > 0 {
                    parts.push(if running_web > 0 {
                        format!("{running} agent(s) running · {running_web} web")
                    } else {
                        format!("{running} agent(s) running")
                    });
                }
                if pending_results > 0 {
                    parts.push(format!("{pending_results} result(s) ready"));
                }
                Some(parts.join(" · "))
            };
            let (line, pasted_images) = match palette::read_command_line(
                &term,
                &mut self.settings.mode,
                self.settings.effort,
                &agent_status,
            )? {
                Some(pair) => pair,
                None => break, // Ctrl-D / Ctrl+C on an empty prompt — quit, like /exit.
            };
            // Shift+Tab may have cycled the mode while editing; persist any change.
            if self.settings.mode != mode_before {
                if let Err(e) = self.settings.save(&self.working_directory) {
                    eprintln!("{} could not save settings: {e}", "⚠️ ".yellow());
                }
            }
            // Images pasted into the editor (Ctrl+V) queue for this message, the same
            // as `/image` / `/paste`. They were already shown as `[Image …]` in the echo.
            if !pasted_images.is_empty() {
                self.pending_images.extend(pasted_images);
            }
            // Web research that finished while the prompt was open lands in the
            // conversation now, before this input is processed — so the next
            // turn already knows the answer (a bare Enter also surfaces it).
            self.surface_web_results();
            let input = line.trim();

            // Nothing to do only if there's no text AND no queued image — an
            // image-only message (e.g. paste then Enter) is still worth sending.
            if input.is_empty()
                && self.pending_images.is_empty()
                && self.pending_documents.is_empty()
            {
                continue;
            }

            // Rewrite `/ask-codebase` / `/ask_codebase` to `/ask` before dispatch.
            let normalized = normalize_command(input);
            let input: &str = normalized.as_deref().unwrap_or(input);

            if let Some(cmd) = input.strip_prefix('!') {
                let cmd = cmd.trim();
                if !cmd.is_empty() {
                    self.run_shell_escape(cmd);
                }
                continue;
            }

            if input == "/exit" {
                break;
            }
            if input == "/help" {
                palette::print_help();
                continue;
            }
            if input == "/mcp" || input.starts_with("/mcp ") {
                let rest = input.strip_prefix("/mcp").unwrap_or("").trim();
                self.handle_mcp_command(rest).await;
                continue;
            }
            if input == "/graph" || input.starts_with("/graph ") {
                let rest = input.strip_prefix("/graph").unwrap_or("").trim();
                self.handle_graph_command(rest);
                continue;
            }
            if !self.graph_enabled && graph_only_command(input) {
                println!(
                    "{} '{}' requires graph mode. Enable it with {}.",
                    "⚠️ ".bright_yellow(),
                    input.split_whitespace().next().unwrap_or(input),
                    "/graph on".cyan()
                );
                continue;
            }
            if input == "/cls" || input == "/clear_screen" {
                Self::clear_screen();
                continue;
            }
            if input == "/doctor" {
                self.run_doctor();
                continue;
            }
            if input == "/storage" {
                self.print_storage();
                continue;
            }
            if input == "/sessions" {
                self.handle_sessions("");
                continue;
            }
            if let Some(argument) = input.strip_prefix("/sessions ") {
                self.handle_sessions(argument);
                continue;
            }
            if input == "/clear" {
                self.history = vec![self.system_message()];
                self.session_cache_key = Uuid::new_v4().to_string();
                self.pending_images.clear();
                self.current_turn_images.clear();
                self.pending_documents.clear();
                self.current_turn_documents.clear();
                // Start a fresh transcript; the previous session stays on disk and is
                // resumable via /resume.
                self.session =
                    session::Session::new(&self.working_directory, Uuid::new_v4().to_string());
                println!(
                    "{}",
                    "🧹  History cleared — new session started.".bright_cyan()
                );
                continue;
            }

            // `/image`, `/document`, and generic `/attach` queue validated local
            // content for the next message; bare commands list their queues.
            if let Some(command) = attachment_command(input) {
                match command {
                    AttachmentCommand::Image(argument) => self.handle_image_attach(argument.trim()),
                    AttachmentCommand::Document(argument) => {
                        self.handle_document_attach(argument.trim())
                    }
                    AttachmentCommand::Any(argument) => self.handle_any_attach(argument.trim()),
                }
                continue;
            }

            // `/paste` pulls an image straight off the clipboard (Snipping Tool,
            // browser "copy image", etc.) and queues it for the next message.
            if input == "/paste" {
                match crate::clipboard::read_clipboard_image() {
                    Ok(img) => {
                        println!(
                            "{} Attached {} — sent with your next message.",
                            "🖼️ ".bright_blue(),
                            img.label.cyan()
                        );
                        self.pending_images.push(img);
                    }
                    Err(e) => println!("{} {}", "⚠️ ".bright_yellow(), e),
                }
                continue;
            }
            if input == "/resume" {
                self.run_resume();
                continue;
            }
            if let Some(command) = conversation_command(input) {
                match command {
                    ConversationCommand::Fork => self.run_fork(),
                    ConversationCommand::ForkUsage => {
                        println!("{} Usage: /fork", "⚠️ ".bright_yellow());
                    }
                    ConversationCommand::Side(Some(question)) => {
                        self.run_btw(question).await?;
                    }
                    ConversationCommand::Side(None) => {
                        if !term.is_term() {
                            println!(
                                "{} Usage: /btw <question> (alias: /side <question>)",
                                "⚠️ ".bright_yellow()
                            );
                            continue;
                        }
                        let question = match dialoguer::Input::<String>::new()
                            .with_prompt("Side question")
                            .interact_text()
                        {
                            Ok(question) => question,
                            Err(_) => {
                                println!("{} Side chat cancelled.", "•".dimmed());
                                continue;
                            }
                        };
                        if question.trim().is_empty() {
                            println!("{} Side chat cancelled.", "•".dimmed());
                            continue;
                        }
                        self.run_btw(question.trim()).await?;
                    }
                }
                continue;
            }
            if input == "/agents" || input == "/tasks" {
                println!("{}", self.agents.list());
                continue;
            }
            if input == "/agent" {
                self.handle_agent_command("").await;
                continue;
            }
            if let Some(argument) = input.strip_prefix("/agent ") {
                self.handle_agent_command(argument).await;
                continue;
            }
            if input == "/goals" {
                self.list_goals();
                continue;
            }
            if input == "/goal" {
                // Bare `/goal` resumes the active goal if it is resumable (active or
                // blocked); otherwise it just shows status.
                match self.goals.active() {
                    Some(t) if matches!(t.status, GoalStatus::Active | GoalStatus::Blocked) => {
                        let id = t.id.clone();
                        self.resume_goal(&id, None).await?;
                    }
                    _ => self.show_active_goal(),
                }
                continue;
            }
            if let Some(rest) = input.strip_prefix("/goal ") {
                let rest = rest.trim();
                if rest == "done" {
                    self.close_active_goal(GoalStatus::Done);
                    continue;
                }
                if rest == "abandon" {
                    self.close_active_goal(GoalStatus::Abandoned);
                    continue;
                }
                if let Some(target) = rest.strip_prefix("switch ") {
                    // Switch the active goal to (a prefix of) the given id and resume it.
                    self.resume_goal(target.trim(), None).await?;
                    continue;
                }
                // With a resumable goal active, `/goal <text>` steers it (and answers a
                // blocked goal's question); otherwise `<text>` starts a new goal.
                match self.goals.active() {
                    Some(t) if matches!(t.status, GoalStatus::Active | GoalStatus::Blocked) => {
                        let id = t.id.clone();
                        self.resume_goal(&id, Some(rest)).await?;
                    }
                    _ => self.start_goal(rest).await?,
                }
                continue;
            }
            if input == "/skills" || input.starts_with("/skills ") {
                let rest = input.strip_prefix("/skills").unwrap_or("").trim();
                self.run_skills(rest).await?;
                continue;
            }
            if input == "/compact" {
                self.run_compact().await;
                continue;
            }
            if let Some(rest) = input.strip_prefix("/compact auto") {
                self.run_compact_auto(rest.trim()).await;
                continue;
            }
            if input == "/index" || input == "/scan" {
                self.run_index();
                continue;
            }
            if input == "/rebuild" {
                self.run_rebuild();
                continue;
            }
            if input == "/squash" {
                self.run_squash();
                continue;
            }
            if input == "/ask-codebase" || input.starts_with("/ask-codebase ") {
                // Raw retrieval: relevant chunks + how they're wired, no LLM
                // synthesis. This is the console twin of the `ask_codebase` tool;
                // `/ask` (below) is the answer-synthesising counterpart.
                let q = input.strip_prefix("/ask-codebase").unwrap_or("").trim();
                if q.is_empty() {
                    println!(
                        "{} Usage: /ask-codebase {{question}}",
                        "⚠️ ".bright_yellow()
                    );
                } else {
                    if self.indexer.is_empty() {
                        self.run_index();
                    }
                    let result = self.indexer.ask(q, 12);
                    println!("{}", harness::render_ask_result(q, &result));
                }
                continue;
            }
            if let Some(q) = input.strip_prefix("/ask ") {
                let q = q.trim();
                if q.is_empty() {
                    println!("{} Usage: /ask {{question}}", "⚠️ ".bright_yellow());
                } else {
                    if self.indexer.is_empty() {
                        self.run_index();
                    }
                    // Embedding-first answer: cheap cosine retrieval feeds a small,
                    // token-bounded LLM synthesis pass (escalating one chunk-set at a
                    // time). If there's no embedding index, or the query-embed / answer
                    // call fails, degrade to the traditional keyword + graph lookup.
                    if self.indexer.has_embeddings() {
                        let fut = self.indexer.ask_inferred(
                            self.client.as_ref(),
                            &self.embedding_model,
                            &self.model,
                            q,
                        );
                        match self.cancellable(fut).await {
                            Some(Ok(ans)) => println!("{}", harness::render_ask_answer(q, &ans)),
                            Some(Err(e)) => {
                                println!(
                                    "{} Inference unavailable ({e}); showing keyword matches.",
                                    "⚠️ ".bright_yellow()
                                );
                                let result = self.indexer.ask(q, 12);
                                println!("{}", harness::render_ask_result(q, &result));
                            }
                            None => {} // cancelled — message already printed
                        }
                    } else {
                        let result = self.indexer.ask(q, 12);
                        println!("{}", harness::render_ask_result(q, &result));
                    }
                }
                continue;
            }
            if input == "/embed" {
                self.run_embed().await;
                continue;
            }
            if input == "/init" || input.starts_with("/init ") {
                // One-shot project warm-up: (re)build the index, then summarise + embed.
                let max = input
                    .strip_prefix("/init ")
                    .and_then(|s| s.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                self.run_index();
                self.run_enrich(max).await;
                continue;
            }
            if input == "/enrich" || input.starts_with("/enrich ") {
                let max = input
                    .strip_prefix("/enrich ")
                    .and_then(|s| s.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                self.run_enrich(max).await;
                continue;
            }
            if input == "/summarize" || input.starts_with("/summarize ") {
                let max = input
                    .strip_prefix("/summarize ")
                    .and_then(|s| s.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                self.run_summarize(max).await;
                continue;
            }
            if input == "/architecture" || input == "/arch" {
                self.run_architecture().await;
                continue;
            }
            if input == "/relationships" || input.starts_with("/relationships ") {
                // /relationships [EDGE_KIND] [max] — tokens in any order: an all-caps
                // word is the edge kind, a number is the call cap.
                let rest = input.strip_prefix("/relationships").unwrap_or("").trim();
                let mut kind = "CALLS".to_string();
                let mut max: Option<usize> = None;
                for tok in rest.split_whitespace() {
                    if let Ok(n) = tok.parse::<usize>() {
                        max = Some(n);
                    } else {
                        kind = tok.to_ascii_uppercase();
                    }
                }
                // Describing every edge is O(edges) inference calls (the call graph can
                // be huge), so default to a sane cap. An explicit count — including 0
                // for "unlimited" — overrides it.
                const DEFAULT_REL_CAP: usize = 200;
                self.run_relationships(&kind, max.unwrap_or(DEFAULT_REL_CAP))
                    .await;
                continue;
            }
            if input == "/query" || input.starts_with("/query ") {
                let rest = input.strip_prefix("/query").unwrap_or("").trim();
                if rest.is_empty() {
                    println!(
                        "{} usage: /query <json> — e.g. {}",
                        "•".dimmed(),
                        r#"/query {"start":{"symbol":"parse"},"traverse":[{"edge":"CALLS","dir":"out","depth":2}]}"#.dimmed()
                    );
                    continue;
                }
                if self.indexer.is_empty() {
                    self.run_index();
                }
                match serde_json::from_str::<ikode::index::GraphQuerySpec>(rest) {
                    Ok(spec) => match self.indexer.graph_query(&spec) {
                        Ok(rows) => {
                            println!("{} {} row(s)", "🕸️ ".bright_cyan(), rows.len());
                            for r in &rows {
                                println!(
                                    "  {} {} — {}:{}-{} [{}]",
                                    r.kind.dimmed(),
                                    r.name,
                                    r.path,
                                    r.line_start,
                                    r.line_end,
                                    r.chunk_id.dimmed()
                                );
                            }
                        }
                        Err(e) => println!("{} {}", "⚠️ ".bright_yellow(), e),
                    },
                    Err(e) => println!("{} invalid query JSON: {}", "⚠️ ".bright_yellow(), e),
                }
                continue;
            }
            if input == "/dir-summaries" || input.starts_with("/dir-summaries ") {
                let max = input
                    .strip_prefix("/dir-summaries ")
                    .and_then(|s| s.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                self.run_directories(max).await;
                continue;
            }
            if input == "/visualize"
                || input.starts_with("/visualize ")
                || input == "/viz"
                || input.starts_with("/viz ")
            {
                // `/visualize [port]` — serve the interactive code map and block until
                // Ctrl+C. Default port 7777; an explicit out-of-range/garbage port falls
                // back to the default rather than erroring.
                const DEFAULT_VIZ_PORT: u16 = 7777;
                let rest = input
                    .strip_prefix("/visualize")
                    .or_else(|| input.strip_prefix("/viz"))
                    .unwrap_or("")
                    .trim();
                let port = rest.parse::<u16>().unwrap_or(DEFAULT_VIZ_PORT);
                self.run_visualize(port).await;
                continue;
            }
            if input == "/effort" {
                println!(
                    "{} Effort: {} — {}",
                    "🧠".bright_blue(),
                    self.settings.effort.as_str().bright_magenta().bold(),
                    self.settings.effort.describe().dimmed()
                );
                continue;
            }
            if let Some(arg) = input.strip_prefix("/effort ") {
                match Effort::parse(arg) {
                    Some(effort) => {
                        self.settings.effort = effort;
                        match self.settings.save(&self.working_directory) {
                            Ok(()) => println!(
                                "{} Effort set to {} — {}",
                                "✅ ".bright_green(),
                                effort.as_str().bright_magenta().bold(),
                                effort.describe().dimmed()
                            ),
                            Err(error) => println!(
                                "{} Effort changed to {} for this session, but could not be persisted: {}",
                                "⚠️ ".bright_yellow(),
                                effort.as_str().bright_magenta().bold(),
                                error
                            ),
                        }
                    }
                    None => println!(
                        "{} Usage: /effort auto|low|med|high|max|ultra",
                        "⚠️ ".bright_yellow()
                    ),
                }
                continue;
            }
            if input == "/model" {
                println!(
                    "{} Chat model: {}",
                    "🤖".bright_blue(),
                    self.model.bright_magenta().bold()
                );
                continue;
            }
            if let Some(arg) = input.strip_prefix("/model ") {
                let arg = arg.trim();
                let mut it = arg.split_whitespace();
                match it.next() {
                    Some("save") => {
                        let value = self.model.clone();
                        self.save_setting("model", &value, it.next());
                    }
                    Some(_) if !arg.is_empty() => {
                        self.model = arg.to_string();
                        println!(
                            "{} Chat model changed to: {} (session only; /model save to persist)",
                            "✅ ".bright_green(),
                            self.model.bright_magenta().bold()
                        );
                    }
                    _ => println!(
                        "{} Usage: /model {{provider::model}} | /model save [global]",
                        "⚠️ ".bright_yellow()
                    ),
                }
                continue;
            }
            if input == "/emodel" {
                println!(
                    "{} Embedding model: {}",
                    "🧬 ".bright_blue(),
                    self.embedding_model.bright_magenta().bold()
                );
                continue;
            }
            if let Some(arg) = input.strip_prefix("/emodel ") {
                let arg = arg.trim();
                let mut it = arg.split_whitespace();
                match it.next() {
                    Some("save") => {
                        let value = self.embedding_model.clone();
                        self.save_setting("embedding_model", &value, it.next());
                    }
                    Some(_) if !arg.is_empty() => {
                        self.embedding_model = arg.to_string();
                        println!(
                            "{} Embedding model changed to: {} (session only; /emodel save to persist). \
                             Stored embeddings will be re-derived on next index.",
                            "✅ ".bright_green(),
                            self.embedding_model.bright_magenta().bold()
                        );
                    }
                    _ => println!(
                        "{} Usage: /emodel {{provider::model}} | /emodel save [global]",
                        "⚠️ ".bright_yellow()
                    ),
                }
                continue;
            }
            if input == "/smodel" {
                println!(
                    "{} Summary model: {}",
                    "📝 ".bright_blue(),
                    self.summary_model.bright_magenta().bold()
                );
                continue;
            }
            if let Some(arg) = input.strip_prefix("/smodel ") {
                let arg = arg.trim();
                let mut it = arg.split_whitespace();
                match it.next() {
                    Some("save") => {
                        let value = self.summary_model.clone();
                        self.save_setting("summary_model", &value, it.next());
                    }
                    Some(_) if !arg.is_empty() => {
                        self.summary_model = arg.to_string();
                        println!(
                            "{} Summary model changed to: {} (session only; /smodel save to persist)",
                            "✅ ".bright_green(),
                            self.summary_model.bright_magenta().bold()
                        );
                    }
                    _ => println!(
                        "{} Usage: /smodel {{provider::model}} | /smodel save [global]",
                        "⚠️ ".bright_yellow()
                    ),
                }
                continue;
            }
            if input == "/wmodel" {
                match &self.web_model {
                    Some(model) => println!(
                        "{} Web model: {}",
                        "🌐 ".bright_blue(),
                        model.bright_magenta().bold()
                    ),
                    None => println!(
                        "{} Web model: {} (inherited from the chat model; /wmodel {{provider::model}} to pin one)",
                        "🌐 ".bright_blue(),
                        self.effective_web_model().bright_magenta().bold()
                    ),
                }
                continue;
            }
            if let Some(arg) = input.strip_prefix("/wmodel ") {
                let arg = arg.trim();
                let mut it = arg.split_whitespace();
                match it.next() {
                    Some("save") => match self.web_model.clone() {
                        Some(value) => self.save_setting("web_model", &value, it.next()),
                        None => println!(
                            "{} No explicit web model set — it inherits the chat model. Set one first with /wmodel {{provider::model}}.",
                            "⚠️ ".bright_yellow()
                        ),
                    },
                    Some("clear") | Some("reset") => {
                        self.web_model = None;
                        println!(
                            "{} Web model cleared — web agents inherit the chat model again ({}).",
                            "✅ ".bright_green(),
                            self.effective_web_model().bright_magenta()
                        );
                    }
                    Some(_) if !arg.is_empty() => {
                        self.web_model = Some(arg.to_string());
                        println!(
                            "{} Web model changed to: {} (session only; /wmodel save to persist). New web agents use it; running ones keep theirs.",
                            "✅ ".bright_green(),
                            arg.bright_magenta().bold()
                        );
                    }
                    _ => println!(
                        "{} Usage: /wmodel {{provider::model}} | /wmodel clear | /wmodel save [global]",
                        "⚠️ ".bright_yellow()
                    ),
                }
                continue;
            }
            if input == "/vars" {
                self.print_vars();
                continue;
            }
            if input == "/mode" {
                self.print_mode();
                continue;
            }
            if let Some(arg) = input.strip_prefix("/mode ") {
                match Mode::parse(arg) {
                    Some(mode) => {
                        self.settings.mode = mode;
                        match self.settings.save(&self.working_directory) {
                            Ok(()) => println!(
                                "{} Mode set to {} — {}",
                                "✅ ".bright_green(),
                                mode.as_str().bright_magenta().bold(),
                                mode.describe().dimmed()
                            ),
                            Err(error) => println!(
                                "{} Mode changed to {} for this session, but could not be persisted: {}",
                                "⚠️ ".bright_yellow(),
                                mode.as_str().bright_magenta().bold(),
                                error
                            ),
                        }
                    }
                    None => println!("{} Usage: /mode plan|agentic|yolo", "⚠️ ".bright_yellow()),
                }
                continue;
            }
            if input == "/allow" {
                self.handle_permission_command(true, "list");
                continue;
            }
            if let Some(argument) = input.strip_prefix("/allow ") {
                self.handle_permission_command(true, argument);
                continue;
            }
            if input == "/deny" {
                self.handle_permission_command(false, "list");
                continue;
            }
            if let Some(argument) = input.strip_prefix("/deny ") {
                self.handle_permission_command(false, argument);
                continue;
            }
            if input == "/history" {
                let limit_display = if self.max_history == 0 {
                    "unlimited".to_string()
                } else {
                    self.max_history.to_string()
                };
                println!("{} History settings:", "📊".bright_blue());
                println!(
                    "  Max messages per request: {}",
                    limit_display.bright_magenta().bold()
                );
                println!(
                    "  Prefix keep:              {}",
                    self.prefix_keep.to_string().bright_magenta().bold()
                );
                println!(
                    "  Total messages stored:    {}",
                    self.history.len().to_string().bright_magenta().bold()
                );
                println!(
                    "  Session tokens:           {} input / {} output / {} cached",
                    self.session_input_tokens.to_string().bright_magenta(),
                    self.session_output_tokens.to_string().bright_magenta(),
                    self.session_cached_tokens.to_string().bright_magenta()
                );
                let threshold = self.auto_compact_threshold().await;
                println!(
                    "  Current context:          {} tokens",
                    ikode::util::comma(self.last_input_tokens).bright_magenta()
                );
                println!(
                    "  Auto-compact at:          {}",
                    crate::app::App::describe_auto_compact_rule(&threshold).bright_magenta()
                );
                let current_bytes = std::fs::metadata(&self.session.path)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                let infos = session::list_sessions(&self.working_directory);
                let storage = session::storage_stats(&infos);
                println!(
                    "  Current transcript:       {}",
                    human_bytes(current_bytes).bright_magenta()
                );
                println!(
                    "  All saved sessions:       {} across {} files",
                    human_bytes(storage.bytes).bright_magenta(),
                    storage.files
                );
                continue;
            }
            if input.starts_with("/max-history ") {
                let value = input.trim_start_matches("/max-history ").trim();
                let mut parts = value.split_whitespace();
                match parts.next() {
                    Some("save") => {
                        self.save_numeric_setting("max_history", self.max_history, parts.next());
                    }
                    Some(raw) => match raw.parse::<usize>() {
                        Ok(n) => {
                        self.max_history = n;
                        let display = if n == 0 {
                            "unlimited".to_string()
                        } else {
                            n.to_string()
                        };
                            if parts.next() == Some("save") {
                                self.save_numeric_setting("max_history", n, parts.next());
                            } else {
                                println!(
                                    "{} Max history set to {} for this session (use '/max-history save' to persist).",
                                    "✅ ".bright_green(),
                                    display.bright_magenta().bold()
                                );
                            }
                        }
                        Err(_) => println!(
                            "{} Invalid number. Usage: /max-history <number> [save [global]] | save [global]",
                            "⚠️ ".bright_yellow()
                        ),
                    },
                    None => println!(
                        "{} Usage: /max-history <number> [save [global]] | save [global]",
                        "⚠️ ".bright_yellow()
                    ),
                }
                continue;
            }
            if input.starts_with("/prefix-keep ") {
                let value = input.trim_start_matches("/prefix-keep ").trim();
                let mut parts = value.split_whitespace();
                match parts.next() {
                    Some("save") => {
                        self.save_numeric_setting("prefix_keep", self.prefix_keep, parts.next());
                    }
                    Some(raw) => match raw.parse::<usize>() {
                        Ok(n) => {
                            self.prefix_keep = n;
                            if parts.next() == Some("save") {
                                self.save_numeric_setting("prefix_keep", n, parts.next());
                            } else {
                                println!(
                                    "{} Prefix keep set to {} for this session (use '/prefix-keep save' to persist).",
                                    "✅ ".bright_green(),
                                    n.to_string().bright_magenta().bold()
                                );
                            }
                        }
                        Err(_) => println!(
                            "{} Invalid number. Usage: /prefix-keep <number> [save [global]] | save [global]",
                            "⚠️ ".bright_yellow()
                        ),
                    },
                    None => println!(
                        "{} Usage: /prefix-keep <number> [save [global]] | save [global]",
                        "⚠️ ".bright_yellow()
                    ),
                }
                continue;
            }

            // A `/`-prefixed line that matched no command above is a typo, not a chat
            // message — report it (with close matches) instead of sending it to the model.
            if input.starts_with('/') {
                let token = input.split_whitespace().next().unwrap_or(input);
                let hits = palette::rank(token);
                let suggestion = if hits.is_empty() {
                    String::new()
                } else {
                    let names: Vec<&str> = hits.iter().map(|c| c.name).collect();
                    format!(" Did you mean {}?", names.join(", "))
                };
                println!(
                    "{} Unknown command: {}.{} Type {} for the list.",
                    "⚠️ ".bright_yellow(),
                    token.cyan(),
                    suggestion,
                    "/help".cyan()
                );
                continue;
            }

            self.process_prompt(input).await?;
        }

        println!("{}", "👋  Goodbye!".bright_yellow());
        self.print_resume_hint();
        Ok(())
    }

    /// On exit, show how to come back to *this* session and the shortcut for the most
    /// recent one (Claude-style). Skipped when nothing was persisted this run, so we
    /// don't advertise an empty transcript.
    fn print_resume_hint(&self) {
        if !self.session.path.exists() {
            return;
        }
        println!(
            "{} Resume this session with {}",
            "↩ ".bright_blue(),
            format!("ikode --resume {}", self.session.id).cyan()
        );
        println!("   {}", "or ikode --continue for the most recent.".dimmed());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_commands_route_exact_names_and_aliases() {
        assert_eq!(
            conversation_command("/fork"),
            Some(ConversationCommand::Fork)
        );
        assert_eq!(
            conversation_command("/fork unexpected"),
            Some(ConversationCommand::ForkUsage)
        );
        assert_eq!(
            conversation_command("/btw"),
            Some(ConversationCommand::Side(None))
        );
        assert_eq!(
            conversation_command("/side"),
            Some(ConversationCommand::Side(None))
        );
        assert_eq!(
            conversation_command("/btw why this branch?"),
            Some(ConversationCommand::Side(Some("why this branch?")))
        );
        assert_eq!(
            conversation_command("/side check the parser"),
            Some(ConversationCommand::Side(Some("check the parser")))
        );
    }

    #[test]
    fn conversation_command_does_not_capture_lookalikes() {
        for input in ["/forked", "/btwfoo", "/sidebar", "fork", "hello"] {
            assert_eq!(conversation_command(input), None, "{input}");
        }
    }

    #[test]
    fn attachment_commands_route_exact_names_aliases_and_arguments() {
        assert_eq!(
            attachment_command("/image"),
            Some(AttachmentCommand::Image(""))
        );
        assert_eq!(
            attachment_command("/img shot.png"),
            Some(AttachmentCommand::Image("shot.png"))
        );
        assert_eq!(
            attachment_command("/document report.pdf"),
            Some(AttachmentCommand::Document("report.pdf"))
        );
        assert_eq!(
            attachment_command("/doc notes.md"),
            Some(AttachmentCommand::Document("notes.md"))
        );
        assert_eq!(
            attachment_command("/attach sheet.xlsx"),
            Some(AttachmentCommand::Any("sheet.xlsx"))
        );
        assert_eq!(
            attachment_command("/attach"),
            Some(AttachmentCommand::Any(""))
        );
    }

    #[test]
    fn attachment_command_does_not_capture_lookalikes() {
        for input in ["/images", "/documentary", "/attachment", "attach", "hello"] {
            assert_eq!(attachment_command(input), None, "{input}");
        }
    }

    #[test]
    fn graph_gate_covers_graph_commands_but_not_runtime_toggle_or_file_tools() {
        for input in [
            "/index",
            "/ask how does auth work",
            "/query {}",
            "/visualize 7777",
        ] {
            assert!(graph_only_command(input), "{input}");
        }
        for input in ["/graph off", "/mcp list", "/mode plan", "/history"] {
            assert!(!graph_only_command(input), "{input}");
        }
    }
}
