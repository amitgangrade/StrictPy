//! Wave-1 Lane D regression test: `examples/exception_control_flow_demo.spy`.
//!
//! Pins three exception control-flow fixes that were silent miscompiles:
//!   1. `try/except/else` — the `else` clause runs iff the body raised nothing.
//!   2. `except (A, B)` — catches A or B but NOT an unrelated C.
//!   3. `raise X from Y` — the cause is preserved (folded into the message).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

fn compile_demo() -> Vec<u8> {
    let src_path = project_root()
        .join("examples")
        .join("exception_control_flow_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read exception_control_flow_demo.spy");
    compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile exception_control_flow_demo.spy: {e}"))
}

#[test]
fn exception_control_flow_demo_compiles() {
    let _ = compile_demo();
}

#[test]
fn exception_control_flow_demo_output() {
    let bytes = compile_demo();
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("exception_control_flow_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run exception_control_flow_demo");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // 1) else runs on the success path, NOT when the body raised.
    assert!(out.contains("else ran: success\n"), "stdout:\n{out}");
    assert!(
        out.contains("else-test: handler ran for ve\n"),
        "stdout:\n{out}"
    );
    assert!(
        !out.contains("else ran: WRONG"),
        "else clause must not run when the body raised; stdout:\n{out}"
    );
    assert!(
        !out.contains("else-test: WRONG handler ran"),
        "handler must not run on the success path; stdout:\n{out}"
    );

    // 2) except (ValueError, KeyError) catches both, but NOT IndexError.
    assert!(
        out.matches("tuple caught: ").count() == 2,
        "tuple must catch exactly ValueError + KeyError; stdout:\n{out}"
    );
    assert!(out.contains("tuple caught: ve\n"), "stdout:\n{out}");
    assert!(out.contains("tuple caught: ke\n"), "stdout:\n{out}");
    assert!(
        !out.contains("tuple caught: WRONG"),
        "IndexError must not be caught by (ValueError, KeyError); stdout:\n{out}"
    );
    assert!(
        out.contains("outer caught uncaught-by-tuple: ie\n"),
        "IndexError must fall through to the outer catch-all; stdout:\n{out}"
    );

    // 3) raise KeyError("wrapped") from cause preserves the ValueError cause.
    //    The cause is folded into the message as a chained suffix.
    assert!(
        out.contains("chained: wrapped [caused by ValueError: ve]\n"),
        "raise-from cause must be preserved in the message; stdout:\n{out}"
    );

    assert!(out.trim_end().ends_with("done"), "stdout:\n{out}");
}
