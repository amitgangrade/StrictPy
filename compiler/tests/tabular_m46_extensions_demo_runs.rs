//! Subprocess integration test for
//! `examples/tabular_m46_extensions_demo.spy` (M46).
//!
//! Compiles + runs the M46 stack/unstack + loc_range + set_index_list
//! + pivot_table extensions walkthrough through `spy.exe` and asserts
//! every printed checkpoint.

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
fn tabular_m46_extensions_demo_compiles() {
    let src_path = project_root()
        .join("examples")
        .join("tabular_m46_extensions_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read demo");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile demo: {e}"));
}

#[test]
fn tabular_m46_extensions_demo_runs_via_spy_exe() {
    let src_path = project_root()
        .join("examples")
        .join("tabular_m46_extensions_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read demo");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile demo: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("tabular_m46_extensions_demo.spyc");
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

    // Construction: 6 sales rows, 3 cols (region, jan, feb).
    assert!(stdout.contains("sales_nrows=6"), "stdout:\n{stdout}");
    assert!(stdout.contains("sales_ncols=3"), "stdout:\n{stdout}");

    // set_index_list(["region"]) — 1-element list → single-col index.
    assert!(stdout.contains("idx_nlev=1"), "stdout:\n{stdout}");
    assert!(stdout.contains("idx_has=true"), "stdout:\n{stdout}");

    // stack() — 6 rows × 2 regular cols (jan, feb) → 12 rows; 2-level MI.
    assert!(stdout.contains("stk_nrows=12"), "stdout:\n{stdout}");
    assert!(stdout.contains("stk_nlev=2"), "stdout:\n{stdout}");
    assert!(stdout.contains("stk_ncols=1"), "stdout:\n{stdout}");

    // unstack() — round-trip restores the single-col index shape.
    // 6 original regions → 6 rows; one column per unique inner-level
    // value (jan, feb).  But the row key here is just "region" with
    // duplicates (east appears twice), so the unique row keys are 5
    // distinct regions: east, north, south, west = 4 distinct,
    // but east is duplicated so we get 4 distinct row keys.
    // Actually east appears 2x, so distinct = 4 (east, north, south, west).
    assert!(stdout.contains("uns_nrows=4"), "stdout:\n{stdout}");
    assert!(stdout.contains("uns_nlev=1"), "stdout:\n{stdout}");

    // loc_range_str("east", "south") — keeps east, north, south.
    // east appears 2x + north 1x + south 2x = 5 rows.  Lex: east < north < south < west.
    assert!(stdout.contains("lr_nrows=5"), "stdout:\n{stdout}");

    // pivot_table_aggfunc_list (cat, month, total, ["sum","mean"]) —
    // 3 month col_keys × 2 aggfuncs = 6 output cols; 2 row_keys
    // (books, toys).
    assert!(stdout.contains("ptm_ncols=6"), "stdout:\n{stdout}");
    assert!(stdout.contains("ptm_nrows=2"), "stdout:\n{stdout}");

    // pivot_table_margins — body 2x3, with margins -> 3x4.
    assert!(stdout.contains("ptg_ncols=4"), "stdout:\n{stdout}");
    assert!(stdout.contains("ptg_nrows=3"), "stdout:\n{stdout}");

    // set_index_list with 2 elements -> MultiIndex (nlev=2).
    assert!(stdout.contains("mil_nlev=2"), "stdout:\n{stdout}");
}
