//! M38 in-process regression tests for the `tabular` round-out.
//!
//! Covers the post-M37 additions: typed DataFrame accessors, restored
//! comparison operations (ne/ge/le/between + starts_with/ends_with),
//! `df.rename`, per-column aggregations (sum/mean/min/max/count/std/
//! var/median), `df.describe`, `Column.fill_null` per subclass,
//! `tabular.from_dict`, and hash-based group-by (the new
//! `GroupedDataFrame` class).
//!
//! See `LANGUAGE_GUIDE.md` §5 (post-M38) for the full surface.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m38_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── Phase A: typed accessors + restored comparison ops ───────────────

#[test]
fn get_column_i64_hit_and_miss() {
    let src = "\
from tabular import ColumnI64, ColumnStr, DataFrame, Column
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    ages: ColumnI64 = tabular.col_i64_simple(vs)
    nms: List[str] = []
    nms.append(\"alice\")
    nms.append(\"bob\")
    nms.append(\"carol\")
    names_col: ColumnStr = tabular.col_str_simple(nms)
    cn: List[str] = []
    cn.append(\"age\")
    cn.append(\"name\")
    cs: List[Column] = []
    cs.append(ages)
    cs.append(names_col)
    df: DataFrame = tabular.from_columns(cn, cs)
    # hit: age is an i64 column
    a: ColumnI64? = df.get_column_i64(\"age\")
    if a is not none:
        println(\"age_len=\" + str(a.length()))
    else:
        println(\"age_len=missing\")
    # miss: name is a str column
    b: ColumnI64? = df.get_column_i64(\"name\")
    if b is none:
        println(\"name_as_i64=none\")
    # miss: missing column
    c: ColumnI64? = df.get_column_i64(\"missing\")
    if c is none:
        println(\"missing=none\")
    return 0
";
    let p = compile_snippet("get_column_i64", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("age_len=3"), "got: {out:?}");
    assert!(out.contains("name_as_i64=none"), "got: {out:?}");
    assert!(out.contains("missing=none"), "got: {out:?}");
}
