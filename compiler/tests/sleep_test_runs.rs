//! Subprocess integration test for `examples/sleep_test.spy` (M20b).
//!
//! Compiles + runs the sleep-and-measure example.  The assertion is the
//! lenient floor printed by the program itself (>= 80ms after a 100ms
//! sleep) — we just verify the program reaches the "slept-yes" branch.

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
fn sleep_test_compiles() {
    let src_path = project_root().join("examples").join("sleep_test.spy");
    let src = fs::read_to_string(&src_path).expect("read sleep_test.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile sleep_test.spy: {e}"));
}

#[test]
fn sleep_test_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("sleep_test.spy");
    let src = fs::read_to_string(&src_path).expect("read sleep_test.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile sleep_test.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sleep_test.spyc");
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
        stdout.contains("slept-yes"),
        "expected slept-yes branch; stdout:\n{stdout}"
    );
}
