//! Integration tests for the language detector + heuristic chunker.

use ikode::lang::{
    chunk_markdown, chunk_source, count_braces, dedent_common, detect_language,
    keywordless_def_name, matches_keyword, parse_modifiers, reindent, Chunk,
};
use std::path::Path;

fn names(chunks: &[Chunk]) -> Vec<&str> {
    chunks.iter().map(|c| c.name.as_str()).collect()
}
fn kinds(chunks: &[Chunk]) -> Vec<&str> {
    chunks.iter().map(|c| c.kind.as_str()).collect()
}

#[test]
fn detects_languages_by_extension() {
    assert_eq!(detect_language(Path::new("a.rs")).unwrap().name, "Rust");
    assert_eq!(detect_language(Path::new("a.py")).unwrap().name, "Python");
    assert_eq!(
        detect_language(Path::new("a.ts")).unwrap().name,
        "TypeScript"
    );
    assert_eq!(detect_language(Path::new("a.go")).unwrap().name, "Go");
    assert_eq!(detect_language(Path::new("a.kt")).unwrap().name, "Kotlin");
    assert_eq!(detect_language(Path::new("a.swift")).unwrap().name, "Swift");
    assert_eq!(detect_language(Path::new("a.zig")).unwrap().name, "Zig");
    assert_eq!(detect_language(Path::new("a.php")).unwrap().name, "PHP");
    assert_eq!(detect_language(Path::new("a.rb")).unwrap().name, "Ruby");
    assert_eq!(detect_language(Path::new("a.scala")).unwrap().name, "Scala");
    assert_eq!(detect_language(Path::new("a.dart")).unwrap().name, "Dart");
    assert_eq!(
        detect_language(Path::new("a.sol")).unwrap().name,
        "Solidity"
    );
    assert_eq!(detect_language(Path::new("a.sh")).unwrap().name, "Text");
    assert_eq!(detect_language(Path::new("a.cs")).unwrap().name, "C#");
    assert_eq!(detect_language(Path::new("a.cpp")).unwrap().name, "C++");
    assert_eq!(detect_language(Path::new("a.md")).unwrap().name, "Markdown");
    assert_eq!(detect_language(Path::new("a.txt")).unwrap().name, "Text");
    // Project / config manifests are detected as "Config" (indexed whole-file).
    assert_eq!(
        detect_language(Path::new("Cargo.toml")).unwrap().name,
        "Config"
    );
    assert_eq!(
        detect_language(Path::new("App.csproj")).unwrap().name,
        "Config"
    );
    assert_eq!(
        detect_language(Path::new("Sln.sln")).unwrap().name,
        "Config"
    );
    assert_eq!(
        detect_language(Path::new("package.json")).unwrap().name,
        "Config"
    );
    assert_eq!(
        detect_language(Path::new("build.gradle")).unwrap().name,
        "Config"
    );
    // Markup / styling / single-file components are detected as "Markup" (whole-file).
    assert_eq!(
        detect_language(Path::new("index.html")).unwrap().name,
        "Markup"
    );
    assert_eq!(
        detect_language(Path::new("page.htm")).unwrap().name,
        "Markup"
    );
    assert_eq!(
        detect_language(Path::new("styles.css")).unwrap().name,
        "Markup"
    );
    assert_eq!(
        detect_language(Path::new("theme.scss")).unwrap().name,
        "Markup"
    );
    assert_eq!(
        detect_language(Path::new("App.vue")).unwrap().name,
        "Markup"
    );
    // case-insensitive extension
    assert_eq!(detect_language(Path::new("A.RS")).unwrap().name, "Rust");
    // unknown / extensionless
    assert!(detect_language(Path::new("a.bin")).is_none());
    assert!(detect_language(Path::new("noext")).is_none());
}

