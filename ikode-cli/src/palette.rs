//! Live slash-command palette for the interactive prompt.
//!
//! When a line begins with `/`, the editor shows a [`MAX_SUGGESTIONS`]-row
//! window of command suggestions under the prompt. Up/down navigation scrolls
//! through every match, and typing narrows them (prefix > substring > fuzzy
//! subsequence, tie-broken by a static priority).
//! The ranking and registry are pure functions so they can be unit-tested
//! without a terminal; the interactive editor lives in [`read_command_line`].
//!
//! Editing model: the whole line is edited in raw mode, framed between two
//! horizontal rules with a colour-coded mode indicator beneath it. Every
//! character (including the first) is insertable/deletable and ←/→/Home/End
//! work. **Shift+Tab** cycles the operating mode (plan → agentic → yolo) live.
//! On submit the frame collapses to a single `> <line>` echo so scrollback
//! stays clean.

use std::io::{self, Write};

use colored::Colorize;
use console::{Key, Term};
use ikode::settings::{Effort, Mode};

use crate::attach::{self, Attachment, AttachmentStore};
use crate::image::LoadedImage;

/// A slash command surfaced in the palette.
pub struct Command {
    /// Includes the leading slash, e.g. `"/ask"`.
    pub name: &'static str,
    /// One-line description shown beside the name.
    pub summary: &'static str,
    /// Full syntax used by `/help`; keeping it here makes the palette registry the
    /// single source of truth for command discovery and documentation.
    pub usage: &'static str,
    /// Help section heading.
    pub category: &'static str,
    /// Higher wins when nothing (or an equal score) narrows the list.
    pub priority: u8,
    /// Whether the command expects arguments after it (controls whether
    /// accepting a suggestion submits immediately or waits for args).
    pub takes_args: bool,
}

macro_rules! command {
    ($name:literal, $usage:literal, $summary:literal, $category:literal, $priority:literal, $takes_args:literal) => {
        Command {
            name: $name,
            usage: $usage,
            summary: $summary,
            category: $category,
            priority: $priority,
            takes_args: $takes_args,
        }
    };
}

