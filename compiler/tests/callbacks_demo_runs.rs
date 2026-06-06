//! M61a acceptance test: `examples/callbacks_demo.spy` — compiles and runs
//! the demo that maps, filters, reduces, and sorts-by-key using user
//! closures (including capturing closures) across the NativeFn boundary.
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
fn callbacks_demo_compiles() {
    let src_path = project_root().join("examples").join("callbacks_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read callbacks_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile callbacks_demo.spy: {e}"));
}

#[test]
fn callbacks_demo_output() {
    let src_path = project_root().join("examples").join("callbacks_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read callbacks_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile callbacks_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("callbacks_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run callbacks_demo");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    let expected = "\
scaled: 50 20 80 10 90 30 70 40 60\n\
big: 5 8 9 7 6\n\
sum=45\n\
product=362880\n\
by_age: Bob(19) Carol(30) Dave(30) Alice(42)\n\
by_name: Alice Bob Carol Dave\n\
by_name_len: Bob Dave Carol Alice\n";
    assert_eq!(out, expected, "stdout mismatch:\n{out}");
}
