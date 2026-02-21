use clap::{Parser, builder::styling};
use gaise_core::contracts::{
    GaiseContent, GaiseInstructRequest, GaiseMessage,
    GaiseToolCall, OneOrMany, GaiseGenerationConfig
};
use gaise_core::GaiseClient;
use gaise_client::{GaiseClientService, GaiseClientConfig};
use gaise_client::ServiceAccount;
use std::io::{self, Write};
use std::process::Command;
use std::fs;
use std::path::{Path, PathBuf};
use dialoguer::Confirm;
use anyhow::{Result, anyhow};
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;
use uuid::Uuid;

mod tools;
use tools::*;

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
    long_about = "A powerful CLI coding agent that assists with development tasks, manages todos, and executes commands.",
    styles = STYLES
)]
struct Args {
    #[arg(short, long, help = "The prompt to process")]
    prompt: Option<String>,

    #[arg(short, long, default_value = "openai::gpt-4o", help = "The model to use")]
    model: String,

    #[arg(short, long, default_value_t = false, help = "Whether to use brave mode (no confirmation for commands)")]
    brave: bool,

    #[arg(short, long, help = "Path to a guide file")]
    guide: Option<String>,
}

struct Todo {
    id: usize,
    task: String,
    completed: bool,
}

struct App {
    client: Box<dyn GaiseClient>,
    history: Vec<GaiseMessage>,
    todos: Vec<Todo>,
    model: String,
    brave: bool,
    system_prompt: String,
    session_cache_key: String,
    working_directory: PathBuf,
}

impl App {
    fn new(model: String, brave: bool, guide_path: Option<String>) -> Result<Self> {
        let mut config = GaiseClientConfig::default();

        if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
            config.openai_api_key = Some(api_key);
        }
        if let Ok(api_url) = std::env::var("OPENAI_API_URL") {
            config.openai_api_url = Some(api_url);
        }
        if let Ok(ollama_url) = std::env::var("OLLAMA_URL") {
            config.ollama_url = Some(ollama_url);
        }
        
        if let Ok(region) = std::env::var("AWS_REGION") {
            config.bedrock_region = Some(region);
        }

        if let Ok(api_url) = std::env::var("VERTEXAI_API_URL") {
            config.vertexai_api_url = Some(api_url);
        }

        if let Ok(sa_path) = std::env::var("VERTEXAI_SA_PATH") {
            if let Ok(sa_content) = std::fs::read_to_string(sa_path) {
                if let Ok(sa) = serde_json::from_str::<serde_json::Value>(&sa_content) {
                   // Map serde_json::Value to ServiceAccount if it matches
                   if let (Some(pk), Some(email)) = (sa["private_key"].as_str(), sa["client_email"].as_str()) {
                       config.vertexai_sa = Some(ServiceAccount {
                           private_key: pk.to_string(),
                           client_email: email.to_string(),
                       });
                   }
                }
            }
        }
        // VertexAI and others can be added as needed

        let client = GaiseClientService::new(config);

        let system_prompt_raw = include_str!("sys-prompt.md");
        let mut system_prompt = Self::format_system_prompt(system_prompt_raw);

        // Check for ikode.md
        if let Ok(content) = fs::read_to_string("ikode.md") {
            system_prompt.push_str("\n\nUser Project Guidelines (from ikode.md):\n");
            system_prompt.push_str(&content);
        }

        // Check for guide argument
        if let Some(path) = guide_path {
            match fs::read_to_string(&path) {
                Ok(content) => {
                    system_prompt.push_str(&format!("\n\nUser Guidelines (from {}):\n", path));
                    system_prompt.push_str(&content);
                },
                Err(e) => eprintln!("{} Warning: Could not read guide file {}: {}", "⚠️".yellow(), path, e),
            }
        }