/// The canonical command set. Ordering here only matters as a stable
/// tie-break source; `priority` drives the default (bare `/`) ordering.
pub const COMMANDS: &[Command] = &[
    command!(
        "/ask",
        "/ask <question>",
        "Answer from retrieved code with LLM synthesis",
        "Codebase",
        100,
        true
    ),
    command!(
        "/ask-codebase",
        "/ask-codebase <question>",
        "Show raw relevant chunks and connectivity (alias: /ask_codebase)",
        "Codebase",
        98,
        true
    ),
    command!(
        "/index",
        "/index",
        "Build or refresh the code and Markdown index",
        "Codebase",
        96,
        false
    ),
    command!(
        "/enrich",
        "/enrich [max]",
        "Run summary then embedding passes",
        "Codebase",
        92,
        true
    ),
    command!(
        "/goal",
        "/goal [objective|switch <id>|done|abandon]",
        "Start, resume, switch, or close an autonomous goal",
        "Agent",
        91,
        true
    ),
    command!(
        "/agent",
        "/agent [spawn|inspect|steer|wait|stop|close|collect] ...",
        "Spawn and control parallel subagent threads",
        "Agent",
        95,
        true
    ),
    command!(
        "/agents",
        "/agents",
        "List parallel subagent threads and statuses",
        "Agent",
        94,
        false
    ),
    command!(
        "/tasks",
        "/tasks",
        "Alias for /agents (background agent tasks)",
        "Agent",
        93,
        false
    ),
    command!(
        "/goals",
        "/goals",
        "List all goal tasks and statuses",
        "Agent",
        89,
        false
    ),
    command!(
        "/effort",
        "/effort [auto|low|med|high|max|ultra]",
        "Show or set reasoning and delegation effort",
        "Agent",
        90,
        true
    ),
    command!(
        "/mode",
        "/mode [plan|agentic|yolo]",
        "Show or persist the operating mode",
        "Configuration",
        90,
        true
    ),
    command!(
        "/help",
        "/help",
        "Display this command reference",
        "General",
        88,
        false
    ),
    command!(
        "/doctor",
        "/doctor",
        "Check providers, configuration, graph and session storage",
        "General",
        87,
        false
    ),
    command!(
        "/embed",
        "/embed",
        "Build the semantic embedding index",
        "Codebase",
        82,
        false
    ),
    command!(
        "/init",
        "/init [max]",
        "One-shot index, summary and embedding warm-up",
        "Codebase",
        80,
        true
    ),
    command!(
        "/summarize",
        "/summarize [max]",
        "Summarise chunks through the summary model",
        "Codebase",
        78,
        true
    ),
    command!(
        "/smodel",
        "/smodel [model|save [global]]",
        "Show or switch the summary model",
        "Configuration",
        76,
        true
    ),
    command!(
        "/wmodel",
        "/wmodel [model|clear|save [global]]",
        "Show or switch the web-research agent model",
        "Configuration",
        75,
        true
    ),
    command!(
        "/vars",
        "/vars",
        "List iKode-related environment variables (keys masked)",
        "Configuration",
        54,
        false
    ),
    command!(
        "/architecture",
        "/architecture",
        "Generate a prose codebase overview",
        "Codebase",
        72,
        false
    ),
    command!(
        "/model",
        "/model [model|save [global]]",
        "Show or switch the chat model",
        "Configuration",
        68,
        true
    ),
    command!(
        "/relationships",
        "/relationships [kind] [max]",
        "Describe graph relationships",
        "Codebase",
        62,
        true
    ),
    command!(
        "/dir-summaries",
        "/dir-summaries [max]",
        "Roll up one summary per directory",
        "Codebase",
        60,
        true
    ),
    command!(
        "/graph",
        "/graph [status|on|off|save [global]]",
        "Show, switch, or persist graph/index mode",
        "Configuration",
        58,
        true
    ),
    command!(
        "/mcp",
        "/mcp [list|tools|refresh|add|remove|enable|disable]",
        "Register and manage third-party MCP servers",
        "Configuration",
        61,
        true
    ),
    command!(
        "/query",
        "/query <json>",
        "Run a structured start/traverse/where graph query",
        "Codebase",
        59,
        true
    ),
    command!(
        "/skills",
        "/skills [name|add|edit|remove]",
        "List, run, or manage project skills",
        "Agent",
        57,
        true
    ),
    command!(
        "/visualize",
        "/visualize [port]",
        "Serve the interactive code map (alias: /viz)",
        "Codebase",
        56,
        true
    ),
    command!(
        "/emodel",
        "/emodel [model|save [global]]",
        "Show or switch the embedding model",
        "Configuration",
        55,
        true
    ),
    command!(
        "/scan",
        "/scan",
        "Incremental index alias for /index",
        "Codebase",
        52,
        false
    ),
    command!(
        "/rebuild",
        "/rebuild",
        "Wipe and rebuild derived graph data",
        "Codebase",
        50,
        false
    ),
    command!(
        "/squash",
        "/squash",
        "Checkpoint exact graph state and discard WAL history",
        "Codebase",
        49,
        false
    ),
    command!(
        "/allow",
        "/allow [list|remove <n|rule>|clear|<rule>]",
        "Manage persistent permission allow rules",
        "Configuration",
        46,
        true
    ),
    command!(
        "/deny",
        "/deny [list|remove <n|rule>|clear|<rule>]",
        "Manage persistent permission deny rules",
        "Configuration",
        44,
        true
    ),
    command!(
        "/history",
        "/history",
        "Show history, token, transcript and byte statistics",
        "Sessions",
        40,
        false
    ),
    command!(
        "/storage",
        "/storage",
        "Show graph WAL and saved-session storage usage",
        "Sessions",
        39,
        false
    ),
    command!(
        "/sessions",
        "/sessions [prune [--keep N] [--max-bytes SIZE] [--apply]]",
        "Inspect or safely prune saved transcripts",
        "Sessions",
        38,
        true
    ),
    command!(
        "/max-history",
        "/max-history <n>|save [global]",
        "Set or persist the request history limit",
        "Sessions",
        37,
        true
    ),
    command!(
        "/prefix-keep",
        "/prefix-keep <n>|save [global]",
        "Set or persist the stable history prefix",
        "Sessions",
        36,
        true
    ),
    command!(
        "/clear",
        "/clear",
        "Reset history and begin a new saved session",
        "Sessions",
        34,
        false
    ),
    command!(
        "/btw",
        "/btw [question]",
        "Ask in an ephemeral read-only side chat (alias: /side)",
        "Sessions",
        65,
        true
    ),
    command!(
        "/fork",
        "/fork",
        "Clone this chat into a fresh saved branch",
        "Sessions",
        63,
        false
    ),
    command!(
        "/resume",
        "/resume",
        "Pick and resume a saved session",
        "Sessions",
        64,
        false
    ),
    command!(
        "/image",
        "/image [path|clear]",
        "Queue a local PNG/JPEG/GIF/WebP (alias: /img)",
        "Sessions",
        53,
        true
    ),
    command!(
        "/document",
        "/document [path|clear]",
        "Queue a local document (alias: /doc)",
        "Sessions",
        55,
        true
    ),
    command!(
        "/attach",
        "/attach [path|clear]",
        "Queue an image or document for the next message",
        "Sessions",
        56,
        true
    ),
    command!(
        "/paste",
        "/paste",
        "Queue an image from the clipboard",
        "Sessions",
        51,
        false
    ),
    command!(
        "/compact",
        "/compact [auto [<percent>%|<tokens>|off|default]]",
        "Summarise older turns to shrink context; `auto` shows/sets the auto-compact threshold",
        "Sessions",
        54,
        true
    ),
    command!("/cls", "/cls", "Clear the terminal", "General", 30, false),
    command!(
        "/exit",
        "/exit",
        "Quit the interactive session",
        "General",
        26,
        false
    ),
];

