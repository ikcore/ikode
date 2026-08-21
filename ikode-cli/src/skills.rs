//! Project **skills**: reusable instruction snippets stored as markdown under
//! `.ikode/skills/<name>.md`, mirroring Claude's skill convention.
//!
//! Each file may carry optional YAML-style frontmatter delimited by `---` lines:
//!
//! ```text
//! ---
//! name: review-pr
//! description: Walk a pull request and flag risky changes
//! ---
//! <instruction body the model receives when the skill is invoked>
//! ```
//!
//! Both keys are optional: a missing `name` falls back to the file stem, and a
//! missing `description` falls back to the first non-empty body line. The body
//! (everything after the frontmatter) is what gets fed to the model on invoke.
//!
//! This module is pure file I/O + parsing — no inference — so listing and editing
//! skills never costs a model call.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// One parsed skill file.
pub struct Skill {
    /// Skill name (frontmatter `name:`, else the file stem).
    pub name: String,
    /// One-line summary (frontmatter `description:`, else first body line).
    pub description: String,
    /// Instruction text handed to the model when the skill is invoked.
    pub body: String,
    /// Path to the backing `.md` file.
    pub path: PathBuf,
}

/// `<root>/.ikode/skills`.
pub fn skills_dir(root: &Path) -> PathBuf {
    root.join(".ikode").join("skills")
}

/// Is `name` a safe skill name? Skills map 1:1 onto `<name>.md` files, so we
/// forbid anything that could escape the skills directory or confuse the file
/// system: empty, path separators, `..`, or leading dots.
pub fn valid_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains("..")
        && !name.contains(['/', '\\', ':'])
}

/// Path to a skill file by name (no existence check).
pub fn skill_path(root: &Path, name: &str) -> PathBuf {
    skills_dir(root).join(format!("{name}.md"))
}

/// All skills found in `.ikode/skills/`, sorted by name. A missing directory
/// yields an empty list rather than an error.
pub fn list(root: &Path) -> Vec<Skill> {
    let dir = skills_dir(root);
    let mut out = Vec::new();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if let Some(skill) = parse(&path) {
            out.push(skill);
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// Load a single skill by name (case-insensitive match against the file stem or
/// frontmatter name).
pub fn load(root: &Path, name: &str) -> Option<Skill> {
    let direct = skill_path(root, name);
    if direct.is_file() {
        return parse(&direct);
    }
    let needle = name.trim().to_lowercase();
    list(root)
        .into_iter()
        .find(|s| s.name.to_lowercase() == needle)
}

/// Parse a skill file. Returns `None` only if the file can't be read.
pub fn parse(path: &Path) -> Option<Skill> {
    let text = fs::read_to_string(path).ok()?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("skill")
        .to_string();

    let (front, body) = split_frontmatter(&text);
    let mut name = None;
    let mut description = None;
    for line in front.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let value = value.trim().trim_matches('"').trim().to_string();
            match key.trim().to_lowercase().as_str() {
                "name" if !value.is_empty() => name = Some(value),
                "description" if !value.is_empty() => description = Some(value),
                _ => {}
            }
        }
    }

    let body = body.trim().to_string();
    let description = description.unwrap_or_else(|| {
        body.lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#'))
            .unwrap_or("(no description)")
            .to_string()
    });

    Some(Skill {
        name: name.unwrap_or(stem),
        description,
        body,
        path: path.to_path_buf(),
    })
}

/// Split a `---`-delimited YAML frontmatter block from the body. If there's no
/// leading frontmatter the whole text is treated as the body.
fn split_frontmatter(text: &str) -> (String, String) {
    let trimmed = text.trim_start_matches('\u{feff}');
    let mut lines = trimmed.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (String::new(), trimmed.to_string());
    }
    let mut front = String::new();
    let mut rest = String::new();
    let mut in_front = true;
    for line in lines {
        if in_front && line.trim() == "---" {
            in_front = false;
            continue;
        }
        if in_front {
            front.push_str(line);
            front.push('\n');
        } else {
            rest.push_str(line);
            rest.push('\n');
        }
    }
    // No closing `---` → not real frontmatter; treat everything as body.
    if in_front {
        return (String::new(), trimmed.to_string());
    }
    (front, rest)
}

/// Create a new skill file, scaffolded with frontmatter. Errors if it already
/// exists so `add` never clobbers an existing skill.
pub fn create(root: &Path, name: &str) -> io::Result<PathBuf> {
    let dir = skills_dir(root);
    fs::create_dir_all(&dir)?;
    let path = skill_path(root, name);
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("skill '{name}' already exists at {}", path.display()),
        ));
    }
    let template = format!(
        "---\nname: {name}\ndescription: One-line summary of what this skill does\n---\n\n\
         # {name}\n\n\
         Describe the steps the model should follow when this skill is invoked.\n",
    );
    fs::write(&path, template)?;
    Ok(path)
}

/// Delete a skill file by name. Returns the path removed.
pub fn remove(root: &Path, name: &str) -> io::Result<PathBuf> {
    let path = match load(root, name) {
        Some(s) => s.path,
        None => {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no skill named '{name}'"),
            ))
        }
    };
    fs::remove_file(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn parses_frontmatter_name_and_description() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write(
            tmp.path(),
            "x.md",
            "---\nname: Review PR\ndescription: Check a diff\n---\n\nDo the thing.\n",
        );
        let s = parse(&path).unwrap();
        assert_eq!(s.name, "Review PR");
        assert_eq!(s.description, "Check a diff");
        assert_eq!(s.body, "Do the thing.");
    }

    #[test]
    fn falls_back_to_stem_and_first_body_line() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write(
            tmp.path(),
            "tidy.md",
            "# heading\n\nFirst real line.\nmore\n",
        );
        let s = parse(&path).unwrap();
        assert_eq!(s.name, "tidy");
        assert_eq!(s.description, "First real line.");
    }

    #[test]
    fn no_frontmatter_keeps_full_body() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write(tmp.path(), "plain.md", "just instructions\nline two\n");
        let s = parse(&path).unwrap();
        assert_eq!(s.body, "just instructions\nline two");
    }

    #[test]
    fn create_then_load_then_remove() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let path = create(root, "demo").unwrap();
        assert!(path.is_file());
        assert!(create(root, "demo").is_err()); // no clobber
        let s = load(root, "demo").unwrap();
        assert_eq!(s.name, "demo");
        assert_eq!(list(root).len(), 1);
        remove(root, "demo").unwrap();
        assert!(load(root, "demo").is_none());
    }
}
