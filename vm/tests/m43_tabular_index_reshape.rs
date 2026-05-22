//! M43 in-process regression tests for `tabular` index propagation
//! through reshape + group_by + pivot_table.
//!
//! M41 added the optional index; M42 propagated it through 11 row/
//! column-transforming methods.  M43 finishes the story by making the
//! remaining reshape ops index-aware: pivot_table, single-column
//! group_by + agg, pivot, melt, concat_rows, concat_cols.
//!
//! Multi-column group_by retains today's "keys-as-regular-columns"
//! shape (MultiIndex is M44+).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m43_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

fn run(name: &str, src: &str) -> String {
    let p = compile_snippet(name, src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "{name}: exit={code} stdout={out:?}");
    out
}

// ── Phase A: pivot_table + single-column group_by index promotion ─────

#[test]
fn pivot_table_promotes_index_col_to_index() {
    // The `index_col` argument's unique values become the output
    // DataFrame's index instead of being inserted as the first regular
    // column.  `index_name = index_col`.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    syms: List[str] = []
    syms.append(\"a\")
    syms.append(\"b\")
    syms.append(\"a\")
    syms.append(\"b\")
    sym: ColumnStr = tabular.col_str_simple(syms)
    sides: List[str] = []
    sides.append(\"buy\")
    sides.append(\"buy\")
    sides.append(\"sell\")
    sides.append(\"sell\")
    side: ColumnStr = tabular.col_str_simple(sides)
    qty_vs: List[i64] = []
    qty_vs.append(10i64)
    qty_vs.append(20i64)
    qty_vs.append(30i64)
    qty_vs.append(40i64)
    qty: ColumnI64 = tabular.col_i64_simple(qty_vs)
    cn: List[str] = []
    cn.append(\"sym\")
    cn.append(\"side\")
    cn.append(\"qty\")
    cols: List[Column] = []
    cols.append(sym)
    cols.append(side)
    cols.append(qty)
    df: DataFrame = tabular.from_columns(cn, cols)
    pt: DataFrame = df.pivot_table(\"sym\", \"side\", \"qty\", \"sum\")
    println(\"nrows=\" + str(pt.length()))
    println(\"ncols=\" + str(pt.ncols()))
    println(\"has=\" + str(pt.has_index()))
    inm: str? = pt.index_name()
    if inm is not none:
        println(\"iname=\" + inm)
    return 0
";
    let out = run("pivot_table_index", src);
    assert!(out.contains("nrows=2"), "got: {out:?}");
    assert!(out.contains("ncols=2"), "got: {out:?}");
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("iname=sym"), "got: {out:?}");
}

#[test]
fn pivot_table_mean_index_dtype_preserved() {
    // index_col is ColumnI64; pivot_table mean should preserve that
    // dtype on the index slot AND emit ColumnF64 value columns.
    let src = "\
from tabular import Column, ColumnI64, ColumnF64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    rs: List[i64] = []
    rs.append(1i64)
    rs.append(1i64)
    rs.append(2i64)
    rs.append(2i64)
    r: ColumnI64 = tabular.col_i64_simple(rs)
    cs: List[str] = []
    cs.append(\"x\")
    cs.append(\"x\")
    cs.append(\"x\")
    cs.append(\"x\")
    c: ColumnStr = tabular.col_str_simple(cs)
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(20i64)
    vs.append(30i64)
    vs.append(50i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"r\")
    cn.append(\"c\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(r)
    cols.append(c)
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    pt: DataFrame = df.pivot_table(\"r\", \"c\", \"v\", \"mean\")
    println(\"has=\" + str(pt.has_index()))
    inm: str? = pt.index_name()
    if inm is not none:
        println(\"iname=\" + inm)
    idx: Column? = pt.index()
    if idx is not none:
        println(\"hasidx=1\")
    xc: ColumnF64? = pt.get_column_f64(\"x\")
    if xc is not none:
        x0: f64? = xc.get(0i64)
        x1: f64? = xc.get(1i64)
        if x0 is not none:
            println(\"a_mean=\" + str(x0))
        if x1 is not none:
            println(\"b_mean=\" + str(x1))
    return 0
";
    let out = run("pivot_table_mean_idx", src);
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("iname=r"), "got: {out:?}");
    assert!(out.contains("a_mean=15"), "got: {out:?}");
    assert!(out.contains("b_mean=40"), "got: {out:?}");
}

#[test]
fn single_col_group_by_sum_promotes_key_to_index() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, GroupedDataFrame, DataFrame
import tabular
fn main() -> i32:
    cats: List[str] = []
    cats.append(\"a\")
    cats.append(\"b\")
    cats.append(\"a\")
    cats.append(\"b\")
    cats.append(\"a\")
    cat: ColumnStr = tabular.col_str_simple(cats)
    qty_vs: List[i64] = []
    qty_vs.append(10i64)
    qty_vs.append(20i64)
    qty_vs.append(30i64)
    qty_vs.append(40i64)
    qty_vs.append(50i64)
    qty: ColumnI64 = tabular.col_i64_simple(qty_vs)
    cn: List[str] = []
    cn.append(\"cat\")
    cn.append(\"qty\")
    cols: List[Column] = []
    cols.append(cat)
    cols.append(qty)
    df: DataFrame = tabular.from_columns(cn, cols)
    keys: List[str] = []
    keys.append(\"cat\")
    gdf: GroupedDataFrame = df.group_by(keys)
    s: DataFrame = gdf.sum()
    println(\"nrows=\" + str(s.length()))
    println(\"ncols=\" + str(s.ncols()))
    println(\"has=\" + str(s.has_index()))
    inm: str? = s.index_name()
    if inm is not none:
        println(\"iname=\" + inm)
    qc: ColumnI64? = s.get_column_i64(\"qty\")
    if qc is not none:
        q0: i64? = qc.get(0i64)
        q1: i64? = qc.get(1i64)
        if q0 is not none:
            println(\"a_sum=\" + str(q0))
        if q1 is not none:
            println(\"b_sum=\" + str(q1))
    return 0
";
    let out = run("gby_sum_promotes", src);
    assert!(out.contains("nrows=2"), "got: {out:?}");
    assert!(out.contains("ncols=1"), "got: {out:?}"); // only `qty` (cat is now index)
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("iname=cat"), "got: {out:?}");
    // a: 10+30+50=90; b: 20+40=60
    assert!(out.contains("a_sum=90"), "got: {out:?}");
    assert!(out.contains("b_sum=60"), "got: {out:?}");
}

#[test]
fn single_col_group_by_mean_promotes_and_emits_f64() {
    let src = "\
from tabular import Column, ColumnI64, ColumnF64, ColumnStr, GroupedDataFrame, DataFrame
import tabular
fn main() -> i32:
    cats: List[str] = []
    cats.append(\"a\")
    cats.append(\"b\")
    cats.append(\"a\")
    cat: ColumnStr = tabular.col_str_simple(cats)
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(20i64)
    vs.append(30i64)
    qty: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"cat\")
    cn.append(\"qty\")
    cols: List[Column] = []
    cols.append(cat)
    cols.append(qty)
    df: DataFrame = tabular.from_columns(cn, cols)
    keys: List[str] = []
    keys.append(\"cat\")
    gdf: GroupedDataFrame = df.group_by(keys)
    m: DataFrame = gdf.mean()
    println(\"has=\" + str(m.has_index()))
    qc: ColumnF64? = m.get_column_f64(\"qty\")
    if qc is not none:
        q0: f64? = qc.get(0i64)
        if q0 is not none:
            println(\"a_mean=\" + str(q0))
    return 0
";
    let out = run("gby_mean_promotes", src);
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("a_mean=20"), "got: {out:?}");
}

#[test]
fn single_col_group_by_agg_specs_promotes() {
    let src = "\
from tabular import Column, ColumnI64, ColumnF64, ColumnStr, GroupedDataFrame, DataFrame
import tabular
fn main() -> i32:
    cats: List[str] = []
    cats.append(\"a\")
    cats.append(\"a\")
    cats.append(\"b\")
    cat: ColumnStr = tabular.col_str_simple(cats)
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(20i64)
    vs.append(30i64)
    qty: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"cat\")
    cn.append(\"qty\")
    cols: List[Column] = []
    cols.append(cat)
    cols.append(qty)
    df: DataFrame = tabular.from_columns(cn, cols)
    keys: List[str] = []
    keys.append(\"cat\")
    gdf: GroupedDataFrame = df.group_by(keys)
    specs: List[Tuple[str, str]] = []
    specs.append((\"qty\", \"sum\"))
    specs.append((\"qty\", \"mean\"))
    g: DataFrame = gdf.agg(specs)
    println(\"ncols=\" + str(g.ncols()))
    println(\"has=\" + str(g.has_index()))
    inm: str? = g.index_name()
    if inm is not none:
        println(\"iname=\" + inm)
    return 0
";
    let out = run("gby_agg_promotes", src);
    assert!(out.contains("ncols=2"), "got: {out:?}"); // qty_sum + qty_mean
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("iname=cat"), "got: {out:?}");
}

#[test]
fn multi_col_group_by_does_not_promote_to_index() {
    // M43 contract: multi-column group_by KEEPS the keys as regular
    // columns and uses RangeIndex.  MultiIndex is M44+.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, GroupedDataFrame, DataFrame
import tabular
fn main() -> i32:
    cats: List[str] = []
    cats.append(\"a\")
    cats.append(\"a\")
    cats.append(\"b\")
    cat: ColumnStr = tabular.col_str_simple(cats)
    regs: List[str] = []
    regs.append(\"east\")
    regs.append(\"west\")
    regs.append(\"east\")
    reg: ColumnStr = tabular.col_str_simple(regs)
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(20i64)
    vs.append(30i64)
    qty: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"cat\")
    cn.append(\"reg\")
    cn.append(\"qty\")
    cols: List[Column] = []
    cols.append(cat)
    cols.append(reg)
    cols.append(qty)
    df: DataFrame = tabular.from_columns(cn, cols)
    keys: List[str] = []
    keys.append(\"cat\")
    keys.append(\"reg\")
    gdf: GroupedDataFrame = df.group_by(keys)
    s: DataFrame = gdf.sum()
    println(\"ncols=\" + str(s.ncols()))
    println(\"has=\" + str(s.has_index()))
    return 0
";
    let out = run("gby_multi_no_promote", src);
    // 3 cols: cat + reg + qty.  Multi-col group_by retains v1 shape.
    assert!(out.contains("ncols=3"), "got: {out:?}");
    assert!(out.contains("has=false"), "got: {out:?}");
}

#[test]
fn single_col_group_by_keys_returns_zero_regular_columns() {
    // `gdf.keys()` for a single-column key returns a 0-regular-col
    // DataFrame whose index IS the unique key values.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, GroupedDataFrame, DataFrame
import tabular
fn main() -> i32:
    cats: List[str] = []
    cats.append(\"a\")
    cats.append(\"b\")
    cats.append(\"a\")
    cat: ColumnStr = tabular.col_str_simple(cats)
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    qty: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"cat\")
    cn.append(\"qty\")
    cols: List[Column] = []
    cols.append(cat)
    cols.append(qty)
    df: DataFrame = tabular.from_columns(cn, cols)
    keys: List[str] = []
    keys.append(\"cat\")
    gdf: GroupedDataFrame = df.group_by(keys)
    k: DataFrame = gdf.keys()
    println(\"nrows=\" + str(k.length()))
    println(\"ncols=\" + str(k.ncols()))
    println(\"has=\" + str(k.has_index()))
    inm: str? = k.index_name()
    if inm is not none:
        println(\"iname=\" + inm)
    return 0
";
    let out = run("gby_keys_single", src);
    assert!(out.contains("nrows=2"), "got: {out:?}");
    assert!(out.contains("ncols=0"), "got: {out:?}");
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("iname=cat"), "got: {out:?}");
}

#[test]
fn single_col_group_by_size_promotes_to_index() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, GroupedDataFrame, DataFrame
import tabular
fn main() -> i32:
    cats: List[str] = []
    cats.append(\"a\")
    cats.append(\"b\")
    cats.append(\"a\")
    cats.append(\"a\")
    cat: ColumnStr = tabular.col_str_simple(cats)
    cn: List[str] = []
    cn.append(\"cat\")
    cols: List[Column] = []
    cols.append(cat)
    df: DataFrame = tabular.from_columns(cn, cols)
    keys: List[str] = []
    keys.append(\"cat\")
    gdf: GroupedDataFrame = df.group_by(keys)
    sz: DataFrame = gdf.size()
    println(\"ncols=\" + str(sz.ncols()))
    println(\"has=\" + str(sz.has_index()))
    inm: str? = sz.index_name()
    if inm is not none:
        println(\"iname=\" + inm)
    szc: ColumnI64? = sz.get_column_i64(\"size\")
    if szc is not none:
        s0: i64? = szc.get(0i64)
        s1: i64? = szc.get(1i64)
        if s0 is not none:
            println(\"a_sz=\" + str(s0))
        if s1 is not none:
            println(\"b_sz=\" + str(s1))
    return 0
";
    let out = run("gby_size_single", src);
    assert!(out.contains("ncols=1"), "got: {out:?}"); // size only
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("iname=cat"), "got: {out:?}");
    assert!(out.contains("a_sz=3"), "got: {out:?}");
    assert!(out.contains("b_sz=1"), "got: {out:?}");
}
