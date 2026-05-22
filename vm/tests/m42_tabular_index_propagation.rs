//! M42 in-process regression tests for `tabular` index propagation
//! through existing DataFrame methods.
//!
//! M41 shipped the optional DatetimeIndex but every existing method
//! that returned a fresh frame DROPPED the index in v1.  M42 closes
//! that scope-down: 11 existing methods (filter, sort_by, head, tail,
//! iloc, select, drop, rename, dropna, dropna_subset, fillna_*, merge)
//! now propagate the index through their row/column transformations.
//!
//! Surface documented in LANGUAGE_GUIDE.md §5 (post-M42).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m42_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

fn run(name: &str, src: &str) -> String {
    let p = compile_snippet(name, src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "{name}: exit={code} stdout={out:?}");
    out
}

// ── Phase A: row-selection ops — filter / sort_by / head / tail / iloc ─

#[test]
fn filter_preserves_index() {
    // M41's filter_drops_index test flipped: filter now propagates the
    // index through its row-selection vector.  Surviving labels are the
    // subset of input labels where the mask was true.
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
    nm: str? = df3.index_name()
    if nm is not none:
        println(\"name=\" + nm)
    idx: Column? = df3.index()
    if idx is not none:
        sc: ColumnStr? = df3.get_column_str(\"v\")
        # cast helper unavailable here — instead test by re-flattening
        flat: DataFrame = df3.reset_index()
        s: ColumnStr? = flat.get_column_str(\"k\")
        if s is not none:
            a: str? = s.get(0i64)
            b: str? = s.get(1i64)
            if a is not none:
                println(\"l0=\" + a)
            if b is not none:
                println(\"l1=\" + b)
    return 0
";
    let out = run("filter_preserves_index", src);
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("nrows=2"), "got: {out:?}");
    assert!(out.contains("name=k"), "got: {out:?}");
    assert!(out.contains("l0=a"), "got: {out:?}");
    assert!(out.contains("l1=c"), "got: {out:?}");
}

#[test]
fn filter_no_index_stays_rangeindex() {
    // Filtering an un-indexed frame keeps the v1 RangeIndex behavior:
    // output frame has no index.
    let src = "\
from tabular import Column, ColumnI64, ColumnBool, DataFrame
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(20i64)
    vs.append(30i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    mask_vs: List[bool] = []
    mask_vs.append(true)
    mask_vs.append(false)
    mask_vs.append(true)
    mask: ColumnBool = tabular.col_bool(mask_vs, [false, false, false])
    df2: DataFrame = df.filter(mask)
    println(\"has=\" + str(df2.has_index()))
    println(\"nrows=\" + str(df2.length()))
    return 0
";
    let out = run("filter_no_index_stays_rangeindex", src);
    assert!(out.contains("has=false"), "got: {out:?}");
    assert!(out.contains("nrows=2"), "got: {out:?}");
}

#[test]
fn sort_by_preserves_index() {
    // sort_by permutes rows by a regular column — the index must move
    // along with each row.
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
    df3: DataFrame = df2.sort_by(\"v\", true)
    println(\"has=\" + str(df3.has_index()))
    flat: DataFrame = df3.reset_index()
    s: ColumnStr? = flat.get_column_str(\"k\")
    if s is not none:
        a: str? = s.get(0i64)
        b: str? = s.get(1i64)
        c: str? = s.get(2i64)
        if a is not none:
            println(\"l0=\" + a)
        if b is not none:
            println(\"l1=\" + b)
        if c is not none:
            println(\"l2=\" + c)
    return 0
";
    let out = run("sort_by_preserves_index", src);
    assert!(out.contains("has=true"), "got: {out:?}");
    // Sorted by v ascending: rows {b=10, c=20, a=30}.
    assert!(out.contains("l0=b"), "got: {out:?}");
    assert!(out.contains("l1=c"), "got: {out:?}");
    assert!(out.contains("l2=a"), "got: {out:?}");
}

#[test]
fn head_preserves_index() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    keys: List[str] = []
    keys.append(\"a\")
    keys.append(\"b\")
    keys.append(\"c\")
    keys.append(\"d\")
    k: ColumnStr = tabular.col_str_simple(keys)
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    vs.append(4i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    df3: DataFrame = df2.head(2i64)
    println(\"has=\" + str(df3.has_index()))
    println(\"nrows=\" + str(df3.length()))
    flat: DataFrame = df3.reset_index()
    s: ColumnStr? = flat.get_column_str(\"k\")
    if s is not none:
        a: str? = s.get(0i64)
        b: str? = s.get(1i64)
        if a is not none:
            println(\"l0=\" + a)
        if b is not none:
            println(\"l1=\" + b)
    return 0
";
    let out = run("head_preserves_index", src);
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("nrows=2"), "got: {out:?}");
    assert!(out.contains("l0=a"), "got: {out:?}");
    assert!(out.contains("l1=b"), "got: {out:?}");
}

#[test]
fn tail_preserves_index() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    keys: List[str] = []
    keys.append(\"a\")
    keys.append(\"b\")
    keys.append(\"c\")
    keys.append(\"d\")
    k: ColumnStr = tabular.col_str_simple(keys)
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    vs.append(4i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    df3: DataFrame = df2.tail(2i64)
    println(\"has=\" + str(df3.has_index()))
    println(\"nrows=\" + str(df3.length()))
    flat: DataFrame = df3.reset_index()
    s: ColumnStr? = flat.get_column_str(\"k\")
    if s is not none:
        a: str? = s.get(0i64)
        b: str? = s.get(1i64)
        if a is not none:
            println(\"l0=\" + a)
        if b is not none:
            println(\"l1=\" + b)
    return 0
";
    let out = run("tail_preserves_index", src);
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("nrows=2"), "got: {out:?}");
    assert!(out.contains("l0=c"), "got: {out:?}");
    assert!(out.contains("l1=d"), "got: {out:?}");
}

#[test]
fn iloc_preserves_index() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    keys: List[str] = []
    keys.append(\"a\")
    keys.append(\"b\")
    keys.append(\"c\")
    keys.append(\"d\")
    keys.append(\"e\")
    k: ColumnStr = tabular.col_str_simple(keys)
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    vs.append(4i64)
    vs.append(5i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    df3: DataFrame = df2.iloc(1i64, 4i64)
    println(\"has=\" + str(df3.has_index()))
    println(\"nrows=\" + str(df3.length()))
    flat: DataFrame = df3.reset_index()
    s: ColumnStr? = flat.get_column_str(\"k\")
    if s is not none:
        a: str? = s.get(0i64)
        b: str? = s.get(1i64)
        c: str? = s.get(2i64)
        if a is not none:
            println(\"l0=\" + a)
        if b is not none:
            println(\"l1=\" + b)
        if c is not none:
            println(\"l2=\" + c)
    return 0
";
    let out = run("iloc_preserves_index", src);
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("nrows=3"), "got: {out:?}");
    assert!(out.contains("l0=b"), "got: {out:?}");
    assert!(out.contains("l1=c"), "got: {out:?}");
    assert!(out.contains("l2=d"), "got: {out:?}");
}

// ── Phase B: column-list ops — select / drop / rename ──────────────────

#[test]
fn select_preserves_index() {
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
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    ws: List[i64] = []
    ws.append(10i64)
    ws.append(20i64)
    ws.append(30i64)
    w: ColumnI64 = tabular.col_i64_simple(ws)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cn.append(\"w\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    cols.append(w)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    want: List[str] = []
    want.append(\"v\")
    df3: DataFrame = df2.select(want)
    println(\"has=\" + str(df3.has_index()))
    println(\"ncols=\" + str(df3.ncols()))
    nm: str? = df3.index_name()
    if nm is not none:
        println(\"name=\" + nm)
    return 0
";
    let out = run("select_preserves_index", src);
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("ncols=1"), "got: {out:?}");
    assert!(out.contains("name=k"), "got: {out:?}");
}

#[test]
fn drop_preserves_index() {
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
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    ws: List[i64] = []
    ws.append(10i64)
    ws.append(20i64)
    ws.append(30i64)
    w: ColumnI64 = tabular.col_i64_simple(ws)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cn.append(\"w\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    cols.append(w)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    drop_names: List[str] = []
    drop_names.append(\"w\")
    df3: DataFrame = df2.drop(drop_names)
    println(\"has=\" + str(df3.has_index()))
    println(\"ncols=\" + str(df3.ncols()))
    nm: str? = df3.index_name()
    if nm is not none:
        println(\"name=\" + nm)
    return 0
";
    let out = run("drop_preserves_index", src);
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("ncols=1"), "got: {out:?}");
    assert!(out.contains("name=k"), "got: {out:?}");
}

#[test]
fn rename_preserves_index() {
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
    renames: List[Tuple[str, str]] = []
    renames.append((\"v\", \"value\"))
    df3: DataFrame = df2.rename(renames)
    println(\"has=\" + str(df3.has_index()))
    println(\"nrows=\" + str(df3.length()))
    nm: str? = df3.index_name()
    if nm is not none:
        println(\"name=\" + nm)
    vc: ColumnI64? = df3.get_column_i64(\"value\")
    if vc is not none:
        println(\"renamed=ok\")
    return 0
";
    let out = run("rename_preserves_index", src);
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("nrows=3"), "got: {out:?}");
    assert!(out.contains("name=k"), "got: {out:?}");
    assert!(out.contains("renamed=ok"), "got: {out:?}");
}
