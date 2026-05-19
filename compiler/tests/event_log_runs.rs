//! Subprocess integration test for `examples/event_log.spy` (M24-B).
//!
//! Compiles + runs the event-log CLI tool through `spy.exe` and verifies:
//!
//!   1. **`--mode=demo`** (default) — runs the 14-probe self-test and
//!      every probe reports PASS.
//!   2. **`--mode=ingest`** then **`--mode=query`** + **`--mode=summary`**
//!      against a known input file — verifies the canonical
//!      "histogram bucket counts" output and the timestamp round-trip.
//!   3. **Missing-file ingest** — exit code 2 + error message on stderr.
//!   4. **Bad `--since`** — exit code 2.
//!
//! The test file is procedurally generated under `CARGO_TARGET_TMPDIR`
//! so cargo cleans it up.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

fn compile_program(slug: &str) -> PathBuf {
    let src_path = project_root().join("examples").join("event_log.spy");
    let src = fs::read_to_string(&src_path).expect("read event_log.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile event_log.spy: {e}"));
    // Per-test slug suffix so concurrent tests don't race over the same
    // .spyc file under CARGO_TARGET_TMPDIR.
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("event_log_{slug}.spyc"));
    fs::write(&spyc_path, &bytes).expect("write spyc");
    spyc_path
}

