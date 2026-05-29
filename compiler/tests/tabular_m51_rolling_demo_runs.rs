//! Subprocess integration test for
//! `examples/tabular_m51_rolling_demo.spy` (M51).
//!
//! Compiles + runs the M51 walkthrough (RollingWindow chainable +
//! center + min_periods + agg + is_ordered bit + categorical sort +
//! outer-MultiIndex loc_range_level) and asserts every checkpoint.

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
fn tabular_m51_rolling_demo_compiles() {
    let src_path = project_root()
        .join("examples")
        .join("tabular_m51_rolling_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read demo");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile demo: {e}"));
}

#[test]
fn tabular_m51_rolling_demo_runs_via_spy_exe() {
    let src_path = project_root()
        .join("examples")
        .join("tabular_m51_rolling_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read demo");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile demo: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("tabular_m51_rolling_demo.spyc");
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

    assert!(stdout.contains("nrows=5"), "stdout:\n{stdout}");

    // Step 2: trailing rolling(3).mean() — row 0 null, row 2 = mean(1,2,3)=2.0.
    assert!(stdout.contains("trail0=null"), "stdout:\n{stdout}");
    assert!(stdout.contains("trail2=2.0"), "stdout:\n{stdout}");

    // Step 3: centered rolling(3).mean() — both ends null, middle centered.
    assert!(stdout.contains("center0=null"), "stdout:\n{stdout}");
    assert!(stdout.contains("center2=3.0"), "stdout:\n{stdout}");
    assert!(stdout.contains("center4=null"), "stdout:\n{stdout}");

    // Step 4: min_periods(1).sum() — partial leading window sum([1])=1.
    assert!(stdout.contains("partial0=1"), "stdout:\n{stdout}");

    // Step 5: agg — 2 cols named qty_sum, price_mean.
    assert!(stdout.contains("agg_ncols=2"), "stdout:\n{stdout}");
    assert!(stdout.contains("has_qty_sum=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("has_price_mean=true"), "stdout:\n{stdout}");

    // Step 6: is_ordered bit.
    assert!(stdout.contains("ordered=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("plain=false"), "stdout:\n{stdout}");

    // Step 7: categorical sort follows category order (low first -> tag 200).
    assert!(stdout.contains("sorted0=200"), "stdout:\n{stdout}");

    // Step 8: loc_range_level_str on outer level -> 2 rows (b, c).
    assert!(stdout.contains("loc_rows=2"), "stdout:\n{stdout}");
}
