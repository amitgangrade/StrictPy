//! M37 in-process regression tests for the `tabular` stdlib module.
//!
//! The module ships a sealed Column hierarchy (ColumnI64/F64/Str/Bool/
//! DateTime), a DataFrame class with named columns + RangeIndex, CSV/SQL
//! I/O, per-column comparison methods producing ColumnBool masks, mask
//! combinators, and `df.filter/select/drop/head/tail/sort_by`.  See
//! `LANGUAGE_GUIDE.md` §5 (post-M37) for the user-facing surface.
//!
//! Tests use temporary files under `CARGO_TARGET_TMPDIR` so they run
//! in parallel without filesystem races (each test name namespaces
//! its own `.spy` bytecode + any CSV side files).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m37_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

fn tmp_path(base: &str) -> String {
    let mut p = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    p.push(format!("strictpy_m37_{base}"));
    p.display().to_string().replace('\\', "/")
}

// ── Phase A: Column hierarchy + DataFrame core ────────────────────────

#[test]
fn construct_col_i64_and_check_length() {
    let src = "\
from tabular import ColumnI64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    vs.append(3i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    println(\"length=\" + str(c.length()))
    println(\"dtype=\" + c.dtype())
    println(\"null_count=\" + str(c.null_count()))
    return 0
";
    let p = compile_snippet("col_i64_basic", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("length=3"), "got: {out:?}");
    assert!(out.contains("dtype=i64"), "got: {out:?}");
    assert!(out.contains("null_count=0"), "got: {out:?}");
}

#[test]
fn col_i64_get_returns_value_and_none_on_null() {
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
    println(\"null_count=\" + str(c.null_count()))
    g0: i64? = c.get(0i64)
    if g0 is not none:
        println(\"g0=\" + str(g0))
    g1: i64? = c.get(1i64)
    if g1 is none:
        println(\"g1=null\")
    g2: i64? = c.get(2i64)
    if g2 is not none:
        println(\"g2=\" + str(g2))
    return 0
";
    let p = compile_snippet("col_i64_get_null", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("null_count=1"), "got: {out:?}");
    assert!(out.contains("g0=10"), "got: {out:?}");
    assert!(out.contains("g1=null"), "got: {out:?}");
    assert!(out.contains("g2=30"), "got: {out:?}");
}

#[test]
fn col_str_basic_construct_and_get() {
    let src = "\
from tabular import ColumnStr
import tabular
fn main() -> i32:
    vs: List[str] = []
    vs.append(\"alice\")
    vs.append(\"bob\")
    vs.append(\"carol\")
    c: ColumnStr = tabular.col_str_simple(vs)
    println(\"dtype=\" + c.dtype())
    println(\"length=\" + str(c.length()))
    g: str? = c.get(1i64)
    if g is not none:
        println(\"g1=\" + g)
    return 0
";
    let p = compile_snippet("col_str_basic", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("dtype=str"), "got: {out:?}");
    assert!(out.contains("length=3"), "got: {out:?}");
    assert!(out.contains("g1=bob"), "got: {out:?}");
}

#[test]
fn from_columns_builds_dataframe_and_inspects() {
    let src = "\
from tabular import ColumnI64, ColumnStr, Column, DataFrame
import tabular
fn main() -> i32:
    ages_v: List[i64] = []
    ages_v.append(30i64)
    ages_v.append(25i64)
    ages_v.append(40i64)
    ages: ColumnI64 = tabular.col_i64_simple(ages_v)
    names_v: List[str] = []
    names_v.append(\"alice\")
    names_v.append(\"bob\")
    names_v.append(\"carol\")
    names: ColumnStr = tabular.col_str_simple(names_v)
    col_names: List[str] = []
    col_names.append(\"age\")
    col_names.append(\"name\")
    cols: List[Column] = []
    cols.append(ages)
    cols.append(names)
    df: DataFrame = tabular.from_columns(col_names, cols)
    println(\"nrows=\" + str(df.length()))
    println(\"ncols=\" + str(df.ncols()))
    println(\"has_age=\" + str(df.has_column(\"age\")))
    println(\"has_foo=\" + str(df.has_column(\"foo\")))
    return 0
";
    let p = compile_snippet("df_from_cols", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("nrows=3"), "got: {out:?}");
    assert!(out.contains("ncols=2"), "got: {out:?}");
    assert!(out.contains("has_age=true"), "got: {out:?}");
    assert!(out.contains("has_foo=false"), "got: {out:?}");
}

#[test]
fn dataframe_show_renders_ascii_table() {
    let src = "\
from tabular import ColumnI64, ColumnStr, Column, DataFrame
import tabular
fn main() -> i32:
    ages_v: List[i64] = []
    ages_v.append(30i64)
    ages_v.append(25i64)
    ages: ColumnI64 = tabular.col_i64_simple(ages_v)
    names_v: List[str] = []
    names_v.append(\"alice\")
    names_v.append(\"bob\")
    names: ColumnStr = tabular.col_str_simple(names_v)
    col_names: List[str] = []
    col_names.append(\"age\")
    col_names.append(\"name\")
    cols: List[Column] = []
    cols.append(ages)
    cols.append(names)
    df: DataFrame = tabular.from_columns(col_names, cols)
    println(df.show(-1i64))
    return 0
";
    let p = compile_snippet("df_show", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("age"), "got: {out:?}");
    assert!(out.contains("name"), "got: {out:?}");
    assert!(out.contains("30"), "got: {out:?}");
    assert!(out.contains("alice"), "got: {out:?}");
    assert!(out.contains("[2 rows x 2 columns]"), "got: {out:?}");
}

#[test]
fn mismatched_lengths_raise_value_error() {
    let src = "\
from tabular import ColumnI64
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(1i64)
    vs.append(2i64)
    ns: List[bool] = []
    ns.append(false)
    try:
        c: ColumnI64 = tabular.col_i64(vs, ns)
        println(\"unreachable\")
    except ValueError:
        println(\"got value error\")
    return 0
";
    let p = compile_snippet("col_mismatch_error", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("got value error"), "got: {out:?}");
}

// ── Phase B: I/O ──────────────────────────────────────────────────────

#[test]
fn write_read_csv_round_trip() {
    let path = tmp_path("rt.csv");
    let src = format!("\
from tabular import ColumnI64, ColumnStr, Column, DataFrame
import tabular
fn main() -> i32:
    ages_v: List[i64] = []
    ages_v.append(30i64)
    ages_v.append(25i64)
    ages_v.append(40i64)
    ages: ColumnI64 = tabular.col_i64_simple(ages_v)
    names_v: List[str] = []
    names_v.append(\"alice\")
    names_v.append(\"bob\")
    names_v.append(\"carol\")
    names: ColumnStr = tabular.col_str_simple(names_v)
    col_names: List[str] = []
    col_names.append(\"age\")
    col_names.append(\"name\")
    cols: List[Column] = []
    cols.append(ages)
    cols.append(names)
    df: DataFrame = tabular.from_columns(col_names, cols)
    tabular.write_csv(\"{path}\", df)
    schema: List[Tuple[str, str]] = []
    schema.append((\"age\", \"i64\"))
    schema.append((\"name\", \"str\"))
    df2: DataFrame = tabular.read_csv(\"{path}\", schema)
    println(\"nrows=\" + str(df2.length()))
    println(\"ncols=\" + str(df2.ncols()))
    println(df2.show(-1i64))
    return 0
");
    let p = compile_snippet("write_read_csv", &src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("nrows=3"), "got: {out:?}");
    assert!(out.contains("ncols=2"), "got: {out:?}");
    assert!(out.contains("alice"), "got: {out:?}");
    assert!(out.contains("carol"), "got: {out:?}");
}

#[test]
fn read_csv_treats_empty_cells_as_null() {
    let path = tmp_path("nulls.csv");
    fs::write(&path, "x,y\n1,a\n,b\n3,\n").expect("write csv");
    let src = format!("\
from tabular import ColumnI64, DataFrame
import tabular
fn main() -> i32:
    schema: List[Tuple[str, str]] = []
    schema.append((\"x\", \"i64\"))
    schema.append((\"y\", \"str\"))
    df: DataFrame = tabular.read_csv(\"{path}\", schema)
    println(\"nrows=\" + str(df.length()))
    return 0
");
    let p = compile_snippet("read_csv_nulls", &src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("nrows=3"), "got: {out:?}");
}

#[test]
fn from_sql_drains_cursor_into_dataframe() {
    let src = "\
import sqlite3
import tabular
from tabular import DataFrame
fn main() -> i32:
    conn: Connection = sqlite3.open(\":memory:\")
    conn.execute(\"CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT, score REAL)\")
    p1: List[str] = []
    p1.append(\"alice\")
    p1.append(\"3.5\")
    conn.execute_params(\"INSERT INTO t (name, score) VALUES (?, ?)\", p1)
    p2: List[str] = []
    p2.append(\"bob\")
    p2.append(\"4.5\")
    conn.execute_params(\"INSERT INTO t (name, score) VALUES (?, ?)\", p2)
    cur: Cursor = conn.query(\"SELECT id, name, score FROM t ORDER BY id\")
    schema: List[Tuple[str, str]] = []
    schema.append((\"id\", \"i64\"))
    schema.append((\"name\", \"str\"))
    schema.append((\"score\", \"f64\"))
    df: DataFrame = tabular.from_sql(cur, schema)
    println(\"nrows=\" + str(df.length()))
    println(\"ncols=\" + str(df.ncols()))
    conn.close()
    return 0
";
    let p = compile_snippet("from_sql_basic", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("nrows=2"), "got: {out:?}");
    assert!(out.contains("ncols=3"), "got: {out:?}");
}

#[test]
fn from_rows_parses_bool_and_f64() {
    let src = "\
import tabular
from tabular import DataFrame
fn main() -> i32:
    row1: List[str] = []
    row1.append(\"true\")
    row1.append(\"3.14\")
    row2: List[str] = []
    row2.append(\"false\")
    row2.append(\"2.71\")
    rows: List[List[str]] = []
    rows.append(row1)
    rows.append(row2)
    schema: List[Tuple[str, str]] = []
    schema.append((\"on\", \"bool\"))
    schema.append((\"v\", \"f64\"))
    df: DataFrame = tabular.from_rows(rows, schema)
    println(\"nrows=\" + str(df.length()))
    return 0
";
    let p = compile_snippet("from_rows_bool_f64", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("nrows=2"), "got: {out:?}");
}

// ── Phase C: comparisons + masks + filter / select / drop / head / tail ─

#[test]
fn col_i64_gt_produces_mask_and_filter_works() {
    let src = "\
from tabular import ColumnI64, ColumnStr, ColumnBool, Column, DataFrame
import tabular
fn main() -> i32:
    ages_v: List[i64] = []
    ages_v.append(30i64)
    ages_v.append(25i64)
    ages_v.append(40i64)
    ages: ColumnI64 = tabular.col_i64_simple(ages_v)
    names_v: List[str] = []
    names_v.append(\"alice\")
    names_v.append(\"bob\")
    names_v.append(\"carol\")
    names: ColumnStr = tabular.col_str_simple(names_v)
    col_names: List[str] = []
    col_names.append(\"age\")
    col_names.append(\"name\")
    cols: List[Column] = []
    cols.append(ages)
    cols.append(names)
    df: DataFrame = tabular.from_columns(col_names, cols)
    mask: ColumnBool = ages.gt(28i64)
    println(\"count=\" + str(mask.count_true()))
    df2: DataFrame = df.filter(mask)
    println(\"filt_nrows=\" + str(df2.length()))
    println(df2.show(-1i64))
    return 0
";
    let p = compile_snippet("col_gt_filter", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("count=2"), "got: {out:?}");
    assert!(out.contains("filt_nrows=2"), "got: {out:?}");
    assert!(out.contains("alice"), "got: {out:?}");
    assert!(out.contains("carol"), "got: {out:?}");
}

#[test]
fn mask_combinators_and_or_not() {
    let src = "\
from tabular import ColumnI64, ColumnBool
import tabular
fn main() -> i32:
    vs: List[i64] = []
    vs.append(10i64)
    vs.append(20i64)
    vs.append(30i64)
    vs.append(40i64)
    c: ColumnI64 = tabular.col_i64_simple(vs)
    m1: ColumnBool = c.gt(15i64)
    m2: ColumnBool = c.lt(35i64)
    both: ColumnBool = m1.and_(m2)
    println(\"both=\" + str(both.count_true()))
    either: ColumnBool = m1.or_(m2)
    println(\"either=\" + str(either.count_true()))
    not_m1: ColumnBool = m1.not_()
    println(\"not_m1=\" + str(not_m1.count_true()))
    return 0
";
    let p = compile_snippet("mask_combinators", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("both=2"), "got: {out:?}");
    assert!(out.contains("either=4"), "got: {out:?}");
    assert!(out.contains("not_m1=1"), "got: {out:?}");
}

#[test]
fn df_select_drop_head_tail() {
    let src = "\
from tabular import ColumnI64, ColumnStr, Column, DataFrame
import tabular
fn main() -> i32:
    a_v: List[i64] = []
    a_v.append(1i64)
    a_v.append(2i64)
    a_v.append(3i64)
    a_v.append(4i64)
    a: ColumnI64 = tabular.col_i64_simple(a_v)
    b_v: List[str] = []
    b_v.append(\"w\")
    b_v.append(\"x\")
    b_v.append(\"y\")
    b_v.append(\"z\")
    b: ColumnStr = tabular.col_str_simple(b_v)
    col_names: List[str] = []
    col_names.append(\"n\")
    col_names.append(\"c\")
    cols: List[Column] = []
    cols.append(a)
    cols.append(b)
    df: DataFrame = tabular.from_columns(col_names, cols)
    pick: List[str] = []
    pick.append(\"c\")
    df_pick: DataFrame = df.select(pick)
    println(\"pick_ncols=\" + str(df_pick.ncols()))
    drop: List[str] = []
    drop.append(\"n\")
    df_drop: DataFrame = df.drop(drop)
    println(\"drop_ncols=\" + str(df_drop.ncols()))
    df_head: DataFrame = df.head(2i64)
    println(\"head_nrows=\" + str(df_head.length()))
    df_tail: DataFrame = df.tail(2i64)
    println(\"tail_nrows=\" + str(df_tail.length()))
    return 0
";
    let p = compile_snippet("df_select_drop", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("pick_ncols=1"), "got: {out:?}");
    assert!(out.contains("drop_ncols=1"), "got: {out:?}");
    assert!(out.contains("head_nrows=2"), "got: {out:?}");
    assert!(out.contains("tail_nrows=2"), "got: {out:?}");
}

#[test]
fn col_str_eq_and_contains() {
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
    e: ColumnBool = c.eq(\"beta\")
    println(\"eq=\" + str(e.count_true()))
    co: ColumnBool = c.contains(\"alph\")
    println(\"contains=\" + str(co.count_true()))
    return 0
";
    let p = compile_snippet("col_str_eq_contains", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("eq=1"), "got: {out:?}");
    assert!(out.contains("contains=2"), "got: {out:?}");
}

// ── Phase D: sort_by ─────────────────────────────────────────────────

#[test]
fn sort_by_ascending_and_descending() {
    let src = "\
from tabular import ColumnI64, ColumnStr, Column, DataFrame
import tabular
fn main() -> i32:
    ages_v: List[i64] = []
    ages_v.append(30i64)
    ages_v.append(25i64)
    ages_v.append(40i64)
    ages: ColumnI64 = tabular.col_i64_simple(ages_v)
    names_v: List[str] = []
    names_v.append(\"alice\")
    names_v.append(\"bob\")
    names_v.append(\"carol\")
    names: ColumnStr = tabular.col_str_simple(names_v)
    col_names: List[str] = []
    col_names.append(\"age\")
    col_names.append(\"name\")
    cols: List[Column] = []
    cols.append(ages)
    cols.append(names)
    df: DataFrame = tabular.from_columns(col_names, cols)
    sorted_asc: DataFrame = df.sort_by(\"age\", true)
    row0: List[str] = sorted_asc.row(0i64)
    println(\"asc_row0=\" + row0[0i32] + \",\" + row0[1i32])
    sorted_desc: DataFrame = df.sort_by(\"age\", false)
    row0d: List[str] = sorted_desc.row(0i64)
    println(\"desc_row0=\" + row0d[0i32] + \",\" + row0d[1i32])
    return 0
";
    let p = compile_snippet("sort_basic", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("asc_row0=25,bob"), "got: {out:?}");
    assert!(out.contains("desc_row0=40,carol"), "got: {out:?}");
}

#[test]
fn sort_by_str_ascending_alphabetical() {
    let src = "\
from tabular import ColumnI64, ColumnStr, Column, DataFrame
import tabular
fn main() -> i32:
    a_v: List[i64] = []
    a_v.append(30i64)
    a_v.append(25i64)
    a_v.append(40i64)
    a: ColumnI64 = tabular.col_i64_simple(a_v)
    n_v: List[str] = []
    n_v.append(\"carol\")
    n_v.append(\"alice\")
    n_v.append(\"bob\")
    n: ColumnStr = tabular.col_str_simple(n_v)
    col_names: List[str] = []
    col_names.append(\"age\")
    col_names.append(\"name\")
    cols: List[Column] = []
    cols.append(a)
    cols.append(n)
    df: DataFrame = tabular.from_columns(col_names, cols)
    sorted_df: DataFrame = df.sort_by(\"name\", true)
    r0: List[str] = sorted_df.row(0i64)
    r1: List[str] = sorted_df.row(1i64)
    r2: List[str] = sorted_df.row(2i64)
    println(\"r0=\" + r0[1i32])
    println(\"r1=\" + r1[1i32])
    println(\"r2=\" + r2[1i32])
    return 0
";
    let p = compile_snippet("sort_str_asc", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("r0=alice"), "got: {out:?}");
    assert!(out.contains("r1=bob"), "got: {out:?}");
    assert!(out.contains("r2=carol"), "got: {out:?}");
}

#[test]
fn sort_by_nulls_go_to_end() {
    let src = "\
from tabular import ColumnI64, Column, DataFrame
import tabular
fn main() -> i32:
    a_v: List[i64] = []
    a_v.append(30i64)
    a_v.append(0i64)
    a_v.append(10i64)
    a_n: List[bool] = []
    a_n.append(false)
    a_n.append(true)
    a_n.append(false)
    a: ColumnI64 = tabular.col_i64(a_v, a_n)
    col_names: List[str] = []
    col_names.append(\"x\")
    cols: List[Column] = []
    cols.append(a)
    df: DataFrame = tabular.from_columns(col_names, cols)
    sorted_df: DataFrame = df.sort_by(\"x\", true)
    r0: List[str] = sorted_df.row(0i64)
    r1: List[str] = sorted_df.row(1i64)
    r2: List[str] = sorted_df.row(2i64)
    println(\"r0=\" + r0[0i32])
    println(\"r1=\" + r1[0i32])
    println(\"r2=\" + r2[0i32])
    return 0
";
    let p = compile_snippet("sort_nulls_end", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("r0=10"), "got: {out:?}");
    assert!(out.contains("r1=30"), "got: {out:?}");
    assert!(out.contains("r2=null"), "got: {out:?}");
}

#[test]
fn null_propagates_in_comparison() {
    let src = "\
from tabular import ColumnI64, ColumnBool
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
    m: ColumnBool = c.gt(5i64)
    println(\"true=\" + str(m.count_true()))
    println(\"null_count=\" + str(m.null_count()))
    return 0
";
    let p = compile_snippet("null_prop_cmp", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("true=2"), "got: {out:?}");
    assert!(out.contains("null_count=1"), "got: {out:?}");
}

#[test]
fn dtypes_returns_all_column_dtypes() {
    let src = "\
from tabular import ColumnI64, ColumnStr, ColumnF64, ColumnBool, Column, DataFrame
import tabular
fn main() -> i32:
    a_v: List[i64] = []
    a_v.append(1i64)
    a: ColumnI64 = tabular.col_i64_simple(a_v)
    b_v: List[f64] = []
    b_v.append(1.0)
    b: ColumnF64 = tabular.col_f64_simple(b_v)
    c_v: List[str] = []
    c_v.append(\"x\")
    c: ColumnStr = tabular.col_str_simple(c_v)
    d_v: List[bool] = []
    d_v.append(true)
    d: ColumnBool = tabular.col_bool_simple(d_v)
    col_names: List[str] = []
    col_names.append(\"i\")
    col_names.append(\"f\")
    col_names.append(\"s\")
    col_names.append(\"b\")
    cols: List[Column] = []
    cols.append(a)
    cols.append(b)
    cols.append(c)
    cols.append(d)
    df: DataFrame = tabular.from_columns(col_names, cols)
    dts: List[str] = df.dtypes()
    i: i32 = 0i32
    while i < i32(i64(len(dts))):
        println(\"dt=\" + dts[i])
        i = i + 1i32
    return 0
";
    let p = compile_snippet("dtypes_all", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("dt=i64"), "got: {out:?}");
    assert!(out.contains("dt=f64"), "got: {out:?}");
    assert!(out.contains("dt=str"), "got: {out:?}");
    assert!(out.contains("dt=bool"), "got: {out:?}");
}
