//! Subprocess integration test for `examples/sqlite_class_demo.spy`
//! (M35 P4-B).
//!
//! Compiles + runs the typed-sqlite demo through `spy.exe` and asserts
//! the printed output is consistent with an end-to-end CREATE / INSERT
//! / SELECT / fetchone-iteration / fetchall / UPDATE workflow against
//! an in-memory SQLite DB via the new `Connection` / `Cursor` classes.

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
fn sqlite_class_demo_compiles() {
    let src_path = project_root().join("examples").join("sqlite_class_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read sqlite_class_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile sqlite_class_demo.spy: {e}"));
}

#[test]
fn sqlite_class_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("sqlite_class_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read sqlite_class_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile sqlite_class_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sqlite_class_demo.spyc");
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

    // After 3 INSERTs the rowid of the most recent row is 3.
    assert!(
        stdout.contains("last_insert_rowid=3"),
        "rowid; stdout:\n{stdout}"
    );
    // SELECT round-trips all three rows.
    assert!(stdout.contains("rows=3"), "row count; stdout:\n{stdout}");
    // fetchone iteration emits each row.
    assert!(stdout.contains("row id=1 title=first"), "first row; stdout:\n{stdout}");
    assert!(
        stdout.contains("row id=3 title=O'Brien"),
        "third title; stdout:\n{stdout}"
    );
    // SQL-injection-shaped body survives verbatim through parameter binding.
    assert!(
        stdout.contains("body='; drop table notes;--"),
        "third body; stdout:\n{stdout}"
    );
    // After iteration, fetchall returns the empty list.
    assert!(stdout.contains("leftover=0"), "leftover; stdout:\n{stdout}");
    // A second cursor on the same connection filters down to "O'Brien".
    assert!(stdout.contains("oh_rows=1"), "oh_rows; stdout:\n{stdout}");
    assert!(stdout.contains("oh_title=O'Brien"), "oh title; stdout:\n{stdout}");
    // UPDATE reports the affected rows via changes().
    assert!(stdout.contains("updated=2"), "updated; stdout:\n{stdout}");
    // Close is idempotent.
    assert!(stdout.contains("closed"), "closed; stdout:\n{stdout}");
}
