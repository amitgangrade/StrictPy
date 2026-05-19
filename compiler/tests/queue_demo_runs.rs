//! Subprocess integration test for `examples/queue_demo.spy` (M23 P3a-C).
//!
//! Compiles + runs the priority-queue demo through `spy.exe` and asserts
//! that items drained in ascending-priority order.

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
fn queue_demo_compiles() {
    let src_path = project_root().join("examples").join("queue_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read queue_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile queue_demo.spy: {e}"));
}

#[test]
fn queue_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("queue_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read queue_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile queue_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("queue_demo.spyc");
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

    assert!(stdout.contains("pq_len=10"), "stdout:\n{stdout}");
    assert!(stdout.contains("pq_empty=false"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("peek-item=0"),
        "expected peek of min item; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("after-peek-len=10"),
        "peek must not pop; stdout:\n{stdout}"
    );
    assert!(stdout.contains("drained-count=10"), "stdout:\n{stdout}");
    assert!(stdout.contains("numeric-pq-sorted=true"), "stdout:\n{stdout}");

    // Named-task scheduler output, in priority order.
    let backup_idx = stdout.find("run: backup").expect("backup line");
    let lint_idx = stdout.find("run: lint").expect("lint line");
    let deploy_idx = stdout.find("run: deploy").expect("deploy line");
    let test_idx = stdout.find("run: test").expect("test line");
    let cleanup_idx = stdout.find("run: cleanup").expect("cleanup line");
    assert!(
        backup_idx < lint_idx
            && lint_idx < deploy_idx
            && deploy_idx < test_idx
            && test_idx < cleanup_idx,
        "expected ordered drain backup<lint<deploy<test<cleanup; stdout:\n{stdout}"
    );
}