#[test]
fn chunks_rust_functions_structs_and_impls() {
    let src = "pub fn alpha() {\n    let x = 1;\n}\n\nstruct Point {\n    x: i32,\n}\n\nimpl Point {\n    fn beta(&self) {}\n}\n";
    let spec = detect_language(Path::new("x.rs")).unwrap();
    let chunks = chunk_source(&spec, src);
    let n = names(&chunks);
    let k = kinds(&chunks);
    assert!(n.contains(&"alpha"));
    assert!(n.contains(&"Point"));
    assert!(k.contains(&"Function"));
    assert!(k.contains(&"Struct"));
    assert!(k.contains(&"ImplBlock"));
    // beta lives inside the impl block: it is now its own Method chunk, parented to
    // the impl (not a top-level Function).
    let beta = chunks.iter().find(|c| c.name == "beta").unwrap();
    assert_eq!(beta.kind, "Method");
    let imp = chunks.iter().find(|c| c.kind == "ImplBlock").unwrap();
    assert_eq!(beta.parent_line, Some(imp.line_start));
    let alpha = chunks.iter().find(|c| c.name == "alpha").unwrap();
    assert_eq!(alpha.kind, "Function");
    assert_eq!(alpha.parent_line, None);
    assert_eq!(alpha.line_start, 1);
    assert_eq!(alpha.line_end, 3);
}

#[test]
fn single_line_braced_defs_are_one_chunk_each() {
    let src = "fn a() {}\nfn b() {}\n";
    let spec = detect_language(Path::new("x.rs")).unwrap();
    let chunks = chunk_source(&spec, src);
    assert_eq!(chunks.len(), 2);
    assert_eq!(names(&chunks), vec!["a", "b"]);
}

#[test]
fn chunks_python_by_indentation() {
    let src = "def foo():\n    return 1\n\nclass Bar:\n    def baz(self):\n        pass\n";
    let spec = detect_language(Path::new("x.py")).unwrap();
    let chunks = chunk_source(&spec, src);
    let n = names(&chunks);
    assert!(n.contains(&"foo"));
    assert!(n.contains(&"Bar"));
    // baz is nested under Bar: now its own Method chunk parented to the class.
    let baz = chunks.iter().find(|c| c.name == "baz").unwrap();
    assert_eq!(baz.kind, "Method");
    let bar = chunks.iter().find(|c| c.name == "Bar").unwrap();
    assert_eq!(baz.parent_line, Some(bar.line_start));
}

#[test]
fn chunks_markdown_by_heading() {
    let src = "# Title\n\nintro\n\n## Section A\n\ncontent a\n\n## Section B\n\ncontent b\n";
    let chunks = chunk_markdown(src);
    let n: Vec<String> = chunks.iter().map(|c| c.name.clone()).collect();
    assert!(n.contains(&"Title".to_string()));
    assert!(n.contains(&"Section A".to_string()));
    assert!(n.contains(&"Section B".to_string()));
    assert!(chunks.iter().all(|c| c.kind == "Section"));
}

#[test]
fn markdown_ignores_headings_inside_code_fences() {
    let src = "# Real\n\n```\n## fake heading\n```\n\nbody\n";
    let chunks = chunk_markdown(src);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].name, "Real");
}

#[test]
fn count_braces_handles_strings_and_comments() {
    assert_eq!(count_braces("let s = \"{{{\";"), (0, 0));
    assert_eq!(count_braces("fn f() { // }"), (1, 0));
    assert_eq!(count_braces("a { b } // { {"), (1, 1));
    assert_eq!(count_braces("plain line"), (0, 0));
}

#[test]
fn matches_keyword_strips_modifiers_and_extracts_name() {
    let (kind, name) = matches_keyword("pub async fn handler() {", &["fn"]).unwrap();
    assert_eq!(kind, "Function");
    assert_eq!(name, "handler");
    assert!(matches_keyword("let x = 1;", &["fn"]).is_none());
    assert!(matches_keyword("fnord()", &["fn"]).is_none()); // word boundary
}

