use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

pub struct TestFixture {
    pub temp_dir: TempDir,
}

impl TestFixture {
    pub fn new() -> Self {
        Self {
            temp_dir: TempDir::new().expect("Failed to create temp directory"),
        }
    }

    pub fn path(&self) -> &std::path::Path {
        self.temp_dir.path()
    }

    pub fn create_file(&self, relative_path: &str, content: &str) -> PathBuf {
        let file_path = self.temp_dir.path().join(relative_path);

        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).expect("Failed to create parent directories");
        }

        fs::write(&file_path, content).expect("Failed to write file");
        file_path
    }

    pub fn create_dir(&self, relative_path: &str) -> PathBuf {
        let dir_path = self.temp_dir.path().join(relative_path);
        fs::create_dir_all(&dir_path).expect("Failed to create directory");
        dir_path
    }

    pub fn read_file(&self, relative_path: &str) -> String {
        let file_path = self.temp_dir.path().join(relative_path);
        fs::read_to_string(&file_path).expect("Failed to read file")
    }

    pub fn file_exists(&self, relative_path: &str) -> bool {
        self.temp_dir.path().join(relative_path).exists()
    }

    pub fn create_nested_structure(&self) -> PathBuf {
        let nested = self.create_dir("src/components/utils");
        self.create_file("src/main.rs", "fn main() {}");
        self.create_file("src/components/mod.rs", "pub mod utils;");
        self.create_file("src/components/utils/helper.rs", "pub fn help() {}");
        nested
    }

    pub fn create_sample_project(&self) {
        self.create_file("README.md", "# Test Project");
        self.create_file("Cargo.toml", "[package]\nname = \"test\"");
        self.create_file("src/main.rs", "fn main() { println!(\"Hello\"); }");
        self.create_file("src/lib.rs", "pub fn add(a: i32, b: i32) -> i32 { a + b }");
        self.create_file("tests/test.rs", "#[test]\nfn test_add() {}");
    }
}

impl Default for TestFixture {
    fn default() -> Self {
        Self::new()
    }
}

pub fn assert_file_contains(path: &std::path::Path, expected: &str) {
    let content = fs::read_to_string(path).expect("Failed to read file");
    assert!(
        content.contains(expected),
        "File does not contain expected text.\nExpected: {}\nActual content: {}",
        expected,
        content
    );
}

pub fn assert_file_equals(path: &std::path::Path, expected: &str) {
    let content = fs::read_to_string(path).expect("Failed to read file");
    assert_eq!(
        content, expected,
        "File content does not match.\nExpected: {}\nActual: {}",
        expected, content
    );
}
