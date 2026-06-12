//! Strings round 2 acceptance test: `examples/strings_round2_demo.spy` —
//! exercises the native string toolkit (join / lower / upper / repeat), the
//! fixed `char_at` wiring (REPORT_V2 bug #7), the O(1) ASCII `s[i]` /
//! O(slice_len) ASCII `s.slice(...)` fast paths with non-ASCII fallback,
//! and the widened chained in-place append (`s = s + a + b`) including
//! alias safety. 16 in-program tests, prefixed `PASS test=N` / `FAIL test=N`.
//!
//! Asserts:
//!   - the program exits 0,
//!   - the final summary line is `OK: 16/16`,
//!   - every individual test prints a `PASS` (no `FAIL` lines).
//!
//! Modeled on compiler/tests/algorithms_lib_runs.rs.

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
fn strings_round2_demo_compiles() {
    let src_path = project_root().join("examples").join("strings_round2_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read strings_round2_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile strings_round2_demo.spy: {e}"));
}

#[test]
fn strings_round2_demo_all_tests_pass() {
    let src_path = project_root().join("examples").join("strings_round2_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read strings_round2_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile strings_round2_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("strings_round2_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run strings_round2_demo");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // No individual test should have failed.
    assert!(
        !out.contains("FAIL test="),
        "at least one test FAILed; stdout:\n{out}"
    );

    // The final summary line is the load-bearing acceptance signal.
    assert!(
        out.contains("OK: 16/16\n") || out.trim_end().ends_with("OK: 16/16"),
        "expected `OK: 16/16` summary line; stdout:\n{out}"
    );
}
