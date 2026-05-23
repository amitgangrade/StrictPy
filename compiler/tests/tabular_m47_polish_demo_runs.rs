//! Subprocess integration test for
//! `examples/tabular_m47_polish_demo.spy` (M47).
//!
//! Compiles + runs the M47 polish-round walkthrough (iloc_2d +
//! negative iloc + rolling_*_min_periods + ColumnCategorical) and
//! asserts every printed checkpoint.

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
fn tabular_m47_polish_demo_compiles() {
    let src_path = project_root()
        .join("examples")
        .join("tabular_m47_polish_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read demo");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile demo: {e}"));
}

#[test]
fn tabular_m47_polish_demo_runs_via_spy_exe() {
    let src_path = project_root()
        .join("examples")
        .join("tabular_m47_polish_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read demo");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile demo: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("tabular_m47_polish_demo.spyc");
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

    // 8 sales rows, 3 cols (region, qty, price).
    assert!(stdout.contains("sales_nrows=8"), "stdout:\n{stdout}");
    assert!(stdout.contains("sales_ncols=3"), "stdout:\n{stdout}");

    // ColumnCategorical: 8 cells; 4 distinct categories (east, north,
    // west, south in first-appearance order).
    assert!(stdout.contains("cat_len=8"), "stdout:\n{stdout}");
    assert!(stdout.contains("cat_dt=categorical"), "stdout:\n{stdout}");
    assert!(stdout.contains("cat_k=4"), "stdout:\n{stdout}");
    assert!(stdout.contains("cat0=east"), "stdout:\n{stdout}");
    assert!(stdout.contains("codes_len=8"), "stdout:\n{stdout}");

    // group_by(["region"]).sum() — 4 groups, single-col index.
    assert!(stdout.contains("g_nrows=4"), "stdout:\n{stdout}");
    assert!(stdout.contains("g_nlev=1"), "stdout:\n{stdout}");

    // rolling_mean_min_periods(3, 1) on qty:
    //   length = 8, null_count = 0 (min_periods=1 emits every cell).
    assert!(stdout.contains("r_len=8"), "stdout:\n{stdout}");
    assert!(stdout.contains("r_nc=0"), "stdout:\n{stdout}");
    // Position 0 = mean([10]) = 10.
    assert!(stdout.contains("r0=10"), "stdout:\n{stdout}");

    // iloc_2d(-5, -1, 0, 2) on 8 rows / 3 cols:
    //   rows [-5..-1) = [3..7) = 4 rows; cols [0..2) = 2 cols.
    assert!(stdout.contains("s2d_nrows=4"), "stdout:\n{stdout}");
    assert!(stdout.contains("s2d_ncols=2"), "stdout:\n{stdout}");

    // iloc(-3, 8) on 8 rows = rows [5..8) = 3 rows.
    assert!(stdout.contains("sneg_nrows=3"), "stdout:\n{stdout}");

    // df.get_column_categorical hit / miss.
    assert!(stdout.contains("got_cat_len=8"), "stdout:\n{stdout}");
    assert!(stdout.contains("miss-none"), "stdout:\n{stdout}");
}
