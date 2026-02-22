use std::fs;
use tempfile::TempDir;

fn read_file(file_path: &std::path::Path, offset: Option<usize>, limit: Option<usize>) -> Result<String, String> {
    let metadata = fs::metadata(file_path).map_err(|e| format!("Error reading file: {}", e))?;

    const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
    if metadata.len() > MAX_FILE_SIZE {
        return Err(format!(
            "Error: file is too large ({:.1} MB). Maximum supported size is {:.0} MB.",
            metadata.len() as f64 / (1024.0 * 1024.0),
            MAX_FILE_SIZE as f64 / (1024.0 * 1024.0)
        ));
    }

    let content = fs::read_to_string(file_path).map_err(|e| format!("Error reading file: {}", e))?;

    let total_lines = content.lines().count();
    let offset = offset.unwrap_or(1).max(1);
    let limit = limit.unwrap_or(2000);

    let selected: Vec<String> = content
        .lines()
        .enumerate()
        .skip(offset - 1)
        .take(limit)
        .map(|(i, line)| format!("{:>6}\t{}", i + 1, line))
        .collect();

    let mut result = selected.join("\n");
    let last_shown = (offset - 1 + selected.len()).min(total_lines);
    if last_shown < total_lines {
        result.push_str(&format!(
            "\n\n... ({} more lines not shown. Use offset={} to continue reading.)",
            total_lines - last_shown,
            last_shown + 1
        ));
    }

    Ok(result)
}

fn apply_edit(file_path: &std::path::Path, old_text: &str, new_text: &str) -> Result<String, String> {
    let content = fs::read_to_string(file_path).map_err(|e| format!("Error reading file: {}", e))?;
    let count = content.matches(old_text).count();
    if count == 0 {
        return Err("Error: old_text not found in file. Make sure it matches exactly, including whitespace and indentation.".to_string());
    }
    if count > 1 {
        return Err(format!("Error: old_text matches {} locations in the file. Provide more surrounding context to make the match unique.", count));
    }
    let new_content = content.replacen(old_text, new_text, 1);
    fs::write(file_path, &new_content).map_err(|e| format!("Error writing file: {}", e))?;
    Ok("File updated successfully.".to_string())
}

#[test]
fn test_edit_file_search_replace() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("code.rs");
    fs::write(&file_path, "fn main() {\n    println!(\"hello\");\n}\n").unwrap();

    let result = apply_edit(&file_path, "println!(\"hello\")", "println!(\"world\")");
    assert!(result.is_ok());

    let content = fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("println!(\"world\")"));
    assert!(!content.contains("println!(\"hello\")"));
}

#[test]
fn test_edit_file_old_text_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("code.rs");
    fs::write(&file_path, "fn main() {}\n").unwrap();

    let result = apply_edit(&file_path, "nonexistent text", "replacement");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("not found"));
}

#[test]
fn test_edit_file_ambiguous_match() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("code.rs");
    fs::write(&file_path, "let x = 1;\nlet y = 1;\n").unwrap();

    let result = apply_edit(&file_path, "= 1;", "= 2;");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("matches 2 locations"));
}

#[test]
fn test_edit_file_preserves_rest_of_file() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("code.rs");
    let original = "line 1\nline 2\nline 3\nline 4\n";
    fs::write(&file_path, original).unwrap();

    let result = apply_edit(&file_path, "line 2", "LINE TWO");
    assert!(result.is_ok());

    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "line 1\nLINE TWO\nline 3\nline 4\n");
}

#[test]
fn test_edit_file_multiline_replacement() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("code.rs");
    fs::write(&file_path, "fn foo() {\n    // old\n}\n").unwrap();

    let result = apply_edit(&file_path, "fn foo() {\n    // old\n}", "fn foo() {\n    // new\n    do_stuff();\n}");
    assert!(result.is_ok());

    let content = fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("do_stuff()"));
    assert!(content.contains("// new"));
}