#[test]
fn parse_modifiers_infers_visibility_and_flags() {
    let m = parse_modifiers("Rust", "foo", "pub async fn foo() {");
    assert_eq!(m.visibility, "public");
    assert!(m.is_async);
    assert!(!m.is_static);

    let m = parse_modifiers("Rust", "bar", "fn bar() {");
    assert_eq!(m.visibility, "private");

    let m = parse_modifiers("Rust", "helper", "pub(crate) fn helper() {");
    assert_eq!(m.visibility, "crate");

    let m = parse_modifiers("Python", "_hidden", "def _hidden(self):");
    assert_eq!(m.visibility, "private");
    let m = parse_modifiers("Python", "shown", "def shown(self):");
    assert_eq!(m.visibility, "public");

    let m = parse_modifiers("Java", "Foo", "public static final class Foo {");
    assert_eq!(m.visibility, "public");
    assert!(m.is_static);
    assert!(m.is_final);

    let m = parse_modifiers("C++", "render", "virtual void render() const {");
    assert!(m.is_virtual);
    assert!(m.is_const);
}

// --- Common-indent factoring (dedent_common / reindent) -------------------------

#[test]
fn dedent_common_extracts_shared_leading_spaces() {
    let code = "    fn beta(&self) {\n        let x = 1;\n    }";
    let (indent, body) = dedent_common(code);
    assert_eq!(indent, "    ");
    assert_eq!(body, "fn beta(&self) {\n    let x = 1;\n}");
}

#[test]
fn dedent_common_extracts_shared_leading_tabs() {
    // "if all code in fn and comments start with 2 tabs ... put that in indent"
    let code = "\t\t// doc\n\t\tfun f() {}\n\t\t\tbody()";
    let (indent, body) = dedent_common(code);
    assert_eq!(indent, "\t\t");
    assert_eq!(body, "// doc\nfun f() {}\n\tbody()");
}

#[test]
fn dedent_common_uses_the_minimum_shared_prefix() {
    // The signature line sits at 2 spaces; the body at 6. The common prefix is 2.
    let code = "  def foo():\n      return 1";
    let (indent, body) = dedent_common(code);
    assert_eq!(indent, "  ");
    assert_eq!(body, "def foo():\n    return 1");
}

#[test]
fn dedent_common_ignores_blank_lines_when_computing_prefix() {
    // The empty middle line must not collapse the shared indent to "".
    let code = "    a();\n\n    b();";
    let (indent, body) = dedent_common(code);
    assert_eq!(indent, "    ");
    assert_eq!(body, "a();\n\nb();");
}

#[test]
fn dedent_common_returns_empty_for_unindented_code() {
    let code = "fn top() {\n    inner();\n}";
    let (indent, body) = dedent_common(code);
    assert_eq!(indent, "");
    assert_eq!(body, code);
}

#[test]
fn dedent_common_bails_on_mixed_tabs_and_spaces() {
    // One line indented with a tab, the next with spaces -> no common whitespace prefix.
    let code = "\tfoo();\n    bar();";
    let (indent, body) = dedent_common(code);
    assert_eq!(indent, "");
    assert_eq!(body, code);
}

#[test]
fn dedent_common_handles_partial_shared_prefix() {
    // Four spaces vs six spaces share four.
    let code = "    x;\n      y;";
    let (indent, body) = dedent_common(code);
    assert_eq!(indent, "    ");
    assert_eq!(body, "x;\n  y;");
}

#[test]
fn reindent_is_the_inverse_of_dedent_common() {
    let originals = [
        "    fn beta(&self) {\n        let x = 1;\n    }",
        "\t\tfun f() {\n\t\t\tbody()\n\t\t}",
        "  def foo():\n      return 1\n      return 2",
        "no_indent();\n  child();",
        "    a();\n\n    b();",
    ];
    for code in originals {
        let (indent, body) = dedent_common(code);
        assert_eq!(
            reindent(&indent, &body),
            *code,
            "round-trip failed for {code:?}"
        );
    }
}

