//! The index and model-call passes driven by slash commands: structural indexing
//! (`/index`, `/rebuild`), the semantic-enrichment family (`/embed`, `/summarize`,
//! `/enrich`, `/architecture`, `/relationships`, `/dir-summaries`), the `/compact`
//! history summariser, and the startup graph-drift check. Each long pass is
//! Ctrl+C-escapable via [`App::cancellable`] and streams per-item progress.

use std::time::Duration;

use colored::*;
use dialoguer::Select;
use gaise_core::contracts::{GaiseContent, GaiseInstructRequest, GaiseMessage, OneOrMany};
use indicatif::{ProgressBar, ProgressStyle};
use uuid::Uuid;

use crate::app::App;
use crate::session;
use ikode::harness::ProjectConfig;
use ikode::index::{SummarizeOptions, SummaryProgress};
use ikode::settings::{AutoCompactRule, AutoCompactThreshold, Mode};

/// Storage size past which we surface the manual checkpoint command. Automatic
/// checkpoints bound transaction-delta growth, but an embedding-heavy exact-state
/// snapshot can itself exceed this threshold.
const WAL_SQUASH_THRESHOLD_BYTES: u64 = 300 * 1024 * 1024;

/// Format a byte count as a short human-readable size (e.g. `312.4 MB`).
fn fmt_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// A parsed `/compact auto <arg>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompactAutoChange {
    /// `<n>%` — percent of the model's context window (1–100).
    Percent(u8),
    /// `<n>` — absolute prompt-token threshold.
    Tokens(usize),
    /// `off` / `0` — disable auto-compaction.
    Off,
    /// `default` / `reset` — clear both settings.
    Default,
}

/// Parse the argument of `/compact auto`. Accepts `80%`, `80 %`, `200000`,
/// `200_000`, `200,000`, `200k`, `off`, `default`. Percentages must be 1–100;
/// `0` or `0%` is `Off`.
pub(crate) fn parse_compact_auto_arg(arg: &str) -> Option<CompactAutoChange> {
    let arg = arg.trim().to_ascii_lowercase();
    match arg.as_str() {
        "off" | "disable" | "disabled" | "none" => return Some(CompactAutoChange::Off),
        "default" | "reset" => return Some(CompactAutoChange::Default),
        _ => {}
    }
    if let Some(pct) = arg.strip_suffix('%') {
        let pct: u8 = pct.trim().parse().ok()?;
        return Some(match pct {
            0 => CompactAutoChange::Off,
            1..=100 => CompactAutoChange::Percent(pct),
            _ => return None,
        });
    }
    let (digits, multiplier) = match arg.strip_suffix('k') {
        Some(n) => (n, 1_000usize),
        None => (arg.as_str(), 1),
    };
    let digits: String = digits.chars().filter(|c| !matches!(c, '_' | ',')).collect();
    let tokens: usize = digits.parse().ok()?;
    let tokens = tokens.checked_mul(multiplier)?;
    Some(if tokens == 0 {
        CompactAutoChange::Off
    } else {
        CompactAutoChange::Tokens(tokens)
    })
}

fn print_ikignore_files(files: &[String]) {
    if files.is_empty() {
        return;
    }
    let shown = files.iter().take(6).cloned().collect::<Vec<_>>();
    let more = if files.len() > shown.len() {
        format!(" (+{} more)", files.len() - shown.len())
    } else {
        String::new()
    };
    println!(
        "  {} Found {} .ikignore file{}: {}{}",
        "✓".bright_green(),
        files.len().to_string().bright_magenta().bold(),
        if files.len() == 1 { "" } else { "s" },
        shown.join(", ").dimmed(),
        more.dimmed()
    );
}

