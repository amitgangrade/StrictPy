//! M15 regression tests for try/except/finally + raise.
//!
//! Before M15, the parser already accepted `try:` / `except T as e:` /
//! `finally:` (since the AST shape had been in place since M3), but IR
//! lowering silently dropped both the handlers and the raise terminator —
//! `compiler/src/ir.rs::Stmt::Try` recursed into every body in source
//! order with no exception edges, and `Stmt::Raise` emitted a `Throw`
//! that the VM trapped with `"THROW not implemented (M5)"`. BUG-025 (no
//! fallible `open()`) sat behind this: native code already produced
//! `VmError::UncaughtException { type_name: "IOError", ... }` on a
//! missing file, but no user program could catch it.
//!
//! Coverage:
//! 1.  Pass-through (no exception → handler doesn't run).
//! 2.  `raise IOError("x")` caught by `except IOError as e:`; `e.message`
//!     is the constructor argument; `e.type_name` is `"IOError"`.
//! 3.  Built-in `IndexError` from `xs[i]` propagation caught by user code.
//! 4.  Handler order — `IOError` matches the IOError arm, not the
//!     `Exception` catch-all that comes second.
//! 5.  `finally` runs on normal completion.
//! 6.  `finally` runs after a caught exception.
//! 7.  `finally` runs while propagating an uncaught exception (then the
//!     exception escapes the outer try and is caught by an even-outer try).
//! 8.  Nested try: inner re-raises (no matching handler), outer catches.
//! 9.  **BUG-025 acceptance**: `open("missing", "r")` catchable.
//! 10. `1 / 0` catchable as `ZeroDivisionError`.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m15_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── (1) Pass-through ────────────────────────────────────────────────────

