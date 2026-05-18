//! Subprocess integration test for `examples/stats_demo.spy` (M22 P2C).
//!
//! Compiles + runs the statistics demo through `spy.exe` and asserts
//! the printed values match expectation.

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
fn stats_demo_compiles() {
    let src_path = project_root().join("examples").join("stats_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read stats_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile stats_demo.spy: {e}"));
}

#[test]
fn stats_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("stats_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read stats_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile stats_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("stats_demo.spyc");
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

    // n=8.
    assert!(stdout.contains("n=8"), "expected n=8; stdout:\n{stdout}");
    // sum=40.
    assert!(stdout.contains("sum=40"), "expected sum=40; stdout:\n{stdout}");
    // mean=5.
    assert!(stdout.contains("mean=5"), "expected mean=5; stdout:\n{stdout}");
    // median=4.5.
    assert!(stdout.contains("median=4.5"), "expected median=4.5; stdout:\n{stdout}");
    // min=2, max=9.
    assert!(stdout.contains("min=2"), "expected min=2; stdout:\n{stdout}");
    assert!(stdout.contains("max=9"), "expected max=9; stdout:\n{stdout}");
    // q0.5 = median = 4.5.
    assert!(
        stdout.contains("q0.5=4.5"),
        "expected q0.5=4.5; stdout:\n{stdout}"
    );
    // Mode of ["red","blue","red","green","red","blue"] = "red".
    assert!(stdout.contains("mode=red"), "expected mode=red; stdout:\n{stdout}");
    // Final OK banner — exact-arithmetic asserts inside stats_demo.spy.
    assert!(
        stdout.contains("OK: all stats correct"),
        "expected OK banner; stdout:\n{stdout}"
    );
}
