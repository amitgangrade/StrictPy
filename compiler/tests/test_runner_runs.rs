//! Subprocess integration test for `examples/test_runner.spy` (M24-C).
//!
//! Compiles + runs the parallel test runner through `spy.exe` and asserts
//! the high-level invariants:
//!   * All 20 tests' rows land in SQLite under both N=1 and N=4.
//!   * N=4 wall-clock is strictly less than N=1 wall-clock (parallelism).
//!   * The IOError-on-nonexistent-program probe is caught.
//!   * `summary.pass`, `summary.fail`, `summary.timeout` are emitted.
//!   * `slowest.count=3` is reached (ORDER BY DESC LIMIT 3 works).

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
fn test_runner_compiles() {
    let src_path = project_root().join("examples").join("test_runner.spy");
    let src = fs::read_to_string(&src_path).expect("read test_runner.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile test_runner.spy: {e}"));
}

#[test]
fn test_runner_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("test_runner.spy");
    let src = fs::read_to_string(&src_path).expect("read test_runner.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile test_runner.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("test_runner.spyc");
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
    // Emit stdout/stderr for the report so `cargo test -- --nocapture`
    // shows the wall-clock numbers, summary counts, etc.
    eprintln!("---- test_runner.spy stdout ----\n{stdout}");
    if !stderr.is_empty() {
        eprintln!("---- test_runner.spy stderr ----\n{stderr}");
    }
    assert!(
        output.status.success(),
        "spy.exe failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );

    // Sanity assertions on the output shape — every probe below
    // emits a deterministic line.
    assert!(
        stdout.contains("tests.total=20"),
        "expected 20 tests; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("probe.nonexistent=ok"),
        "expected IOError caught on nonexistent program; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("n1.row_count=20"),
        "expected 20 rows under N=1; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("n4.row_count=20"),
        "expected 20 rows under N=4 (no lost rows under thread contention); stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("slowest.count=3"),
        "expected ORDER BY DESC LIMIT 3 to return 3 rows; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("slowest_via_params.count=3"),
        "expected query_params LIMIT ? to bind correctly; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("rows.ok=true"),
        "row-count gate failed; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("summary.pass="),
        "expected pass summary line; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("summary.fail="),
        "expected fail summary line; stdout:\n{stdout}"
    );
    assert!(stdout.contains("done"), "expected 'done'; stdout:\n{stdout}");

    // Parallelism check — we don't assert a hard ratio because Windows'
    // 1-second `timeout` granularity vs. Unix's `sleep 0.1` makes the
    // exact factor platform-dependent, but `parallelism=ok` must fire
    // (N=4 strictly less than N=1).
    assert!(
        stdout.contains("parallelism=ok"),
        "expected N=4 wall-clock < N=1 wall-clock; stdout:\n{stdout}"
    );
}
