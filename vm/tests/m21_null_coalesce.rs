//! M21 regression for BUG-037: the null-coalesce operator `x ?? fallback`
//! used to lower to `Copy(fallback)` unconditionally — `x` was evaluated
//! but its value was discarded, so the operator always returned the
//! fallback regardless of whether `x` was none. Same shape as BUG-008
//! (`is not` was inverted) and BUG-034 (`str !=` always true): a binary
//! operator whose lowering was a placeholder that silently ran the wrong
//! branch.
//!
//! Found by the M20a `os` agent — the workaround (`if x is none: ...
//! else: x`) was used in tests until M21 fixed it.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m21_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn null_coalesce_returns_lhs_when_not_none() {
    let src = "\
fn main() -> i32:
    x: i32? = 42
    y: i32 = x ?? 999
    println(\"y=\" + str(y))
    return 0
";
    let p = compile_snippet("nc_not_none", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(
        out.contains("y=42\n"),
        "BUG-037 regression — expected y=42, got: {out:?}"
    );
}

#[test]
fn null_coalesce_returns_rhs_when_none() {
    let src = "\
fn main() -> i32:
    x: i32? = none
    y: i32 = x ?? 999
    println(\"y=\" + str(y))
    return 0
";
    let p = compile_snippet("nc_none", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(
        out.contains("y=999\n"),
        "BUG-037 regression — expected y=999, got: {out:?}"
    );
}

#[test]
fn null_coalesce_does_not_evaluate_rhs_when_lhs_not_none() {
    // Short-circuit semantics: rhs is only evaluated when lhs IS none.
    // If lhs is non-none, the trapping rhs (raise) must NOT run. If the
    // old placeholder lowering came back, this test would catch the raise
    // and exit non-zero.
    let src = "\
fn boom() -> i32:
    raise RuntimeError(\"rhs should not have run\")
    return 0

fn main() -> i32:
    x: i32? = 42
    y: i32 = x ?? boom()
    println(\"y=\" + str(y))
    return 0
";
    let p = compile_snippet("nc_short_circuit", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("y=42\n"), "stdout: {out:?}");
}

#[test]
fn null_coalesce_does_evaluate_rhs_when_lhs_is_none() {
    // Symmetric: when lhs IS none, rhs runs.
    let src = "\
fn produce() -> i32:
    return 999

fn main() -> i32:
    x: i32? = none
    y: i32 = x ?? produce()
    println(\"y=\" + str(y))
    return 0
";
    let p = compile_snippet("nc_rhs_runs", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("y=999\n"), "stdout: {out:?}");
}

#[test]
fn null_coalesce_str() {
    let src = "\
fn main() -> i32:
    a: str? = none
    b: str = a ?? \"default\"
    println(b)
    c: str? = \"actual\"
    d: str = c ?? \"default\"
    println(d)
    return 0
";
    let p = compile_snippet("nc_str", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("default\n"), "stdout: {out:?}");
    assert!(out.contains("actual\n"), "stdout: {out:?}");
}

#[test]
fn null_coalesce_chained() {
    // a ?? b ?? c — left-associative; if a is none, try b; if b is none, c.
    let src = "\
fn main() -> i32:
    a: i32? = none
    b: i32? = none
    c: i32 = 7
    r: i32 = a ?? (b ?? c)
    println(\"r=\" + str(r))
    return 0
";
    let p = compile_snippet("nc_chained", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("r=7\n"), "stdout: {out:?}");
}