/// Render `/help` from [`COMMANDS`] so palette suggestions and documentation
/// cannot drift apart.
pub fn print_help() {
    println!("{}", "\nAvailable commands:".bright_green().bold());
    for category in ["General", "Agent", "Codebase", "Configuration", "Sessions"] {
        println!("\n  {}", category.bright_white().bold());
        for command in COMMANDS
            .iter()
            .filter(|command| command.category == category)
        {
            let usage = format!("{:<56}", command.usage);
            println!("    {} {}", usage.cyan(), command.summary.dimmed());
        }
    }
    let usage = format!("{:<56}", "!<command>");
    println!(
        "\n  {}\n    {} {}",
        "Shell".bright_white().bold(),
        usage.cyan(),
        "Run a shell command and feed its output back to the model".dimmed()
    );
}

/// Maximum suggestions shown at once.
pub const MAX_SUGGESTIONS: usize = 5;

const PROMPT: &str = "> ";
/// Visible width of [`PROMPT`] (used for cursor math; the rendered prompt is
/// colourised but still two columns wide).
const PROMPT_WIDTH: usize = 2;
/// Column width reserved for the command name so summaries line up.
const NAME_COL: usize = 16;

/// Returns true when `needle`'s chars appear in order within `hay`.
fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut chars = hay.chars();
    needle.chars().all(|c| chars.any(|h| h == c))
}

/// Relevance score of `command` for `query` (both start with `/`), or `None`
/// if it doesn't match. Higher is better. A bare `/` matches everything at a
/// neutral score so `priority` alone orders the default list.
fn score(query: &str, command: &str) -> Option<i32> {
    let q = query.to_ascii_lowercase();
    let c = command.to_ascii_lowercase();
    if q == "/" {
        return Some(0);
    }
    if c.starts_with(&q) {
        // Prefix match: prefer the closest-length command.
        return Some(3000 - (c.len() as i32 - q.len() as i32));
    }
    let qbody = q.trim_start_matches('/');
    let cbody = c.trim_start_matches('/');
    if cbody.contains(qbody) {
        return Some(2000 - (cbody.len() as i32 - qbody.len() as i32));
    }
    if is_subsequence(qbody, cbody) {
        return Some(1000 - (cbody.len() as i32 - qbody.len() as i32));
    }
    None
}

/// All matching commands for `query` (which must start with `/`), best first.
/// Ties break by `priority`, then by name for stability.
fn rank_all(query: &str) -> Vec<&'static Command> {
    let mut scored: Vec<(i32, &Command)> = COMMANDS
        .iter()
        .filter_map(|c| score(query, c.name).map(|s| (s, c)))
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(b.1.priority.cmp(&a.1.priority))
            .then(a.1.name.cmp(b.1.name))
    });
    scored.into_iter().map(|(_, c)| c).collect()
}

/// Top [`MAX_SUGGESTIONS`] commands for `query`, used for concise typo hints.
pub fn rank(query: &str) -> Vec<&'static Command> {
    rank_all(query).into_iter().take(MAX_SUGGESTIONS).collect()
}

/// Adjust the five-row viewport so `selected` remains visible, then return its
/// range within the complete ranked suggestion list.
fn suggestion_window(
    selected: usize,
    suggestion_count: usize,
    window_start: &mut usize,
) -> std::ops::Range<usize> {
    if suggestion_count == 0 {
        *window_start = 0;
        return 0..0;
    }

    *window_start = (*window_start).min(suggestion_count.saturating_sub(MAX_SUGGESTIONS));
    if selected < *window_start {
        *window_start = selected;
    } else if selected >= window_start.saturating_add(MAX_SUGGESTIONS) {
        *window_start = selected + 1 - MAX_SUGGESTIONS;
    }

    *window_start
        ..window_start
            .saturating_add(MAX_SUGGESTIONS)
            .min(suggestion_count)
}

/// Whether the palette overlay should be shown for the current buffer: a
/// non-empty `/token` with no whitespace yet (the command not yet decided).
fn palette_active(buf: &[char]) -> bool {
    matches!(buf.first(), Some('/')) && !buf.iter().any(|c| c.is_whitespace())
}

/// (row, col) of the caret after the first `upto` buffer chars, rendered behind a
/// `prompt_w`-column prompt into a `width`-column terminal. Treats `'\n'` as a hard
/// line break (col resets to 0, only the first row carries the prompt indent) while
/// still soft-wrapping long lines at the edge — so a multi-line buffer positions the
/// cursor correctly. `cell_w` gives each char's display width in columns, so a
/// collapsed-paste sentinel (rendered as a multi-column `[Pasted N lines]`) advances
/// the caret by its full placeholder width rather than a single column.
fn layout(
    buf: &[char],
    upto: usize,
    prompt_w: usize,
    width: usize,
    cell_w: &dyn Fn(char) -> usize,
) -> (usize, usize) {
    let w = width.max(1);
    let mut row = 0usize;
    let mut col = prompt_w;
    for &ch in buf.iter().take(upto) {
        if ch == '\n' {
            row += 1;
            col = 0;
        } else {
            // Advance one terminal cell at a time so a wide cell that crosses the
            // right edge wraps exactly as the terminal flows it.
            for _ in 0..cell_w(ch).max(1) {
                col += 1;
                if col >= w {
                    row += 1;
                    col = 0;
                }
            }
        }
    }
    (row, col)
}

