//! Subprocess integration test for `examples/unittest_demo.spy`
//! (LANE E: unittest-style assert helpers + runner).

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
fn unittest_demo_compiles() {
    let src_path = project_root().join("examples").join("unittest_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read unittest_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile unittest_demo.spy: {e}"));
}

#[test]
fn unittest_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("unittest_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read unittest_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile unittest_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("unittest_demo.spyc");
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

    assert!(stdout.contains("ran=5"), "stdout:\n{stdout}");
    assert!(stdout.contains("ran 5 checks: OK"), "stdout:\n{stdout}");
    assert!(stdout.contains("failures=0"), "stdout:\n{stdout}");
    assert!(stdout.contains("failure_count=0"), "stdout:\n{stdout}");
}
