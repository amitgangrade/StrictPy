//! Subprocess integration test for `examples/struct_demo.spy` (M22 P2D).
//!
//! Compiles + runs the struct demo through `spy.exe` and asserts the
//! printed output covers pack/unpack round-trips for u32 / u64 / f64
//! in both endiannesses + the out-of-bounds ValueError handler.

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
fn struct_demo_compiles() {
    let src_path = project_root().join("examples").join("struct_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read struct_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile struct_demo.spy: {e}"));
}

#[test]
fn struct_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("struct_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read struct_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile struct_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("struct_demo.spyc");
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

    // u32 BE len + round-trip.
    assert!(stdout.contains("u32_be len: 4"), "stdout:\n{stdout}");
    assert!(stdout.contains("u32_be round-trip: 16909060"), "stdout:\n{stdout}");
    // u32 LE round-trip — same input value, different byte layout, same i64.
    assert!(stdout.contains("u32_le round-trip: 16909060"), "stdout:\n{stdout}");
    assert!(stdout.contains("BE/LE differ: ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("u32 max round-trip: 4294967295"), "stdout:\n{stdout}");
    // u64 round-trip on i64::MAX.
    assert!(stdout.contains("u64_be len: 8"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("u64_be round-trip: 9223372036854775807"),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("u64_le round-trip: 9223372036854775807"),
        "stdout:\n{stdout}"
    );
    // f64 IEEE 754 round-trip.
    assert!(stdout.contains("f64_be len: 8"), "stdout:\n{stdout}");
    assert!(stdout.contains("f64_be round-trip pi: ok"), "stdout:\n{stdout}");
    assert!(stdout.contains("f64_le round-trip pi: ok"), "stdout:\n{stdout}");
    // Offset unpack.
    assert!(stdout.contains("cat len: 8"), "stdout:\n{stdout}");
    assert!(stdout.contains("second u32: 287454020"), "stdout:\n{stdout}");
    // OOB raises ValueError.
    assert!(stdout.contains("caught oob"), "stdout:\n{stdout}");
    assert!(stdout.contains("done"), "stdout:\n{stdout}");
}
