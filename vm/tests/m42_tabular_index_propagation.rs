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

// ── Phase C: null-handling — dropna / dropna_subset / fillna_* ─────────

#[test]
fn dropna_preserves_index() {
    // dropna drops the 1 row with a null in v; the surviving 2 labels
    // are a subset of the input labels.
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
    vs.append(0i64)
    vs.append(3i64)
    ns: List[bool] = []
    ns.append(false)
    ns.append(true)
    ns.append(false)
    v: ColumnI64 = tabular.col_i64(vs, ns)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    df3: DataFrame = df2.dropna()
    println(\"has=\" + str(df3.has_index()))
    println(\"nrows=\" + str(df3.length()))
    nm: str? = df3.index_name()
    if nm is not none:
        println(\"name=\" + nm)
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
    let out = run("dropna_preserves_index", src);
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("nrows=2"), "got: {out:?}");
    assert!(out.contains("name=k"), "got: {out:?}");
    assert!(out.contains("l0=a"), "got: {out:?}");
    assert!(out.contains("l1=c"), "got: {out:?}");
}

#[test]
fn dropna_subset_preserves_index() {
    let src = "\
from tabular import Column, ColumnI64, ColumnF64, ColumnStr, DataFrame
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
    fs: List[f64] = []
    fs.append(0.0)
    fs.append(2.5)
    fs.append(0.0)
    fns: List[bool] = []
    fns.append(true)
    fns.append(false)
    fns.append(true)
    fc: ColumnF64 = tabular.col_f64(fs, fns)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cn.append(\"f\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    cols.append(fc)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    subset: List[str] = []
    subset.append(\"f\")
    df3: DataFrame = df2.dropna_subset(subset)
    println(\"has=\" + str(df3.has_index()))
    println(\"nrows=\" + str(df3.length()))
    flat: DataFrame = df3.reset_index()
    s: ColumnStr? = flat.get_column_str(\"k\")
    if s is not none:
        a: str? = s.get(0i64)
        if a is not none:
            println(\"l0=\" + a)
    return 0
";
    let out = run("dropna_subset_preserves_index", src);
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("nrows=1"), "got: {out:?}");
    assert!(out.contains("l0=b"), "got: {out:?}");
}

#[test]
fn fillna_i64_preserves_index() {
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
    vs.append(0i64)
    vs.append(3i64)
    ns: List[bool] = []
    ns.append(false)
    ns.append(true)
    ns.append(false)
    v: ColumnI64 = tabular.col_i64(vs, ns)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    df3: DataFrame = df2.fillna_i64(99i64)
    println(\"has=\" + str(df3.has_index()))
    println(\"nrows=\" + str(df3.length()))
    flat: DataFrame = df3.reset_index()
    s: ColumnStr? = flat.get_column_str(\"k\")
    if s is not none:
        a: str? = s.get(1i64)
        if a is not none:
            println(\"l1=\" + a)
    vc: ColumnI64? = df3.get_column_i64(\"v\")
    if vc is not none:
        v1: i64? = vc.get(1i64)
        if v1 is not none:
            println(\"v1=\" + str(v1))
    return 0
";
    let out = run("fillna_i64_preserves_index", src);
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("nrows=3"), "got: {out:?}");
    assert!(out.contains("l1=b"), "got: {out:?}");
    assert!(out.contains("v1=99"), "got: {out:?}");
}

#[test]
fn fillna_f64_preserves_index() {
    let src = "\
from tabular import Column, ColumnF64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    keys: List[str] = []
    keys.append(\"a\")
    keys.append(\"b\")
    k: ColumnStr = tabular.col_str_simple(keys)
    fs: List[f64] = []
    fs.append(0.0)
    fs.append(1.5)
    ns: List[bool] = []
    ns.append(true)
    ns.append(false)
    f: ColumnF64 = tabular.col_f64(fs, ns)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"f\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(f)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    df3: DataFrame = df2.fillna_f64(0.0)
    println(\"has=\" + str(df3.has_index()))
    println(\"nrows=\" + str(df3.length()))
    return 0
";
    let out = run("fillna_f64_preserves_index", src);
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("nrows=2"), "got: {out:?}");
}

#[test]
fn fillna_str_preserves_index() {
    let src = "\
from tabular import Column, ColumnStr, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    keys: List[str] = []
    keys.append(\"a\")
    keys.append(\"b\")
    keys.append(\"c\")
    k: ColumnStr = tabular.col_str_simple(keys)
    vs: List[str] = []
    vs.append(\"x\")
    vs.append(\"\")
    vs.append(\"z\")
    ns: List[bool] = []
    ns.append(false)
    ns.append(true)
    ns.append(false)
    v: ColumnStr = tabular.col_str(vs, ns)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(k)
    cols.append(v)
    df: DataFrame = tabular.from_columns(cn, cols)
    df2: DataFrame = df.set_index(\"k\")
    df3: DataFrame = df2.fillna_str(\"missing\")
    println(\"has=\" + str(df3.has_index()))
    println(\"nrows=\" + str(df3.length()))
    return 0
";
    let out = run("fillna_str_preserves_index", src);
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("nrows=3"), "got: {out:?}");
}

// ── Phase D: merge — index propagation per how ─────────────────────────

fn build_merge_helpers() -> &'static str {
    // Two small frames sharing a `tid` join column.  lhs has a string
    // "lid" index; rhs has a string "rid" index.  Both build helpers
    // are reused by the four how-mode tests below.
    "fn build_lhs() -> DataFrame:
    lkeys: List[str] = []
    lkeys.append(\"L1\")
    lkeys.append(\"L2\")
    lkeys.append(\"L3\")
    lk: ColumnStr = tabular.col_str_simple(lkeys)
    tids: List[i64] = []
    tids.append(1i64)
    tids.append(2i64)
    tids.append(3i64)
    t: ColumnI64 = tabular.col_i64_simple(tids)
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(20i64)
    vs.append(30i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"lid\")
    cn.append(\"tid\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(lk)
    cols.append(t)
    cols.append(v)
    return tabular.from_columns(cn, cols).set_index(\"lid\")

fn build_rhs() -> DataFrame:
    rkeys: List[str] = []
    rkeys.append(\"R1\")
    rkeys.append(\"R2\")
    rk: ColumnStr = tabular.col_str_simple(rkeys)
    tids: List[i64] = []
    tids.append(2i64)
    tids.append(4i64)
    t: ColumnI64 = tabular.col_i64_simple(tids)
    rates: List[f64] = []
    rates.append(0.5)
    rates.append(0.7)
    r: ColumnF64 = tabular.col_f64_simple(rates)
    cn: List[str] = []
    cn.append(\"rid\")
    cn.append(\"tid\")
    cn.append(\"rate\")
    cols: List[Column] = []
    cols.append(rk)
    cols.append(t)
    cols.append(r)
    return tabular.from_columns(cn, cols).set_index(\"rid\")

"
}

#[test]
fn merge_inner_preserves_lhs_index() {
    let src = format!(
        "from tabular import Column, ColumnI64, ColumnF64, ColumnStr, DataFrame
import tabular
{}fn main() -> i32:
    lhs: DataFrame = build_lhs()
    rhs: DataFrame = build_rhs()
    on: List[str] = []
    on.append(\"tid\")
    merged: DataFrame = lhs.merge(rhs, on, \"inner\")
    println(\"has=\" + str(merged.has_index()))
    println(\"nrows=\" + str(merged.length()))
    nm: str? = merged.index_name()
    if nm is not none:
        println(\"name=\" + nm)
    flat: DataFrame = merged.reset_index()
    s: ColumnStr? = flat.get_column_str(\"lid\")
    if s is not none:
        a: str? = s.get(0i64)
        if a is not none:
            println(\"l0=\" + a)
    return 0
",
        build_merge_helpers()
    );
    let out = run("merge_inner_preserves_lhs_index", &src);
    // Only tid=2 matches between lhs (1,2,3) and rhs (2,4); that's
    // lhs row 1 ("L2").
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("nrows=1"), "got: {out:?}");
    assert!(out.contains("name=lid"), "got: {out:?}");
    assert!(out.contains("l0=L2"), "got: {out:?}");
}

#[test]
fn merge_left_preserves_lhs_index() {
    let src = format!(
        "from tabular import Column, ColumnI64, ColumnF64, ColumnStr, DataFrame
import tabular
{}fn main() -> i32:
    lhs: DataFrame = build_lhs()
    rhs: DataFrame = build_rhs()
    on: List[str] = []
    on.append(\"tid\")
    merged: DataFrame = lhs.merge(rhs, on, \"left\")
    println(\"has=\" + str(merged.has_index()))
    println(\"nrows=\" + str(merged.length()))
    nm: str? = merged.index_name()
    if nm is not none:
        println(\"name=\" + nm)
    flat: DataFrame = merged.reset_index()
    s: ColumnStr? = flat.get_column_str(\"lid\")
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
",
        build_merge_helpers()
    );
    let out = run("merge_left_preserves_lhs_index", &src);
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("nrows=3"), "got: {out:?}");
    assert!(out.contains("name=lid"), "got: {out:?}");
    assert!(out.contains("l0=L1"), "got: {out:?}");
    assert!(out.contains("l1=L2"), "got: {out:?}");
    assert!(out.contains("l2=L3"), "got: {out:?}");
}

#[test]
fn merge_right_preserves_rhs_index() {
    let src = format!(
        "from tabular import Column, ColumnI64, ColumnF64, ColumnStr, DataFrame
import tabular
{}fn main() -> i32:
    lhs: DataFrame = build_lhs()
    rhs: DataFrame = build_rhs()
    on: List[str] = []
    on.append(\"tid\")
    merged: DataFrame = lhs.merge(rhs, on, \"right\")
    println(\"has=\" + str(merged.has_index()))
    println(\"nrows=\" + str(merged.length()))
    nm: str? = merged.index_name()
    if nm is not none:
        println(\"name=\" + nm)
    return 0
",
        build_merge_helpers()
    );
    let out = run("merge_right_preserves_rhs_index", &src);
    // rhs has 2 rows (tid=2 matches lhs, tid=4 unmatched); right join
    // preserves both, indexed by rhs's "rid".
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("nrows=2"), "got: {out:?}");
    assert!(out.contains("name=rid"), "got: {out:?}");
}

#[test]
fn merge_outer_preserves_mixed_index() {
    // outer with matching index dtypes (both ColumnStr): lhs index for
    // matched/left-only rows, rhs index for right-only rows.
    let src = format!(
        "from tabular import Column, ColumnI64, ColumnF64, ColumnStr, DataFrame
import tabular
{}fn main() -> i32:
    lhs: DataFrame = build_lhs()
    rhs: DataFrame = build_rhs()
    on: List[str] = []
    on.append(\"tid\")
    merged: DataFrame = lhs.merge(rhs, on, \"outer\")
    println(\"has=\" + str(merged.has_index()))
    println(\"nrows=\" + str(merged.length()))
    nm: str? = merged.index_name()
    if nm is not none:
        println(\"name=\" + nm)
    return 0
",
        build_merge_helpers()
    );
    let out = run("merge_outer_preserves_mixed_index", &src);
    // 3 lhs rows (L1, L2, L3) + 1 right-only (R2 for tid=4) = 4 rows.
    assert!(out.contains("has=true"), "got: {out:?}");
    assert!(out.contains("nrows=4"), "got: {out:?}");
    // lhs wins the index_name policy.
    assert!(out.contains("name=lid"), "got: {out:?}");
}

#[test]
fn merge_unindexed_lhs_drops_index() {
    // If lhs has no index, an inner/left merge falls back to RangeIndex.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    tids: List[i64] = []
    tids.append(1i64)
    tids.append(2i64)
    t: ColumnI64 = tabular.col_i64_simple(tids)
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(20i64)
    v: ColumnI64 = tabular.col_i64_simple(vs)
    cn: List[str] = []
    cn.append(\"tid\")
    cn.append(\"v\")
    cols: List[Column] = []
    cols.append(t)
    cols.append(v)
    lhs: DataFrame = tabular.from_columns(cn, cols)
    # rhs with index
    rkeys: List[str] = []
    rkeys.append(\"R1\")
    rk: ColumnStr = tabular.col_str_simple(rkeys)
    rtids: List[i64] = []
    rtids.append(2i64)
    rt: ColumnI64 = tabular.col_i64_simple(rtids)
    rn: List[str] = []
    rn.append(\"rid\")
    rn.append(\"tid\")
    rc: List[Column] = []
    rc.append(rk)
    rc.append(rt)
    rhs: DataFrame = tabular.from_columns(rn, rc).set_index(\"rid\")
    on: List[str] = []
    on.append(\"tid\")
    merged: DataFrame = lhs.merge(rhs, on, \"left\")
    println(\"has=\" + str(merged.has_index()))
    println(\"nrows=\" + str(merged.length()))
    return 0
";
    let out = run("merge_unindexed_lhs_drops_index", src);
    assert!(out.contains("has=false"), "got: {out:?}");
    assert!(out.contains("nrows=2"), "got: {out:?}");
}
