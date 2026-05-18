//! Subprocess integration test for `examples/json_demo.spy` (M20c).
//!
//! Compiles + runs the JSON validate/reserialize demo through `spy.exe`
//! and asserts the printed output matches the expected canonicalised
//! form and the try/except ValueError handler fires on bad input.

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
fn json_demo_compiles() {
    let src_path = project_root().join("examples").join("json_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read json_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile json_demo.spy: {e}"));
}

#[test]
fn json_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("json_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read json_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile json_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("json_demo.spyc");
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

    // The canonical form sorts keys alphabetically.
    assert!(
        stdout.contains("canonical: {\"active\":true,\"age\":30,\"name\":\"alice\",\"tags\":[\"admin\",\"user\"]}"),
        "expected canonical line; stdout:\n{stdout}"
    );
    assert!(stdout.contains("is_valid: true"), "is_valid; stdout:\n{stdout}");
    assert!(stdout.contains("minify == parse_to_string: true"), "stdout:\n{stdout}");
    assert!(stdout.contains("caught ValueError"), "stdout:\n{stdout}");
    assert!(stdout.contains("is_valid(bad): false"), "stdout:\n{stdout}");
    assert!(stdout.contains("nested round-trip: true"), "stdout:\n{stdout}");
    assert!(stdout.contains("done"), "stdout:\n{stdout}");
}
