//! Subprocess integration test for `examples/tabular_reshape_demo.spy` (M39).
//!
//! Compiles + runs the M39 reshape walkthrough through `spy.exe` and
//! asserts the printed output matches the documented unique →
//! value_counts → merge → pivot → melt → concat workflow.  See the
//! demo file's docstring for the user-facing surface tour.

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
fn tabular_reshape_demo_compiles() {
    let src_path = project_root().join("examples").join("tabular_reshape_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_reshape_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_reshape_demo.spy: {e}"));
}

#[test]
fn tabular_reshape_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("tabular_reshape_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_reshape_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_reshape_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tabular_reshape_demo.spyc");
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
    assert!(stdout.contains("orders_nrows=5"), "stdout:\n{stdout}");
    assert!(stdout.contains("customers_nrows=3"), "stdout:\n{stdout}");
    // unique on region — three distinct regions.
    assert!(stdout.contains("unique_regions=3"), "stdout:\n{stdout}");
    // value_counts: 3 rows; top tied between us(2) and uk(2), first
    // occurrence wins (us appears first in row 0).
    assert!(stdout.contains("vc_nrows=3"), "stdout:\n{stdout}");
    assert!(stdout.contains("vc_top_row=us:2"), "stdout:\n{stdout}");
    // Inner merge: 5 rows × 4 columns (customer_id, region, qty, name).
    assert!(stdout.contains("merged_nrows=5"), "stdout:\n{stdout}");
    assert!(stdout.contains("merged_ncols=4"), "stdout:\n{stdout}");
    // Pivot: 3 unique regions × 3 unique names = 3×3.  M43 promoted
    // "region" to the index, so ncols dropped from 4 to 3.
    assert!(stdout.contains("piv_nrows=3"), "stdout:\n{stdout}");
    assert!(stdout.contains("piv_ncols=3"), "stdout:\n{stdout}");
    // Melt: 3 regions × 3 name-value-vars = 9 rows.
    assert!(stdout.contains("melted_nrows=9"), "stdout:\n{stdout}");
    // concat_rows stacking orders twice → 10 rows.
    assert!(stdout.contains("stacked_nrows=10"), "stdout:\n{stdout}");
    assert!(stdout.contains("done"), "stdout:\n{stdout}");
}
