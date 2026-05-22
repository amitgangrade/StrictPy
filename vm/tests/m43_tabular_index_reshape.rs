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

// ── Phase B: pivot + concat_rows + concat_cols ────────────────────────

#[test]
fn pivot_promotes_index_value_to_index() {
    // df.pivot(index, columns, values): `index` becomes the output's
    // index (index_name = index).  Same data, just promoted.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    rg: List[str] = []
    rg.append(\"us\")
    rg.append(\"us\")
    rg.append(\"uk\")
    rg.append(\"uk\")
    reg: ColumnStr = tabular.col_str_simple(rg)
    mo: List[str] = []
    mo.append(\"jan\")
    mo.append(\"feb\")
    mo.append(\"jan\")
    mo.append(\"feb\")
    month: ColumnStr = tabular.col_str_simple(mo)
    sl: List[i64] = []
    sl.append(10i64)
    sl.append(20i64)
    sl.append(30i64)
    sl.append(40i64)
    sales: ColumnI64 = tabular.col_i64_simple(sl)
    cn: List[str] = []
    cn.append(\"reg\")
    cn.append(\"month\")
    cn.append(\"sales\")
    cols: List[Column] = []
    cols.append(reg)
    cols.append(month)
    cols.append(sales)
    df: DataFrame = tabular.from_columns(cn, cols)
    p: DataFrame = df.pivot(\"reg\", \"month\", \"sales\")
    println(\"nrows=\" + str(p.length()))
    println(\"ncols=\" + str(p.ncols()))
    println(\"has=\" + str(p.has_index()))
    inm: str? = p.index_name()
    if inm is not none:
        println(\"iname=\" + inm)
    return 0
";
    let out = run("pivot_promotes_idx", src);
    assert!(out.contains("nrows=2"), "got: {out:?}");
    assert!(out.contains("ncols=2"), "got: {out:?}");
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("iname=reg"), "got: {out:?}");
}

#[test]
fn concat_rows_happy_path_concatenates_shared_indexes() {
    // Two indexed frames with same dtype + index_name: the output's
    // index is the concatenation of both inputs' indexes.
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    k1: List[i64] = []
    k1.append(10i64)
    k1.append(20i64)
    k1c: ColumnI64 = tabular.col_i64_simple(k1)
    v1: List[i64] = []
    v1.append(1i64)
    v1.append(2i64)
    v1c: ColumnI64 = tabular.col_i64_simple(v1)
    cn1: List[str] = []
    cn1.append(\"k\")
    cn1.append(\"v\")
    c1: List[Column] = []
    c1.append(k1c)
    c1.append(v1c)
    df1: DataFrame = tabular.from_columns(cn1, c1)
    df1i: DataFrame = df1.set_index(\"k\")

    k2: List[i64] = []
    k2.append(30i64)
    k2.append(40i64)
    k2c: ColumnI64 = tabular.col_i64_simple(k2)
    v2: List[i64] = []
    v2.append(3i64)
    v2.append(4i64)
    v2c: ColumnI64 = tabular.col_i64_simple(v2)
    cn2: List[str] = []
    cn2.append(\"k\")
    cn2.append(\"v\")
    c2: List[Column] = []
    c2.append(k2c)
    c2.append(v2c)
    df2: DataFrame = tabular.from_columns(cn2, c2)
    df2i: DataFrame = df2.set_index(\"k\")

    dfs: List[DataFrame] = []
    dfs.append(df1i)
    dfs.append(df2i)
    out: DataFrame = tabular.concat_rows(dfs)
    println(\"nrows=\" + str(out.length()))
    println(\"has=\" + str(out.has_index()))
    inm: str? = out.index_name()
    if inm is not none:
        println(\"iname=\" + inm)
    sorted_out: DataFrame = out.sort_index(true)
    flat: DataFrame = sorted_out.reset_index()
    kc: ColumnI64? = flat.get_column_i64(\"k\")
    if kc is not none:
        k0: i64? = kc.get(0i64)
        k3: i64? = kc.get(3i64)
        if k0 is not none:
            println(\"k0=\" + str(k0))
        if k3 is not none:
            println(\"k3=\" + str(k3))
    return 0
";
    let out = run("concat_rows_happy", src);
    assert!(out.contains("nrows=4"), "got: {out:?}");
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("iname=k"), "got: {out:?}");
    assert!(out.contains("k0=10"), "got: {out:?}");
    assert!(out.contains("k3=40"), "got: {out:?}");
}

#[test]
fn concat_rows_unindexed_input_falls_back_to_range_index() {
    // If any input has no index, the output falls back to RangeIndex.
    // Both frames here share schema (one regular column `v`); df1 is
    // indexed, df2 is not.
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    k1: List[i64] = []
    k1.append(10i64)
    k1c: ColumnI64 = tabular.col_i64_simple(k1)
    v1: List[i64] = []
    v1.append(1i64)
    v1c: ColumnI64 = tabular.col_i64_simple(v1)
    cn1: List[str] = []
    cn1.append(\"k\")
    cn1.append(\"v\")
    c1: List[Column] = []
    c1.append(k1c)
    c1.append(v1c)
    df1: DataFrame = tabular.from_columns(cn1, c1)
    df1i: DataFrame = df1.set_index(\"k\")

    v2: List[i64] = []
    v2.append(2i64)
    v2c: ColumnI64 = tabular.col_i64_simple(v2)
    cn2: List[str] = []
    cn2.append(\"v\")
    c2: List[Column] = []
    c2.append(v2c)
    df2: DataFrame = tabular.from_columns(cn2, c2)

    dfs: List[DataFrame] = []
    dfs.append(df1i)
    dfs.append(df2)
    out: DataFrame = tabular.concat_rows(dfs)
    println(\"has=\" + str(out.has_index()))
    return 0
";
    let out = run("concat_rows_unindexed", src);
    assert!(out.contains("has=false"), "got: {out:?}");
}

#[test]
fn concat_rows_mismatched_index_dtype_falls_back() {
    // Different index dtypes → RangeIndex fallback.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    k1: List[i64] = []
    k1.append(10i64)
    k1c: ColumnI64 = tabular.col_i64_simple(k1)
    cn1: List[str] = []
    cn1.append(\"k\")
    c1: List[Column] = []
    c1.append(k1c)
    df1: DataFrame = tabular.from_columns(cn1, c1)
    df1i: DataFrame = df1.set_index(\"k\")

    k2: List[str] = []
    k2.append(\"a\")
    k2c: ColumnStr = tabular.col_str_simple(k2)
    cn2: List[str] = []
    cn2.append(\"k\")
    c2: List[Column] = []
    c2.append(k2c)
    df2: DataFrame = tabular.from_columns(cn2, c2)
    df2i: DataFrame = df2.set_index(\"k\")

    dfs: List[DataFrame] = []
    dfs.append(df1i)
    dfs.append(df2i)
    out: DataFrame = tabular.concat_rows(dfs)
    println(\"has=\" + str(out.has_index()))
    return 0
";
    let out = run("concat_rows_dtype_mismatch", src);
    // Note: concat_rows requires same column dtypes — but here we set k
    // as the index in both, so the only column is k.  We deliberately
    // wrap each in set_index BEFORE concat to avoid the column-dtype-
    // mismatch check.  Wait — with set_index, both df1i and df2i have
    // ZERO regular columns.  So the column-shape check passes.  But
    // the index dtypes differ → fallback.
    assert!(out.contains("has=false"), "got: {out:?}");
}

#[test]
fn concat_rows_mismatched_index_name_falls_back() {
    // Different index_names → RangeIndex fallback.
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    k1: List[i64] = []
    k1.append(10i64)
    k1c: ColumnI64 = tabular.col_i64_simple(k1)
    cn1: List[str] = []
    cn1.append(\"a\")
    c1: List[Column] = []
    c1.append(k1c)
    df1: DataFrame = tabular.from_columns(cn1, c1)
    df1i: DataFrame = df1.set_index(\"a\")

    k2: List[i64] = []
    k2.append(20i64)
    k2c: ColumnI64 = tabular.col_i64_simple(k2)
    cn2: List[str] = []
    cn2.append(\"b\")
    c2: List[Column] = []
    c2.append(k2c)
    df2: DataFrame = tabular.from_columns(cn2, c2)
    df2i: DataFrame = df2.set_index(\"b\")

    dfs: List[DataFrame] = []
    dfs.append(df1i)
    dfs.append(df2i)
    out: DataFrame = tabular.concat_rows(dfs)
    println(\"has=\" + str(out.has_index()))
    return 0
";
    let out = run("concat_rows_name_mismatch", src);
    assert!(out.contains("has=false"), "got: {out:?}");
}

#[test]
fn concat_cols_lhs_index_wins() {
    // If lhs has an index, output gets that index; rhs's index is
    // ignored (consistent with M42's merge lhs-wins policy).
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    k1: List[i64] = []
    k1.append(10i64)
    k1.append(20i64)
    k1c: ColumnI64 = tabular.col_i64_simple(k1)
    a1: List[i64] = []
    a1.append(1i64)
    a1.append(2i64)
    a1c: ColumnI64 = tabular.col_i64_simple(a1)
    cn1: List[str] = []
    cn1.append(\"k\")
    cn1.append(\"a\")
    c1: List[Column] = []
    c1.append(k1c)
    c1.append(a1c)
    df1: DataFrame = tabular.from_columns(cn1, c1)
    df1i: DataFrame = df1.set_index(\"k\")

    b: List[i64] = []
    b.append(3i64)
    b.append(4i64)
    bc: ColumnI64 = tabular.col_i64_simple(b)
    cn2: List[str] = []
    cn2.append(\"b\")
    c2: List[Column] = []
    c2.append(bc)
    df2: DataFrame = tabular.from_columns(cn2, c2)

    dfs: List[DataFrame] = []
    dfs.append(df1i)
    dfs.append(df2)
    out: DataFrame = tabular.concat_cols(dfs)
    println(\"nrows=\" + str(out.length()))
    println(\"ncols=\" + str(out.ncols()))
    println(\"has=\" + str(out.has_index()))
    inm: str? = out.index_name()
    if inm is not none:
        println(\"iname=\" + inm)
    return 0
";
    let out = run("concat_cols_lhs", src);
    assert!(out.contains("nrows=2"), "got: {out:?}");
    assert!(out.contains("ncols=2"), "got: {out:?}"); // a + b
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("iname=k"), "got: {out:?}");
}

#[test]
fn concat_cols_unindexed_lhs_falls_back_to_range_index() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    a: List[i64] = []
    a.append(1i64)
    a.append(2i64)
    ac: ColumnI64 = tabular.col_i64_simple(a)
    cn1: List[str] = []
    cn1.append(\"a\")
    c1: List[Column] = []
    c1.append(ac)
    df1: DataFrame = tabular.from_columns(cn1, c1)

    b: List[i64] = []
    b.append(3i64)
    b.append(4i64)
    bc: ColumnI64 = tabular.col_i64_simple(b)
    cn2: List[str] = []
    cn2.append(\"b\")
    c2: List[Column] = []
    c2.append(bc)
    df2: DataFrame = tabular.from_columns(cn2, c2)
    df2i: DataFrame = df2.set_index(\"b\")

    dfs: List[DataFrame] = []
    dfs.append(df1)
    dfs.append(df2i)
    out: DataFrame = tabular.concat_cols(dfs)
    println(\"has=\" + str(out.has_index()))
    return 0
";
    let out = run("concat_cols_unindexed_lhs", src);
    // lhs has no index, so the output uses RangeIndex even though rhs
    // has an index.
    assert!(out.contains("has=false"), "got: {out:?}");
}

// ── Phase C: melt index repetition ───────────────────────────────────

#[test]
fn melt_repeats_input_index_per_value_var() {
    // Input has 3 rows with index labels [100, 200, 300].  2 value_vars
    // → output has 6 rows with index [100,100,200,200,300,300].
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    ids: List[i64] = []
    ids.append(100i64)
    ids.append(200i64)
    ids.append(300i64)
    id: ColumnI64 = tabular.col_i64_simple(ids)
    av: List[i64] = []
    av.append(1i64)
    av.append(2i64)
    av.append(3i64)
    a: ColumnI64 = tabular.col_i64_simple(av)
    bv: List[i64] = []
    bv.append(10i64)
    bv.append(20i64)
    bv.append(30i64)
    b: ColumnI64 = tabular.col_i64_simple(bv)
    cn: List[str] = []
    cn.append(\"id\")
    cn.append(\"a\")
    cn.append(\"b\")
    cols: List[Column] = []
    cols.append(id)
    cols.append(a)
    cols.append(b)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"id\")
    id_vars: List[str] = []
    value_vars: List[str] = []
    value_vars.append(\"a\")
    value_vars.append(\"b\")
    m: DataFrame = df2.melt(id_vars, value_vars)
    println(\"nrows=\" + str(m.length()))
    println(\"has=\" + str(m.has_index()))
    inm: str? = m.index_name()
    if inm is not none:
        println(\"iname=\" + inm)
    flat: DataFrame = m.reset_index()
    ic: ColumnI64? = flat.get_column_i64(\"id\")
    if ic is not none:
        i0: i64? = ic.get(0i64)
        i1: i64? = ic.get(1i64)
        i2: i64? = ic.get(2i64)
        i3: i64? = ic.get(3i64)
        if i0 is not none:
            println(\"i0=\" + str(i0))
        if i1 is not none:
            println(\"i1=\" + str(i1))
        if i2 is not none:
            println(\"i2=\" + str(i2))
        if i3 is not none:
            println(\"i3=\" + str(i3))
    return 0
";
    let out = run("melt_index_repeats", src);
    assert!(out.contains("nrows=6"), "got: {out:?}");
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("iname=id"), "got: {out:?}");
    // Each input row index repeated twice (one per value_var):
    // 100,100,200,200,300,300
    assert!(out.contains("i0=100"), "got: {out:?}");
    assert!(out.contains("i1=100"), "got: {out:?}");
    assert!(out.contains("i2=200"), "got: {out:?}");
    assert!(out.contains("i3=200"), "got: {out:?}");
}

#[test]
fn melt_without_index_produces_range_index() {
    // Input has no index → output uses RangeIndex (today's behavior).
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    av: List[i64] = []
    av.append(1i64)
    av.append(2i64)
    a: ColumnI64 = tabular.col_i64_simple(av)
    bv: List[i64] = []
    bv.append(10i64)
    bv.append(20i64)
    b: ColumnI64 = tabular.col_i64_simple(bv)
    cn: List[str] = []
    cn.append(\"a\")
    cn.append(\"b\")
    cols: List[Column] = []
    cols.append(a)
    cols.append(b)
    df: DataFrame = tabular.from_columns(cn, cols)
    id_vars: List[str] = []
    value_vars: List[str] = []
    value_vars.append(\"a\")
    value_vars.append(\"b\")
    m: DataFrame = df.melt(id_vars, value_vars)
    println(\"has=\" + str(m.has_index()))
    return 0
";
    let out = run("melt_no_index", src);
    assert!(out.contains("has=false"), "got: {out:?}");
}

#[test]
fn melt_preserves_index_dtype_str() {
    // String-typed index repeats per value_var.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    ids: List[str] = []
    ids.append(\"x\")
    ids.append(\"y\")
    id: ColumnStr = tabular.col_str_simple(ids)
    av: List[i64] = []
    av.append(1i64)
    av.append(2i64)
    a: ColumnI64 = tabular.col_i64_simple(av)
    bv: List[i64] = []
    bv.append(10i64)
    bv.append(20i64)
    b: ColumnI64 = tabular.col_i64_simple(bv)
    cn: List[str] = []
    cn.append(\"id\")
    cn.append(\"a\")
    cn.append(\"b\")
    cols: List[Column] = []
    cols.append(id)
    cols.append(a)
    cols.append(b)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"id\")
    id_vars: List[str] = []
    value_vars: List[str] = []
    value_vars.append(\"a\")
    value_vars.append(\"b\")
    m: DataFrame = df2.melt(id_vars, value_vars)
    println(\"nrows=\" + str(m.length()))
    flat: DataFrame = m.reset_index()
    ic: ColumnStr? = flat.get_column_str(\"id\")
    if ic is not none:
        i0: str? = ic.get(0i64)
        i2: str? = ic.get(2i64)
        if i0 is not none:
            println(\"i0=\" + i0)
        if i2 is not none:
            println(\"i2=\" + i2)
    return 0
";
    let out = run("melt_index_str", src);
    assert!(out.contains("nrows=4"), "got: {out:?}");
    assert!(out.contains("i0=x"), "got: {out:?}");
    assert!(out.contains("i2=y"), "got: {out:?}");
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
