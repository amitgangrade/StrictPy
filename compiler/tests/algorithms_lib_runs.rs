//! M18 acceptance test: `examples/algorithms_lib.spy` — a small generic
//! algorithms library that stress-combines ALL of the M13–M17 surface:
//! generic free functions (M17) returning tuples (M14), with internal
//! `try/except` around `xs[i]` (M15), short-circuit `and` in bounds
//! checks (M13), and `isinstance`-flavoured exception-type-name
//! inspection (M16-ish, since user-defined exception subclasses are
//! v0.2). 18 in-program tests, prefixed `PASS test=N` / `FAIL test=N`.
//!
//! Asserts:
//!   - the program exits 0,
//!   - the final summary line is `OK: 18/18`,
//!   - every individual test prints a `PASS` (no `FAIL` lines).

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
fn algorithms_lib_compiles() {
    let src_path = project_root().join("examples").join("algorithms_lib.spy");
    let src = fs::read_to_string(&src_path).expect("read algorithms_lib.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile algorithms_lib.spy: {e}"));
}

#[test]
fn algorithms_lib_all_tests_pass() {
    let src_path = project_root().join("examples").join("algorithms_lib.spy");
    let src = fs::read_to_string(&src_path).expect("read algorithms_lib.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile algorithms_lib.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("algorithms_lib.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run algorithms_lib");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // No individual test should have failed.
    assert!(
        !out.contains("FAIL test="),
        "at least one test FAILed; stdout:\n{out}"
    );

    // The final summary line is the load-bearing acceptance signal.
    assert!(
        out.contains("OK: 18/18\n") || out.trim_end().ends_with("OK: 18/18"),
        "expected `OK: 18/18` summary line; stdout:\n{out}"
    );
}
