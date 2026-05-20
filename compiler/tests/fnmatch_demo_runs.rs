//! Subprocess integration test for `examples/fnmatch_demo.spy` (M27 P3c-B).
//!
//! Compiles + runs the fnmatch-module demo through `spy.exe` and asserts
//! the OK banner lands on stdout.  Internal asserts inside the demo are
//! the authoritative correctness check.

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
fn fnmatch_demo_compiles() {
    let src_path = project_root().join("examples").join("fnmatch_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read fnmatch_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile fnmatch_demo.spy: {e}"));
}

#[test]
fn fnmatch_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("fnmatch_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read fnmatch_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile fnmatch_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fnmatch_demo.spyc");
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
        stdout.contains("OK"),
        "expected OK banner; stdout:\n{stdout}"
    );
}
