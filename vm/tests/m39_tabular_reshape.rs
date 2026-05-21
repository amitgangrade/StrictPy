//! M39 in-process regression tests for the `tabular` reshape operations.
//!
//! Covers Phase 4 of the Pandas-shaped data package: typed `unique`
//! accessors per dtype, `value_counts`, `tabular.concat_rows` /
//! `tabular.concat_cols`, `df.merge` (hash-join inner/left/right/outer),
//! `df.pivot` (long-to-wide), and `df.melt` (wide-to-long).
//!
//! See `LANGUAGE_GUIDE.md` §5 (post-M39) for the full surface.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m39_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

fn run(name: &str, src: &str) -> String {
    let p = compile_snippet(name, src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "{name}: exit={code} stdout={out:?}");
    out
}

// ── Phase A: unique_* per dtype ──────────────────────────────────────

#[test]
fn unique_i64_preserves_first_occurrence_order() {
    let src = "\
from tabular import ColumnI64, DataFrame, Column
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(3i64)
    vs.append(1i64)
    vs.append(3i64)
    vs.append(2i64)
    vs.append(1i64)
    ns: List[bool] = []
    ns.append(false)
    ns.append(false)
    ns.append(false)
    ns.append(false)
    ns.append(false)
    c: ColumnI64 = tabular.col_i64(vs, ns)
    cn: List[str] = []
    cn.append(\"x\")
    cs: List[Column] = []
    cs.append(c)
    df: DataFrame = tabular.from_columns(cn, cs)
    u: ColumnI64? = df.unique_i64(\"x\")
    if u is not none:
        println(\"unique_len=\" + str(u.length()))
        v0: i64? = u.get(0i64)
        if v0 is not none:
            println(\"u0=\" + str(v0))
        v1: i64? = u.get(1i64)
        if v1 is not none:
            println(\"u1=\" + str(v1))
        v2: i64? = u.get(2i64)
        if v2 is not none:
            println(\"u2=\" + str(v2))
    return 0
";
    let out = run("unique_i64", src);
    assert!(out.contains("unique_len=3"), "got: {out:?}");
    assert!(out.contains("u0=3"), "got: {out:?}");
    assert!(out.contains("u1=1"), "got: {out:?}");
    assert!(out.contains("u2=2"), "got: {out:?}");
}

#[test]
fn unique_i64_excludes_nulls() {
    let src = "\
from tabular import ColumnI64, DataFrame, Column
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(0i64)
    vs.append(1i64)
    vs.append(2i64)
    ns: List[bool] = []
    ns.append(false)
    ns.append(true)
    ns.append(false)
    ns.append(false)
    c: ColumnI64 = tabular.col_i64(vs, ns)
    cn: List[str] = []
    cn.append(\"x\")
    cs: List[Column] = []
    cs.append(c)
    df: DataFrame = tabular.from_columns(cn, cs)
    u: ColumnI64? = df.unique_i64(\"x\")
    if u is not none:
        println(\"len=\" + str(u.length()))
    return 0
";
    let out = run("unique_i64_null", src);
    assert!(out.contains("len=2"), "got: {out:?}");
}

#[test]
fn unique_str_basic() {
    let src = "\
from tabular import ColumnStr, DataFrame, Column
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"a\")
    vs.append(\"b\")
    vs.append(\"a\")
    vs.append(\"c\")
    c: ColumnStr = tabular.col_str_simple(vs)
    cn: List[str] = []
    cn.append(\"k\")
    cs: List[Column] = []
    cs.append(c)
    df: DataFrame = tabular.from_columns(cn, cs)
    u: ColumnStr? = df.unique_str(\"k\")
    if u is not none:
        println(\"u_len=\" + str(u.length()))
    return 0
";
    let out = run("unique_str", src);
    assert!(out.contains("u_len=3"), "got: {out:?}");
}

#[test]
fn unique_bool_basic() {
    let src = "\
from tabular import ColumnBool, DataFrame, Column
import tabular
fn main() -> i32:
    vs: List[bool] = []
    vs.append(true)
    vs.append(false)
    vs.append(true)
    c: ColumnBool = tabular.col_bool_simple(vs)
    cn: List[str] = []
    cn.append(\"flag\")
    cs: List[Column] = []
    cs.append(c)
    df: DataFrame = tabular.from_columns(cn, cs)
    u: ColumnBool? = df.unique_bool(\"flag\")
    if u is not none:
        println(\"u_len=\" + str(u.length()))
    return 0
";
    let out = run("unique_bool", src);
    assert!(out.contains("u_len=2"), "got: {out:?}");
}

#[test]
fn unique_f64_basic() {
    let src = "\
from tabular import ColumnF64, DataFrame, Column
import tabular
fn main() -> i32:
    vs: List[f64] = []
    vs.append(1.5f64)
    vs.append(2.5f64)
    vs.append(1.5f64)
    c: ColumnF64 = tabular.col_f64_simple(vs)
    cn: List[str] = []
    cn.append(\"price\")
    cs: List[Column] = []
    cs.append(c)
    df: DataFrame = tabular.from_columns(cn, cs)
    u: ColumnF64? = df.unique_f64(\"price\")
    if u is not none:
        println(\"u_len=\" + str(u.length()))
    return 0
";
    let out = run("unique_f64", src);
    assert!(out.contains("u_len=2"), "got: {out:?}");
}

#[test]
fn unique_wrong_dtype_returns_none() {
    let src = "\
from tabular import ColumnI64, ColumnStr, DataFrame, Column
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"a\")
    c: ColumnStr = tabular.col_str_simple(vs)
    cn: List[str] = []
    cn.append(\"k\")
    cs: List[Column] = []
    cs.append(c)
    df: DataFrame = tabular.from_columns(cn, cs)
    u: ColumnI64? = df.unique_i64(\"k\")
    if u is none:
        println(\"wrong_dtype=none\")
    return 0
";
    let out = run("unique_wrong_dtype", src);
    assert!(out.contains("wrong_dtype=none"), "got: {out:?}");
}

// ── Phase A: value_counts ────────────────────────────────────────────

#[test]
fn value_counts_str_with_ties() {
    let src = "\
from tabular import ColumnStr, DataFrame, Column
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"a\")
    vs.append(\"b\")
    vs.append(\"a\")
    vs.append(\"c\")
    vs.append(\"b\")
    vs.append(\"a\")
    c: ColumnStr = tabular.col_str_simple(vs)
    cn: List[str] = []
    cn.append(\"k\")
    cs: List[Column] = []
    cs.append(c)
    df: DataFrame = tabular.from_columns(cn, cs)
    vc: DataFrame = df.value_counts(\"k\")
    println(\"vc_nrows=\" + str(vc.length()))
    r0: List[str] = vc.row(0i64)
    r1: List[str] = vc.row(1i64)
    r2: List[str] = vc.row(2i64)
    println(\"r0=\" + r0[0i32] + \":\" + r0[1i32])
    println(\"r1=\" + r1[0i32] + \":\" + r1[1i32])
    println(\"r2=\" + r2[0i32] + \":\" + r2[1i32])
    return 0
";
    let out = run("value_counts_str", src);
    assert!(out.contains("vc_nrows=3"), "got: {out:?}");
    assert!(out.contains("r0=a:3"), "got: {out:?}");
    // Ties broken by first-occurrence: 'b' (row 1) comes before 'c' (row 3).
    assert!(out.contains("r1=b:2"), "got: {out:?}");
    assert!(out.contains("r2=c:1"), "got: {out:?}");
}

#[test]
fn value_counts_excludes_nulls() {
    let src = "\
from tabular import ColumnStr, DataFrame, Column
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"a\")
    vs.append(\"\")
    vs.append(\"a\")
    ns: List[bool] = []
    ns.append(false)
    ns.append(true)
    ns.append(false)
    c: ColumnStr = tabular.col_str(vs, ns)
    cn: List[str] = []
    cn.append(\"k\")
    cs: List[Column] = []
    cs.append(c)
    df: DataFrame = tabular.from_columns(cn, cs)
    vc: DataFrame = df.value_counts(\"k\")
    println(\"vc_nrows=\" + str(vc.length()))
    return 0
";
    let out = run("value_counts_nulls", src);
    assert!(out.contains("vc_nrows=1"), "got: {out:?}");
}

// ── Phase A: concat_rows / concat_cols ───────────────────────────────

#[test]
fn concat_rows_happy_path() {
    let src = "\
from tabular import ColumnI64, ColumnStr, DataFrame, Column
import tabular
fn build(vals: List[i64], names: List[str]) -> DataFrame:
    c1: ColumnI64 = tabular.col_i64_simple(vals)
    c2: ColumnStr = tabular.col_str_simple(names)
    cn: List[str] = []
    cn.append(\"k\")
    cn.append(\"name\")
    cs: List[Column] = []
    cs.append(c1)
    cs.append(c2)
    return tabular.from_columns(cn, cs)
fn main() -> i32:
    v1: List[i64] = []
    v1.append(1i64)
    v1.append(2i64)
    n1: List[str] = []
    n1.append(\"a\")
    n1.append(\"b\")
    df1: DataFrame = build(v1, n1)
    v2: List[i64] = []
    v2.append(3i64)
    n2: List[str] = []
    n2.append(\"c\")
    df2: DataFrame = build(v2, n2)
    dfs: List[DataFrame] = []
    dfs.append(df1)
    dfs.append(df2)
    out: DataFrame = tabular.concat_rows(dfs)
    println(\"nrows=\" + str(out.length()))
    println(\"ncols=\" + str(out.ncols()))
    return 0
";
    let out = run("concat_rows_ok", src);
    assert!(out.contains("nrows=3"), "got: {out:?}");
    assert!(out.contains("ncols=2"), "got: {out:?}");
}

#[test]
fn concat_rows_schema_mismatch_raises() {
    let src = "\
from tabular import ColumnI64, ColumnStr, DataFrame, Column
import tabular
fn main() -> i32:
    v1: List[i64] = []
    v1.append(1i64)
    c1: ColumnI64 = tabular.col_i64_simple(v1)
    cn1: List[str] = []
    cn1.append(\"k\")
    cs1: List[Column] = []
    cs1.append(c1)
    df1: DataFrame = tabular.from_columns(cn1, cs1)
    v2: List[str] = []
    v2.append(\"x\")
    c2: ColumnStr = tabular.col_str_simple(v2)
    cn2: List[str] = []
    cn2.append(\"k\")
    cs2: List[Column] = []
    cs2.append(c2)
    df2: DataFrame = tabular.from_columns(cn2, cs2)
    dfs: List[DataFrame] = []
    dfs.append(df1)
    dfs.append(df2)
    try:
        out: DataFrame = tabular.concat_rows(dfs)
        println(\"unexpected ok\")
    except ValueError as e:
        println(\"raised\")
    return 0
";
    let out = run("concat_rows_mismatch", src);
    assert!(out.contains("raised"), "got: {out:?}");
}

#[test]
fn concat_cols_happy_path() {
    let src = "\
from tabular import ColumnI64, ColumnStr, DataFrame, Column
import tabular
fn main() -> i32:
    v1: List[i64] = []
    v1.append(1i64)
    v1.append(2i64)
    c1: ColumnI64 = tabular.col_i64_simple(v1)
    cn1: List[str] = []
    cn1.append(\"a\")
    cs1: List[Column] = []
    cs1.append(c1)
    df1: DataFrame = tabular.from_columns(cn1, cs1)
    v2: List[str] = []
    v2.append(\"x\")
    v2.append(\"y\")
    c2: ColumnStr = tabular.col_str_simple(v2)
    cn2: List[str] = []
    cn2.append(\"b\")
    cs2: List[Column] = []
    cs2.append(c2)
    df2: DataFrame = tabular.from_columns(cn2, cs2)
    dfs: List[DataFrame] = []
    dfs.append(df1)
    dfs.append(df2)
    out: DataFrame = tabular.concat_cols(dfs)
    println(\"nrows=\" + str(out.length()))
    println(\"ncols=\" + str(out.ncols()))
    return 0
";
    let out = run("concat_cols_ok", src);
    assert!(out.contains("nrows=2"), "got: {out:?}");
    assert!(out.contains("ncols=2"), "got: {out:?}");
}

// ── Phase B: merge (hash-join) ───────────────────────────────────────

fn merge_setup(how: &str) -> String {
    // Two frames sharing `customer_id`.
    // left:  customer_id=[1,2,3,4]   spend=[10,20,30,40]
    // right: customer_id=[1,2,3,5]   country=["US","UK","DE","FR"]
    // inner: 3 rows (cid 1,2,3); left: 4 rows; right: 4 rows; outer: 5 rows.
    format!(
        "
from tabular import ColumnI64, ColumnStr, DataFrame, Column
import tabular
fn build_left() -> DataFrame:
    ids: List[i64] = []
    ids.append(1i64)
    ids.append(2i64)
    ids.append(3i64)
    ids.append(4i64)
    spend: List[i64] = []
    spend.append(10i64)
    spend.append(20i64)
    spend.append(30i64)
    spend.append(40i64)
    c1: ColumnI64 = tabular.col_i64_simple(ids)
    c2: ColumnI64 = tabular.col_i64_simple(spend)
    cn: List[str] = []
    cn.append(\"cid\")
    cn.append(\"spend\")
    cs: List[Column] = []
    cs.append(c1)
    cs.append(c2)
    return tabular.from_columns(cn, cs)
fn build_right() -> DataFrame:
    ids: List[i64] = []
    ids.append(1i64)
    ids.append(2i64)
    ids.append(3i64)
    ids.append(5i64)
    country: List[str] = []
    country.append(\"US\")
    country.append(\"UK\")
    country.append(\"DE\")
    country.append(\"FR\")
    c1: ColumnI64 = tabular.col_i64_simple(ids)
    c2: ColumnStr = tabular.col_str_simple(country)
    cn: List[str] = []
    cn.append(\"cid\")
    cn.append(\"country\")
    cs: List[Column] = []
    cs.append(c1)
    cs.append(c2)
    return tabular.from_columns(cn, cs)
fn main() -> i32:
    L: DataFrame = build_left()
    R: DataFrame = build_right()
    on: List[str] = []
    on.append(\"cid\")
    out: DataFrame = L.merge(R, on, \"{how}\")
    println(\"nrows=\" + str(out.length()))
    println(\"ncols=\" + str(out.ncols()))
    i: i64 = 0i64
    while i < out.length():
        r: List[str] = out.row(i)
        println(\"row=\" + r[0i32] + \",\" + r[1i32] + \",\" + r[2i32])
        i = i + 1i64
    return 0
",
        how = how
    )
}

#[test]
fn merge_inner_basic() {
    let src = merge_setup("inner");
    let out = run("merge_inner", &src);
    assert!(out.contains("nrows=3"), "got: {out:?}");
    assert!(out.contains("ncols=3"), "got: {out:?}");
    // No row should have null cells (inner join).
    assert!(!out.contains("null"), "got: {out:?}");
}

#[test]
fn merge_left_emits_unmatched() {
    let src = merge_setup("left");
    let out = run("merge_left", &src);
    assert!(out.contains("nrows=4"), "got: {out:?}");
    // The unmatched left row (cid=4) should have country=null.
    assert!(out.contains("row=4,40,null"), "got: {out:?}");
}

#[test]
fn merge_right_emits_unmatched() {
    let src = merge_setup("right");
    let out = run("merge_right", &src);
    assert!(out.contains("nrows=4"), "got: {out:?}");
    // The unmatched right row (cid=5) should have spend=null with cid
    // carried over from rhs.
    assert!(out.contains("row=5,null,FR"), "got: {out:?}");
}

#[test]
fn merge_outer_emits_both_sides_unmatched() {
    let src = merge_setup("outer");
    let out = run("merge_outer", &src);
    assert!(out.contains("nrows=5"), "got: {out:?}");
    assert!(out.contains("row=4,40,null"), "got: {out:?}");
    assert!(out.contains("row=5,null,FR"), "got: {out:?}");
}

#[test]
fn merge_invalid_how_raises() {
    let src = "\
from tabular import ColumnI64, DataFrame, Column
import tabular
fn make1() -> DataFrame:
    ids: List[i64] = []
    ids.append(1i64)
    c1: ColumnI64 = tabular.col_i64_simple(ids)
    cn: List[str] = []
    cn.append(\"k\")
    cs: List[Column] = []
    cs.append(c1)
    return tabular.from_columns(cn, cs)
fn main() -> i32:
    L: DataFrame = make1()
    R: DataFrame = make1()
    on: List[str] = []
    on.append(\"k\")
    try:
        out: DataFrame = L.merge(R, on, \"semi\")
        println(\"unexpected ok\")
    except ValueError as e:
        println(\"raised\")
    return 0
";
    let out = run("merge_invalid_how", src);
    assert!(out.contains("raised"), "got: {out:?}");
}

#[test]
fn merge_null_join_keys_drop_in_inner() {
    // A row with a null key in lhs must NOT match anything on inner;
    // in left it emits with rhs side null.
    let src = "\
from tabular import ColumnI64, ColumnStr, DataFrame, Column
import tabular
fn main() -> i32:
    lids: List[i64] = []
    lids.append(1i64)
    lids.append(0i64)
    lns: List[bool] = []
    lns.append(false)
    lns.append(true)
    lc: ColumnI64 = tabular.col_i64(lids, lns)
    lcn: List[str] = []
    lcn.append(\"k\")
    lcs: List[Column] = []
    lcs.append(lc)
    L: DataFrame = tabular.from_columns(lcn, lcs)
    rids: List[i64] = []
    rids.append(1i64)
    rc: ColumnI64 = tabular.col_i64_simple(rids)
    rnames: List[str] = []
    rnames.append(\"hit\")
    rname_col: ColumnStr = tabular.col_str_simple(rnames)
    rcn: List[str] = []
    rcn.append(\"k\")
    rcn.append(\"label\")
    rcs: List[Column] = []
    rcs.append(rc)
    rcs.append(rname_col)
    R: DataFrame = tabular.from_columns(rcn, rcs)
    on: List[str] = []
    on.append(\"k\")
    inner: DataFrame = L.merge(R, on, \"inner\")
    println(\"inner_nrows=\" + str(inner.length()))
    left: DataFrame = L.merge(R, on, \"left\")
    println(\"left_nrows=\" + str(left.length()))
    return 0
";
    let out = run("merge_null_keys", src);
    assert!(out.contains("inner_nrows=1"), "got: {out:?}");
    assert!(out.contains("left_nrows=2"), "got: {out:?}");
}
#[test]
fn concat_cols_row_mismatch_raises() {
    let src = "\
from tabular import ColumnI64, DataFrame, Column
import tabular
fn main() -> i32:
    v1: List[i64] = []
    v1.append(1i64)
    v1.append(2i64)
    c1: ColumnI64 = tabular.col_i64_simple(v1)
    cn1: List[str] = []
    cn1.append(\"a\")
    cs1: List[Column] = []
    cs1.append(c1)
    df1: DataFrame = tabular.from_columns(cn1, cs1)
    v2: List[i64] = []
    v2.append(3i64)
    c2: ColumnI64 = tabular.col_i64_simple(v2)
    cn2: List[str] = []
    cn2.append(\"b\")
    cs2: List[Column] = []
    cs2.append(c2)
    df2: DataFrame = tabular.from_columns(cn2, cs2)
    dfs: List[DataFrame] = []
    dfs.append(df1)
    dfs.append(df2)
    try:
        out: DataFrame = tabular.concat_cols(dfs)
        println(\"unexpected ok\")
    except ValueError as e:
        println(\"raised\")
    return 0
";
    let out = run("concat_cols_mismatch", src);
    assert!(out.contains("raised"), "got: {out:?}");
}