impl App {
    /// After the WAL-backed graph is loaded at startup, tell the user if it has
    /// drifted from the working tree (files added / changed / deleted since the
    /// last index) and offer to re-sync. Purely algorithmic (hash diff, no
    /// inference). No-ops when there's no prior graph to compare against, so a
    /// brand-new project is never nagged.
    pub(crate) async fn check_startup_sync(&mut self) {
        if !self.graph_enabled {
            return;
        }
        self.warn_if_wal_oversized();
        if self.indexer.graph_file_paths().is_empty() {
            return; // no prior index on disk — nothing to compare
        }
        let d = self.indexer.project_discrepancies();
        if d.is_empty() {
            println!(
                "{} Code graph loaded — in sync with the project.",
                "🕸️ ".bright_green()
            );
            return;
        }

        println!(
            "{} Code graph is out of sync with the project ({} change{}):",
            "⚠️ ".bright_yellow(),
            d.total(),
            if d.total() == 1 { "" } else { "s" }
        );
        let line = |label: &str, items: &[String]| {
            if items.is_empty() {
                return;
            }
            let shown: Vec<&str> = items.iter().take(5).map(|s| s.as_str()).collect();
            let more = if items.len() > shown.len() {
                format!(" (+{} more)", items.len() - shown.len())
            } else {
                String::new()
            };
            println!(
                "  {} {} {}: {}{}",
                "•".dimmed(),
                items.len().to_string().bright_magenta().bold(),
                label,
                shown.join(", ").dimmed(),
                more.dimmed()
            );
        };
        line("changed", &d.changed);
        line("new", &d.added);
        line("deleted", &d.deleted);

        // Three choices, defaulting to the richest: re-index *and* re-enrich so the
        // semantic index is current too. Both passes are hash-gated, so "enrich" only
        // costs model calls for chunks that actually changed. In yolo/brave we keep the
        // old behaviour (re-index only, no prompt) so a non-interactive launch never
        // silently fires off model calls.
        if self.brave || self.settings.mode == Mode::Yolo {
            self.run_index();
            return;
        }
        let items = ["Yes and enrich", "Yes", "No"];
        let prompt = format!(
            "{} {}?",
            "❓ ".bright_yellow(),
            "Re-index now to bring the graph in sync".cyan()
        );
        let choice = Select::new()
            .with_prompt(prompt)
            .items(&items)
            .default(0)
            .interact()
            .unwrap_or(2);
        match choice {
            0 => {
                self.run_index();
                self.run_enrich(0).await;
            }
            1 => self.run_index(),
            _ => println!(
                "{} Skipped. Run {} any time to sync.",
                "•".dimmed(),
                "/index".cyan()
            ),
        }
    }

    /// If graph storage is large, surface `/squash`. Purely informational: automatic
    /// delta checkpoints remain enabled and a large exact snapshot may not shrink.
    fn warn_if_wal_oversized(&self) {
        let size = self.indexer.wal_size_bytes();
        if size >= WAL_SQUASH_THRESHOLD_BYTES {
            println!(
                "{} The code-graph storage file is large ({}). Run {} to checkpoint \
                 the exact state and discard any remaining transaction history.",
                "🗜️ ".bright_yellow(),
                fmt_bytes(size).bright_magenta().bold(),
                "/squash".cyan(),
            );
        }
    }

    pub(crate) fn run_index(&mut self) {
        // Per-chunk progress streams directly (indexing is synchronous and fast), so no
        // spinner here — the chunk lines are the progress indicator.
        println!("{}", "📚  Indexing code and markdown...".dimmed());
        let mut on_chunk = |cid: &str| {
            println!("  {} {}", "chunked".dimmed(), cid.dimmed());
        };
        let stats = self.indexer.index_all_reporting(&mut on_chunk);
        if let Some(error) = &stats.error {
            println!("{} Indexing aborted: {}", "⚠️ ".bright_yellow(), error);
            return;
        }
        print_ikignore_files(&stats.ikignore_files);

        let removed_note = if stats.removed > 0 {
            format!(", {} removed", stats.removed)
        } else {
            String::new()
        };
        let ignored_note = if stats.ignored > 0 {
            format!(", {} ignored", stats.ignored)
        } else {
            String::new()
        };
        println!(
            "{} Indexed {} files, {} chunks, {} references ({} skipped{}{}).",
            "📚 ".bright_green(),
            stats.files.to_string().bright_magenta().bold(),
            stats.chunks.to_string().bright_magenta().bold(),
            stats.links.to_string().bright_magenta().bold(),
            stats.skipped,
            ignored_note,
            removed_note
        );
        let breakdown = self.indexer.language_breakdown();
        if !breakdown.is_empty() {
            let summary: Vec<String> = breakdown
                .iter()
                .map(|(lang, n)| format!("{} ({})", lang, n))
                .collect();
            println!("  {}", summary.join(", ").dimmed());
        }
    }

