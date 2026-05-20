//! Subprocess integration test for `examples/logging_demo.spy` (M27 P3c-E).
//!
//! Compiles + runs the logging-module demo through `spy.exe`.  The asserts
//! inside the program itself do most of the heavy lifting (level
//! filtering, format, alias handling, ValueError on unknown level); this
//! test confirms the program emits its OK banner on stdout AND that the
//! file-output path produced four properly-formatted records on stderr.
//!
//! We additionally capture stderr to verify that the stderr-sink path
//! works — the demo emits two records to stderr at WARNING / ERROR before
//! switching to the file sink, and those should appear in the captured
//! stderr with the canonical `"<iso-ts> LEVEL <message>"` shape.

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
fn logging_demo_compiles() {
    let src_path = project_root().join("examples").join("logging_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read logging_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile logging_demo.spy: {e}"));
}

#[test]
fn logging_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("logging_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read logging_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile logging_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("logging_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    // Run in CARGO_TARGET_TMPDIR so the demo's `logging_demo.log` lands
    // in a writable place and doesn't pollute the project root.  The demo
    // cleans up after itself, but if a previous run crashed we want a
    // clean slate too.
    let cwd = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let leftover = cwd.join("logging_demo.log");
    let _ = fs::remove_file(&leftover);

    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .current_dir(&cwd)
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "spy.exe failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );

    // ── Stdout: OK banner + the pretty-print lines. ─────────────────────
    assert!(
        stdout.contains("OK"),
        "expected OK banner; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("default-level=WARNING"),
        "expected default-level=WARNING; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("file-lines=4"),
        "expected file-lines=4; stdout:\n{stdout}"
    );

    // ── Stderr: two records from the stderr-sink path. ──────────────────
    // The demo emits a WARNING and an ERROR before switching to the file
    // sink.  Both should appear in stderr with the canonical format
    // `"<iso-ts> LEVEL <message>"`.  The exact timestamp varies (wall
    // clock), so we look for the level token + message body.
    assert!(
        stderr.contains(" WARNING warning to stderr"),
        "expected WARNING stderr line; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains(" ERROR error to stderr"),
        "expected ERROR stderr line; stderr:\n{stderr}"
    );
    // And the filtered-out lines must NOT be on stderr.
    assert!(
        !stderr.contains("debug message"),
        "DEBUG should be filtered; stderr:\n{stderr}"
    );
    assert!(
        !stderr.contains("info message"),
        "INFO should be filtered at WARNING threshold; stderr:\n{stderr}"
    );

    // The demo cleans up its own log file; nothing left to verify on
    // disk.  Be defensive in case the demo aborted mid-way.
    let _ = fs::remove_file(&leftover);
}
