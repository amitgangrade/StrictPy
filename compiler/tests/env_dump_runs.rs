//! Subprocess integration test for `examples/env_dump.spy` (M20a).
//!
//! Sets PATH from the test side to a known synthetic value to make the
//! assertion deterministic across hosts.  Asserts that the header line
//! and at least one of our synthetic entries appears.

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
fn env_dump_compiles() {
    let src_path = project_root().join("examples").join("env_dump.spy");
    let src = fs::read_to_string(&src_path).expect("read env_dump.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile env_dump.spy: {e}"));
}

#[test]
fn env_dump_prints_set_entries_via_spy_exe() {
    let src_path = project_root().join("examples").join("env_dump.spy");
    let src = fs::read_to_string(&src_path).expect("read env_dump.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile env_dump.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("env_dump.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    // Build a synthetic PATH using the platform separator so we can
    // assert on a known entry regardless of the real environment.
    let sep = if cfg!(windows) { ";" } else { ":" };
    let synthetic = format!("/alpha{sep}/beta{sep}/gamma");

    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .env("PATH", &synthetic)
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(output.status.success(),
        "spy.exe failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status);

    assert!(stdout.contains("PATH entries:"),
        "missing header; stdout:\n{stdout}");
    assert!(stdout.contains("/alpha"),
        "first synthetic entry missing; stdout:\n{stdout}");
    assert!(stdout.contains("/beta"),
        "middle synthetic entry missing; stdout:\n{stdout}");
    assert!(stdout.contains("/gamma"),
        "last synthetic entry missing; stdout:\n{stdout}");
}
