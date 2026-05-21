//! M34 integration test for `examples/json_typed_demo.spy`.
//!
//! Compiles the typed-JsonValue demo and runs it through the VM
//! in-process (no `spy.exe` subprocess), asserting on the expected
//! outputs.  The demo exercises:
//!
//!   * `json.parse` returning a typed `JsonValue` tree
//!   * `match v: case JFoo(...):` walking the tree
//!   * `JObject` method API (.has / .get / .keys / .length)
//!   * `JList` method API (.length / .get / .items)
//!   * Programmatic construction via `json.j_*` helpers and direct
//!     class constructors
//!   * `json.stringify` / `json.stringify_pretty` round-trip
//!   * `try / except ValueError` on malformed input

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
fn json_typed_demo_compiles() {
    let src_path = project_root().join("examples").join("json_typed_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read json_typed_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile json_typed_demo.spy: {e}"));
}

#[test]
fn json_typed_demo_runs_in_process() {
    let src_path = project_root().join("examples").join("json_typed_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read json_typed_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile json_typed_demo.spy: {e}"));

    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("json_typed_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run");
    assert_eq!(code, 0, "exit code; stdout={out}");

    // Parsed tree round-trip
    assert!(out.contains("parsed:"), "missing parsed: line\nstdout:\n{out}");
    assert!(out.contains("greeted:   hello, alice"), "stdout:\n{out}");

    // Programmatic build emits the same shape
    assert!(out.contains("built:"), "missing built: line\nstdout:\n{out}");

    // Encoded canonical form
    assert!(
        out.contains(r#"encoded:   {"name":"alice","age":30,"tags":["admin","user"],"active":true}"#),
        "missing encoded line; stdout:\n{out}"
    );

    // Reparsed tree describes back to the same shape
    assert!(out.contains("reparsed:"), "missing reparsed:; stdout:\n{out}");

    // Pretty output exists with newlines + indent
    assert!(out.contains("pretty:"), "missing pretty:; stdout:\n{out}");
    assert!(
        out.contains("\n  \"name\":"),
        "pretty output missing indented key; stdout:\n{out}"
    );

    // Error path
    assert!(out.contains("caught:    ValueError"), "stdout:\n{out}");
    assert!(!out.contains("should not reach"), "stdout:\n{out}");

    // JList method walk
    assert!(out.contains("list len:  4"), "stdout:\n{out}");
    assert!(out.contains("sum:       100"), "stdout:\n{out}");

    assert!(out.contains("done"), "missing done; stdout:\n{out}");
}
