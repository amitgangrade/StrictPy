//! M35 P4-A integration test for `examples/re_pattern_demo.spy`.
//!
//! Compiles the compiled-regex Pattern demo and runs it through the VM
//! in-process, asserting on the expected outputs across the six demo
//! sections (matches / find_all / find / replace / split / source).

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
fn re_pattern_demo_compiles() {
    let src_path = project_root().join("examples").join("re_pattern_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read re_pattern_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile re_pattern_demo.spy: {e}"));
}

#[test]
fn re_pattern_demo_runs_in_process() {
    let src_path = project_root().join("examples").join("re_pattern_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read re_pattern_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile re_pattern_demo.spy: {e}"));

    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("re_pattern_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run");
    assert_eq!(code, 0, "exit code; stdout={out}");

    // Section 1: matches
    assert!(out.contains("=== matches ==="), "missing matches header\n{out}");
    assert!(out.contains("valid: alice@example.com"), "stdout:\n{out}");
    assert!(out.contains("invalid: not-an-email"), "stdout:\n{out}");
    assert!(out.contains("valid: bob@strictpy.dev"), "stdout:\n{out}");

    // Section 2: find_all
    assert!(out.contains("=== find_all ==="), "stdout:\n{out}");
    assert!(out.contains("uptime 42 minutes -> 1 hit(s)"), "stdout:\n{out}");
    assert!(out.contains("score 17 / 100 -> 2 hit(s)"), "stdout:\n{out}");
    assert!(out.contains("no digits here -> 0 hit(s)"), "stdout:\n{out}");

    // Section 3: find (optional)
    assert!(out.contains("=== find ==="), "stdout:\n{out}");
    assert!(out.contains("first capword: Quick"), "stdout:\n{out}");

    // Section 4: replace + replace_all
    assert!(out.contains("=== replace ==="), "stdout:\n{out}");
    assert!(out.contains("abc*23"), "stdout:\n{out}");
    assert!(out.contains("abc***"), "stdout:\n{out}");

    // Section 5: split
    assert!(out.contains("=== split ==="), "stdout:\n{out}");
    assert!(out.contains("count=4"), "stdout:\n{out}");

    // Section 6: source round-trip
    assert!(out.contains("=== source ==="), "stdout:\n{out}");
    assert!(out.contains("\\d+"), "stdout:\n{out}");
}
