//! Subprocess integration test for `examples/tabular_demo.spy` (M37).
//!
//! Compiles + runs the tabular module's walkthrough through `spy.exe`
//! and asserts the printed output is consistent with an end-to-end
//! construct → CSV round-trip → filter → sort → project workflow.
//! See the demo file's docstring for the user-facing surface tour.

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
fn tabular_demo_compiles() {
    let src_path = project_root().join("examples").join("tabular_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_demo.spy: {e}"));
}

#[test]
fn tabular_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("tabular_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tabular_demo.spyc");
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

    // Construction.
    assert!(stdout.contains("nrows=4"), "initial nrows; stdout:\n{stdout}");
    assert!(stdout.contains("ncols=2"), "initial ncols; stdout:\n{stdout}");
    assert!(stdout.contains("has_age=true"), "has_age; stdout:\n{stdout}");
    assert!(stdout.contains("has_height=false"), "has_height; stdout:\n{stdout}");
    // CSV round-trip.
    assert!(stdout.contains("rt_nrows=4"), "rt rows; stdout:\n{stdout}");
    // Filter (age > 25 → alice (30), carol (40), bob's 25 excluded, dave's null excluded).
    assert!(stdout.contains("mask_count=2"), "mask count; stdout:\n{stdout}");
    // Sorted by name asc — alice first.
    assert!(stdout.contains("--- sorted by name asc ---"), "sort header; stdout:\n{stdout}");
    // Numeric sort descending puts the null row at the end (dave's
    // age was null in the demo's construction step).
    assert!(stdout.contains("nulls_at_end=null"), "nulls last; stdout:\n{stdout}");
    // After projection.
    assert!(stdout.contains("head1_nrows=1"), "head1; stdout:\n{stdout}");
    assert!(stdout.contains("tail1_nrows=1"), "tail1; stdout:\n{stdout}");
    assert!(stdout.contains("done"), "done; stdout:\n{stdout}");
}
