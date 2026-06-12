//! Subprocess integration test for `examples/struct_builder_demo.spy` —
//! the `struct` module's batch writer/reader surface.
//!
//! Compiles + runs the demo through `spy.exe` and asserts the printed
//! output covers: byte-identity with the old pack_* concatenation, full
//! write→finish→reader→sequential-read roundtrips (BE and LE),
//! interleaved-handle independence, the overrun ValueError, and the
//! stale-handle ValueError.  Modeled on struct_demo_runs.rs.

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
fn struct_builder_demo_compiles() {
    let src_path = project_root()
        .join("examples")
        .join("struct_builder_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read struct_builder_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile struct_builder_demo.spy: {e}"));
}

#[test]
fn struct_builder_demo_runs_via_spy_exe() {
    let src_path = project_root()
        .join("examples")
        .join("struct_builder_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read struct_builder_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile struct_builder_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("struct_builder_demo.spyc");
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

    // 1. Record assembly: 4 + 8 + 8 bytes.
    assert!(stdout.contains("record len: 20"), "stdout:\n{stdout}");
    // 2. Byte-identity with the per-field pack_* concatenation.
    assert!(
        stdout.contains("matches pack_* concat: ok"),
        "stdout:\n{stdout}"
    );
    // 3. Sequential reads recover every field.
    assert!(stdout.contains("id: 16909060"), "stdout:\n{stdout}");
    assert!(stdout.contains("score round-trip: ok"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("stamp: 9223372036854775807"),
        "stdout:\n{stdout}"
    );
    // 4. Little-endian mirrors.
    assert!(stdout.contains("le u32: 16909060"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("le u64: 9223372036854775807"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("le f64: ok"), "stdout:\n{stdout}");
    // 5. Interleaved handles stay independent.
    assert!(stdout.contains("a: 1,3"), "stdout:\n{stdout}");
    assert!(stdout.contains("b: 2,4"), "stdout:\n{stdout}");
    // 6. Overrun raises ValueError.
    assert!(stdout.contains("caught overrun"), "stdout:\n{stdout}");
    // 7. Finished handle reuse raises ValueError.
    assert!(stdout.contains("caught stale writer"), "stdout:\n{stdout}");
    assert!(stdout.contains("done"), "stdout:\n{stdout}");
    assert!(!stdout.contains("UNREACHED"), "stdout:\n{stdout}");
    assert!(!stdout.contains("FAIL"), "stdout:\n{stdout}");
}