#[test]
fn test_create_file_fails_if_exists() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("existing.txt");
    fs::write(&file_path, "content").unwrap();

    assert!(file_path.exists());
}

#[test]
fn test_read_file_success() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    let content = "Hello, World!";
    fs::write(&file_path, content).unwrap();

    let read_content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(read_content, content);
}

#[test]
fn test_read_file_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("nonexistent.txt");

    let result = fs::read_to_string(&file_path);
    assert!(result.is_err());
}

#[test]
fn test_write_file_success() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("output.txt");
    let content = "Test content";

    let result = fs::write(&file_path, content);
    assert!(result.is_ok());

    let read_back = fs::read_to_string(&file_path).unwrap();
    assert_eq!(read_back, content);
}

#[test]
fn test_write_file_overwrites_existing() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("file.txt");

    fs::write(&file_path, "original").unwrap();
    fs::write(&file_path, "updated").unwrap();

    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "updated");
}

#[test]
fn test_write_file_creates_if_not_exists() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("new_file.txt");

    assert!(!file_path.exists());

    fs::write(&file_path, "new content").unwrap();
    assert!(file_path.exists());
}

#[test]
fn test_read_empty_file() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("empty.txt");
    fs::write(&file_path, "").unwrap();

    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "");
}

#[test]
fn test_read_multiline_file() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("multiline.txt");
    let content = "line 1\nline 2\nline 3";
    fs::write(&file_path, content).unwrap();

    let read_content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(read_content, content);
    assert_eq!(read_content.lines().count(), 3);
}

#[test]
fn test_read_large_file() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("large.txt");
    let content = "x".repeat(1_000_000);
    fs::write(&file_path, &content).unwrap();

    let read_content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(read_content.len(), 1_000_000);
}

#[test]
fn test_write_unicode_content() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("unicode.txt");
    let content = "Hello 世界 🌍 Привет";
    fs::write(&file_path, content).unwrap();

    let read_content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(read_content, content);
}

#[test]
fn test_read_write_nested_directory() {
    let temp_dir = TempDir::new().unwrap();
    let nested = temp_dir.path().join("a").join("b").join("c");
    fs::create_dir_all(&nested).unwrap();

    let file_path = nested.join("file.txt");
    fs::write(&file_path, "nested content").unwrap();

    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "nested content");
}

fn make_message(role: &str, text: &str) -> (String, String) {
    (role.to_string(), text.to_string())
}

fn build_request_history(history: &[(String, String)]) -> Vec<(String, String)> {
    const MAX_HISTORY_MESSAGES: usize = 80;
    const PREFIX_MESSAGES: usize = 4;

    let total = history.len();
    if total <= MAX_HISTORY_MESSAGES {
        return history.to_vec();
    }

    let mut result = Vec::with_capacity(MAX_HISTORY_MESSAGES);
    let prefix_end = (1 + PREFIX_MESSAGES).min(total);
    result.extend_from_slice(&history[..prefix_end]);

    let tail_count = MAX_HISTORY_MESSAGES - prefix_end;
    let tail_start = total - tail_count;

    if tail_start > prefix_end {
        result.push(make_message("system", &format!(
            "[Note: {} earlier messages were truncated to save context.]",
            tail_start - prefix_end
        )));
    }

    let actual_tail_start = tail_start.max(prefix_end);
    result.extend_from_slice(&history[actual_tail_start..]);

    result
}

#[test]
fn test_history_no_truncation_when_small() {
    let history: Vec<_> = (0..50).map(|i| make_message("user", &format!("msg {}", i))).collect();
    let result = build_request_history(&history);
    assert_eq!(result.len(), 50);
}

