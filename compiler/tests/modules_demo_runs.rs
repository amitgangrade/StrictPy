//! Integration test for examples/modules_demo/main.spy (M60).
//!
//! Proves the user-module import feature end-to-end against the shipped
//! multi-file example: `import mathutil` (qualified calls, a qualified
//! class, and a top-level constant) plus `from strutil import shout,
//! repeat`. We compile via `compile_file` so that imports resolve relative
//! to the example's own directory, then run the merged module and assert on
//! deterministic stdout.

use std::path::PathBuf;

use strictpy_compiler::compile_file;
use strictpy_vm::run_file_capture;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn modules_demo_compiles() {
    let main = project_root()
        .join("examples")
        .join("modules_demo")
        .join("main.spy");
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("modules_demo_compile.spyc");
    compile_file(&main, &out).unwrap_or_else(|e| panic!("compile modules_demo: {e}"));
}

#[test]
fn modules_demo_runs_with_expected_output() {
    let main = project_root()
        .join("examples")
        .join("modules_demo")
        .join("main.spy");
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("modules_demo_run.spyc");
    compile_file(&main, &out).unwrap_or_else(|e| panic!("compile modules_demo: {e}"));

    let (code, stdout) = run_file_capture(&out).expect("run modules_demo");
    eprintln!("STDOUT:\n{stdout}");
    assert_eq!(code, 0, "non-zero exit; stdout:\n{stdout}");

    assert!(stdout.contains("sum = 7\n"), "sum line:\n{stdout}");
    assert!(stdout.contains("square = 25\n"), "square line:\n{stdout}");
    assert!(stdout.contains("golden = 1618\n"), "golden line:\n{stdout}");
    assert!(stdout.contains("counter = 13\n"), "counter line:\n{stdout}");
    assert!(stdout.contains("greeting = hello!\n"), "greeting line:\n{stdout}");
    assert!(stdout.contains("ha = hahaha\n"), "repeat line:\n{stdout}");
}
