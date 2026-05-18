//! Regression tests for the six "real-world" stress-test bugs fixed in
//! the parallel cleanup pass. Each test compiles a minimal repro through
//! the real compiler, runs the resulting `.spyc` via `run_file_capture`,
//! and asserts the stdout that proves the fix.
//!
//! Bug index (see also BUGS_KNOWN.md for the deferred items):
//!   1. `is not none` was lowered to `RefEq` (no inversion) and so
//!      evaluated identically to `is none`.
//!   2. `str(c: char)` routed through `StrFromAny`, which formatted the
//!      codepoint as a decimal integer: `str('h')` → "104".
//!   3. `char(i32)` was rejected by the typechecker with E2011
//!      ("not callable") because the numeric-ctor allow-list omitted it.
//!   4. `dict.has(k)` was rejected by the typechecker with E2004
//!      ("no method `has`") even though the VM implemented `DictHas`.
//!   5. `xs.pop()` had no native id, no synth entry, and no IR dispatch
//!      — the underlying `Opcode::ListPop` existed but was unreachable
//!      from source.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_real_world_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// real-world: fix — bug #1 (`is not none` inverted).
#[test]
fn is_not_none_takes_correct_branch_when_value_is_some() {
    let src = "\
fn main() -> i32:
    x: i32? = 42
    if x is not none:
        println(\"some\")
    else:
        println(\"none\")
    return 0
";
    let p = compile_snippet("is_not_none_some", src);
    let (code, out) = run_file_capture(&p).expect("must run cleanly");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(
        out.contains("some"),
        "expected `is not none` to take the if branch when x=42; got: {out:?}",
    );
    assert!(
        !out.contains("none\n"),
        "`else` branch must NOT run when x=42; got: {out:?}",
    );
}

#[test]
fn is_not_none_takes_else_branch_when_value_is_none() {
    let src = "\
fn main() -> i32:
    y: i32? = none
    if y is not none:
        println(\"wrong\")
    else:
        println(\"right\")
    return 0
";
    let p = compile_snippet("is_not_none_none", src);
    let (code, out) = run_file_capture(&p).expect("must run cleanly");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(
        out.contains("right"),
        "expected `is not none` to take the else branch when y=none; got: {out:?}",
    );
    assert!(
        !out.contains("wrong"),
        "if branch must NOT run when y=none; got: {out:?}",
    );
}

#[test]
fn is_none_still_works_after_is_not_fix() {
    // Belt-and-braces: ensure the `is none` direction wasn't broken.
    let src = "\
fn main() -> i32:
    a: i32? = none
    if a is none:
        println(\"a-none\")
    b: i32? = 7
    if b is none:
        println(\"b-none\")
    else:
        println(\"b-some\")
    return 0
";
    let p = compile_snippet("is_none_smoke", src);
    let (code, out) = run_file_capture(&p).expect("must run cleanly");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(out.contains("a-none"), "got: {out:?}");
    assert!(out.contains("b-some"), "got: {out:?}");
    assert!(!out.contains("b-none"), "got: {out:?}");
}

// real-world: fix — bug #2 (`str(char)` returned decimal codepoint).
#[test]
fn str_of_char_returns_single_codepoint_string() {
    let src = "\
fn main() -> i32:
    println(str('h'))
    println(str('A'))
    return 0
";
    let p = compile_snippet("str_of_char", src);
    let (code, out) = run_file_capture(&p).expect("must run cleanly");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(
        out.contains("h\n"),
        "expected str('h') = \"h\" (not \"104\"); got: {out:?}",
    );
    assert!(
        out.contains("A\n"),
        "expected str('A') = \"A\" (not \"65\"); got: {out:?}",
    );
}

// real-world: fix — bug #3 (`char(i32)` rejected by typechecker).
#[test]
fn char_constructor_typechecks_and_produces_correct_codepoint() {
    let src = "\
fn main() -> i32:
    c: char = char(72)
    println(str(c))
    return 0
";
    let p = compile_snippet("char_ctor", src);
    let (code, out) = run_file_capture(&p).expect("must run cleanly");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(
        out.contains("H\n"),
        "expected char(72) = 'H' rendered as \"H\"; got: {out:?}",
    );
}

// real-world: fix — bug #4 (`dict.has` rejected by typechecker).
#[test]
fn dict_has_typechecks_and_returns_bool() {
    let src = "\
fn main() -> i32:
    d: Dict[str, i32] = {\"a\": 1, \"b\": 2}
    if d.has(\"a\"):
        println(\"has-a\")
    if d.has(\"z\"):
        println(\"has-z-wrong\")
    else:
        println(\"no-z\")
    return 0
";
    let p = compile_snippet("dict_has", src);
    let (code, out) = run_file_capture(&p).expect("must run cleanly");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(out.contains("has-a"), "got: {out:?}");
    assert!(out.contains("no-z"), "got: {out:?}");
    assert!(!out.contains("has-z-wrong"), "got: {out:?}");
}

// real-world: fix — bug #6 (`list.pop()` missing).
#[test]
fn list_pop_removes_and_returns_last_element() {
    let src = "\
fn main() -> i32:
    xs: List[i64] = [10, 20, 30]
    last: i64 = xs.pop()
    println(str(last))
    println(str(xs.len()))
    return 0
";
    let p = compile_snippet("list_pop", src);
    let (code, out) = run_file_capture(&p).expect("must run cleanly");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(out.contains("30\n"), "expected popped value 30; got: {out:?}");
    assert!(out.contains("2\n"), "expected new length 2; got: {out:?}");
}

// real-world: combined demo that exercises all four CRITICAL fixes in a
// single program (this is the script the task report quotes verbatim).
#[test]
fn demo_program_exercises_all_critical_fixes() {
    let src = "\
fn main() -> i32:
    x: i32? = 42
    if x is not none:
        println(\"is not none works: \" + str(x))
    println(\"str(char): \" + str('h'))
    c: char = char(72)
    println(\"char(72) = \" + str(c))
    d: Dict[str, i32] = {\"a\": 1, \"b\": 2}
    if d.has(\"a\"):
        println(\"dict.has works\")
    return 0
";
    let p = compile_snippet("real_world_demo", src);
    let (code, out) = run_file_capture(&p).expect("must run cleanly");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    let want = "is not none works: 42\n\
                str(char): h\n\
                char(72) = H\n\
                dict.has works\n";
    assert_eq!(out, want, "demo stdout mismatch; got: {out:?}");
}
