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

// ── Phase B: rolling ─────────────────────────────────────────────────

#[test]
fn rolling_sum_window_3_on_5_rows() {
    // [1, 2, 3, 4, 5] window=3 → [null, null, 6, 9, 12]
    let src = "\
from tabular import ColumnI64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    vs.append(4i64)
    vs.append(5i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    r: ColumnI64 = c.rolling_sum(3i64)
    println(\"nc=\" + str(r.null_count()))
    v2: i64? = r.get(2i64)
    v3: i64? = r.get(3i64)
    v4: i64? = r.get(4i64)
    if v2 is not none:
        println(\"v2=\" + str(v2))
    if v3 is not none:
        println(\"v3=\" + str(v3))
    if v4 is not none:
        println(\"v4=\" + str(v4))
    return 0
";
    let out = run("rolling_sum_3", src);
    assert!(out.contains("nc=2"), "got: {out:?}");
    assert!(out.contains("v2=6"), "got: {out:?}");
    assert!(out.contains("v3=9"), "got: {out:?}");
    assert!(out.contains("v4=12"), "got: {out:?}");
}

#[test]
fn rolling_mean_returns_f64_from_i64() {
    let src = "\
from tabular import ColumnI64, ColumnF64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    r: ColumnF64 = c.rolling_mean(2i64)
    v1: f64? = r.get(1i64)
    if v1 is not none:
        println(\"v1=\" + str(v1))
    v2: f64? = r.get(2i64)
    if v2 is not none:
        println(\"v2=\" + str(v2))
    return 0
";
    let out = run("rolling_mean", src);
    assert!(out.contains("v1=1.5"), "got: {out:?}");
    assert!(out.contains("v2=2.5"), "got: {out:?}");
}

#[test]
fn rolling_std_sanity() {
    // [1, 2, 3, 4, 5] window=3 → sample std on [1,2,3]=1, [2,3,4]=1, [3,4,5]=1.
    let src = "\
from tabular import ColumnI64, ColumnF64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    vs.append(4i64)
    vs.append(5i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    r: ColumnF64 = c.rolling_std(3i64)
    v2: f64? = r.get(2i64)
    if v2 is not none:
        println(\"v2=\" + str(v2))
    return 0
";
    let out = run("rolling_std", src);
    // sample std of [1,2,3] = sqrt(((1-2)^2+(2-2)^2+(3-2)^2)/2) = sqrt(1) = 1
    assert!(out.contains("v2=1"), "got: {out:?}");
}

#[test]
fn rolling_window_too_big_raises() {
    let src = "\
from tabular import ColumnI64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    try:
        r: ColumnI64 = c.rolling_sum(5i64)
        println(\"no-raise\")
    except ValueError:
        println(\"got-ve\")
    return 0
";
    let out = run("rolling_too_big", src);
    assert!(out.contains("got-ve"), "got: {out:?}");
}

#[test]
fn rolling_propagates_input_nulls() {
    // [1, null, 3, 4] window=2 → [null, null, null, 7]
    let src = "\
from tabular import ColumnI64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(0i64)
    vs.append(3i64)
    vs.append(4i64)
    ns: List[bool] = []
    ns.append(false)
    ns.append(true)
    ns.append(false)
    ns.append(false)
    c: ColumnI64 = tabular.col_i64(vs, ns)
    r: ColumnI64 = c.rolling_sum(2i64)
    println(\"nc=\" + str(r.null_count()))
    v3: i64? = r.get(3i64)
    if v3 is not none:
        println(\"v3=\" + str(v3))
    return 0
";
    let out = run("rolling_null", src);
    assert!(out.contains("nc=3"), "got: {out:?}");
    assert!(out.contains("v3=7"), "got: {out:?}");
}

#[test]
fn rolling_max_min_smoke() {
    let src = "\
from tabular import ColumnI64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(3i64)
    vs.append(1i64)
    vs.append(4i64)
    vs.append(1i64)
    vs.append(5i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    rmx: ColumnI64 = c.rolling_max(3i64)
    rmn: ColumnI64 = c.rolling_min(3i64)
    mx2: i64? = rmx.get(2i64)
    mn2: i64? = rmn.get(2i64)
    mx4: i64? = rmx.get(4i64)
    mn4: i64? = rmn.get(4i64)
    if mx2 is not none:
        println(\"mx2=\" + str(mx2))
    if mn2 is not none:
        println(\"mn2=\" + str(mn2))
    if mx4 is not none:
        println(\"mx4=\" + str(mx4))
    if mn4 is not none:
        println(\"mn4=\" + str(mn4))
    return 0
";
    let out = run("rolling_max_min", src);
    assert!(out.contains("mx2=4"), "got: {out:?}");
    assert!(out.contains("mn2=1"), "got: {out:?}");
    assert!(out.contains("mx4=5"), "got: {out:?}");
    assert!(out.contains("mn4=1"), "got: {out:?}");
}

#[test]
fn rolling_sum_f64_smoke() {
    let src = "\
from tabular import ColumnF64
import tabular
fn main() -> i32:
    vs: List[f64] = []
    vs.append(1.0)
    vs.append(2.0)
    vs.append(3.0)
    vs.append(4.0)
    c: ColumnF64 = tabular.col_f64_simple(vs)
    r: ColumnF64 = c.rolling_sum(2i64)
    v1: f64? = r.get(1i64)
    if v1 is not none:
        println(\"v1=\" + str(v1))
    return 0
";
    let out = run("rolling_sum_f64", src);
    assert!(out.contains("v1=3"), "got: {out:?}");
}

// ── Phase C: resample + asof_merge ───────────────────────────────────

#[test]
fn resample_daily_sum() {
    // 5 timestamps spanning 5 days, all with amount=10, daily sum.
    let src = "\
from tabular import Column, ColumnDateTime, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    day_ms: i64 = 86400000i64
    ts_vs: List[i64] = []
    ts_vs.append(0i64)
    ts_vs.append(day_ms)
    ts_vs.append(2i64 * day_ms)
    ts_vs.append(3i64 * day_ms)
    ts_vs.append(4i64 * day_ms)
    tc: ColumnDateTime = tabular.col_datetime(ts_vs, [false, false, false, false, false])
    amt_vs: List[i64] = []
    amt_vs.append(10i64)
    amt_vs.append(10i64)
    amt_vs.append(10i64)
    amt_vs.append(10i64)
    amt_vs.append(10i64)
    ac: ColumnI64 = tabular.col_i64_simple(amt_vs)
    cn: List[str] = []
    cn.append(\"ts\")
    cn.append(\"amt\")
    cs: List[Column] = []
    cs.append(tc)
    cs.append(ac)
    df: DataFrame = tabular.from_columns(cn, cs)
    r: DataFrame = df.resample(\"ts\", \"1d\", \"sum\")
    println(\"rlen=\" + str(r.length()))
    sa: ColumnI64? = r.get_column_i64(\"amt\")
    if sa is not none:
        v0: i64? = sa.get(0i64)
        v4: i64? = sa.get(4i64)
        if v0 is not none:
            println(\"v0=\" + str(v0))
        if v4 is not none:
            println(\"v4=\" + str(v4))
    return 0
";
    let out = run("resample_daily", src);
    assert!(out.contains("rlen=5"), "got: {out:?}");
    assert!(out.contains("v0=10"), "got: {out:?}");
    assert!(out.contains("v4=10"), "got: {out:?}");
}

#[test]
fn resample_empty_bucket_gets_null() {
    // Timestamps at day 0 and day 3 (no day 1 or 2 data).
    let src = "\
from tabular import Column, ColumnDateTime, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    day_ms: i64 = 86400000i64
    ts_vs: List[i64] = []
    ts_vs.append(0i64)
    ts_vs.append(3i64 * day_ms)
    tc: ColumnDateTime = tabular.col_datetime(ts_vs, [false, false])
    amt_vs: List[i64] = []
    amt_vs.append(5i64)
    amt_vs.append(7i64)
    ac: ColumnI64 = tabular.col_i64_simple(amt_vs)
    cn: List[str] = []
    cn.append(\"ts\")
    cn.append(\"amt\")
    cs: List[Column] = []
    cs.append(tc)
    cs.append(ac)
    df: DataFrame = tabular.from_columns(cn, cs)
    r: DataFrame = df.resample(\"ts\", \"1d\", \"sum\")
    println(\"rlen=\" + str(r.length()))
    sa: ColumnI64? = r.get_column_i64(\"amt\")
    if sa is not none:
        println(\"nc=\" + str(sa.null_count()))
    return 0
";
    let out = run("resample_empty", src);
    assert!(out.contains("rlen=4"), "got: {out:?}");
    // Days 1 and 2 are empty buckets → 2 nulls.
    assert!(out.contains("nc=2"), "got: {out:?}");
}

#[test]
fn resample_bad_rule_raises() {
    let src = "\
from tabular import Column, ColumnDateTime, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    ts_vs: List[i64] = []
    ts_vs.append(0i64)
    tc: ColumnDateTime = tabular.col_datetime(ts_vs, [false])
    amt_vs: List[i64] = []
    amt_vs.append(1i64)
    ac: ColumnI64 = tabular.col_i64_simple(amt_vs)
    cn: List[str] = []
    cn.append(\"ts\")
    cn.append(\"amt\")
    cs: List[Column] = []
    cs.append(tc)
    cs.append(ac)
    df: DataFrame = tabular.from_columns(cn, cs)
    try:
        r: DataFrame = df.resample(\"ts\", \"xyz\", \"sum\")
        println(\"no-raise\")
    except ValueError:
        println(\"got-ve\")
    return 0
";
    let out = run("resample_bad_rule", src);
    assert!(out.contains("got-ve"), "got: {out:?}");
}

#[test]
fn asof_merge_happy_path() {
    // Left ts: [10, 20, 30].  Right ts: [5, 15, 25] with vals [a, b, c].
    // Matches: 10→a (5<=10), 20→b (15<=20), 30→c (25<=30).
    let src = "\
from tabular import Column, ColumnDateTime, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    lts_vs: List[i64] = []
    lts_vs.append(10i64)
    lts_vs.append(20i64)
    lts_vs.append(30i64)
    ltc: ColumnDateTime = tabular.col_datetime(lts_vs, [false, false, false])
    lcn: List[str] = []
    lcn.append(\"lts\")
    lcs: List[Column] = []
    lcs.append(ltc)
    lhs: DataFrame = tabular.from_columns(lcn, lcs)
    rts_vs: List[i64] = []
    rts_vs.append(5i64)
    rts_vs.append(15i64)
    rts_vs.append(25i64)
    rtc: ColumnDateTime = tabular.col_datetime(rts_vs, [false, false, false])
    tag_vs: List[str] = []
    tag_vs.append(\"a\")
    tag_vs.append(\"b\")
    tag_vs.append(\"c\")
    tag: ColumnStr = tabular.col_str_simple(tag_vs)
    rcn: List[str] = []
    rcn.append(\"rts\")
    rcn.append(\"tag\")
    rcs: List[Column] = []
    rcs.append(rtc)
    rcs.append(tag)
    rhs: DataFrame = tabular.from_columns(rcn, rcs)
    merged: DataFrame = lhs.asof_merge(rhs, \"lts\", \"rts\")
    println(\"mlen=\" + str(merged.length()))
    println(\"mcols=\" + str(merged.ncols()))
    tg: ColumnStr? = merged.get_column_str(\"tag\")
    if tg is not none:
        t0: str? = tg.get(0i64)
        t1: str? = tg.get(1i64)
        t2: str? = tg.get(2i64)
        if t0 is not none:
            println(\"t0=\" + t0)
        if t1 is not none:
            println(\"t1=\" + t1)
        if t2 is not none:
            println(\"t2=\" + t2)
    return 0
";
    let out = run("asof_happy", src);
    assert!(out.contains("mlen=3"), "got: {out:?}");
    assert!(out.contains("mcols=2"), "got: {out:?}");
    assert!(out.contains("t0=a"), "got: {out:?}");
    assert!(out.contains("t1=b"), "got: {out:?}");
    assert!(out.contains("t2=c"), "got: {out:?}");
}

#[test]
fn asof_merge_no_match_emits_null() {
    // Left ts: [1, 5].  Right ts: [10, 20].  No right row <= 1 or <= 5
    // → both null.
    let src = "\
from tabular import Column, ColumnDateTime, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    lts_vs: List[i64] = []
    lts_vs.append(1i64)
    lts_vs.append(5i64)
    ltc: ColumnDateTime = tabular.col_datetime(lts_vs, [false, false])
    lcn: List[str] = []
    lcn.append(\"lts\")
    lcs: List[Column] = []
    lcs.append(ltc)
    lhs: DataFrame = tabular.from_columns(lcn, lcs)
    rts_vs: List[i64] = []
    rts_vs.append(10i64)
    rts_vs.append(20i64)
    rtc: ColumnDateTime = tabular.col_datetime(rts_vs, [false, false])
    tag_vs: List[str] = []
    tag_vs.append(\"a\")
    tag_vs.append(\"b\")
    tag: ColumnStr = tabular.col_str_simple(tag_vs)
    rcn: List[str] = []
    rcn.append(\"rts\")
    rcn.append(\"tag\")
    rcs: List[Column] = []
    rcs.append(rtc)
    rcs.append(tag)
    rhs: DataFrame = tabular.from_columns(rcn, rcs)
    merged: DataFrame = lhs.asof_merge(rhs, \"lts\", \"rts\")
    tg: ColumnStr? = merged.get_column_str(\"tag\")
    if tg is not none:
        println(\"nc=\" + str(tg.null_count()))
    return 0
";
    let out = run("asof_nomatch", src);
    assert!(out.contains("nc=2"), "got: {out:?}");
}

#[test]
fn asof_merge_dtype_mismatch_raises() {
    let src = "\
from tabular import Column, ColumnDateTime, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    lts_vs: List[i64] = []
    lts_vs.append(1i64)
    ltc: ColumnDateTime = tabular.col_datetime(lts_vs, [false])
    lcn: List[str] = []
    lcn.append(\"lts\")
    lcs: List[Column] = []
    lcs.append(ltc)
    lhs: DataFrame = tabular.from_columns(lcn, lcs)
    rkey_vs: List[i64] = []
    rkey_vs.append(1i64)
    rkey: ColumnI64 = tabular.col_i64_simple(rkey_vs)
    rcn: List[str] = []
    rcn.append(\"rkey\")
    rcs: List[Column] = []
    rcs.append(rkey)
    rhs: DataFrame = tabular.from_columns(rcn, rcs)
    try:
        merged: DataFrame = lhs.asof_merge(rhs, \"lts\", \"rkey\")
        println(\"no-raise\")
    except ValueError:
        println(\"got-ve\")
    return 0
";
    let out = run("asof_dtype", src);
    assert!(out.contains("got-ve"), "got: {out:?}");
}

#[test]
fn asof_merge_i64_keys_work() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    lk_vs: List[i64] = []
    lk_vs.append(10i64)
    lk_vs.append(20i64)
    lk: ColumnI64 = tabular.col_i64_simple(lk_vs)
    lcn: List[str] = []
    lcn.append(\"k\")
    lcs: List[Column] = []
    lcs.append(lk)
    lhs: DataFrame = tabular.from_columns(lcn, lcs)
    rk_vs: List[i64] = []
    rk_vs.append(5i64)
    rk_vs.append(15i64)
    rk: ColumnI64 = tabular.col_i64_simple(rk_vs)
    tag_vs: List[str] = []
    tag_vs.append(\"x\")
    tag_vs.append(\"y\")
    tag: ColumnStr = tabular.col_str_simple(tag_vs)
    rcn: List[str] = []
    rcn.append(\"k\")
    rcn.append(\"tag\")
    rcs: List[Column] = []
    rcs.append(rk)
    rcs.append(tag)
    rhs: DataFrame = tabular.from_columns(rcn, rcs)
    merged: DataFrame = lhs.asof_merge(rhs, \"k\", \"k\")
    tg: ColumnStr? = merged.get_column_str(\"tag\")
    if tg is not none:
        t1: str? = tg.get(1i64)
        if t1 is not none:
            println(\"t1=\" + t1)
    return 0
";
    let out = run("asof_i64", src);
    assert!(out.contains("t1=y"), "got: {out:?}");
}