/// Reads one line of input, driving the palette while a command token is being
/// typed. The entire line is edited in raw mode (so every character — including
/// the first — is insertable/deletable, and ←/→/Home/End work), with the
/// suggestion overlay shown only while the buffer is a bare `/command`. The
/// current `mode` is shown beneath the input and is mutated in place when the
/// user presses Shift+Tab to cycle it; the caller persists any change. Returns
/// `Ok(None)` — treated as "quit" by the caller, exactly like `/exit` — on EOF
/// (Ctrl-D) or Ctrl+C on an empty line. Ctrl+C with text typed clears the line.
///
/// `status` supplies a short live annotation (e.g. running-subagent counts)
/// rendered right-aligned inside the frame's rule; it is re-evaluated on every
/// redraw, so it refreshes on each keypress. Return `None` for no annotation.
pub fn read_command_line(
    term: &Term,
    mode: &mut Mode,
    effort: Effort,
    status: &dyn Fn() -> Option<String>,
) -> io::Result<Option<(String, Vec<LoadedImage>)>> {
    // Non-interactive (piped stdin, redirected output): just read a line.
    if !term.is_term() {
        print!("{}", PROMPT.bright_blue().bold());
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            return Ok(None);
        }
        return Ok(Some((line, Vec::new())));
    }

    // Ask the terminal to forward full-fidelity key events while we edit, so the
    // Ctrl modifier on Enter survives (Ctrl+Enter → newline). Restored on return.
    let _fidelity = crate::keyread::full_key_fidelity();

    let mut buf: Vec<char> = Vec::new();
    let mut cursor = 0usize;
    let mut sel = 0usize;
    let mut suggestion_start = 0usize;
    // Pasted (and, later, other) attachments collapsed into single sentinel chars
    // within `buf`; expanded back to their real text on submit.
    let mut pastes = AttachmentStore::new();
    // Row offset of the cursor within the rendered block, carried between redraws
    // so the next render can climb back to the top of what it previously drew.
    let mut cur_row = 0usize;
    // Terminal width at the last draw, so the next render can detect a resize and
    // recompute the climb against the reflowed block (0 = nothing drawn yet).
    let mut last_width = 0usize;

    loop {
        let active = palette_active(&buf);
        let suggestions = if active {
            rank_all(&buf.iter().collect::<String>())
        } else {
            Vec::new()
        };
        if sel >= suggestions.len() {
            sel = suggestions.len().saturating_sub(1);
        }
        let suggestion_range = suggestion_window(sel, suggestions.len(), &mut suggestion_start);
        let visible_suggestions = &suggestions[suggestion_range];
        let visible_sel = sel.saturating_sub(suggestion_start);
        render(
            term,
            &buf,
            cursor,
            visible_suggestions,
            visible_sel,
            *mode,
            effort,
            status().as_deref(),
            &pastes,
            &mut cur_row,
            &mut last_width,
        )?;

        let event = crate::keyread::read_event(term)?;
        let key = match event {
            crate::keyread::Event::Key(k) => k,
            // A paste arrives as a burst: collapse it to a placeholder if it's
            // multi-line or long, otherwise insert it literally (see `handle_paste`).
            crate::keyread::Event::Paste(text) => {
                handle_paste(&mut buf, &mut cursor, &mut pastes, &text);
                sel = 0;
                suggestion_start = 0;
                continue;
            }
        };
        match key {
            Key::Char(c) => {
                if c == '\u{4}' && buf.is_empty() {
                    return Ok(None); // Ctrl-D on an empty line: quit.
                }
                if c == '\u{16}' {
                    // Ctrl+V (SYN): attach a clipboard image if present, else fall
                    // back to pasting clipboard text. NB many terminals intercept
                    // Ctrl+V for their own paste, so this only fires where the key
                    // reaches us; `/paste` is the reliable path.
                    handle_clipboard_paste(&mut buf, &mut cursor, &mut pastes);
                    sel = 0;
                    suggestion_start = 0;
                    continue;
                }
                if c == '\n' {
                    // Ctrl+Enter (see keyread): insert a hard line break and keep editing.
                    buf.insert(cursor, '\n');
                    cursor += 1;
                    sel = 0;
                    suggestion_start = 0;
                    continue;
                }
                if c.is_control() {
                    continue; // ignore other stray control chars (e.g. pasted \r handled as Enter)
                }
                buf.insert(cursor, c);
                cursor += 1;
                sel = 0;
                suggestion_start = 0;
            }
            Key::Enter => {
                // With the palette open on a partially-typed command, Enter accepts the
                // highlighted suggestion (fills it in) rather than submitting the typo;
                // press Enter again to run it. Otherwise Enter submits what's typed.
                if !suggestions.is_empty() {
                    let chosen = suggestions[sel];
                    let typed: String = buf.iter().collect();
                    if typed != chosen.name {
                        fill(&mut buf, &mut cursor, chosen);
                        sel = 0;
                        suggestion_start = 0;
                        continue;
                    }
                }
                return submit(term, &buf, &pastes, &mut cur_row);
            }
            Key::Tab => {
                if !suggestions.is_empty() {
                    fill(&mut buf, &mut cursor, suggestions[sel]);
                    sel = 0;
                    suggestion_start = 0;
                }
            }
            Key::ArrowUp => sel = sel.saturating_sub(1),
            // Shift+Tab cycles the operating mode regardless of palette state.
            Key::BackTab => *mode = mode.cycle(),
            Key::ArrowDown => {
                if sel + 1 < suggestions.len() {
                    sel += 1;
                }
            }
            Key::ArrowLeft => cursor = cursor.saturating_sub(1),
            Key::ArrowRight => {
                if cursor < buf.len() {
                    cursor += 1;
                }
            }
            Key::Home => cursor = 0,
            Key::End => cursor = buf.len(),
            Key::Backspace => {
                if cursor > 0 {
                    // One sentinel char == one whole paste, so this deletes a
                    // collapsed paste atomically; free its stored text too.
                    let removed = buf.remove(cursor - 1);
                    if attach::is_sentinel(removed) {
                        pastes.remove(removed);
                    }
                    cursor -= 1;
                    sel = 0;
                    suggestion_start = 0;
                }
            }
            Key::Del => {
                if cursor < buf.len() {
                    let removed = buf.remove(cursor);
                    if attach::is_sentinel(removed) {
                        pastes.remove(removed);
                    }
                    sel = 0;
                    suggestion_start = 0;
                }
            }
            Key::Escape => {
                // Dismiss: clear the current input and start fresh.
                buf.clear();
                cursor = 0;
                sel = 0;
                suggestion_start = 0;
                pastes.clear();
            }
            Key::CtrlC => {
                collapse(term, "^C", &mut cur_row)?;
                if buf.is_empty() {
                    // Empty prompt: quit immediately, exactly like /exit.
                    return Ok(None); // caller treats None as "quit the app"
                }
                // Text typed: cancel the current line (clear in place), like a shell.
                return Ok(Some((String::new(), Vec::new())));
            }
            _ => {}
        }
    }
}

