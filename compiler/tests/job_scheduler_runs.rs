//! Subprocess integration test for `examples/job_scheduler.spy` (M24-A).
//!
//! The scheduler combines `subprocess`, `threading.Lock + Semaphore`,
//! `queue.PriorityQueue`, and `datetime` to run 6 hardcoded shell jobs
//! across 3 worker threads.  Two of the jobs deliberately fail (exit
//! non-zero) and exhaust their retry budgets.  We assert that:
//!
//!   * The program compiles and exits with status 0.
//!   * Every job appears in the summary table.
//!   * `success=4` (alpha/bravo/charlie/delta succeed).
//!   * `failed=2` (fail-eager + fail-late both exhaust retries).
//!   * Each failing job shows the expected retry count.
//!
//! Additionally exercises the 9 stress probes from the M24-A brief via
//! sibling `examples/_probe_*.spy` files.  Each probe is a separate
//! `#[test]` that runs the canonical assertions on its stdout.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

fn spy_bin() -> PathBuf {
    if cfg!(windows) {
        project_root().join("target").join("release").join("spy.exe")
    } else {
        project_root().join("target").join("release").join("spy")
    }
}

fn compile_and_run(example_name: &str) -> Option<(std::process::ExitStatus, String, String)> {
    let src_path = project_root().join("examples").join(example_name);
    let src = fs::read_to_string(&src_path).unwrap_or_else(|e| panic!("read {example_name}: {e}"));
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile {example_name}: {e}"));
    let stem = example_name.trim_end_matches(".spy");
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{stem}.spyc"));
    fs::write(&spyc_path, &bytes).expect("write spyc");
    let bin = spy_bin();
    if !bin.exists() {
        eprintln!("skipping {example_name}: {} not present", bin.display());
        return None;
    }
    let output = Command::new(&bin)
        .arg(&spyc_path)
        .current_dir(project_root())
        .output()
        .expect("invoke spy.exe");
    Some((
        output.status,
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    ))
}

#[test]
fn job_scheduler_compiles() {
    let src_path = project_root().join("examples").join("job_scheduler.spy");
    let src = fs::read_to_string(&src_path).expect("read job_scheduler.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile job_scheduler.spy: {e}"));
}

