//! Subprocess integration test for `examples/bytearray_demo.spy`
//! (LANE E: mutable byte buffer).

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
fn bytearray_demo_compiles() {
    let src_path = project_root().join("examples").join("bytearray_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read bytearray_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile bytearray_demo.spy: {e}"));
}

#[test]
fn bytearray_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("bytearray_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read bytearray_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile bytearray_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bytearray_demo.spyc");
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

    assert!(stdout.contains("len0=0"), "stdout:\n{stdout}");
    assert!(stdout.contains("len3=3"), "stdout:\n{stdout}");
    assert!(stdout.contains("get0=65"), "stdout:\n{stdout}");
    assert!(stdout.contains("get_neg=67"), "stdout:\n{stdout}");
    assert!(stdout.contains("text=ABC"), "stdout:\n{stdout}");
    assert!(stdout.contains("hex=414243"), "stdout:\n{stdout}");
    assert!(stdout.contains("after_set=AZC"), "stdout:\n{stdout}");
    assert!(stdout.contains("popped=67"), "stdout:\n{stdout}");
    assert!(stdout.contains("after_pop=AZ"), "stdout:\n{stdout}");
    assert!(stdout.contains("from_str_len=2"), "stdout:\n{stdout}");
    assert!(stdout.contains("from_str_hex=6869"), "stdout:\n{stdout}");
    assert!(stdout.contains("after_clear=0"), "stdout:\n{stdout}");
}
