//! M51 in-process regression tests for the `tabular` RollingWindow
//! chainable class + grab-bag follow-ups.
//!
//! See `docs/thesis/m51_round/SHARED_BRIEF.md` for the full scope.
//! Surface documented in LANGUAGE_GUIDE.md §5 (post-M51).
//!
//! Test groups:
//!   A-*: RollingWindow chainable (mean/sum/std/min/max/agg + center +
//!        min_periods + index preservation + errors + col-drop).
//!   B-*: explicit ColumnCategorical is_ordered bit.
//!   C-*: categorical ordered-sort follows category order.
//!   D-*: outer-MultiIndex loc_range_level_*.

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
// Phase A: RollingWindow chainable
// ──────────────────────────────────────────────────────────────────

#[test]
fn rolling_mean_trailing_first_two_null() {
    // [1,2,3,4,5].rolling(3).mean() -> [null, null, 2, 3, 4]
    let src = "\
from tabular import Column, ColumnI64, ColumnF64, DataFrame, RollingWindow
import tabular
fn main() -> i32:
    a: List[i64] = []
    a.append(1i64)
    a.append(2i64)
    a.append(3i64)
    a.append(4i64)
    a.append(5i64)
    ca: ColumnI64 = tabular.col_i64_simple(a)
    names: List[str] = []
    names.append(\"a\")
    cols: List[Column] = []
    cols.append(ca)
    df: DataFrame = tabular.from_columns(names, cols)
    rw: RollingWindow = df.rolling(3i64)
    out: DataFrame = rw.mean()
    println(\"nrows=\" + str(out.length()))
    col: ColumnF64? = out.get_column_f64(\"a\")
    if col is not none:
        v0: f64? = col.get(0i64)
        v1: f64? = col.get(1i64)
        v2: f64? = col.get(2i64)
        v3: f64? = col.get(3i64)
        v4: f64? = col.get(4i64)
        if v0 is none:
            println(\"v0=null\")
        if v1 is none:
            println(\"v1=null\")
        if v2 is not none:
            println(\"v2=\" + str(v2))
        if v3 is not none:
            println(\"v3=\" + str(v3))
        if v4 is not none:
            println(\"v4=\" + str(v4))
    return 0
";
    let out = run("rolling_mean_trailing", src);
    assert!(out.contains("nrows=5"), "got: {out:?}");
    assert!(out.contains("v0=null"), "got: {out:?}");
    assert!(out.contains("v1=null"), "got: {out:?}");
    assert!(out.contains("v2=2.0"), "got: {out:?}");
    assert!(out.contains("v3=3.0"), "got: {out:?}");
    assert!(out.contains("v4=4.0"), "got: {out:?}");
}

#[test]
fn rolling_mean_center_odd_window() {
    // [1,2,3,4,5].rolling(3, center=True).mean()
    // offset = (3-1)/2 = 1.  Window for i centered: [i-1, i+1].
    // i=0 -> [.,0,1] clipped [0,1] but min_periods default=3 -> null.
    // i=1 -> [0,1,2] -> mean(1,2,3)=2
    // i=2 -> [1,2,3] -> mean(2,3,4)=3
    // i=3 -> [2,3,4] -> mean(3,4,5)=4
    // i=4 -> [3,4,.] -> 2 vals < 3 -> null
    let src = "\
from tabular import Column, ColumnI64, ColumnF64, DataFrame, RollingWindow
import tabular
fn main() -> i32:
    a: List[i64] = []
    a.append(1i64)
    a.append(2i64)
    a.append(3i64)
    a.append(4i64)
    a.append(5i64)
    ca: ColumnI64 = tabular.col_i64_simple(a)
    names: List[str] = []
    names.append(\"a\")
    cols: List[Column] = []
    cols.append(ca)
    df: DataFrame = tabular.from_columns(names, cols)
    rw: RollingWindow = df.rolling(3i64).center(true)
    out: DataFrame = rw.mean()
    col: ColumnF64? = out.get_column_f64(\"a\")
    if col is not none:
        v0: f64? = col.get(0i64)
        v1: f64? = col.get(1i64)
        v2: f64? = col.get(2i64)
        v3: f64? = col.get(3i64)
        v4: f64? = col.get(4i64)
        if v0 is none:
            println(\"v0=null\")
        if v1 is not none:
            println(\"v1=\" + str(v1))
        if v2 is not none:
            println(\"v2=\" + str(v2))
        if v3 is not none:
            println(\"v3=\" + str(v3))
        if v4 is none:
            println(\"v4=null\")
    return 0
";
    let out = run("rolling_mean_center_odd", src);
    assert!(out.contains("v0=null"), "got: {out:?}");
    assert!(out.contains("v1=2.0"), "got: {out:?}");
    assert!(out.contains("v2=3.0"), "got: {out:?}");
    assert!(out.contains("v3=4.0"), "got: {out:?}");
    assert!(out.contains("v4=null"), "got: {out:?}");
}

#[test]
fn rolling_mean_center_even_window_min_periods_1() {
    // pandas: pd.Series([1,2,3,4,5]).rolling(4, center=True,
    //   min_periods=1).mean() ->
    //   offset = (4-1)/2 = 1.  Window for i: [i+1-3, i+1] = [i-2, i+1].
    //   i=0 -> [.,.,0,1] -> mean(1,2)=1.5
    //   i=1 -> [.,0,1,2] -> mean(1,2,3)=2.0
    //   i=2 -> [0,1,2,3] -> mean(1,2,3,4)=2.5
    //   i=3 -> [1,2,3,4] -> mean(2,3,4,5)=3.5
    //   i=4 -> [2,3,4,.] -> mean(3,4,5)=4.0
    let src = "\
from tabular import Column, ColumnI64, ColumnF64, DataFrame, RollingWindow
import tabular
fn main() -> i32:
    a: List[i64] = []
    a.append(1i64)
    a.append(2i64)
    a.append(3i64)
    a.append(4i64)
    a.append(5i64)
    ca: ColumnI64 = tabular.col_i64_simple(a)
    names: List[str] = []
    names.append(\"a\")
    cols: List[Column] = []
    cols.append(ca)
    df: DataFrame = tabular.from_columns(names, cols)
    rw: RollingWindow = df.rolling(4i64).center(true).min_periods(1i64)
    out: DataFrame = rw.mean()
    col: ColumnF64? = out.get_column_f64(\"a\")
    if col is not none:
        v0: f64? = col.get(0i64)
        v1: f64? = col.get(1i64)
        v2: f64? = col.get(2i64)
        v3: f64? = col.get(3i64)
        v4: f64? = col.get(4i64)
        if v0 is not none:
            println(\"v0=\" + str(v0))
        if v1 is not none:
            println(\"v1=\" + str(v1))
        if v2 is not none:
            println(\"v2=\" + str(v2))
        if v3 is not none:
            println(\"v3=\" + str(v3))
        if v4 is not none:
            println(\"v4=\" + str(v4))
    return 0
";
    let out = run("rolling_mean_center_even", src);
    assert!(out.contains("v0=1.5"), "got: {out:?}");
    assert!(out.contains("v1=2.0"), "got: {out:?}");
    assert!(out.contains("v2=2.5"), "got: {out:?}");
    assert!(out.contains("v3=3.5"), "got: {out:?}");
    assert!(out.contains("v4=4.0"), "got: {out:?}");
}

#[test]
fn rolling_min_periods_partial_leading_edge() {
    // [1,2,3].rolling(3).min_periods(1).sum() -> [1, 3, 6]
    let src = "\
from tabular import Column, ColumnI64, DataFrame, RollingWindow
import tabular
fn main() -> i32:
    a: List[i64] = []
    a.append(1i64)
    a.append(2i64)
    a.append(3i64)
    ca: ColumnI64 = tabular.col_i64_simple(a)
    names: List[str] = []
    names.append(\"a\")
    cols: List[Column] = []
    cols.append(ca)
    df: DataFrame = tabular.from_columns(names, cols)
    rw: RollingWindow = df.rolling(3i64).min_periods(1i64)
    out: DataFrame = rw.sum()
    col: ColumnI64? = out.get_column_i64(\"a\")
    if col is not none:
        v0: i64? = col.get(0i64)
        v1: i64? = col.get(1i64)
        v2: i64? = col.get(2i64)
        if v0 is not none:
            println(\"v0=\" + str(v0))
        if v1 is not none:
            println(\"v1=\" + str(v1))
        if v2 is not none:
            println(\"v2=\" + str(v2))
    return 0
";
    let out = run("rolling_min_periods", src);
    assert!(out.contains("v0=1"), "got: {out:?}");
    assert!(out.contains("v1=3"), "got: {out:?}");
    assert!(out.contains("v2=6"), "got: {out:?}");
}

#[test]
fn rolling_sum_i64_stays_i64() {
    // sum on i64 stays ColumnI64 (get_column_i64 succeeds).
    let src = "\
from tabular import Column, ColumnI64, DataFrame, RollingWindow
import tabular
fn main() -> i32:
    a: List[i64] = []
    a.append(10i64)
    a.append(20i64)
    a.append(30i64)
    ca: ColumnI64 = tabular.col_i64_simple(a)
    names: List[str] = []
    names.append(\"a\")
    cols: List[Column] = []
    cols.append(ca)
    df: DataFrame = tabular.from_columns(names, cols)
    out: DataFrame = df.rolling(2i64).sum()
    col: ColumnI64? = out.get_column_i64(\"a\")
    if col is not none:
        println(\"is_i64=yes\")
        v1: i64? = col.get(1i64)
        if v1 is not none:
            println(\"v1=\" + str(v1))
    return 0
";
    let out = run("rolling_sum_i64", src);
    assert!(out.contains("is_i64=yes"), "got: {out:?}");
    assert!(out.contains("v1=30"), "got: {out:?}");
}

#[test]
fn rolling_std_always_f64() {
    // std on i64 column -> ColumnF64.
    let src = "\
from tabular import Column, ColumnI64, ColumnF64, DataFrame, RollingWindow
import tabular
fn main() -> i32:
    a: List[i64] = []
    a.append(2i64)
    a.append(4i64)
    a.append(6i64)
    ca: ColumnI64 = tabular.col_i64_simple(a)
    names: List[str] = []
    names.append(\"a\")
    cols: List[Column] = []
    cols.append(ca)
    df: DataFrame = tabular.from_columns(names, cols)
    out: DataFrame = df.rolling(3i64).std()
    col: ColumnF64? = out.get_column_f64(\"a\")
    if col is not none:
        println(\"is_f64=yes\")
        v2: f64? = col.get(2i64)
        if v2 is not none:
            println(\"v2=\" + str(v2))
    return 0
";
    let out = run("rolling_std_f64", src);
    assert!(out.contains("is_f64=yes"), "got: {out:?}");
    // sample std of [2,4,6] = 2.0
    assert!(out.contains("v2=2.0"), "got: {out:?}");
}

#[test]
fn rolling_min_max_datetime_supported() {
    // min/max keep a ColumnDateTime column (output ColumnDateTime).
    let src = "\
from tabular import Column, ColumnI64, ColumnDateTime, DataFrame, RollingWindow
import tabular
fn main() -> i32:
    ts: List[i64] = []
    ts.append(100i64)
    ts.append(200i64)
    ts.append(300i64)
    tn: List[bool] = []
    tn.append(false)
    tn.append(false)
    tn.append(false)
    ct: ColumnDateTime = tabular.col_datetime(ts, tn)
    names: List[str] = []
    names.append(\"t\")
    cols: List[Column] = []
    cols.append(ct)
    df: DataFrame = tabular.from_columns(names, cols)
    out: DataFrame = df.rolling(2i64).max()
    col: ColumnDateTime? = out.get_column_datetime(\"t\")
    if col is not none:
        println(\"is_dt=yes\")
        v1: i64? = col.get_ms(1i64)
        if v1 is not none:
            println(\"v1=\" + str(v1))
    return 0
";
    let out = run("rolling_minmax_dt", src);
    assert!(out.contains("is_dt=yes"), "got: {out:?}");
    assert!(out.contains("v1=200"), "got: {out:?}");
}

#[test]
fn rolling_agg_two_columns() {
    // .agg([("a","mean"),("b","sum")]) -> 2 columns named a_mean, b_sum.
    let src = "\
from tabular import Column, ColumnI64, DataFrame, RollingWindow
import tabular
fn main() -> i32:
    a: List[i64] = []
    a.append(1i64)
    a.append(2i64)
    a.append(3i64)
    b: List[i64] = []
    b.append(10i64)
    b.append(20i64)
    b.append(30i64)
    ca: ColumnI64 = tabular.col_i64_simple(a)
    cb: ColumnI64 = tabular.col_i64_simple(b)
    names: List[str] = []
    names.append(\"a\")
    names.append(\"b\")
    cols: List[Column] = []
    cols.append(ca)
    cols.append(cb)
    df: DataFrame = tabular.from_columns(names, cols)
    specs: List[Tuple[str, str]] = []
    specs.append((\"a\", \"mean\"))
    specs.append((\"b\", \"sum\"))
    out: DataFrame = df.rolling(2i64).agg(specs)
    println(\"ncols=\" + str(out.ncols()))
    println(\"has_a_mean=\" + str(out.has_column(\"a_mean\")))
    println(\"has_b_sum=\" + str(out.has_column(\"b_sum\")))
    return 0
";
    let out = run("rolling_agg", src);
    assert!(out.contains("ncols=2"), "got: {out:?}");
    assert!(out.contains("has_a_mean=true"), "got: {out:?}");
    assert!(out.contains("has_b_sum=true"), "got: {out:?}");
}

#[test]
fn rolling_preserves_single_col_index() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame, RollingWindow
import tabular
fn main() -> i32:
    idx: List[i64] = []
    idx.append(100i64)
    idx.append(200i64)
    idx.append(300i64)
    a: List[i64] = []
    a.append(1i64)
    a.append(2i64)
    a.append(3i64)
    cidx: ColumnI64 = tabular.col_i64_simple(idx)
    ca: ColumnI64 = tabular.col_i64_simple(a)
    names: List[str] = []
    names.append(\"id\")
    names.append(\"a\")
    cols: List[Column] = []
    cols.append(cidx)
    cols.append(ca)
    df: DataFrame = tabular.from_columns(names, cols)
    df2: DataFrame = df.set_index(\"id\")
    out: DataFrame = df2.rolling(2i64).mean()
    println(\"has_index=\" + str(out.has_index()))
    println(\"nrows=\" + str(out.length()))
    return 0
";
    let out = run("rolling_index_single", src);
    assert!(out.contains("has_index=true"), "got: {out:?}");
    assert!(out.contains("nrows=3"), "got: {out:?}");
}

#[test]
fn rolling_preserves_multiindex() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame, RollingWindow
import tabular
fn main() -> i32:
    o: List[str] = []
    o.append(\"x\")
    o.append(\"x\")
    o.append(\"y\")
    i: List[i64] = []
    i.append(1i64)
    i.append(2i64)
    i.append(3i64)
    a: List[i64] = []
    a.append(1i64)
    a.append(2i64)
    a.append(3i64)
    co: ColumnStr = tabular.col_str_simple(o)
    ci: ColumnI64 = tabular.col_i64_simple(i)
    ca: ColumnI64 = tabular.col_i64_simple(a)
    names: List[str] = []
    names.append(\"o\")
    names.append(\"i\")
    names.append(\"a\")
    cols: List[Column] = []
    cols.append(co)
    cols.append(ci)
    cols.append(ca)
    df: DataFrame = tabular.from_columns(names, cols)
    idx: List[str] = []
    idx.append(\"o\")
    idx.append(\"i\")
    df2: DataFrame = df.set_index_list(idx)
    out: DataFrame = df2.rolling(2i64).mean()
    println(\"nlevels=\" + str(out.index_nlevels()))
    println(\"nrows=\" + str(out.length()))
    return 0
";
    let out = run("rolling_index_multi", src);
    assert!(out.contains("nlevels=2"), "got: {out:?}");
    assert!(out.contains("nrows=3"), "got: {out:?}");
}

#[test]
fn rolling_drops_non_numeric_columns() {
    // A str column is dropped from .mean() output.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame, RollingWindow
import tabular
fn main() -> i32:
    s: List[str] = []
    s.append(\"p\")
    s.append(\"q\")
    s.append(\"r\")
    a: List[i64] = []
    a.append(1i64)
    a.append(2i64)
    a.append(3i64)
    cs: ColumnStr = tabular.col_str_simple(s)
    ca: ColumnI64 = tabular.col_i64_simple(a)
    names: List[str] = []
    names.append(\"label\")
    names.append(\"a\")
    cols: List[Column] = []
    cols.append(cs)
    cols.append(ca)
    df: DataFrame = tabular.from_columns(names, cols)
    out: DataFrame = df.rolling(2i64).mean()
    println(\"ncols=\" + str(out.ncols()))
    println(\"has_label=\" + str(out.has_column(\"label\")))
    println(\"has_a=\" + str(out.has_column(\"a\")))
    return 0
";
    let out = run("rolling_drop_nonnumeric", src);
    assert!(out.contains("ncols=1"), "got: {out:?}");
    assert!(out.contains("has_label=false"), "got: {out:?}");
    assert!(out.contains("has_a=true"), "got: {out:?}");
}

#[test]
fn rolling_zero_window_errors() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame, RollingWindow
import tabular
fn main() -> i32:
    a: List[i64] = []
    a.append(1i64)
    ca: ColumnI64 = tabular.col_i64_simple(a)
    names: List[str] = []
    names.append(\"a\")
    cols: List[Column] = []
    cols.append(ca)
    df: DataFrame = tabular.from_columns(names, cols)
    rw: RollingWindow = df.rolling(0i64)
    println(\"unreached\")
    return 0
";
    let p = compile_snippet("rolling_zero_window", src);
    let res = run_file_capture(&p);
    assert!(res.is_err(), "expected uncaught ValueError; got {res:?}");
}

#[test]
fn rolling_zero_min_periods_errors() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame, RollingWindow
import tabular
fn main() -> i32:
    a: List[i64] = []
    a.append(1i64)
    ca: ColumnI64 = tabular.col_i64_simple(a)
    names: List[str] = []
    names.append(\"a\")
    cols: List[Column] = []
    cols.append(ca)
    df: DataFrame = tabular.from_columns(names, cols)
    rw: RollingWindow = df.rolling(2i64).min_periods(0i64)
    println(\"unreached\")
    return 0
";
    let p = compile_snippet("rolling_zero_minp", src);
    let res = run_file_capture(&p);
    assert!(res.is_err(), "expected uncaught ValueError; got {res:?}");
}

// ──────────────────────────────────────────────────────────────────
// Phase B: explicit is_ordered bit
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
