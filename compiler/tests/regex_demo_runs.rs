//! Subprocess integration test for `examples/regex_demo.spy` (M20c).
//!
//! Compiles + runs the regex demo through `spy.exe` and asserts the
//! printed output covers match/search/find/find_all/replace/split.

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
fn regex_demo_compiles() {
    let src_path = project_root().join("examples").join("regex_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read regex_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile regex_demo.spy: {e}"));
}

#[test]
fn regex_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("regex_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read regex_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile regex_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("regex_demo.spyc");
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

    assert!(stdout.contains("match alphanumeric: true"), "stdout:\n{stdout}");
    assert!(stdout.contains("match partial: false"), "stdout:\n{stdout}");
    assert!(stdout.contains("search digit: true"), "stdout:\n{stdout}");
    assert!(stdout.contains("find pos: (3, 6)"), "stdout:\n{stdout}");
    assert!(stdout.contains("find miss: (-1, -1)"), "stdout:\n{stdout}");
    assert!(stdout.contains("find_all count: 3"), "stdout:\n{stdout}");
    assert!(stdout.contains("replace: aXbXcX"), "stdout:\n{stdout}");
    assert!(stdout.contains("split count: 4"), "stdout:\n{stdout}");
    assert!(stdout.contains("is_valid good: true"), "stdout:\n{stdout}");
    assert!(stdout.contains("is_valid bad: false"), "stdout:\n{stdout}");
    assert!(stdout.contains("caught bad pattern"), "stdout:\n{stdout}");
    assert!(stdout.contains("done"), "stdout:\n{stdout}");
}
