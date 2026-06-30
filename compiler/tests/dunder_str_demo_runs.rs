//! Wave-2 Lane A integration test for `examples/dunder_str_demo.spy`.
//!
//! Compiles + runs the demo through `spy.exe` and asserts that `str(obj)` /
//! `print(obj)` on a user class dispatch `__str__` (preferred), then
//! `__repr__` (fallback), then a synthesised default field repr — closing
//! the "StrFromAny garbage for class" deferral in BUGS_KNOWN.md.

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
fn dunder_str_demo_compiles() {
    let src_path = project_root().join("examples").join("dunder_str_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read dunder_str_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile dunder_str_demo.spy: {e}"));
}

#[test]
fn dunder_str_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("dunder_str_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read dunder_str_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile dunder_str_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("dunder_str_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping run leg: {} not present", spy_bin.display());
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

    // 1. __str__ (preferred) via print(obj) and str(obj). `print("p = ")`
    // has no trailing newline, so `println(p)` continues the same line.
    assert!(stdout.contains("p = Point(3, 4)\n"), "stdout:\n{stdout}");
    assert!(stdout.contains("str(p) = Point(3, 4)"), "stdout:\n{stdout}");
    // 2. __repr__ fallback.
    assert!(stdout.contains("Money<1099c>"), "stdout:\n{stdout}");
    assert!(stdout.contains("str(m) = Money<1099c>"), "stdout:\n{stdout}");
    // 3. default field repr.
    assert!(stdout.contains("Color(r=255, g=128, b=0)"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("str(c) = Color(r=255, g=128, b=0)"),
        "stdout:\n{stdout}"
    );
}
