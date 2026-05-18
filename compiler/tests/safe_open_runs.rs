//! Integration test for `examples/safe_open.spy` — the M15 BUG-025 demo.
//!
//! Before M15, the same program (without try/except) would have terminated
//! the VM with a non-zero exit code as soon as `open("missing", "r")`
//! traps. With try/except wired through the IR + codegen + interpreter,
//! the same call is recoverable: the missing-file path prints a "could
//! not open …" diagnostic and the program continues to its next case.
//!
//! Acceptance: `examples/safe_open.spy` should run cleanly (exit 0) and
//! produce the four canonical output lines plus the inner-finally
//! propagation footer.

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
fn safe_open_compiles() {
    let src_path = project_root().join("examples").join("safe_open.spy");
    let src = fs::read_to_string(&src_path).expect("read safe_open.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile safe_open.spy: {e}"));
}

#[test]
fn safe_open_recovers_from_missing_file_and_finishes_clean() {
    let src_path = project_root().join("examples").join("safe_open.spy");
    let src = fs::read_to_string(&src_path).expect("read safe_open.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile safe_open.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("safe_open.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run safe_open");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // Real file A opened cleanly.
    assert!(out.contains("opened a OK: alpha"), "stdout:\n{out}");
    // Missing file B was caught and recovered from.
    assert!(out.contains("safe_open: could not open safe_open_b_definitely_missing_m15.txt"),
            "missing-file diagnostic absent; stdout:\n{out}");
    assert!(out.contains("recovered from missing b"), "stdout:\n{out}");
    // Real file C still opened (program kept going past the recovery).
    assert!(out.contains("opened c OK: gamma"), "stdout:\n{out}");
    // Inner-finally propagation: cleaned incremented twice (once in
    // the finally, once in the outer handler).
    assert!(out.contains("propagated past finally: simulated downstream failure"),
            "stdout:\n{out}");
    assert!(out.contains("cleaned=2"), "cleaned counter wrong; stdout:\n{out}");
}
