//! M20c in-process regression tests for the `json` and `re` stdlib
//! modules.  Follows the M19/M20a/M20b pattern: compile a tiny `.spy`
//! snippet, write the bytecode to `CARGO_TARGET_TMPDIR`, run through
//! `run_file_capture`, and assert on stdout.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m20c_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── json module ────────────────────────────────────────────────────────

#[test]
fn json_parse_to_string_canonicalises() {
    let src = "\
import json
fn main() -> i32:
    s: str = \"{\\\"b\\\": 2, \\\"a\\\": 1}\"
    out: str = json.parse_to_string(s)
    println(out)
    return 0
";
    let p = compile_snippet("json_canonical", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // serde_json's BTreeMap default sorts keys lexicographically.
    assert!(out.contains("{\"a\":1,\"b\":2}"), "got: {out:?}");
}

#[test]
fn json_is_valid_predicate() {
    let src = "\
import json
fn main() -> i32:
    if json.is_valid(\"[1, 2, 3]\"):
        println(\"good\")
    if json.is_valid(\"not json\"):
        println(\"bad-thought-good\")
    else:
        println(\"bad-rejected\")
    return 0
";
    let p = compile_snippet("json_is_valid", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("good"));
    assert!(out.contains("bad-rejected"));
    assert!(!out.contains("bad-thought-good"));
}

#[test]
fn json_pretty_emits_indented_output() {
    let src = "\
import json
fn main() -> i32:
    s: str = \"{\\\"a\\\":1,\\\"b\\\":[2,3]}\"
    println(json.pretty(s, 2i32))
    return 0
";
    let p = compile_snippet("json_pretty", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // Two-space indent; the nested array should be on its own lines.
    assert!(out.contains("{\n  \"a\":"), "got: {out:?}");
    assert!(out.contains("  \"b\": [\n    2,\n    3\n  ]"), "got: {out:?}");
}

#[test]
fn json_pretty_indent_zero_collapses_to_compact() {
    let src = "\
import json
fn main() -> i32:
    s: str = \"{\\\"a\\\":1}\"
    out: str = json.pretty(s, 0i32)
    println(out)
    return 0
";
    let p = compile_snippet("json_pretty_zero", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("{\"a\":1}"), "got: {out:?}");
}

#[test]
fn json_escape_round_trips() {
    let src = "\
import json
fn main() -> i32:
    raw: str = \"hello\\nworld\"
    esc: str = json.escape(raw)
    println(esc)
    return 0
";
    let p = compile_snippet("json_escape", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // Output should contain quoted form with \n escape.
    assert!(out.contains("\"hello\\nworld\""), "got: {out:?}");
}

#[test]
fn json_parse_raises_value_error_on_bad_input() {
    let src = "\
import json
fn main() -> i32:
    try:
        s: str = json.parse_to_string(\"oops\")
        println(\"UNREACHED \" + s)
    except ValueError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("json_bad", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

#[test]
fn json_minify_alias_matches_parse_to_string() {
    let src = "\
import json
fn main() -> i32:
    s: str = \"[1,  2,   3]\"
    a: str = json.parse_to_string(s)
    b: str = json.minify(s)
    if a == b:
        println(\"same\")
    else:
        println(\"different a=\" + a + \" b=\" + b)
    return 0
";
    let p = compile_snippet("json_minify", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("same"), "got: {out:?}");
}

// ── re module ──────────────────────────────────────────────────────────

#[test]
fn re_fullmatch_anchored_both_ends() {
    let src = "\
import re
fn main() -> i32:
    if re.fullmatch(\"^[a-z]+$\", \"hello\"):
        println(\"yes-full\")
    if re.fullmatch(\"[a-z]+\", \"abc123\"):
        println(\"NO-partial-should-not-match\")
    else:
        println(\"good-partial-rejected\")
    return 0
";
    let p = compile_snippet("re_fullmatch", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("yes-full"));
    assert!(out.contains("good-partial-rejected"));
    assert!(!out.contains("NO-partial-should-not-match"));
}

#[test]
fn re_search_matches_anywhere() {
    let src = "\
import re
fn main() -> i32:
    if re.search(\"[0-9]+\", \"abc123def\"):
        println(\"found\")
    if re.search(\"[0-9]+\", \"abcdef\"):
        println(\"BAD-not-found\")
    else:
        println(\"none\")
    return 0
";
    let p = compile_snippet("re_search", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("found"));
    assert!(out.contains("none"));
}

#[test]
fn re_find_returns_tuple_with_positions() {
    let src = "\
import re
fn main() -> i32:
    pos: Tuple[i32, i32] = re.find(\"[0-9]+\", \"abc123def\")
    println(\"start=\" + str(pos.0))
    println(\"end=\" + str(pos.1))
    miss: Tuple[i32, i32] = re.find(\"xyz\", \"abc\")
    println(\"miss-start=\" + str(miss.0))
    println(\"miss-end=\" + str(miss.1))
    return 0
";
    let p = compile_snippet("re_find", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("start=3"), "got: {out:?}");
    assert!(out.contains("end=6"), "got: {out:?}");
    assert!(out.contains("miss-start=-1"), "got: {out:?}");
    assert!(out.contains("miss-end=-1"), "got: {out:?}");
}

#[test]
fn re_find_all_collects_non_overlapping() {
    let src = "\
import re
fn main() -> i32:
    ws: List[str] = re.find_all(\"[a-z]+\", \"abc 123 def 456 ghi\")
    println(\"n=\" + str(len(ws)))
    i: i64 = 0
    while i < i64(len(ws)):
        println(ws[i])
        i = i + 1
    return 0
";
    let p = compile_snippet("re_find_all", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("n=3"), "got: {out:?}");
    assert!(out.contains("abc"));
    assert!(out.contains("def"));
    assert!(out.contains("ghi"));
}

#[test]
fn re_replace_global_substitution() {
    let src = "\
import re
fn main() -> i32:
    out: str = re.replace(\"[0-9]\", \"X\", \"a1b2c3\")
    println(out)
    return 0
";
    let p = compile_snippet("re_replace", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("aXbXcX"), "got: {out:?}");
}

#[test]
fn re_split_basic() {
    let src = "\
import re
fn main() -> i32:
    parts: List[str] = re.split(\",\\\\s*\", \"a, b, c, d\")
    println(\"n=\" + str(len(parts)))
    i: i64 = 0
    while i < i64(len(parts)):
        println(parts[i])
        i = i + 1
    return 0
";
    let p = compile_snippet("re_split", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("n=4"), "got: {out:?}");
    assert!(out.contains("a\n"), "got: {out:?}");
    assert!(out.contains("d\n"), "got: {out:?}");
}

#[test]
fn re_is_valid_predicate() {
    let src = "\
import re
fn main() -> i32:
    if re.is_valid(\"[a-z]+\"):
        println(\"good\")
    if re.is_valid(\"[bad\"):
        println(\"BAD-thought-good\")
    else:
        println(\"bad-rejected\")
    return 0
";
    let p = compile_snippet("re_is_valid", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("good"));
    assert!(out.contains("bad-rejected"));
}

#[test]
fn re_bad_pattern_raises_value_error() {
    let src = "\
import re
fn main() -> i32:
    try:
        unused: bool = re.search(\"[invalid\", \"x\")
        println(\"UNREACHED \" + str(unused))
    except ValueError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("re_bad", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

#[test]
fn re_replace_with_backreferences() {
    // `$1` backreference — feature-flag for compat with Python's
    // `\1` syntax (which regex-crate also accepts as `${1}` style).
    let src = "\
import re
fn main() -> i32:
    out: str = re.replace(\"(\\\\w+)@(\\\\w+)\", \"$2/$1\", \"alice@example\")
    println(out)
    return 0
";
    let p = compile_snippet("re_replace_back", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("example/alice"), "got: {out:?}");
}