#[test]
fn reindent_with_empty_indent_is_identity() {
    let code = "line one\nline two";
    assert_eq!(reindent("", code), code);
}

#[test]
fn chunk_source_factors_indent_out_of_nested_methods() {
    // A method nested in an impl is indented 4 spaces; that prefix should move into
    // `indent` and be gone from the stored `code`, while line numbers are preserved.
    let src = "impl Point {\n    fn beta(&self) {\n        let x = 1;\n    }\n}\n";
    let spec = detect_language(Path::new("x.rs")).unwrap();
    let chunks = chunk_source(&spec, src);
    let beta = chunks.iter().find(|c| c.name == "beta").unwrap();
    assert_eq!(beta.indent, "    ");
    // Leading shared indent gone from the first line; the body keeps only its
    // *extra* indent (8 - 4 = 4 spaces).
    assert_eq!(beta.code, "fn beta(&self) {\n    let x = 1;\n}");
    // Reconstruction yields the original source slice (its lines 2..=4).
    let want = src.lines().skip(1).take(3).collect::<Vec<_>>().join("\n");
    assert_eq!(reindent(&beta.indent, &beta.code), want);
}

#[test]
fn chunk_source_leaves_top_level_chunks_unindented() {
    let src = "fn alpha() {\n    let x = 1;\n}\n";
    let spec = detect_language(Path::new("x.rs")).unwrap();
    let chunks = chunk_source(&spec, src);
    let alpha = chunks.iter().find(|c| c.name == "alpha").unwrap();
    assert_eq!(alpha.indent, "");
    assert!(alpha.code.contains("\n    let x = 1;"));
}

#[test]
fn chunk_source_does_not_dedent_single_line_chunks() {
    // Single-line defs have no second line, so we skip the factoring entirely.
    let src = "fn a() {}\nfn b() {}\n";
    let spec = detect_language(Path::new("x.rs")).unwrap();
    let chunks = chunk_source(&spec, src);
    assert!(chunks.iter().all(|c| c.indent.is_empty()));
}

#[test]
fn chunk_source_dedents_python_methods() {
    let src = "class Bar:\n    def baz(self):\n        return 1\n";
    let spec = detect_language(Path::new("x.py")).unwrap();
    let chunks = chunk_source(&spec, src);
    let baz = chunks.iter().find(|c| c.name == "baz").unwrap();
    assert_eq!(baz.indent, "    ");
    assert!(baz.code.starts_with("def baz"));
}

// --- Decoration backscan (attributes / annotations / doc comments) --------------

#[test]
fn rust_attribute_and_doc_comment_fold_into_the_chunk() {
    let src =
        "/// Adds two numbers.\n#[inline]\npub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";
    let spec = detect_language(Path::new("x.rs")).unwrap();
    let chunks = chunk_source(&spec, src);
    let add = chunks.iter().find(|c| c.name == "add").unwrap();
    // The chunk now begins at the doc comment, not the `fn` line.
    assert_eq!(add.line_start, 1);
    assert!(add
        .code
        .starts_with("/// Adds two numbers.\n#[inline]\npub fn add"));
    // Reconstruction is faithful to the on-disk slice [line_start, line_end].
    let want = src
        .lines()
        .take(add.line_end)
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(reindent(&add.indent, &add.code), want);
}

#[test]
fn rust_method_attribute_is_folded_and_dedented() {
    let src =
        "impl Svc {\n    /// docs\n    #[test]\n    fn checks(&self) {\n        ok();\n    }\n}\n";
    let spec = detect_language(Path::new("x.rs")).unwrap();
    let chunks = chunk_source(&spec, src);
    let m = chunks.iter().find(|c| c.name == "checks").unwrap();
    assert_eq!(m.kind, "Method");
    // Decoration starts at line 2 (the `/// docs` line), parented to the impl.
    assert_eq!(m.line_start, 2);
    let imp = chunks.iter().find(|c| c.kind == "ImplBlock").unwrap();
    assert_eq!(m.parent_line, Some(imp.line_start));
    // The shared 4-space indent (covering the doc, attribute, and body) is factored out.
    assert_eq!(m.indent, "    ");
    assert!(m.code.starts_with("/// docs\n#[test]\nfn checks"));
}

