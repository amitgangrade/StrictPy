//! Subprocess integration test for
//! `examples/tabular_multiindex_propagation_demo.spy` (M45).
//!
//! Compiles + runs the M45 MultiIndex-propagation walkthrough through
//! `spy.exe` and asserts every printed checkpoint matches the
//! documented end-to-end flow through sort_by → dropna_subset →
//! fillna_i64 → rename → set_index_multi → concat_cols → select →
//! melt.  At every step the MultiIndex (nlev=2) must survive.

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
fn tabular_multiindex_propagation_demo_compiles() {
    let src_path = project_root()
        .join("examples")
        .join("tabular_multiindex_propagation_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read demo");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile demo: {e}"));
}

#[test]
fn tabular_multiindex_propagation_demo_runs_via_spy_exe() {
    let src_path = project_root()
        .join("examples")
        .join("tabular_multiindex_propagation_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read demo");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile demo: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("tabular_multiindex_propagation_demo.spyc");
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

    // Construction: 6 rows.
    assert!(stdout.contains("sales_nrows=6"), "stdout:\n{stdout}");

    // After set_index_multi: 1 regular col (total), nlev=2.
    assert!(stdout.contains("mi_nrows=6"), "stdout:\n{stdout}");
    assert!(stdout.contains("mi_ncols=1"), "stdout:\n{stdout}");
    assert!(stdout.contains("mi_nlev=2"), "stdout:\n{stdout}");

    // sort_by preserves MultiIndex.
    assert!(stdout.contains("sort_nrows=6"), "stdout:\n{stdout}");
    assert!(stdout.contains("sort_nlev=2"), "stdout:\n{stdout}");

    // dropna_subset(["total"]) drops the 1 null row.
    assert!(stdout.contains("clean_nrows=5"), "stdout:\n{stdout}");
    assert!(stdout.contains("clean_nlev=2"), "stdout:\n{stdout}");

    // fillna_i64 preserves MultiIndex (pass-through).
    assert!(stdout.contains("fill_nlev=2"), "stdout:\n{stdout}");

    // rename — MultiIndex preserved; the renamed column appears in cols().
    assert!(stdout.contains("ren_nlev=2"), "stdout:\n{stdout}");
    assert!(stdout.contains("ren_col0=amount"), "stdout:\n{stdout}");

    // round-trip reset + re-promote → MultiIndex back.
    assert!(stdout.contains("promo_nlev=2"), "stdout:\n{stdout}");

    // concat_cols — lhs MultiIndex wins.  promo had 1 regular col
    // (amount); rhs adds 1 (score) → ncols=2; nlev=2.
    assert!(stdout.contains("cc_ncols=2"), "stdout:\n{stdout}");
    assert!(stdout.contains("cc_nlev=2"), "stdout:\n{stdout}");

    // select(["amount","score"]) — MultiIndex preserved.
    assert!(stdout.contains("sel_ncols=2"), "stdout:\n{stdout}");
    assert!(stdout.contains("sel_nlev=2"), "stdout:\n{stdout}");

    // melt: 5 rows × 2 value_vars = 10 output rows.  nlev=2.
    assert!(stdout.contains("mlt_nrows=10"), "stdout:\n{stdout}");
    assert!(stdout.contains("mlt_nlev=2"), "stdout:\n{stdout}");

    // Levels accessible after melt.
    assert!(stdout.contains("mlt_l0_ok=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("mlt_l1_ok=true"), "stdout:\n{stdout}");
}
