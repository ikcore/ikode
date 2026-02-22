# iKode - AI Coding Assistant

> **For AI Assistants (Claude, GPT, etc.)**: This file provides essential context about the iKode project to help you understand the codebase and contribute effectively.

## What is iKode?

iKode is an **AI-powered coding assistant** built in Rust that helps developers with:
- Reading and editing files
- Executing shell commands
- Managing development tasks
- Providing multi-model AI support

It consists of two main components:
1. **ikode-cli**: The command-line interface users interact with
2. **GAISe**: A unified AI service abstraction layer supporting OpenAI, Ollama, Vertex AI, and AWS Bedrock

**Author**: Ian Knowles
**License**: AGPLv3
**Current Status**: Active development, production-ready for basic operations

---

## Quick Reference

### Key Files You'll Work With

| File | Purpose | Important Notes |
|------|---------|----------------|
| `ikode-cli/src/main.rs` | Core app logic, REPL, tool handlers | Contains `App` struct, `handle_tool_call()`, path validation |
| `ikode-cli/src/tools.rs` | Tool definitions for function calling | Defines `read_file`, `edit_file`, `create_file`, `execute_command`, todo tools |
| `ikode-cli/src/sys-prompt.md` | System prompt template | **Keep concise!** Uses ~3K tokens/request |
| `ikode-cli/tests/*` | Test suite | 37+ tests covering security, tools, integration, history truncation |
| `gaise/gaise-core/` | AI service trait definitions | `GaiseClient` trait, contracts, models |
| `gaise/gaise-client/` | Provider instantiation | Client factory for all AI providers |

### Architecture Overview

```
User → ikode CLI → GAISe Client → AI Provider (OpenAI/Ollama/Vertex/Bedrock)
         ↓
    Tool Execution
    (read/write files, run commands, manage todos)
```

---

## Critical Security Constraints

### ⚠️ Path Validation (NEVER BYPASS!)

**All file operations must validate paths** to prevent path traversal attacks:

```rust
// ✅ CORRECT - Always use validate_path()
match self.validate_path(&args.path) {
    Ok(validated_path) => fs::read_to_string(&validated_path),
    Err(e) => return Err(e),
}

// ❌ WRONG - Never access paths directly
fs::read_to_string(&args.path)  // SECURITY VULNERABILITY!
```

**Why?** Without validation, malicious prompts could access:
- `/etc/passwd`
- `../../../sensitive-data.txt`
- Files outside the working directory

**Implementation**: `App::validate_path()` at line 160 in `main.rs`
- Canonicalizes paths (resolves symlinks and `..`)
- Ensures path starts with `working_directory`
- Rejects escapes via absolute paths or parent directory traversal

### Test Coverage
See `ikode-cli/tests/path_validation_tests.rs`:
- ✅ `test_rejects_parent_directory_traversal`
- ✅ `test_rejects_absolute_path_outside_working_dir`
- ✅ `test_rejects_symlink_escape`

---

## How ikode-cli Works

### 1. Initialization Flow
```rust
App::new(model, brave, guide_path)
  → Load providers (OpenAI/Ollama/Vertex/Bedrock)
  → Load system prompt from sys-prompt.md
  → Apply template replacements (__WORKING_DIRECTORY__, __TODAY_DATE__, etc.)
  → Optionally load user guidelines (ikode.md or --guide)
  → Initialize empty history and todo list
```

### 2. REPL Loop
```rust
loop {
  1. User enters prompt
  2. Build GaiseInstructRequest with:
     - History (includes system message)
     - User message
     - Available tools (read_file, edit_file, create_file, execute_command, todos)
  3. Send to AI provider via client.instruct()
  4. Handle response:
     - Text content → Display to user
     - Tool calls → Execute via handle_tool_call()
  5. Append assistant response to history
  6. If tool calls exist, send tool results back to AI
  7. Repeat until no more tool calls
}
```