#[test]
fn python_decorator_folds_into_the_method() {
    let src = "class C:\n    @staticmethod\n    def make():\n        return C()\n";
    let spec = detect_language(Path::new("x.py")).unwrap();
    let chunks = chunk_source(&spec, src);
    let make = chunks.iter().find(|c| c.name == "make").unwrap();
    assert_eq!(make.line_start, 2); // the @staticmethod line
    assert!(make.code.starts_with("@staticmethod\ndef make"));
}

#[test]
fn csharp_attribute_folds_into_the_method() {
    let src = "class T {\n    [Fact]\n    public void Works() {\n        Assert();\n    }\n}\n";
    let spec = detect_language(Path::new("x.cs")).unwrap();
    let chunks = chunk_source(&spec, src);
    let m = chunks.iter().find(|c| c.name == "Works").unwrap();
    assert!(m.code.starts_with("[Fact]\npublic void Works"));
    assert_eq!(m.line_start, 2);
}

#[test]
fn inner_attributes_and_inner_doc_comments_are_not_folded() {
    // `#![...]` and `//!` decorate the enclosing module, not the next item.
    let src = "#![allow(dead_code)]\n//! crate docs\npub fn first() {\n    go();\n}\n";
    let spec = detect_language(Path::new("x.rs")).unwrap();
    let chunks = chunk_source(&spec, src);
    let f = chunks.iter().find(|c| c.name == "first").unwrap();
    assert_eq!(f.line_start, 3); // the `pub fn` line, not the inner attr/doc
    assert!(f.code.starts_with("pub fn first"));
}

#[test]
fn backscan_bridges_a_blank_between_decorations_but_not_a_lone_gap() {
    // Doc comment, blank, attribute, def -> the blank is bridged (all folded).
    let bridged = "/// doc\n\n#[inline]\nfn a() {\n    x();\n}\n";
    let spec = detect_language(Path::new("x.rs")).unwrap();
    let a = chunk_source(&spec, bridged)
        .into_iter()
        .find(|c| c.name == "a")
        .unwrap();
    assert_eq!(a.line_start, 1);

    // A lone blank directly above the def (no decoration beyond it) is NOT folded.
    let lone = "let z = 1;\n\nfn b() {\n    y();\n}\n";
    let b = chunk_source(&spec, lone)
        .into_iter()
        .find(|c| c.name == "b")
        .unwrap();
    assert_eq!(b.line_start, 3); // the `fn b` line
}

#[test]
fn backscan_does_not_cross_a_previous_sibling() {
    let src = "fn a() {\n    work();\n}\nfn b() {\n    more();\n}\n";
    let spec = detect_language(Path::new("x.rs")).unwrap();
    let chunks = chunk_source(&spec, src);
    let b = chunks.iter().find(|c| c.name == "b").unwrap();
    // b must start at its own `fn` line, not absorb a()'s closing brace.
    assert_eq!(b.line_start, 4);
    assert!(b.code.starts_with("fn b()"));
}

// --- Keyword-less method/function detection (Java / C# / C / C++) ----------------

