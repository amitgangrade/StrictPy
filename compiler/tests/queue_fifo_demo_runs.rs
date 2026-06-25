//! Subprocess integration test for `examples/queue_fifo_demo.spy`
//! (LANE E: plain FIFO queue.Queue).

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
fn queue_fifo_demo_compiles() {
    let src_path = project_root().join("examples").join("queue_fifo_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read queue_fifo_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile queue_fifo_demo.spy: {e}"));
}

#[test]
fn queue_fifo_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("queue_fifo_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read queue_fifo_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile queue_fifo_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("queue_fifo_demo.spyc");
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

    assert!(stdout.contains("empty0=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("qsize=5"), "stdout:\n{stdout}");
    assert!(stdout.contains("empty1=false"), "stdout:\n{stdout}");
    assert!(stdout.contains("fifo-ordered=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("drained=5"), "stdout:\n{stdout}");

    // string FIFO drains in insertion order
    let first = stdout.find("task: first").expect("first line");
    let second = stdout.find("task: second").expect("second line");
    let third = stdout.find("task: third").expect("third line");
    assert!(
        first < second && second < third,
        "expected FIFO order first<second<third; stdout:\n{stdout}"
    );
}
