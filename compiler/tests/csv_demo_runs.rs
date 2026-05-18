//! Subprocess integration test for `examples/csv_demo.spy` (M22 P2A).
//!
//! Compiles + runs the csv demo via `spy.exe`, passing a tempfile path
//! for the round-trip output.  Asserts on the printed row count and
//! cell-content checks for the quoted-comma and double-quote cases.

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
fn csv_demo_compiles() {
    let src_path = project_root().join("examples").join("csv_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read csv_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile csv_demo.spy: {e}"));
}

#[test]
fn csv_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("csv_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read csv_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile csv_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("csv_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    // Use a per-run tempfile path so parallel test runs don't collide.
    let csv_out_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("csv_demo_run.csv");
    let _ = fs::remove_file(&csv_out_path);

    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg(&csv_out_path)
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

    // Round-trip row count.
    assert!(stdout.contains("rows: 4"), "got:\n{stdout}");
    // Spot-check the `hello, world` cell (quoted-comma escape survived).
    assert!(stdout.contains("r1c2: hello, world"), "got:\n{stdout}");
    // Per-line parser checks.
    assert!(stdout.contains("pieces: 3"), "got:\n{stdout}");
    assert!(stdout.contains("p0: foo"), "got:\n{stdout}");
    assert!(stdout.contains("p1: bar,baz"), "got:\n{stdout}");
    assert!(stdout.contains("p2: q\"x"), "got:\n{stdout}");
    // `format_row` output (quoting a,b but leaving the others plain,
    // and double-escaping the embedded quote).
    assert!(
        stdout.contains("formatted: \"a,b\",plain,\"quote\"\"here\""),
        "got:\n{stdout}"
    );

    // The on-disk file should also round-trip — re-parse it from Rust to
    // double-check the bytes that hit the disk.
    let disk = fs::read_to_string(&csv_out_path).expect("read csv_out");
    assert!(disk.contains("name,city,note\n"), "disk: {disk:?}");
    assert!(disk.contains("Alice,Berlin,\"hello, world\""), "disk: {disk:?}");
    // The `quote "inside"` field should be quoted and have its inner
    // quotes doubled.
    assert!(disk.contains("\"quote \"\"inside\"\"\""), "disk: {disk:?}");
}