/// Replace the buffer with a chosen command, leaving a trailing space (and the
/// cursor after it) for arg-taking commands so the user can type arguments.
fn fill(buf: &mut Vec<char>, cursor: &mut usize, chosen: &Command) {
    *buf = chosen.name.chars().collect();
    if chosen.takes_args {
        buf.push(' ');
    }
    *cursor = buf.len();
}

/// Apply a pasted burst to the buffer at the cursor. CRLF/CR are normalised to
/// LF first. A multi-line or long paste is interned and represented by a single
/// sentinel char (rendered later as `[Pasted N lines]`); a short single-line
/// paste is inserted literally so it behaves exactly as if typed.
/// Handle a Ctrl+V in the editor: if the clipboard holds an image, intern it as an
/// `[Image …]` attachment at the cursor (one sentinel char, deletable like a paste);
/// otherwise fall back to pasting clipboard text so Ctrl+V still does the usual thing.
fn handle_clipboard_paste(buf: &mut Vec<char>, cursor: &mut usize, store: &mut AttachmentStore) {
    match crate::clipboard::read_clipboard_image() {
        Ok(img) => {
            let sentinel = store.intern(Attachment::Image(img));
            buf.insert(*cursor, sentinel);
            *cursor += 1;
        }
        Err(_) => {
            if let Some(text) = crate::clipboard::read_clipboard_text() {
                if !text.is_empty() {
                    handle_paste(buf, cursor, store, &text);
                }
            }
        }
    }
}

fn handle_paste(buf: &mut Vec<char>, cursor: &mut usize, store: &mut AttachmentStore, text: &str) {
    // Normalise newlines and strip stray control chars up front, so both the
    // collapsed and the inline path store/insert only clean text.
    let text = attach::sanitize_paste(text);
    if attach::should_collapse(&text) {
        let sentinel = store.intern(attach::Attachment::Pasted(text));
        buf.insert(*cursor, sentinel);
        *cursor += 1;
    } else {
        for ch in text.chars() {
            buf.insert(*cursor, ch);
            *cursor += 1;
        }
    }
}

/// The visible input line: buffer chars verbatim, except attachment sentinels,
/// which render as their dimmed placeholder (e.g. `[Pasted 1423 lines]`). Used for
/// both the live frame and the collapsed submit echo, so the two always agree.
fn render_line(buf: &[char], store: &AttachmentStore) -> String {
    let mut out = String::new();
    for &c in buf {
        match store.get(c) {
            Some(a) => out.push_str(&a.placeholder().dimmed().to_string()),
            None => out.push(c),
        }
    }
    out
}

/// Finalise `buf` as the submitted line: collapse the framed input down to a
/// single `> <line>` echo so scrollback stays tidy, then return the line with any
/// collapsed pastes expanded back to their real text (what the caller/model sees).
fn submit(
    term: &Term,
    buf: &[char],
    store: &AttachmentStore,
    cur_row: &mut usize,
) -> io::Result<Option<(String, Vec<LoadedImage>)>> {
    let echo = render_line(buf, store);
    collapse(term, &echo, cur_row)?;
    // Text from expansion, images gathered from their sentinels — together the
    // multimodal message the caller forwards to the turn.
    Ok(Some((
        attach::expand(buf, store),
        attach::collect_images(buf, store),
    )))
}

