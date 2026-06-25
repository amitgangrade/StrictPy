//! Subprocess integration test for `examples/enum_demo.spy`
//! (LANE E: enum named-constant registry).

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
fn enum_demo_compiles() {
    let src_path = project_root().join("examples").join("enum_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read enum_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile enum_demo.spy: {e}"));
}

#[test]
fn enum_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("enum_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read enum_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile enum_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("enum_demo.spyc");
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

    assert!(stdout.contains("len=3"), "stdout:\n{stdout}");
    assert!(stdout.contains("GREEN=1"), "stdout:\n{stdout}");
    assert!(stdout.contains("name_of_2=BLUE"), "stdout:\n{stdout}");
    assert!(stdout.contains("has_RED=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("has_PINK=false"), "stdout:\n{stdout}");
    assert!(stdout.contains("OK=200"), "stdout:\n{stdout}");
    assert!(stdout.contains("name_of_404=NOT_FOUND"), "stdout:\n{stdout}");
    assert!(stdout.contains("status_len=2"), "stdout:\n{stdout}");
}
