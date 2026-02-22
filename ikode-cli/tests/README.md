# ikode-cli Test Suite

This directory contains comprehensive tests for the ikode-cli application.

## Test Organization

### `path_validation_tests.rs`
Tests for path security and validation:
- Valid relative and nested paths
- Parent directory traversal attacks (`../../../etc/passwd`)
- Absolute path validation
- Symlink escape prevention
- Edge cases (empty paths, current directory, nonexistent files)

**Key Tests:**
- ✅ `test_valid_relative_path` - Ensures relative paths work
- ✅ `test_rejects_parent_directory_traversal` - Security: blocks `../` attacks
- ✅ `test_rejects_absolute_path_outside_working_dir` - Security: blocks `/etc/passwd`
- ✅ `test_rejects_symlink_escape` - Security: prevents symlink escapes

### `tool_tests.rs`
Tests for file operation tools:
- Read file operations (success, not found, empty, large files, line numbers, offset/limit, size cap)
- Search-and-replace edit operations (exact match, not found, ambiguous match, multiline, preservation)
- Write file operations (create, overwrite, unicode)
- History truncation (prefix preservation, recent messages, truncation notice, max size, small history passthrough)
- Nested directory handling
- Edge cases (multiline, unicode, 1MB+ files)

**Key Tests:**
- ✅ `test_read_file_success` - Basic read operations
- ✅ `test_read_file_with_line_numbers` - Line number output format
- ✅ `test_read_file_with_offset_and_limit` - Line range support
- ✅ `test_read_file_truncation_message` - Default 2000-line cap
- ✅ `test_read_file_size_cap` - 10 MB file size rejection
- ✅ `test_edit_file_search_replace` - Patch-based editing
- ✅ `test_edit_file_old_text_not_found` - Error on missing text
- ✅ `test_edit_file_ambiguous_match` - Error on multiple matches
- ✅ `test_edit_file_multiline_replacement` - Multi-line edits
- ✅ `test_edit_file_preserves_rest_of_file` - Non-targeted content unchanged
- ✅ `test_history_truncation_preserves_prefix` - System prompt + early messages kept
- ✅ `test_history_truncation_keeps_recent_messages` - Recent tail preserved
- ✅ `test_history_truncation_inserts_notice` - Truncation notice added
- ✅ `test_history_truncation_respects_max_size` - Max size enforced
- ✅ `test_write_file_overwrites_existing` - Overwrite behavior
- ✅ `test_write_unicode_content` - Unicode support
- ✅ `test_read_large_file` - Performance with large files

### `integration_tests.rs`
End-to-end CLI and functionality tests:
- CLI flags (`--help`, `--version`, `--model`)
- Todo system (add, complete, list)
- System prompt formatting
- JSON parsing for tool arguments
- Command execution
- Date and environment detection

**Key Tests:**
- ✅ `test_cli_help_flag` - CLI interface works
- ✅ `test_todo_add_functionality` - Todo system
- ✅ `test_system_prompt_formatting` - Dynamic prompt generation
- ✅ `test_date_formatting` - Date utilities

### `common/mod.rs`
Shared test utilities and fixtures:
- `TestFixture` - Manages temporary test directories
- Helper methods for file/directory creation
- Assertion utilities

**Usage:**
```rust
use common::TestFixture;

#[test]
fn my_test() {
    let fixture = TestFixture::new();
    fixture.create_file("test.txt", "content");
    assert!(fixture.file_exists("test.txt"));
}
```

## Running Tests

### Run all tests
```bash
cargo test
```

### Run specific test file
```bash
cargo test --test path_validation_tests
cargo test --test tool_tests
cargo test --test integration_tests
```

### Run specific test
```bash
cargo test test_rejects_parent_directory_traversal
```

### Run with output
```bash
cargo test -- --nocapture
```

### Run in parallel
```bash
cargo test -- --test-threads=4
```

## Test Coverage

### Security Tests (Critical)
- ✅ Path traversal prevention
- ✅ Absolute path restrictions
- ✅ Symlink escape prevention
- ✅ Command injection (basic)

### Functionality Tests
- ✅ File read/write operations
- ✅ File read with line numbers, offset, limit
- ✅ File size cap (10 MB)
- ✅ Search-and-replace editing (exact match, ambiguous, not found)
- ✅ History truncation (prefix, tail, notice, max size)
- ✅ Todo management
- ✅ CLI argument parsing
- ✅ JSON tool argument parsing
- ✅ Date formatting

### Integration Tests
- ✅ CLI flags (including `--max-history`, `--prefix-keep`)
- ✅ System prompt generation
- ✅ Environment detection
- ✅ Command execution

## Future Test Additions

### High Priority
- [ ] Mock LLM responses for full end-to-end tests
- [ ] Command timeout tests
- [ ] Brave mode security tests
- [ ] Tool call parsing error handling

### Medium Priority
- [ ] Performance benchmarks
- [ ] Streaming response tests (when implemented)
- [ ] Multi-provider configuration tests
- [ ] Guide file loading tests
- [ ] Session cache key generation

### Low Priority
- [ ] UI/UX tests (spinner, colors)
- [ ] Platform-specific tests
- [ ] Memory leak tests

## Contributing Tests

When adding new features:
1. Add unit tests for core logic
2. Add integration tests for user-facing behavior
3. Add security tests if touching file/command operations
4. Update this README with test descriptions

## Test Best Practices

1. **Use `TestFixture`** for temporary directories - auto-cleanup
2. **Test security boundaries** - always test what should be blocked
3. **Test edge cases** - empty inputs, large inputs, unicode, etc.
4. **Keep tests fast** - avoid network calls, large file operations when possible
5. **Use descriptive names** - `test_rejects_X` is clearer than `test_security_1`
