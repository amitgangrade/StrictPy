//! Subprocess integration test for `examples/datetime_demo.spy` (M23 P3a-B).
//!
//! Compiles + runs the datetime-module demo through `spy.exe` and asserts
//! the printed values match expectation.  The asserts inside the demo
//! itself are the authoritative correctness check; this test merely
//! confirms the program prints its OK banner and the expected sentinel
//! values land in stdout.

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
fn datetime_demo_compiles() {
    let src_path = project_root().join("examples").join("datetime_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read datetime_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile datetime_demo.spy: {e}"));
}

#[test]
fn datetime_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("datetime_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read datetime_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile datetime_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("datetime_demo.spyc");
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

    // Every internal assert passed.
    assert!(
        stdout.contains("OK"),
        "expected OK banner; stdout:\n{stdout}"
    );
    // Pretty-printed lines.
    assert!(
        stdout.contains("date=2026-05-19"),
        "expected date= line; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("time=14:23:11"),
        "expected time= line; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("iso=2026-05-19T14:23:11Z"),
        "expected iso= line; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("weekday=1"),
        "expected weekday=1 (Tuesday); stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("days-in-30=30"),
        "expected days-in-30=30; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("local-offset-in-range=true"),
        "expected local-offset-in-range=true; stdout:\n{stdout}"
    );
}