### 3. Tool Execution
When AI requests a tool (via function calling):
```rust
handle_tool_call(name, arguments) {
  match name {
    "read_file" => {
      1. Parse JSON arguments (path, optional offset/limit)
      2. Validate path (SECURITY!)
      3. Check file size (10 MB cap)
      4. Read file, apply line range (default: first 2000 lines)
      5. Return numbered lines with truncation notice if needed
    }
    "edit_file" => {
      1. Parse JSON arguments (path, old_text, new_text)
      2. Validate path (SECURITY!)
      3. Find old_text (must match exactly once)
      4. Confirm with user (unless --brave)
      5. Replace old_text with new_text
      6. Return success/error
    }
    "create_file" => {
      1. Parse JSON arguments (path, content)
      2. Validate path (SECURITY!)
      3. Check file doesn't already exist
      4. Confirm with user (unless --brave)
      5. Create parent directories if needed
      6. Write file
    }
    "execute_command" => {
      1. Parse JSON arguments
      2. Confirm with user (unless --brave)
      3. Execute via Command::new()
      4. Capture stdout/stderr
      5. Return output
    }
    "todo_*" => Manage internal todo list
  }
}
```

---

## Recent Improvements (What Changed)

### 2026-02-22 Session (Token Efficiency & Quality)
1. ✅ **Patch-based edit tool** - `edit_file` now uses `old_text`/`new_text` search-and-replace instead of full file content (95%+ output token reduction on typical edits)
2. ✅ **Separate `create_file` tool** - For new files only, fails if file exists
3. ✅ **File read line ranges** - `read_file` supports `offset`/`limit` params, returns numbered lines, default cap of 2000 lines
4. ✅ **File size cap** - `read_file` rejects files over 10 MB
5. ✅ **Cache-friendly history truncation** - Configurable sliding window (`--max-history`, `--prefix-keep`) keeps system prompt + early messages stable for cache hits
6. ✅ **Runtime history controls** - `/history`, `/max-history`, `/prefix-keep` slash commands
7. ✅ **Removed iKode Code doc references** from system prompt (was a copy-paste artifact)
8. ✅ **37+ tests** covering edit, read, truncation, security

### 2026-02-21 Session
1. ✅ **Fixed hardcoded date** (was `2024-02-08`, now `chrono::Local::now()`)
2. ✅ **Implemented path validation** (prevents `/etc/passwd` access)
3. ✅ **Optimized system prompt** (32% reduction → saves 1,500-2,000 tokens/request)
4. ✅ **Added comprehensive test suite** (20+ tests in `ikode-cli/tests/`)
5. ✅ **Fixed Rust edition** (was `2024`, now `2021`)

---

## GAISe: The AI Service Layer

### Provider Architecture
GAISe abstracts multiple AI providers behind a common trait:

```rust
#[async_trait]
pub trait GaiseClient: Send + Sync {
    async fn instruct(&self, req: &GaiseInstructRequest) -> Result<GaiseInstructResponse>;
    async fn instruct_stream(&self, req: &GaiseInstructRequest) -> Result<GaiseInstructStream>;
    async fn embed(&self, req: &GaiseEmbedRequest) -> Result<GaiseEmbedResponse>;
}
```

### Supported Providers
| Provider | Module | Models |
|----------|--------|--------|
| OpenAI | `gaise-provider-openai` | GPT-4o, GPT-4, o1, o1-mini |
| Ollama | `gaise-provider-ollama` | llama3, mistral, etc. (local) |
| Vertex AI | `gaise-provider-vertexai` | Gemini 1.5, Gemini 2.0 |
| AWS Bedrock | `gaise-provider-bedrock` | Claude 3.5 Sonnet, etc. |

### Model Selection
Users specify models via `--model` flag:
```bash
ikode --model "openai::gpt-4o"
ikode --model "ollama::llama3"
ikode --model "vertexai::gemini-1.5-pro"
```

Format: `<provider>::<model_name>`

### History & Token Controls
```bash
# Set max history messages per request (default: 80, 0 = unlimited)
ikode --max-history 120

# Set number of early messages to always keep for cache stability (default: 4)
ikode --prefix-keep 6

# Runtime commands (during REPL session)
/history              # Show current settings and message count
/max-history 100      # Change max history
/prefix-keep 8        # Change prefix keep
```

---

## Common Tasks

### Adding a New Tool

1. **Define the tool** in `ikode-cli/src/tools.rs`:
```rust
pub fn get_search_files_tool() -> GaiseTool {
    GaiseTool {
        name: "search_files".to_string(),
        description: "Search for files matching a pattern".to_string(),
        parameters: /* JSON schema */,
    }
}
```

