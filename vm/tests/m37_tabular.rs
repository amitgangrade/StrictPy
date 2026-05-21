//! M37 in-process regression tests for the `tabular` stdlib module.
//!
//! The module ships a sealed Column hierarchy (ColumnI64/F64/Str/Bool/
//! DateTime), a DataFrame class with named columns + RangeIndex, CSV/SQL
//! I/O, per-column comparison methods producing ColumnBool masks, mask
//! combinators, and `df.filter/select/drop/head/tail/sort_by`.  See
//! `LANGUAGE_GUIDE.md` §5 (post-M37) for the user-facing surface.
//!
//! Tests use temporary files under `CARGO_TARGET_TMPDIR` so they run
//! in parallel without filesystem races (each test name namespaces
//! its own `.spy` bytecode + any CSV side files).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m37_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn construct_col_i64_and_check_length() {
    let src = "\
from tabular import ColumnI64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    println(\"length=\" + str(c.length()))
    println(\"dtype=\" + c.dtype())
    println(\"null_count=\" + str(c.null_count()))
    return 0
";
    let p = compile_snippet("col_i64_basic", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("length=3"), "got: {out:?}");
    assert!(out.contains("dtype=i64"), "got: {out:?}");
    assert!(out.contains("null_count=0"), "got: {out:?}");
}
