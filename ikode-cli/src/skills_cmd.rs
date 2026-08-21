//! The `/skills` command family: list, add/edit/remove (plain markdown files under
//! `.ikode/skills/`), and invoke — running a skill's instruction body as a normal
//! agent turn. The skill store itself lives in [`ikode::skills`]; this is the
//! interactive shell over it.

use std::path::Path;
use std::process::Command;

use anyhow::Result;
use colored::*;

use crate::app::App;
use ikode::skills;

impl App {
    /// `/skills [<name> | add|edit|remove <name>]` — manage the project's skills,
    /// markdown instruction snippets under `.ikode/skills/`. With no argument it
    /// lists them; the management verbs add/edit/remove operate on a file; anything
    /// else is treated as a skill name and run immediately (Claude-style), so
    /// `/skills review-pr` invokes the `review-pr` skill.
    pub(crate) async fn run_skills(&mut self, args: &str) -> Result<()> {
        let mut parts = args.splitn(2, char::is_whitespace);
        let first = parts.next().unwrap_or("").trim();
        let rest = parts.next().unwrap_or("").trim();

        match first {
            "" | "list" => self.skills_list(),
            "add" | "new" | "create" => self.skills_add(rest),
            "edit" => self.skills_edit(rest),
            "remove" | "rm" | "delete" => self.skills_remove(rest),
            // Explicit verb still accepted, but `/skills <name>` is the primary form.
            "invoke" | "run" | "use" => return self.skills_invoke(rest).await,
            name => return self.skills_invoke(name).await,
        }
        Ok(())
    }

    fn skills_list(&self) {
        let skills = skills::list(&self.working_directory);
        if skills.is_empty() {
            println!(
                "{} No skills yet. Add one with {} — they live in {}.",
                "•".dimmed(),
                "/skills add <name>".cyan(),
                skills::skills_dir(&self.working_directory)
                    .display()
                    .to_string()
                    .dimmed()
            );
            return;
        }
        println!("{}", "\nAvailable skills:".bright_green().bold());
        let width = skills.iter().map(|s| s.name.len()).max().unwrap_or(0);
        for s in &skills {
            println!(
                "  {:<width$}  {}",
                s.name.cyan(),
                s.description.dimmed(),
                width = width
            );
        }
        println!(
            "\n{} {} to run one.\n",
            "•".dimmed(),
            "/skills <name>".cyan()
        );
    }

    fn skills_add(&self, name: &str) {
        if name.is_empty() {
            println!("{} Usage: /skills add <name>", "⚠️ ".bright_yellow());
            return;
        }
        if !skills::valid_name(name) {
            println!(
                "{} Invalid skill name '{}' (no path separators, '..', or leading dots).",
                "⚠️ ".bright_yellow(),
                name
            );
            return;
        }
        match skills::create(&self.working_directory, name) {
            Ok(path) => {
                println!("{} Created skill {}", "✨ ".bright_green(), name.cyan());
                self.open_in_editor(&path);
            }
            Err(e) => println!("{} Could not add skill: {e}", "⚠️ ".bright_yellow()),
        }
    }

    fn skills_edit(&self, name: &str) {
        if name.is_empty() {
            println!("{} Usage: /skills edit <name>", "⚠️ ".bright_yellow());
            return;
        }
        match skills::load(&self.working_directory, name) {
            Some(skill) => self.open_in_editor(&skill.path),
            None => println!("{} No skill named '{}'.", "⚠️ ".bright_yellow(), name),
        }
    }

    fn skills_remove(&self, name: &str) {
        if name.is_empty() {
            println!("{} Usage: /skills remove <name>", "⚠️ ".bright_yellow());
            return;
        }
        if skills::load(&self.working_directory, name).is_none() {
            println!("{} No skill named '{}'.", "⚠️ ".bright_yellow(), name);
            return;
        }
        if !self.ask_permission_sync(&format!("Delete skill '{name}'"), false) {
            println!("{} Cancelled.", "•".dimmed());
            return;
        }
        match skills::remove(&self.working_directory, name) {
            Ok(path) => println!(
                "{} Removed {}",
                "🗑️ ".bright_yellow(),
                path.display().to_string().dimmed()
            ),
            Err(e) => println!("{} Could not remove skill: {e}", "⚠️ ".bright_yellow()),
        }
    }

    /// Run a skill: feed its instruction body to the model as a normal turn, so the
    /// agent acts on it exactly as if the user had typed those instructions.
    async fn skills_invoke(&mut self, name: &str) -> Result<()> {
        if name.is_empty() {
            println!("{} Usage: /skills invoke <name>", "⚠️ ".bright_yellow());
            return Ok(());
        }
        let skill = match skills::load(&self.working_directory, name) {
            Some(s) => s,
            None => {
                println!("{} No skill named '{}'.", "⚠️ ".bright_yellow(), name);
                return Ok(());
            }
        };
        if skill.body.is_empty() {
            println!(
                "{} Skill '{}' has no instructions.",
                "⚠️ ".bright_yellow(),
                skill.name
            );
            return Ok(());
        }
        println!(
            "{} Invoking skill {}",
            "▶️ ".bright_cyan(),
            skill.name.cyan()
        );
        let prompt = format!(
            "You are running the \"{}\" skill. Follow these instructions:\n\n{}",
            skill.name, skill.body
        );
        self.process_prompt(&prompt).await
    }

    /// Open `path` in the user's editor ($VISUAL / $EDITOR, falling back to notepad
    /// on Windows and vi elsewhere). Blocks until the editor exits.
    fn open_in_editor(&self, path: &Path) {
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| {
                if cfg!(windows) {
                    "notepad".to_string()
                } else {
                    "vi".to_string()
                }
            });
        println!(
            "{} Opening {} in {}…",
            "✏️ ".bright_blue(),
            path.display().to_string().dimmed(),
            editor.dimmed()
        );
        match Command::new(&editor).arg(path).status() {
            Ok(_) => {}
            Err(e) => println!(
                "{} Could not launch '{}' ({e}). Edit {} directly.",
                "⚠️ ".bright_yellow(),
                editor,
                path.display()
            ),
        }
    }
}