2. **Add to tool list** in `ikode-cli/src/main.rs:220`:
```rust
tools: vec![
    get_read_file_tool(),
    get_edit_file_tool(),
    get_search_files_tool(),  // NEW
    // ...
],
```

3. **Implement handler** in `ikode-cli/src/main.rs:305+`:
```rust
async fn handle_tool_call(&mut self, name: &str, arguments: &Option<String>) -> Result<String> {
    match name {
        // ... existing tools ...
        "search_files" => {
            let args: SearchFilesArgs = serde_json::from_str(args_str)?;
            // Implement search logic
            Ok(results)
        }
    }
}
```

4. **Write tests** in `ikode-cli/tests/tool_tests.rs`

### Modifying the System Prompt

**Location**: `ikode-cli/src/sys-prompt.md`

**IMPORTANT**: Keep it concise! This is sent with EVERY request.
- Current size: ~133 lines (~3,000 tokens)
- Each line costs ~23 tokens per request
- Use examples sparingly (3 max)
- Avoid redundancy

**Template Variables**:
- `__WORKING_DIRECTORY__`: Current directory
- `__PLATFORM__`: OS (darwin/linux/windows)
- `__OS_VERSION__`: OS version string
- `__TODAY_DATE__`: Current date (YYYY-MM-DD)
- `__IS_GIT_REPO__`: "Yes" or "No"

### Running Tests

```bash
# All tests
cargo test

# Specific test file
cargo test --test path_validation_tests

# Specific test
cargo test test_rejects_parent_directory_traversal

# With output
cargo test -- --nocapture

# Lint
cargo clippy
```

---

## Known Issues & Limitations

### High Priority (Should Fix Soon)
- ❌ **No streaming responses** - User sees spinner for entire generation
  - `instruct_stream()` exists but isn't used
  - Would dramatically improve UX
- ❌ **No command timeout** - Long-running commands can't be killed

### Medium Priority
- ⚠️ **Tool argument parsing** - Uses `.unwrap_or("{}")`, masks errors
- ⚠️ **No configuration file** - Only environment variables
- ⚠️ **Missing tools**: `search_files`, `list_files`, git operations

### Low Priority
- 📝 **Hardcoded tools** - Should use trait-based plugin system
- 📝 **Excessive cloning** - History cloned on every request
- 📝 **No session persistence** - Can't save/load conversations

See `.junie/guidelines.md` for complete list.

---

## Code Style & Conventions

### Rust Idioms
- **Edition**: 2021
- **Error handling**: `anyhow` for app errors, `Result<T>` everywhere
- **Async**: Use `tokio`, `async fn`, `.await`
- **Formatting**: `cargo fmt` before committing
- **Linting**: `cargo clippy` should pass

### Naming Conventions
- **Structs**: `PascalCase` (e.g., `GaiseClient`, `TodoItem`)
- **Functions**: `snake_case` (e.g., `handle_tool_call`, `validate_path`)
- **Constants**: `SCREAMING_SNAKE_CASE` (e.g., `DEFAULT_MODEL`)
- **Files**: `snake_case.rs` (e.g., `main.rs`, `sys-prompt.md`)

### Comments
- **Minimize comments** - Code should be self-documenting
- **Use comments for**:
  - Complex algorithms
  - Security-critical sections
  - TODOs (but file issues instead)

---

## Testing Philosophy

### What to Test
1. **Security boundaries** - Path validation, command injection
2. **Tool behavior** - File read/write, command execution
3. **Integration** - CLI flags, REPL commands
4. **Edge cases** - Empty input, large files, unicode

### Test Utilities
Use `TestFixture` for file operations:
```rust
use common::TestFixture;

#[test]
fn my_test() {
    let fixture = TestFixture::new();
    fixture.create_file("test.txt", "content");
    assert!(fixture.file_exists("test.txt"));
}
```

### Security Test Examples
```rust
#[test]
fn test_rejects_parent_directory_traversal() {
    let app = create_test_app(temp_dir);
    let result = app.validate_path("../../../etc/passwd");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("outside the working directory"));
}
```

---

## Performance Considerations

