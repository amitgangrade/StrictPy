//! Subprocess integration test for `examples/argparse_demo.spy` (M22 P2A).
//!
//! Compiles + runs the argparse demo through `spy.exe` and asserts:
//!   - the happy path (positional + flag + opt) prints the expected lines,
//!   - `--help` short-circuits to the auto-generated usage text,
//!   - a missing positional triggers the ValueError handler.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

fn compile_demo() -> PathBuf {
    let src_path = project_root().join("examples").join("argparse_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read argparse_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile argparse_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("argparse_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");
    spyc_path
}

#[test]
fn argparse_demo_compiles() {
    let _ = compile_demo();
}

#[test]
fn argparse_demo_happy_path() {
    let spyc_path = compile_demo();
    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }
    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg("hello.in")
        .arg("--verbose")
        .arg("--output")
        .arg("result.bin")
        .current_dir(project_root())
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "spy.exe failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );
    assert!(stdout.contains("input = hello.in"), "got: {stdout}");
    assert!(stdout.contains("output = result.bin"), "got: {stdout}");
    assert!(stdout.contains("verbose = true"), "got: {stdout}");
}

#[test]
fn argparse_demo_help_request() {
    // Note on test design: clap (the spy.exe CLI driver) reserves
    // `--help` and `-h` for *itself*, even with `trailing_var_arg`.
    // Inside spy.exe, the user program never sees those tokens — they
    // get intercepted at the binary boundary.  The `--` clap-stop token
    // lets us pass `--help` through to the StrictPy `sys.argv`, which
    // is what end-users would do (or what an integration wrapper script
    // would do).  argparse's `help_requested` then picks it up.
    let spyc_path = compile_demo();
    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }
    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg("--")
        .arg("--help")
        .current_dir(project_root())
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(output.status.success(), "stdout:\n{stdout}");
    // The auto-generated usage block.
    assert!(stdout.contains("usage: argparse_demo"), "got: {stdout}");
    assert!(stdout.contains("<input>"), "got: {stdout}");
    assert!(stdout.contains("--verbose"), "got: {stdout}");
    assert!(stdout.contains("--output"), "got: {stdout}");
}

#[test]
fn argparse_demo_missing_positional_returns_2() {
    let spyc_path = compile_demo();
    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }
    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        // Deliberately omit the positional `input`.
        .current_dir(project_root())
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected exit code 2; stdout:\n{stdout}"
    );
    assert!(stdout.contains("error:"), "got: {stdout}");
    assert!(stdout.contains("input"), "got: {stdout}");
}
