//! Subprocess integration test for `examples/glob_demo.spy` (M27 P3c-B).
//!
//! Compiles + runs the glob-module demo through `spy.exe` and asserts
//! the printed values match expectation.  The asserts inside the demo
//! itself are the authoritative correctness check; this test merely
//! confirms the program prints its OK banner and the expected sentinel
//! values land in stdout.

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
fn glob_demo_compiles() {
    let src_path = project_root().join("examples").join("glob_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read glob_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile glob_demo.spy: {e}"));
}

#[test]
fn glob_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("glob_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read glob_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile glob_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("glob_demo.spyc");
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

    // Every internal assert passed.
    assert!(
        stdout.contains("OK"),
        "expected OK banner; stdout:\n{stdout}"
    );
    // Pretty-printed sentinels.
    assert!(
        stdout.contains("spy_count=2"),
        "expected spy_count=2 (a.spy + b.spy); stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("rec_count=3"),
        "expected rec_count=3 (a.spy, b.spy, sub/x.spy); stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("txt_count=1"),
        "expected txt_count=1 (c.txt); stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("escape_grew=true"),
        "expected escape_grew sentinel; stdout:\n{stdout}"
    );
}
