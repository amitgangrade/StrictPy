//! M31 acceptance test: `examples/generic_classes_demo.spy` exercises
//! the new generic-class surface end-to-end.
//!
//! Asserts:
//!   - the program compiles and runs to exit 0,
//!   - the expected lines appear in order: Box[i64], Box[str], Pair
//!     fields, Stack[i64] mutation, final size.

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
fn generic_classes_demo_compiles() {
    let src_path = project_root()
        .join("examples")
        .join("generic_classes_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read generic_classes_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile generic_classes_demo.spy: {e}"));
}

#[test]
fn generic_classes_demo_runs() {
    let src_path = project_root()
        .join("examples")
        .join("generic_classes_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read generic_classes_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile generic_classes_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("generic_classes_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run demo");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // Box[i64] line.
    assert!(out.contains("900000000000\n"), "Box[i64].unwrap: {out:?}");
    // Box[str] line.
    assert!(out.contains("hello\n"), "Box[str].unwrap: {out:?}");
    // Pair[str, i32] fields.
    assert!(out.contains("answer\n"), "Pair.first: {out:?}");
    assert!(out.contains("42\n"), "Pair.second: {out:?}");
    // Stack[i64] sequence: size=3 after pushes, pops yield 30 then 20,
    // final size 1.
    assert!(out.contains("3\n"), "Stack.size after pushes: {out:?}");
    assert!(out.contains("30\n"), "Stack.pop (third): {out:?}");
    assert!(out.contains("20\n"), "Stack.pop (second): {out:?}");
    assert!(out.contains("1\n"), "Stack.size final: {out:?}");
}
