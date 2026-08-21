//! Hierarchical project-local path matching for index exclusions.
//!
//! Every `.ikignore` beneath the project root contributes rules for its own
//! directory tree. The indexer uses the resulting matcher while walking source,
//! so ignored files never enter the graph and newly ignored graph files are
//! pruned during reconciliation.

use std::io;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

const IKIGNORE_FILE: &str = ".ikignore";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// A pattern without a path separator matches any file or directory name.
    Name,
    /// A path pattern is relative to the directory containing `.ikignore`.
    Relative,
    /// An absolute path pattern is matched against the full on-disk path.
    Absolute,
}

#[derive(Debug, Clone)]
struct Rule {
    pattern: String,
    scope: Scope,
    /// Project-relative directory containing the rule's `.ikignore`.
    base: String,
}

/// Discovered and parsed `.ikignore` rules for one project tree.
#[derive(Debug, Clone)]
pub(crate) struct IkIgnore {
    root: String,
    rules: Vec<Rule>,
    files: Vec<String>,
}

impl IkIgnore {
    /// Discover every `.ikignore` under `root`, except below the indexer's built-in
    /// excluded directories, then parse each file relative to its containing folder.
    ///
    /// Discovery is deliberately separate from the source walk: a parent rule can
    /// prune a directory efficiently during indexing without preventing us from
    /// detecting and reporting a nested `.ikignore` first.
    pub(crate) fn discover(root: &Path, excluded_dirs: &[&str]) -> io::Result<Self> {
        let absolute_root = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
        let mut paths: Vec<PathBuf> = Vec::new();
        let walker = WalkDir::new(&absolute_root)
            .into_iter()
            .filter_entry(|entry| {
                entry.depth() == 0
                    || !entry.file_type().is_dir()
                    || !excluded_dirs.contains(&entry.file_name().to_string_lossy().as_ref())
            });
        for entry in walker {
            let entry = entry.map_err(|error| io::Error::other(error.to_string()))?;
            if entry.file_type().is_file() && entry.file_name() == IKIGNORE_FILE {
                paths.push(entry.into_path());
            }
        }
        paths.sort_by_key(|path| slash_path(path));

        let mut rules = Vec::new();
        let mut files = Vec::with_capacity(paths.len());
        for path in paths {
            let relative_file = path.strip_prefix(&absolute_root).unwrap_or(&path);
            files.push(slash_path(relative_file));
            let base = relative_file
                .parent()
                .map(|parent| normalize_path(&parent.to_string_lossy()))
                .unwrap_or_default();
            let contents = std::fs::read_to_string(&path)?;
            rules.extend(parse_rules(
                contents.strip_prefix('\u{feff}').unwrap_or(&contents),
                &base,
            ));
        }

        Ok(Self {
            root: normalize_path(&absolute_root.to_string_lossy()),
            rules,
            files,
        })
    }

    /// Project-relative `.ikignore` paths found during discovery.
    pub(crate) fn files(&self) -> &[String] {
        &self.files
    }

    /// Whether a project-relative or absolute file/directory path is excluded.
    ///
    /// Matching every ancestor as well as the file itself makes `generated/` and
    /// `src/*` useful directory exclusions. Relative and bare-name rules only see
    /// the portion below their own `.ikignore`; absolute rules remain explicit
    /// full-path matches.
    pub(crate) fn is_ignored(&self, path: &str) -> bool {
        if self.rules.is_empty() {
            return false;
        }

        let normalized = normalize_path(path);
        let (relative, absolute) = if is_absolute(&normalized) {
            let relative = normalized
                .strip_prefix(&self.root)
                .and_then(|rest| rest.strip_prefix('/'))
                .unwrap_or(&normalized)
                .to_string();
            (relative, normalized)
        } else {
            let relative = normalized
                .strip_prefix("./")
                .unwrap_or(&normalized)
                .trim_start_matches('/')
                .to_string();
            let absolute = join_path(&self.root, &relative);
            (relative, absolute)
        };

        self.rules.iter().any(|rule| match rule.scope {
            Scope::Absolute => {
                ancestors(&absolute).any(|candidate| glob_matches(&rule.pattern, candidate))
            }
            Scope::Name | Scope::Relative => match below_base(&relative, &rule.base) {
                Some(scoped) => match rule.scope {
                    Scope::Name => scoped
                        .split('/')
                        .filter(|part| !part.is_empty())
                        .any(|part| glob_matches(&rule.pattern, part)),
                    Scope::Relative => {
                        ancestors(scoped).any(|candidate| glob_matches(&rule.pattern, candidate))
                    }
                    Scope::Absolute => unreachable!(),
                },
                None => false,
            },
        })
    }
}

