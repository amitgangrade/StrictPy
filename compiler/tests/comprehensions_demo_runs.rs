//! M62a acceptance test: `examples/comprehensions_demo.spy` — a small program
//! that exercises a list comprehension (with `if` filter), a dict
//! comprehension, and a set comprehension. Output is deterministic (the list
//! is indexed by position and the dict is probed via `in`, so neither depends
//! on hash-iteration order).
//!
//! Asserts:
//!   - the example compiles,
//!   - it exits 0,
//!   - the final `OK: comprehensions` summary line is present.
//!
//! Modeled on `compiler/tests/algorithms_lib_runs.rs`.

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
fn comprehensions_demo_compiles() {
    let src_path = project_root().join("examples").join("comprehensions_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read comprehensions_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile comprehensions_demo.spy: {e}"));
}

#[test]
fn comprehensions_demo_runs_ok() {
    let src_path = project_root().join("examples").join("comprehensions_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read comprehensions_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile comprehensions_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("comprehensions_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run comprehensions_demo");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // Spot-check the deterministic list-comp output.
    assert!(out.contains("even_squares len=3\n"), "stdout:\n{out}");
    assert!(out.contains("even_squares[0]=4\n"), "stdout:\n{out}");
    assert!(out.contains("even_squares[2]=36\n"), "stdout:\n{out}");
    // Dict comp probed by key.
    assert!(out.contains("squares has key 3\n"), "stdout:\n{out}");
    assert!(out.contains("squares lacks key 7\n"), "stdout:\n{out}");

    // The load-bearing acceptance signal.
    assert!(
        out.contains("OK: comprehensions\n") || out.trim_end().ends_with("OK: comprehensions"),
        "expected `OK: comprehensions` summary; stdout:\n{out}"
    );
}
