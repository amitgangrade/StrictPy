//! Subprocess integration test for `examples/tabular_m49_codes_demo.spy`.
//!
//! Compiles + runs the M49 codes-optimization walkthrough through
//! `spy.exe` and asserts the printed output matches the documented
//! categorical-codes / ordered / merge / resample / unstack workflow.
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
fn tabular_m49_codes_demo_compiles() {
    let src_path = project_root().join("examples").join("tabular_m49_codes_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_m49_codes_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_m49_codes_demo.spy: {e}"));
}

#[test]
fn tabular_m49_codes_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("tabular_m49_codes_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tabular_m49_codes_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tabular_m49_codes_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tabular_m49_codes_demo.spyc");
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

    // 1. codes-hash group_by
    assert!(stdout.contains("df.length=6"), "stdout:\n{stdout}");
    // The ordered categorical has 3 used categories (hot, cold, warm)
    // out of 4 in the categories[] table; group_by sees 3 groups.
    assert!(stdout.contains("ngroups=3"), "stdout:\n{stdout}");
    // 2. is_ordered: explicit categories with `extreme` unused -> true.
    assert!(stdout.contains("is_ordered=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("rebuilt_len=6"), "stdout:\n{stdout}");
    // 3. merge_codes_hash: df1 has k=a,b,c; df2 has k=b,c -> 2 matches.
    assert!(stdout.contains("merged_rows=2"), "stdout:\n{stdout}");
    // 4. 14 days = 2 weekly buckets; spans 1 calendar month (all in Jan).
    assert!(stdout.contains("weekly_buckets=2"), "stdout:\n{stdout}");
    assert!(stdout.contains("monthly_buckets=1"), "stdout:\n{stdout}");
    // 5. unstack of 2 regular cols × 2 inner values = 4 wide cols.
    assert!(stdout.contains("unstack_ncols=4"), "stdout:\n{stdout}");
    assert!(stdout.contains("done"), "stdout:\n{stdout}");
}
