//! M30 regression for BUG-028: the lexer used to require explicit `\`
//! or wrapping `()` for line continuation across a trailing infix
//! operator. After M30 the lexer suppresses the `Newline` token when
//! the last significant token is a binary operator that requires a
//! right-hand operand (mirrors implicit continuation inside parens).
//!
//! See `compiler/src/lexer.rs::last_token_continues_line` for the full
//! set of trigger tokens, and STRICTPY_SPEC §3.2.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m30_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

/// The canonical BUG-028 repro from `docs/thesis/bugs/catalog.md`.
/// Pre-M30: errored E0001 at the unindented continuation. Post-M30: works.
#[test]
fn bug_028_trailing_plus_continues_across_newline() {
    let src = "\
fn make_msg() -> str:
    return \"a \" +
        \"b\"

fn main() -> i32:
    println(make_msg())
    return 0
";
    let p = compile_snippet("plus_continuation", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(
        out.contains("a b\n"),
        "BUG-028: expected 'a b', got: {out:?}"
    );
}

#[test]
fn boolean_and_continues_across_newline() {
    // `and` at end of line keeps lexing on the next physical line.
    let src = "\
fn main() -> i32:
    a: bool = true
    b: bool = true
    c: bool = true
    if a and
            b and
            c:
        println(\"all true\")
    return 0
";
    let p = compile_snippet("and_continuation", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("all true\n"), "stdout: {out:?}");
}

#[test]
fn boolean_or_continues_across_newline() {
    let src = "\
fn main() -> i32:
    a: bool = false
    b: bool = false
    c: bool = true
    if a or
            b or
            c:
        println(\"at least one\")
    return 0
";
    let p = compile_snippet("or_continuation", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("at least one\n"), "stdout: {out:?}");
}

#[test]
fn equality_continues_across_newline() {
    let src = "\
fn main() -> i32:
    x: i64 = 42
    if x ==
            42:
        println(\"equal\")
    return 0
";
    let p = compile_snippet("eq_continuation", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("equal\n"), "stdout: {out:?}");
}

#[test]
fn less_than_continues_across_newline() {
    let src = "\
fn main() -> i32:
    x: i64 = 1
    y: i64 = 5
    if x <
            y:
        println(\"x lt y\")
    return 0
";
    let p = compile_snippet("lt_continuation", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("x lt y\n"), "stdout: {out:?}");
}

#[test]
fn assignment_continues_across_newline() {
    // `=` triggers continuation: an rvalue is needed after.
    let src = "\
fn main() -> i32:
    s: str =
        \"hello\"
    println(s)
    return 0
";
    let p = compile_snippet("assign_continuation", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("hello\n"), "stdout: {out:?}");
}

#[test]
fn compound_assign_continues_across_newline() {
    // `+=` also triggers continuation (same as bare `=`).
    let src = "\
fn main() -> i32:
    n: i64 = 1
    n +=
        2
    println(\"n=\" + str(n))
    return 0
";
    let p = compile_snippet("plus_eq_continuation", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("n=3\n"), "stdout: {out:?}");
}

/// Existing paren-counting continuation must still work (regression).
#[test]
fn paren_continuation_still_works() {
    let src = "\
fn make_msg() -> str:
    return (\"a \" +
        \"b\")

fn main() -> i32:
    println(make_msg())
    return 0
";
    let p = compile_snippet("paren_regression", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("a b\n"), "stdout: {out:?}");
}

/// A `+` *inside* a string literal must NOT trigger line continuation.
/// The lexer must distinguish "trailing `+` token" from "the byte `+`
/// inside a string".
#[test]
fn plus_inside_string_literal_does_not_continue() {
    // The string `"a + "` contains a literal `+` byte but is one token,
    // and the program below has no actual trailing binary operator.
    let src = "\
fn main() -> i32:
    s: str = \"a + b\"
    println(s)
    t: str = \"x + y\"
    println(t)
    return 0
";
    let p = compile_snippet("string_with_plus", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("a + b\n"), "stdout: {out:?}");
    assert!(out.contains("x + y\n"), "stdout: {out:?}");
}

/// A bare line that happens to end in a non-continuation token (a string
/// literal) must still terminate the logical line — i.e. continuation
/// does NOT trigger on every token, only on the continuation set.
#[test]
fn non_operator_trailing_token_still_terminates_line() {
    let src = "\
fn main() -> i32:
    x: i64 = 1
    y: i64 = 2
    println(\"x=\" + str(x))
    println(\"y=\" + str(y))
    return 0
";
    let p = compile_snippet("no_false_continuation", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("x=1\n"), "stdout: {out:?}");
    assert!(out.contains("y=2\n"), "stdout: {out:?}");
}

/// Combined: trailing `+` followed by another trailing `+` on the
/// continuation line — multiple chained continuations.
#[test]
fn chained_continuations_work() {
    let src = "\
fn build() -> str:
    return \"a\" +
        \"b\" +
        \"c\" +
        \"d\"

fn main() -> i32:
    println(build())
    return 0
";
    let p = compile_snippet("chained_plus", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("abcd\n"), "stdout: {out:?}");
}
