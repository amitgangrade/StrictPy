//! Subprocess integration test for `examples/tabular_index_reshape_demo.spy` (M43).
//!
//! Compiles + runs the M43 reshape-index walkthrough through `spy.exe`
//! and asserts the printed output is consistent with the documented
//! pivot_table → group_by → concat_rows → melt index-aware workflow.

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
fn tabular_index_reshape_demo_compiles() {
    let src_path = project_root()
        .join("examples")
        .join("tabular_index_reshape_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_index_reshape_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_index_reshape_demo.spy: {e}"));
}

#[test]
fn tabular_index_reshape_demo_runs_via_spy_exe() {
    let src_path = project_root()
        .join("examples")
        .join("tabular_index_reshape_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_index_reshape_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_index_reshape_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tabular_index_reshape_demo.spyc");
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
    assert!(stdout.contains("ix_has=true"), "stdout:\n{stdout}");

    // pivot_table: 2 symbols × 2 sides; symbol → index.
    assert!(stdout.contains("pt_nrows=2"), "stdout:\n{stdout}");
    assert!(stdout.contains("pt_ncols=2"), "stdout:\n{stdout}");
    assert!(stdout.contains("pt_has=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("pt_iname=symbol"), "stdout:\n{stdout}");

    // group_by mean: symbol → index (single-column promotion).  All
    // numeric non-key cols (trade_id + qty + price) get aggregated.
    assert!(stdout.contains("m_ncols=3"), "stdout:\n{stdout}");
    assert!(stdout.contains("m_has=true"), "stdout:\n{stdout}");
    // AAPL qty mean = (10+5+8)/3 ≈ 7.666...; GOOG = (7+3+2)/3 = 4.0
    assert!(
        stdout.contains("m_aapl=7.666") || stdout.contains("m_aapl=7.67"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("m_goog=4"), "stdout:\n{stdout}");

    // concat_rows: 3 + 3 = 6 rows; shared trade_id index.
    assert!(stdout.contains("st_nrows=6"), "stdout:\n{stdout}");
    assert!(stdout.contains("st_has=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("st_t0=100"), "stdout:\n{stdout}");
    assert!(stdout.contains("st_t5=105"), "stdout:\n{stdout}");

    // melt: 6 input rows × 2 value_vars = 12 output rows; trade_id index
    // each label repeated twice.
    assert!(stdout.contains("ml_nrows=12"), "stdout:\n{stdout}");
    assert!(stdout.contains("ml_has=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("ml_iname=trade_id"), "stdout:\n{stdout}");
    assert!(stdout.contains("ml_t0=100"), "stdout:\n{stdout}");
    assert!(stdout.contains("ml_t1=100"), "stdout:\n{stdout}");

    assert!(stdout.contains("done"), "stdout:\n{stdout}");
}
