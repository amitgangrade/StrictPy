//! Integration test for `examples/sum_args.spy`.
//!
//! Validates two M19 surfaces in one program:
//!   * `import sys` + `sys.argv` indexing — happy path: parse three
//!     integers, sum them, exit 0.
//!   * `from sys import exit; exit(1)` — sad path: pass a non-numeric
//!     arg, observe the program terminates with exit code 1 and prints
//!     a diagnostic.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

fn compile_and_write() -> PathBuf {
    let src_path = project_root().join("examples").join("sum_args.spy");
    let src = fs::read_to_string(&src_path).expect("read sum_args.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile sum_args.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sum_args.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");
    spyc_path
}

#[test]
fn sum_args_compiles() {
    let _ = compile_and_write();
}

#[test]
fn sum_args_with_valid_inputs_prints_sum() {
    let spyc_path = compile_and_write();
    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }
    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg("10")
        .arg("20")
        .arg("30")
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(output.status.success(),
        "expected success; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status);
    assert!(stdout.contains("sum = 60"),
        "expected 'sum = 60'; stdout:\n{stdout}");
}

#[test]
fn sum_args_with_no_inputs_prints_zero() {
    let spyc_path = compile_and_write();
    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }
    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(output.status.success(), "should succeed");
    assert!(stdout.contains("sum = 0"),
        "expected 'sum = 0' with no args; stdout:\n{stdout}");
}

#[test]
fn sum_args_with_garbage_input_exits_one_via_sys_exit() {
    let spyc_path = compile_and_write();
    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }
    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg("42")
        .arg("not_a_number")
        .arg("7")
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    // Exit code 1 — Windows reports it as Some(1).
    assert_eq!(
        output.status.code(),
        Some(1),
        "expected exit code 1; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // Diagnostic printed before sys.exit fires.
    assert!(stdout.contains("sum_args: cannot parse not_a_number"),
        "missing parse diagnostic; stdout:\n{stdout}");
    // The "sum = ..." line must NOT have appeared.
    assert!(!stdout.contains("sum ="),
        "sys.exit failed to short-circuit; stdout:\n{stdout}");
}