    /// Destructive graph rebuild (`/rebuild`): wipes the WAL + in-memory state and
    /// re-indexes from scratch. Confirms first since derived data is lost.
    pub(crate) fn run_rebuild(&mut self) {
        if !self.brave
            && !self.ask_permission_sync(
                "Rebuild the code graph from scratch (deletes .ikode/graph.log)",
                false,
            )
        {
            println!("{} Rebuild cancelled.", "•".dimmed());
            return;
        }
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("{spinner:.green} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        pb.set_message("Rebuilding graph from scratch...");
        pb.enable_steady_tick(Duration::from_millis(100));
        let stats = self.indexer.rebuild();
        pb.finish_and_clear();
        if let Some(error) = &stats.error {
            println!(
                "{} Rebuild indexing aborted: {}",
                "⚠️ ".bright_yellow(),
                error
            );
            return;
        }
        print_ikignore_files(&stats.ikignore_files);
        println!(
            "{} Rebuilt: {} files, {} chunks, {} references ({} skipped, {} ignored).",
            "♻️".bright_green(),
            stats.files.to_string().bright_magenta().bold(),
            stats.chunks.to_string().bright_magenta().bold(),
            stats.links.to_string().bright_magenta().bold(),
            stats.skipped,
            stats.ignored
        );
    }

    fn apply_ikignore_now(&mut self) -> bool {
        match self.indexer.prune_ikignored() {
            Ok(report) => {
                print_ikignore_files(&report.files);
                if report.removed > 0 {
                    println!(
                        "  {} Removed {} newly ignored file{} from the graph.",
                        "−".bright_yellow(),
                        report.removed.to_string().bright_magenta().bold(),
                        if report.removed == 1 { "" } else { "s" }
                    );
                }
                true
            }
            Err(error) => {
                println!(
                    "{} Could not apply .ikignore: {}",
                    "⚠️ ".bright_yellow(),
                    error
                );
                false
            }
        }
    }

    /// Compact the WAL (`/squash`) to one exact, compressed graph snapshot. This
    /// preserves internal IDs/indexes/idempotency state while discarding history.
    /// Confirms first (history is irreversibly lost), auto-confirming in brave/yolo.
    pub(crate) fn run_squash(&mut self) {
        let before = self.indexer.wal_size_bytes();
        if before == 0 {
            println!("{} No WAL to squash yet.", "•".dimmed());
            return;
        }
        if !self.brave
            && !self.ask_permission_sync(
                &format!(
                    "Checkpoint the WAL ({}) — discard history, preserve exact state",
                    fmt_bytes(before)
                ),
                false,
            )
        {
            println!("{} Squash cancelled.", "•".dimmed());
            return;
        }
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("{spinner:.green} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        pb.set_message("Checkpointing WAL — encoding exact graph state...");
        pb.enable_steady_tick(Duration::from_millis(100));
        let result = self.indexer.squash();
        pb.finish_and_clear();
        match result {
            Ok(stats) => {
                let saved = stats.before_bytes.saturating_sub(stats.after_bytes);
                let pct = if stats.before_bytes > 0 {
                    (saved as f64 / stats.before_bytes as f64) * 100.0
                } else {
                    0.0
                };
                println!(
                    "{} Checkpointed WAL: {} → {} ({} reclaimed, {:.0}%). Preserved {} nodes, {} edges at sequence {} (raw snapshot {}).",
                    "🗜️ ".bright_green(),
                    fmt_bytes(stats.before_bytes).bright_magenta().bold(),
                    fmt_bytes(stats.after_bytes).bright_magenta().bold(),
                    fmt_bytes(saved),
                    pct,
                    stats.nodes.to_string().bright_magenta().bold(),
                    stats.edges.to_string().bright_magenta().bold(),
                    stats.last_sequence.to_string().bright_magenta().bold(),
                    fmt_bytes(stats.uncompressed_snapshot_bytes),
                );
            }
            Err(e) => println!("{} Squash failed: {}", "⚠️ ".bright_yellow(), e),
        }
    }

    /// `/embed` — build (or refresh) the semantic embedding index over all chunks,
    /// using the configured embedding model. Enables semantic `/ask`.
    /// `/embed` — returns `false` if the user cancelled with Ctrl+C, so `/enrich` can
    /// stop the pipeline rather than press on to the next pass.
    pub(crate) async fn run_embed(&mut self) -> bool {
        self.run_embed_with_ignore_refresh(true).await
    }

    async fn run_embed_with_ignore_refresh(&mut self, refresh_ignores: bool) -> bool {
        if self.indexer.is_empty() {
            if refresh_ignores {
                self.run_index();
            }
        } else if refresh_ignores && !self.apply_ikignore_now() {
            return false;
        }
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("{spinner:.green} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        pb.set_message(format!("Embedding chunks with {}...", self.embedding_model));
        pb.enable_steady_tick(Duration::from_millis(100));
        let mut on_item = |cid: &str| {
            pb.suspend(|| println!("  {} {}", "embedded".dimmed(), cid.dimmed()));
        };
        let fut = self.indexer.embed_index_reporting(
            self.client.as_ref(),
            &self.embedding_model,
            &mut on_item,
        );
        let result = self.cancellable(fut).await;
        pb.finish_and_clear();
        match result {
            None => false,
            Some(Ok(stats)) => {
                let ignored = if stats.ignored == 0 {
                    String::new()
                } else {
                    format!(", {} excluded by .ikignore", stats.ignored)
                };
                println!(
                    "{} Embedded {} chunks ({} already current{}).",
                    "🧬 ".bright_green(),
                    stats.embedded.to_string().bright_magenta().bold(),
                    stats.skipped,
                    ignored
                );
                true
            }
            Some(Err(e)) => {
                println!("{} Embedding failed: {}", "⚠️ ".bright_yellow(), e);
                // Report failure so `/enrich` doesn't go on to claim "semantic /ask
                // is ready" when no vectors were actually stored.
                false
            }
        }
    }

    /// `/enrich [max]` — the full semantic-index pass in dependency order:
    /// summarise chunks first, then embed. Because an embedding document folds in a
    /// chunk's stored summary, summarising before embedding yields richer vectors in
    /// one shot. `max` caps the summary call count (0 = all); embedding always covers
    /// every chunk that is out of date. Both passes are incremental/hash-gated, so
    /// re-running `/enrich` only touches what changed.
    pub(crate) async fn run_enrich(&mut self, max: usize) {
        // A Ctrl+C in either pass aborts the whole pipeline rather than silently
        // continuing to the next pass.
        if !self.run_summarize(max).await {
            return;
        }
        if !self.run_embed_with_ignore_refresh(false).await {
            return;
        }
        println!(
            "{} Enrich complete — semantic /ask is ready.",
            "✨ ".bright_green()
        );
    }

    /// `/summarize [max]` — generate one-line natural-language summaries for chunks
    /// via the summary model and store them on the graph. `max` caps inference calls.
    /// Returns `false` if the user cancelled with Ctrl+C.
    pub(crate) async fn run_summarize(&mut self, max: usize) -> bool {
        if self.indexer.is_empty() {
            self.run_index();
        } else if !self.apply_ikignore_now() {
            return false;
        }
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("{spinner:.green} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        // Summary policy comes from resolved config so the `update_settings` tool's
        // writes take effect on the next pass without a restart.
        let cfg = ProjectConfig::resolve(&self.working_directory);
        let opts = SummarizeOptions {
            include_tests: cfg.summarize_tests.unwrap_or(false),
            min_body_lines: cfg.summarize_min_lines.unwrap_or(4),
            concurrency: cfg.summarize_concurrency.unwrap_or(4),
        };
        pb.set_message(format!(
            "Summarising chunks (x{} concurrent{})...",
            opts.concurrency.max(1),
            if opts.include_tests {
                ", incl. tests"
            } else {
                ""
            }
        ));
        pb.enable_steady_tick(Duration::from_millis(100));
        let mut on_item = |cid: &str, status: SummaryProgress| {
            let (verb, suffix) = match status {
                SummaryProgress::Summarised => ("summarised", ""),
                SummaryProgress::Skipped => ("skipped", " (already enriched)"),
            };
            pb.suspend(|| println!("  {} {}{}", verb.dimmed(), cid.dimmed(), suffix.dimmed()));
        };
        let fut = self.indexer.summarize_index_with(
            self.client.as_ref(),
            &self.summary_model,
            max,
            opts,
            &mut on_item,
        );
        let result = self.cancellable(fut).await;
        pb.finish_and_clear();
        match result {
            None => false,
            Some(Ok(stats)) => {
                let ignored = if stats.ignored == 0 {
                    String::new()
                } else {
                    format!(", {} excluded by .ikignore", stats.ignored)
                };
                println!(
                    "{} Summarised {} chunks ({} already current{}).",
                    "📝 ".bright_green(),
                    stats.summarized.to_string().bright_magenta().bold(),
                    stats.skipped,
                    ignored
                );
                true
            }
            Some(Err(e)) => {
                println!("{} Summarisation failed: {}", "⚠️ ".bright_yellow(), e);
                true
            }
        }
    }

    /// `/architecture` — a single-call prose overview of how the codebase fits
    /// together, derived from the structural graph.
    pub(crate) async fn run_architecture(&mut self) {
        if self.indexer.is_empty() {
            self.run_index();
        } else if !self.apply_ikignore_now() {
            return;
        }
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("{spinner:.green} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        pb.set_message("Summarising architecture...");
        pb.enable_steady_tick(Duration::from_millis(100));
        let fut = self
            .indexer
            .summarize_architecture(self.client.as_ref(), &self.summary_model);
        let result = self.cancellable(fut).await;
        pb.finish_and_clear();
        match result {
            None => {}
            Some(Ok(text)) => println!(
                "{} {}\n{}",
                "🏛️".bright_green(),
                "Architecture overview:".bold(),
                text
            ),
            Some(Err(e)) => println!(
                "{} Architecture summary failed: {}",
                "⚠️ ".bright_yellow(),
                e
            ),
        }
    }

    /// `/relationships [kind] [max]` — describe each edge of `kind` (default
    /// `REFERENCES`) in one line via the chat model, stored on the edge. Surfaces in
    /// `/ask` connectivity.
    pub(crate) async fn run_relationships(&mut self, kind: &str, max: usize) {
        if self.indexer.is_empty() {
            self.run_index();
        } else if !self.apply_ikignore_now() {
            return;
        }
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("{spinner:.green} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        pb.set_message(format!("Describing {} relationships...", kind));
        pb.enable_steady_tick(Duration::from_millis(100));
        let mut on_item = |detail: &str| {
            pb.suspend(|| println!("  {} {}", "described".dimmed(), detail.dimmed()));
        };
        let fut = self.indexer.summarize_relationships_reporting(
            self.client.as_ref(),
            &self.summary_model,
            kind,
            max,
            &mut on_item,
        );
        let result = self.cancellable(fut).await;
        pb.finish_and_clear();
        match result {
            None => {}
            Some(Ok(stats)) => println!(
                "{} Described {} {} edge(s) ({} already current).",
                "🔗".bright_green(),
                stats.summarized.to_string().bright_magenta().bold(),
                kind,
                stats.skipped
            ),
            Some(Err(e)) => println!(
                "{} Relationship summary failed: {}",
                "⚠️ ".bright_yellow(),
                e
            ),
        }
    }

    /// `/dir-summaries [max]` — roll up a one-line summary for each directory from its
    /// chunk summaries (best run after `/summarize`), stored on the Directory node.
    pub(crate) async fn run_directories(&mut self, max: usize) {
        if self.indexer.is_empty() {
            self.run_index();
        } else if !self.apply_ikignore_now() {
            return;
        }
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("{spinner:.green} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        pb.set_message("Summarising directories...");
        pb.enable_steady_tick(Duration::from_millis(100));
        let mut on_item = |detail: &str| {
            pb.suspend(|| println!("  {} {}", "summarised".dimmed(), detail.dimmed()));
        };
        let fut = self.indexer.summarize_directories_reporting(
            self.client.as_ref(),
            &self.summary_model,
            max,
            &mut on_item,
        );
        let result = self.cancellable(fut).await;
        pb.finish_and_clear();
        match result {
            None => {}
            Some(Ok(stats)) => println!(
                "{} Summarised {} director{} ({} already current).",
                "🗂️".bright_green(),
                stats.summarized.to_string().bright_magenta().bold(),
                if stats.summarized == 1 { "y" } else { "ies" },
                stats.skipped
            ),
            Some(Err(e)) => println!("{} Directory summary failed: {}", "⚠️ ".bright_yellow(), e),
        }
    }

    /// `/compact` — summarise older turns into a single note via the chat model,
    /// keeping the most recent turns verbatim, then continue in the same session.
    /// Ctrl+C-escapable; persists the compacted transcript.
    pub(crate) async fn run_compact(&mut self) {
        match self.compact_messages(&self.history).await {
            Some(new_history) => self.apply_compacted_interactive(new_history),
            None => println!("{} Not enough history to compact yet.", "•".dimmed()),
        }
    }

    /// `/compact auto [<percent>% | <tokens> | off | default]` — show or change the
    /// auto-compaction threshold. Changes persist to `.ikode/settings.local.json`.
    ///
    /// - `<n>%` sets `auto_compact_percent` (of the chat model's context window) and
    ///   clears any absolute `auto_compact_tokens` override;
    /// - `<n>` (plain number) sets an absolute `auto_compact_tokens` override;
    /// - `off` disables; `default` restores the built-in percentage.
    pub(crate) async fn run_compact_auto(&mut self, arg: &str) {
        let arg = arg.trim();
        if arg.is_empty() {
            self.show_compact_auto().await;
            return;
        }
        let change = match parse_compact_auto_arg(arg) {
            Some(change) => change,
            None => {
                println!(
                    "{} Usage: /compact auto [<percent>% | <tokens> | off | default]",
                    "⚠️ ".bright_yellow()
                );
                return;
            }
        };
        match change {
            CompactAutoChange::Percent(percent) => {
                self.settings.auto_compact_percent = Some(percent);
                self.settings.auto_compact_tokens = None;
            }
            CompactAutoChange::Tokens(tokens) => {
                self.settings.auto_compact_tokens = Some(tokens);
            }
            CompactAutoChange::Off => {
                self.settings.auto_compact_tokens = Some(0);
            }
            CompactAutoChange::Default => {
                self.settings.auto_compact_percent = None;
                self.settings.auto_compact_tokens = None;
            }
        }
        if let Err(error) = self.settings.save(&self.working_directory) {
            println!(
                "{} Auto-compact setting changed for this session, but could not be persisted: {}",
                "⚠️ ".bright_yellow(),
                error
            );
        }
        self.show_compact_auto().await;
    }

    /// Print the auto-compact policy for the current chat model and how close the
    /// conversation is to it.
    async fn show_compact_auto(&mut self) {
        let threshold = self.auto_compact_threshold().await;
        println!(
            "{} Auto-compact: {}",
            "🗜️ ".bright_blue(),
            Self::describe_auto_compact_rule(&threshold).bright_magenta()
        );
        if threshold.tokens == 0 {
            println!(
                "  {} Manual {} only. Re-enable with {} or {}.",
                "•".dimmed(),
                "/compact".cyan(),
                "/compact auto 80%".cyan(),
                "/compact auto default".cyan()
            );
            return;
        }
        if matches!(threshold.rule, AutoCompactRule::Fallback) {
            println!(
                "  {} No context window is known for {} (not in the GAISe registry and \
                 the provider reports none); set an absolute limit with {}.",
                "•".dimmed(),
                self.model.bright_magenta(),
                "/compact auto <tokens>".cyan()
            );
        }
        let context = self.last_input_tokens;
        let used = if threshold.tokens > 0 {
            (context as f64 / threshold.tokens as f64 * 100.0).round() as usize
        } else {
            0
        };
        println!(
            "  {} Current context: {} tokens ({}% of the threshold).",
            "•".dimmed(),
            ikode::util::comma(context).bright_magenta(),
            used
        );
    }

    /// The chat model's context window in tokens (`None` = unknown), resolved once
    /// per model and cached: GAISe's bundled registry first, then a bounded live
    /// `list_models` call to the provider. See [`ikode::models`].
    pub(crate) async fn context_window(&mut self) -> Option<u64> {
        if let Some(window) = self.context_windows.get(&self.model) {
            return *window;
        }
        let window = ikode::models::context_window(self.client.as_ref(), &self.model).await;
        self.context_windows.insert(self.model.clone(), window);
        window
    }

    /// The auto-compact threshold in force for the current chat model: an explicit
    /// `auto_compact_tokens`, else `auto_compact_percent` of the model's context
    /// window, else the absolute fallback when the window is unknown.
    pub(crate) async fn auto_compact_threshold(&mut self) -> AutoCompactThreshold {
        let window = if self.settings.auto_compact_tokens.is_some()
            || self.settings.auto_compact_percent() == 0
        {
            // Neither rule consults the window — skip the lookup.
            None
        } else {
            self.context_window().await
        };
        self.settings.auto_compact_threshold(window)
    }

    /// One-line description of how a threshold was derived, for status messages.
    pub(crate) fn describe_auto_compact_rule(threshold: &AutoCompactThreshold) -> String {
        match threshold.rule {
            AutoCompactRule::Disabled => "auto-compaction disabled".to_string(),
            AutoCompactRule::Tokens => format!(
                "{} tokens (explicit auto_compact_tokens)",
                ikode::util::comma(threshold.tokens)
            ),
            AutoCompactRule::Percent { percent, window } => format!(
                "{} tokens ({percent}% of the model's {}-token context window)",
                ikode::util::comma(threshold.tokens),
                ikode::util::comma(window as usize)
            ),
            AutoCompactRule::Fallback => format!(
                "{} tokens (context window unknown — absolute fallback)",
                ikode::util::comma(threshold.tokens)
            ),
        }
    }

    /// Auto-compact the interactive conversation when its context (the last request's
    /// prompt size) has grown past the configured threshold. Quiet on a no-op so it
    /// can be called after every turn without nagging.
    pub(crate) async fn maybe_auto_compact(&mut self) {
        // Cheap pre-check before any (possibly live) window lookup: nothing measured
        // yet means nothing to compact.
        if self.last_input_tokens == 0 {
            return;
        }
        let threshold = self.auto_compact_threshold().await;
        if threshold.tokens == 0 || self.last_input_tokens < threshold.tokens {
            return;
        }
        println!(
            "{} Context at {} tokens ≥ {} — auto-compacting…",
            "🗜️ ".bright_cyan(),
            ikode::util::comma(self.last_input_tokens),
            Self::describe_auto_compact_rule(&threshold)
        );
        if let Some(new_history) = self.compact_messages(&self.history).await {
            self.apply_compacted_interactive(new_history);
            // Reset the gauge so we don't re-trigger before the next real measurement.
            self.last_input_tokens = 0;
        }
    }

    /// Swap in a freshly-compacted history for the interactive session: reset the
    /// cache lane (the prefix changed) and rewrite the transcript on disk.
    fn apply_compacted_interactive(&mut self, new_history: Vec<GaiseMessage>) {
        let old_len = self.history.len();
        let kept = new_history.len().saturating_sub(2);
        let summarised = old_len.saturating_sub(kept + 1);
        self.history = new_history;
        self.session_cache_key = Uuid::new_v4().to_string();
        if let Err(e) = self.session.rewrite(&self.history[1..]) {
            eprintln!("{} could not rewrite session: {e}", "⚠️ ".yellow());
        }
        println!(
            "{} Compacted {} earlier messages into a summary ({} recent kept).",
            "🗜️ ".bright_green(),
            summarised.to_string().bright_magenta().bold(),
            kept
        );
    }

    /// Auto-compact a goal task's isolated history past the token threshold. Mirrors
    /// [`maybe_auto_compact`] but rewrites the task's own session and rotates the
    /// task's cache lane. Quiet on a no-op.
    pub(crate) async fn maybe_auto_compact_task(&mut self, task: &mut crate::goal::GoalTask) {
        if self.last_input_tokens == 0 {
            return;
        }
        let threshold = self.auto_compact_threshold().await;
        if threshold.tokens == 0 || self.last_input_tokens < threshold.tokens {
            return;
        }
        println!(
            "{} Goal context at {} tokens ≥ {} — auto-compacting…",
            "🗜️ ".bright_cyan(),
            ikode::util::comma(self.last_input_tokens),
            Self::describe_auto_compact_rule(&threshold)
        );
        if let Some(new_history) = self.compact_messages(&task.history).await {
            task.history = new_history;
            task.cache_key = Uuid::new_v4().to_string();
            let sess = session::Session::new(&self.working_directory, task.session_id.clone());
            if let Err(e) = sess.rewrite(&task.history[1..]) {
                eprintln!("{} could not rewrite goal session: {e}", "⚠️ ".yellow());
            }
            self.last_input_tokens = 0;
            println!("{} Goal context compacted.", "🗜️ ".bright_green());
        }
    }

    /// Summarise the older portion of `history` into one briefing note and return a
    /// rebuilt history: `[system, summary, …recent tail]`. The split is the latest
    /// `user` turn before the recent window so the kept tail never starts with an
    /// orphaned tool result. Returns `None` when there isn't enough to compact, the
    /// transcript is empty, or the summary call fails/cancels (failures print). Pure
    /// w.r.t. `self` — the caller assigns the result to the right history and session.
    async fn compact_messages(&self, history: &[GaiseMessage]) -> Option<Vec<GaiseMessage>> {
        const KEEP_RECENT: usize = 6;
        let target = history.len().saturating_sub(KEEP_RECENT);
        let split = (1..target).rev().find(|&i| history[i].role == "user")?;

        let mut transcript = String::new();
        for m in &history[1..split] {
            if let Some(text) = session::message_text(m) {
                transcript.push_str(&format!("{}: {}\n", m.role, text));
            } else if m.tool_calls.is_some() {
                transcript.push_str(&format!("{}: [made tool calls]\n", m.role));
            }
        }
        if transcript.trim().is_empty() {
            return None;
        }

        let req = GaiseInstructRequest {
            input: OneOrMany::Many(vec![GaiseMessage {
                role: "user".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text {
                    text: format!(
                        "Summarise the conversation below into a compact briefing that lets an \
                         assistant continue seamlessly. Preserve key decisions, file changes, \
                         current goals, and unresolved threads; drop pleasantries. Use terse \
                         bullet points.\n\n---\n{transcript}"
                    ),
                })),
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
            }]),
            model: self.summary_model.clone(),
            ..Default::default()
        };

        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("{spinner:.green} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        pb.set_message("Compacting conversation...");
        pb.enable_steady_tick(Duration::from_millis(100));
        let result = self.cancellable(self.client.instruct(&req)).await;
        pb.finish_and_clear();

        let summary = match result {
            Some(Ok(resp)) => match resp.output {
                OneOrMany::One(m) => session::message_text(&m),
                OneOrMany::Many(ms) => ms.first().and_then(session::message_text),
            },
            Some(Err(e)) => {
                println!("{} Compaction failed: {}", "⚠️ ".bright_yellow(), e);
                return None;
            }
            None => return None, // cancelled — message already printed
        };
        let summary = match summary {
            Some(s) if !s.trim().is_empty() => s,
            _ => {
                println!("{} Compaction produced no summary.", "⚠️ ".bright_yellow());
                return None;
            }
        };

        let kept = history.len() - split;
        let mut new_history = Vec::with_capacity(kept + 2);
        new_history.push(self.system_message());
        new_history.push(GaiseMessage {
            role: "system".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text {
                text: format!("Summary of earlier conversation (compacted):\n{summary}"),
            })),
            tool_calls: None,
            tool_call_id: None,
            tool_name: None,
        });
        new_history.extend_from_slice(&history[split..]);
        Some(new_history)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use gaise_core::contracts::{
        GaiseEmbeddingsRequest, GaiseEmbeddingsResponse, GaiseInstructResponse,
        GaiseInstructStreamResponse,
    };
    use gaise_core::GaiseClient;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    #[test]
    fn parses_compact_auto_arguments() {
        use CompactAutoChange::*;
        assert_eq!(parse_compact_auto_arg("80%"), Some(Percent(80)));
        assert_eq!(parse_compact_auto_arg(" 65 % "), Some(Percent(65)));
        assert_eq!(parse_compact_auto_arg("100%"), Some(Percent(100)));
        assert_eq!(parse_compact_auto_arg("0%"), Some(Off));
        assert_eq!(parse_compact_auto_arg("101%"), None);
        assert_eq!(parse_compact_auto_arg("200000"), Some(Tokens(200_000)));
        assert_eq!(parse_compact_auto_arg("200_000"), Some(Tokens(200_000)));
        assert_eq!(parse_compact_auto_arg("200,000"), Some(Tokens(200_000)));
        assert_eq!(parse_compact_auto_arg("200k"), Some(Tokens(200_000)));
        assert_eq!(parse_compact_auto_arg("0"), Some(Off));
        assert_eq!(parse_compact_auto_arg("off"), Some(Off));
        assert_eq!(parse_compact_auto_arg("Default"), Some(Default));
        assert_eq!(parse_compact_auto_arg("reset"), Some(Default));
        assert_eq!(parse_compact_auto_arg("lots"), None);
        assert_eq!(parse_compact_auto_arg(""), None);
    }

    /// Serves a canned compaction summary and counts how often it was asked.
    struct SummaryClient {
        calls: Mutex<usize>,
    }

    #[async_trait]
    impl GaiseClient for SummaryClient {
        async fn instruct_stream(
            &self,
            _request: &GaiseInstructRequest,
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
            Err("not used".into())
        }

        async fn instruct(
            &self,
            _request: &GaiseInstructRequest,
        ) -> std::result::Result<GaiseInstructResponse, Box<dyn std::error::Error + Send + Sync>>
        {
            *self.calls.lock().unwrap() += 1;
            Ok(GaiseInstructResponse {
                output: OneOrMany::One(GaiseMessage {
                    role: "assistant".to_string(),
                    content: Some(OneOrMany::One(GaiseContent::Text {
                        text: "briefing".to_string(),
                    })),
                    tool_calls: None,
                    tool_call_id: None,
                    tool_name: None,
                }),
                external_id: None,
                usage: None,
            })
        }

        async fn embeddings(
            &self,
            _request: &GaiseEmbeddingsRequest,
        ) -> std::result::Result<GaiseEmbeddingsResponse, Box<dyn std::error::Error + Send + Sync>>
        {
            Err("not used".into())
        }
    }

    fn app(root: &std::path::Path, model: &str) -> (App, Arc<SummaryClient>) {
        let client = Arc::new(SummaryClient {
            calls: Mutex::new(0),
        });
        let mut app = App::new(
            model.to_string(),
            "openai::fake-embedding".to_string(),
            "openai::fake-summary".to_string(),
            false,
            None,
            100,
            3,
            root.to_path_buf(),
            false,
            ikode::settings::LocalSettings::default(),
            None,
        )
        .unwrap();
        app.client = client.clone();
        // Enough turns that compact_messages has something to summarise.
        for i in 0..10 {
            app.record(crate::turn::user_message(&format!("question {i}")));
            app.record(GaiseMessage {
                role: "assistant".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text {
                    text: format!("answer {i}"),
                })),
                tool_calls: None,
                tool_call_id: None,
                tool_name: None,
            });
        }
        (app, client)
    }

    #[tokio::test]
    async fn threshold_is_a_share_of_the_registry_context_window() {
        let root = tempfile::tempdir().unwrap();
        // gpt-5.4-mini has a 400k window in the bundled registry → 80% = 320k.
        let (mut app, client) = app(root.path(), "openai::gpt-5.4-mini");
        let threshold = app.auto_compact_threshold().await;
        assert_eq!(threshold.tokens, 320_000);
        assert_eq!(
            threshold.rule,
            AutoCompactRule::Percent {
                percent: 80,
                window: 400_000
            }
        );
        assert_eq!(app.context_windows.get("openai::gpt-5.4-mini"), Some(&Some(400_000)));

        // Below the line: nothing happens.
        let before = app.history.len();
        app.last_input_tokens = 319_999;
        app.maybe_auto_compact().await;
        assert_eq!(app.history.len(), before);
        assert_eq!(*client.calls.lock().unwrap(), 0);

        // At the line: compacts, resets the gauge.
        app.last_input_tokens = 320_000;
        app.maybe_auto_compact().await;
        assert!(app.history.len() < before);
        assert_eq!(*client.calls.lock().unwrap(), 1);
        assert_eq!(app.last_input_tokens, 0);

        // A user percentage re-derives from the cached window.
        app.settings.auto_compact_percent = Some(50);
        assert_eq!(app.auto_compact_threshold().await.tokens, 200_000);
    }

    #[tokio::test]
    async fn unknown_window_falls_back_to_the_absolute_default() {
        let root = tempfile::tempdir().unwrap();
        // Not in the registry, and the fake client's list_models is unsupported.
        let (mut app, _) = app(root.path(), "openai::made-up-model");
        let threshold = app.auto_compact_threshold().await;
        assert_eq!(threshold.rule, AutoCompactRule::Fallback);
        assert_eq!(threshold.tokens, ikode::settings::DEFAULT_AUTO_COMPACT_TOKENS);
        assert_eq!(app.context_windows.get("openai::made-up-model"), Some(&None));

        // `/compact auto 150k` persists an absolute override that wins.
        app.run_compact_auto("150k").await;
        assert_eq!(app.settings.auto_compact_tokens, Some(150_000));
        assert_eq!(app.auto_compact_threshold().await.tokens, 150_000);
        let saved = ikode::settings::LocalSettings::load(&app.working_directory);
        assert_eq!(saved.auto_compact_tokens, Some(150_000));

        // `/compact auto 70%` switches back to the percentage rule and clears it.
        app.run_compact_auto("70%").await;
        assert_eq!(app.settings.auto_compact_tokens, None);
        assert_eq!(app.settings.auto_compact_percent, Some(70));

        // `/compact auto off` disables; `default` restores the built-in policy.
        app.run_compact_auto("off").await;
        assert_eq!(app.auto_compact_threshold().await.rule, AutoCompactRule::Disabled);
        app.run_compact_auto("default").await;
        assert_eq!(app.settings.auto_compact_percent, None);
        assert_eq!(app.settings.auto_compact_tokens, None);
    }
}
