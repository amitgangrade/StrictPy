//! Subprocess integration test for `examples/bz2_demo.spy` (M27 P3c-C).
//!
//! Compiles + runs the bz2 round-trip demo through `spy.exe` and
//! asserts every check line reports `ok`.

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
fn bz2_demo_compiles() {
    let src_path = project_root().join("examples").join("bz2_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read bz2_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile bz2_demo.spy: {e}"));
}

#[test]
fn bz2_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("bz2_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read bz2_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile bz2_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bz2_demo.spyc");
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

    assert!(
        !stdout.contains("FAIL"),
        "unexpected FAIL line; stdout:\n{stdout}"
    );

    assert!(stdout.contains("default round-trip: ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("repeat round-trip: ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("repeat compresses smaller: ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("level 1 round-trip: ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("level 9 round-trip: ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("empty round-trip: ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("caught bad bz2"), "stdout:\n{stdout}");
    assert!(stdout.contains("caught bad level"), "stdout:\n{stdout}");
    assert!(stdout.contains("done"), "stdout:\n{stdout}");
}