/// Wipe the drawn frame and echo a single `> <text>` line followed by a newline.
/// Used on submit and on Ctrl-C (with `text = "^C"`).
fn collapse(term: &Term, text: &str, cur_row: &mut usize) -> io::Result<()> {
    term.hide_cursor()?;
    if *cur_row > 0 {
        term.move_cursor_up(*cur_row)?;
    }
    term.write_str("\r")?;
    term.clear_to_end_of_screen()?;
    term.write_str(&format!("{}{}\n", PROMPT.bright_blue().bold(), text))?;
    *cur_row = 0;
    term.show_cursor()?;
    term.flush()
}

/// The frame's horizontal rule, with an optional short status (e.g. running
/// subagents) set right-aligned into it: `────┤ 2 agents · 1 web ├──`. Every
/// glyph used is single-cell, and the row never exceeds `width - 1` cells, so
/// the row-count math in `render` is unaffected. A status too wide for the
/// terminal falls back to the plain rule.
fn rule_row(width: usize, status: Option<&str>) -> String {
    let cells = width.saturating_sub(1).max(1);
    if let Some(status) = status.map(str::trim).filter(|s| !s.is_empty()) {
        let len = status.chars().count();
        // "┤ " + status + " ├" + trailing "──" plus at least one leading cell.
        if len + 7 <= cells {
            let lead = cells - len - 6;
            return format!(
                "{}{}{}{}{}",
                "─".repeat(lead).dimmed(),
                "┤ ".dimmed(),
                status.bright_cyan(),
                " ├".dimmed(),
                "──".dimmed()
            );
        }
    }
    "─".repeat(cells).dimmed().to_string()
}

/// Colour-coded mode indicator drawn beneath the input frame, with the
/// Shift+Tab affordance. The colour cues the risk level (blue → green → red).
fn mode_line(mode: Mode, effort: Effort) -> String {
    let label = match mode {
        Mode::Plan => "plan".blue().bold(),
        Mode::Agentic => "agentic".green().bold(),
        Mode::Yolo => "yolo".red().bold(),
    };
    // Ctrl+Enter newline only works where the raw key reader can see the modifier
    // (Windows). Elsewhere the terminal can't distinguish it from Enter, so we
    // don't advertise it.
    let newline_hint = if cfg!(windows) {
        " · ctrl+enter for newline".dimmed()
    } else {
        "".dimmed()
    };
    format!(
        "  {} {} · {} effort · {}{}",
        label,
        "mode".dimmed(),
        effort.as_str().bright_magenta(),
        "shift+tab to cycle".dimmed(),
        newline_hint
    )
}

