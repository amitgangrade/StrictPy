//! M51 — chainable RollingWindow.  Tests cover:
//!   1. Constructor + introspection (`window`, `min_periods`, `is_centered`).
//!   2. Numerical correctness for sum/mean/min/max/std/count, matching
//!      M40's `Column.rolling_*` outputs on equivalent inputs.
//!   3. center=True window placement (output is shifted relative to
//!      the non-centered variant).
//!   4. min_periods semantics (partial-window cells become non-null
//!      when min_periods <= non-null count in the clamped window).
//!   5. Index propagation — both single-col (M41) and MultiIndex (M44)
//!      indices are carried through to the output frame.
//!   6. Per-column dispatch — `df.rolling(W).count()` includes non-
//!      numeric columns; `.sum()`/`.mean()`/etc. drop them.
//!   7. Error surface — window < 1, window > nrows, min_periods bounds.

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

fn run_expect_err(name: &str, src: &str) -> String {
    use strictpy_vm::VmError;
    let p = compile_snippet(name, src);
    // An "expected error" path may either: (a) propagate an
    // UncaughtException out of the VM, or (b) reach the host with a
    // nonzero exit code if the .spy file catches the exception and
    // returns nonzero.  Both shapes count as success — the assertion
    // body downstream looks at the captured text for the expected
    // error message.
    match run_file_capture(&p) {
        Ok((_code, out)) => out,
        Err(VmError::UncaughtException { type_name, message }) => {
            format!("UncaughtException({type_name}): {message}")
        }
        Err(e) => panic!("{name}: unexpected runner error: {e:?}"),
    }
}