fn parse_rules(contents: &str, base: &str) -> Vec<Rule> {
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }

            let mut pattern = normalize_path(line);
            let absolute = is_absolute(&pattern);
            while pattern.len() > 1 && pattern.ends_with('/') {
                pattern.pop();
            }
            if pattern.is_empty() {
                return None;
            }

            let scope = if absolute {
                Scope::Absolute
            } else if pattern.contains('/') {
                pattern = pattern
                    .strip_prefix("./")
                    .unwrap_or(&pattern)
                    .trim_start_matches('/')
                    .to_string();
                Scope::Relative
            } else {
                Scope::Name
            };
            Some(Rule {
                pattern,
                scope,
                base: base.to_string(),
            })
        })
        .collect()
}

/// Return the part of `path` below `base`, enforcing a component boundary.
fn below_base<'a>(path: &'a str, base: &str) -> Option<&'a str> {
    if base.is_empty() {
        Some(path)
    } else if path == base {
        Some("")
    } else {
        path.strip_prefix(base)?.strip_prefix('/')
    }
}

/// Yield `path`, then each of its parent paths. This models matching a directory
/// during a walk: if a rule matches an ancestor, every file beneath it is ignored.
fn ancestors(path: &str) -> impl Iterator<Item = &str> {
    std::iter::successors(Some(path), |current| {
        if *current == "/" {
            return None;
        }
        let slash = current.rfind('/')?;
        if slash == 0 {
            Some(&current[..1])
        } else {
            Some(&current[..slash])
        }
    })
}

fn join_path(root: &str, relative: &str) -> String {
    if root == "/" {
        format!("/{relative}")
    } else if relative.is_empty() {
        root.to_string()
    } else {
        format!("{}/{relative}", root.trim_end_matches('/'))
    }
}