        let working_directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        Ok(Self {
            client: Box::new(client),
            history: vec![GaiseMessage {
                role: "system".to_string(),
                content: Some(OneOrMany::One(GaiseContent::Text { text: system_prompt.clone() })),
                tool_calls: None,
                tool_call_id: None,
            }],
            todos: Vec::new(),
            model,
            brave,
            system_prompt,
            session_cache_key: Uuid::new_v4().to_string(),
            working_directory,
        })
    }

    fn format_system_prompt(raw: &str) -> String {
        let wd = std::env::current_dir().unwrap_or_default().to_string_lossy().to_string();
        let platform = std::env::consts::OS;
        let os_version = "Unknown";
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let is_git = Path::new(".git").exists();

        raw.replace("__WORKING_DIRECTORY__", &wd)
           .replace("__PLATFORM__", platform)
           .replace("__OS_VERSION__", os_version)
           .replace("__TODAY_DATE__", &today)
           .replace("__IS_GIT_REPO__", if is_git { "Yes" } else { "No" })
    }

    fn validate_path(&self, path: &str) -> Result<PathBuf> {
        let requested_path = Path::new(path);
        let canonical_path = if requested_path.is_absolute() {
            requested_path.canonicalize().unwrap_or_else(|_| requested_path.to_path_buf())
        } else {
            self.working_directory.join(requested_path)
                .canonicalize()
                .unwrap_or_else(|_| self.working_directory.join(requested_path))
        };

        if !canonical_path.starts_with(&self.working_directory) {
            return Err(anyhow!(
                "Path '{}' is outside the working directory. For security reasons, file operations are restricted to the working directory and its subdirectories.",
                path
            ));
        }

        Ok(canonical_path)
    }


    fn clear_screen() {
        if cfg!(windows) {
            let _ = Command::new("cmd").args(["/c", "cls"]).status();
        } else {
            let _ = Command::new("clear").status();
        }
    }

    async fn run_loop(&mut self) -> Result<()> {
        println!("{}", "✨ Welcome to iKode! Your AI coding assistant..".bright_cyan().bold());
        println!("{}", "Type '/help' for a list of commands, or '/exit' to quit.\n".dimmed());

        loop {
            print!("{}", "> ".bright_blue().bold());
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let input = input.trim();

            if input.is_empty() {
                continue;
            }

            if input == "/exit" {
                println!("{}", "👋 Goodbye!".bright_yellow());
                break;
            }
            if input == "/help" {
                println!("{}", "\nAvailable commands:".bright_green().bold());
                println!("  {} - Display this help message", "/help".cyan());
                println!("  {} - Display the current model", "/model".cyan());
                println!("  {} {{model}} - Switch to a different model", "/model".cyan());
                println!("  {} - Reset the conversation history", "/clear".cyan());
                println!("  {} - Clear the terminal screen", "/cls".cyan());
                println!("  {} - Quit the interactive session\n", "/exit".cyan());
                continue;
            }
            if input == "/cls" || input == "/clear_screen" {
                Self::clear_screen();
                continue;
            }
            if input == "/clear" {
                self.history = vec![GaiseMessage {
                    role: "system".to_string(),
                    content: Some(OneOrMany::One(GaiseContent::Text { text: self.system_prompt.clone() })),
                    tool_calls: None,
                    tool_call_id: None,
                }];
                self.session_cache_key = Uuid::new_v4().to_string();
                println!("{}", "🧹 History cleared.".bright_cyan());
                continue;
            }
            if input == "/model" {
                println!("{} Current model: {}", "🤖".bright_blue(), self.model.bright_magenta().bold());
                continue;
            }
            if input.starts_with("/model ") {
                let new_model = input.trim_start_matches("/model ").trim();
                if !new_model.is_empty() {
                    self.model = new_model.to_string();
                    println!("{} Model changed to: {}", "✅".bright_green(), self.model.bright_magenta().bold());
                } else {
                    println!("{} Please specify a model name. Usage: /model {{model_name}}", "⚠️".bright_yellow());
                }
                continue;
            }

            self.process_prompt(input).await?;
        }
        Ok(())
    }

    async fn process_prompt(&mut self, prompt: &str) -> Result<()> {
        self.history.push(GaiseMessage {
            role: "user".to_string(),
            content: Some(OneOrMany::One(GaiseContent::Text { text: prompt.to_string() })),
            tool_calls: None,
            tool_call_id: None,
        });

        loop {
            let mut generation_config = None;
            if self.model.starts_with("openai::gpt-5") {
                generation_config = Some(GaiseGenerationConfig {
                    cache_key: Some(self.session_cache_key.clone()),
                    ..Default::default()
                });
            }

            let request = GaiseInstructRequest {
                input: OneOrMany::Many(self.history.clone()),
                model: self.model.clone(),
                tools: Some(tools::get_tools()),
                generation_config,
                ..Default::default()
            };

            let pb = ProgressBar::new_spinner();
            pb.set_style(ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("{spinner:.green} {msg}")?);
            pb.set_message("Thinking...");
            pb.enable_steady_tick(Duration::from_millis(100));

            let response = self.client.instruct(&request).await;
            pb.finish_and_clear();
            let response = response.map_err(|e| anyhow!("{}", e))?;
            
            let assistant_messages = match response.output {
                OneOrMany::One(m) => vec![m],
                OneOrMany::Many(ms) => ms,
            };

            for assistant_message in assistant_messages {
                self.history.push(assistant_message.clone());

                if let Some(content) = &assistant_message.content {
                    match content {
                        OneOrMany::One(GaiseContent::Text { text }) => {
                            println!("{}", text);
                        }
                        OneOrMany::Many(parts) => {
                            for part in parts {
                                if let GaiseContent::Text { text } = part {
                                    println!("{}", text);
                                }
                            }
                        }
                        _ => {}
                    }
                }

                if let Some(tool_calls) = assistant_message.tool_calls {
                    for tool_call in tool_calls {
                        let result = self.handle_tool_call(&tool_call).await?;
                        self.history.push(GaiseMessage {
                            role: "tool".to_string(),
                            content: Some(OneOrMany::One(GaiseContent::Text { text: result })),
                            tool_calls: None,
                            tool_call_id: Some(tool_call.id.clone()),
                        });
                    }
                } else {
                    return Ok(());
                }
            }
        }
    }

    async fn handle_tool_call(&mut self, tool_call: &GaiseToolCall) -> Result<String> {
        let name = &tool_call.function.name;
        let arguments = &tool_call.function.arguments;

        println!("{} Calling tool: {}", "🛠️".bright_yellow(), name.bright_magenta().bold());

        match name.as_str() {
            "todo_add" => {
                let args_str = arguments.as_deref().unwrap_or("{}");
                let args: TodoAddArgs = serde_json::from_str(args_str)?;
                for task in args.tasks {
                    println!("{} Adding task: {}", "📝".bright_blue(), task.bright_blue());
                    let id = self.todos.len() + 1;
                    self.todos.push(Todo { id, task, completed: false });
                }
                Ok("Tasks added.".to_string())
            }
            "todo_insert" => {
                let args_str = arguments.as_deref().unwrap_or("{}");
                let args: TodoInsertArgs = serde_json::from_str(args_str)?;
                
                let index = self.todos.iter().position(|t| t.id == args.before_id).unwrap_or(self.todos.len());
                println!("{} Inserting task: {} before ID {}", "📝".bright_blue(), args.task.bright_blue(), args.before_id);
                
                self.todos.insert(index, Todo { id: 0, task: args.task, completed: false });
                
                // Re-ID todos
                for (i, todo) in self.todos.iter_mut().enumerate() {
                    todo.id = i + 1;
                }
                
                // Return updated list as string
                let mut list = String::new();
                for todo in &self.todos {
                    let status = if todo.completed { "✅ completed" } else { "⏳ pending" };
                    list.push_str(&format!("{}) {} ({})\n", todo.id, todo.task, status));
                }
                Ok(if list.is_empty() { "No tasks.".to_string() } else { list })
            }
            "todo_complete" => {
                let args_str = arguments.as_deref().unwrap_or("{}");
                let args: TodoCompleteArgs = serde_json::from_str(args_str)?;
                for id in args.ids {
                    if let Some(todo) = self.todos.iter_mut().find(|t| t.id == id) {
                        println!("{} Completed task: {}", "✅".bright_green(), todo.task.bright_green());
                        todo.completed = true;
                    }
                }
                Ok("Tasks marked as complete.".to_string())
            }
            "todo_list" => {
                let mut list = String::new();
                for todo in &self.todos {
                    let status = if todo.completed { "✅ completed" } else { "⏳ pending" };
                    list.push_str(&format!("{}) {} ({})\n", todo.id, todo.task, status));
                }
                if list.is_empty() {
                    Ok("No tasks.".to_string())
                } else {
                    Ok(list)
                }
            }
            "execute_command" => {
                let args_str = arguments.as_deref().unwrap_or("{}");
                let args: ExecuteCommandArgs = serde_json::from_str(args_str)?;
                println!("{} Executing: {}", "🚀".bright_magenta(), args.command.bright_magenta());

                if !self.brave {
                    let prompt = format!("{} Execute command: {}?", "❓".bright_yellow(), args.command.cyan());
                    if !Confirm::new().with_prompt(prompt).interact()? {
                        return Ok("Command cancelled by user.".to_string());
                    }
                }

                let output = if cfg!(target_os = "windows") {
                    Command::new("cmd").args(["/C", &args.command]).output()?
                } else {
                    Command::new("sh").args(["-c", &args.command]).output()?
                };

                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                Ok(format!("STDOUT:\n{}\nSTDERR:\n{}", stdout, stderr))
            }
            "read_file" => {
                let args_str = arguments.as_deref().unwrap_or("{}");
                let args: ReadFileArgs = serde_json::from_str(args_str)?;
                println!("{} Reading file: {}", "📖".bright_cyan(), args.path.bold().bright_cyan());

                match self.validate_path(&args.path) {
                    Ok(validated_path) => {
                        match fs::read_to_string(&validated_path) {
                            Ok(content) => Ok(content),
                            Err(e) => Ok(format!("Error reading file: {}", e)),
                        }
                    }
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }
            "edit_file" => {
                let args_str = arguments.as_deref().unwrap_or("{}");
                let args: EditFileArgs = serde_json::from_str(args_str)?;
                println!("{} Editing file: {}", "✍️".bright_yellow(), args.path.bold().bright_yellow());

                match self.validate_path(&args.path) {
                    Ok(validated_path) => {
                        if !self.brave {
                            let prompt = format!("{} Edit file {}?", "❓".bright_yellow(), args.path.bold().cyan());
                            if !Confirm::new().with_prompt(prompt).interact()? {
                                return Ok("File edit cancelled by user.".to_string());
                            }
                        }

                        match fs::write(&validated_path, &args.content) {
                            Ok(_) => Ok("File updated successfully.".to_string()),
                            Err(e) => Ok(format!("Error writing file: {}", e)),
                        }
                    }
                    Err(e) => Ok(format!("Error: {}", e)),
                }
            }
            _ => Ok(format!("Unknown tool: {}", name)),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args()
        .map(|arg| {
            if arg.starts_with('—') {
                let suffix = &arg['—'.len_utf8()..];
                if suffix.chars().count() == 1 {
                    // Replace em dash with single hyphen for short flags (e.g., —m -> -m)
                    format!("-{}", suffix)
                } else {
                    // Replace em dash with double hyphen for long flags or just being safe
                    format!("--{}", suffix)
                }
            } else {
                arg
            }
        })
        .collect();

    let args = Args::parse_from(args);
    let mut app = App::new(args.model, args.brave, args.guide)?;

    if let Some(prompt) = args.prompt {
        app.process_prompt(&prompt).await?;
    } else {
        app.run_loop().await?;
    }

    Ok(())
}