### Token Optimization
- **System prompt**: ~2,500 tokens per request → keep minimal
- **History truncation**: Cache-friendly sliding window (default 80 messages, configurable via `--max-history`)
  - Keeps system prompt + first N messages stable (cache-friendly prefix)
  - Truncates middle, keeps recent tail
  - Set `--max-history 0` to disable truncation
- **Edit tool**: Patch-based (`old_text` → `new_text`) instead of full file rewrite
- **Read tool**: Line-range support with 2000-line default cap, 10 MB file size limit
- **String allocations**: 88+ `.to_string()` calls → use `&str` where possible

### Potential Optimizations
1. Use streaming responses (`instruct_stream`)
2. Cache system prompt formatting
3. Reduce string allocations

---

## Environment Setup

### Development
```bash
# Clone repo
git clone <repo-url>
cd ikode

# Build
cargo build

# Run tests
cargo test

# Install locally
cargo install --path ikode-cli

# Run
ikode --prompt "Hello, world"
```

### Provider Configuration
```bash
# OpenAI (required for default)
export OPENAI_API_KEY="sk-..."

# Ollama (optional)
export OLLAMA_URL="http://localhost:11434"

# Vertex AI (optional)
export VERTEXAI_API_URL="https://..."
export VERTEXAI_SA_PATH="/path/to/service-account.json"

# AWS Bedrock (optional)
export AWS_REGION="us-east-1"
```

---

## Debugging Tips

### Enable Verbose Logging
Currently no structured logging, but you can:
1. Add `println!()` statements (temporary)
2. Use `dbg!()` macro for quick debugging
3. Check stderr for errors (`eprintln!()`)

### Common Issues
| Problem | Solution |
|---------|----------|
| "Model not found" | Check provider env vars are set |
| "Path outside working directory" | Path validation working correctly! |
| Tests failing | Run `cargo clean && cargo test` |
| Clippy warnings | Run `cargo clippy --fix` |

### Test Failures
```bash
# Run with backtrace
RUST_BACKTRACE=1 cargo test <test_name>

# Run single test with output
cargo test <test_name> -- --nocapture
```

---

## Important Reminders for AI Assistants

### DO ✅
- Always validate file paths before operations
- Keep system prompt concise (token efficiency)
- Write tests for security-critical code
- Use `anyhow::Context` for error messages
- Run `cargo clippy` and `cargo fmt` before suggesting code

### DON'T ❌
- Never bypass `validate_path()` for file operations
- Don't add verbose comments unless necessary
- Don't assume libraries are available (check Cargo.toml)
- Don't commit secrets or API keys
- Don't make system prompt longer without removing content

### When Making Changes
1. **Read existing code first** - Understand patterns
2. **Check tests** - Understand expected behavior
3. **Follow conventions** - Match existing code style
4. **Test your changes** - Add tests, run `cargo test`
5. **Think about security** - Especially for file/command operations

---

## Project Status

**Current Version**: 0.1.0
**Development Stage**: Active, production-ready for basic operations
**Last Major Update**: 2026-02-22 (token efficiency: patch edits, read line ranges, history truncation)

### What Works Well
- ✅ Multi-provider AI support
- ✅ Patch-based file editing (search-and-replace)
- ✅ File reading with line ranges and size cap
- ✅ Command execution with confirmations
- ✅ Todo management
- ✅ Path validation security
- ✅ Cache-friendly history truncation (configurable)
- ✅ Comprehensive test coverage (37+ tests)

### What Needs Work
- ⚠️ Streaming responses
- ⚠️ Session persistence
- ⚠️ Configuration file support
- ⚠️ Additional tools (search, list, git)
- ⚠️ Better error messages to users

---

## Questions?

If you (AI assistant) are unsure about something:
1. **Check `.junie/guidelines.md`** - Development guidelines
2. **Check tests** - See expected behavior
3. **Read the code** - Main logic in `ikode-cli/src/main.rs`
4. **Ask the user** - When in doubt, ask for clarification

## Quick Links
- User guide: `INSTRUCTIONS.md`
- Developer guide: `.junie/guidelines.md`
- Main app: `ikode-cli/src/main.rs`
- System prompt: `ikode-cli/src/sys-prompt.md`
- Tests: `ikode-cli/tests/`

---

**Last Updated**: 2026-02-22 by Ian Knowles (with AI assistance)
