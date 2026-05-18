//! Subprocess integration test for `examples/hashlib_demo.spy` (M22 P2B).
//!
//! Compiles + runs the hashlib known-vector demo through `spy.exe` and
//! asserts every algorithm reports `ok` for its NIST/RFC test vector.

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
fn hashlib_demo_compiles() {
    let src_path = project_root().join("examples").join("hashlib_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read hashlib_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile hashlib_demo.spy: {e}"));
}

#[test]
fn hashlib_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("hashlib_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read hashlib_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile hashlib_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hashlib_demo.spyc");
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

    // Every check should print "ok".  Any "FAIL" line is a regression.
    assert!(!stdout.contains("FAIL"), "unexpected FAIL line; stdout:\n{stdout}");

    assert!(stdout.contains("md5(\"\"): ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("md5(pangram): ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("sha1(\"\"): ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("sha1(\"abc\"): ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("sha256(\"\"): ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("sha256(\"abc\"): ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("sha512(\"abc\"): ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("hmac_sha256(key, pangram): ok"), "stdout:\n{stdout}");

    // Digest-length sanity checks.
    assert!(stdout.contains("md5 len: 32"), "stdout:\n{stdout}");
    assert!(stdout.contains("sha1 len: 40"), "stdout:\n{stdout}");
    assert!(stdout.contains("sha256 len: 64"), "stdout:\n{stdout}");
    assert!(stdout.contains("sha512 len: 128"), "stdout:\n{stdout}");

    assert!(stdout.contains("done"), "stdout:\n{stdout}");
}
