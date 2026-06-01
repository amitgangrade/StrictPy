//! M51 grab-bag regression tests for the `tabular` package — the
//! features layered on top of the (independently-authored) RollingWindow
//! that already lives on main. RollingWindow itself is covered by
//! `vm/tests/m51_rolling_window.rs`; this file covers only:
//!
//!   B-*: explicit ColumnCategorical is_ordered bit (replaces the M49
//!        heuristic).
//!   C-*: categorical ordered-sort follows category (codes) order.
//!   D-*: outer-MultiIndex loc_range_level_* on a chosen level.
//!
//! Surface documented in LANGUAGE_GUIDE.md §5 (post-M51).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m51_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

fn run(name: &str, src: &str) -> String {
    let p = compile_snippet(name, src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "{name}: exit={code} stdout={out:?}");
    out
}


// ──────────────────────────────────────────────────────────────────
// M51 grab-bag (layered onto the remote RollingWindow): Phase B
// (ColumnCategorical is_ordered bit), Phase C (categorical ordered-
// sort), Phase D (outer-MultiIndex loc_range_level_*).
// RollingWindow itself is covered by vm/tests/m51_rolling_window.rs.
// ──────────────────────────────────────────────────────────────────

#[test]
fn is_ordered_true_when_all_categories_used() {
    // The case the OLD heuristic got wrong: ordered categorical where
    // every category is referenced by a code should STILL report true.
    let src = "\
from tabular import ColumnCategorical
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"a\")
    vs.append(\"b\")
    vs.append(\"c\")
    cats: List[str] = []
    cats.append(\"a\")
    cats.append(\"b\")
    cats.append(\"c\")
    cc: ColumnCategorical = tabular.col_categorical_ordered(vs, cats)
    if cc.is_ordered():
        println(\"ordered=true\")
    else:
        println(\"ordered=false\")
    return 0
";
    let out = run("is_ordered_all_used", src);
    assert!(out.contains("ordered=true"), "got: {out:?}");
}

#[test]
fn is_ordered_false_for_plain_constructor() {
    let src = "\
from tabular import ColumnCategorical
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"a\")
    vs.append(\"b\")
    vs.append(\"a\")
    cc: ColumnCategorical = tabular.col_categorical(vs)
    if cc.is_ordered():
        println(\"ordered=true\")
    else:
        println(\"ordered=false\")
    return 0
";
    let out = run("is_ordered_plain", src);
    assert!(out.contains("ordered=false"), "got: {out:?}");
}

#[test]
fn is_ordered_true_for_from_codes() {
    let src = "\
from tabular import ColumnCategorical
import tabular
fn main() -> i32:
    codes: List[i64] = []
    codes.append(0i64)
    codes.append(1i64)
    cats: List[str] = []
    cats.append(\"a\")
    cats.append(\"b\")
    cc: ColumnCategorical = tabular.col_categorical_from_codes(codes, cats)
    if cc.is_ordered():
        println(\"ordered=true\")
    else:
        println(\"ordered=false\")
    return 0
";
    let out = run("is_ordered_from_codes", src);
    assert!(out.contains("ordered=true"), "got: {out:?}");
}

// ──────────────────────────────────────────────────────────────────
// Phase C: categorical ordered-sort
// ──────────────────────────────────────────────────────────────────

#[test]
fn sort_by_categorical_follows_category_order() {
    // categories ["low","mid","high"] (non-alphabetical).  Sorting a
    // categorical column ascending sorts by code (= category order),
    // NOT lexical order.  Values: high, low, mid -> codes 2,0,1.
    // After ascending sort by code -> low(0), mid(1), high(2), i.e.
    // the companion value column reorders to follow.
    let src = "\
from tabular import Column, ColumnCategorical, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"high\")
    vs.append(\"low\")
    vs.append(\"mid\")
    cats: List[str] = []
    cats.append(\"low\")
    cats.append(\"mid\")
    cats.append(\"high\")
    cc: ColumnCategorical = tabular.col_categorical_ordered(vs, cats)
    tag: List[i64] = []
    tag.append(100i64)
    tag.append(200i64)
    tag.append(300i64)
    ct: ColumnI64 = tabular.col_i64_simple(tag)
    names: List[str] = []
    names.append(\"level\")
    names.append(\"tag\")
    cols: List[Column] = []
    cols.append(cc)
    cols.append(ct)
    df: DataFrame = tabular.from_columns(names, cols)
    s: DataFrame = df.sort_by(\"level\", true)
    col: ColumnI64? = s.get_column_i64(\"tag\")
    if col is not none:
        t0: i64? = col.get(0i64)
        t1: i64? = col.get(1i64)
        t2: i64? = col.get(2i64)
        if t0 is not none:
            println(\"t0=\" + str(t0))
        if t1 is not none:
            println(\"t1=\" + str(t1))
        if t2 is not none:
            println(\"t2=\" + str(t2))
    return 0
";
    let out = run("sort_cat_order", src);
    // ascending by code: low(tag 200), mid(tag 300), high(tag 100).
    assert!(out.contains("t0=200"), "got: {out:?}");
    assert!(out.contains("t1=300"), "got: {out:?}");
    assert!(out.contains("t2=100"), "got: {out:?}");
}

#[test]
fn sort_by_categorical_descending() {
    let src = "\
from tabular import Column, ColumnCategorical, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"low\")
    vs.append(\"high\")
    vs.append(\"mid\")
    cats: List[str] = []
    cats.append(\"low\")
    cats.append(\"mid\")
    cats.append(\"high\")
    cc: ColumnCategorical = tabular.col_categorical_ordered(vs, cats)
    tag: List[i64] = []
    tag.append(1i64)
    tag.append(2i64)
    tag.append(3i64)
    ct: ColumnI64 = tabular.col_i64_simple(tag)
    names: List[str] = []
    names.append(\"level\")
    names.append(\"tag\")
    cols: List[Column] = []
    cols.append(cc)
    cols.append(ct)
    df: DataFrame = tabular.from_columns(names, cols)
    s: DataFrame = df.sort_by(\"level\", false)
    col: ColumnI64? = s.get_column_i64(\"tag\")
    if col is not none:
        t0: i64? = col.get(0i64)
        if t0 is not none:
            println(\"t0=\" + str(t0))
    return 0
";
    let out = run("sort_cat_desc", src);
    // descending by code: high(tag 2) first.
    assert!(out.contains("t0=2"), "got: {out:?}");
}

// ──────────────────────────────────────────────────────────────────
// Phase D: outer-MultiIndex loc_range_level_*
// ──────────────────────────────────────────────────────────────────

#[test]
fn loc_range_level_outer_str() {
    // 2-level MultiIndex; filter on level 0 (outer).
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    outer: List[str] = []
    outer.append(\"a\")
    outer.append(\"b\")
    outer.append(\"c\")
    outer.append(\"d\")
    inner: List[i64] = []
    inner.append(1i64)
    inner.append(2i64)
    inner.append(3i64)
    inner.append(4i64)
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(20i64)
    vs.append(30i64)
    vs.append(40i64)
    co: ColumnStr = tabular.col_str_simple(outer)
    ci: ColumnI64 = tabular.col_i64_simple(inner)
    cv: ColumnI64 = tabular.col_i64_simple(vs)
    names: List[str] = []
    names.append(\"o\")
    names.append(\"i\")
    names.append(\"v\")
    cols: List[Column] = []
    cols.append(co)
    cols.append(ci)
    cols.append(cv)
    df: DataFrame = tabular.from_columns(names, cols)
    idx: List[str] = []
    idx.append(\"o\")
    idx.append(\"i\")
    df2: DataFrame = df.set_index_list(idx)
    sub: DataFrame = df2.loc_range_level_str(0i64, \"b\", \"c\")
    println(\"rows=\" + str(sub.length()))
    return 0
";
    let out = run("loc_range_level_outer_str", src);
    assert!(out.contains("rows=2"), "got: {out:?}");
}

#[test]
fn loc_range_level_inner_matches_multi() {
    // Filtering level 1 (inner) with loc_range_level_i64 must match
    // the existing loc_range_multi_i64.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    outer: List[str] = []
    outer.append(\"x\")
    outer.append(\"x\")
    outer.append(\"y\")
    outer.append(\"y\")
    inner: List[i64] = []
    inner.append(10i64)
    inner.append(20i64)
    inner.append(30i64)
    inner.append(40i64)
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    vs.append(4i64)
    co: ColumnStr = tabular.col_str_simple(outer)
    ci: ColumnI64 = tabular.col_i64_simple(inner)
    cv: ColumnI64 = tabular.col_i64_simple(vs)
    names: List[str] = []
    names.append(\"o\")
    names.append(\"i\")
    names.append(\"v\")
    cols: List[Column] = []
    cols.append(co)
    cols.append(ci)
    cols.append(cv)
    df: DataFrame = tabular.from_columns(names, cols)
    idx: List[str] = []
    idx.append(\"o\")
    idx.append(\"i\")
    df2: DataFrame = df.set_index_list(idx)
    a: DataFrame = df2.loc_range_level_i64(1i64, 15i64, 35i64)
    b: DataFrame = df2.loc_range_multi_i64(15i64, 35i64)
    println(\"a_rows=\" + str(a.length()))
    println(\"b_rows=\" + str(b.length()))
    return 0
";
    let out = run("loc_range_level_inner", src);
    assert!(out.contains("a_rows=2"), "got: {out:?}");
    assert!(out.contains("b_rows=2"), "got: {out:?}");
}

#[test]
fn loc_range_level_out_of_range_errors() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    outer: List[str] = []
    outer.append(\"x\")
    outer.append(\"y\")
    inner: List[i64] = []
    inner.append(1i64)
    inner.append(2i64)
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(20i64)
    co: ColumnStr = tabular.col_str_simple(outer)
    ci: ColumnI64 = tabular.col_i64_simple(inner)
    cv: ColumnI64 = tabular.col_i64_simple(vs)
    names: List[str] = []
    names.append(\"o\")
    names.append(\"i\")
    names.append(\"v\")
    cols: List[Column] = []
    cols.append(co)
    cols.append(ci)
    cols.append(cv)
    df: DataFrame = tabular.from_columns(names, cols)
    idx: List[str] = []
    idx.append(\"o\")
    idx.append(\"i\")
    df2: DataFrame = df.set_index_list(idx)
    sub: DataFrame = df2.loc_range_level_i64(5i64, 0i64, 100i64)
    println(\"unreached\")
    return 0
";
    let p = compile_snippet("loc_range_level_oob", src);
    let res = run_file_capture(&p);
    assert!(res.is_err(), "expected uncaught ValueError; got {res:?}");
}

#[test]
fn loc_range_level_wrong_dtype_errors() {
    // level 0 is str; calling loc_range_level_i64 on it should error.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    outer: List[str] = []
    outer.append(\"x\")
    outer.append(\"y\")
    inner: List[i64] = []
    inner.append(1i64)
    inner.append(2i64)
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(20i64)
    co: ColumnStr = tabular.col_str_simple(outer)
    ci: ColumnI64 = tabular.col_i64_simple(inner)
    cv: ColumnI64 = tabular.col_i64_simple(vs)
    names: List[str] = []
    names.append(\"o\")
    names.append(\"i\")
    names.append(\"v\")
    cols: List[Column] = []
    cols.append(co)
    cols.append(ci)
    cols.append(cv)
    df: DataFrame = tabular.from_columns(names, cols)
    idx: List[str] = []
    idx.append(\"o\")
    idx.append(\"i\")
    df2: DataFrame = df.set_index_list(idx)
    sub: DataFrame = df2.loc_range_level_i64(0i64, 0i64, 100i64)
    println(\"unreached\")
    return 0
";
    let p = compile_snippet("loc_range_level_wrongdtype", src);
    let res = run_file_capture(&p);
    assert!(res.is_err(), "expected uncaught ValueError; got {res:?}");
}
