use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LangStyle {
    Braced,
    Indent,
    Markdown,
    Plain,
}

#[derive(Debug, Clone)]
pub struct LangSpec {
    pub name: &'static str,
    pub style: LangStyle,
    pub def_keywords: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub name: String,
    pub kind: String,
    pub line_start: usize,
    pub line_end: usize,
    /// Body text with the chunk's common leading-whitespace prefix (`indent`)
    /// stripped from every line, so the shared indentation isn't repeated on each
    /// line. Reconstruct the on-disk text with `reindent(&indent, &code)`.
    pub code: String,
    /// The longest leading-whitespace prefix shared by all non-blank lines of the
    /// chunk (e.g. `"\t\t"` for a body indented two tabs). Stored once instead of on
    /// every line of `code`. Empty for top-level / single-line chunks.
    pub indent: String,
    /// `line_start` of the enclosing chunk (a class/impl/trait/module), if this is a
    /// nested member. `None` for top-level chunks. Lets the indexer wire a
    /// `DEFINES` edge from the parent node rather than the File.
    pub parent_line: Option<usize>,
}

/// Longest common prefix of two leading-whitespace runs, by bytes (space/tab are
/// single-byte, so byte slicing is safe).
fn common_ws_prefix<'a>(a: &'a str, b: &str) -> &'a str {
    let n = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    &a[..n]
}

/// Split a chunk body into its common leading-whitespace prefix and the body with
/// that prefix removed from every line. Blank lines don't constrain the prefix and
/// are left empty in the output. Returns `(indent, dedented)`; reconstruct the
/// original (up to whitespace-only-line normalization) with `reindent`.
pub fn dedent_common(code: &str) -> (String, String) {
    let mut prefix: Option<&str> = None;
    for line in code.lines() {
        if line.trim().is_empty() {
            continue; // blank lines don't pin the common indent
        }
        let ws = &line[..line.len() - line.trim_start().len()];
        prefix = Some(match prefix {
            None => ws,
            Some(p) => common_ws_prefix(p, ws),
        });
        if prefix == Some("") {
            break; // can't shrink past empty
        }
    }
    let indent = prefix.unwrap_or("");
    if indent.is_empty() {
        return (String::new(), code.to_string());
    }
    let n = indent.len();
    let dedented = code
        .lines()
        .map(|l| if l.starts_with(indent) { &l[n..] } else { "" })
        .collect::<Vec<_>>()
        .join("\n");
    (indent.to_string(), dedented)
}

