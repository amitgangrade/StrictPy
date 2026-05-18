//! Subprocess integration test for `examples/file_stats.spy` (M20a).
//!
//! Feeds the program a known-good path (the workspace `Cargo.toml`)
//! via `sys.argv[1]` and asserts each line of the report matches.

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
fn file_stats_compiles() {
    let src_path = project_root().join("examples").join("file_stats.spy");
    let src = fs::read_to_string(&src_path).expect("read file_stats.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile file_stats.spy: {e}"));
}

#[test]
fn file_stats_reports_known_path_via_spy_exe() {
    let src_path = project_root().join("examples").join("file_stats.spy");
    let src = fs::read_to_string(&src_path).expect("read file_stats.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile file_stats.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("file_stats.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    // Use a relative path to keep the assertion text portable.
    let target = "Cargo.toml";

    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg(target)
        .current_dir(project_root())
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(output.status.success(),
        "spy.exe failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status);

    assert!(stdout.contains("path     : Cargo.toml"),
        "missing path line; stdout:\n{stdout}");
    assert!(stdout.contains("exists   : true"),
        "Cargo.toml should exist; stdout:\n{stdout}");
    assert!(stdout.contains("is_file  : true"),
        "Cargo.toml should be a file; stdout:\n{stdout}");
    assert!(stdout.contains("is_dir   : false"),
        "Cargo.toml should not be a dir; stdout:\n{stdout}");
    assert!(stdout.contains("basename : Cargo.toml"),
        "basename mismatch; stdout:\n{stdout}");
    assert!(stdout.contains("splitext : (Cargo, .toml)"),
        "splitext output wrong; stdout:\n{stdout}");
}

#[test]
fn file_stats_handles_no_args_gracefully() {
    let src_path = project_root().join("examples").join("file_stats.spy");
    let src = fs::read_to_string(&src_path).expect("read file_stats.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile file_stats.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("file_stats_noargs.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(stdout.contains("usage:"),
        "expected usage banner without args; stdout:\n{stdout}");
}
