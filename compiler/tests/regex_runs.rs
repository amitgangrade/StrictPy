//! Integration test for `examples/regex.spy` — a Thompson-NFA regex
//! engine that exercises post-M11 sealed-class virtual dispatch with 6
//! virtual methods on the base, class-typed subclass fields (every
//! variadic node carries `inner: RegexNode` or two `RegexNode` children),
//! and heavy recursion through both parsing and NFA construction.
//!
//! IMPORTANT: this test runs the compiled program as a SUBPROCESS via
//! `target/release/spy.exe`, NOT through `run_file_capture`. The
//! class-heavy programs (calculator, json_parse, lisp) all observed
//! teardown crashes when run in-process via `run_file_capture`. We
//! follow the same pattern here.
//!
//! The program prints `PASS pat=X input=Y` or `FAIL pat=X input=Y ...`
//! for every internal test case and finishes with `OK: N/N` if every
//! case matched the expected verdict. We assert that stdout contains
//! `OK: ` on its own line.

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
fn regex_compiles() {
    let src_path = project_root().join("examples").join("regex.spy");
    let src = fs::read_to_string(&src_path).expect("read regex.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile regex.spy: {e}"));
}

#[test]
fn regex_all_cases_pass() {
    let src_path = project_root().join("examples").join("regex.spy");
    let src = fs::read_to_string(&src_path).expect("read regex.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile regex.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("regex.spyc");
    fs::write(&spyc_path, &bytes).expect("write regex.spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    eprintln!("STDOUT:\n{stdout}");
    eprintln!("STDERR:\n{stderr}");
    eprintln!("exit status: {:?}", output.status);

    assert!(
        stdout.lines().any(|l| l.starts_with("OK: ")),
        "no OK summary line; stdout was: {stdout:?}"
    );
}

/// Re-run the regex program 10 times back-to-back to flush out any
/// non-deterministic teardown crash of the BUGS_KNOWN §4 family. The
/// regex program is class-heavy (8-variant sealed hierarchy + 6 virtual
/// methods on the base + class-typed subclass fields), exactly the
/// shape that destabilised the M9/M10 era heap. We require all 10 runs
/// to exit cleanly with the expected `OK: 15/15` final line.
#[test]
fn regex_runs_clean_10x_no_heap_corruption() {
    let src_path = project_root().join("examples").join("regex.spy");
    let src = fs::read_to_string(&src_path).expect("read regex.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile regex.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("regex_repeat.spyc");
    fs::write(&spyc_path, &bytes).expect("write regex.spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    let mut clean: usize = 0;
    let mut last_stdout = String::new();
    let mut last_status: Option<std::process::ExitStatus> = None;
    for i in 0..10 {
        let output = Command::new(&spy_bin)
            .arg(&spyc_path)
            .output()
            .expect("invoke spy.exe");
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let ok = output.status.success()
            && stdout.lines().any(|l| l == "OK: 15/15");
        if ok {
            clean += 1;
        }
        eprintln!(
            "run {i}: status={:?} ok={} tail={:?}",
            output.status,
            ok,
            stdout.lines().last().unwrap_or("")
        );
        last_stdout = stdout;
        last_status = Some(output.status);
    }
    assert_eq!(
        clean, 10,
        "expected 10/10 clean runs, got {clean}/10. last_status={:?} last_stdout={:?}",
        last_status, last_stdout
    );
}
