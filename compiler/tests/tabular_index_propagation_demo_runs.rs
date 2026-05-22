//! Subprocess integration test for `examples/tabular_index_propagation_demo.spy` (M42).
//!
//! Compiles + runs the M42 index-propagation walkthrough through
//! `spy.exe` and asserts the printed output matches the documented
//! filter -> sort_by -> dropna_subset -> fillna_f64 -> merge -> select
//! -> sort_index workflow.  Each step preserves the parent's index
//! per the M42 propagation rules.

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
fn tabular_index_propagation_demo_compiles() {
    let src_path = project_root().join("examples").join("tabular_index_propagation_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read demo");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile demo: {e}"));
}

#[test]
fn tabular_index_propagation_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("tabular_index_propagation_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read demo");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile demo: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tabular_index_propagation_demo.spyc");
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
    assert!(stdout.contains("trades_nrows=5"), "stdout:\n{stdout}");

    // set_index: trade_id moves into the index.
    assert!(stdout.contains("indexed_has=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("index_name=trade_id"), "stdout:\n{stdout}");

    // filter — amount >= 1000 keeps rows 0,1,2,4 (4 rows; row 3 has
    // amount=750).  M42: index preserved.
    assert!(stdout.contains("filt_nrows=4"), "stdout:\n{stdout}");
    assert!(stdout.contains("filt_has=true"), "stdout:\n{stdout}");

    // sort_by price descending.  M42: index threads through.
    assert!(stdout.contains("sort_has=true"), "stdout:\n{stdout}");

    // dropna_subset on counterparty drops the row where counterparty was
    // null (row 1, trade_id=101).  3 rows remain.  M42: index restricted.
    assert!(stdout.contains("clean_nrows=3"), "stdout:\n{stdout}");
    assert!(stdout.contains("clean_has=true"), "stdout:\n{stdout}");

    // fillna_f64 — index unchanged.
    assert!(stdout.contains("filled_has=true"), "stdout:\n{stdout}");

    // left-merge with traders.  All 3 lhs rows preserved.  M42: lhs
    // index preserved.
    assert!(stdout.contains("enr_nrows=3"), "stdout:\n{stdout}");
    assert!(stdout.contains("enr_has=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("enr_name=trade_id"), "stdout:\n{stdout}");

    // select — 3 columns, index unchanged.
    assert!(stdout.contains("tidy_ncols=3"), "stdout:\n{stdout}");
    assert!(stdout.contains("tidy_has=true"), "stdout:\n{stdout}");

    // sort_index ascending: smallest trade_id first.  Surviving trades
    // (filter -> dropna) are trade_ids 100, 102, 104.  After
    // sort_index(true) the order is 100, 102, 104.
    assert!(stdout.contains("final_has=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("t0=100"), "stdout:\n{stdout}");
    assert!(stdout.contains("t1=102"), "stdout:\n{stdout}");

    assert!(stdout.contains("done"), "stdout:\n{stdout}");
}
