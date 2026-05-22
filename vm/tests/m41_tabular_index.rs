//! M41 in-process regression tests for the `tabular` DatetimeIndex
//! (minimum viable) + pivot_table surface.
//!
//! Phase 5b of the Pandas-shaped data package.  Surface documented in
//! LANGUAGE_GUIDE.md §5 (post-M41).
//!
//! EXPLICIT SCOPE-DOWN: every existing DataFrame method that returns a
//! fresh frame still drops the index in v1 — only sort_index,
//! resample_index, asof_merge_index, and select_by_label_* preserve it.
//! Full index propagation through filter/sort/head/etc. is M42 work.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m41_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

fn run(name: &str, src: &str) -> String {
    let p = compile_snippet(name, src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "{name}: exit={code} stdout={out:?}");
    out
}

/// Wrap the candidate body in a try/except so we get a non-panicking
/// nonzero-exit path that prints a sentinel on caught ValueError.  The
/// caller asserts on the sentinel rather than on exit code, matching
/// the M40 convention.
fn run_value_error(name: &str, src: &str) -> String {
    run(name, src)
}

// ── Phase A: index storage + accessors + sort_index ────────────────────

#[test]
fn set_index_round_trip() {
    // set_index then reset_index yields a frame equivalent to the
    // original (modulo the index column ending up at position 0).
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    keys: List[str] = []
    keys.append(\"a\")
    keys.append(\"b\")
    keys.append(\"c\")
    k: ColumnStr = tabular.col_str_simple(keys)
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(20i64)
    vs.append(30i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    println(\"has=\" + str(df.has_index()))
    df2: DataFrame = df.set_index(\"k\")
    println(\"has2=\" + str(df2.has_index()))
    nm: str? = df2.index_name()
    if nm is not none:
        println(\"name=\" + nm)
    println(\"ncols2=\" + str(df2.ncols()))
    df3: DataFrame = df2.reset_index()
    println(\"has3=\" + str(df3.has_index()))
    println(\"ncols3=\" + str(df3.ncols()))
    return 0
";
    let out = run("set_index_round_trip", src);
    assert!(out.contains("has=false"), "got: {out:?}");
    assert!(out.contains("has2=true"), "got: {out:?}");
    assert!(out.contains("name=k"), "got: {out:?}");
    // After set_index, ncols drops by one (k is now the index).
    assert!(out.contains("ncols2=1"), "got: {out:?}");
    assert!(out.contains("has3=false"), "got: {out:?}");
    // After reset_index, k is back as a regular column at position 0.
    assert!(out.contains("ncols3=2"), "got: {out:?}");
}

#[test]
fn set_index_missing_column_raises() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    try:
        df2: DataFrame = df.set_index(\"absent\")
        println(\"no-raise\")
    except ValueError:
        println(\"got-valueerror\")
    return 0
";
    let out = run_value_error("set_index_missing", src);
    assert!(out.contains("got-valueerror"), "got: {out:?}");
}

#[test]
fn set_index_twice_raises() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    keys: List[str] = []
    keys.append(\"a\")
    k: ColumnStr = tabular.col_str_simple(keys)
    vs: List[i64] = []
    vs.append(1i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    try:
        df3: DataFrame = df2.set_index(\"v\")
        println(\"no-raise\")
    except ValueError:
        println(\"got-valueerror\")
    return 0
";
    let out = run_value_error("set_index_twice", src);
    assert!(out.contains("got-valueerror"), "got: {out:?}");
}

#[test]
fn reset_index_no_op_on_range_index() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(7i64)
    vs.append(8i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.reset_index()
    println(\"has=\" + str(df2.has_index()))
    println(\"nrows=\" + str(df2.length()))
    println(\"ncols=\" + str(df2.ncols()))
    return 0
";
    let out = run("reset_index_no_op", src);
    assert!(out.contains("has=false"), "got: {out:?}");
    assert!(out.contains("nrows=2"), "got: {out:?}");
    assert!(out.contains("ncols=1"), "got: {out:?}");
}

#[test]
fn index_accessor_returns_none_when_no_index() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    ic: Column? = df.index()
    if ic is none:
        println(\"index=none\")
    nm: str? = df.index_name()
    if nm is none:
        println(\"name=none\")
    return 0
";
    let out = run("index_none", src);
    assert!(out.contains("index=none"), "got: {out:?}");
    assert!(out.contains("name=none"), "got: {out:?}");
}

#[test]
fn sort_index_ascending_str() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    keys: List[str] = []
    keys.append(\"c\")
    keys.append(\"a\")
    keys.append(\"b\")
    k: ColumnStr = tabular.col_str_simple(keys)
    vs: List[i64] = []
    vs.append(30i64)
    vs.append(10i64)
    vs.append(20i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    df3: DataFrame = df2.sort_index(true)
    vc: ColumnI64? = df3.get_column_i64(\"v\")
    if vc is not none:
        v0: i64? = vc.get(0i64)
        v2: i64? = vc.get(2i64)
        if v0 is not none:
            println(\"v0=\" + str(v0))
        if v2 is not none:
            println(\"v2=\" + str(v2))
    println(\"has=\" + str(df3.has_index()))
    return 0
";
    let out = run("sort_index_asc", src);
    // Sorted by key ascending: a(10), b(20), c(30).
    assert!(out.contains("v0=10"), "got: {out:?}");
    assert!(out.contains("v2=30"), "got: {out:?}");
    // sort_index preserves the index.
    assert!(out.contains("has=true"), "got: {out:?}");
}

#[test]
fn sort_index_descending_i64() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    keys: List[i64] = []
    keys.append(2i64)
    keys.append(1i64)
    keys.append(3i64)
    k: ColumnI64 = tabular.col_i64_simple(keys)
    vs: List[i64] = []
    vs.append(20i64)
    vs.append(10i64)
    vs.append(30i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    df3: DataFrame = df2.sort_index(false)
    vc: ColumnI64? = df3.get_column_i64(\"v\")
    if vc is not none:
        v0: i64? = vc.get(0i64)
        v2: i64? = vc.get(2i64)
        if v0 is not none:
            println(\"v0=\" + str(v0))
        if v2 is not none:
            println(\"v2=\" + str(v2))
    return 0
";
    let out = run("sort_index_desc", src);
    // Descending: 3(30), 2(20), 1(10).
    assert!(out.contains("v0=30"), "got: {out:?}");
    assert!(out.contains("v2=10"), "got: {out:?}");
}

#[test]
fn sort_index_no_index_raises() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    try:
        df2: DataFrame = df.sort_index(true)
        println(\"no-raise\")
    except ValueError:
        println(\"got-valueerror\")
    return 0
";
    let out = run_value_error("sort_index_no_index", src);
    assert!(out.contains("got-valueerror"), "got: {out:?}");
}

// ── Phase B: index-aware time-series + select by label ────────────────

#[test]
fn resample_index_happy_path() {
    let src = "\
from tabular import Column, ColumnI64, ColumnDateTime, DataFrame
import tabular
fn main() -> i32:
    day: i64 = 86400000i64
    ts_vs: List[i64] = []
    ts_vs.append(0i64)
    ts_vs.append(day)
    ts_vs.append(2i64 * day)
    ts_vs.append(3i64 * day)
    ts_vs.append(4i64 * day)
    ts_ns: List[bool] = []
    ts_ns.append(false)
    ts_ns.append(false)
    ts_ns.append(false)
    ts_ns.append(false)
    ts_ns.append(false)
    ts: ColumnDateTime = tabular.col_datetime(ts_vs, ts_ns)
    amt_vs: List[i64] = []
    amt_vs.append(10i64)
    amt_vs.append(20i64)
    amt_vs.append(30i64)
    amt_vs.append(40i64)
    amt_vs.append(50i64)
    amt: ColumnI64 = tabular.col_i64_simple(amt_vs)
    cn: List[str] = []
    cn.append(\"ts\")
    cn.append(\"amt\")
    cols: List[Column] = []
    cols.append(ts)
    cols.append(amt)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"ts\")
    daily: DataFrame = df2.resample_index(\"1d\", \"sum\")
    println(\"nrows=\" + str(daily.length()))
    println(\"has=\" + str(daily.has_index()))
    ac: ColumnI64? = daily.get_column_i64(\"amt\")
    if ac is not none:
        d0: i64? = ac.get(0i64)
        d4: i64? = ac.get(4i64)
        if d0 is not none:
            println(\"d0=\" + str(d0))
        if d4 is not none:
            println(\"d4=\" + str(d4))
    return 0
";
    let out = run("resample_index_happy", src);
    assert!(out.contains("nrows=5"), "got: {out:?}");
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("d0=10"), "got: {out:?}");
    assert!(out.contains("d4=50"), "got: {out:?}");
}

#[test]
fn resample_index_no_index_raises() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    try:
        df2: DataFrame = df.resample_index(\"1d\", \"sum\")
        println(\"no-raise\")
    except ValueError:
        println(\"got-valueerror\")
    return 0
";
    let out = run_value_error("resample_index_no_index", src);
    assert!(out.contains("got-valueerror"), "got: {out:?}");
}

#[test]
fn resample_index_non_datetime_raises() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    keys: List[i64] = []
    keys.append(1i64)
    k: ColumnI64 = tabular.col_i64_simple(keys)
    vs: List[i64] = []
    vs.append(10i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    try:
        df3: DataFrame = df2.resample_index(\"1d\", \"sum\")
        println(\"no-raise\")
    except ValueError:
        println(\"got-valueerror\")
    return 0
";
    let out = run_value_error("resample_index_wrong_dtype", src);
    assert!(out.contains("got-valueerror"), "got: {out:?}");
}

#[test]
fn asof_merge_index_happy_path() {
    let src = "\
from tabular import Column, ColumnI64, ColumnDateTime, ColumnF64, DataFrame
import tabular
fn main() -> i32:
    day: i64 = 86400000i64
    ts_vs: List[i64] = []
    ts_vs.append(0i64)
    ts_vs.append(2i64 * day)
    ts_vs.append(5i64 * day)
    ts: ColumnDateTime = tabular.col_datetime(ts_vs, [false, false, false])
    amt_vs: List[i64] = []
    amt_vs.append(10i64)
    amt_vs.append(20i64)
    amt_vs.append(30i64)
    amt: ColumnI64 = tabular.col_i64_simple(amt_vs)
    cn: List[str] = []
    cn.append(\"ts\")
    cn.append(\"amt\")
    cols: List[Column] = []
    cols.append(ts)
    cols.append(amt)
    events: DataFrame = tabular.from_columns(cn, cols)
    events2: DataFrame = events.set_index(\"ts\")
    rts_vs: List[i64] = []
    rts_vs.append(0i64)
    rts_vs.append(3i64 * day)
    rts: ColumnDateTime = tabular.col_datetime(rts_vs, [false, false])
    rate_vs: List[f64] = []
    rate_vs.append(0.05)
    rate_vs.append(0.07)
    rate: ColumnF64 = tabular.col_f64_simple(rate_vs)
    rcn: List[str] = []
    rcn.append(\"ts\")
    rcn.append(\"rate\")
    rcols: List[Column] = []
    rcols.append(rts)
    rcols.append(rate)
    rates: DataFrame = tabular.from_columns(rcn, rcols)
    rates2: DataFrame = rates.set_index(\"ts\")
    merged: DataFrame = events2.asof_merge_index(rates2)
    println(\"nrows=\" + str(merged.length()))
    println(\"has=\" + str(merged.has_index()))
    rc: ColumnF64? = merged.get_column_f64(\"rate\")
    if rc is not none:
        r0: f64? = rc.get(0i64)
        r2: f64? = rc.get(2i64)
        if r0 is not none:
            println(\"r0=\" + str(r0))
        if r2 is not none:
            println(\"r2=\" + str(r2))
    return 0
";
    let out = run("asof_merge_index_happy", src);
    assert!(out.contains("nrows=3"), "got: {out:?}");
    assert!(out.contains("has=true"), "got: {out:?}");
    // event[0].ts=0 matches rates[0] (0, 0.05)
    // event[2].ts=5d matches rates[1] (3d, 0.07)
    assert!(out.contains("r0=0.05"), "got: {out:?}");
    assert!(out.contains("r2=0.07"), "got: {out:?}");
}

#[test]
fn asof_merge_index_missing_index_raises() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(v)
    left: DataFrame = tabular.from_columns(cn, cols)
    right: DataFrame = tabular.from_columns(cn, cols)
    try:
        merged: DataFrame = left.asof_merge_index(right)
        println(\"no-raise\")
    except ValueError:
        println(\"got-valueerror\")
    return 0
";
    let out = run_value_error("asof_merge_index_no_index", src);
    assert!(out.contains("got-valueerror"), "got: {out:?}");
}

#[test]
fn select_by_label_str_hit_and_miss() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    keys: List[str] = []
    keys.append(\"a\")
    keys.append(\"b\")
    keys.append(\"c\")
    k: ColumnStr = tabular.col_str_simple(keys)
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(20i64)
    vs.append(30i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    hit: DataFrame? = df2.select_by_label_str(\"b\")
    if hit is not none:
        println(\"hit_nrows=\" + str(hit.length()))
        hv: ColumnI64? = hit.get_column_i64(\"v\")
        if hv is not none:
            h0: i64? = hv.get(0i64)
            if h0 is not none:
                println(\"hit_v=\" + str(h0))
    miss: DataFrame? = df2.select_by_label_str(\"z\")
    if miss is none:
        println(\"miss=none\")
    return 0
";
    let out = run("select_by_label_str", src);
    assert!(out.contains("hit_nrows=1"), "got: {out:?}");
    assert!(out.contains("hit_v=20"), "got: {out:?}");
    assert!(out.contains("miss=none"), "got: {out:?}");
}

#[test]
fn select_by_label_i64_hit() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    keys: List[i64] = []
    keys.append(100i64)
    keys.append(200i64)
    keys.append(300i64)
    k: ColumnI64 = tabular.col_i64_simple(keys)
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(20i64)
    vs.append(30i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    hit: DataFrame? = df2.select_by_label_i64(200i64)
    if hit is not none:
        hv: ColumnI64? = hit.get_column_i64(\"v\")
        if hv is not none:
            h0: i64? = hv.get(0i64)
            if h0 is not none:
                println(\"hit_v=\" + str(h0))
    miss: DataFrame? = df2.select_by_label_i64(999i64)
    if miss is none:
        println(\"miss=none\")
    return 0
";
    let out = run("select_by_label_i64", src);
    assert!(out.contains("hit_v=20"), "got: {out:?}");
    assert!(out.contains("miss=none"), "got: {out:?}");
}

#[test]
fn select_by_label_datetime_hit() {
    let src = "\
from tabular import Column, ColumnI64, ColumnDateTime, DataFrame
import tabular
fn main() -> i32:
    day: i64 = 86400000i64
    ts_vs: List[i64] = []
    ts_vs.append(0i64)
    ts_vs.append(day)
    ts_vs.append(2i64 * day)
    ts: ColumnDateTime = tabular.col_datetime(ts_vs, [false, false, false])
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(20i64)
    vs.append(30i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"ts\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(ts)
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"ts\")
    hit: DataFrame? = df2.select_by_label_datetime(day)
    if hit is not none:
        hv: ColumnI64? = hit.get_column_i64(\"v\")
        if hv is not none:
            h0: i64? = hv.get(0i64)
            if h0 is not none:
                println(\"hit_v=\" + str(h0))
    return 0
";
    let out = run("select_by_label_datetime", src);
    assert!(out.contains("hit_v=20"), "got: {out:?}");
}

#[test]
fn select_by_label_wrong_dtype_raises() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    keys: List[str] = []
    keys.append(\"a\")
    k: ColumnStr = tabular.col_str_simple(keys)
    vs: List[i64] = []
    vs.append(1i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    try:
        hit: DataFrame? = df2.select_by_label_i64(1i64)
        println(\"no-raise\")
    except ValueError:
        println(\"got-valueerror\")
    return 0
";
    let out = run_value_error("select_by_label_wrong_dtype", src);
    assert!(out.contains("got-valueerror"), "got: {out:?}");
}

// ── Phase C: pivot_table ──────────────────────────────────────────────

#[test]
fn pivot_table_sum_happy_path_m43() {
    // M43 flipped pivot_table to promote `index_col` to the output's
    // index instead of inserting it as a regular column.  The original
    // M41 test asserted `ncols=3` (sym + buy + sell); post-M43 it's
    // `ncols=2` (just buy + sell) and the frame's index is the unique
    // sym values.  Original name: pivot_table_sum_happy_path.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    syms: List[str] = []
    syms.append(\"a\")
    syms.append(\"a\")
    syms.append(\"b\")
    syms.append(\"b\")
    syms.append(\"a\")
    syms.append(\"b\")
    sym: ColumnStr = tabular.col_str_simple(syms)
    sides: List[str] = []
    sides.append(\"buy\")
    sides.append(\"sell\")
    sides.append(\"buy\")
    sides.append(\"sell\")
    sides.append(\"buy\")
    sides.append(\"buy\")
    side: ColumnStr = tabular.col_str_simple(sides)
    qty_vs: List[i64] = []
    qty_vs.append(10i64)
    qty_vs.append(5i64)
    qty_vs.append(7i64)
    qty_vs.append(3i64)
    qty_vs.append(2i64)
    qty_vs.append(1i64)
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
    bc: ColumnI64? = pt.get_column_i64(\"buy\")
    if bc is not none:
        b0: i64? = bc.get(0i64)
        b1: i64? = bc.get(1i64)
        if b0 is not none:
            println(\"a_buy=\" + str(b0))
        if b1 is not none:
            println(\"b_buy=\" + str(b1))
    sc: ColumnI64? = pt.get_column_i64(\"sell\")
    if sc is not none:
        s0: i64? = sc.get(0i64)
        s1: i64? = sc.get(1i64)
        if s0 is not none:
            println(\"a_sell=\" + str(s0))
        if s1 is not none:
            println(\"b_sell=\" + str(s1))
    return 0
";
    let out = run("pivot_table_sum", src);
    // 2 sym values × (2 side cols) = 2 cols — sym promoted to index.
    assert!(out.contains("nrows=2"), "got: {out:?}");
    assert!(out.contains("ncols=2"), "got: {out:?}");
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("iname=sym"), "got: {out:?}");
    // a: buys = 10 + 2 = 12, sell = 5
    // b: buys = 7 + 1 = 8, sell = 3
    assert!(out.contains("a_buy=12"), "got: {out:?}");
    assert!(out.contains("a_sell=5"), "got: {out:?}");
    assert!(out.contains("b_buy=8"), "got: {out:?}");
    assert!(out.contains("b_sell=3"), "got: {out:?}");
}

#[test]
fn pivot_table_mean_produces_f64() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, ColumnF64, DataFrame
import tabular
fn main() -> i32:
    rs: List[str] = []
    rs.append(\"a\")
    rs.append(\"a\")
    rs.append(\"b\")
    rs.append(\"b\")
    r: ColumnStr = tabular.col_str_simple(rs)
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
    let out = run("pivot_table_mean", src);
    // a: mean(10, 20) = 15.0; b: mean(30, 50) = 40.0
    assert!(out.contains("a_mean=15"), "got: {out:?}");
    assert!(out.contains("b_mean=40"), "got: {out:?}");
}

#[test]
fn pivot_table_count_produces_i64() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    rs: List[str] = []
    rs.append(\"a\")
    rs.append(\"a\")
    rs.append(\"a\")
    rs.append(\"b\")
    r: ColumnStr = tabular.col_str_simple(rs)
    cs: List[str] = []
    cs.append(\"x\")
    cs.append(\"x\")
    cs.append(\"x\")
    cs.append(\"x\")
    c: ColumnStr = tabular.col_str_simple(cs)
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    vs.append(4i64)
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
    pt: DataFrame = df.pivot_table(\"r\", \"c\", \"v\", \"count\")
    xc: ColumnI64? = pt.get_column_i64(\"x\")
    if xc is not none:
        x0: i64? = xc.get(0i64)
        x1: i64? = xc.get(1i64)
        if x0 is not none:
            println(\"a_cnt=\" + str(x0))
        if x1 is not none:
            println(\"b_cnt=\" + str(x1))
    return 0
";
    let out = run("pivot_table_count", src);
    assert!(out.contains("a_cnt=3"), "got: {out:?}");
    assert!(out.contains("b_cnt=1"), "got: {out:?}");
}

#[test]
fn pivot_table_missing_cells_are_null() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    rs: List[str] = []
    rs.append(\"a\")
    rs.append(\"b\")
    r: ColumnStr = tabular.col_str_simple(rs)
    cs: List[str] = []
    cs.append(\"x\")
    cs.append(\"y\")
    c: ColumnStr = tabular.col_str_simple(cs)
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(20i64)
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
    pt: DataFrame = df.pivot_table(\"r\", \"c\", \"v\", \"sum\")
    println(\"nrows=\" + str(pt.length()))
    println(\"ncols=\" + str(pt.ncols()))
    xc: ColumnI64? = pt.get_column_i64(\"x\")
    if xc is not none:
        x_a: i64? = xc.get(0i64)
        x_b: i64? = xc.get(1i64)
        if x_a is not none:
            println(\"a_x=\" + str(x_a))
        if x_b is none:
            println(\"b_x=null\")
    yc: ColumnI64? = pt.get_column_i64(\"y\")
    if yc is not none:
        y_a: i64? = yc.get(0i64)
        if y_a is none:
            println(\"a_y=null\")
    return 0
";
    let out = run("pivot_table_null_cells", src);
    assert!(out.contains("nrows=2"), "got: {out:?}");
    assert!(out.contains("a_x=10"), "got: {out:?}");
    assert!(out.contains("b_x=null"), "got: {out:?}");
    assert!(out.contains("a_y=null"), "got: {out:?}");
}

#[test]
fn pivot_table_bad_aggfunc_raises() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    rs: List[str] = []
    rs.append(\"a\")
    r: ColumnStr = tabular.col_str_simple(rs)
    cs: List[str] = []
    cs.append(\"x\")
    c: ColumnStr = tabular.col_str_simple(cs)
    vs: List[i64] = []
    vs.append(1i64)
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
    try:
        pt: DataFrame = df.pivot_table(\"r\", \"c\", \"v\", \"bogus\")
        println(\"no-raise\")
    except ValueError:
        println(\"got-valueerror\")
    return 0
";
    let out = run_value_error("pivot_table_bad_aggfunc", src);
    assert!(out.contains("got-valueerror"), "got: {out:?}");
}

// ── Existing-ops index-propagation sanity check (M42 update) ─────────

#[test]
fn filter_preserves_index_m42() {
    // M42 flipped the v1 scope-down: filter now PROPAGATES the index
    // through its row-selection vector.  The full M42 coverage lives in
    // vm/tests/m42_tabular_index_propagation.rs; this is the kept-but-
    // flipped M41 sanity check.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, ColumnBool, DataFrame
import tabular
fn main() -> i32:
    keys: List[str] = []
    keys.append(\"a\")
    keys.append(\"b\")
    keys.append(\"c\")
    k: ColumnStr = tabular.col_str_simple(keys)
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    mask_vs: List[bool] = []
    mask_vs.append(true)
    mask_vs.append(false)
    mask_vs.append(true)
    mask: ColumnBool = tabular.col_bool(mask_vs, [false, false, false])
    df3: DataFrame = df2.filter(mask)
    println(\"has=\" + str(df3.has_index()))
    println(\"nrows=\" + str(df3.length()))
    return 0
";
    let out = run("filter_preserves_index_m42", src);
    // Post-M42: filter PROPAGATES the index, returning an indexed frame.
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("nrows=2"), "got: {out:?}");
}
