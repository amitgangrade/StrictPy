//! Subprocess integration test for `examples/pathlib_demo.spy` (M23 P3a-A).
//!
//! Compiles + runs the pathlib demo through `spy.exe` and asserts the
//! printed output covers join/with_suffix/with_name/stem/suffix/parts/
//! is_absolute/absolute/read_text/write_text/read_lines/relative_to.

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
fn pathlib_demo_compiles() {
    let src_path = project_root().join("examples").join("pathlib_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read pathlib_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile pathlib_demo.spy: {e}"));
}

#[test]
fn pathlib_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("pathlib_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read pathlib_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile pathlib_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("pathlib_demo.spyc");
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

    // join.len: "a" + sep + "b" = 3 chars.
    assert!(stdout.contains("join.len: 3"), "stdout:\n{stdout}");
    // with_suffix.
    assert!(stdout.contains("with_suffix: report.csv"), "stdout:\n{stdout}");
    assert!(stdout.contains("add_suffix: README.md"), "stdout:\n{stdout}");
    // with_name.
    assert!(stdout.contains("with_name.suffix: .csv"), "stdout:\n{stdout}");
    assert!(stdout.contains("with_name.name: x.csv"), "stdout:\n{stdout}");
    // stem (python: archive.tar.gz → archive.tar).
    assert!(stdout.contains("stem: archive.tar"), "stdout:\n{stdout}");
    assert!(stdout.contains("suffix: .gz"), "stdout:\n{stdout}");
    // parts.
    assert!(stdout.contains("parts.len: 3"), "stdout:\n{stdout}");
    assert!(stdout.contains("parts.0: a"), "stdout:\n{stdout}");
    assert!(stdout.contains("parts.2: c"), "stdout:\n{stdout}");
    // is_absolute on relative path.
    assert!(stdout.contains("is_abs.rel: false"), "stdout:\n{stdout}");
    // absolute(p) yields an absolute path.
    assert!(stdout.contains("absolute.is_abs: true"), "stdout:\n{stdout}");
    // read_text round-trip.
    assert!(stdout.contains("read_text: ok"), "stdout:\n{stdout}");
    // read_lines strips trailing newline → 3 lines.
    assert!(stdout.contains("read_lines.count: 3"), "stdout:\n{stdout}");
    assert!(stdout.contains("read_lines.0: line1"), "stdout:\n{stdout}");
    assert!(stdout.contains("read_lines.2: line3"), "stdout:\n{stdout}");
    // relative_to happy path.
    assert!(stdout.contains("relative_to: c"), "stdout:\n{stdout}");
    // relative_to not-a-subpath raises ValueError.
    assert!(stdout.contains("caught ValueError on relative_to"), "stdout:\n{stdout}");
    assert!(stdout.contains("done"), "stdout:\n{stdout}");
}
