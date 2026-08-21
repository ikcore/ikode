//! iKode CLI entry point. This file owns only the process boundary: command-line
//! parsing (clap), provider/model resolution, optional session resume, and the
//! hand-off to either a one-shot prompt or the interactive REPL.
//!
//! Everything else is split across sibling modules that each add behaviour to the
//! [`App`] aggregate defined in [`app`]:
//! - [`app`] — `App` + construction, history/session plumbing, permissions,
//!   and the `ToolHost` implementation.
//! - [`repl`] — the interactive loop and slash-command dispatch.
//! - [`turn`] — the agent turn (streaming, tool calls, cancellation).
//! - [`passes`] — index/enrich/summarise passes, `/compact`, startup sync.
//! - [`skills_cmd`] — the `/skills` command family.
//! - [`palette`] / [`keyread`] — the framed prompt editor and raw key reader.
//! - [`session`] — on-disk transcript persistence.
//!
//! Pure, App-free helpers live in the library at [`ikode::util`] so the
//! integration tests under `tests/` can exercise them directly.

use clap::{builder::styling, Parser, Subcommand};
use colored::*;
use std::path::PathBuf;

mod agent;
mod app;
mod attach;
mod clipboard;
mod document;
mod goal;
mod image;
mod keyread;
mod palette;
mod passes;
mod repl;
mod session;
mod skills_cmd;
mod turn;
mod visualize;

use app::App;
use ikode::harness::{self, ProjectConfig};
use ikode::settings::{Effort, LocalSettings, Mode};

