//! M16 acceptance test: `examples/calculator_with_match.spy` is an AST
//! evaluator rewritten to use `sealed class` + `match` / `case`. The
//! pre-M16 form in `calculator.spy` had to use virtual methods because
//! match-lowering was an M4 placeholder; this test gates that the new
//! `match v: case Lit(value): ...` form compiles and runs correctly.

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
fn calculator_with_match_compiles() {
    let src_path = project_root().join("examples").join("calculator_with_match.spy");
    let src = fs::read_to_string(&src_path).expect("read calculator_with_match.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile calculator_with_match.spy: {e}"));
}

#[test]
fn calculator_with_match_evaluates_all_four_expressions() {
    let src_path = project_root().join("examples").join("calculator_with_match.spy");
    let src = fs::read_to_string(&src_path).expect("read calculator_with_match.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("calculator_with_match.spyc");
    fs::write(&spyc_path, &bytes).expect("write");

    let (code, out) = run_file_capture(&spyc_path).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    // 1 + 2 = 3
    assert!(out.contains("1 + 2 = (1"), "missing first expr: {out:?}");
    assert!(out.contains("=> 3"), "missing 1+2 result: {out:?}");
    // 3 * (4 - 1) = 9
    assert!(out.contains("=> 9"), "missing 3*(4-1)=9: {out:?}");
    // -(2 + 3) * 4 = -20
    assert!(out.contains("=> -20"), "missing -(2+3)*4: {out:?}");
    // (10 - 2) / (1 + 1) = 4
    assert!(out.contains("=> 4"), "missing (10-2)/(1+1): {out:?}");
}
