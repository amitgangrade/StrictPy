//! Subprocess integration test for `examples/tabular_index_demo.spy` (M41).
//!
//! Compiles + runs the M41 DatetimeIndex + pivot_table walkthrough
//! through `spy.exe` and asserts the printed output matches the
//! documented set_index -> resample_index -> sort_index -> pivot_table
//! -> asof_merge_index -> select_by_label_str -> reset_index workflow.
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
fn tabular_index_demo_compiles() {
    let src_path = project_root().join("examples").join("tabular_index_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_index_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_index_demo.spy: {e}"));
}

#[test]
fn tabular_index_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("tabular_index_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_index_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_index_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tabular_index_demo.spyc");
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
    assert!(stdout.contains("trades_nrows=6"), "stdout:\n{stdout}");

    // set_index: ts moves into the index, leaving 4 regular columns
    // (symbol, side, qty, price).
    assert!(stdout.contains("indexed_has=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("index_name=ts"), "stdout:\n{stdout}");
    assert!(stdout.contains("indexed_ncols=4"), "stdout:\n{stdout}");

    // resample_index daily-sum: 6 buckets (days 0..5).  Sums of qty:
    // day 0 = 10, day 1 = 5, day 2 = 3, day 3 = 7, day 4 = 2, day 5 = 4.
    assert!(stdout.contains("daily_nrows=6"), "stdout:\n{stdout}");
    assert!(stdout.contains("daily_has=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("d0=10"), "stdout:\n{stdout}");
    assert!(stdout.contains("d5=4"), "stdout:\n{stdout}");

    // sort_index descending: rev0 is the day-5 bucket (qty=4).
    assert!(stdout.contains("rev0=4"), "stdout:\n{stdout}");

    // pivot_table: AAPL has 4 trades, GOOG has 2.  M43 promotes the
    // index_col ("symbol") to the output's index; 2 rows × 2 side cols.
    assert!(stdout.contains("pivot_nrows=2"), "stdout:\n{stdout}");
    assert!(stdout.contains("pivot_ncols=2"), "stdout:\n{stdout}");
    // AAPL buys = 10 + 7 + 4 = 21
    // GOOG buys = 2
    // AAPL sells = 3
    // First-seen index order: AAPL then GOOG.
    assert!(stdout.contains("pv_buy0=21"), "stdout:\n{stdout}");
    assert!(stdout.contains("pv_buy1=2"), "stdout:\n{stdout}");
    assert!(stdout.contains("pv_sell0=3"), "stdout:\n{stdout}");

    // asof_merge_index: indexed has 6 events.
    assert!(stdout.contains("merged_nrows=6"), "stdout:\n{stdout}");
    assert!(stdout.contains("merged_has=true"), "stdout:\n{stdout}");
    // event[0].ts=0 -> matches rates[0] (ts=0, rate=0.05)
    // event[5].ts=5d -> matches rates[1] (ts=3d, rate=0.07)
    assert!(stdout.contains("mr0=0.05"), "stdout:\n{stdout}");
    assert!(stdout.contains("mr5=0.07"), "stdout:\n{stdout}");

    // select_by_label_str on the pivot output keyed by symbol — AAPL row.
    assert!(stdout.contains("aapl_nrows=1"), "stdout:\n{stdout}");
    assert!(stdout.contains("aapl_buy=21"), "stdout:\n{stdout}");

    // reset_index: ts comes back as a column at position 0; ncols=5.
    assert!(stdout.contains("flat_has=false"), "stdout:\n{stdout}");
    assert!(stdout.contains("flat_ncols=5"), "stdout:\n{stdout}");

    assert!(stdout.contains("done"), "stdout:\n{stdout}");
}