const STYLES: styling::Styles = styling::Styles::styled()
    .header(styling::AnsiColor::Green.on_default().bold())
    .usage(styling::AnsiColor::Green.on_default().bold())
    .literal(styling::AnsiColor::Cyan.on_default().bold())
    .placeholder(styling::AnsiColor::Cyan.on_default());

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "ikode: A CLI coding agent",
    long_about = "A powerful CLI coding agent that indexes code and markdown, manages todos, and executes commands.",
    styles = STYLES
)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(short, long, help = "The prompt to process")]
    prompt: Option<String>,

    #[arg(
        short = 'i',
        long = "image",
        value_name = "PATH",
        action = clap::ArgAction::Append,
        help = "Attach an image to the first prompt (repeatable)"
    )]
    images: Vec<String>,

    #[arg(
        short = 'a',
        long = "attach",
        value_name = "PATH",
        action = clap::ArgAction::Append,
        help = "Attach an image or document to the first prompt (repeatable)"
    )]
    attachments: Vec<String>,

    #[arg(
        short,
        long,
        help = "The chat model to use (overrides config; default openai::gpt-5.6-luna)"
    )]
    model: Option<String>,

    #[arg(
        long,
        value_name = "LEVEL",
        help = "Reasoning effort for this session: auto, low, medium, high, max, or ultra"
    )]
    effort: Option<String>,

    #[arg(
        short = 'e',
        long = "emodel",
        help = "The embedding model to use (overrides config; default openai::text-embedding-3-small)"
    )]
    embedding_model: Option<String>,

    #[arg(
        short = 's',
        long = "smodel",
        help = "The summary model to use for summarisation passes (overrides config; default openai::chat-5.6-luna)"
    )]
    summary_model: Option<String>,

    #[arg(
        short,
        long,
        default_value_t = false,
        help = "Whether to use brave mode (no confirmation for commands)"
    )]
    brave: bool,

    #[arg(short, long, help = "Path to a guide file")]
    guide: Option<String>,

    #[arg(
        long,
        help = "Maximum number of history messages sent per request (overrides config; default 80)"
    )]
    max_history: Option<usize>,

    #[arg(
        long,
        help = "Number of early messages to always keep for cache stability (overrides config; default 4)"
    )]
    prefix_keep: Option<usize>,

    #[arg(
        long = "continue",
        default_value_t = false,
        help = "Resume the most recent session in this project"
    )]
    resume_latest: bool,

    #[arg(
        long,
        default_value_t = false,
        help = "Skip configured startup indexing and the interactive startup sync check"
    )]
    no_index: bool,

    #[arg(
        long,
        default_value_t = false,
        help = "Disable graph/index retrieval and run as a traditional file/shell harness"
    )]
    no_graph: bool,

    #[arg(
        long,
        value_name = "ID",
        help = "Resume a session by id (or unique prefix)"
    )]
    resume: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Scaffold the .ikode/ project folder (ikode.md, config.toml, .gitignore entries)
    Init,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args()
        .map(|arg| {
            if arg.starts_with('—') {
                let suffix = &arg['—'.len_utf8()..];
                if suffix.chars().count() == 1 {
                    format!("-{}", suffix)
                } else {
                    format!("--{}", suffix)
                }
            } else {
                arg
            }
        })
        .collect();

    let args = Args::parse_from(args);

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_root = harness::find_project_root(&cwd);

    if let Some(Commands::Init) = args.command {
        return harness::run_init(&project_root);
    }

    // Load `.env` (and `.ikode/.env`) into the environment before any provider keys
    // are read in `App::new`. Files under `.ikode/` are explicit project config and
    // override inherited environment variables; root-level `.env`/`.ikenv` never
    // clobber the real environment. Only filenames are printed — never values.
    let loaded_env = harness::load_dotenv(&project_root);
    if !loaded_env.is_empty() {
        println!(
            "{} Loaded environment from {}",
            "🔑".bright_blue(),
            loaded_env.join(", ")
        );
    }

    // Layered model/setting resolution, highest precedence first:
    //   CLI flags > .ikode/settings.local.json > .ikode/config.toml > global > default.
    let config = ProjectConfig::resolve(&project_root);
    let mut settings = LocalSettings::load(&project_root);
    let local_chat = settings.models.as_ref().and_then(|m| m.chat.clone());
    let local_embed = settings.models.as_ref().and_then(|m| m.embedding.clone());
    let local_summary = settings.models.as_ref().and_then(|m| m.summary.clone());
    let model = args
        .model
        .or(local_chat)
        .or(config.model)
        .unwrap_or_else(|| "openai::gpt-5.6-luna".to_string());
    let embedding_model = args
        .embedding_model
        .or(local_embed)
        .or(config.embedding_model)
        .unwrap_or_else(|| "openai::text-embedding-3-small".to_string());
    // Summary model defaults to chat-5.6-luna when unset.
    let summary_model = args
        .summary_model
        .or(local_summary)
        .or(config.summary_model)
        .unwrap_or_else(|| "openai::chat-5.6-luna".to_string());
    let brave = args.brave || config.brave.unwrap_or(false);
    // `--brave` is a session-level "yolo" switch: it forces the mode regardless of
    // what the local settings file says, without persisting the change.
    if brave {
        settings.mode = Mode::Yolo;
    }
    if let Some(effort) = parse_effort_override(args.effort.as_deref())? {
        settings.effort = effort;
    }
    let max_history = args.max_history.or(config.max_history).unwrap_or(80);
    let prefix_keep = args.prefix_keep.or(config.prefix_keep).unwrap_or(4);
    let graph_enabled = config.graph_enabled.unwrap_or(true) && !args.no_graph;
    let auto_index = graph_enabled && config.auto_index.unwrap_or(false) && !args.no_index;

    // Web research posture, decided once at startup: resolve the search backend
    // from config + environment, and scan for an installed Chromium-family
    // browser (Chrome/Edge/Chromium/Brave — Windows, macOS, and Linux locations)
    // to power headless JavaScript rendering in web fetches.
    let browser = ikode::web::chrome::discover();
    match &browser {
        Some(path) => println!(
            "{} Chrome found: {} — JavaScript rendering enabled for web fetches.",
            "🌐".bright_cyan(),
            path.display().to_string().cyan()
        ),
        None => println!(
            "{} Chrome could not be found — web fetches will read static HTML only.",
            "🌐".bright_yellow()
        ),
    }
    // Fetching needs no credential, so web research is always available; a
    // missing search backend only degrades it to fetch-only (explicit URLs).
    let env_lookup = |name: &str| std::env::var(name).ok();
    let backend = ikode::web::SearchBackend::resolve(
        config.web_search_backend.as_deref(),
        config.web_search_endpoint.as_deref(),
        &env_lookup,
    );
    let web = ikode::web::WebConfig {
        backend: backend.as_ref().ok().cloned(),
        browser,
        max_searches: config
            .web_max_searches
            .unwrap_or(ikode::web::DEFAULT_MAX_SEARCHES),
        max_fetches: config
            .web_max_fetches
            .unwrap_or(ikode::web::DEFAULT_MAX_FETCHES),
    };
    match backend {
        Ok(_) => println!(
            "{} Web research enabled ({}).",
            "🔎".bright_cyan(),
            web.describe()
        ),
        Err(reason) => println!(
            "{} Web research: fetch-only — pages at explicit URLs can be read, but searching is off ({reason}).",
            "🔎".bright_yellow()
        ),
    }
    let web_config = Some(web);

    let mut app = App::new(
        model,
        embedding_model,
        summary_model,
        brave,
        args.guide,
        max_history,
        prefix_keep,
        project_root,
        graph_enabled,
        settings,
        web_config,
    )?;

    app.reload_mcp().await;
    app.print_mcp_startup_status();

    for path in &args.images {
        let image = image::load_image(path, &app.working_directory)
            .map_err(|error| anyhow::anyhow!("--image {path}: {error}"))?;
        app.pending_images.push(image);
    }
    for path in &args.attachments {
        match image::load_image(path, &app.working_directory) {
            Ok(image) => app.pending_images.push(image),
            Err(error) if image::has_supported_extension(path) => {
                anyhow::bail!("--attach {path}: {error}");
            }
            Err(_) => {
                let document = document::load_document(path, &app.working_directory)
                    .map_err(|error| anyhow::anyhow!("--attach {path}: {error}"))?;
                if let Some(error) = document.support_error(&app.model) {
                    anyhow::bail!("--attach {path}: {error}");
                }
                app.pending_documents.push(document);
            }
        }
    }

    // Resume a prior session if requested (--continue = latest, --resume <id> = specific).
    let resume_dir = app.working_directory.clone();
    if let Some(id) = args.resume.as_ref() {
        let infos = session::list_sessions(&resume_dir);
        match session::match_session(&infos, id) {
            session::SessionMatch::Unique(info) => app.load_session(info)?,
            session::SessionMatch::None => {
                anyhow::bail!("no session matching '{id}'")
            }
            session::SessionMatch::Ambiguous(matches) => {
                let ids = matches
                    .iter()
                    .map(|info| info.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                anyhow::bail!("session prefix '{id}' is ambiguous; matches: {ids}")
            }
        }
    } else if args.resume_latest {
        match session::list_sessions(&resume_dir).first() {
            Some(info) => app.load_session(info)?,
            None => println!("{} No session to continue.", "•".dimmed()),
        }
    }

    if auto_index {
        app.run_index();
    }

    if let Some(prompt) = args.prompt {
        app.process_prompt(&prompt).await?;
    } else {
        app.run_loop(auto_index || args.no_index || !graph_enabled)
            .await?;
    }

    Ok(())
}

