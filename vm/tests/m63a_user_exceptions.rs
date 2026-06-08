//! M63a regression tests for user-defined exception subclasses.
//!
//! M15 shipped try/except/finally + raise for the 10 built-in exception
//! names.  M63a lets a program declare its own exception types by
//! subclassing the built-in `Exception` base (or any built-in exception such
//! as `ValueError`), `raise` instances of them, and `catch` them — including
//! catching a `Derived` via a base-class `except Base` clause.
//!
//! Mechanism (no VM format change): the compiler stamps an exception
//! subclass instance's inherited `type_name` field (offset 0) with the
//! concrete class name at construction time, so the existing M15 `Throw` /
//! `propagate_exception` string matcher works unchanged.  Subclass matching
//! is achieved by `ir::lower_try` expanding each `except Base` clause into
//! one VM handler arm per matching `Derived` type name.
//!
//! Coverage:
//! 1.  Raise a user exception, catch it by its own type; read the inherited
//!     `message` field and a subclass-specific field.
//! 2.  A base-class `except Base` clause catches a raised `Derived`.
//! 3.  `except Exception` catches any user exception type.
//! 4.  Subclass of a *built-in* exception (`class ParseError(ValueError)`)
//!     is caught by `except ValueError`.
//! 5.  Re-raise (`raise e`) inside a handler, caught by an outer try.
//! 6.  An uncaught user exception propagates to the top with its type name.
//! 7.  NEGATIVE: raising a non-exception class is a compile error.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::error::VmError;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m63a_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// Shared class declarations used by several tests: a base AppError(Exception)
// and a derived ParseError(AppError) that adds a field.
const DECLS: &str = "\
open class AppError(Exception):
    fn __init__(self, message: str) -> None:
        self.message = message

final class ParseError(AppError):
    position: i32

    fn __init__(self, message: str, position: i32) -> None:
        self.message = message
        self.position = position
";

// ── (1) Raise + catch own type; read inherited + own fields ─────────────

#[test]
fn raise_and_catch_own_type_with_fields() {
    let src = format!(
        "{DECLS}
fn main() -> i32:
    try:
        raise ParseError(\"bad token\", 7i32)
    except ParseError as e:
        println(\"caught: \" + e.message + \" @ \" + str(e.position))
    return 0
"
    );
    let p = compile_snippet("own_type", &src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("caught: bad token @ 7\n"), "got: {out:?}");
}

// ── (2) Base-class clause catches a derived raise ───────────────────────

#[test]
fn base_class_clause_catches_derived() {
    let src = format!(
        "{DECLS}
fn main() -> i32:
    try:
        raise ParseError(\"derived\", 3i32)
    except AppError as e:
        println(\"base caught: \" + e.message)
    return 0
"
    );
    let p = compile_snippet("base_catches_derived", &src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("base caught: derived\n"), "got: {out:?}");
}

// ── (3) `except Exception` catches a user type ──────────────────────────

#[test]
fn except_exception_catches_user_type() {
    let src = format!(
        "{DECLS}
fn main() -> i32:
    try:
        raise AppError(\"any\")
    except Exception as e:
        println(\"generic caught: \" + e.message + \" type=\" + e.type_name)
    return 0
"
    );
    let p = compile_snippet("exception_catch_all", &src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(
        out.contains("generic caught: any type=AppError\n"),
        "got: {out:?}"
    );
}

// ── (4) Subclass of a built-in exception caught by the built-in clause ──

#[test]
fn subclass_of_builtin_caught_by_builtin_clause() {
    let src = "\
final class FieldError(ValueError):
    fn __init__(self, message: str) -> None:
        self.message = message

fn main() -> i32:
    try:
        raise FieldError(\"bad field\")
    except ValueError as e:
        println(\"value-err caught: \" + e.message + \" (\" + e.type_name + \")\")
    return 0
";
    let p = compile_snippet("subclass_builtin", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(
        out.contains("value-err caught: bad field (FieldError)\n"),
        "got: {out:?}"
    );
}

// ── (5) Re-raise inside a handler, caught by an outer try ───────────────

#[test]
fn reraise_inside_handler_caught_by_outer() {
    let src = format!(
        "{DECLS}
fn main() -> i32:
    try:
        try:
            raise AppError(\"first\")
        except AppError as e:
            println(\"inner: \" + e.message)
            raise e
    except Exception as e:
        println(\"outer: \" + e.message)
    return 0
"
    );
    let p = compile_snippet("reraise", &src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    let inner = out.find("inner: first").expect("inner printed");
    let outer = out.find("outer: first").expect("outer printed");
    assert!(inner < outer, "order: {out:?}");
}

// ── (6) Uncaught user exception propagates with its type name ────────────

#[test]
fn uncaught_user_exception_propagates_with_type_name() {
    let src = format!(
        "{DECLS}
fn main() -> i32:
    raise ParseError(\"escapes\", 1i32)
    return 0
"
    );
    let p = compile_snippet("uncaught", &src);
    let err = run_file_capture(&p).expect_err("uncaught exception should trap");
    match err {
        VmError::UncaughtException { type_name, message } => {
            assert_eq!(type_name, "ParseError", "type_name: {type_name:?}");
            assert_eq!(message, "escapes", "message: {message:?}");
        }
        other => panic!("expected UncaughtException, got: {other:?}"),
    }
}

// ── (7) NEGATIVE: raising a non-exception class is a compile error ───────

#[test]
fn raising_non_exception_is_compile_error() {
    let src = "\
final class Point:
    x: i32

    fn __init__(self, x: i32) -> None:
        self.x = x

fn main() -> i32:
    raise Point(1i32)
    return 0
";
    let err = compile_source("neg.spy".to_string(), src)
        .expect_err("raising a non-exception class must be a compile error");
    let msg = format!("{err}");
    assert!(
        msg.contains("E2050") || msg.to_lowercase().contains("exception"),
        "expected an E2050 not-an-exception error, got: {msg}"
    );
}
