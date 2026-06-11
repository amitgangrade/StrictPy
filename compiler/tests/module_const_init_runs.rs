//! Regression tests for module-level `final` const initialisers that
//! reference other consts.
//!
//! Historically only bare-literal initialisers were folded into
//! `module_consts` during IR lowering; anything else (e.g.
//! `final SOLAR_MASS: f64 = 4.0 * PI * PI`) was silently dropped and every
//! reference site lowered to `Const(None)` — numeric 0 — corrupting
//! programs without any diagnostic. Discovered via an n-body benchmark
//! where all planet masses evaluated to 0.
//!
//! The fix has two halves, both covered here:
//! 1. Const initialisers built from literals, other consts, unary +/-,
//!    and arithmetic now evaluate at compile time, in dependency order
//!    (fixed point — declaration order does not matter).
//! 2. Anything that still cannot be evaluated (calls, cycles, ...) is a
//!    compile error (`E3003`, `CompileError::Semantic`) instead of a
//!    silent 0.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_compiler::error::CompileError;
use strictpy_vm::run_file_capture;

fn compile_to_temp(name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{name}.spy"), src)
        .unwrap_or_else(|e| panic!("compile {name}: {e}"));
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

/// The original repro: SOLAR_MASS printed 0 instead of 39478417.
#[test]
fn const_referencing_const_evaluates_correctly() {
    let src = "\
final PI: f64 = 3.141592653589793
final SOLAR_MASS: f64 = 4.0 * PI * PI
final MASS_RATIO: f64 = SOLAR_MASS / 81.0

fn main() -> i32:
    println(str(i64(PI * 1000000.0)))
    println(str(i64(SOLAR_MASS * 1000000.0)))
    println(str(i64(MASS_RATIO * 1000000.0)))
    return 0
";
    let p = compile_to_temp("module_const_chain", src);
    let (code, out) = run_file_capture(&p).expect("must run cleanly");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(out.contains("3141592\n"), "PI wrong; stdout: {out:?}");
    assert!(out.contains("39478417\n"), "SOLAR_MASS wrong; stdout: {out:?}");
    assert!(out.contains("487387\n"), "MASS_RATIO wrong; stdout: {out:?}");
}

/// Integer const chains, including a use that precedes the definitions —
/// the module merger can reorder decls, so folding must be declaration-
/// order independent.
#[test]
fn int_const_chain_is_declaration_order_independent() {
    let src = "\
final AREA: i64 = WIDTH * HEIGHT
final WIDTH: i64 = 60
final HEIGHT: i64 = 30
final HALF: i64 = AREA / 2

fn main() -> i32:
    println(str(AREA))
    println(str(HALF))
    return 0
";
    let p = compile_to_temp("module_const_order", src);
    let (code, out) = run_file_capture(&p).expect("must run cleanly");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(out.contains("1800\n"), "AREA wrong; stdout: {out:?}");
    assert!(out.contains("900\n"), "HALF wrong; stdout: {out:?}");
}

/// A const initialiser that cannot be evaluated at compile time must be a
/// compile error — a silent 0 is the worst possible outcome.
#[test]
fn non_const_initialiser_is_rejected_not_silent_zero() {
    let src = "\
fn answer() -> i64:
    return 42

final X: i64 = answer()

fn main() -> i32:
    println(str(X))
    return 0
";
    let err = compile_source("const_call_init.spy".to_string(), src)
        .expect_err("call in const initialiser must be rejected");
    assert!(
        matches!(err, CompileError::Semantic { .. }),
        "expected E3003 Semantic error, got: {err:?}"
    );
}

/// Consts whose initialisers reference each other can never be evaluated;
/// the fixed point must terminate and report an error.
#[test]
fn const_reference_cycle_is_rejected() {
    let src = "\
final A: i64 = B + 1
final B: i64 = A + 1

fn main() -> i32:
    return 0
";
    let err = compile_source("const_cycle.spy".to_string(), src)
        .expect_err("const reference cycle must be rejected");
    assert!(
        matches!(err, CompileError::Semantic { .. }),
        "expected E3003 Semantic error, got: {err:?}"
    );
}
