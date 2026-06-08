//! M62b acceptance test: `examples/generators_demo.spy` — compiles and runs
//! the demo that defines generator functions (`-> Iterator[T]` with `yield`)
//! and consumes them with `for v: T in gen():`.
//!
//! Asserts:
//!   - the program compiles,
//!   - it exits 0,
//!   - the deterministic output matches exactly.

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
fn generators_demo_compiles() {
    let src_path = project_root().join("examples").join("generators_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read generators_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile generators_demo.spy: {e}"));
}

#[test]
fn generators_demo_output() {
    let src_path = project_root().join("examples").join("generators_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read generators_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile generators_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("generators_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run generators_demo");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    let expected = "\
count_up: 0 1 2 3 4\n\
fib: 0 1 1 2 3 5 8 13 21 34\n\
empty iterations=0\n\
evens: 0 2 4 6 8\n\
evens count=5\n\
twice total=12\n";
    assert_eq!(out, expected, "stdout mismatch:\n{out}");
}
