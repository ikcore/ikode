# iKode CLI User Instructions

iKode is a powerful AI-driven coding assistant that helps you manage your development tasks directly from your terminal. It can read and edit files, execute shell commands, and maintain a task list to help you stay focused on your goals.

## Table of Contents
- [Installation](#installation)
- [Getting Started](#getting-started)
- [Usage Modes](#usage-modes)
- [Core Features](#core-features)
- [Configuration](#configuration)
- [User Guidelines](#user-guidelines)
- [Safety and Permissions](#safety-and-permissions)

---

## Installation

Ensure you have [Rust and Cargo](https://rustup.rs/) installed on your system.

To install iKode CLI from the source:
```bash
cargo install --path ikode-cli
```

## Getting Started

Before running iKode, you need to configure at least one AI provider. The simplest way is to use OpenAI:

```bash
export OPENAI_API_KEY="your-api-key-here"
```

Once configured, you can start iKode by simply typing:
```bash
ikode
```

## Usage Modes

### 1. Interactive Mode (Default)
Running `ikode` without arguments starts an interactive session.
- Type your requests in plain English.
- Use `/help` to see all available commands.
- Use `/model` to output the current model.
- Use `/model { model_name }` to change the model.
- Use `/history` to view history settings and message count.
- Use `/max-history { n }` to change the max history messages sent per request (0 = unlimited).
- Use `/prefix-keep { n }` to change how many early messages are always kept for cache stability.
- Use `/clear` to reset the conversation history.
- Use `/cls` to clear the terminal screen.
- Use `/exit` to leave the session.

### 2. Direct Prompt Mode
You can ask iKode to perform a specific task and exit immediately using the `--prompt` (or `-p`) flag:
```bash
ikode --prompt "Check if there are any TODOs in src/main.rs"
```

## Core Features

iKode is not just a chatbot; it's an agent capable of performing actions:

- **File Operations**: It can `read_file` to understand your code (with line numbers, line ranges, and a 2000-line default cap), `edit_file` to apply surgical search-and-replace changes, and `create_file` to write new files.
- **Command Execution**: It can `execute_command` to run tests, build your project, or list directory contents.
- **Todo Management**: It maintains an internal todo list. You can ask it to "Add a task to my todo list" or "Show my todos". It uses this list to track its own progress on complex tasks.

## Configuration

iKode supports multiple AI providers via environment variables:

| Provider | Environment Variable | Example / Description |
| :--- | :--- | :--- |
| **OpenAI** | `OPENAI_API_KEY` | `sk-...` |
| | `OPENAI_API_URL` | Custom endpoint (optional) |
| **Ollama** | `OLLAMA_URL` | `http://localhost:11434` |
| **Vertex AI**| `VERTEXAI_API_URL` | Your GCP endpoint |
| | `VERTEXAI_SA_PATH` | Path to your Service Account JSON |
| **AWS Bedrock**| `AWS_REGION` | e.g., `us-east-1` |

### Selecting a Model
Use the `--model` (or `-m`) flag to switch between providers and models:
```bash
ikode --model "ollama::llama3"
ikode --model "openai::gpt-4-turbo"
```
*Default model is `openai::gpt-4o`.*

### History & Token Controls
iKode uses a cache-friendly history truncation strategy to keep token usage efficient during long sessions. You can tune this at startup:

```bash
# Set max history messages per request (default: 80, 0 = unlimited)
ikode --max-history 120

# Set number of early messages to always keep for cache stability (default: 4)
ikode --prefix-keep 6
```

These can also be changed at runtime using `/max-history` and `/prefix-keep` commands. Use `/history` to see current settings.

## User Guidelines

You can provide iKode with specific context or rules for your project:

1. **Auto-loading**: Create a file named `ikode.md` in your project root. iKode will automatically read this file and follow any instructions or style guides defined there.
2. **Manual Flag**: Use the `--guide` (or `-g`) flag to specify a different instructions file:
   ```bash
   ikode --guide docs/coding-standards.md
   ```

## Safety and Permissions

By default, iKode is "cautious". It will ask for your confirmation before:
- Editing a file.
- Executing a shell command.

### Brave Mode
If you trust the agent and want it to work autonomously without interruptions, use the `--brave` (or `-b`) flag:
```bash
ikode --brave --prompt "Fix all compiler warnings in this project"
```
**Warning:** Use Brave Mode with caution, especially on commands that delete files or perform destructive actions.
