# iKode

[![Rust](https://img.shields.io/badge/rust-1.91%2B-orange.svg)](https://www.rust-lang.org)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL%20v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)

iKode is a Rust-based AI coding assistant and generative AI service. It provides a powerful CLI for development tasks and a flexible backend (GAISe) that integrates with multiple AI providers.

Written by: Ian Knowles

## Project Structure

- **`ikode-cli/`**: The main command-line interface. It acts as a coding agent that can read/edit files, execute commands, and manage tasks.
- **`gaise/`**: Generative AI Service (GAISe) - a unified interface for AI providers:
    - `gaise-core`: Shared traits and contracts.
    - `gaise-client`: Easy-to-use client for interacting with GAISe.
    - `gaise-provider-*`: Implementations for OpenAI, Ollama, Vertex AI, and AWS Bedrock.

## iKode CLI Features

- 🤖 **Multi-Model Support**: Use OpenAI (GPT-4o, etc.), Ollama, Vertex AI, or Bedrock.
- 📁 **File Operations**: The agent can read and edit files in your workspace.
- 🐚 **Command Execution**: Run shell commands with optional user confirmation.
- 📝 **Todo Management**: Built-in todo list to keep track of agent goals.
- 📚 **Context Aware**: Automatically includes OS information, working directory, and user guidelines in the system prompt.

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
- `/clear`: Reset the conversation history.
- `/exit`: Quit the interactive session.

### Direct Prompt
```bash
ikode --prompt "Refactor src/main.rs to use a more efficient algorithm"
```

### Custom Model
```bash
ikode --model "ollama::llama3"
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
export OPENAI_API_KEY="your-key"
export OLLAMA_URL="http://localhost:11434"
# For Vertex AI:
export VERTEXAI_API_URL="..."
export VERTEXAI_SA_PATH="/path/to/service-account.json"
```

Use the `--brave` flag to allow the agent to execute commands and edit files without asking for confirmation (use with caution!).

## Contributing

Contributions are welcome! Please see the individual module READMEs for more details on development.

## License

AGPLv3