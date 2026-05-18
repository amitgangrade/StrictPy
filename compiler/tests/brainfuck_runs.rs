//! Integration test for the Brainfuck interpreter example.
//!
//! Compiles `examples/brainfuck.spy`, runs it through the VM's
//! `run_file_capture`, and asserts that the stdout contains the famous
//! "Hello World!" string emitted by the embedded 106-byte canonical
//! "Hello World" BF program.

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
fn brainfuck_prints_hello_world() {
    let src_path = project_root().join("examples").join("brainfuck.spy");
    let src = fs::read_to_string(&src_path).expect("read brainfuck.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile brainfuck.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("brainfuck.spyc");
    fs::write(&spyc_path, &bytes).expect("write brainfuck.spyc");

    let (code, out) =
        run_file_capture(&spyc_path).unwrap_or_else(|e| panic!("brainfuck.spy: vm error: {e}"));
    assert_eq!(code, 0, "exit code; stdout was: {out:?}");

    // The canonical BF Hello World program emits exactly "Hello World!\n".
    // Our `run()` returns that as a `str` and the program then `println`s
    // it, so stdout should contain "Hello World!" somewhere.
    assert!(
        out.contains("Hello World!"),
        "missing Hello World output: {out:?}"
    );
}
