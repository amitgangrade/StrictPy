//! Subprocess integration test for `examples/tabular_serve_demo.spy` (M50a).
//!
//! Compiles + runs the tabular.serve walkthrough through `spy.exe`
//! and asserts the program exited cleanly after the bundled
//! `serve_with_timeout` deadline.  The demo uses a 600ms timeout so the
//! test completes in well under a second.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn tabular_serve_demo_compiles() {
    let src_path = project_root().join("examples").join("tabular_serve_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_serve_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_serve_demo.spy: {e}"));
}

#[test]
fn tabular_serve_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("tabular_serve_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_serve_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_serve_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tabular_serve_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    let start = std::time::Instant::now();
    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .current_dir(project_root())
        .output()
        .expect("invoke spy.exe");
    let elapsed = start.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "spy.exe failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );

    // Construction.
    assert!(stdout.contains("built DataFrame with 6 rows"), "stdout:\n{stdout}");
    // Banner before the server boots.
    assert!(stdout.contains("Booting localhost HTTP server"), "stdout:\n{stdout}");
    // Endpoints documented.
    assert!(stdout.contains("/api/schema"), "stdout:\n{stdout}");
    assert!(stdout.contains("/api/filter"), "stdout:\n{stdout}");
    assert!(stdout.contains("/api/groupby"), "stdout:\n{stdout}");
    // Clean shutdown.
    assert!(stdout.contains("Server shut down with exit code: 0"), "stdout:\n{stdout}");
    // Should have completed well under 5 seconds (timeout is 600ms +
    // VM startup + bytecode load + GC teardown).
    assert!(
        elapsed < Duration::from_secs(15),
        "demo took too long: {:?}; stdout:\n{stdout}",
        elapsed
    );
}