#[test]
fn try_body_completes_normally_handler_does_not_run() {
    let src = "\
fn main() -> i32:
    x: i32 = 0i32
    try:
        x = 7i32
        println(\"body\")
    except Exception as e:
        println(\"handler should not run: \" + e.message)
    println(\"after x=\" + str(x))
    return 0
";
    let p = compile_snippet("pass_through", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("body\n"), "got: {out:?}");
    assert!(out.contains("after x=7\n"), "got: {out:?}");
    assert!(!out.contains("handler should not run"), "got: {out:?}");
}

// ── (2) `raise IOError("x")` caught + e.message + e.type_name ───────────

#[test]
fn raise_io_error_caught_by_io_error_handler_with_bound_message() {
    let src = "\
fn main() -> i32:
    try:
        raise IOError(\"file not found\")
    except IOError as e:
        println(\"caught: \" + e.type_name + \": \" + e.message)
    return 0
";
    let p = compile_snippet("raise_io_caught", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(
        out.contains("caught: IOError: file not found\n"),
        "got: {out:?}"
    );
}

// ── (3) Native IndexError from xs[i] caught by user code ─────────────────

#[test]
fn list_index_error_caught_by_user_handler() {
    let src = "\
fn main() -> i32:
    xs: List[i32] = [1i32, 2i32]
    try:
        v: i32 = xs[10i32]
        println(\"never: \" + str(v))
    except IndexError as e:
        println(\"caught index: \" + e.message)
    return 0
";
    let p = compile_snippet("list_index_caught", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("caught index: "), "got: {out:?}");
    assert!(!out.contains("never:"), "got: {out:?}");
}

// ── (4) Handler order — most-specific first wins; catch-all comes second ─

#[test]
fn specific_handler_matches_before_exception_catch_all() {
    let src = "\
fn main() -> i32:
    try:
        raise IOError(\"x\")
    except ValueError as e:
        println(\"wrong: ValueError handler\")
    except IOError as e:
        println(\"right: IOError handler — \" + e.message)
    except Exception as e:
        println(\"wrong: catch-all\")
    return 0
";
    let p = compile_snippet("handler_order", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("right: IOError handler — x\n"), "got: {out:?}");
    assert!(!out.contains("wrong:"), "got: {out:?}");
}

// ── (5) finally runs on normal completion ───────────────────────────────

#[test]
fn finally_runs_on_normal_completion() {
    let src = "\
fn main() -> i32:
    try:
        println(\"body\")
    except Exception as e:
        println(\"handler\")
    finally:
        println(\"finally\")
    println(\"after\")
    return 0
";
    let p = compile_snippet("finally_normal", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    let body_pos = out.find("body").expect("body printed");
    let finally_pos = out.find("finally").expect("finally printed");
    let after_pos = out.find("after").expect("after printed");
    assert!(body_pos < finally_pos && finally_pos < after_pos, "order: {out:?}");
    assert!(!out.contains("handler\n"), "got: {out:?}");
}

// ── (6) finally runs after a caught exception ───────────────────────────

#[test]
fn finally_runs_after_caught_exception() {
    let src = "\
fn main() -> i32:
    try:
        raise ValueError(\"oops\")
    except ValueError as e:
        println(\"caught: \" + e.message)
    finally:
        println(\"finally\")
    println(\"after\")
    return 0
";
    let p = compile_snippet("finally_caught", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    let caught = out.find("caught:").expect("caught");
    let finally = out.find("finally").expect("finally");
    let after = out.find("after").expect("after");
    assert!(caught < finally && finally < after, "order: {out:?}");
}

// ── (7) finally runs while propagating, then outer try catches ──────────

#[test]
fn finally_runs_while_propagating_then_outer_catches() {
    let src = "\
fn main() -> i32:
    try:
        try:
            raise IOError(\"inner\")
        finally:
            println(\"inner finally\")
    except IOError as e:
        println(\"outer caught: \" + e.message)
    println(\"done\")
    return 0
";
    let p = compile_snippet("finally_propagating", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    let inner_finally = out.find("inner finally").expect("inner finally");
    let outer_caught = out.find("outer caught").expect("outer caught");
    let done = out.find("done").expect("done");
    assert!(inner_finally < outer_caught && outer_caught < done, "order: {out:?}");
    assert!(out.contains("outer caught: inner\n"), "got: {out:?}");
}

// ── (8) Nested try: inner doesn't match, outer catches ──────────────────

#[test]
fn nested_try_unmatched_inner_propagates_to_outer() {
    let src = "\
fn main() -> i32:
    try:
        try:
            raise IOError(\"x\")
        except ValueError as v:
            println(\"wrong inner branch\")
    except IOError as e:
        println(\"outer caught: \" + e.message)
    return 0
";
    let p = compile_snippet("nested_propagate", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("outer caught: x\n"), "got: {out:?}");
    assert!(!out.contains("wrong inner branch"), "got: {out:?}");
}

// ── (9) BUG-025 acceptance: open() of a missing file is catchable ───────

#[test]
fn open_of_missing_file_is_catchable_as_io_error() {
    let src = "\
fn main() -> i32:
    try:
        f: io.File = open(\"definitely_does_not_exist_m15_unique.txt\", \"r\")
        println(\"opened?!\")
    except IOError as e:
        println(\"missing: \" + e.message)
    return 0
";
    let p = compile_snippet("bug025_open_missing", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.starts_with("missing: "), "got: {out:?}");
    assert!(!out.contains("opened?!"), "got: {out:?}");
}

// ── (10) Division by zero catchable as ZeroDivisionError ───────────────

#[test]
fn division_by_zero_catchable() {
    // The runtime emits `type_name: "DivisionByZeroError"` (legacy from
    // M3). The resolver registers `ZeroDivisionError` as an alias for
    // Python-compat. Both names must catch.
    let src = "\
fn main() -> i32:
    a: i32 = 1i32
    b: i32 = 0i32
    try:
        c: i32 = a // b
        println(\"never: \" + str(c))
    except Exception as e:
        println(\"caught: \" + e.type_name)
    return 0
";
    let p = compile_snippet("div_by_zero", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(
        out.contains("caught: DivisionByZeroError\n")
            || out.contains("caught: ZeroDivisionError\n"),
        "got: {out:?}"
    );
    assert!(!out.contains("never:"), "got: {out:?}");
}