#[test]
fn job_scheduler_runs_via_spy_exe() {
    let Some((status, stdout, stderr)) = compile_and_run("job_scheduler.spy") else {
        return;
    };
    assert!(
        status.success(),
        "spy.exe failed; status={status:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // Headline counts — the whole point of the program.
    assert!(
        stdout.contains("success=4"),
        "expected success=4; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("failed=2"),
        "expected failed=2; stdout:\n{stdout}"
    );

    // Every job name appears in the summary.
    for n in ["alpha", "bravo", "charlie", "delta", "fail-eager", "fail-late"] {
        assert!(
            stdout.contains(&format!("job {n} ")),
            "expected job {n} in summary; stdout:\n{stdout}"
        );
    }

    // Failing jobs hit their retry caps.
    // fail-eager: retry_max=2 -> 1 initial + 2 retries = retries=3 in the
    // counter (each FAIL bumps the counter, including the final one).
    assert!(
        stdout.contains("fail-eager") && stdout.contains("retries=3"),
        "fail-eager should have retries=3 (1 initial + 2 retries); stdout:\n{stdout}"
    );
    // fail-late: retry_max=1 -> retries=2.
    assert!(
        stdout.contains("fail-late") && stdout.contains("retries=2"),
        "fail-late should have retries=2 (1 initial + 1 retry); stdout:\n{stdout}"
    );

    assert!(stdout.contains("done"), "expected done marker; stdout:\n{stdout}");
}

// ── Probes ───────────────────────────────────────────────────────────

#[test]
fn probe_1_subprocess_echo() {
    let Some((status, stdout, stderr)) = compile_and_run("_probe_subprocess_echo.spy") else {
        return;
    };
    assert!(
        status.success(),
        "probe 1 failed; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("exit=0"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("stdout-has-hello=true"),
        "stdout:\n{stdout}"
    );
}

#[test]
fn probe_2_spawn_wait_in_worker_thread() {
    let Some((status, stdout, stderr)) = compile_and_run("_probe_spawn_wait_in_thread.spy") else {
        return;
    };
    assert!(
        status.success(),
        "probe 2 failed; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("count=4"), "stdout:\n{stdout}");
    assert!(stdout.contains("all-zero=true"), "stdout:\n{stdout}");
}

#[test]
fn probe_6_semaphore_blocking_signal() {
    let Some((status, stdout, stderr)) = compile_and_run("_probe_semaphore_signal.spy") else {
        return;
    };
    assert!(
        status.success(),
        "probe 6 failed; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("before-release=0"), "stdout:\n{stdout}");
    assert!(stdout.contains("after-release=3"), "stdout:\n{stdout}");
}

#[test]
fn probe_3_try_wait_returns_none_then_code() {
    let Some((status, stdout, stderr)) = compile_and_run("_probe_subprocess_try_wait.spy") else {
        return;
    };
    assert!(
        status.success(),
        "probe 3 failed; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("saw-none-at-least-once=true"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("saw-done=true"), "stdout:\n{stdout}");
}

#[test]
fn probe_4_kill_from_other_thread() {
    let Some((status, stdout, stderr)) = compile_and_run("_probe_subprocess_kill.spy") else {
        return;
    };
    assert!(
        status.success(),
        "probe 4 failed; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("spawned-handle-positive=true"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("wait-returned=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("elapsed-under-5s=true"), "stdout:\n{stdout}");
}

#[test]
fn probe_5_lock_contention_200_inserts() {
    // Run 3 times to check determinism.
    for run_idx in 0..3 {
        let Some((status, stdout, stderr)) = compile_and_run("_probe_lock_contention.spy") else {
            return;
        };
        assert!(
            status.success(),
            "probe 5 run {run_idx} failed; stdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(
            stdout.contains("len=200"),
            "run {run_idx} expected len=200; stdout:\n{stdout}"
        );
        assert!(
            stdout.contains("expected-200=true"),
            "run {run_idx} stdout:\n{stdout}"
        );
    }
}

#[test]
fn probe_7_pq_concurrent_push_pop() {
    let Some((status, stdout, stderr)) = compile_and_run("_probe_pq_concurrent.spy") else {
        return;
    };
    assert!(
        status.success(),
        "probe 7 failed; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // 20 seed items + 20 retries = exactly 40 total pops.
    assert!(stdout.contains("popped-count=40"), "stdout:\n{stdout}");
    assert!(stdout.contains("final-empty=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("saw-min-priority=true"), "stdout:\n{stdout}");
}

#[test]
fn probe_8_datetime_boundaries() {
    let Some((status, stdout, stderr)) = compile_and_run("_probe_datetime_boundaries.spy") else {
        return;
    };
    assert!(
        status.success(),
        "probe 8 failed; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("plus30s=2026-05-18T12:00:30Z"), "stdout:\n{stdout}");
    assert!(stdout.contains("plus60s=2026-05-18T12:01:00Z"), "stdout:\n{stdout}");
    assert!(stdout.contains("jan31+1=2026-02-01"), "stdout:\n{stdout}");
    assert!(stdout.contains("jan31+30=2026-03-02"), "stdout:\n{stdout}");
    assert!(stdout.contains("leap+1=2024-03-01"), "stdout:\n{stdout}");
    assert!(stdout.contains("leap+365=2025-02-28"), "stdout:\n{stdout}");
    assert!(stdout.contains("dec31+1=2027-01-01"), "stdout:\n{stdout}");
    assert!(stdout.contains("diff=60"), "stdout:\n{stdout}");
}

#[test]
fn probe_9_tuple_str_i32_i64_in_shared_list() {
    let Some((status, stdout, stderr)) = compile_and_run("_probe_tuple_shared_list.spy") else {
        return;
    };
    assert!(
        status.success(),
        "probe 9 failed; stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(stdout.contains("count=100"), "stdout:\n{stdout}");
    assert!(stdout.contains("alpha=25"), "stdout:\n{stdout}");
    assert!(stdout.contains("bravo=25"), "stdout:\n{stdout}");
    assert!(stdout.contains("charlie=25"), "stdout:\n{stdout}");
    assert!(stdout.contains("delta=25"), "stdout:\n{stdout}");
    assert!(stdout.contains("bad=0"), "stdout:\n{stdout}");
}