/// Redraws the framed input — the prompt + buffer on top, a horizontal rule, the
/// mode line, then the optional suggestion overlay below — and parks the cursor at
/// `cursor` within the buffer. Width-aware so a buffer that wraps across rows is
/// cleared and repositioned correctly. `cur_row` tracks the cursor's row offset
/// from the *start of the input* across calls so the next redraw can climb back.
///
/// The input line is deliberately the topmost thing we draw: everything else (the
/// rule, mode line, suggestions) sits *below* the cursor, so a redraw only ever
/// clears downward with `clear_to_end_of_screen`. That makes resize robust — when
/// the terminal reflows on a width change we never have to erase wrapped content
/// *above* the cursor (which we can't track reliably), only re-find the input's
/// own start at the new width.
#[allow(clippy::too_many_arguments)] // Explicit terminal-frame state keeps redraws allocation-free.
fn render(
    term: &Term,
    buf: &[char],
    cursor: usize,
    suggestions: &[&Command],
    sel: usize,
    mode: Mode,
    effort: Effort,
    status: Option<&str>,
    store: &AttachmentStore,
    cur_row: &mut usize,
    last_width: &mut usize,
) -> io::Result<()> {
    let (_, cols) = term.size();
    let w = (cols as usize).max(1);
    // One char short of full width so the rule can't trip terminal auto-wrap.
    let rule = rule_row(w, status);
    // A collapsed-paste sentinel occupies its placeholder's width on screen; every
    // other char is one cell. The cursor math below flows through this.
    let cw = |c: char| store.cell_width(c);

    term.hide_cursor()?;
    // Climb to the start of the input line drawn last time, then wipe from there to
    // the end of the screen (the rule, mode line, and suggestion overlay below it).
    //
    // Normally the cursor sits `*cur_row` rows below the input's start, so we climb
    // that. But when the terminal was resized since the last draw (width changed),
    // the input may have reflowed and `*cur_row` (an old-width row count) is stale;
    // recompute the cursor's row within the input at the new width instead — valid
    // because buf/cursor are unchanged across a resize redraw. Nothing we drew sits
    // above the input, so there is never reflowed content above us to mis-erase.
    let climb = if *last_width != 0 && *last_width != w {
        layout(buf, cursor, PROMPT_WIDTH, w, &cw).0
    } else {
        *cur_row
    };
    if climb > 0 {
        term.move_cursor_up(climb)?;
    }
    term.write_str("\r")?;
    term.clear_to_end_of_screen()?;
    *last_width = w;

    // Frame: prompt + buffer (terminal wraps long lines on its own), a rule beneath
    // it, then the mode line. Collapsed pastes render as their placeholder.
    let line = render_line(buf, store);
    term.write_str(&format!("{}{}", PROMPT.bright_blue().bold(), line))?;
    let (input_end_row, _) = layout(buf, buf.len(), PROMPT_WIDTH, w, &cw);
    term.write_str(&format!("\n{rule}"))?;
    term.write_str(&format!("\n{}", mode_line(mode, effort)))?;

    // Suggestion overlay (each line is short — assumed not to wrap).
    for (i, cmd) in suggestions.iter().enumerate() {
        term.write_str("\n")?;
        let marker = if i == sel { "▸" } else { " " };
        let name = format!("{:<width$}", cmd.name, width = NAME_COL);
        let row = if i == sel {
            format!(
                "  {} {} {}",
                marker.bright_cyan(),
                name.bright_cyan().bold(),
                cmd.summary.dimmed()
            )
        } else {
            format!("  {} {} {}", marker, name.cyan(), cmd.summary.dimmed())
        };
        term.write_str(&row)?;
    }

    // Row layout within the block (0 = input's first row): the input spans rows
    // 0..=input_end_row, then the rule, the mode line, then the suggestions. After
    // writing everything the cursor is on the last line; climb back to the input's
    // start, then step down/right to the cursor's cell within the input.
    let last_row = input_end_row + 2 + suggestions.len();
    if last_row > 0 {
        term.move_cursor_up(last_row)?;
    }
    term.write_str("\r")?;
    let (row_in_input, target_col) = layout(buf, cursor, PROMPT_WIDTH, w, &cw);
    if row_in_input > 0 {
        term.move_cursor_down(row_in_input)?;
    }
    if target_col > 0 {
        term.move_cursor_right(target_col)?;
    }
    *cur_row = row_in_input;
    term.show_cursor()?;
    term.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_slash_lists_highest_priority_first() {
        let top = rank("/");
        assert_eq!(top.len(), MAX_SUGGESTIONS);
        assert_eq!(top[0].name, "/ask"); // priority 100
                                         // Strictly descending priority among the shown set.
        for pair in top.windows(2) {
            assert!(pair[0].priority >= pair[1].priority);
        }
    }

    #[test]
    fn rule_row_right_aligns_status_and_falls_back_when_too_narrow() {
        let plain = console::strip_ansi_codes(&rule_row(40, Some("2 agents"))).to_string();
        assert!(plain.ends_with("┤ 2 agents ├──"));
        assert_eq!(plain.chars().count(), 39, "row must never wrap");
        assert_eq!(
            console::strip_ansi_codes(&rule_row(40, None)).to_string(),
            "─".repeat(39)
        );
        // A status too wide for the terminal degrades to the plain rule.
        assert_eq!(
            console::strip_ansi_codes(&rule_row(10, Some("a very long status"))).to_string(),
            "─".repeat(9)
        );
        // Blank statuses render nothing.
        assert_eq!(
            console::strip_ansi_codes(&rule_row(40, Some("  "))).to_string(),
            "─".repeat(39)
        );
    }

    #[test]
    fn prefix_match_beats_substring_and_fuzzy() {
        let top = rank("/em");
        // "/embed" and "/emodel" are prefix matches; they must precede any
        // fuzzy hit and "/embed" (shorter) ranks above "/emodel".
        assert_eq!(top[0].name, "/embed");
        assert_eq!(top[1].name, "/emodel");
    }

    #[test]
    fn exact_typed_command_is_first() {
        let top = rank("/ask");
        assert_eq!(top[0].name, "/ask");
    }

    #[test]
    fn branching_commands_are_discoverable() {
        assert_eq!(rank("/btw")[0].name, "/btw");
        assert_eq!(rank("/fork")[0].name, "/fork");
        let btw = COMMANDS
            .iter()
            .find(|command| command.name == "/btw")
            .unwrap();
        assert!(btw.takes_args);
        assert!(btw.summary.contains("/side"));
    }

    #[test]
    fn substring_matches_when_not_a_prefix() {
        // "hist" is not a prefix of any command but is a substring of "/history".
        let top = rank("/hist");
        assert!(top.iter().any(|c| c.name == "/history"));
    }

    #[test]
    fn fuzzy_subsequence_still_matches() {
        // "drs" -> /di**R**-**S**ummaries? subsequence d,r,s appears in "dir-summaries".
        let top = rank("/drs");
        assert!(top.iter().any(|c| c.name == "/dir-summaries"));
    }

    #[test]
    fn nonsense_query_returns_nothing() {
        assert!(rank("/zzqq").is_empty());
    }

    #[test]
    fn never_returns_more_than_max() {
        assert!(rank("/").len() <= MAX_SUGGESTIONS);
        assert!(rank("/e").len() <= MAX_SUGGESTIONS);
    }

    #[test]
    fn full_rank_keeps_matches_beyond_the_visible_window() {
        let matches = rank_all("/");
        let visible = rank("/");
        assert_eq!(matches.len(), COMMANDS.len());
        assert!(matches.len() > MAX_SUGGESTIONS);
        assert_eq!(
            visible
                .iter()
                .map(|command| command.name)
                .collect::<Vec<_>>(),
            matches[..MAX_SUGGESTIONS]
                .iter()
                .map(|command| command.name)
                .collect::<Vec<_>>()
        );
        assert!(!visible
            .iter()
            .any(|command| command.name == matches[MAX_SUGGESTIONS].name));
    }

    #[test]
    fn suggestion_window_scrolls_down_and_back_up() {
        let suggestion_count = MAX_SUGGESTIONS + 3;
        let mut start = 0;

        assert_eq!(
            suggestion_window(0, suggestion_count, &mut start),
            0..MAX_SUGGESTIONS
        );
        assert_eq!(
            suggestion_window(MAX_SUGGESTIONS - 1, suggestion_count, &mut start),
            0..MAX_SUGGESTIONS
        );

        // Moving to the sixth result scrolls the five-row viewport down once.
        assert_eq!(
            suggestion_window(MAX_SUGGESTIONS, suggestion_count, &mut start),
            1..MAX_SUGGESTIONS + 1
        );
        assert_eq!(start, 1);

        // The viewport stays put while the selection remains visible, then follows
        // it back to the first result when Up crosses the top edge.
        assert_eq!(
            suggestion_window(MAX_SUGGESTIONS - 1, suggestion_count, &mut start),
            1..MAX_SUGGESTIONS + 1
        );
        assert_eq!(
            suggestion_window(0, suggestion_count, &mut start),
            0..MAX_SUGGESTIONS
        );

        assert_eq!(
            suggestion_window(suggestion_count - 1, suggestion_count, &mut start),
            suggestion_count - MAX_SUGGESTIONS..suggestion_count
        );
    }

    #[test]
    fn layout_wraps_and_breaks_on_newlines() {
        // Default cell width: one column per char.
        let one = |_: char| 1usize;
        let flat: Vec<char> = "abcdef".chars().collect();
        // No newlines: behaves like flat cell math behind a 2-col prompt.
        assert_eq!(layout(&flat, 0, 2, 80, &one), (0, 2)); // caret right after prompt
        assert_eq!(layout(&flat, 6, 2, 80, &one), (0, 8)); // 6 chars in, same row

        // Soft wrap at the right edge (prompt 2 + 78 chars == 80 == width).
        let long: Vec<char> = std::iter::repeat_n('x', 100).collect();
        assert_eq!(layout(&long, 78, 2, 80, &one), (1, 0)); // wrapped to row 1, col 0

        // Hard line break: '\n' drops a row and resets to col 0 (no prompt indent).
        let multi: Vec<char> = "ab\ncd".chars().collect();
        assert_eq!(layout(&multi, 2, 2, 80, &one), (0, 4)); // just before the '\n'
        assert_eq!(layout(&multi, 3, 2, 80, &one), (1, 0)); // just after the '\n'
        assert_eq!(layout(&multi, 5, 2, 80, &one), (1, 2)); // end of second line

        assert_eq!(layout(&flat, 5, 2, 0, &one), (5, 0)); // zero width clamps to 1, no panic
    }

    #[test]
    fn layout_counts_a_wide_paste_sentinel_by_its_placeholder_width() {
        // A sentinel whose placeholder is 6 columns advances the caret by 6, not 1.
        let wide = |c: char| if c == '\u{E000}' { 6 } else { 1 };
        let buf: Vec<char> = vec!['a', '\u{E000}', 'b'];
        // After 'a' (col 3), the sentinel adds 6 → col 9, then 'b' → col 10.
        assert_eq!(layout(&buf, 1, 2, 80, &wide), (0, 3)); // before the sentinel
        assert_eq!(layout(&buf, 2, 2, 80, &wide), (0, 9)); // after the 6-col sentinel
        assert_eq!(layout(&buf, 3, 2, 80, &wide), (0, 10));
    }

    #[test]
    fn palette_active_only_for_bare_command_token() {
        assert!(palette_active(&"/as".chars().collect::<Vec<_>>()));
        assert!(palette_active(&"/".chars().collect::<Vec<_>>()));
        assert!(!palette_active(&"/ask ".chars().collect::<Vec<_>>())); // has space
        assert!(!palette_active(&"hello".chars().collect::<Vec<_>>())); // not a command
        assert!(!palette_active(&[])); // empty
    }

    #[test]
    fn registry_names_are_unique_and_slash_prefixed() {
        let mut seen = std::collections::HashSet::new();
        for c in COMMANDS {
            assert!(c.name.starts_with('/'), "{} missing slash", c.name);
            assert!(!c.usage.is_empty(), "{} missing usage", c.name);
            assert!(!c.summary.is_empty(), "{} missing summary", c.name);
            assert!(!c.category.is_empty(), "{} missing category", c.name);
            assert!(seen.insert(c.name), "duplicate command {}", c.name);
        }
    }
}