/// Inverse of `dedent_common`: re-prepend `indent` to every non-blank line of
/// `code`. With `indent` empty this is a no-op.
pub fn reindent(indent: &str, code: &str) -> String {
    if indent.is_empty() {
        return code.to_string();
    }
    code.lines()
        .map(|l| {
            if l.is_empty() {
                String::new()
            } else {
                format!("{indent}{l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn detect_language(path: &Path) -> Option<LangSpec> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())?;

    let spec = match ext.as_str() {
        "rs" => LangSpec {
            name: "Rust",
            style: LangStyle::Braced,
            def_keywords: &[
                "fn",
                "struct",
                "enum",
                "trait",
                "impl",
                "mod",
                "macro_rules!",
                "union",
                "type",
                "const",
                "static",
            ],
        },
        "py" | "pyi" => LangSpec {
            name: "Python",
            style: LangStyle::Indent,
            def_keywords: &["def", "class", "async def"],
        },
        "js" | "mjs" | "cjs" | "jsx" => LangSpec {
            name: "JavaScript",
            style: LangStyle::Braced,
            def_keywords: &[
                "function",
                "class",
                "async function",
                "export function",
                "export class",
                "export default function",
            ],
        },
        "ts" | "tsx" | "mts" | "cts" => LangSpec {
            name: "TypeScript",
            style: LangStyle::Braced,
            def_keywords: &[
                "function",
                "class",
                "interface",
                "enum",
                "type",
                "namespace",
                "export function",
                "export class",
                "export interface",
                "export enum",
            ],
        },
        "go" => LangSpec {
            name: "Go",
            style: LangStyle::Braced,
            def_keywords: &["func", "type"],
        },
        "kt" | "kts" => LangSpec {
            name: "Kotlin",
            style: LangStyle::Braced,
            def_keywords: &["fun", "class", "interface", "object", "enum"],
        },
        "swift" => LangSpec {
            name: "Swift",
            style: LangStyle::Braced,
            def_keywords: &["func", "class", "struct", "enum", "protocol", "extension"],
        },
        "zig" => LangSpec {
            name: "Zig",
            style: LangStyle::Braced,
            def_keywords: &["fn", "struct", "enum", "union", "const", "test"],
        },
        "php" | "phtml" => LangSpec {
            name: "PHP",
            style: LangStyle::Braced,
            def_keywords: &[
                "function",
                "class",
                "interface",
                "trait",
                "enum",
                "namespace",
            ],
        },
        "rb" => LangSpec {
            name: "Ruby",
            style: LangStyle::Indent,
            def_keywords: &["def", "class", "module"],
        },
        "scala" | "sc" => LangSpec {
            name: "Scala",
            style: LangStyle::Braced,
            def_keywords: &["def", "class", "object", "trait", "enum"],
        },
        "dart" => LangSpec {
            name: "Dart",
            style: LangStyle::Braced,
            def_keywords: &["class", "enum", "mixin", "extension"],
        },
        "sol" => LangSpec {
            name: "Solidity",
            style: LangStyle::Braced,
            def_keywords: &[
                "contract",
                "interface",
                "library",
                "function",
                "struct",
                "enum",
                "modifier",
            ],
        },
        "java" => LangSpec {
            name: "Java",
            style: LangStyle::Braced,
            def_keywords: &["class", "interface", "enum", "record"],
        },
        "cs" => LangSpec {
            name: "C#",
            style: LangStyle::Braced,
            def_keywords: &[
                "class",
                "interface",
                "enum",
                "struct",
                "record",
                "namespace",
            ],
        },
        "c" | "h" => LangSpec {
            name: "C",
            style: LangStyle::Braced,
            def_keywords: &["struct", "enum", "union", "typedef"],
        },
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => LangSpec {
            name: "C++",
            style: LangStyle::Braced,
            def_keywords: &["class", "struct", "enum", "union", "namespace", "template"],
        },
        "md" | "markdown" | "mdx" => LangSpec {
            name: "Markdown",
            style: LangStyle::Markdown,
            def_keywords: &[],
        },
        // Build / project / config manifests. Indexed whole-file (one chunk each) so
        // they participate in search, embeddings, and summaries — a repo may hold many
        // (`Cargo.toml`, `*.csproj`, `*.sln`, `package.json`, …) and each is content
        // worth retrieving, even though they carry no code structure.
        "toml" | "json" | "yaml" | "yml" | "xml" | "csproj" | "vbproj" | "fsproj" | "sln"
        | "props" | "targets" | "gradle" | "lock" | "cmake" | "cfg" | "ini" | "properties"
        | "plist" | "editorconfig" | "env" => LangSpec {
            name: "Config",
            style: LangStyle::Plain,
            def_keywords: &[],
        },
        // Markup, styling, and single-file components. These have no reliable
        // code-structure to chunk (and we deliberately avoid native HTML/CSS parsers),
        // so each is indexed whole-file like a config manifest — still searchable,
        // embeddable, and summarisable.
        "html" | "htm" | "xhtml" | "css" | "scss" | "sass" | "less" | "vue" | "svelte"
        | "astro" => LangSpec {
            name: "Markup",
            style: LangStyle::Plain,
            def_keywords: &[],
        },
        "txt" | "rst" | "sh" | "bash" | "zsh" | "sql" | "graphql" => LangSpec {
            name: "Text",
            style: LangStyle::Plain,
            def_keywords: &[],
        },
        _ => return None,
    };
    Some(spec)
}

pub fn chunk_source(spec: &LangSpec, source: &str) -> Vec<Chunk> {
    let mut chunks = match spec.style {
        LangStyle::Braced => chunk_braced(spec, source),
        LangStyle::Indent => chunk_indent(spec, source),
        LangStyle::Markdown => chunk_markdown(source),
        LangStyle::Plain => chunk_plain(source),
    };
    // Factor out each multi-line chunk's shared leading indentation into `indent`,
    // so the common prefix isn't stored on every line of `code`.
    for ch in &mut chunks {
        if ch.code.lines().nth(1).is_some() {
            let (indent, dedented) = dedent_common(&ch.code);
            ch.indent = indent;
            ch.code = dedented;
        }
    }
    chunks
}

/// Inferred visibility + modifiers for a definition, parsed heuristically from its
/// signature line. Best-effort across languages — no inference, just keyword scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Modifiers {
    pub visibility: &'static str, // public | private | protected | internal | crate | default
    pub is_static: bool,
    pub is_const: bool,
    pub is_async: bool,
    pub is_abstract: bool,
    pub is_virtual: bool,
    pub is_final: bool,
}

/// Infer visibility/modifiers from a definition's first line. `lang` and `name`
/// drive language-specific defaults (Rust items are private unless `pub`; Python
/// uses a leading underscore as the private convention).
pub fn parse_modifiers(lang: &str, name: &str, line: &str) -> Modifiers {
    // Scan the signature minus the parameter list: keep the leading modifiers and
    // any trailing qualifiers (C++ `const`/`override`), but drop param names so a
    // param called `static` can't be mistaken for a modifier.
    let sig = line.split('{').next().unwrap_or(line);
    let scan = match (sig.find('('), sig.rfind(')')) {
        (Some(s), Some(e)) if e > s => format!("{} {}", &sig[..s], &sig[e + 1..]),
        _ => sig.to_string(),
    };
    let scan = scan.split('=').next().unwrap_or(&scan);
    let toks: Vec<&str> = scan
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|s| !s.is_empty())
        .collect();
    let has = |w: &str| toks.contains(&w);

    let visibility = if line.contains("pub(crate)") || line.contains("pub(super)") {
        "crate"
    } else if has("pub") || has("public") {
        "public"
    } else if has("private") {
        "private"
    } else if has("protected") {
        "protected"
    } else if has("internal") {
        "internal"
    } else {
        match lang {
            "Rust" => "private",
            "Python" => {
                if name.starts_with('_') {
                    "private"
                } else {
                    "public"
                }
            }
            _ => "default",
        }
    };

    Modifiers {
        visibility,
        is_static: has("static"),
        is_const: has("const") || has("constexpr"),
        is_async: has("async"),
        is_abstract: has("abstract"),
        is_virtual: has("virtual"),
        is_final: has("final") || has("sealed"),
    }
}

pub fn matches_keyword(line: &str, keywords: &[&str]) -> Option<(String, String)> {
    let trimmed = line.trim_start();
    let trimmed = trimmed
        .trim_start_matches("pub ")
        .trim_start_matches("pub(crate) ")
        .trim_start_matches("export ")
        .trim_start_matches("default ")
        .trim_start_matches("async ")
        .trim_start_matches("static ")
        .trim_start_matches("public ")
        .trim_start_matches("private ")
        .trim_start_matches("protected ")
        .trim_start_matches("abstract ")
        .trim_start_matches("final ")
        .trim_start_matches("case ")
        .trim_start_matches("sealed ")
        .trim_start_matches("open ")
        .trim_start_matches("override ")
        .trim_start_matches("suspend ")
        .trim_start_matches("data ");

    for kw in keywords {
        let kw = kw
            .trim_start_matches("export ")
            .trim_start_matches("async ")
            .trim_start_matches("default ");
        if let Some(rest) = trimmed.strip_prefix(kw) {
            let boundary_ok =
                rest.is_empty() || rest.starts_with(|c: char| !c.is_alphanumeric() && c != '_');
            if boundary_ok {
                let name = rest
                    .trim_start()
                    .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '!')
                    .find(|s| !s.is_empty())
                    .unwrap_or("")
                    .to_string();
                let kind = keyword_kind(kw);
                return Some((kind, name));
            }
        }
    }
    None
}

fn keyword_kind(kw: &str) -> String {
    match kw {
        "fn" | "func" | "fun" | "function" | "def" | "async def" => "Function",
        "struct" => "Struct",
        "enum" => "Enum",
        "trait" | "protocol" => "Trait",
        "interface" => "Interface",
        "impl" | "extension" => "ImplBlock",
        "class" | "record" | "object" | "contract" => "Class",
        "mixin" => "Trait",
        "mod" | "namespace" | "module" | "library" => "Module",
        "type" | "typedef" => "TypeAlias",
        "union" => "Union",
        "const" | "static" => "Const",
        "modifier" => "Function",
        "template" => "Template",
        m if m.starts_with("macro_rules") => "Macro",
        _ => "Definition",
    }
    .to_string()
}

/// A kind whose body holds member definitions worth descending into.
fn is_container_kind(kind: &str) -> bool {
    members_are_methods(kind) || matches!(kind, "Module")
}

/// A container whose nested functions are *methods* (a type/trait/impl), as opposed
/// to a `Module`/namespace whose nested functions stay free `Function`s.
fn members_are_methods(kind: &str) -> bool {
    matches!(
        kind,
        "Class" | "Struct" | "Enum" | "Trait" | "Interface" | "ImplBlock" | "Union"
    )
}

pub fn count_braces(line: &str) -> (i32, i32) {
    let mut opens = 0;
    let mut closes = 0;
    let mut in_str = false;
    let mut str_ch = ' ';
    let mut prev = ' ';
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_str {
            if c == str_ch && prev != '\\' {
                in_str = false;
            }
            prev = c;
            continue;
        }
        match c {
            '/' if chars.peek() == Some(&'/') => break,
            '"' | '\'' | '`' => {
                in_str = true;
                str_ch = c;
            }
            '{' => opens += 1,
            '}' => closes += 1,
            _ => {}
        }
        prev = c;
    }
    (opens, closes)
}