#[test]
fn test_history_truncation_preserves_prefix() {
    let mut history = vec![make_message("system", "system prompt")];
    for i in 1..=120 {
        history.push(make_message("user", &format!("msg {}", i)));
    }

    let result = build_request_history(&history);

    assert_eq!(result[0].1, "system prompt");
    assert_eq!(result[1].1, "msg 1");
    assert_eq!(result[2].1, "msg 2");
    assert_eq!(result[3].1, "msg 3");
    assert_eq!(result[4].1, "msg 4");
}

#[test]
fn test_history_truncation_keeps_recent_messages() {
    let mut history = vec![make_message("system", "system prompt")];
    for i in 1..=120 {
        history.push(make_message("user", &format!("msg {}", i)));
    }

    let result = build_request_history(&history);
    let last = &result[result.len() - 1];
    assert_eq!(last.1, "msg 120");
}

#[test]
fn test_history_truncation_inserts_notice() {
    let mut history = vec![make_message("system", "system prompt")];
    for i in 1..=120 {
        history.push(make_message("user", &format!("msg {}", i)));
    }

    let result = build_request_history(&history);
    let notice = result.iter().find(|(role, text)| role == "system" && text.contains("truncated"));
    assert!(notice.is_some());
}

#[test]
fn test_history_truncation_respects_max_size() {
    let mut history = vec![make_message("system", "system prompt")];
    for i in 1..=500 {
        history.push(make_message("user", &format!("msg {}", i)));
    }

    let result = build_request_history(&history);
    assert!(result.len() <= 82);
}

#[test]
fn test_read_file_with_line_numbers() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("numbered.txt");
    fs::write(&file_path, "alpha\nbeta\ngamma\n").unwrap();

    let result = read_file(&file_path, None, None).unwrap();
    assert!(result.contains("     1\talpha"));
    assert!(result.contains("     2\tbeta"));
    assert!(result.contains("     3\tgamma"));
}

#[test]
fn test_read_file_with_offset() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("offset.txt");
    fs::write(&file_path, "line1\nline2\nline3\nline4\nline5\n").unwrap();

    let result = read_file(&file_path, Some(3), None).unwrap();
    assert!(!result.contains("line1"));
    assert!(!result.contains("line2"));
    assert!(result.contains("     3\tline3"));
    assert!(result.contains("     4\tline4"));
}

#[test]
fn test_read_file_with_limit() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("limited.txt");
    let content: String = (1..=100).map(|i| format!("line {}\n", i)).collect();
    fs::write(&file_path, &content).unwrap();

    let result = read_file(&file_path, None, Some(5)).unwrap();
    assert!(result.contains("     1\tline 1"));
    assert!(result.contains("     5\tline 5"));
    assert!(!result.contains("     6\t"));
    assert!(result.contains("95 more lines not shown"));
}

#[test]
fn test_read_file_with_offset_and_limit() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("range.txt");
    let content: String = (1..=50).map(|i| format!("line {}\n", i)).collect();
    fs::write(&file_path, &content).unwrap();

    let result = read_file(&file_path, Some(10), Some(5)).unwrap();
    assert!(result.contains("    10\tline 10"));
    assert!(result.contains("    14\tline 14"));
    assert!(!result.contains("    15\t"));
    assert!(result.contains("more lines not shown"));
    assert!(result.contains("offset=15"));
}

#[test]
fn test_read_file_truncation_message() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("big.txt");
    let content: String = (1..=3000).map(|i| format!("line {}\n", i)).collect();
    fs::write(&file_path, &content).unwrap();

    let result = read_file(&file_path, None, None).unwrap();
    assert!(result.contains("     1\tline 1"));
    assert!(result.contains("  2000\tline 2000"));
    assert!(!result.contains("  2001\t"));
    assert!(result.contains("1000 more lines not shown"));
    assert!(result.contains("offset=2001"));
}

#[test]
fn test_read_file_size_cap() {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("huge.bin");
    let content = vec![b'x'; 11 * 1024 * 1024];
    fs::write(&file_path, &content).unwrap();

    let result = read_file(&file_path, None, None);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("too large"));
}
