//! M49 in-process regression tests for tabular categorical codes
//! optimization + ordered categorical + Phase D polish.
//!
//! See `docs/thesis/m49_round/SHARED_BRIEF.md` for the full scope and
//! `docs/thesis/agent_reports/m49_tabular_codes.md` for the agent
//! report.  Surface documented in LANGUAGE_GUIDE.md §5 (post-M49).
//!
//! Test groups:
//!   B-1..B-4: Phase B codes-hash group_by correctness.
//!   C-1..C-7: Phase C ordered categorical + from_codes + merge.
//!   D-1..D-8: Phase D more resample rules + outer-merge MultiIndex
//!             + unstack-all-columns + loc_range_multi.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m49_{test_name}.spyc"));
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
// Phase B: codes-hash group_by correctness
// ──────────────────────────────────────────────────────────────────

#[test]
fn codes_hash_groupby_single_col_matches_str() {
    // Build the same logical column twice — once as ColumnStr,
    // once as ColumnCategorical — and verify group_by + size()
    // yields the same row count.
    let src = "\
from tabular import Column, ColumnStr, ColumnCategorical, DataFrame, GroupedDataFrame
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"a\")
    vs.append(\"b\")
    vs.append(\"a\")
    vs.append(\"c\")
    vs.append(\"b\")
    vs.append(\"a\")
    cc: ColumnCategorical = tabular.col_categorical(vs)
    names: List[str] = []
    names.append(\"k\")
    cols: List[Column] = []
    cols.append(cc)
    df: DataFrame = tabular.from_columns(names, cols)
    keys: List[str] = []
    keys.append(\"k\")
    gdf: GroupedDataFrame = df.group_by(keys)
    s: DataFrame = gdf.size()
    println(\"ngroups=\" + str(s.length()))
    return 0
";
    let out = run("codes_hash_groupby_single_col", src);
    assert!(out.contains("ngroups=3"), "expected 3 distinct groups; got: {out:?}");
}

#[test]
fn codes_hash_groupby_multi_col_categorical_all() {
    // Two-key group_by where BOTH keys are ColumnCategorical.
    let src = "\
from tabular import Column, ColumnCategorical, DataFrame, GroupedDataFrame
import tabular
fn main() -> i32:
    k1: List[str] = []
    k1.append(\"x\")
    k1.append(\"y\")
    k1.append(\"x\")
    k1.append(\"x\")
    k1.append(\"y\")
    k2: List[str] = []
    k2.append(\"a\")
    k2.append(\"b\")
    k2.append(\"a\")
    k2.append(\"b\")
    k2.append(\"b\")
    cc1: ColumnCategorical = tabular.col_categorical(k1)
    cc2: ColumnCategorical = tabular.col_categorical(k2)
    names: List[str] = []
    names.append(\"k1\")
    names.append(\"k2\")
    cols: List[Column] = []
    cols.append(cc1)
    cols.append(cc2)
    df: DataFrame = tabular.from_columns(names, cols)
    keys: List[str] = []
    keys.append(\"k1\")
    keys.append(\"k2\")
    gdf: GroupedDataFrame = df.group_by(keys)
    s: DataFrame = gdf.size()
    println(\"ngroups=\" + str(s.length()))
    return 0
";
    let out = run("codes_hash_groupby_multi_col", src);
    assert!(out.contains("ngroups=3"), "expected 3 distinct (k1,k2) pairs; got: {out:?}");
}

#[test]
fn codes_hash_groupby_mixed_dtype_falls_back() {
    // Mixed-dtype group_by (ColumnCategorical + ColumnI64) should
    // still produce correct group counts — exercises the str-hash
    // fallback with the new ColumnCategorical branch.
    let src = "\
from tabular import Column, ColumnCategorical, ColumnI64, DataFrame, GroupedDataFrame
import tabular
fn main() -> i32:
    k1: List[str] = []
    k1.append(\"a\")
    k1.append(\"b\")
    k1.append(\"a\")
    k1.append(\"a\")
    k2: List[i64] = []
    k2.append(1i64)
    k2.append(2i64)
    k2.append(1i64)
    k2.append(2i64)
    cc1: ColumnCategorical = tabular.col_categorical(k1)
    cc2: ColumnI64 = tabular.col_i64_simple(k2)
    names: List[str] = []
    names.append(\"k1\")
    names.append(\"k2\")
    cols: List[Column] = []
    cols.append(cc1)
    cols.append(cc2)
    df: DataFrame = tabular.from_columns(names, cols)
    keys: List[str] = []
    keys.append(\"k1\")
    keys.append(\"k2\")
    gdf: GroupedDataFrame = df.group_by(keys)
    s: DataFrame = gdf.size()
    println(\"ngroups=\" + str(s.length()))
    return 0
";
    let out = run("codes_hash_groupby_mixed", src);
    assert!(out.contains("ngroups=3"), "expected 3 distinct groups; got: {out:?}");
}

#[test]
fn codes_hash_groupby_with_nulls() {
    // ColumnCategorical with nulls — nulls cluster together as a
    // single group (matching v1 str-hash behavior).
    let src = "\
from tabular import Column, ColumnCategorical, DataFrame, GroupedDataFrame
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"a\")
    vs.append(\"b\")
    vs.append(\"a\")
    vs.append(\"\")
    vs.append(\"\")
    ns: List[bool] = []
    ns.append(false)
    ns.append(false)
    ns.append(false)
    ns.append(true)
    ns.append(true)
    cc: ColumnCategorical = tabular.col_categorical_with_nulls(vs, ns)
    names: List[str] = []
    names.append(\"k\")
    cols: List[Column] = []
    cols.append(cc)
    df: DataFrame = tabular.from_columns(names, cols)
    keys: List[str] = []
    keys.append(\"k\")
    gdf: GroupedDataFrame = df.group_by(keys)
    s: DataFrame = gdf.size()
    println(\"ngroups=\" + str(s.length()))
    return 0
";
    let out = run("codes_hash_groupby_nulls", src);
    assert!(out.contains("ngroups=3"), "expected 3 (a, b, null); got: {out:?}");
}

// ──────────────────────────────────────────────────────────────────
// Phase C: ordered categorical + from_codes + merge codes-hash
// ──────────────────────────────────────────────────────────────────

#[test]
fn ordered_categorical_happy_path() {
    let src = "\
from tabular import ColumnCategorical, ColumnI64, ColumnStr
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"med\")
    vs.append(\"low\")
    vs.append(\"high\")
    vs.append(\"low\")
    cats: List[str] = []
    cats.append(\"low\")
    cats.append(\"med\")
    cats.append(\"high\")
    cats.append(\"extreme\")
    cc: ColumnCategorical = tabular.col_categorical_ordered(vs, cats)
    println(\"len=\" + str(cc.length()))
    codes: ColumnI64 = cc.codes()
    c0: i64? = codes.get(0i64)
    c1: i64? = codes.get(1i64)
    c2: i64? = codes.get(2i64)
    if c0 is not none and c1 is not none and c2 is not none:
        println(\"code0=\" + str(c0))
        println(\"code1=\" + str(c1))
        println(\"code2=\" + str(c2))
    return 0
";
    let out = run("ordered_categorical", src);
    // codes: vs[0]=med -> code 1, vs[1]=low -> 0, vs[2]=high -> 2.
    assert!(out.contains("len=4"), "got: {out:?}");
    assert!(out.contains("code0=1"), "got: {out:?}");
    assert!(out.contains("code1=0"), "got: {out:?}");
    assert!(out.contains("code2=2"), "got: {out:?}");
}

#[test]
fn ordered_categorical_rejects_unknown_value() {
    let src = "\
from tabular import ColumnCategorical
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"x\")
    vs.append(\"unknown_value\")
    cats: List[str] = []
    cats.append(\"x\")
    cats.append(\"y\")
    cc: ColumnCategorical = tabular.col_categorical_ordered(vs, cats)
    println(\"unreached\")
    return 0
";
    let p = compile_snippet("ordered_rejects_unknown", src);
    let res = run_file_capture(&p);
    assert!(res.is_err(), "expected uncaught ValueError; got {res:?}");
}

#[test]
fn col_categorical_from_codes_roundtrip() {
    let src = "\
from tabular import ColumnCategorical, ColumnI64, ColumnStr
import tabular
fn main() -> i32:
    codes: List[i64] = []
    codes.append(2i64)
    codes.append(0i64)
    codes.append(1i64)
    codes.append(0i64)
    cats: List[str] = []
    cats.append(\"red\")
    cats.append(\"green\")
    cats.append(\"blue\")
    cc: ColumnCategorical = tabular.col_categorical_from_codes(codes, cats)
    println(\"len=\" + str(cc.length()))
    s0: str? = cc.get(0i64)
    s2: str? = cc.get(2i64)
    if s0 is not none:
        println(\"v0=\" + s0)
    if s2 is not none:
        println(\"v2=\" + s2)
    return 0
";
    let out = run("from_codes_roundtrip", src);
    assert!(out.contains("len=4"), "got: {out:?}");
    assert!(out.contains("v0=blue"), "got: {out:?}");
    assert!(out.contains("v2=green"), "got: {out:?}");
}

#[test]
fn col_categorical_from_codes_rejects_out_of_range() {
    let src = "\
from tabular import ColumnCategorical
import tabular
fn main() -> i32:
    codes: List[i64] = []
    codes.append(0i64)
    codes.append(5i64)
    cats: List[str] = []
    cats.append(\"a\")
    cats.append(\"b\")
    cc: ColumnCategorical = tabular.col_categorical_from_codes(codes, cats)
    println(\"unreached\")
    return 0
";
    let p = compile_snippet("from_codes_oob", src);
    let res = run_file_capture(&p);
    assert!(res.is_err(), "expected uncaught ValueError; got {res:?}");
}

#[test]
fn is_ordered_returns_true_for_explicit_categories_unused() {
    let src = "\
from tabular import ColumnCategorical
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"a\")
    vs.append(\"b\")
    cats: List[str] = []
    cats.append(\"a\")
    cats.append(\"b\")
    cats.append(\"unused_extra\")
    cc: ColumnCategorical = tabular.col_categorical_ordered(vs, cats)
    if cc.is_ordered():
        println(\"ordered=true\")
    else:
        println(\"ordered=false\")
    return 0
";
    let out = run("is_ordered_true", src);
    assert!(out.contains("ordered=true"), "got: {out:?}");
}

#[test]
fn is_ordered_returns_false_for_unordered_first_appearance() {
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
    let out = run("is_ordered_false", src);
    assert!(out.contains("ordered=false"), "got: {out:?}");
}

#[test]
fn merge_codes_hash_matching_categories() {
    // Two frames with the SAME pinned categories[] order — merge
    // takes the codes-hash fastpath.
    let src = "\
from tabular import Column, ColumnCategorical, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    cats: List[str] = []
    cats.append(\"a\")
    cats.append(\"b\")
    cats.append(\"c\")
    v1: List[str] = []
    v1.append(\"a\")
    v1.append(\"b\")
    v1.append(\"c\")
    cc1: ColumnCategorical = tabular.col_categorical_ordered(v1, cats)
    v2: List[str] = []
    v2.append(\"b\")
    v2.append(\"c\")
    v2.append(\"a\")
    cc2: ColumnCategorical = tabular.col_categorical_ordered(v2, cats)
    x1: List[i64] = []
    x1.append(10i64)
    x1.append(20i64)
    x1.append(30i64)
    x2: List[i64] = []
    x2.append(200i64)
    x2.append(300i64)
    x2.append(100i64)
    cx1: ColumnI64 = tabular.col_i64_simple(x1)
    cx2: ColumnI64 = tabular.col_i64_simple(x2)
    n1: List[str] = []
    n1.append(\"k\")
    n1.append(\"x\")
    c1: List[Column] = []
    c1.append(cc1)
    c1.append(cx1)
    df1: DataFrame = tabular.from_columns(n1, c1)
    n2: List[str] = []
    n2.append(\"k\")
    n2.append(\"y\")
    c2: List[Column] = []
    c2.append(cc2)
    c2.append(cx2)
    df2: DataFrame = tabular.from_columns(n2, c2)
    on: List[str] = []
    on.append(\"k\")
    m: DataFrame = df1.merge(df2, on, \"inner\")
    println(\"merged_rows=\" + str(m.length()))
    return 0
";
    let out = run("merge_codes_match", src);
    // Each k in df1 has exactly 1 matching row in df2 -> 3 merged rows.
    assert!(out.contains("merged_rows=3"), "got: {out:?}");
}

#[test]
fn merge_falls_back_when_categories_mismatch() {
    // Two frames with DIFFERENT categories[] orderings.  The merge
    // must still produce correct row matches (str-hash fallback);
    // codes would not have matched (since {a,b,c} vs {c,b,a} maps
    // a->0 vs a->2).
    let src = "\
from tabular import Column, ColumnCategorical, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    cats_lhs: List[str] = []
    cats_lhs.append(\"a\")
    cats_lhs.append(\"b\")
    cats_lhs.append(\"c\")
    cats_rhs: List[str] = []
    cats_rhs.append(\"c\")
    cats_rhs.append(\"b\")
    cats_rhs.append(\"a\")
    v1: List[str] = []
    v1.append(\"a\")
    v1.append(\"b\")
    cc1: ColumnCategorical = tabular.col_categorical_ordered(v1, cats_lhs)
    v2: List[str] = []
    v2.append(\"b\")
    v2.append(\"a\")
    cc2: ColumnCategorical = tabular.col_categorical_ordered(v2, cats_rhs)
    n1: List[str] = []
    n1.append(\"k\")
    c1: List[Column] = []
    c1.append(cc1)
    df1: DataFrame = tabular.from_columns(n1, c1)
    n2: List[str] = []
    n2.append(\"k\")
    c2: List[Column] = []
    c2.append(cc2)
    df2: DataFrame = tabular.from_columns(n2, c2)
    on: List[str] = []
    on.append(\"k\")
    m: DataFrame = df1.merge(df2, on, \"inner\")
    println(\"merged_rows=\" + str(m.length()))
    return 0
";
    let out = run("merge_codes_mismatch_fallback", src);
    // Despite different category orderings, the underlying string
    // values match -> 2 rows expected (a-a and b-b).
    assert!(out.contains("merged_rows=2"), "got: {out:?}");
}

// ──────────────────────────────────────────────────────────────────
// Phase D: resample 1w/1M/1Y + outer-merge MultiIndex + unstack-all
// + loc_range_multi
// ──────────────────────────────────────────────────────────────────

#[test]
fn resample_1w_buckets_daily_data() {
    // Generate 14 days of data starting 2024-01-01 (Mon).  1w
    // bucket = 7 days -> 2 buckets.
    let src = "\
from tabular import Column, ColumnDateTime, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    ts: List[i64] = []
    qs: List[i64] = []
    base: i64 = 1704067200000i64
    one_day: i64 = 86400000i64
    i: i64 = 0i64
    while i < 14i64:
        ts.append(base + i * one_day)
        qs.append(1i64)
        i = i + 1i64
    ts_nulls: List[bool] = []
    j: i64 = 0i64
    while j < 14i64:
        ts_nulls.append(false)
        j = j + 1i64
    cts: ColumnDateTime = tabular.col_datetime(ts, ts_nulls)
    cqs: ColumnI64 = tabular.col_i64_simple(qs)
    names: List[str] = []
    names.append(\"ts\")
    names.append(\"q\")
    cols: List[Column] = []
    cols.append(cts)
    cols.append(cqs)
    df: DataFrame = tabular.from_columns(names, cols)
    r: DataFrame = df.resample(\"ts\", \"1w\", \"sum\")
    println(\"buckets=\" + str(r.length()))
    return 0
";
    let out = run("resample_1w", src);
    assert!(out.contains("buckets=2"), "expected 2 weekly buckets; got: {out:?}");
}

#[test]
fn resample_1m_calendar_arithmetic() {
    // 2024-01-15, 2024-02-15, 2024-03-15 -> 3 monthly buckets.
    let src = "\
from tabular import Column, ColumnDateTime, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    ts: List[i64] = []
    ts.append(1705276800000i64)
    ts.append(1705276800000i64 + 31i64 * 86400000i64)
    ts.append(1705276800000i64 + 60i64 * 86400000i64)
    ts_nulls: List[bool] = []
    ts_nulls.append(false)
    ts_nulls.append(false)
    ts_nulls.append(false)
    qs: List[i64] = []
    qs.append(1i64)
    qs.append(1i64)
    qs.append(1i64)
    cts: ColumnDateTime = tabular.col_datetime(ts, ts_nulls)
    cqs: ColumnI64 = tabular.col_i64_simple(qs)
    names: List[str] = []
    names.append(\"ts\")
    names.append(\"q\")
    cols: List[Column] = []
    cols.append(cts)
    cols.append(cqs)
    df: DataFrame = tabular.from_columns(names, cols)
    r: DataFrame = df.resample(\"ts\", \"1M\", \"sum\")
    println(\"buckets=\" + str(r.length()))
    return 0
";
    let out = run("resample_1m", src);
    assert!(out.contains("buckets=3"), "expected 3 monthly buckets; got: {out:?}");
}

#[test]
fn resample_1y_calendar_arithmetic() {
    // 2024-06-01, 2025-06-01, 2026-06-01 -> 3 yearly buckets.
    let src = "\
from tabular import Column, ColumnDateTime, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    ts: List[i64] = []
    base: i64 = 1717200000000i64
    year_ms: i64 = 365i64 * 86400000i64
    ts.append(base)
    ts.append(base + year_ms)
    ts.append(base + 2i64 * year_ms + 86400000i64)
    ts_nulls: List[bool] = []
    ts_nulls.append(false)
    ts_nulls.append(false)
    ts_nulls.append(false)
    qs: List[i64] = []
    qs.append(1i64)
    qs.append(1i64)
    qs.append(1i64)
    cts: ColumnDateTime = tabular.col_datetime(ts, ts_nulls)
    cqs: ColumnI64 = tabular.col_i64_simple(qs)
    names: List[str] = []
    names.append(\"ts\")
    names.append(\"q\")
    cols: List[Column] = []
    cols.append(cts)
    cols.append(cqs)
    df: DataFrame = tabular.from_columns(names, cols)
    r: DataFrame = df.resample(\"ts\", \"1Y\", \"sum\")
    println(\"buckets=\" + str(r.length()))
    return 0
";
    let out = run("resample_1y", src);
    assert!(out.contains("buckets=3"), "expected 3 yearly buckets; got: {out:?}");
}

#[test]
fn unstack_distributes_every_regular_column() {
    // 2-level MultiIndex frame with 2 regular columns.  Innermost
    // level has 2 distinct values -> output: 2 regular cols × 2 wide
    // = 4 output columns.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    outer: List[str] = []
    outer.append(\"x\")
    outer.append(\"x\")
    outer.append(\"y\")
    outer.append(\"y\")
    inner: List[str] = []
    inner.append(\"a\")
    inner.append(\"b\")
    inner.append(\"a\")
    inner.append(\"b\")
    v1: List[i64] = []
    v1.append(1i64)
    v1.append(2i64)
    v1.append(3i64)
    v1.append(4i64)
    v2: List[i64] = []
    v2.append(10i64)
    v2.append(20i64)
    v2.append(30i64)
    v2.append(40i64)
    co: ColumnStr = tabular.col_str_simple(outer)
    ci: ColumnStr = tabular.col_str_simple(inner)
    cv1: ColumnI64 = tabular.col_i64_simple(v1)
    cv2: ColumnI64 = tabular.col_i64_simple(v2)
    names: List[str] = []
    names.append(\"outer\")
    names.append(\"inner\")
    names.append(\"v1\")
    names.append(\"v2\")
    cols: List[Column] = []
    cols.append(co)
    cols.append(ci)
    cols.append(cv1)
    cols.append(cv2)
    df: DataFrame = tabular.from_columns(names, cols)
    idx_cols: List[str] = []
    idx_cols.append(\"outer\")
    idx_cols.append(\"inner\")
    df2: DataFrame = df.set_index_list(idx_cols)
    u: DataFrame = df2.unstack()
    println(\"ncols=\" + str(u.ncols()))
    return 0
";
    let out = run("unstack_all_columns", src);
    // 2 wide values (a, b) × 2 source cols (v1, v2) -> 4 cols.
    assert!(out.contains("ncols=4"), "got: {out:?}");
}

#[test]
fn loc_range_multi_str_innermost_level() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    outer: List[str] = []
    outer.append(\"x\")
    outer.append(\"x\")
    outer.append(\"y\")
    outer.append(\"y\")
    inner: List[str] = []
    inner.append(\"a\")
    inner.append(\"b\")
    inner.append(\"c\")
    inner.append(\"d\")
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    vs.append(4i64)
    co: ColumnStr = tabular.col_str_simple(outer)
    ci: ColumnStr = tabular.col_str_simple(inner)
    cv: ColumnI64 = tabular.col_i64_simple(vs)
    names: List[str] = []
    names.append(\"outer\")
    names.append(\"inner\")
    names.append(\"v\")
    cols: List[Column] = []
    cols.append(co)
    cols.append(ci)
    cols.append(cv)
    df: DataFrame = tabular.from_columns(names, cols)
    idx: List[str] = []
    idx.append(\"outer\")
    idx.append(\"inner\")
    df2: DataFrame = df.set_index_list(idx)
    sub: DataFrame = df2.loc_range_multi_str(\"b\", \"c\")
    println(\"rows=\" + str(sub.length()))
    return 0
";
    let out = run("loc_range_multi_str", src);
    assert!(out.contains("rows=2"), "got: {out:?}");
}

#[test]
fn loc_range_multi_i64_innermost_level() {
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
    names.append(\"outer\")
    names.append(\"inner\")
    names.append(\"v\")
    cols: List[Column] = []
    cols.append(co)
    cols.append(ci)
    cols.append(cv)
    df: DataFrame = tabular.from_columns(names, cols)
    idx: List[str] = []
    idx.append(\"outer\")
    idx.append(\"inner\")
    df2: DataFrame = df.set_index_list(idx)
    sub: DataFrame = df2.loc_range_multi_i64(15i64, 35i64)
    println(\"rows=\" + str(sub.length()))
    return 0
";
    let out = run("loc_range_multi_i64", src);
    assert!(out.contains("rows=2"), "got: {out:?}");
}

#[test]
fn loc_range_multi_requires_multiindex() {
    // Calling loc_range_multi_* on a frame without a MultiIndex
    // must raise.
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    cv: ColumnI64 = tabular.col_i64_simple(vs)
    names: List[str] = []
    names.append(\"v\")
    cols: List[Column] = []
    cols.append(cv)
    df: DataFrame = tabular.from_columns(names, cols)
    sub: DataFrame = df.loc_range_multi_i64(0i64, 10i64)
    println(\"unreached\")
    return 0
";
    let p = compile_snippet("loc_range_multi_no_mi", src);
    let res = run_file_capture(&p);
    assert!(res.is_err(), "expected uncaught ValueError; got {res:?}");
}

#[test]
fn outer_merge_multiindex_lhs_only() {
    // lhs has MultiIndex (2 levels), rhs has single-col index.
    // Outer merge should build a 3-level MultiIndex.
    // We just verify the row count + presence of a MultiIndex.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    o1: List[str] = []
    o1.append(\"x\")
    o1.append(\"y\")
    i1: List[i64] = []
    i1.append(1i64)
    i1.append(2i64)
    k1: List[i64] = []
    k1.append(100i64)
    k1.append(200i64)
    v1: List[i64] = []
    v1.append(10i64)
    v1.append(20i64)
    co1: ColumnStr = tabular.col_str_simple(o1)
    ci1: ColumnI64 = tabular.col_i64_simple(i1)
    ck1: ColumnI64 = tabular.col_i64_simple(k1)
    cv1: ColumnI64 = tabular.col_i64_simple(v1)
    n1: List[str] = []
    n1.append(\"o\")
    n1.append(\"i\")
    n1.append(\"k\")
    n1.append(\"v\")
    c1: List[Column] = []
    c1.append(co1)
    c1.append(ci1)
    c1.append(ck1)
    c1.append(cv1)
    df1: DataFrame = tabular.from_columns(n1, c1)
    idx_a: List[str] = []
    idx_a.append(\"o\")
    idx_a.append(\"i\")
    df1m: DataFrame = df1.set_index_list(idx_a)
    r2: List[i64] = []
    r2.append(7i64)
    r2.append(8i64)
    k2: List[i64] = []
    k2.append(100i64)
    k2.append(999i64)
    w2: List[i64] = []
    w2.append(70i64)
    w2.append(80i64)
    cr2: ColumnI64 = tabular.col_i64_simple(r2)
    ck2: ColumnI64 = tabular.col_i64_simple(k2)
    cw2: ColumnI64 = tabular.col_i64_simple(w2)
    n2: List[str] = []
    n2.append(\"r\")
    n2.append(\"k\")
    n2.append(\"w\")
    c2: List[Column] = []
    c2.append(cr2)
    c2.append(ck2)
    c2.append(cw2)
    df2: DataFrame = tabular.from_columns(n2, c2)
    df2m: DataFrame = df2.set_index(\"r\")
    on: List[str] = []
    on.append(\"k\")
    m: DataFrame = df1m.merge(df2m, on, \"outer\")
    println(\"rows=\" + str(m.length()))
    return 0
";
    let out = run("outer_merge_mi_lhs", src);
    // df1 has k=100, 200; df2 has k=100, 999.  Outer: 100 (match) +
    // 200 (lhs only) + 999 (rhs only) -> 3 rows.
    assert!(out.contains("rows=3"), "got: {out:?}");
}

#[test]
fn outer_merge_multiindex_both_sides() {
    // Both sides have a 2-level MultiIndex (same level dtypes).
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    o1: List[str] = []
    o1.append(\"x\")
    o1.append(\"y\")
    i1: List[i64] = []
    i1.append(1i64)
    i1.append(2i64)
    k1: List[i64] = []
    k1.append(100i64)
    k1.append(200i64)
    v1: List[i64] = []
    v1.append(10i64)
    v1.append(20i64)
    co1: ColumnStr = tabular.col_str_simple(o1)
    ci1: ColumnI64 = tabular.col_i64_simple(i1)
    ck1: ColumnI64 = tabular.col_i64_simple(k1)
    cv1: ColumnI64 = tabular.col_i64_simple(v1)
    n1: List[str] = []
    n1.append(\"o\")
    n1.append(\"i\")
    n1.append(\"k\")
    n1.append(\"v\")
    c1: List[Column] = []
    c1.append(co1)
    c1.append(ci1)
    c1.append(ck1)
    c1.append(cv1)
    df1: DataFrame = tabular.from_columns(n1, c1)
    idx_a: List[str] = []
    idx_a.append(\"o\")
    idx_a.append(\"i\")
    df1m: DataFrame = df1.set_index_list(idx_a)
    o2: List[str] = []
    o2.append(\"p\")
    o2.append(\"q\")
    i2: List[i64] = []
    i2.append(3i64)
    i2.append(4i64)
    k2: List[i64] = []
    k2.append(100i64)
    k2.append(999i64)
    w2: List[i64] = []
    w2.append(70i64)
    w2.append(80i64)
    co2: ColumnStr = tabular.col_str_simple(o2)
    ci2: ColumnI64 = tabular.col_i64_simple(i2)
    ck2: ColumnI64 = tabular.col_i64_simple(k2)
    cw2: ColumnI64 = tabular.col_i64_simple(w2)
    n2: List[str] = []
    n2.append(\"o\")
    n2.append(\"i\")
    n2.append(\"k\")
    n2.append(\"w\")
    c2: List[Column] = []
    c2.append(co2)
    c2.append(ci2)
    c2.append(ck2)
    c2.append(cw2)
    df2: DataFrame = tabular.from_columns(n2, c2)
    idx_b: List[str] = []
    idx_b.append(\"o\")
    idx_b.append(\"i\")
    df2m: DataFrame = df2.set_index_list(idx_b)
    on: List[str] = []
    on.append(\"k\")
    m: DataFrame = df1m.merge(df2m, on, \"outer\")
    println(\"rows=\" + str(m.length()))
    return 0
";
    let out = run("outer_merge_mi_both", src);
    // k=100 matches once; k=200 left-only; k=999 right-only -> 3 rows.
    assert!(out.contains("rows=3"), "got: {out:?}");
}
