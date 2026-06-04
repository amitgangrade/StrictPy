//! M63b acceptance test: `examples/bounded_generics_demo.spy`.
//!
//! Exercises bounded generics end to end:
//!   - `max2[T: Comparable]` instantiated over i64, f64, and str — the `<`
//!     inside the body type-checks only because `T` carries `Comparable`,
//!     and each instantiation lowers to the right per-type compare op.
//!   - `Pair2[T: Comparable]`, a bounded generic class that orders its two
//!     values in `__init__`, instantiated over i64 and str.
//!
//! Asserts the program exits 0 and produces the expected deterministic
//! stdout.

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
fn bounded_generics_demo_compiles() {
    let src_path = project_root().join("examples").join("bounded_generics_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read bounded_generics_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile bounded_generics_demo.spy: {e}"));
}

#[test]
fn bounded_generics_demo_runs_with_expected_output() {
    let src_path = project_root().join("examples").join("bounded_generics_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read bounded_generics_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile bounded_generics_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("bounded_generics_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run bounded_generics_demo");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    let expected = "\
7
9.25
banana
7
ordered
zebra
";
    assert_eq!(out, expected, "stdout mismatch:\n{out}");
}