fn normalize_path(path: &str) -> String {
    let normalized = slash_path(Path::new(path));
    if cfg!(windows) {
        normalized.to_lowercase()
    } else {
        normalized
    }
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn is_absolute(path: &str) -> bool {
    path.starts_with('/')
        || path.starts_with("//")
        || (path.len() >= 3
            && path.as_bytes()[0].is_ascii_alphabetic()
            && path.as_bytes()[1] == b':'
            && path.as_bytes()[2] == b'/')
}

/// Path-aware glob matching:
///
/// - `*` matches zero or more characters within one path component.
/// - `?` matches one character within one path component.
/// - `**` matches across path separators; `**/` may also match zero directories.
fn glob_matches(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let mut memo = vec![vec![None; text.len() + 1]; pattern.len() + 1];

    fn matches_from(
        pattern: &[char],
        text: &[char],
        pattern_at: usize,
        text_at: usize,
        memo: &mut [Vec<Option<bool>>],
    ) -> bool {
        if let Some(result) = memo[pattern_at][text_at] {
            return result;
        }

        let result = if pattern_at == pattern.len() {
            text_at == text.len()
        } else {
            match pattern[pattern_at] {
                '*' if pattern.get(pattern_at + 1) == Some(&'*') => {
                    let mut next = pattern_at + 2;
                    while pattern.get(next) == Some(&'*') {
                        next += 1;
                    }
                    if pattern.get(next) == Some(&'/') {
                        matches_from(pattern, text, next + 1, text_at, memo)
                            || (text_at < text.len()
                                && matches_from(pattern, text, pattern_at, text_at + 1, memo))
                    } else {
                        matches_from(pattern, text, next, text_at, memo)
                            || (text_at < text.len()
                                && matches_from(pattern, text, pattern_at, text_at + 1, memo))
                    }
                }
                '*' => {
                    matches_from(pattern, text, pattern_at + 1, text_at, memo)
                        || (text_at < text.len()
                            && text[text_at] != '/'
                            && matches_from(pattern, text, pattern_at, text_at + 1, memo))
                }
                '?' => {
                    text_at < text.len()
                        && text[text_at] != '/'
                        && matches_from(pattern, text, pattern_at + 1, text_at + 1, memo)
                }
                literal => {
                    text.get(text_at) == Some(&literal)
                        && matches_from(pattern, text, pattern_at + 1, text_at + 1, memo)
                }
            }
        };
        memo[pattern_at][text_at] = Some(result);
        result
    }

    matches_from(&pattern, &text, 0, 0, &mut memo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn glob_supports_component_and_recursive_wildcards() {
        assert!(glob_matches("*.rs", "generated.rs"));
        assert!(glob_matches("file-?.rs", "file-1.rs"));
        assert!(!glob_matches("*.rs", "nested/generated.rs"));
        assert!(glob_matches("src/**/generated-?.rs", "src/generated-1.rs"));
        assert!(glob_matches(
            "src/**/generated-?.rs",
            "src/deep/tree/generated-2.rs"
        ));
        assert!(!glob_matches(
            "src/**/generated-?.rs",
            "src/deep/generated-long.rs"
        ));
    }

    #[test]
    fn matches_names_relative_paths_directories_and_absolute_paths() {
        let dir = tempdir().unwrap();
        let absolute_file = dir.path().join("absolute").join("skip.rs");
        let absolute_pattern = absolute_file.to_string_lossy().replace('\\', "/");
        fs::write(
            dir.path().join(IKIGNORE_FILE),
            format!(
                "# index exclusions\n\
                 waste.rs\n\
                 relative/exact.rs\n\
                 generated/\n\
                 wild/**/skip-?.rs\n\
                 {absolute_pattern}\n"
            ),
        )
        .unwrap();

        let ignore = IkIgnore::discover(dir.path(), &[]).unwrap();
        assert_eq!(ignore.files(), &[".ikignore"]);
        assert!(ignore.is_ignored("nested/waste.rs"));
        assert!(ignore.is_ignored("relative/exact.rs"));
        assert!(ignore.is_ignored("generated/deep/item.rs"));
        assert!(ignore.is_ignored("wild/skip-1.rs"));
        assert!(ignore.is_ignored("wild/deep/tree/skip-2.rs"));
        assert!(ignore.is_ignored("absolute/skip.rs"));
        assert!(!ignore.is_ignored("relative/other.rs"));
        assert!(!ignore.is_ignored("wild/deep/skip-long.rs"));
    }

    #[test]
    fn missing_file_matches_nothing() {
        let dir = tempdir().unwrap();
        let ignore = IkIgnore::discover(dir.path(), &[]).unwrap();
        assert!(ignore.files().is_empty());
        assert!(!ignore.is_ignored("src/lib.rs"));
    }

    #[test]
    fn nested_rules_are_relative_to_their_own_directory() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("packages").join("alpha")).unwrap();
        fs::create_dir_all(dir.path().join("packages").join("beta")).unwrap();
        fs::write(
            dir.path()
                .join("packages")
                .join("alpha")
                .join(IKIGNORE_FILE),
            "generated/\nlocal-?.rs\n./root-only.rs\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("packages").join("beta").join(IKIGNORE_FILE),
            "snapshots/**/*.snap\n",
        )
        .unwrap();

        let ignore = IkIgnore::discover(dir.path(), &[]).unwrap();
        assert_eq!(
            ignore.files(),
            &[
                "packages/alpha/.ikignore".to_string(),
                "packages/beta/.ikignore".to_string()
            ]
        );
        assert!(ignore.is_ignored("packages/alpha/generated/deep.rs"));
        assert!(ignore.is_ignored("packages/alpha/deep/local-1.rs"));
        assert!(ignore.is_ignored("packages/alpha/root-only.rs"));
        assert!(!ignore.is_ignored("packages/alpha/deep/root-only.rs"));
        assert!(!ignore.is_ignored("packages/beta/generated/deep.rs"));
        assert!(!ignore.is_ignored("packages/beta/local-1.rs"));
        assert!(ignore.is_ignored("packages/beta/snapshots/ui/home.snap"));
        assert!(!ignore.is_ignored("snapshots/ui/home.snap"));
    }

    #[test]
    fn absolute_ancestors_stop_at_the_filesystem_root() {
        assert_eq!(
            ancestors("/project/src/lib.rs").collect::<Vec<_>>(),
            vec!["/project/src/lib.rs", "/project/src", "/project", "/"]
        );
    }
}
