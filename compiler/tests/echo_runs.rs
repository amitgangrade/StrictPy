//! Integration test for `examples/echo.spy` — the smallest M19 `sys.argv`
//! consumer. Runs via the out-of-process `spy.exe` so we exercise the
//! real CLI argv plumbing (`run_file_with_args` ← `clap` ← argv[]).
//!
//! What's tested:
//!   1. The program COMPILES via `compile_source`.
//!   2. Invoking `spy.exe echo.spyc alpha beta gamma` prints the
//!      argv lines including argv[0] (the .spyc path).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn echo_compiles() {
    let src_path = project_root().join("examples").join("echo.spy");
    let src = fs::read_to_string(&src_path).expect("read echo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile echo.spy: {e}"));
}

#[test]
fn echo_prints_all_argv_elements_via_spy_exe() {
    let src_path = project_root().join("examples").join("echo.spy");
    let src = fs::read_to_string(&src_path).expect("read echo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile echo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("echo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present (run `cargo build --release` first)", spy_bin.display());
        return;
    }

    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg("alpha")
        .arg("beta")
        .arg("gamma")
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(output.status.success(),
        "spy.exe failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status);

    // argv[0] is the .spyc path (we only assert the suffix to dodge
    // cross-platform path differences).
    assert!(
        stdout.contains("argv[0]") && stdout.contains("echo.spyc"),
        "missing argv[0] line; stdout:\n{stdout}"
    );
    assert!(stdout.contains("argv[1] = alpha"), "stdout:\n{stdout}");
    assert!(stdout.contains("argv[2] = beta"),  "stdout:\n{stdout}");
    assert!(stdout.contains("argv[3] = gamma"), "stdout:\n{stdout}");
}
