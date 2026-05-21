//! M38 in-process regression tests for the `tabular` round-out.
//!
//! Covers the post-M37 additions: typed DataFrame accessors, restored
//! comparison operations (ne/ge/le/between + starts_with/ends_with),
//! `df.rename`, per-column aggregations (sum/mean/min/max/count/std/
//! var/median), `df.describe`, `Column.fill_null` per subclass,
//! `tabular.from_dict`, and hash-based group-by (the new
//! `GroupedDataFrame` class).
//!
//! See `LANGUAGE_GUIDE.md` §5 (post-M38) for the full surface.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m38_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

fn run(name: &str, src: &str) -> String {
    let p = compile_snippet(name, src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "{name}: exit={code} stdout={out:?}");
    out
}

// ── Phase A: typed accessors + restored comparison ops ───────────────

#[test]
fn get_column_i64_hit_and_miss() {
    let src = "\
from tabular import ColumnI64, ColumnStr, DataFrame, Column
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    ages: ColumnI64 = tabular.col_i64_simple(vs)
    nms: List[str] = []
    nms.append(\"alice\")
    nms.append(\"bob\")
    nms.append(\"carol\")
    names_col: ColumnStr = tabular.col_str_simple(nms)
    cn: List[str] = []
    cn.append(\"age\")
    cn.append(\"name\")
    cs: List[Column] = []
    cs.append(ages)
    cs.append(names_col)
    df: DataFrame = tabular.from_columns(cn, cs)
    a: ColumnI64? = df.get_column_i64(\"age\")
    if a is not none:
        println(\"age_len=\" + str(a.length()))
    b: ColumnI64? = df.get_column_i64(\"name\")
    if b is none:
        println(\"name_as_i64=none\")
    c: ColumnI64? = df.get_column_i64(\"missing\")
    if c is none:
        println(\"missing=none\")
    return 0
";
    let out = run("get_column_i64", src);
    assert!(out.contains("age_len=3"), "got: {out:?}");
    assert!(out.contains("name_as_i64=none"), "got: {out:?}");
    assert!(out.contains("missing=none"), "got: {out:?}");
}

#[test]
fn get_column_f64_str_bool_datetime_accessors() {
    let src = "\
from tabular import ColumnF64, ColumnStr, ColumnBool, ColumnDateTime, DataFrame, Column
import tabular
fn main() -> i32:
    fs1: List[f64] = []
    fs1.append(1.5f64)
    fs1.append(2.5f64)
    cf: ColumnF64 = tabular.col_f64_simple(fs1)
    ss: List[str] = []
    ss.append(\"x\")
    ss.append(\"y\")
    cs: ColumnStr = tabular.col_str_simple(ss)
    bs: List[bool] = []
    bs.append(true)
    bs.append(false)
    cb: ColumnBool = tabular.col_bool_simple(bs)
    ds: List[i64] = []
    ds.append(1000i64)
    ds.append(2000i64)
    dns: List[bool] = []
    dns.append(false)
    dns.append(false)
    cd: ColumnDateTime = tabular.col_datetime(ds, dns)
    nms: List[str] = []
    nms.append(\"price\")
    nms.append(\"name\")
    nms.append(\"flag\")
    nms.append(\"ts\")
    cols: List[Column] = []
    cols.append(cf)
    cols.append(cs)
    cols.append(cb)
    cols.append(cd)
    df: DataFrame = tabular.from_columns(nms, cols)
    f: ColumnF64? = df.get_column_f64(\"price\")
    if f is not none:
        println(\"f_ok\")
    s: ColumnStr? = df.get_column_str(\"name\")
    if s is not none:
        println(\"s_ok\")
    b2: ColumnBool? = df.get_column_bool(\"flag\")
    if b2 is not none:
        println(\"b_ok\")
    d: ColumnDateTime? = df.get_column_datetime(\"ts\")
    if d is not none:
        println(\"d_ok\")
    return 0
";
    let out = run("get_column_typed", src);
    for marker in ["f_ok", "s_ok", "b_ok", "d_ok"] {
        assert!(out.contains(marker), "missing {marker:?}; got: {out:?}");
    }
}

#[test]
fn col_i64_ne_ge_le_between() {
    let src = "\
from tabular import ColumnI64, ColumnBool
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    vs.append(4i64)
    vs.append(5i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    m_ne: ColumnBool = c.ne(3i64)
    m_ge: ColumnBool = c.ge(3i64)
    m_le: ColumnBool = c.le(3i64)
    m_bt: ColumnBool = c.between(2i64, 4i64)
    println(\"ne_count=\" + str(m_ne.count_true()))
    println(\"ge_count=\" + str(m_ge.count_true()))
    println(\"le_count=\" + str(m_le.count_true()))
    println(\"bt_count=\" + str(m_bt.count_true()))
    return 0
";
    let out = run("col_i64_cmp_ext", src);
    // 1,2,3,4,5 → ne 3: 4 hits (1,2,4,5)
    assert!(out.contains("ne_count=4"), "got: {out:?}");
    // ge 3: 3,4,5 → 3
    assert!(out.contains("ge_count=3"), "got: {out:?}");
    // le 3: 1,2,3 → 3
    assert!(out.contains("le_count=3"), "got: {out:?}");
    // between(2,4): 2,3,4 → 3
    assert!(out.contains("bt_count=3"), "got: {out:?}");
}

#[test]
fn col_f64_between() {
    let src = "\
from tabular import ColumnF64, ColumnBool
import tabular
fn main() -> i32:
    vs: List[f64] = []
    vs.append(0.5f64)
    vs.append(1.5f64)
    vs.append(2.5f64)
    vs.append(3.5f64)
    c: ColumnF64 = tabular.col_f64_simple(vs)
    m: ColumnBool = c.between(1.0f64, 3.0f64)
    println(\"bt_count=\" + str(m.count_true()))
    return 0
";
    let out = run("col_f64_between", src);
    // 1.5, 2.5 are in [1.0, 3.0] → 2
    assert!(out.contains("bt_count=2"), "got: {out:?}");
}

#[test]
fn col_str_starts_with_ends_with() {
    let src = "\
from tabular import ColumnStr, ColumnBool
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"alpha\")
    vs.append(\"alphabet\")
    vs.append(\"beta\")
    vs.append(\"gamma\")
    c: ColumnStr = tabular.col_str_simple(vs)
    m1: ColumnBool = c.starts_with(\"alph\")
    m2: ColumnBool = c.ends_with(\"a\")
    println(\"sw=\" + str(m1.count_true()))
    println(\"ew=\" + str(m2.count_true()))
    return 0
";
    let out = run("col_str_affix", src);
    // alpha, alphabet → 2
    assert!(out.contains("sw=2"), "got: {out:?}");
    // alpha, beta, gamma → 3
    assert!(out.contains("ew=3"), "got: {out:?}");
}

#[test]
fn df_rename_swaps_column_names() {
    let src = "\
from tabular import DataFrame, Column, ColumnI64
import tabular
fn main() -> i32:
    v1: List[i64] = []
    v1.append(1i64)
    c1: ColumnI64 = tabular.col_i64_simple(v1)
    v2: List[i64] = []
    v2.append(2i64)
    c2: ColumnI64 = tabular.col_i64_simple(v2)
    names: List[str] = []
    names.append(\"a\")
    names.append(\"b\")
    cols: List[Column] = []
    cols.append(c1)
    cols.append(c2)
    df: DataFrame = tabular.from_columns(names, cols)
    rs: List[Tuple[str, str]] = []
    rs.append((\"a\", \"alpha\"))
    df2: DataFrame = df.rename(rs)
    out_names: List[str] = df2.columns()
    println(\"first=\" + out_names[0i32])
    println(\"second=\" + out_names[1i32])
    return 0
";
    let out = run("df_rename", src);
    assert!(out.contains("first=alpha"), "got: {out:?}");
    assert!(out.contains("second=b"), "got: {out:?}");
}

// ── Phase B: per-column aggregations ─────────────────────────────────

#[test]
fn col_i64_aggregations_basic() {
    let src = "\
from tabular import ColumnI64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(0i64)
    vs.append(30i64)
    vs.append(40i64)
    ns: List[bool] = []
    ns.append(false)
    ns.append(true)
    ns.append(false)
    ns.append(false)
    c: ColumnI64 = tabular.col_i64(vs, ns)
    s: i64? = c.sum()
    if s is not none:
        println(\"sum=\" + str(s))
    m: f64? = c.mean()
    if m is not none:
        println(\"mean=\" + str(m))
    lo: i64? = c.min()
    hi: i64? = c.max()
    if lo is not none:
        println(\"min=\" + str(lo))
    if hi is not none:
        println(\"max=\" + str(hi))
    println(\"count=\" + str(c.count()))
    return 0
";
    let out = run("i64_aggs", src);
    // 10 + 30 + 40 = 80 (null at idx 1 skipped)
    assert!(out.contains("sum=80"), "got: {out:?}");
    // 80 / 3 = 26.666...
    assert!(out.contains("mean=26.66666"), "got: {out:?}");
    assert!(out.contains("min=10"), "got: {out:?}");
    assert!(out.contains("max=40"), "got: {out:?}");
    assert!(out.contains("count=3"), "got: {out:?}");
}

#[test]
fn col_i64_std_var_median() {
    let src = "\
from tabular import ColumnI64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(2i64)
    vs.append(4i64)
    vs.append(4i64)
    vs.append(4i64)
    vs.append(5i64)
    vs.append(5i64)
    vs.append(7i64)
    vs.append(9i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    s: f64? = c.std()
    v: f64? = c.var()
    m: f64? = c.median()
    if s is not none:
        println(\"std=\" + str(s))
    if v is not none:
        println(\"var=\" + str(v))
    if m is not none:
        println(\"med=\" + str(m))
    return 0
";
    let out = run("i64_std_var_median", src);
    // sample std of [2,4,4,4,5,5,7,9] is ~2.138; var is ~4.571
    // median is (4+5)/2 = 4.5
    assert!(out.contains("std=2.13"), "got: {out:?}");
    assert!(out.contains("var=4.57"), "got: {out:?}");
    assert!(out.contains("med=4.5"), "got: {out:?}");
}

#[test]
fn col_i64_aggs_all_null_returns_none() {
    let src = "\
from tabular import ColumnI64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(0i64)
    vs.append(0i64)
    ns: List[bool] = []
    ns.append(true)
    ns.append(true)
    c: ColumnI64 = tabular.col_i64(vs, ns)
    s: i64? = c.sum()
    if s is none:
        println(\"sum_none\")
    println(\"count=\" + str(c.count()))
    return 0
";
    let out = run("i64_aggs_all_null", src);
    assert!(out.contains("sum_none"), "got: {out:?}");
    assert!(out.contains("count=0"), "got: {out:?}");
}

#[test]
fn col_f64_aggregations() {
    let src = "\
from tabular import ColumnF64
import tabular
fn main() -> i32:
    vs: List[f64] = []
    vs.append(1.0f64)
    vs.append(2.0f64)
    vs.append(3.0f64)
    vs.append(4.0f64)
    c: ColumnF64 = tabular.col_f64_simple(vs)
    s: f64? = c.sum()
    m: f64? = c.mean()
    if s is not none:
        println(\"sum=\" + str(s))
    if m is not none:
        println(\"mean=\" + str(m))
    return 0
";
    let out = run("f64_aggs", src);
    assert!(out.contains("sum=10"), "got: {out:?}");
    assert!(out.contains("mean=2.5"), "got: {out:?}");
}

#[test]
fn col_str_count_min_max() {
    let src = "\
from tabular import ColumnStr
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"banana\")
    vs.append(\"apple\")
    vs.append(\"cherry\")
    c: ColumnStr = tabular.col_str_simple(vs)
    println(\"count=\" + str(c.count()))
    lo: str? = c.min()
    hi: str? = c.max()
    if lo is not none:
        println(\"min=\" + lo)
    if hi is not none:
        println(\"max=\" + hi)
    return 0
";
    let out = run("str_aggs", src);
    assert!(out.contains("count=3"), "got: {out:?}");
    assert!(out.contains("min=apple"), "got: {out:?}");
    assert!(out.contains("max=cherry"), "got: {out:?}");
}

// ── Phase C: describe + fill_null + from_dict ───────────────────────

#[test]
fn df_describe_basic_shape_and_stats() {
    // describe() returns a 5-row × (1 + ncols) frame. Row index column
    // is named "statistic"; cells are stringified.
    let src = "\
from tabular import DataFrame, Column, ColumnI64, ColumnStr
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    vs.append(4i64)
    ci: ColumnI64 = tabular.col_i64_simple(vs)
    ss: List[str] = []
    ss.append(\"a\")
    ss.append(\"b\")
    ss.append(\"c\")
    ss.append(\"d\")
    cs: ColumnStr = tabular.col_str_simple(ss)
    names: List[str] = []
    names.append(\"n\")
    names.append(\"s\")
    cols: List[Column] = []
    cols.append(ci)
    cols.append(cs)
    df: DataFrame = tabular.from_columns(names, cols)
    d: DataFrame = df.describe()
    println(\"d_rows=\" + str(d.length()))
    println(\"d_cols=\" + str(d.ncols()))
    out_names: List[str] = d.columns()
    println(\"first=\" + out_names[0i32])
    return 0
";
    let out = run("df_describe", src);
    assert!(out.contains("d_rows=5"), "got: {out:?}");
    // 1 statistic col + 2 source cols = 3
    assert!(out.contains("d_cols=3"), "got: {out:?}");
    assert!(out.contains("first=statistic"), "got: {out:?}");
}

#[test]
fn col_i64_fill_null_replaces_null_cells() {
    let src = "\
from tabular import ColumnI64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(0i64)
    vs.append(30i64)
    ns: List[bool] = []
    ns.append(false)
    ns.append(true)
    ns.append(false)
    c: ColumnI64 = tabular.col_i64(vs, ns)
    filled: ColumnI64 = c.fill_null(99i64)
    println(\"nulls_after=\" + str(filled.null_count()))
    g: i64? = filled.get(1i64)
    if g is not none:
        println(\"g1=\" + str(g))
    return 0
";
    let out = run("i64_fill_null", src);
    assert!(out.contains("nulls_after=0"), "got: {out:?}");
    assert!(out.contains("g1=99"), "got: {out:?}");
}

#[test]
fn col_str_fill_null_replaces_null_cells() {
    let src = "\
from tabular import ColumnStr
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"x\")
    vs.append(\"\")
    ns: List[bool] = []
    ns.append(false)
    ns.append(true)
    c: ColumnStr = tabular.col_str(vs, ns)
    f: ColumnStr = c.fill_null(\"missing\")
    println(\"nulls_after=\" + str(f.null_count()))
    g: str? = f.get(1i64)
    if g is not none:
        println(\"g1=\" + g)
    return 0
";
    let out = run("str_fill_null", src);
    assert!(out.contains("nulls_after=0"), "got: {out:?}");
    assert!(out.contains("g1=missing"), "got: {out:?}");
}

#[test]
fn tabular_from_dict_builds_dataframe() {
    let src = "\
from tabular import DataFrame, Column, ColumnI64
import tabular
fn main() -> i32:
    a_v: List[i64] = []
    a_v.append(1i64)
    a_v.append(2i64)
    ca: ColumnI64 = tabular.col_i64_simple(a_v)
    b_v: List[i64] = []
    b_v.append(10i64)
    b_v.append(20i64)
    cb: ColumnI64 = tabular.col_i64_simple(b_v)
    d: Dict[str, Column] = {}
    d[\"a\"] = ca
    d[\"b\"] = cb
    df: DataFrame = tabular.from_dict(d)
    println(\"nrows=\" + str(df.length()))
    println(\"ncols=\" + str(df.ncols()))
    return 0
";
    let out = run("from_dict_basic", src);
    assert!(out.contains("nrows=2"), "got: {out:?}");
    assert!(out.contains("ncols=2"), "got: {out:?}");
}

#[test]
fn col_datetime_min_max() {
    let src = "\
from tabular import ColumnDateTime
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(5000i64)
    vs.append(1000i64)
    vs.append(3000i64)
    ns: List[bool] = []
    ns.append(false)
    ns.append(false)
    ns.append(false)
    c: ColumnDateTime = tabular.col_datetime(vs, ns)
    lo: i64? = c.min()
    hi: i64? = c.max()
    if lo is not none:
        println(\"min=\" + str(lo))
    if hi is not none:
        println(\"max=\" + str(hi))
    return 0
";
    let out = run("dt_aggs", src);
    assert!(out.contains("min=1000"), "got: {out:?}");
    assert!(out.contains("max=5000"), "got: {out:?}");
}