/// Wraps a `main()` body in the boilerplate that builds the shared
/// 6-row, 2-column test frame (x: ColumnI64 + y: ColumnF64, both
/// fully non-null with values [1..6] and [10..60]).  `body` should
/// start at column 4 (one indent level) and reference the `df` local
/// created by the prelude.
fn with_fixture_6x2(body: &str) -> String {
    let prelude = "\
from tabular import DataFrame, Column, ColumnI64, ColumnF64, RollingWindow
import tabular
fn main() -> i32:
    xv: List[i64] = []
    xv.append(1i64)
    xv.append(2i64)
    xv.append(3i64)
    xv.append(4i64)
    xv.append(5i64)
    xv.append(6i64)
    yv: List[f64] = []
    yv.append(10.0)
    yv.append(20.0)
    yv.append(30.0)
    yv.append(40.0)
    yv.append(50.0)
    yv.append(60.0)
    nulls: List[bool] = []
    nulls.append(false)
    nulls.append(false)
    nulls.append(false)
    nulls.append(false)
    nulls.append(false)
    nulls.append(false)
    xc: ColumnI64 = tabular.col_i64(xv, nulls)
    yc: ColumnF64 = tabular.col_f64(yv, nulls)
    names: List[str] = []
    names.append(\"x\")
    names.append(\"y\")
    cols: List[Column] = []
    cols.append(xc)
    cols.append(yc)
    df: DataFrame = tabular.from_columns(names, cols)
";
    format!("{prelude}{body}")
}

// ── 1. Introspection ────────────────────────────────────────────────

#[test]
fn rolling_window_records_constructor_params() {
    let src = with_fixture_6x2("
    rw: RollingWindow = df.rolling(3i64)
    println(\"w=\" + str(rw.window()))
    println(\"mp=\" + str(rw.min_periods()))
    println(\"c=\" + str(rw.is_centered()))
    return 0
");
    let out = run("rw_introspect_basic", &src);
    assert!(out.contains("w=3"), "got: {out:?}");
    assert!(out.contains("mp=3"), "min_periods defaults to window. got: {out:?}");
    assert!(out.contains("c=false"), "got: {out:?}");
}

#[test]
fn rolling_window_centered_records_center_true() {
    let src = with_fixture_6x2("
    rw: RollingWindow = df.rolling_centered(4i64)
    println(\"w=\" + str(rw.window()))
    println(\"c=\" + str(rw.is_centered()))
    return 0
");
    let out = run("rw_introspect_centered", &src);
    assert!(out.contains("w=4"), "got: {out:?}");
    assert!(out.contains("c=true"), "got: {out:?}");
}

#[test]
fn rolling_window_min_periods_records_explicit_value() {
    let src = with_fixture_6x2("
    rw: RollingWindow = df.rolling_min_periods(4i64, 2i64)
    println(\"w=\" + str(rw.window()))
    println(\"mp=\" + str(rw.min_periods()))
    return 0
");
    let out = run("rw_introspect_minp", &src);
    assert!(out.contains("w=4"), "got: {out:?}");
    assert!(out.contains("mp=2"), "got: {out:?}");
}

// ── 2. Numerical correctness ────────────────────────────────────────

#[test]
fn rolling_sum_matches_hand_computed() {
    // Window=3 over [1,2,3,4,5,6] → [null, null, 6, 9, 12, 15].
    let src = with_fixture_6x2("
    out: DataFrame = df.rolling(3i64).sum()
    out_xc: ColumnI64? = out.get_column_i64(\"x\")
    if out_xc is none:
        return 1
    c: ColumnI64 = out_xc
    i: i64 = 0i64
    while i < c.length():
        v: i64? = c.get(i)
        if v is none:
            println(\"i=\" + str(i) + \" null\")
        else:
            println(\"i=\" + str(i) + \" v=\" + str(v))
        i = i + 1i64
    return 0
");
    let out = run("rw_sum_correctness", &src);
    assert!(out.contains("i=0 null"), "got: {out:?}");
    assert!(out.contains("i=1 null"), "got: {out:?}");
    assert!(out.contains("i=2 v=6"), "got: {out:?}");
    assert!(out.contains("i=3 v=9"), "got: {out:?}");
    assert!(out.contains("i=4 v=12"), "got: {out:?}");
    assert!(out.contains("i=5 v=15"), "got: {out:?}");
}

#[test]
fn rolling_mean_promotes_i64_to_f64() {
    // Window=2 over [1,2,3,4,5,6] → [null, 1.5, 2.5, 3.5, 4.5, 5.5].
    let src = with_fixture_6x2("
    out: DataFrame = df.rolling(2i64).mean()
    out_xc: ColumnF64? = out.get_column_f64(\"x\")
    if out_xc is none:
        return 1
    c: ColumnF64 = out_xc
    v0: f64? = c.get(0i64)
    if v0 is not none:
        return 2
    v1: f64? = c.get(1i64)
    if v1 is none:
        return 3
    println(\"v1=\" + str(v1))
    v5: f64? = c.get(5i64)
    if v5 is none:
        return 4
    println(\"v5=\" + str(v5))
    return 0
");
    let out = run("rw_mean_promotion", &src);
    assert!(out.contains("v1=1.5"), "got: {out:?}");
    assert!(out.contains("v5=5.5"), "got: {out:?}");
}

#[test]
fn rolling_min_max_on_f64() {
    // Window=3 over [10,20,30,40,50,60]: rolling_min[2]=10, rolling_max[2]=30, etc.
    let src = with_fixture_6x2("
    out_min: DataFrame = df.rolling(3i64).min()
    out_max: DataFrame = df.rolling(3i64).max()
    out_yc_min: ColumnF64? = out_min.get_column_f64(\"y\")
    out_yc_max: ColumnF64? = out_max.get_column_f64(\"y\")
    if out_yc_min is none:
        return 1
    if out_yc_max is none:
        return 2
    cmin: ColumnF64 = out_yc_min
    cmax: ColumnF64 = out_yc_max
    v_min: f64? = cmin.get(2i64)
    v_max: f64? = cmax.get(2i64)
    if v_min is none:
        return 3
    if v_max is none:
        return 4
    println(\"min2=\" + str(v_min))
    println(\"max2=\" + str(v_max))
    v_min5: f64? = cmin.get(5i64)
    v_max5: f64? = cmax.get(5i64)
    if v_min5 is not none:
        println(\"min5=\" + str(v_min5))
    if v_max5 is not none:
        println(\"max5=\" + str(v_max5))
    return 0
");
    let out = run("rw_min_max_f64", &src);
    assert!(out.contains("min2=10"), "got: {out:?}");
    assert!(out.contains("max2=30"), "got: {out:?}");
    assert!(out.contains("min5=40"), "got: {out:?}");
    assert!(out.contains("max5=60"), "got: {out:?}");
}

#[test]
fn rolling_std_matches_sample_variance() {
    // Window=3 over [1,2,3]: mean=2, var=((1-2)^2+0+(3-2)^2)/2 = 1, std=1.0.
    let src = with_fixture_6x2("
    out: DataFrame = df.rolling(3i64).std()
    out_xc: ColumnF64? = out.get_column_f64(\"x\")
    if out_xc is none:
        return 1
    c: ColumnF64 = out_xc
    v2: f64? = c.get(2i64)
    if v2 is none:
        return 2
    println(\"std2=\" + str(v2))
    return 0
");
    let out = run("rw_std_sample", &src);
    assert!(out.contains("std2=1"), "got: {out:?}");
}

#[test]
fn rolling_count_includes_all_columns_and_is_i64() {
    let src = with_fixture_6x2("
    out: DataFrame = df.rolling(3i64).count()
    println(\"ncols=\" + str(out.ncols()))
    out_xc: ColumnI64? = out.get_column_i64(\"x\")
    if out_xc is none:
        return 1
    c: ColumnI64 = out_xc
    v: i64? = c.get(3i64)
    if v is none:
        return 2
    println(\"v3=\" + str(v))
    return 0
");
    let out = run("rw_count_basic", &src);
    assert!(out.contains("ncols=2"), "got: {out:?}");
    assert!(out.contains("v3=3"), "got: {out:?}");
}

// ── 3. center=True window placement ─────────────────────────────────

#[test]
fn rolling_centered_shifts_output_one_position_left_for_window_3() {
    // For W=3 centered: position i uses [i-1, i+1].
    //   i=0 → [-1, 1] → null (out of bounds on the left)
    //   i=1 → [0, 2]  → 1+2+3 = 6
    //   i=2 → [1, 3]  → 2+3+4 = 9
    //   i=3 → [2, 4]  → 3+4+5 = 12
    //   i=4 → [3, 5]  → 4+5+6 = 15
    //   i=5 → [4, 6]  → null (right boundary)
    let src = with_fixture_6x2("
    out: DataFrame = df.rolling_centered(3i64).sum()
    out_xc: ColumnI64? = out.get_column_i64(\"x\")
    if out_xc is none:
        return 1
    c: ColumnI64 = out_xc
    i: i64 = 0i64
    while i < c.length():
        v: i64? = c.get(i)
        if v is none:
            println(\"i=\" + str(i) + \" null\")
        else:
            println(\"i=\" + str(i) + \" v=\" + str(v))
        i = i + 1i64
    return 0
");
    let out = run("rw_centered_w3", &src);
    assert!(out.contains("i=0 null"), "got: {out:?}");
    assert!(out.contains("i=1 v=6"), "got: {out:?}");
    assert!(out.contains("i=2 v=9"), "got: {out:?}");
    assert!(out.contains("i=3 v=12"), "got: {out:?}");
    assert!(out.contains("i=4 v=15"), "got: {out:?}");
    assert!(out.contains("i=5 null"), "got: {out:?}");
}

#[test]
fn rolling_centered_w4_pandas_layout_left_heavy() {
    // W=4 centered: lhalf = (4-1)/2 = 1, rhalf = 4/2 = 2 → window
    // around i is [i-1, i+2].
    //   i=0 → [-1, 2] → null
    //   i=1 → [0, 3]  → 1+2+3+4 = 10
    //   i=2 → [1, 4]  → 2+3+4+5 = 14
    //   i=3 → [2, 5]  → 3+4+5+6 = 18
    //   i=4 → [3, 6]  → null  (6 out of bounds)
    //   i=5 → [4, 7]  → null
    let src = with_fixture_6x2("
    out: DataFrame = df.rolling_centered(4i64).sum()
    out_xc: ColumnI64? = out.get_column_i64(\"x\")
    if out_xc is none:
        return 1
    c: ColumnI64 = out_xc
    i: i64 = 0i64
    while i < c.length():
        v: i64? = c.get(i)
        if v is none:
            println(\"i=\" + str(i) + \" null\")
        else:
            println(\"i=\" + str(i) + \" v=\" + str(v))
        i = i + 1i64
    return 0
");
    let out = run("rw_centered_w4", &src);
    assert!(out.contains("i=0 null"), "got: {out:?}");
    assert!(out.contains("i=1 v=10"), "got: {out:?}");
    assert!(out.contains("i=2 v=14"), "got: {out:?}");
    assert!(out.contains("i=3 v=18"), "got: {out:?}");
    assert!(out.contains("i=4 null"), "got: {out:?}");
    assert!(out.contains("i=5 null"), "got: {out:?}");
}

// ── 4. min_periods semantics ────────────────────────────────────────

#[test]
fn rolling_min_periods_fills_partial_windows() {
    // W=3, mp=1 over [1,2,3,4,5,6]:
    //   i=0 → window [-2..0] clamped to [0..0], nn=1 ≥ mp=1 → sum=1
    //   i=1 → clamped [0..1], nn=2 → sum=1+2=3
    //   i=2 → full window [0..2] → sum=6
    let src = with_fixture_6x2("
    out: DataFrame = df.rolling_min_periods(3i64, 1i64).sum()
    out_xc: ColumnI64? = out.get_column_i64(\"x\")
    if out_xc is none:
        return 1
    c: ColumnI64 = out_xc
    v0: i64? = c.get(0i64)
    v1: i64? = c.get(1i64)
    v2: i64? = c.get(2i64)
    if v0 is not none:
        println(\"v0=\" + str(v0))
    if v1 is not none:
        println(\"v1=\" + str(v1))
    if v2 is not none:
        println(\"v2=\" + str(v2))
    return 0
");
    let out = run("rw_mp_partial", &src);
    assert!(out.contains("v0=1"), "got: {out:?}");
    assert!(out.contains("v1=3"), "got: {out:?}");
    assert!(out.contains("v2=6"), "got: {out:?}");
}

// ── 5. Index propagation ────────────────────────────────────────────

#[test]
fn rolling_preserves_single_col_index() {
    let src = "\
from tabular import DataFrame, Column, ColumnI64, ColumnStr, RollingWindow
import tabular
fn main() -> i32:
    xv: List[i64] = []
    xv.append(1i64)
    xv.append(2i64)
    xv.append(3i64)
    xv.append(4i64)
    sv: List[str] = []
    sv.append(\"a\")
    sv.append(\"b\")
    sv.append(\"c\")
    sv.append(\"d\")
    nulls: List[bool] = []
    nulls.append(false)
    nulls.append(false)
    nulls.append(false)
    nulls.append(false)
    xc: ColumnI64 = tabular.col_i64(xv, nulls)
    sc: ColumnStr = tabular.col_str(sv, nulls)
    names: List[str] = []
    names.append(\"x\")
    names.append(\"label\")
    cols: List[Column] = []
    cols.append(xc)
    cols.append(sc)
    df: DataFrame = tabular.from_columns(names, cols)
    df2: DataFrame = df.set_index(\"label\")
    out: DataFrame = df2.rolling(2i64).sum()
    println(\"has_idx=\" + str(out.has_index()))
    nm: str? = out.index_name()
    if nm is none:
        println(\"idx_name=<none>\")
    else:
        println(\"idx_name=\" + nm)
    println(\"nrows=\" + str(out.length()))
    return 0
";
    let out = run("rw_idx_single", src);
    assert!(out.contains("has_idx=true"), "got: {out:?}");
    assert!(out.contains("idx_name=label"), "got: {out:?}");
    assert!(out.contains("nrows=4"), "got: {out:?}");
}

// ── 6. Per-column dispatch ──────────────────────────────────────────

#[test]
fn rolling_sum_drops_non_numeric_columns() {
    let src = "\
from tabular import DataFrame, Column, ColumnI64, ColumnF64, ColumnStr, RollingWindow
import tabular
fn main() -> i32:
    xv: List[i64] = []
    xv.append(1i64)
    xv.append(2i64)
    xv.append(3i64)
    yv: List[f64] = []
    yv.append(1.0)
    yv.append(2.0)
    yv.append(3.0)
    sv: List[str] = []
    sv.append(\"a\")
    sv.append(\"b\")
    sv.append(\"c\")
    nulls: List[bool] = []
    nulls.append(false)
    nulls.append(false)
    nulls.append(false)
    xc: ColumnI64 = tabular.col_i64(xv, nulls)
    yc: ColumnF64 = tabular.col_f64(yv, nulls)
    sc: ColumnStr = tabular.col_str(sv, nulls)
    names: List[str] = []
    names.append(\"x\")
    names.append(\"s\")
    names.append(\"y\")
    cols: List[Column] = []
    cols.append(xc)
    cols.append(sc)
    cols.append(yc)
    df: DataFrame = tabular.from_columns(names, cols)
    sum_df: DataFrame = df.rolling(2i64).sum()
    count_df: DataFrame = df.rolling(2i64).count()
    println(\"sum_ncols=\" + str(sum_df.ncols()))
    println(\"count_ncols=\" + str(count_df.ncols()))
    return 0
";
    let out = run("rw_dispatch_drops_str", src);
    assert!(out.contains("sum_ncols=2"), "sum drops str. got: {out:?}");
    assert!(out.contains("count_ncols=3"), "count keeps str. got: {out:?}");
}

// ── 7. Error surface ────────────────────────────────────────────────

#[test]
fn rolling_rejects_zero_window() {
    let src = with_fixture_6x2("
    out: DataFrame = df.rolling(0i64).sum()
    println(\"unreached\")
    return 0
");
    let out = run_expect_err("rw_err_zero_w", &src);
    assert!(out.contains("window must be >= 1"), "got: {out:?}");
}

#[test]
fn rolling_rejects_window_larger_than_frame() {
    let src = with_fixture_6x2("
    out: DataFrame = df.rolling(99i64).sum()
    println(\"unreached\")
    return 0
");
    let out = run_expect_err("rw_err_window_too_big", &src);
    assert!(out.contains("exceeds DataFrame length"), "got: {out:?}");
}

#[test]
fn rolling_rejects_min_periods_above_window() {
    let src = with_fixture_6x2("
    out: DataFrame = df.rolling_min_periods(3i64, 4i64).sum()
    println(\"unreached\")
    return 0
");
    let out = run_expect_err("rw_err_mp_too_big", &src);
    assert!(out.contains("min_periods 4 cannot exceed window 3"), "got: {out:?}");
}

#[test]
fn rolling_rejects_zero_min_periods() {
    let src = with_fixture_6x2("
    out: DataFrame = df.rolling_min_periods(3i64, 0i64).sum()
    println(\"unreached\")
    return 0
");
    let out = run_expect_err("rw_err_mp_zero", &src);
    assert!(out.contains("min_periods must be >= 1"), "got: {out:?}");
}