/// Languages whose class members (and free functions) carry no leading definition
/// keyword (`fn`/`def`/…), so a method must be recognised from its signature shape
/// instead of a keyword. Java/C#/C/C++ are the known gaps.
fn keywordless_methods(spec: &LangSpec) -> bool {
    matches!(spec.name, "Java" | "C#" | "C" | "C++")
}

/// Statement / control-flow keywords that can be followed by `(` but never *name* a
/// definition. Used to reject false positives (`if (...)`, `return foo()`) in
/// keyword-less method detection.
const NON_DEF_KEYWORDS: &[&str] = &[
    "if",
    "for",
    "while",
    "switch",
    "catch",
    "return",
    "sizeof",
    "new",
    "delete",
    "synchronized",
    "else",
    "do",
    "throw",
    "case",
    "goto",
    "using",
    "namespace",
    "typeof",
    "await",
    "assert",
    "yield",
    "lock",
    "foreach",
    "with",
];

/// True if `s` is a bare code identifier.
fn is_ident(s: &str) -> bool {
    let mut cs = s.chars();
    matches!(cs.next(), Some(c) if c.is_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Heuristically detect a keyword-less function/method signature for Java/C#/C/C++
/// (e.g. `public void foo() {`, `int bar(int x);`, a constructor `Foo()`, or an
/// abstract/interface method `double area();`). Returns the definition name, or
/// `None` for calls, field initialisers, and control-flow lines. `container` is the
/// enclosing type's name so a constructor (name == container) is accepted despite
/// having no return type.
pub fn keywordless_def_name(line: &str, container: Option<&str>) -> Option<String> {
    let t = line.trim();
    // The method names itself immediately before its parameter list's `(`.
    let lparen = t.find('(')?;
    let head = &t[..lparen];
    // An assignment / field initialiser (`int x = make()`) is not a definition.
    if head.contains('=') {
        return None;
    }
    // The part before `(` is `<return type / modifiers> <name>`; the name is the
    // last identifier token, earlier tokens are the return type and modifiers.
    let toks: Vec<&str> = head
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|s| !s.is_empty())
        .collect();
    let name = (*toks.last()?).to_string();
    if !is_ident(&name) || name.chars().next().is_some_and(|c| c.is_numeric()) {
        return None;
    }
    // Reject member-access calls like `obj.method(` (a `.` directly before the name).
    let name_at = head.rfind(&name)?;
    if head[..name_at].trim_end().ends_with('.') {
        return None;
    }
    let is_ctor = container == Some(name.as_str());
    // A real definition has a return type / modifier before the name (`void foo`);
    // a bare `foo(` with nothing before it is a call, not a definition. Constructors
    // are the exception.
    if toks.len() < 2 && !is_ctor {
        return None;
    }
    // No leading token (nor the name itself) may be a control-flow keyword.
    if toks.iter().any(|tk| NON_DEF_KEYWORDS.contains(tk)) {
        return None;
    }
    Some(name)
}

/// A single line that decorates the definition just below it — an attribute,
/// annotation/decorator, or comment — which is folded into that chunk so the
/// definition stays semantically whole. `hash_comment` enables `#`-style comments
/// (Python/Ruby). Inner attributes (`#![…]`) and inner doc comments (`//!`) belong
/// to the enclosing module, not the next item, so they are deliberately excluded.
fn is_decoration_line(t: &str, hash_comment: bool) -> bool {
    if t.starts_with("#![") || t.starts_with("//!") {
        return false;
    }
    t.starts_with("#[")            // Rust outer attribute
        || t.starts_with("///")     // outer doc comment
        || t.starts_with("//")      // line comment
        || t.starts_with("/*")      // block comment open / single-line
        || t.starts_with('*')       // block comment body / close (`*/`)
        || t.starts_with('@')       // decorator / annotation (Python/Java/TS/…)
        || (t.starts_with('[') && t.ends_with(']')) // C# attribute
        || (hash_comment && t.starts_with('#')) // Python/Ruby comment
}

/// Walk upward from a definition at `def_line`, absorbing the contiguous block of
/// decoration lines (attributes/annotations/comments) directly above it — and any
/// blank lines that merely bridge two decoration lines — down to `floor`. Returns
/// the new start index (the topmost decoration), or `def_line` if there is none.
fn backscan_decorations(
    lines: &[&str],
    def_line: usize,
    floor: usize,
    hash_comment: bool,
) -> usize {
    let mut candidate = def_line;
    let mut i = def_line;
    while i > floor {
        i -= 1;
        let t = lines[i].trim();
        if t.is_empty() {
            continue; // a blank bridges to decorations above, but is not itself a stop
        }
        if is_decoration_line(t, hash_comment) {
            candidate = i;
        } else {
            break; // hit real code (a previous sibling, or the container's own line)
        }
    }
    candidate
}

fn chunk_braced(spec: &LangSpec, source: &str) -> Vec<Chunk> {
    let lines: Vec<&str> = source.lines().collect();
    let mut chunks = Vec::new();
    extract_braced(spec, &lines, 0, lines.len(), None, false, None, &mut chunks);
    if chunks.is_empty() {
        chunks.extend(chunk_plain(source));
    }
    chunks
}

/// Scan `lines[lo..hi]` for top-level definitions (relative to this scope's brace
/// depth) and append them to `out`. For container definitions (class/impl/trait/…)
/// recurse into the body so members become their own chunks. `parent_line` is the
/// enclosing chunk's start line; `member_context` is true when a nested `Function`
/// should be recorded as a `Method`.
#[allow(clippy::too_many_arguments)] // The range and nesting context are explicit parser state.
fn extract_braced(
    spec: &LangSpec,
    lines: &[&str],
    lo: usize,
    hi: usize,
    parent_line: Option<usize>,
    member_context: bool,
    container_name: Option<&str>,
    out: &mut Vec<Chunk>,
) {
    let mut depth: i32 = 0;
    let mut i = lo;

    while i < hi {
        let line = lines[i];
        if depth == 0 {
            // A definition is either a keyword form (`fn`/`class`/…) or, for
            // keyword-less languages (Java/C#/C/C++), a bare method/function
            // signature recognised by its shape.
            let detected = matches_keyword(line, spec.def_keywords).or_else(|| {
                if keywordless_methods(spec) {
                    keywordless_def_name(line, container_name).map(|n| ("Function".to_string(), n))
                } else {
                    None
                }
            });
            if let Some((raw_kind, name)) = detected {
                let def_line = i;
                // Fold any attributes / annotations / doc comments sitting directly
                // above the definition into this chunk. Brace counting still starts
                // at the definition line, never the comments (a `{` inside a block
                // comment must not perturb the balance).
                let start =
                    backscan_decorations(lines, def_line, lo, spec.style == LangStyle::Indent);
                let mut local_depth = 0i32;
                let mut seen_brace = false;
                let mut body_open: Option<usize> = None;
                let mut end = def_line;
                let mut j = def_line;
                while j < hi {
                    let (o, c) = count_braces(lines[j]);
                    if o > 0 && !seen_brace {
                        body_open = Some(j);
                        seen_brace = true;
                    }
                    local_depth += o - c;
                    end = j;
                    if seen_brace && local_depth <= 0 {
                        break;
                    }
                    if !seen_brace && lines[j].trim_end().ends_with(';') {
                        break;
                    }
                    j += 1;
                }
                let kind = if member_context && raw_kind == "Function" {
                    "Method".to_string()
                } else {
                    raw_kind
                };
                let code = lines[start..=end.min(lines.len() - 1)].join("\n");
                let this_line = start + 1;
                let own_name = if name.is_empty() {
                    format!("{}@{}", kind, start + 1)
                } else {
                    name
                };
                out.push(Chunk {
                    name: own_name.clone(),
                    kind: kind.clone(),
                    line_start: this_line,
                    line_end: end + 1,
                    code,
                    indent: String::new(),
                    parent_line,
                });
                // Descend into the body of a container so its members are chunked.
                if seen_brace && is_container_kind(&kind) {
                    if let Some(bo) = body_open {
                        if bo + 1 < end {
                            extract_braced(
                                spec,
                                lines,
                                bo + 1,
                                end,
                                Some(this_line),
                                members_are_methods(&kind),
                                Some(own_name.as_str()),
                                out,
                            );
                        }
                    }
                }
                i = end + 1;
                continue;
            }
        }
        let (o, c) = count_braces(line);
        depth += o - c;
        if depth < 0 {
            depth = 0;
        }
        i += 1;
    }
}

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

fn chunk_indent(spec: &LangSpec, source: &str) -> Vec<Chunk> {
    let lines: Vec<&str> = source.lines().collect();
    let mut chunks = Vec::new();
    extract_indent(spec, &lines, 0, lines.len(), None, false, &mut chunks);
    if chunks.is_empty() {
        chunks.extend(chunk_plain(source));
    }
    chunks
}

/// Indentation-based analogue of `extract_braced`: scan `lines[lo..hi]` for
/// definitions at the shallowest indent of this scope and, for containers
/// (class/module), recurse into the more-indented body so members are chunked.
fn extract_indent(
    spec: &LangSpec,
    lines: &[&str],
    lo: usize,
    hi: usize,
    parent_line: Option<usize>,
    member_context: bool,
    out: &mut Vec<Chunk>,
) {
    let mut i = lo;

    while i < hi {
        let line = lines[i];
        if line.trim().is_empty() {
            i += 1;
            continue;
        }
        let base_indent = indent_of(line);
        if let Some((raw_kind, name)) = matches_keyword(line, spec.def_keywords) {
            let def_line = i;
            // Fold decorators / comments sitting directly above the definition into
            // the chunk. The body extent below is still measured from the definition
            // line (so its indentation, not a comment's, sets the scope).
            let start = backscan_decorations(lines, def_line, lo, true);
            let mut end = def_line;
            let mut j = def_line + 1;
            while j < hi {
                let l = lines[j];
                if l.trim().is_empty() {
                    j += 1;
                    continue;
                }
                if indent_of(l) <= base_indent {
                    break;
                }
                end = j;
                j += 1;
            }
            let kind = if member_context && raw_kind == "Function" {
                "Method".to_string()
            } else {
                raw_kind
            };
            let code = lines[start..=end.min(lines.len() - 1)].join("\n");
            let this_line = start + 1;
            out.push(Chunk {
                name: if name.is_empty() {
                    format!("{}@{}", kind, start + 1)
                } else {
                    name
                },
                kind: kind.clone(),
                line_start: this_line,
                line_end: end + 1,
                code,
                indent: String::new(),
                parent_line,
            });
            if is_container_kind(&kind) && end > def_line {
                extract_indent(
                    spec,
                    lines,
                    def_line + 1,
                    end + 1,
                    Some(this_line),
                    members_are_methods(&kind),
                    out,
                );
            }
            i = end + 1;
            continue;
        }
        i += 1;
    }
}

pub fn chunk_markdown(source: &str) -> Vec<Chunk> {
    let lines: Vec<&str> = source.lines().collect();
    let mut chunks = Vec::new();
    let mut start = 0usize;
    let mut current_name = String::from("(preamble)");
    let mut in_fence = false;

    let flush = |chunks: &mut Vec<Chunk>, name: &str, s: usize, e: usize, lines: &[&str]| {
        if e < s {
            return;
        }
        let code = lines[s..=e.min(lines.len() - 1)].join("\n");
        if code.trim().is_empty() {
            return;
        }
        chunks.push(Chunk {
            name: name.to_string(),
            kind: "Section".to_string(),
            line_start: s + 1,
            line_end: e + 1,
            code,
            indent: String::new(),
            parent_line: None,
        });
    };

    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence && t.starts_with('#') {
            let level = t.chars().take_while(|c| *c == '#').count();
            if (1..=6).contains(&level) && t[level..].starts_with(' ') {
                if i > start {
                    flush(&mut chunks, &current_name, start, i - 1, &lines);
                }
                current_name = t[level..].trim().to_string();
                start = i;
            }
        }
    }
    if !lines.is_empty() {
        flush(&mut chunks, &current_name, start, lines.len() - 1, &lines);
    }

    if chunks.is_empty() {
        chunks.extend(chunk_plain(source));
    }
    chunks
}

fn chunk_plain(source: &str) -> Vec<Chunk> {
    let line_count = source.lines().count().max(1);
    vec![Chunk {
        name: "(file)".to_string(),
        // "Document", not "File": the whole-file fallback chunk must not share the
        // "File" node label (reserved for the File node itself), or it inflates the
        // File count in /graph and muddles containment.
        kind: "Document".to_string(),
        line_start: 1,
        line_end: line_count,
        code: source.to_string(),
        indent: String::new(),
        parent_line: None,
    }]
}
