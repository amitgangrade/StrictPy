//! M63a acceptance test: `examples/user_exceptions_demo.spy` — user-defined
//! exception subclasses.  Exercises declaring an exception subclass with a
//! custom field, raising it, catching by its own type, catching a derived
//! exception via a base-class `except` clause, `except Exception` as a
//! catch-all, field access on the caught value, and re-raise.
//!
//! Asserts the program compiles, exits 0, and prints the expected
//! deterministic lines in order.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn user_exceptions_demo_compiles() {
    let src_path = project_root()
        .join("examples")
        .join("user_exceptions_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read user_exceptions_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile user_exceptions_demo.spy: {e}"));
}

#[test]
fn user_exceptions_demo_output() {
    let src_path = project_root()
        .join("examples")
        .join("user_exceptions_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read user_exceptions_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile user_exceptions_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("user_exceptions_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run user_exceptions_demo");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // 1) own-type catch reads the inherited message + subclass position field.
    assert!(
        out.contains("caught ParseError: unexpected token @ 7\n"),
        "stdout:\n{out}"
    );
    // 2) base-class clause catches the derived ParseError.
    assert!(
        out.contains("base clause caught: unexpected token\n"),
        "stdout:\n{out}"
    );
    // 3) except Exception catches a user type.
    assert!(
        out.contains("except Exception caught: boom\n"),
        "stdout:\n{out}"
    );
    // 4) re-raise propagates to the outer handler.
    assert!(
        out.contains("inner handler, re-raising: boom\n"),
        "stdout:\n{out}"
    );
    assert!(
        out.contains("outer handler caught re-raise: boom\n"),
        "stdout:\n{out}"
    );
    assert!(out.trim_end().ends_with("done"), "stdout:\n{out}");

    // The "no error" branch must NOT print (parse(true) always raises).
    assert!(!out.contains("no error"), "stdout:\n{out}");
}
