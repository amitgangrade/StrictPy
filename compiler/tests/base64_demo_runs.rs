//! Subprocess integration test for `examples/base64_demo.spy` (M22 P2B).
//!
//! Compiles + runs the base64 round-trip demo through `spy.exe` and
//! asserts every labelled line appears in the output, including the
//! caught ValueError for malformed input.

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
fn base64_demo_compiles() {
    let src_path = project_root().join("examples").join("base64_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read base64_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile base64_demo.spy: {e}"));
}

#[test]
fn base64_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("base64_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read base64_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile base64_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("base64_demo.spyc");
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
        stdout.contains("encoded: SGVsbG8sIFN0cmljdFB5IQ=="),
        "expected std encoding; stdout:\n{stdout}"
    );
    assert!(stdout.contains("decoded: Hello, StrictPy!"), "stdout:\n{stdout}");
    assert!(stdout.contains("round-trip: true"), "stdout:\n{stdout}");
    assert!(stdout.contains("empty encode: ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("empty decode: ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("utf8 round-trip: true"), "stdout:\n{stdout}");
    assert!(stdout.contains("url-safe round-trip: true"), "stdout:\n{stdout}");
    assert!(stdout.contains("known vector Man: ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("known vector decode: ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("caught bad base64"), "stdout:\n{stdout}");
    assert!(stdout.contains("caught bad url-safe"), "stdout:\n{stdout}");
    assert!(stdout.contains("done"), "stdout:\n{stdout}");
}
