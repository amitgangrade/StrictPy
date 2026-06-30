//! Wave 2 / Lane C acceptance test: `examples/dunder_index_demo.spy` —
//! subscript read/write on a user class dispatching to `__getitem__` /
//! `__setitem__`, plus a negative case proving a wrong key type is a clean
//! type error rather than a runtime trap.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_compiler::error::CompileError;
use strictpy_vm::run_file_capture;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn dunder_index_demo_compiles() {
    let src_path = project_root().join("examples").join("dunder_index_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read dunder_index_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile dunder_index_demo.spy: {e}"));
}

#[test]
fn dunder_index_demo_runs_ok() {
    let src_path = project_root().join("examples").join("dunder_index_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read dunder_index_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile dunder_index_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("dunder_index_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run dunder_index_demo");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // __getitem__ reads.
    assert!(out.contains("v[0]=10\n"), "stdout:\n{out}");
    assert!(out.contains("v[1]=20\n"), "stdout:\n{out}");
    assert!(out.contains("v[2]=30\n"), "stdout:\n{out}");
    // __setitem__ write then read-back.
    assert!(out.contains("after v[1]=99: 99\n"), "stdout:\n{out}");
    // aug-assign: __getitem__ load + __setitem__ store.
    assert!(out.contains("v[2]+5=35\n"), "stdout:\n{out}");
    // generic class: __getitem__/__setitem__ monomorphised at Box[str].
    assert!(out.contains("bx[0]=alpha\n"), "stdout:\n{out}");
    assert!(out.contains("bx[1]=GAMMA\n"), "stdout:\n{out}");
    // str-keyed container: key type follows the declared `str` parameter.
    assert!(out.contains("apples=4\n"), "stdout:\n{out}");
    assert!(out.contains("pears=7\n"), "stdout:\n{out}");
    assert!(out.contains("missing=0\n"), "stdout:\n{out}");
    assert!(
        out.contains("OK: dunder-index\n")
            || out.trim_end().ends_with("OK: dunder-index"),
        "stdout:\n{out}"
    );
}

/// A wrong key type on a class subscript-read must be rejected by the type
/// checker — the key `"x"` (str) does not match `__getitem__`'s `i64` param.
#[test]
fn dunder_index_wrong_key_type_is_type_error() {
    let src = r#"
final class IntVec:
    data: List[i64]

    fn __init__(self, seed: i64) -> None:
        self.data = [seed]

    fn __getitem__(self, i: i64) -> i64:
        return self.data[i]

fn main() -> i32:
    v: IntVec = IntVec(10)
    bad: i64 = v["x"]
    return 0
"#;
    let err = compile_source("dunder_bad_key.spy".to_string(), src)
        .expect_err("subscript with str key on i64-keyed __getitem__ must fail");
    assert!(
        matches!(err, CompileError::Type { .. }),
        "expected a Type error, got: {err:?}"
    );
}

/// A wrong value type on a class subscript-store must be rejected by the type
/// checker — assigning a `str` into an `i64`-valued `__setitem__`.
#[test]
fn dunder_index_wrong_value_type_is_type_error() {
    let src = r#"
final class IntVec:
    data: List[i64]

    fn __init__(self, seed: i64) -> None:
        self.data = [seed]

    fn __setitem__(self, i: i64, v: i64) -> None:
        self.data[i] = v

fn main() -> i32:
    v: IntVec = IntVec(10)
    v[0] = "nope"
    return 0
"#;
    let err = compile_source("dunder_bad_value.spy".to_string(), src)
        .expect_err("storing str into i64-valued __setitem__ must fail");
    assert!(
        matches!(err, CompileError::Type { .. }),
        "expected a Type error, got: {err:?}"
    );
}
