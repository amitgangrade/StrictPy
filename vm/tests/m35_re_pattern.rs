//! M35 P4-A integration tests for the compiled `re.Pattern` class.
//!
//! Each test compiles a small `.spy` snippet, runs it through
//! `run_file_capture`, and asserts on stdout.  The tests exercise:
//!   * `re.compile()` returning a Pattern handle
//!   * `Pattern.matches()` for full-string match
//!   * `Pattern.find()` returning an optional match string
//!   * `Pattern.find_all()` over a multi-match input
//!   * `Pattern.replace()` first-match only
//!   * `Pattern.replace_all()` against multi-match input
//!   * `Pattern.split()` for tokenisation
//!   * `Pattern.source()` round-trip
//!   * The compile-once-reuse hot-loop idiom (the perf-motivating use
//!     case)
//!   * Bad-regex `ValueError` from `re.compile`
//!
//! End-to-end tests by design — M35 P4-A touches the resolver, the IR,
//! the codegen, and the VM in one go.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m35_p4a_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn compile_and_match_success() {
    let src = r#"
import re

fn main() -> i32:
    p: Pattern = re.compile("a.*b")
    if p.matches("axxxb"):
        println("match")
    else:
        println("no")
    return 0
"#;
    let p = compile_snippet("compile_and_match_success", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; out={out}");
    assert!(out.contains("match"), "got: {out:?}");
}

#[test]
fn compile_and_no_match() {
    let src = r#"
import re

fn main() -> i32:
    p: Pattern = re.compile("^[0-9]+$")
    if p.matches("hello"):
        println("yes")
    else:
        println("no")
    return 0
"#;
    let p = compile_snippet("compile_and_no_match", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; out={out}");
    assert!(out.contains("no"), "got: {out:?}");
}

#[test]
fn find_all_multiple_matches() {
    let src = r#"
import re

fn main() -> i32:
    p: Pattern = re.compile("\\d+")
    xs: List[str] = p.find_all("a1 b22 c333")
    println("count=" + str(len(xs)))
    println("e0=" + xs[0])
    println("e1=" + xs[1])
    println("e2=" + xs[2])
    return 0
"#;
    let p = compile_snippet("find_all_multiple_matches", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; out={out}");
    assert!(out.contains("count=3"), "want length 3, got: {out:?}");
    assert!(out.contains("e0=1"), "want '1', got: {out:?}");
    assert!(out.contains("e1=22"), "want '22', got: {out:?}");
    assert!(out.contains("e2=333"), "want '333', got: {out:?}");
}

#[test]
fn replace_all_substitution() {
    let src = r#"
import re

fn main() -> i32:
    p: Pattern = re.compile("[0-9]")
    out: str = p.replace_all("a1b2c3", "X")
    println(out)
    return 0
"#;
    let p = compile_snippet("replace_all_substitution", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; out={out}");
    assert!(out.contains("aXbXcX"), "got: {out:?}");
}

#[test]
fn split_on_whitespace() {
    let src = r#"
import re

fn main() -> i32:
    p: Pattern = re.compile("\\s+")
    parts: List[str] = p.split("hello   world  foo")
    println("count=" + str(len(parts)))
    println("a=" + parts[0])
    println("b=" + parts[1])
    println("c=" + parts[2])
    return 0
"#;
    let p = compile_snippet("split_on_whitespace", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; out={out}");
    assert!(out.contains("count=3"), "want length 3, got: {out:?}");
    assert!(out.contains("a=hello"), "got: {out:?}");
    assert!(out.contains("b=world"), "got: {out:?}");
    assert!(out.contains("c=foo"), "got: {out:?}");
}

#[test]
fn bad_regex_raises_value_error() {
    let src = r#"
import re

fn main() -> i32:
    try:
        p: Pattern = re.compile("(unclosed")
        println("compiled-bad")
    except ValueError as e:
        println("caught-" + e.type_name)
    return 0
"#;
    let p = compile_snippet("bad_regex_raises_value_error", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; out={out}");
    assert!(out.contains("caught-ValueError"), "got: {out:?}");
    assert!(!out.contains("compiled-bad"), "got: {out:?}");
}

#[test]
fn source_round_trip() {
    let src = r#"
import re

fn main() -> i32:
    p: Pattern = re.compile("hello.*world")
    println(p.source())
    return 0
"#;
    let p = compile_snippet("source_round_trip", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; out={out}");
    assert!(out.contains("hello.*world"), "got: {out:?}");
}

#[test]
fn compile_once_reuse_hot_loop() {
    // The perf-motivating use case: compile a regex once, apply it to
    // many inputs in a hot loop.  Verifies the slot-table reuse works
    // (a fresh `re.find_all(...)` recompiles the pattern on every
    // call; the Pattern handle should not).
    let src = r#"
import re

fn main() -> i32:
    p: Pattern = re.compile("[A-Z]+")
    inputs: List[str] = ["foo BAR baz", "QUX quux", "no caps", "MIXED case ABC"]
    total: i32 = 0
    for s: str in inputs:
        hits: List[str] = p.find_all(s)
        total = total + (i32(len(hits)))
    println("total=" + str(total))
    return 0
"#;
    let p = compile_snippet("compile_once_reuse_hot_loop", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; out={out}");
    // BAR (1) + QUX (1) + MIXED (1) + ABC (1) = 4
    assert!(out.contains("total=4"), "want 'total=4', got: {out:?}");
}

#[test]
fn find_returns_optional() {
    // Pattern.find returns str? — match present yields the matched
    // text, absent yields none.
    let src = r#"
import re

fn main() -> i32:
    p: Pattern = re.compile("\\d+")
    a: str? = p.find("the answer is 42")
    b: str? = p.find("no digits here")
    if a is not none:
        println(a)
    else:
        println("a-none")
    if b is none:
        println("b-none")
    else:
        println(b)
    return 0
"#;
    let p = compile_snippet("find_returns_optional", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; out={out}");
    assert!(out.contains("42"), "got: {out:?}");
    assert!(out.contains("b-none"), "got: {out:?}");
}

#[test]
fn replace_first_only() {
    let src = r#"
import re

fn main() -> i32:
    p: Pattern = re.compile("[0-9]")
    out: str = p.replace("a1b2c3", "X")
    println(out)
    return 0
"#;
    let p = compile_snippet("replace_first_only", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; out={out}");
    assert!(out.contains("aXb2c3"), "got: {out:?}");
}
