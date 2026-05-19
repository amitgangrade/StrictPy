//! Subprocess integration test for `examples/sqlite_demo.spy`
//! (M23 P3a-D).
//!
//! Compiles + runs the sqlite demo through `spy.exe` and asserts the
//! printed output is consistent with an end-to-end CREATE / INSERT
//! (parameter-bound) / SELECT / UPDATE / column-discovery / try-except
//! workflow against an in-memory SQLite DB.

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
fn sqlite_demo_compiles() {
    let src_path = project_root().join("examples").join("sqlite_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read sqlite_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile sqlite_demo.spy: {e}"));
}

#[test]
fn sqlite_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("sqlite_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read sqlite_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile sqlite_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sqlite_demo.spyc");
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
        stdout.contains("last_insert_rowid: 3"),
        "rowid; stdout:\n{stdout}"
    );
    // SELECT round-trips all three rows.
    assert!(stdout.contains("row count: 3"), "row count; stdout:\n{stdout}");
    assert!(stdout.contains("first title: first"), "first; stdout:\n{stdout}");
    // The apostrophe-containing title survives parameter binding.
    assert!(
        stdout.contains("third title: O'Brien"),
        "third title; stdout:\n{stdout}"
    );
    // The SQL-injection-shaped body is stored verbatim, not executed.
    assert!(
        stdout.contains("third body: drop table notes;--"),
        "third body; stdout:\n{stdout}"
    );
    // column_names returns id / title / body.
    assert!(stdout.contains("columns: 3"), "columns; stdout:\n{stdout}");
    assert!(stdout.contains("col0: id"), "col0; stdout:\n{stdout}");
    assert!(stdout.contains("col1: title"), "col1; stdout:\n{stdout}");
    assert!(stdout.contains("col2: body"), "col2; stdout:\n{stdout}");
    // query_params filters the O'Brien row out of three notes.
    assert!(
        stdout.contains("matches for O'Brien: 1"),
        "qparams; stdout:\n{stdout}"
    );
    // changes() reports the most recent UPDATE.
    assert!(stdout.contains("rows updated: 1"), "changes; stdout:\n{stdout}");
    // Bad SQL trips the try/except ValueError.
    assert!(
        stdout.contains("caught ValueError on bad SQL"),
        "try/except; stdout:\n{stdout}"
    );
    assert!(stdout.contains("done"), "done; stdout:\n{stdout}");
}
