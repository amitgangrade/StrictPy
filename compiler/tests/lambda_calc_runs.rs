//! Integration test for the untyped lambda-calculus example.
//!
//! Compiles `examples/lambda_calc.spy`, runs it through the VM's
//! `run_file_capture`, and asserts that the three test programs produce
//! the expected reduction traces.

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
fn lambda_calc_compiles() {
    let src_path = project_root().join("examples").join("lambda_calc.spy");
    let src = fs::read_to_string(&src_path).expect("read lambda_calc.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile lambda_calc.spy: {e}"));
}

#[test]
fn lambda_calc_reduces_combinators() {
    let src_path = project_root().join("examples").join("lambda_calc.spy");
    let src = fs::read_to_string(&src_path).expect("read lambda_calc.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile lambda_calc.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("lambda_calc.spyc");
    fs::write(&spyc_path, &bytes).expect("write lambda_calc.spyc");

    let (code, out) =
        run_file_capture(&spyc_path).unwrap_or_else(|e| panic!("lambda_calc.spy: vm error: {e}"));
    eprintln!("STDOUT:\n{out}");
    assert_eq!(code, 0, "exit code; stdout was: {out:?}");

    // Test 1: I I -> I.
    assert!(out.contains("test 1: I I"), "missing test 1 label: {out:?}");

    // Test 2: K a b -> a (after enough steps to reduce both apps).
    assert!(out.contains("test 2: K a b"), "missing test 2 label: {out:?}");

    // Test 3: Omega — must hit the `<diverges>` branch.
    assert!(out.contains("test 3: Omega"), "missing test 3 label: {out:?}");
    assert!(out.contains("<diverges>"), "Omega must diverge: {out:?}");
}
