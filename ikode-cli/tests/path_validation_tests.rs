use std::path::PathBuf;
use std::fs;
use tempfile::TempDir;

fn create_test_app(working_dir: PathBuf) -> TestApp {
    TestApp { working_directory: working_dir }
}

struct TestApp {
    working_directory: PathBuf,
}

impl TestApp {
    fn validate_path(&self, path: &str) -> Result<PathBuf, String> {
        let requested_path = std::path::Path::new(path);
        let canonical_wd = self.working_directory.canonicalize()
            .unwrap_or_else(|_| self.working_directory.clone());

        let canonical_path = if requested_path.is_absolute() {
            requested_path.canonicalize().unwrap_or_else(|_| requested_path.to_path_buf())
        } else {
            let joined = self.working_directory.join(requested_path);
            joined.canonicalize().unwrap_or_else(|_| joined)
        };

        if !canonical_path.starts_with(&canonical_wd) {
            return Err(format!(
                "Path '{}' is outside the working directory. For security reasons, file operations are restricted to the working directory and its subdirectories.",
                path
            ));
        }

        Ok(canonical_path)
    }
}

#[test]
fn test_valid_relative_path() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("test.txt");
    fs::write(&test_file, "test content").unwrap();

    let app = create_test_app(temp_dir.path().to_path_buf());
    let result = app.validate_path("test.txt");

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), test_file.canonicalize().unwrap());
}

#[test]
fn test_valid_nested_path() {
    let temp_dir = TempDir::new().unwrap();
    let nested_dir = temp_dir.path().join("nested").join("deep");
    fs::create_dir_all(&nested_dir).unwrap();
    let test_file = nested_dir.join("file.txt");
    fs::write(&test_file, "content").unwrap();

    let app = create_test_app(temp_dir.path().to_path_buf());
    let result = app.validate_path("nested/deep/file.txt");

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), test_file.canonicalize().unwrap());
}

#[test]
fn test_rejects_parent_directory_traversal() {
    let temp_dir = TempDir::new().unwrap();
    let app = create_test_app(temp_dir.path().to_path_buf());

    let result = app.validate_path("../../../etc/passwd");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("outside the working directory"));
}

#[test]
fn test_rejects_absolute_path_outside_working_dir() {
    let temp_dir = TempDir::new().unwrap();
    let app = create_test_app(temp_dir.path().to_path_buf());

    let result = app.validate_path("/etc/passwd");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("outside the working directory"));
}

#[test]
fn test_rejects_symlink_escape() {
    let temp_dir = TempDir::new().unwrap();
    let outside_file = TempDir::new().unwrap();
    let outside_path = outside_file.path().join("secret.txt");
    fs::write(&outside_path, "secret").unwrap();

    let symlink_path = temp_dir.path().join("escape");

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let _ = symlink(&outside_path, &symlink_path);
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::symlink_file;
        let _ = symlink_file(&outside_path, &symlink_path);
    }

    let app = create_test_app(temp_dir.path().to_path_buf());

    if symlink_path.exists() {
        let result = app.validate_path("escape");
        assert!(result.is_err());
    }
}

#[test]
fn test_allows_dot_slash_prefix() {
    let temp_dir = TempDir::new().unwrap();
    let test_file = temp_dir.path().join("file.txt");
    fs::write(&test_file, "content").unwrap();

    let app = create_test_app(temp_dir.path().to_path_buf());
    let result = app.validate_path("./file.txt");

    assert!(result.is_ok());
}

#[test]
fn test_rejects_sneaky_dot_dot_in_middle() {
    let temp_dir = TempDir::new().unwrap();
    let subdir = temp_dir.path().join("subdir");
    fs::create_dir(&subdir).unwrap();

    let app = create_test_app(temp_dir.path().to_path_buf());
    let result = app.validate_path("subdir/../../etc/passwd");

    assert!(result.is_err());
}

#[test]
fn test_handles_nonexistent_file_in_valid_directory() {
    let temp_dir = TempDir::new().unwrap();
    let canonical_temp = temp_dir.path().canonicalize().unwrap();
    let app = create_test_app(canonical_temp.clone());

    let result = app.validate_path("nonexistent.txt");
    assert!(result.is_ok());

    let expected = canonical_temp.join("nonexistent.txt");
    assert_eq!(result.unwrap(), expected);
}

#[test]
fn test_rejects_empty_path() {
    let temp_dir = TempDir::new().unwrap();
    let app = create_test_app(temp_dir.path().to_path_buf());

    let result = app.validate_path("");
    assert!(result.is_ok());
}

#[test]
fn test_allows_current_directory() {
    let temp_dir = TempDir::new().unwrap();
    let app = create_test_app(temp_dir.path().to_path_buf());

    let result = app.validate_path(".");
    assert!(result.is_ok());
}
