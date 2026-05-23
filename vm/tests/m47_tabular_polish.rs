//! M47 in-process regression tests for the `tabular` v0.4 polish round:
//! iloc 2-D + negative iloc + rolling Welford/min_periods +
//! ColumnCategorical dtype.
//!
//! See `docs/thesis/m47_round/SHARED_BRIEF.md` for the full scope.
//! Surface documented in LANGUAGE_GUIDE.md §5 (post-M47).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m47_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

fn run(name: &str, src: &str) -> String {
    let p = compile_snippet(name, src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "{name}: exit={code} stdout={out:?}");
    out
}

// Helper: append values to a List[i64] one per line.
fn make_i64_appends(name: &str, values: &[i64]) -> String {
    let mut s = String::new();
    for v in values {
        s.push_str(&format!("    {}.append({}i64)\n", name, v));
    }
    s
}

// ── Phase A: iloc_2d + negative iloc ──────────────────────────────────

#[test]
fn iloc_2d_happy_path() {
    // 5-row × 3-col frame; iloc_2d(1, 4, 0, 2) = rows [1..4) × cols [0..2)
    // → 3 rows × 2 cols.
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    a: List[i64] = []
    a.append(10i64)
    a.append(20i64)
    a.append(30i64)
    a.append(40i64)
    a.append(50i64)
    b: List[i64] = []
    b.append(11i64)
    b.append(21i64)
    b.append(31i64)
    b.append(41i64)
    b.append(51i64)
    c: List[i64] = []
    c.append(12i64)
    c.append(22i64)
    c.append(32i64)
    c.append(42i64)
    c.append(52i64)
    ca: ColumnI64 = tabular.col_i64_simple(a)
    cb: ColumnI64 = tabular.col_i64_simple(b)
    cc: ColumnI64 = tabular.col_i64_simple(c)
    cn: List[str] = []
    cn.append(\"a\")
    cn.append(\"b\")
    cn.append(\"c\")
    cs: List[Column] = []
    cs.append(ca)
    cs.append(cb)
    cs.append(cc)
    df: DataFrame = tabular.from_columns(cn, cs)
    s: DataFrame = df.iloc_2d(1i64, 4i64, 0i64, 2i64)
    println(\"nrows=\" + str(s.length()))
    println(\"ncols=\" + str(s.ncols()))
    return 0
";
    let out = run("iloc_2d_basic", src);
    assert!(out.contains("nrows=3"), "got: {out:?}");
    assert!(out.contains("ncols=2"), "got: {out:?}");
}

#[test]
fn iloc_2d_negative_indices_both_axes() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    a: List[i64] = []
    a.append(10i64)
    a.append(20i64)
    a.append(30i64)
    a.append(40i64)
    a.append(50i64)
    b: List[i64] = []
    b.append(11i64)
    b.append(21i64)
    b.append(31i64)
    b.append(41i64)
    b.append(51i64)
    c: List[i64] = []
    c.append(12i64)
    c.append(22i64)
    c.append(32i64)
    c.append(42i64)
    c.append(52i64)
    ca: ColumnI64 = tabular.col_i64_simple(a)
    cb: ColumnI64 = tabular.col_i64_simple(b)
    cc: ColumnI64 = tabular.col_i64_simple(c)
    cn: List[str] = []
    cn.append(\"a\")
    cn.append(\"b\")
    cn.append(\"c\")
    cs: List[Column] = []
    cs.append(ca)
    cs.append(cb)
    cs.append(cc)
    df: DataFrame = tabular.from_columns(cn, cs)
    s: DataFrame = df.iloc_2d(-3i64, 5i64, -2i64, 3i64)
    println(\"nrows=\" + str(s.length()))
    println(\"ncols=\" + str(s.ncols()))
    return 0
";
    let out = run("iloc_2d_neg", src);
    assert!(out.contains("nrows=3"), "got: {out:?}");
    assert!(out.contains("ncols=2"), "got: {out:?}");
}

#[test]
fn iloc_2d_clamps_out_of_range() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    a: List[i64] = []
    a.append(1i64)
    a.append(2i64)
    a.append(3i64)
    ca: ColumnI64 = tabular.col_i64_simple(a)
    cn: List[str] = []
    cn.append(\"a\")
    cs: List[Column] = []
    cs.append(ca)
    df: DataFrame = tabular.from_columns(cn, cs)
    s: DataFrame = df.iloc_2d(0i64, 999i64, 0i64, 999i64)
    println(\"nrows=\" + str(s.length()))
    println(\"ncols=\" + str(s.ncols()))
    return 0
";
    let out = run("iloc_2d_clamp", src);
    assert!(out.contains("nrows=3"), "got: {out:?}");
    assert!(out.contains("ncols=1"), "got: {out:?}");
}

#[test]
fn iloc_negative_both_bounds() {
    // iloc(-3, -1) = rows [nrows-3..nrows-1) = 2 rows.
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    a: List[i64] = []
    a.append(1i64)
    a.append(2i64)
    a.append(3i64)
    a.append(4i64)
    a.append(5i64)
    ca: ColumnI64 = tabular.col_i64_simple(a)
    cn: List[str] = []
    cn.append(\"a\")
    cs: List[Column] = []
    cs.append(ca)
    df: DataFrame = tabular.from_columns(cn, cs)
    s: DataFrame = df.iloc(-3i64, -1i64)
    println(\"nrows=\" + str(s.length()))
    return 0
";
    let out = run("iloc_neg_neg", src);
    assert!(out.contains("nrows=2"), "got: {out:?}");
}

#[test]
fn iloc_2d_empty_result_when_bounds_collapse() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    a: List[i64] = []
    a.append(1i64)
    a.append(2i64)
    a.append(3i64)
    ca: ColumnI64 = tabular.col_i64_simple(a)
    cn: List[str] = []
    cn.append(\"a\")
    cs: List[Column] = []
    cs.append(ca)
    df: DataFrame = tabular.from_columns(cn, cs)
    s: DataFrame = df.iloc_2d(2i64, 1i64, 0i64, 1i64)
    println(\"nrows=\" + str(s.length()))
    return 0
";
    let out = run("iloc_2d_empty", src);
    assert!(out.contains("nrows=0"), "got: {out:?}");
}

// ── Phase B: rolling Welford std + min_periods variants ──────────────

#[test]
fn rolling_std_min_periods_smoke_i64() {
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
    r: ColumnF64 = c.rolling_std_min_periods(3i64, 1i64)
    println(\"len=\" + str(r.length()))
    println(\"nc=\" + str(r.null_count()))
    return 0
";
    let out = run("roll_std_mp_i64", src);
    assert!(out.contains("len=5"), "got: {out:?}");
    // min_periods=1 → no nulls.
    assert!(out.contains("nc=0"), "got: {out:?}");
}

#[test]
fn rolling_sum_min_periods_partial_windows_fill_in() {
    // [1, 2, 3, 4, 5] window=3 min_periods=1 → [1, 3, 6, 9, 12]
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
    r: ColumnI64 = c.rolling_sum_min_periods(3i64, 1i64)
    v0: i64? = r.get(0i64)
    v1: i64? = r.get(1i64)
    v2: i64? = r.get(2i64)
    if v0 is not none:
        println(\"v0=\" + str(v0))
    if v1 is not none:
        println(\"v1=\" + str(v1))
    if v2 is not none:
        println(\"v2=\" + str(v2))
    return 0
";
    let out = run("roll_sum_mp_partial", src);
    assert!(out.contains("v0=1"), "got: {out:?}");
    assert!(out.contains("v1=3"), "got: {out:?}");
    assert!(out.contains("v2=6"), "got: {out:?}");
}

#[test]
fn rolling_sum_min_periods_equals_window_matches_m40() {
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
    r: ColumnI64 = c.rolling_sum_min_periods(3i64, 3i64)
    println(\"nc=\" + str(r.null_count()))
    println(\"is0=\" + str(r.is_null(0i64)))
    println(\"is2=\" + str(r.is_null(2i64)))
    return 0
";
    let out = run("roll_sum_mp_window", src);
    assert!(out.contains("nc=2"), "got: {out:?}");
    assert!(out.contains("is0=true"), "got: {out:?}");
    assert!(out.contains("is2=false"), "got: {out:?}");
}

#[test]
fn rolling_mean_min_periods_f64_smoke() {
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
    r: ColumnF64 = c.rolling_mean_min_periods(3i64, 2i64)
    println(\"len=\" + str(r.length()))
    println(\"is0=\" + str(r.is_null(0i64)))
    println(\"is1=\" + str(r.is_null(1i64)))
    return 0
";
    let out = run("roll_mean_mp_f64", src);
    assert!(out.contains("len=4"), "got: {out:?}");
    assert!(out.contains("is0=true"), "got: {out:?}");
    assert!(out.contains("is1=false"), "got: {out:?}");
}

#[test]
fn rolling_min_max_min_periods_i64() {
    let src = "\
from tabular import ColumnI64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(5i64)
    vs.append(2i64)
    vs.append(8i64)
    vs.append(1i64)
    vs.append(3i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    rmin: ColumnI64 = c.rolling_min_min_periods(3i64, 1i64)
    rmax: ColumnI64 = c.rolling_max_min_periods(3i64, 1i64)
    mn2: i64? = rmin.get(2i64)
    mx2: i64? = rmax.get(2i64)
    if mn2 is not none:
        println(\"mn2=\" + str(mn2))
    if mx2 is not none:
        println(\"mx2=\" + str(mx2))
    return 0
";
    let out = run("roll_min_max_mp", src);
    assert!(out.contains("mn2=2"), "got: {out:?}");
    assert!(out.contains("mx2=8"), "got: {out:?}");
}

#[test]
fn rolling_std_min_periods_window_too_big_raises() {
    let src = "\
from tabular import ColumnI64, ColumnF64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    try:
        r: ColumnF64 = c.rolling_std_min_periods(5i64, 1i64)
        println(\"no-raise\")
    except ValueError:
        println(\"got-valueerror\")
    return 0
";
    let out = run("roll_std_mp_too_big", src);
    assert!(out.contains("got-valueerror"), "got: {out:?}");
}

#[test]
fn rolling_min_periods_out_of_range_raises() {
    // min_periods > window → ValueError.
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
    try:
        r: ColumnI64 = c.rolling_sum_min_periods(2i64, 5i64)
        println(\"no-raise\")
    except ValueError:
        println(\"got-valueerror\")
    return 0
";
    let out = run("roll_sum_mp_bad_mp", src);
    assert!(out.contains("got-valueerror"), "got: {out:?}");
}

#[test]
fn rolling_std_welford_matches_m40_on_small_input() {
    // Welford should give identical results to M40's naive formula on
    // small/well-conditioned inputs.
    let src = "\
from tabular import ColumnF64
import tabular
fn main() -> i32:
    vs: List[f64] = []
    vs.append(1.0)
    vs.append(2.0)
    vs.append(3.0)
    vs.append(4.0)
    vs.append(5.0)
    c: ColumnF64 = tabular.col_f64_simple(vs)
    a: ColumnF64 = c.rolling_std(3i64)
    b: ColumnF64 = c.rolling_std_min_periods(3i64, 3i64)
    va: f64? = a.get(4i64)
    vb: f64? = b.get(4i64)
    if va is not none:
        println(\"va=\" + str(va))
    if vb is not none:
        println(\"vb=\" + str(vb))
    return 0
";
    let out = run("roll_std_welford_eq", src);
    // std([3,4,5]) sample variance = 1.0.
    assert!(out.contains("va=1"), "got: {out:?}");
    assert!(out.contains("vb=1"), "got: {out:?}");
}

#[test]
fn rolling_std_min_periods_f64() {
    let src = "\
from tabular import ColumnF64
import tabular
fn main() -> i32:
    vs: List[f64] = []
    vs.append(2.0)
    vs.append(4.0)
    vs.append(4.0)
    vs.append(4.0)
    vs.append(5.0)
    vs.append(5.0)
    vs.append(7.0)
    vs.append(9.0)
    c: ColumnF64 = tabular.col_f64_simple(vs)
    r: ColumnF64 = c.rolling_std_min_periods(8i64, 8i64)
    v: f64? = r.get(7i64)
    if v is not none:
        if v > 2.0:
            if v < 2.5:
                println(\"std-ok\")
    return 0
";
    let out = run("roll_std_mp_f64", src);
    assert!(out.contains("std-ok"), "got: {out:?}");
}

#[test]
fn rolling_mean_min_periods_default_window_equals_m40() {
    let src = "\
from tabular import ColumnI64, ColumnF64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(2i64)
    vs.append(4i64)
    vs.append(6i64)
    vs.append(8i64)
    vs.append(10i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    a: ColumnF64 = c.rolling_mean(3i64)
    b: ColumnF64 = c.rolling_mean_min_periods(3i64, 3i64)
    va: f64? = a.get(2i64)
    vb: f64? = b.get(2i64)
    if va is not none:
        println(\"a2=\" + str(va))
    if vb is not none:
        println(\"b2=\" + str(vb))
    return 0
";
    let out = run("roll_mean_mp_eq_m40", src);
    assert!(out.contains("a2=4"), "got: {out:?}");
    assert!(out.contains("b2=4"), "got: {out:?}");
}

#[test]
fn rolling_sum_min_periods_with_nulls() {
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
    r: ColumnI64 = c.rolling_sum_min_periods(3i64, 1i64)
    v1: i64? = r.get(1i64)
    v2: i64? = r.get(2i64)
    if v1 is not none:
        println(\"v1=\" + str(v1))
    if v2 is not none:
        println(\"v2=\" + str(v2))
    return 0
";
    let out = run("roll_sum_mp_nulls", src);
    assert!(out.contains("v1=1"), "got: {out:?}");
    assert!(out.contains("v2=4"), "got: {out:?}");
}

#[test]
fn rolling_max_min_periods_f64() {
    let src = "\
from tabular import ColumnF64
import tabular
fn main() -> i32:
    vs: List[f64] = []
    vs.append(1.5)
    vs.append(2.5)
    vs.append(0.5)
    vs.append(3.5)
    c: ColumnF64 = tabular.col_f64_simple(vs)
    r: ColumnF64 = c.rolling_max_min_periods(2i64, 1i64)
    v3: f64? = r.get(3i64)
    if v3 is not none:
        if v3 > 3.0:
            if v3 < 4.0:
                println(\"max-ok\")
    return 0
";
    let out = run("roll_max_mp_f64", src);
    assert!(out.contains("max-ok"), "got: {out:?}");
}

// ── Phase C: ColumnCategorical ────────────────────────────────────────

#[test]
fn col_categorical_construction_happy_path() {
    let src = "\
from tabular import ColumnCategorical, ColumnI64, ColumnStr
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"east\")
    vs.append(\"north\")
    vs.append(\"east\")
    vs.append(\"west\")
    vs.append(\"north\")
    vs.append(\"east\")
    c: ColumnCategorical = tabular.col_categorical(vs)
    println(\"len=\" + str(c.length()))
    println(\"dt=\" + c.dtype())
    println(\"nc=\" + str(c.null_count()))
    cats: ColumnStr = c.categories()
    println(\"k=\" + str(cats.length()))
    cs: str? = cats.get(1i64)
    if cs is not none:
        println(\"cat1=\" + cs)
    return 0
";
    let out = run("cat_basic", src);
    assert!(out.contains("len=6"), "got: {out:?}");
    assert!(out.contains("dt=categorical"), "got: {out:?}");
    assert!(out.contains("nc=0"), "got: {out:?}");
    assert!(out.contains("k=3"), "got: {out:?}");
    assert!(out.contains("cat1=north"), "got: {out:?}");
}

#[test]
fn col_categorical_codes_match_first_appearance_order() {
    // Codes: east=0, north=1, east=0, west=2, north=1, east=0.
    let src = "\
from tabular import ColumnCategorical, ColumnI64
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"east\")
    vs.append(\"north\")
    vs.append(\"east\")
    vs.append(\"west\")
    vs.append(\"north\")
    vs.append(\"east\")
    c: ColumnCategorical = tabular.col_categorical(vs)
    codes: ColumnI64 = c.codes()
    v0: i64? = codes.get(0i64)
    v3: i64? = codes.get(3i64)
    v5: i64? = codes.get(5i64)
    if v0 is not none:
        println(\"v0=\" + str(v0))
    if v3 is not none:
        println(\"v3=\" + str(v3))
    if v5 is not none:
        println(\"v5=\" + str(v5))
    return 0
";
    let out = run("cat_codes", src);
    assert!(out.contains("v0=0"), "got: {out:?}");
    assert!(out.contains("v3=2"), "got: {out:?}");
    assert!(out.contains("v5=0"), "got: {out:?}");
}

#[test]
fn col_categorical_to_strings_roundtrips() {
    let src = "\
from tabular import ColumnCategorical, ColumnStr
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"a\")
    vs.append(\"b\")
    vs.append(\"a\")
    vs.append(\"c\")
    c: ColumnCategorical = tabular.col_categorical(vs)
    s: ColumnStr = c.to_strings()
    println(\"len=\" + str(s.length()))
    v0: str? = s.get(0i64)
    v1: str? = s.get(1i64)
    v3: str? = s.get(3i64)
    if v0 is not none:
        println(\"v0=\" + v0)
    if v1 is not none:
        println(\"v1=\" + v1)
    if v3 is not none:
        println(\"v3=\" + v3)
    return 0
";
    let out = run("cat_to_strings", src);
    assert!(out.contains("len=4"), "got: {out:?}");
    assert!(out.contains("v0=a"), "got: {out:?}");
    assert!(out.contains("v1=b"), "got: {out:?}");
    assert!(out.contains("v3=c"), "got: {out:?}");
}

#[test]
fn col_categorical_with_nulls_preserves_mask() {
    let src = "\
from tabular import ColumnCategorical
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"a\")
    vs.append(\"b\")
    vs.append(\"a\")
    vs.append(\"c\")
    ns: List[bool] = []
    ns.append(false)
    ns.append(true)
    ns.append(false)
    ns.append(false)
    c: ColumnCategorical = tabular.col_categorical_with_nulls(vs, ns)
    println(\"len=\" + str(c.length()))
    println(\"nc=\" + str(c.null_count()))
    println(\"is1=\" + str(c.is_null(1i64)))
    println(\"is0=\" + str(c.is_null(0i64)))
    return 0
";
    let out = run("cat_nulls", src);
    assert!(out.contains("len=4"), "got: {out:?}");
    assert!(out.contains("nc=1"), "got: {out:?}");
    assert!(out.contains("is1=true"), "got: {out:?}");
    assert!(out.contains("is0=false"), "got: {out:?}");
}

#[test]
fn col_categorical_get_returns_string() {
    let src = "\
from tabular import ColumnCategorical
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"a\")
    vs.append(\"b\")
    vs.append(\"a\")
    c: ColumnCategorical = tabular.col_categorical(vs)
    v0: str? = c.get(0i64)
    v1: str? = c.get(1i64)
    if v0 is not none:
        println(\"v0=\" + v0)
    if v1 is not none:
        println(\"v1=\" + v1)
    return 0
";
    let out = run("cat_get", src);
    assert!(out.contains("v0=a"), "got: {out:?}");
    assert!(out.contains("v1=b"), "got: {out:?}");
}

#[test]
fn df_get_column_categorical_hit() {
    let src = "\
from tabular import Column, ColumnI64, ColumnCategorical, DataFrame
import tabular
fn main() -> i32:
    rs: List[str] = []
    rs.append(\"east\")
    rs.append(\"west\")
    rs.append(\"east\")
    cc: ColumnCategorical = tabular.col_categorical(rs)
    qs: List[i64] = []
    qs.append(10i64)
    qs.append(20i64)
    qs.append(30i64)
    cq: ColumnI64 = tabular.col_i64_simple(qs)
    cn: List[str] = []
    cn.append(\"region\")
    cn.append(\"qty\")
    cs: List[Column] = []
    cs.append(cc)
    cs.append(cq)
    df: DataFrame = tabular.from_columns(cn, cs)
    got: ColumnCategorical? = df.get_column_categorical(\"region\")
    if got is not none:
        println(\"got_len=\" + str(got.length()))
    miss: ColumnCategorical? = df.get_column_categorical(\"qty\")
    if miss is none:
        println(\"miss-none\")
    nope: ColumnCategorical? = df.get_column_categorical(\"nope\")
    if nope is none:
        println(\"nope-none\")
    return 0
";
    let out = run("df_get_cat", src);
    assert!(out.contains("got_len=3"), "got: {out:?}");
    assert!(out.contains("miss-none"), "got: {out:?}");
    assert!(out.contains("nope-none"), "got: {out:?}");
}

#[test]
fn col_categorical_dedupes_repeated_values() {
    let src = "\
from tabular import ColumnCategorical, ColumnStr
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"x\")
    vs.append(\"x\")
    vs.append(\"x\")
    vs.append(\"x\")
    c: ColumnCategorical = tabular.col_categorical(vs)
    cats: ColumnStr = c.categories()
    println(\"k=\" + str(cats.length()))
    println(\"len=\" + str(c.length()))
    return 0
";
    let out = run("cat_dedupe", src);
    assert!(out.contains("k=1"), "got: {out:?}");
    assert!(out.contains("len=4"), "got: {out:?}");
}

#[test]
fn col_categorical_in_filter_via_to_strings_pattern() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, ColumnCategorical, ColumnBool, DataFrame
import tabular
fn main() -> i32:
    rs: List[str] = []
    rs.append(\"east\")
    rs.append(\"west\")
    rs.append(\"east\")
    rs.append(\"north\")
    cc: ColumnCategorical = tabular.col_categorical(rs)
    qs: List[i64] = []
    qs.append(10i64)
    qs.append(20i64)
    qs.append(30i64)
    qs.append(40i64)
    cq: ColumnI64 = tabular.col_i64_simple(qs)
    cn: List[str] = []
    cn.append(\"region\")
    cn.append(\"qty\")
    cs: List[Column] = []
    cs.append(cc.to_strings())
    cs.append(cq)
    df: DataFrame = tabular.from_columns(cn, cs)
    reg_col: ColumnStr? = df.get_column_str(\"region\")
    if reg_col is not none:
        mask: ColumnBool = reg_col.eq(\"east\")
        out: DataFrame = df.filter(mask)
        println(\"f_nrows=\" + str(out.length()))
    return 0
";
    let out = run("cat_filter", src);
    assert!(out.contains("f_nrows=2"), "got: {out:?}");
}

#[test]
fn col_categorical_group_by_via_to_strings() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, ColumnCategorical, DataFrame, GroupedDataFrame
import tabular
fn main() -> i32:
    rs: List[str] = []
    rs.append(\"east\")
    rs.append(\"west\")
    rs.append(\"east\")
    rs.append(\"west\")
    cc: ColumnCategorical = tabular.col_categorical(rs)
    qs: List[i64] = []
    qs.append(10i64)
    qs.append(20i64)
    qs.append(30i64)
    qs.append(40i64)
    cq: ColumnI64 = tabular.col_i64_simple(qs)
    cn: List[str] = []
    cn.append(\"region\")
    cn.append(\"qty\")
    cs: List[Column] = []
    cs.append(cc.to_strings())
    cs.append(cq)
    df: DataFrame = tabular.from_columns(cn, cs)
    keys: List[str] = []
    keys.append(\"region\")
    g: GroupedDataFrame = df.group_by(keys)
    s: DataFrame = g.sum()
    println(\"g_nrows=\" + str(s.length()))
    return 0
";
    let out = run("cat_groupby", src);
    assert!(out.contains("g_nrows=2"), "got: {out:?}");
}

#[test]
fn col_categorical_codes_with_nulls_zero_for_null_cell() {
    // Codes col inherits the null mask from the categorical — so
    // codes.get(1) returns none (cell is null).  v0 is non-null and
    // returns the index of category "a" = 0.
    let src = "\
from tabular import ColumnCategorical, ColumnI64
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"a\")
    vs.append(\"x\")
    vs.append(\"a\")
    ns: List[bool] = []
    ns.append(false)
    ns.append(true)
    ns.append(false)
    c: ColumnCategorical = tabular.col_categorical_with_nulls(vs, ns)
    codes: ColumnI64 = c.codes()
    println(\"clen=\" + str(codes.length()))
    println(\"cnc=\" + str(codes.null_count()))
    v0: i64? = codes.get(0i64)
    v1: i64? = codes.get(1i64)
    if v0 is not none:
        println(\"v0=\" + str(v0))
    if v1 is none:
        println(\"v1-null\")
    return 0
";
    let out = run("cat_nulls_codes", src);
    assert!(out.contains("clen=3"), "got: {out:?}");
    assert!(out.contains("cnc=1"), "got: {out:?}");
    assert!(out.contains("v0=0"), "got: {out:?}");
    assert!(out.contains("v1-null"), "got: {out:?}");
}

#[test]
fn col_categorical_to_strings_preserves_null_mask() {
    let src = "\
from tabular import ColumnCategorical, ColumnStr
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"a\")
    vs.append(\"x\")
    vs.append(\"b\")
    ns: List[bool] = []
    ns.append(false)
    ns.append(true)
    ns.append(false)
    c: ColumnCategorical = tabular.col_categorical_with_nulls(vs, ns)
    s: ColumnStr = c.to_strings()
    println(\"snc=\" + str(s.null_count()))
    println(\"sis1=\" + str(s.is_null(1i64)))
    return 0
";
    let out = run("cat_to_strings_nulls", src);
    assert!(out.contains("snc=1"), "got: {out:?}");
    assert!(out.contains("sis1=true"), "got: {out:?}");
}

#[test]
fn col_categorical_dtype_string_is_categorical() {
    let src = "\
from tabular import ColumnCategorical
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"a\")
    c: ColumnCategorical = tabular.col_categorical(vs)
    println(\"dt=\" + c.dtype())
    return 0
";
    let out = run("cat_dtype", src);
    assert!(out.contains("dt=categorical"), "got: {out:?}");
}

#[test]
fn col_categorical_categories_first_appearance_order() {
    let src = "\
from tabular import ColumnCategorical, ColumnStr
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"zebra\")
    vs.append(\"apple\")
    vs.append(\"zebra\")
    vs.append(\"mango\")
    c: ColumnCategorical = tabular.col_categorical(vs)
    cats: ColumnStr = c.categories()
    v0: str? = cats.get(0i64)
    v1: str? = cats.get(1i64)
    v2: str? = cats.get(2i64)
    if v0 is not none:
        println(\"v0=\" + v0)
    if v1 is not none:
        println(\"v1=\" + v1)
    if v2 is not none:
        println(\"v2=\" + v2)
    return 0
";
    let out = run("cat_cats_order", src);
    // First-appearance: zebra, apple, mango (NOT alphabetical).
    assert!(out.contains("v0=zebra"), "got: {out:?}");
    assert!(out.contains("v1=apple"), "got: {out:?}");
    assert!(out.contains("v2=mango"), "got: {out:?}");
}

// Suppress dead-code warning for the helper.
#[allow(dead_code)]
fn _use_helper() -> String {
    make_i64_appends("x", &[])
}