fn parse_effort_override(raw: Option<&str>) -> anyhow::Result<Option<Effort>> {
    raw.map(|value| {
        Effort::parse(value).ok_or_else(|| {
            anyhow::anyhow!("invalid effort '{value}'; expected auto|low|medium|high|max|ultra")
        })
    })
    .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_effort_flag_reaches_the_validated_override() {
        let args = Args::try_parse_from(["ikode", "--effort", "med"]).unwrap();
        assert_eq!(
            parse_effort_override(args.effort.as_deref()).unwrap(),
            Some(Effort::Medium)
        );
        assert_eq!(parse_effort_override(None).unwrap(), None);
        assert_eq!(
            parse_effort_override(Some("ULTRA")).unwrap(),
            Some(Effort::Ultra)
        );
    }

    #[test]
    fn cli_accepts_repeatable_image_and_generic_attachment_paths() {
        let args = Args::try_parse_from([
            "ikode",
            "--image",
            "one.png",
            "-i",
            "two.jpg",
            "--attach",
            "report.pdf",
            "-a",
            "notes.md",
            "--prompt",
            "review these",
        ])
        .unwrap();

        assert_eq!(args.images, ["one.png", "two.jpg"]);
        assert_eq!(args.attachments, ["report.pdf", "notes.md"]);
        assert_eq!(args.prompt.as_deref(), Some("review these"));
    }

    #[test]
    fn cli_effort_override_rejects_unknown_or_missing_values() {
        let error = parse_effort_override(Some("warp-speed")).unwrap_err();
        assert!(error
            .to_string()
            .contains("expected auto|low|medium|high|max|ultra"));
        assert!(Args::try_parse_from(["ikode", "--effort"]).is_err());
    }

    #[test]
    fn cli_accepts_session_only_graph_disable_override() {
        let args = Args::try_parse_from(["ikode", "--no-graph"]).unwrap();
        assert!(args.no_graph);
    }
}
