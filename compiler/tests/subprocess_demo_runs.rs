//! Subprocess integration test for `examples/subprocess_demo.spy` (M23 P3a-A).
//!
//! Compiles + runs the subprocess demo through `spy.exe` and asserts the
//! printed output covers `run`, `run_with_stdin`, `spawn` + `wait`, plus
//! the bogus-spawn IOError path.  Mirrors the pattern from M22 P2D's
//! struct_demo_runs.rs.

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
fn subprocess_demo_compiles() {
    let src_path = project_root().join("examples").join("subprocess_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read subprocess_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile subprocess_demo.spy: {e}"));
}

#[test]
fn subprocess_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("subprocess_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read subprocess_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile subprocess_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("subprocess_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
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

    // Platform banner.
    assert!(stdout.contains("platform:"), "stdout:\n{stdout}");
    // `subprocess.run` happy path.
    assert!(stdout.contains("run.exit: 0"), "stdout:\n{stdout}");
    assert!(stdout.contains("run.stdout: ok"), "stdout:\n{stdout}");
    // Non-zero exit-code propagates.
    assert!(stdout.contains("exit7.code: 7"), "stdout:\n{stdout}");
    // run_with_stdin round-trips.
    assert!(stdout.contains("stdin.exit: 0"), "stdout:\n{stdout}");
    assert!(stdout.contains("stdin.stdout: ok"), "stdout:\n{stdout}");
    // spawn + wait.
    assert!(stdout.contains("spawned handle > 0: true"), "stdout:\n{stdout}");
    assert!(stdout.contains("wait.code: 0"), "stdout:\n{stdout}");
    // Bogus spawn raises IOError that the user catches.
    assert!(stdout.contains("caught IOError on bogus spawn"), "stdout:\n{stdout}");
    assert!(stdout.contains("done"), "stdout:\n{stdout}");
}
