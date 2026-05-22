//! Subprocess integration test for `examples/tabular_multiindex_demo.spy` (M44).
//!
//! Compiles + runs the M44 MultiIndex walkthrough through `spy.exe` and
//! asserts the printed output is consistent with the documented
//! multi-column group_by → sort_index_multi → filter → reset_index_multi
//! workflow.

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
fn tabular_multiindex_demo_compiles() {
    let src_path = project_root()
        .join("examples")
        .join("tabular_multiindex_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_multiindex_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_multiindex_demo.spy: {e}"));
}

#[test]
fn tabular_multiindex_demo_runs_via_spy_exe() {
    let src_path = project_root()
        .join("examples")
        .join("tabular_multiindex_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_multiindex_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_multiindex_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tabular_multiindex_demo.spyc");
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
    assert!(stdout.contains("sales_nrows=8"), "stdout:\n{stdout}");

    // Multi-column group_by sum: 4 unique (reg, category) groups.
    // Only qty remains as a regular column; nlev = 2 (MultiIndex).
    assert!(stdout.contains("gb_nrows=4"), "stdout:\n{stdout}");
    assert!(stdout.contains("gb_ncols=1"), "stdout:\n{stdout}");
    assert!(stdout.contains("gb_nlev=2"), "stdout:\n{stdout}");

    // Level names survive.
    assert!(stdout.contains("lvl0_name=reg"), "stdout:\n{stdout}");
    assert!(stdout.contains("lvl1_name=category"), "stdout:\n{stdout}");

    // sort_index_multi ascending:
    //   east-books (10+8=18), east-toys (5+3=8),
    //   west-books (20+12=32), west-toys (15+7=22)
    assert!(stdout.contains("sorted_q0=18"), "stdout:\n{stdout}");
    assert!(stdout.contains("sorted_q1=8"), "stdout:\n{stdout}");
    assert!(stdout.contains("sorted_q2=32"), "stdout:\n{stdout}");
    assert!(stdout.contains("sorted_q3=22"), "stdout:\n{stdout}");

    // filter qty > 12: keeps east-books (18), west-books (32), west-toys (22)
    // = 3 rows; MultiIndex preserved (nlev=2).
    assert!(stdout.contains("flt_nrows=3"), "stdout:\n{stdout}");
    assert!(stdout.contains("flt_nlev=2"), "stdout:\n{stdout}");

    // index_level access.
    assert!(stdout.contains("lvl0_ok=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("lvl1_ok=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("lvl9_oob=true"), "stdout:\n{stdout}");

    // reset_index_multi — both levels back as regular columns; RangeIndex.
    assert!(stdout.contains("flat_nrows=3"), "stdout:\n{stdout}");
    assert!(stdout.contains("flat_ncols=3"), "stdout:\n{stdout}");
    assert!(stdout.contains("flat_nlev=0"), "stdout:\n{stdout}");
    assert!(stdout.contains("flat_col0=reg"), "stdout:\n{stdout}");
    assert!(stdout.contains("flat_col1=category"), "stdout:\n{stdout}");
    assert!(stdout.contains("flat_col2=qty"), "stdout:\n{stdout}");
}
