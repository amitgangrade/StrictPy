//! Subprocess integration test for `examples/tabular_groupby_demo.spy` (M38).
//!
//! Compiles + runs the M38 group-by walkthrough through `spy.exe` and
//! asserts the printed output is consistent with the documented
//! group_by → sum/mean → agg/spec workflow.  See the demo file's
//! docstring for the user-facing surface tour.

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
fn tabular_groupby_demo_compiles() {
    let src_path = project_root().join("examples").join("tabular_groupby_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_groupby_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_groupby_demo.spy: {e}"));
}

#[test]
fn tabular_groupby_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("tabular_groupby_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_groupby_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_groupby_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tabular_groupby_demo.spyc");
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
    assert!(stdout.contains("nrows=6"), "stdout:\n{stdout}");
    assert!(stdout.contains("ncols=3"), "stdout:\n{stdout}");
    // Filter (between 1.0 and 30.0 — drops the 40.0 and 60.0 tools).
    assert!(stdout.contains("filtered_nrows=4"), "stdout:\n{stdout}");
    // Per-column aggregation on filtered frame.  3 food rows
    // (10, 7, 3 = 20) + 1 tools row (2) = 22.
    assert!(stdout.contains("qty_total=22"), "stdout:\n{stdout}");
    // Group-by.
    assert!(stdout.contains("groups=2"), "stdout:\n{stdout}");
    // food: 10 + 7 + 3 = 20; tools: 2.
    assert!(stdout.contains("food_qty=20"), "stdout:\n{stdout}");
    assert!(stdout.contains("tools_qty=2"), "stdout:\n{stdout}");
    // Custom agg specs: category + qty_sum + price_mean = 3 cols.
    assert!(stdout.contains("agg_cols=3"), "stdout:\n{stdout}");
    // Rename.
    assert!(stdout.contains("renamed_first=cat"), "stdout:\n{stdout}");
    assert!(stdout.contains("done"), "stdout:\n{stdout}");
}
