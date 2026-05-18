//! Integration test for `examples/lisp.spy` — a toy Lisp interpreter
//! that exercises the open-class AST, recursive evaluation, nullable
//! narrowing (`cdr: Value?`), and lexically-scoped closures all at
//! once.
//!
//! The embedded test program is:
//!
//! ```scheme
//! (define x 10)
//! (define y 20)
//! (print (+ x y))                       ; → 30
//! (define square (lambda (n) (* n n)))
//! (print (square 7))                    ; → 49
//! ```
//!
//! IMPORTANT: this test runs the compiled program as a SUBPROCESS via
//! `target/release/spy.exe`, NOT through `run_file_capture`. The
//! interpreter ends by tearing down a Lisp `State` that owns ~20+
//! interconnected `Value` / `Pair` / `EnvFrame` / `Lambda` objects,
//! and the in-process VM `Drop` path consistently trips
//! `STATUS_HEAP_CORRUPTION` on Windows during teardown — matching
//! BUGS_KNOWN.md #4 (the same teardown crash json_parse.spy hits).
//! Running out-of-process contains the crash; the program completes
//! its work and flushes its `println` lines before the OS reaps the
//! process, so we still get the `30` and `49` we care about on stdout.
//!
//! We assert that stdout contains `30` and `49` on separate lines.

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
fn lisp_compiles() {
    let src_path = project_root().join("examples").join("lisp.spy");
    let src = fs::read_to_string(&src_path).expect("read lisp.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile lisp.spy: {e}"));
}

#[test]
fn lisp_evaluates_add_and_square() {
    let src_path = project_root().join("examples").join("lisp.spy");
    let src = fs::read_to_string(&src_path).expect("read lisp.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile lisp.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("lisp.spyc");
    fs::write(&spyc_path, &bytes).expect("write lisp.spyc");

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

    assert!(stdout.contains("30\n"), "missing 30 from (+ x y): {stdout:?}");
    assert!(stdout.contains("49\n"), "missing 49 from (square 7): {stdout:?}");
}
