//! M40 in-process regression tests for the `tabular` time-series,
//! cumulative, null-handling, range-slicing, rolling-window, resample,
//! and asof-merge operations.
//!
//! Phase 5 of the Pandas-shaped data package.  Surface documented in
//! LANGUAGE_GUIDE.md §5 (post-M40).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m40_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

fn run(name: &str, src: &str) -> String {
    let p = compile_snippet(name, src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "{name}: exit={code} stdout={out:?}");
    out
}

// ── Phase A: cumulative ──────────────────────────────────────────────

#[test]
fn cumsum_i64_happy_path() {
    let src = "\
from tabular import ColumnI64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    vs.append(4i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    cs: ColumnI64 = c.cumsum()
    println(\"len=\" + str(cs.length()))
    v0: i64? = cs.get(0i64)
    v3: i64? = cs.get(3i64)
    if v0 is not none:
        println(\"v0=\" + str(v0))
    if v3 is not none:
        println(\"v3=\" + str(v3))
    return 0
";
    let out = run("cumsum_i64", src);
    assert!(out.contains("len=4"), "got: {out:?}");
    assert!(out.contains("v0=1"), "got: {out:?}");
    assert!(out.contains("v3=10"), "got: {out:?}");
}

#[test]
fn cumsum_null_propagates_forward() {
    // [1, 2, null, 4] → [1, 3, null, null] (null propagates)
    let src = "\
from tabular import ColumnI64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(0i64)
    vs.append(4i64)
    ns: List[bool] = []
    ns.append(false)
    ns.append(false)
    ns.append(true)
    ns.append(false)
    c: ColumnI64 = tabular.col_i64(vs, ns)
    cs: ColumnI64 = c.cumsum()
    println(\"nc=\" + str(cs.null_count()))
    v1: i64? = cs.get(1i64)
    if v1 is not none:
        println(\"v1=\" + str(v1))
    v2: i64? = cs.get(2i64)
    if v2 is none:
        println(\"v2=null\")
    v3: i64? = cs.get(3i64)
    if v3 is none:
        println(\"v3=null\")
    return 0
";
    let out = run("cumsum_null", src);
    assert!(out.contains("nc=2"), "got: {out:?}");
    assert!(out.contains("v1=3"), "got: {out:?}");
    assert!(out.contains("v2=null"), "got: {out:?}");
    assert!(out.contains("v3=null"), "got: {out:?}");
}

#[test]
fn cumprod_cummax_cummin_i64_smoke() {
    let src = "\
from tabular import ColumnI64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(2i64)
    vs.append(3i64)
    vs.append(1i64)
    vs.append(4i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    cp: ColumnI64 = c.cumprod()
    cmx: ColumnI64 = c.cummax()
    cmn: ColumnI64 = c.cummin()
    p3: i64? = cp.get(3i64)
    mx3: i64? = cmx.get(3i64)
    mn3: i64? = cmn.get(3i64)
    if p3 is not none:
        println(\"prod3=\" + str(p3))
    if mx3 is not none:
        println(\"max3=\" + str(mx3))
    if mn3 is not none:
        println(\"min3=\" + str(mn3))
    return 0
";
    let out = run("cumprod_cummax_cummin", src);
    assert!(out.contains("prod3=24"), "got: {out:?}");
    assert!(out.contains("max3=4"), "got: {out:?}");
    assert!(out.contains("min3=1"), "got: {out:?}");
}

#[test]
fn cumsum_f64_smoke() {
    let src = "\
from tabular import ColumnF64
import tabular
fn main() -> i32:
    vs: List[f64] = []
    vs.append(1.5)
    vs.append(2.5)
    vs.append(3.0)
    c: ColumnF64 = tabular.col_f64_simple(vs)
    cs: ColumnF64 = c.cumsum()
    v2: f64? = cs.get(2i64)
    if v2 is not none:
        println(\"v2=\" + str(v2))
    return 0
";
    let out = run("cumsum_f64", src);
    assert!(out.contains("v2=7"), "got: {out:?}");
}

// ── Phase A: dropna / fillna ─────────────────────────────────────────

#[test]
fn dropna_drops_rows_with_any_null() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    a_vs: List[i64] = []
    a_vs.append(1i64)
    a_vs.append(2i64)
    a_vs.append(3i64)
    a_ns: List[bool] = []
    a_ns.append(false)
    a_ns.append(true)
    a_ns.append(false)
    a: ColumnI64 = tabular.col_i64(a_vs, a_ns)
    b_vs: List[str] = []
    b_vs.append(\"x\")
    b_vs.append(\"y\")
    b_vs.append(\"z\")
    b: ColumnStr = tabular.col_str_simple(b_vs)
    cn: List[str] = []
    cn.append(\"a\")
    cn.append(\"b\")
    cs: List[Column] = []
    cs.append(a)
    cs.append(b)
    df: DataFrame = tabular.from_columns(cn, cs)
    cleaned: DataFrame = df.dropna()
    println(\"len=\" + str(cleaned.length()))
    return 0
";
    let out = run("dropna", src);
    assert!(out.contains("len=2"), "got: {out:?}");
}

#[test]
fn dropna_subset_only_considers_listed_columns() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    a_vs: List[i64] = []
    a_vs.append(1i64)
    a_vs.append(2i64)
    a_vs.append(3i64)
    a_ns: List[bool] = []
    a_ns.append(false)
    a_ns.append(true)
    a_ns.append(false)
    a: ColumnI64 = tabular.col_i64(a_vs, a_ns)
    b_vs: List[str] = []
    b_vs.append(\"x\")
    b_vs.append(\"y\")
    b_vs.append(\"z\")
    b_ns: List[bool] = []
    b_ns.append(false)
    b_ns.append(false)
    b_ns.append(true)
    b: ColumnStr = tabular.col_str(b_vs, b_ns)
    cn: List[str] = []
    cn.append(\"a\")
    cn.append(\"b\")
    cs: List[Column] = []
    cs.append(a)
    cs.append(b)
    df: DataFrame = tabular.from_columns(cn, cs)
    # Drop only by 'a' — keep rows 0 and 2 (b's null doesn't count).
    keep_cols: List[str] = []
    keep_cols.append(\"a\")
    cleaned: DataFrame = df.dropna_subset(keep_cols)
    println(\"len=\" + str(cleaned.length()))
    return 0
";
    let out = run("dropna_subset", src);
    assert!(out.contains("len=2"), "got: {out:?}");
}

#[test]
fn fillna_i64_fills_only_i64_columns() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    a_vs: List[i64] = []
    a_vs.append(1i64)
    a_vs.append(0i64)
    a_vs.append(3i64)
    a_ns: List[bool] = []
    a_ns.append(false)
    a_ns.append(true)
    a_ns.append(false)
    a: ColumnI64 = tabular.col_i64(a_vs, a_ns)
    b_vs: List[str] = []
    b_vs.append(\"x\")
    b_vs.append(\"y\")
    b_vs.append(\"z\")
    b: ColumnStr = tabular.col_str_simple(b_vs)
    cn: List[str] = []
    cn.append(\"a\")
    cn.append(\"b\")
    cs: List[Column] = []
    cs.append(a)
    cs.append(b)
    df: DataFrame = tabular.from_columns(cn, cs)
    filled: DataFrame = df.fillna_i64(99i64)
    fa: ColumnI64? = filled.get_column_i64(\"a\")
    if fa is not none:
        println(\"nc=\" + str(fa.null_count()))
        v1: i64? = fa.get(1i64)
        if v1 is not none:
            println(\"v1=\" + str(v1))
    return 0
";
    let out = run("fillna_i64", src);
    assert!(out.contains("nc=0"), "got: {out:?}");
    assert!(out.contains("v1=99"), "got: {out:?}");
}

#[test]
fn fillna_f64_basic() {
    let src = "\
from tabular import Column, ColumnF64, DataFrame
import tabular
fn main() -> i32:
    vs: List[f64] = []
    vs.append(1.0)
    vs.append(0.0)
    vs.append(3.0)
    ns: List[bool] = []
    ns.append(false)
    ns.append(true)
    ns.append(false)
    c: ColumnF64 = tabular.col_f64(vs, ns)
    cn: List[str] = []
    cn.append(\"x\")
    cs: List[Column] = []
    cs.append(c)
    df: DataFrame = tabular.from_columns(cn, cs)
    filled: DataFrame = df.fillna_f64(7.5)
    fc: ColumnF64? = filled.get_column_f64(\"x\")
    if fc is not none:
        v1: f64? = fc.get(1i64)
        if v1 is not none:
            println(\"v1=\" + str(v1))
    return 0
";
    let out = run("fillna_f64", src);
    assert!(out.contains("v1=7.5"), "got: {out:?}");
}

#[test]
fn fillna_str_basic() {
    let src = "\
from tabular import Column, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"a\")
    vs.append(\"\")
    vs.append(\"c\")
    ns: List[bool] = []
    ns.append(false)
    ns.append(true)
    ns.append(false)
    c: ColumnStr = tabular.col_str(vs, ns)
    cn: List[str] = []
    cn.append(\"x\")
    cs: List[Column] = []
    cs.append(c)
    df: DataFrame = tabular.from_columns(cn, cs)
    filled: DataFrame = df.fillna_str(\"missing\")
    fc: ColumnStr? = filled.get_column_str(\"x\")
    if fc is not none:
        v1: str? = fc.get(1i64)
        if v1 is not none:
            println(\"v1=\" + v1)
    return 0
";
    let out = run("fillna_str", src);
    assert!(out.contains("v1=missing"), "got: {out:?}");
}

// ── Phase A: iloc ────────────────────────────────────────────────────

#[test]
fn iloc_happy_path() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(0i64)
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    vs.append(4i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"x\")
    cs: List[Column] = []
    cs.append(c)
    df: DataFrame = tabular.from_columns(cn, cs)
    s: DataFrame = df.iloc(1i64, 4i64)
    println(\"len=\" + str(s.length()))
    return 0
";
    let out = run("iloc_basic", src);
    assert!(out.contains("len=3"), "got: {out:?}");
}

#[test]
fn iloc_clamps_at_end() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(0i64)
    vs.append(1i64)
    vs.append(2i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"x\")
    cs: List[Column] = []
    cs.append(c)
    df: DataFrame = tabular.from_columns(cn, cs)
    s: DataFrame = df.iloc(1i64, 999i64)
    println(\"len=\" + str(s.length()))
    return 0
";
    let out = run("iloc_clamp", src);
    assert!(out.contains("len=2"), "got: {out:?}");
}

#[test]
fn iloc_negative_start_raises() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(0i64)
    vs.append(1i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"x\")
    cs: List[Column] = []
    cs.append(c)
    df: DataFrame = tabular.from_columns(cn, cs)
    try:
        s: DataFrame = df.iloc(-1i64, 1i64)
        println(\"no-raise\")
    except ValueError:
        println(\"got-valueerror\")
    return 0
";
    let out = run("iloc_neg", src);
    assert!(out.contains("got-valueerror"), "got: {out:?}");
}
