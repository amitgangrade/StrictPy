//! Subprocess integration test for `examples/timer_demo.spy` (M20b).
//!
//! Compiles + runs the monotonic-clock micro-benchmark through `spy.exe`
//! and asserts the printed sum (deterministic) and the
//! elapsed-non-negative flag (any host-clock value satisfies this).

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
fn timer_demo_compiles() {
    let src_path = project_root().join("examples").join("timer_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read timer_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile timer_demo.spy: {e}"));
}

#[test]
fn timer_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("timer_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read timer_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile timer_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("timer_demo.spyc");
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

    // Sum 1..=100_000 = 5_000_050_000.
    assert!(
        stdout.contains("sum=5000050000"),
        "expected sum line; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("elapsed-ms-positive=true"),
        "elapsed should be non-negative; stdout:\n{stdout}"
    );
}
