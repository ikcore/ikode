# iKode

[![Rust](https://img.shields.io/badge/rust-1.91%2B-orange.svg)](https://www.rust-lang.org)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)

iKode is a Rust-based AI coding assistant and generative AI service. It provides a powerful CLI for development tasks and a flexible backend (GAISe) that integrates with multiple AI providers.

Written by: Ian Knowles

Project page: [BadAI Project Page](https://badai.company/open-source/ikode)

## Project Structure

- **`ikode-cli/`**: The main command-line interface. It acts as a coding agent that can read/edit files, execute commands, and manage tasks.
- **`gaise/`**: Generative AI Service (GAISe) - a unified interface for AI providers:
    - `gaise-core`: Shared traits and contracts.
    - `gaise-client`: Easy-to-use client for interacting with GAISe.
    - `gaise-provider-*`: Implementations for OpenAI, Anthropic, Ollama, Vertex AI, and AWS Bedrock.

## iKode CLI Features

- 🤖 **Multi-Model Support**: Use OpenAI (GPT-4o, etc.), Anthropic (Claude), Ollama, Vertex AI, or Bedrock.
- 📁 **File Operations**: Read files with line numbers and line ranges, edit files with surgical search-and-replace, create new files.
- 🐚 **Command Execution**: Run shell commands with optional user confirmation.
- 📝 **Todo Management**: Built-in todo list to keep track of agent goals.
- 📚 **Context Aware**: Automatically includes OS information, working directory, and user guidelines in the system prompt.
- ⚡ **Token Efficient**: Patch-based edits, file read caps (2000 lines / 10 MB), and cache-friendly history truncation keep costs low.

## Installation

Ensure you have [Rust](https://rustup.rs/) installed.

```bash
cargo install --path ikode-cli
```

## Usage

### Interactive Mode
Start a chat session with the default model (GPT-4o):
```bash
ikode
```

During the interactive session, you can use the following commands:
- `/help`: Display a list of available commands and their descriptions.
- `/model`: Display the current model being used.
- `/model {model_name}`: Switch to a different model (e.g., `/model ollama::llama3`).
- `/history`: Show history truncation settings and message count.
- `/max-history {n}`: Set max history messages per request (0 = unlimited).
- `/prefix-keep {n}`: Set number of early messages to always keep for cache stability.
- `/clear`: Reset the conversation history.
- `/cls`: Clear the terminal screen.
- `/exit`: Quit the interactive session.

### Direct Prompt
```bash
ikode --prompt "Refactor src/main.rs to use a more efficient algorithm"
```

### Custom Model
```bash
# OpenAI (GPT-4o, GPT-4, o1, etc.)
ikode --model "openai::gpt-4o"

# Anthropic (Claude models)
ikode --model "anthropic::claude-3-5-sonnet-20241022"

# Ollama (local models)
ikode --model "ollama::llama3"

# Vertex AI (Gemini models)
ikode --model "vertexai::gemini-1.5-pro"

# AWS Bedrock (Claude, etc.)
ikode --model "bedrock::anthropic.claude-3-5-sonnet-20241022-v2:0"
```

### User Guidelines
You can provide custom instructions or project context to the agent in two ways:
1. **`ikode.md`**: If this file exists in your current directory, it will be automatically loaded as project guidelines.
2. **`--guide` flag**: Specify a custom path to a guidelines file.
   ```bash
   ikode --guide docs/internal-standards.md
   ```

## Configuration

Set the necessary environment variables for your chosen providers:

```bash
# OpenAI
export OPENAI_API_KEY="your-openai-key"

# Anthropic
export ANTHROPIC_API_KEY="your-anthropic-key"

# Ollama (local)
export OLLAMA_URL="http://localhost:11434"

# Vertex AI
export VERTEXAI_API_URL="your-vertexai-url"
export VERTEXAI_SA_PATH="/path/to/service-account.json"

# AWS Bedrock
export AWS_REGION="us-east-1"
```

### Options

```bash
# Brave mode - skip confirmation prompts (use with caution!)
ikode --brave

# Control history truncation
ikode --max-history 120        # Max messages per request (default: 80, 0 = unlimited)
ikode --prefix-keep 6          # Early messages to always keep (default: 4)
```

## Contributing

Contributions are welcome! Please see the individual module READMEs for more details on development.

## License

AGPLv3
