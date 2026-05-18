//! M18 acceptance test: `examples/expr_interp.spy` is a small
//! expression-language interpreter exercising the M13-M17 surface end
//! to end.  Specifically:
//!
//!   * Sealed `Expr` (5 variants: Lit / Var / Binop / If / Let) walked
//!     by a single `match` statement (M16).
//!   * Sealed `Value` (2 variants: IntVal / StrVal) discriminated by
//!     nested `match` inside Binop arms.
//!   * `try` / `except` (M15) catching four distinct exception flavours
//!     raised from inside recursive eval frames.
//!   * `raise TypeError(...)` from inside a match arm body — handler
//!     frames must unwind correctly across nested matches.
//!
//! Acceptance: the program runs ten test cases, prints `PASS test=N`
//! for each, and exits 0 with a `OK: 10/10` summary line.

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
fn expr_interp_compiles() {
    let src_path = project_root().join("examples").join("expr_interp.spy");
    let src = fs::read_to_string(&src_path).expect("read expr_interp.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile expr_interp.spy: {e}"));
}

#[test]
fn expr_interp_runs_all_ten_cases() {
    let src_path = project_root().join("examples").join("expr_interp.spy");
    let src = fs::read_to_string(&src_path).expect("read expr_interp.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile expr_interp.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("expr_interp.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run expr_interp");
    assert_eq!(code, 0, "exit code; stdout:\n{out}");

    // Each test case prints "PASS test=N (...)"; we just check all ten
    // are present.  The summary line gates pass/fail at the program
    // level — if any case FAILed, main() returns 1 and `code` would be 1.
    for n in 1..=10 {
        let needle = format!("PASS test={n} ");
        assert!(out.contains(&needle), "missing {needle}; stdout:\n{out}");
    }
    assert!(out.contains("OK: 10/10"), "summary missing; stdout:\n{out}");

    // Sanity-check a few specific computed values to guard against a
    // future refactor accidentally swapping arms.
    assert!(out.contains("1 + 2 => 3"), "1+2; stdout:\n{out}");
    assert!(out.contains("let x = 10 in x * x => 100"), "let-mul; stdout:\n{out}");
    assert!(out.contains("=> \"yes\""), "if-yes-branch; stdout:\n{out}");
    // M18 (BUG-036) fix: runtime now emits the canonical
    // `ZeroDivisionError` (Python-compatible). The legacy
    // `DivisionByZeroError` still works as an `except`-arm filter via
    // the alias table in vm/src/interp.rs::exception_name_alias.
    assert!(out.contains("raised ZeroDivisionError"),
            "div-by-zero name; stdout:\n{out}");
    assert!(out.contains("raised TypeError"), "type err; stdout:\n{out}");
    assert!(out.contains("raised RuntimeError"), "runtime err; stdout:\n{out}");
    // Nested let exercises a 5+ frame deep eval.
    assert!(out.contains("nested let => 4"), "nested let; stdout:\n{out}");
    // String concat over StrVal.
    assert!(out.contains("=> \"hello world\""), "string concat; stdout:\n{out}");
}
