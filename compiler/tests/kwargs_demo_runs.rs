//! M61b acceptance test: `examples/kwargs_demo.spy` — compiles and runs the
//! demo that exercises default argument values and keyword arguments on free
//! functions and a constructor.
//!
//! Asserts:
//!   - the program compiles,
//!   - it exits 0,
//!   - the deterministic output matches exactly.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

fn compile_and_write() -> PathBuf {
    let src_path = project_root().join("examples").join("kwargs_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read kwargs_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile kwargs_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("kwargs_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");
    spyc_path
}

#[test]
fn kwargs_demo_compiles() {
    let _ = compile_and_write();
}

#[test]
fn kwargs_demo_output() {
    let spyc_path = compile_and_write();
    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping run: {} not present", spy_bin.display());
        return;
    }
    let output = std::process::Command::new(&spy_bin)
        .arg(&spyc_path)
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "expected success; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );
    let expected = "\
hi, alice!\n\
hey, bob!\n\
yo, cleo!\n\
grid 3x4\n\
row 5x1\n\
box 2x1\n\
(7, 0)\n\
(1, 9)\n";
    assert_eq!(stdout, expected, "stdout mismatch:\n{stdout}");
}
