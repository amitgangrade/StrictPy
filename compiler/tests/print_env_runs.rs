//! Integration test for `examples/print_env.spy` (M19).
//!
//! Banner-style program; we assert that the three known-stable lines
//! ("version : ...", "argc    : ...") appear and that the platform line
//! is one of the four legal values.

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
fn print_env_compiles() {
    let src_path = project_root().join("examples").join("print_env.spy");
    let src = fs::read_to_string(&src_path).expect("read print_env.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile print_env.spy: {e}"));
}

#[test]
fn print_env_prints_a_legal_banner_via_spy_exe() {
    let src_path = project_root().join("examples").join("print_env.spy");
    let src = fs::read_to_string(&src_path).expect("read print_env.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile print_env.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("print_env.spyc");
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
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(output.status.success(),
        "spy.exe failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status);

    assert!(stdout.contains("== StrictPy banner =="),
        "missing banner; stdout:\n{stdout}");
    assert!(stdout.contains("version : StrictPy v0.2"),
        "missing/wrong version line; stdout:\n{stdout}");
    // argv = [program-path] when no trailing args → argc = 1.
    assert!(stdout.contains("argc    : 1"),
        "expected argc=1; stdout:\n{stdout}");
    // Find the platform line and validate.
    let plat_line = stdout.lines()
        .find(|l| l.starts_with("platform: "))
        .unwrap_or_else(|| panic!("no platform line; stdout:\n{stdout}"));
    let plat = plat_line.trim_start_matches("platform: ").trim();
    assert!(
        matches!(plat, "windows" | "linux" | "macos" | "unknown"),
        "illegal platform value {plat:?}; stdout:\n{stdout}"
    );
}
