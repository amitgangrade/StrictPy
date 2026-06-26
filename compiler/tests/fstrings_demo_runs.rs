//! Lane B acceptance test: `examples/fstrings_demo.spy` — basic f-string
//! interpolation desugared to string concatenation. Asserts the example
//! compiles, exits 0, and the interpolated lines render as expected.
//!
//! Modeled on `compiler/tests/comprehensions_demo_runs.rs`.

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
fn fstrings_demo_compiles() {
    let src_path = project_root().join("examples").join("fstrings_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read fstrings_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile fstrings_demo.spy: {e}"));
}

#[test]
fn fstrings_demo_runs_ok() {
    let src_path = project_root().join("examples").join("fstrings_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read fstrings_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile fstrings_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fstrings_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run fstrings_demo");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    assert!(out.contains("hello, world!\n"), "stdout:\n{out}");
    assert!(out.contains("n = 42, name = world\n"), "stdout:\n{out}");
    assert!(out.contains("world42\n"), "stdout:\n{out}");
    assert!(out.contains("sum = 50\n"), "stdout:\n{out}");
    assert!(out.contains("plain text\n"), "stdout:\n{out}");
    assert!(
        out.contains("OK: fstrings\n") || out.trim_end().ends_with("OK: fstrings"),
        "expected `OK: fstrings` summary; stdout:\n{out}"
    );
}
