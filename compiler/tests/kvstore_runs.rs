//! Integration test for the KV store example.
//!
//! 1. Run the program once with an empty WAL — verify it exits 0 and that
//!    the WAL on disk contains the expected SET/DEL lines.
//! 2. Run a second time on the same WAL — verify that the worker replays
//!    the prior history and that the SET of `name` (which was set to
//!    "bob" in run 1) is visible on a GET in run 2.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn kvstore_persists_via_wal_and_recovers() {
    // Compile the example.
    let src_path = project_root().join("examples").join("kvstore.spy");
    let src = fs::read_to_string(&src_path).expect("read kvstore.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile kvstore.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("kvstore.spyc");
    fs::write(&spyc_path, &bytes).expect("write kvstore.spyc");

    // Unique work dir so we don't race other tests, and so the relative
    // `kvstore.wal` path the program uses lands here.
    let work_dir = std::env::temp_dir().join("strictpy_kvstore_test");
    let _ = fs::remove_dir_all(&work_dir);
    fs::create_dir_all(&work_dir).expect("mkdir work dir");
    let wal_path = work_dir.join("kvstore.wal");
    fs::write(&wal_path, b"").expect("touch empty WAL");

    // chdir for the relative open() lookup.
    let original = std::env::current_dir().expect("cwd");

    // ── Run 1: fresh WAL ───────────────────────────────────────────────
    std::env::set_current_dir(&work_dir).expect("chdir for run 1");
    let outcome = std::panic::catch_unwind(|| run_file_capture(&spyc_path));
    let _ = std::env::set_current_dir(&original);
    let result = outcome.expect("run 1 panicked");
    let (code, out) = result.expect("run 1 must succeed");
    assert_eq!(code, 0, "run 1 exit code; stdout was: {out:?}");

    // The scripted workload prints "<cmd> → <resp>" for each request.
    assert!(out.contains("GET\tname → alice"), "GET name=alice missing: {out:?}");
    assert!(out.contains("GET\tage → 30"), "GET age=30 missing: {out:?}");
    assert!(out.contains("GET\tmissing → <none>"), "GET missing: {out:?}");
    // After the SET name bob, the next GET should see bob.
    assert!(out.contains("GET\tname → bob"), "GET name=bob missing: {out:?}");
    // The DEL nope branch.
    assert!(out.contains("DEL\tnope → <missing>"), "DEL nope: {out:?}");
    // Last bulk-set value should be visible via GET counter.
    assert!(out.contains("GET\tcounter → iter-199"), "GET counter: {out:?}");
    assert!(out.contains("SHUTDOWN → BYE"), "SHUTDOWN: {out:?}");

    // WAL should contain matching SET/DEL records.
    let wal = fs::read_to_string(&wal_path).expect("read WAL after run 1");
    assert!(wal.contains("SET\tname\talice\n"), "missing SET name alice in WAL: {wal:?}");
    assert!(wal.contains("SET\tname\tbob\n"), "missing SET name bob: {wal:?}");
    assert!(wal.contains("DEL\tage\n"), "missing DEL age: {wal:?}");
    assert!(wal.contains("SET\tcounter\titer-199\n"), "missing final SET: {wal:?}");
    // DEL nope was rejected pre-write — so no record for it.
    assert!(!wal.contains("DEL\tnope\n"), "spurious DEL nope: {wal:?}");

    // ── Run 2: replay the WAL ──────────────────────────────────────────
    std::env::set_current_dir(&work_dir).expect("chdir for run 2");
    let outcome2 = std::panic::catch_unwind(|| run_file_capture(&spyc_path));
    let _ = std::env::set_current_dir(&original);
    let result2 = outcome2.expect("run 2 panicked");
    let (code2, out2) = result2.expect("run 2 must succeed");
    assert_eq!(code2, 0, "run 2 exit code; stdout was: {out2:?}");

    // Run 2 still runs the same scripted workload, so the asserts above
    // still hold. The key test is that the WAL has grown — it should now
    // contain BOTH runs' records.
    let wal2 = fs::read_to_string(&wal_path).expect("read WAL after run 2");
    assert!(wal2.len() > wal.len(), "WAL did not grow on run 2: was {} now {}", wal.len(), wal2.len());
}
