//! Subprocess integration test for `examples/threading_demo.spy` (M23 P3a-C).
//!
//! Compiles + runs the lock-protected-counter demo through `spy.exe` and
//! asserts the final counter equals 400 (4 workers × 100 increments).
//! Without `threading.Lock` correctness, the counter would land below
//! 400 essentially every run on a multi-core host.

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
fn threading_demo_compiles() {
    let src_path = project_root().join("examples").join("threading_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read threading_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile threading_demo.spy: {e}"));
}

#[test]
fn threading_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("threading_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read threading_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile threading_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("threading_demo.spyc");
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

    // Final counter must equal 400 — proves the lock is real exclusion.
    assert!(
        stdout.contains("counter=400"),
        "expected counter=400 (lock correctness); stdout:\n{stdout}"
    );
    assert!(stdout.contains("semaphore-ok=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("try-acquire-fresh=true"), "stdout:\n{stdout}");
}