#[test]
fn keywordless_def_name_accepts_methods_and_rejects_non_defs() {
    // Methods / functions with a return type + name.
    assert_eq!(
        keywordless_def_name("public void foo() {", Some("Bar")),
        Some("foo".to_string())
    );
    assert_eq!(
        keywordless_def_name("int main() {", None),
        Some("main".to_string())
    );
    assert_eq!(
        keywordless_def_name("double area();", Some("Shape")),
        Some("area".to_string())
    );
    assert_eq!(
        keywordless_def_name("public <T> T get(int i) {", Some("Box")),
        Some("get".to_string())
    );
    // A constructor has no return type but its name matches the container.
    assert_eq!(
        keywordless_def_name("Bar(int x) {", Some("Bar")),
        Some("Bar".to_string())
    );

    // Rejections: bare call, member-access call, field initialiser, control flow.
    assert_eq!(keywordless_def_name("foo();", Some("Bar")), None); // no return type, not ctor
    assert_eq!(keywordless_def_name("obj.method();", Some("Bar")), None); // member-access call
    assert_eq!(keywordless_def_name("int x = make();", Some("Bar")), None); // assignment
    assert_eq!(keywordless_def_name("if (x > 0) {", Some("Bar")), None);
    assert_eq!(
        keywordless_def_name("return compute(x);", Some("Bar")),
        None
    );
    assert_eq!(keywordless_def_name("while (true) {", Some("Bar")), None);
    assert_eq!(keywordless_def_name("int field;", Some("Bar")), None); // no parens
}

#[test]
fn chunks_java_class_methods_as_members() {
    let src = "public class Calc {\n    public int add(int a, int b) {\n        return a + b;\n    }\n    private void reset() {\n        total = 0;\n    }\n}\n";
    let spec = detect_language(Path::new("Calc.java")).unwrap();
    let chunks = chunk_source(&spec, src);
    let add = chunks.iter().find(|c| c.name == "add").unwrap();
    let reset = chunks.iter().find(|c| c.name == "reset").unwrap();
    assert_eq!(add.kind, "Method");
    assert_eq!(reset.kind, "Method");
    let calc = chunks.iter().find(|c| c.name == "Calc").unwrap();
    assert_eq!(add.parent_line, Some(calc.line_start));
    assert_eq!(reset.parent_line, Some(calc.line_start));
    // The method body's shared indent is factored out as usual.
    assert_eq!(add.indent, "    ");
}

#[test]
fn chunks_java_interface_method_declarations() {
    let src = "interface Shape {\n    double area();\n    double perimeter();\n}\n";
    let spec = detect_language(Path::new("Shape.java")).unwrap();
    let chunks = chunk_source(&spec, src);
    let n = names(&chunks);
    assert!(n.contains(&"area"));
    assert!(n.contains(&"perimeter"));
    assert!(chunks.iter().filter(|c| c.kind == "Method").count() == 2);
}

#[test]
fn chunks_cpp_class_methods_and_free_functions() {
    let src = "int helper(int x) {\n    return x * 2;\n}\nclass Widget {\n    void draw() {\n        render();\n    }\n};\n";
    let spec = detect_language(Path::new("w.cpp")).unwrap();
    let chunks = chunk_source(&spec, src);
    let helper = chunks.iter().find(|c| c.name == "helper").unwrap();
    assert_eq!(helper.kind, "Function"); // free function, top-level
    assert_eq!(helper.parent_line, None);
    let draw = chunks.iter().find(|c| c.name == "draw").unwrap();
    assert_eq!(draw.kind, "Method"); // class member
    let widget = chunks.iter().find(|c| c.name == "Widget").unwrap();
    assert_eq!(draw.parent_line, Some(widget.line_start));
}

#[test]
fn keywordless_detection_does_not_fire_for_keyworded_languages() {
    // Rust has `fn`; a bare `int foo()`-shaped line must not be picked up as a def
    // (and a Rust method call inside a body is not chunked).
    let src = "fn run() {\n    do_thing();\n}\n";
    let spec = detect_language(Path::new("x.rs")).unwrap();
    let chunks = chunk_source(&spec, src);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].name, "run");
}

#[test]
fn braced_file_with_no_defs_falls_back_to_plain() {
    let src = "let a = 1;\nlet b = 2;\n";
    let spec = detect_language(Path::new("x.rs")).unwrap();
    let chunks = chunk_source(&spec, src);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].kind, "Document");
}