fn spy_exe() -> Option<PathBuf> {
    let p = project_root().join("target").join("release").join("spy.exe");
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

#[test]
fn event_log_compiles() {
    let _ = compile_program("compile_check");
}

#[test]
fn event_log_demo_all_probes_pass() {
    let spyc_path = compile_program("demo");
    let Some(spy_bin) = spy_exe() else {
        eprintln!("skipping: spy.exe not present");
        return;
    };

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

    // Banner.
    assert!(
        stdout.contains("== M24-B event_log demo =="),
        "demo banner; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("== demo done =="),
        "demo finish; stdout:\n{stdout}"
    );

    // Every probe label reports PASS.  We list them explicitly rather
    // than counting "PROBE.*PASS" lines so a regression that flips one
    // probe to FAIL surfaces with the specific name.
    let expected_pass_lines = [
        "PROBE 1 argparse 3-mode --mode opt: PASS",
        "PROBE 2 1200 INSERTs via execute_params: PASS",
        "PROBE 3 datetime.from_iso edge cases: PASS",
        "PROBE 4 month-boundary add_seconds: PASS",
        "PROBE 5 query empty result: PASS",
        "PROBE 6 SQL injection survives parameter binding: PASS",
        "PROBE 7 sqlite + datetime round-trip: PASS",
        "PROBE 8 pathlib.read_text missing file IOError: PASS",
        "PROBE 9 5000 events ingested + queried: PASS",
        "PROBE 10 hour-bucket histogram correctness: PASS",
        "PROBE U UTF-8 multibyte param binding: PASS",
        "PROBE P pre-epoch datetime round trip: PASS",
        "PROBE Y year-boundary add_days(-1): PASS",
        "PROBE D sqlite3.connect to bad dir IOError: PASS",
    ];
    for line in expected_pass_lines {
        assert!(
            stdout.contains(line),
            "expected PROBE pass line '{line}' missing\nstdout:\n{stdout}"
        );
    }
    // No FAILs anywhere.
    assert!(
        !stdout.contains("FAIL"),
        "found FAIL in output; stdout:\n{stdout}"
    );
}

const CANONICAL_INPUT: &str = "\
2026-05-19T14:00:00Z auth user login successful
2026-05-19T14:15:30Z auth user logout
2026-05-19T14:32:00Z db query ran in 5ms
2026-05-19T15:00:00Z auth failed login attempt
2026-05-19T15:10:00Z net request to api timed out
2026-05-19T15:11:00Z net retried request OK
2026-05-19T16:00:00Z db replication caught up
2026-05-19T16:30:00Z auth password reset
";

#[test]
fn event_log_ingest_query_summary_cycle() {
    let spyc_path = compile_program("cycle");
    let Some(spy_bin) = spy_exe() else {
        eprintln!("skipping: spy.exe not present");
        return;
    };

    // Use CARGO_TARGET_TMPDIR so cargo cleans up between runs.  Per-
    // test filename suffix avoids parallel-test races.
    let tmpdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let input_path = tmpdir.join("event_log_input_cycle.txt");
    let db_path = tmpdir.join("event_log_cycle.db");
    // Remove any stale DB from a previous run — otherwise the ingest
    // accumulates and the assertions count too many rows.
    let _ = fs::remove_file(&db_path);
    fs::write(&input_path, CANONICAL_INPUT).expect("write input");

    // ── 1. INGEST ────────────────────────────────────────────────
    let ingest = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg("--mode")
        .arg("ingest")
        .arg("--file")
        .arg(&input_path)
        .arg("--db")
        .arg(&db_path)
        .current_dir(project_root())
        .output()
        .expect("invoke spy.exe ingest");
    let ingest_stdout = String::from_utf8_lossy(&ingest.stdout).to_string();
    let ingest_stderr = String::from_utf8_lossy(&ingest.stderr).to_string();
    assert!(
        ingest.status.success(),
        "ingest failed; stdout:\n{ingest_stdout}\nstderr:\n{ingest_stderr}"
    );
    assert!(
        ingest_stdout.contains("ingest: inserted=8 skipped=0"),
        "ingest line; stdout:\n{ingest_stdout}"
    );

    // ── 2. QUERY ─────────────────────────────────────────────────
    let query = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg("--mode")
        .arg("query")
        .arg("--since")
        .arg("2026-05-19T15:00:00Z")
        .arg("--category")
        .arg("auth")
        .arg("--db")
        .arg(&db_path)
        .current_dir(project_root())
        .output()
        .expect("invoke spy.exe query");
    let query_stdout = String::from_utf8_lossy(&query.stdout).to_string();
    assert!(query.status.success(), "query failed: {query_stdout}");
    assert!(
        query_stdout.contains("query: matched=2"),
        "query count; stdout:\n{query_stdout}"
    );
    // Round-trip check: the integer epoch in the DB renders back to
    // exactly the input ISO string.
    assert!(
        query_stdout.contains("2026-05-19T15:00:00Z [auth] failed login attempt"),
        "first matched row; stdout:\n{query_stdout}"
    );
    assert!(
        query_stdout.contains("2026-05-19T16:30:00Z [auth] password reset"),
        "second matched row; stdout:\n{query_stdout}"
    );

    // ── 3. SUMMARY (hour) ────────────────────────────────────────
    let summary = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg("--mode")
        .arg("summary")
        .arg("--buckets")
        .arg("hour")
        .arg("--db")
        .arg(&db_path)
        .current_dir(project_root())
        .output()
        .expect("invoke spy.exe summary");
    let summary_stdout = String::from_utf8_lossy(&summary.stdout).to_string();
    assert!(summary.status.success(), "summary failed: {summary_stdout}");
    assert!(
        summary_stdout.contains("summary: buckets=hour unique=3"),
        "summary header; stdout:\n{summary_stdout}"
    );
    // Histogram lines.  The bucket label depends on hour-of-day, which
    // is UTC since datetime is UTC throughout.  Three buckets each
    // with their event count:
    //   14:00 → 3 events (login + logout + db)
    //   15:00 → 3 events (failed login + 2x net)
    //   16:00 → 2 events (db replication + password reset)
    assert!(
        summary_stdout.contains("2026-05-19 14: *** (3)"),
        "hour-14 bucket; stdout:\n{summary_stdout}"
    );
    assert!(
        summary_stdout.contains("2026-05-19 15: *** (3)"),
        "hour-15 bucket; stdout:\n{summary_stdout}"
    );
    assert!(
        summary_stdout.contains("2026-05-19 16: ** (2)"),
        "hour-16 bucket; stdout:\n{summary_stdout}"
    );

    // ── 4. SUMMARY (day) ─────────────────────────────────────────
    let day = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg("--mode")
        .arg("summary")
        .arg("--buckets")
        .arg("day")
        .arg("--db")
        .arg(&db_path)
        .current_dir(project_root())
        .output()
        .expect("invoke spy.exe day summary");
    let day_stdout = String::from_utf8_lossy(&day.stdout).to_string();
    assert!(day.status.success(), "day summary failed: {day_stdout}");
    assert!(
        day_stdout.contains("summary: buckets=day unique=1"),
        "day summary header; stdout:\n{day_stdout}"
    );
    assert!(
        day_stdout.contains("2026-05-19: ******** (8)"),
        "day bucket; stdout:\n{day_stdout}"
    );

    // Clean up the on-disk DB and input file the test wrote.
    let _ = fs::remove_file(&db_path);
    let _ = fs::remove_file(&input_path);
}

#[test]
fn event_log_ingest_missing_file_returns_2() {
    let spyc_path = compile_program("missing_file");
    let Some(spy_bin) = spy_exe() else {
        eprintln!("skipping: spy.exe not present");
        return;
    };
    let tmpdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let missing_path = tmpdir.join("_no_such_event_log_input_.txt");
    let _ = fs::remove_file(&missing_path);
    let db_path = tmpdir.join("event_log_err.db");
    let _ = fs::remove_file(&db_path);

    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg("--mode")
        .arg("ingest")
        .arg("--file")
        .arg(&missing_path)
        .arg("--db")
        .arg(&db_path)
        .current_dir(project_root())
        .output()
        .expect("invoke spy.exe");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected exit 2 for missing file; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("cannot read input file"),
        "stderr should describe the IOError; got:\n{stderr}"
    );
}

#[test]
fn event_log_query_bad_since_returns_2() {
    let spyc_path = compile_program("bad_since");
    let Some(spy_bin) = spy_exe() else {
        eprintln!("skipping: spy.exe not present");
        return;
    };
    let tmpdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let db_path = tmpdir.join("event_log_bad_since.db");
    // OK if the file doesn't exist; query will create the schema.
    let _ = fs::remove_file(&db_path);

    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg("--mode")
        .arg("query")
        .arg("--since")
        .arg("definitely-not-a-date")
        .arg("--category")
        .arg("auth")
        .arg("--db")
        .arg(&db_path)
        .current_dir(project_root())
        .output()
        .expect("invoke spy.exe");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected exit 2 for bad --since; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("bad --since"),
        "stderr should mention bad --since; got:\n{stderr}"
    );
    let _ = fs::remove_file(&db_path);
}
