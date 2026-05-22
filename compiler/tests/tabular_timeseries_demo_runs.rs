//! Subprocess integration test for `examples/tabular_timeseries_demo.spy` (M40).
//!
//! Compiles + runs the M40 time-series walkthrough through `spy.exe`
//! and asserts the printed output matches the documented cumsum →
//! rolling → resample → asof_merge → iloc → dropna workflow.  See
//! the demo file's docstring for the user-facing surface tour.

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
fn tabular_timeseries_demo_compiles() {
    let src_path = project_root().join("examples").join("tabular_timeseries_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_timeseries_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_timeseries_demo.spy: {e}"));
}

#[test]
fn tabular_timeseries_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("tabular_timeseries_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_timeseries_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_timeseries_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tabular_timeseries_demo.spyc");
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
    assert!(stdout.contains("events_nrows=6"), "stdout:\n{stdout}");
    assert!(stdout.contains("tax_nrows=2"), "stdout:\n{stdout}");
    // cumsum / cummax on cleaned amount [10, 20, 30, 0, 50, 60]:
    //   cumsum = [10, 30, 60, 60, 110, 170] → cumsum5 = 170
    //   cummax = [10, 20, 30, 30, 50, 60]   → cummax5 = 60
    assert!(stdout.contains("cumsum5=170"), "stdout:\n{stdout}");
    assert!(stdout.contains("cummax5=60"), "stdout:\n{stdout}");
    // rolling_mean window=3 on [10, 20, 30, 0, 50, 60]:
    //   index 2: mean(10,20,30) = 20
    //   index 5: mean(0,50,60) = 36.666...
    assert!(stdout.contains("rmean2=20"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("rmean5=36.66666"),
        "stdout:\n{stdout}"
    );
    // Resample daily sum — 6 buckets (days 0..5).
    assert!(stdout.contains("daily_nrows=6"), "stdout:\n{stdout}");
    assert!(stdout.contains("d0=10"), "stdout:\n{stdout}");
    // Day 3 had null amount → fill_i64(0) → sum = 0.
    assert!(stdout.contains("d3=0"), "stdout:\n{stdout}");
    // asof_merge: 6 events × (3 src cols + 1 rate col) = 4 columns.
    assert!(stdout.contains("merged_nrows=6"), "stdout:\n{stdout}");
    assert!(stdout.contains("merged_ncols=4"), "stdout:\n{stdout}");
    // event[0].ts=0 → matches tax[0] (ts=0, rate=0.05)
    // event[5].ts=5d → matches tax[1] (ts=3d, rate=0.07)
    assert!(stdout.contains("rate0=0.05"), "stdout:\n{stdout}");
    assert!(stdout.contains("rate5=0.07"), "stdout:\n{stdout}");
    // iloc(0, 3) → 3 rows.
    assert!(stdout.contains("head3_nrows=3"), "stdout:\n{stdout}");
    // dropna → 5 rows (one row had null amount).
    assert!(stdout.contains("clean_nrows=5"), "stdout:\n{stdout}");
    assert!(stdout.contains("done"), "stdout:\n{stdout}");
}
