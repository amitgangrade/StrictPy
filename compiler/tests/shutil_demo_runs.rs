//! Subprocess integration test for `examples/shutil_demo.spy` (M27 P3c-A).
//!
//! Compiles + runs the shutil demo through `spy.exe` and asserts the
//! printed output covers copy/copytree/move/rmtree/which/disk_usage and
//! the error-path branches (copytree existing dst, rmtree missing path).

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
fn shutil_demo_compiles() {
    let src_path = project_root().join("examples").join("shutil_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read shutil_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile shutil_demo.spy: {e}"));
}

#[test]
fn shutil_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("shutil_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read shutil_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile shutil_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shutil_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = if cfg!(windows) {
        project_root().join("target").join("release").join("spy.exe")
    } else {
        project_root().join("target").join("release").join("spy")
    };
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

    assert!(stdout.contains("created tempdir"), "stdout:\n{stdout}");
    assert!(stdout.contains("copy: ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("copytree: ok"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("caught IOError on copytree existing dst"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("move: ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("which: ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("which.miss: ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("disk_usage: ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("rmtree: ok"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("caught IOError on rmtree missing"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("done"), "stdout:\n{stdout}");
}
