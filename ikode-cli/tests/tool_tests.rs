use std::fs;
use tempfile::TempDir;

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
