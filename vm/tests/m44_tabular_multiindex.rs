//! M44 in-process regression tests for `tabular` MultiIndex.
//!
//! M41 added an optional single-column index; M42 propagated it through
//! 11 row/column-transforming methods; M43 finished the v1 single-index
//! story through the reshape side.  M44 adds **MultiIndex** — the
//! headline missing piece for multi-column `group_by` to produce a
//! structured row label.
//!
//! M44a scope: storage + accessors + multi-column group_by promotion +
//! minimal propagation through filter / head / tail / iloc.  Other ops
//! drop the MultiIndex back to RangeIndex (M44b anchor).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m44_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

fn run(name: &str, src: &str) -> String {
    let p = compile_snippet(name, src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "{name}: exit={code} stdout={out:?}");
    out
}

// Common DataFrame builder used across many tests.
const MULTI_FRAME_HEADER: &str = "\
from tabular import Column, ColumnI64, ColumnStr, GroupedDataFrame, DataFrame
import tabular
fn make_frame() -> DataFrame:
    regs_v: List[str] = []
    regs_v.append(\"east\")
    regs_v.append(\"east\")
    regs_v.append(\"west\")
    regs_v.append(\"west\")
    regs_v.append(\"east\")
    regs: ColumnStr = tabular.col_str_simple(regs_v)
    cats_v: List[str] = []
    cats_v.append(\"a\")
    cats_v.append(\"b\")
    cats_v.append(\"a\")
    cats_v.append(\"b\")
    cats_v.append(\"a\")
    cats: ColumnStr = tabular.col_str_simple(cats_v)
    qty_v: List[i64] = []
    qty_v.append(10i64)
    qty_v.append(20i64)
    qty_v.append(30i64)
    qty_v.append(40i64)
    qty_v.append(50i64)
    qty: ColumnI64 = tabular.col_i64_simple(qty_v)
    n: List[str] = []
    n.append(\"reg\")
    n.append(\"cat\")
    n.append(\"qty\")
    cols: List[Column] = []
    cols.append(regs)
    cols.append(cats)
    cols.append(qty)
    return tabular.from_columns(n, cols)
";

// ───────────────────────────────────────────────────────────────────────
// Phase A: storage + accessors
// ───────────────────────────────────────────────────────────────────────

#[test]
fn set_index_multi_round_trip() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    println(\"ncols=\" + str(mi.ncols()))\n    println(\"nlev=\" + str(mi.index_nlevels()))\n    rr: DataFrame = mi.reset_index_multi()\n    println(\"rcols=\" + str(rr.ncols()))\n    println(\"rlev=\" + str(rr.index_nlevels()))\n    rn: List[str] = rr.columns()\n    println(\"col0=\" + rn[0i32])\n    println(\"col1=\" + rn[1i32])\n    return 0\n"
    );
    let out = run("set_index_multi_round_trip", &src);
    // After set_index_multi: qty remains as the only regular col; nlevels=2.
    assert!(out.contains("ncols=1"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
    // After reset_index_multi: 3 regular cols; back to RangeIndex.
    assert!(out.contains("rcols=3"), "got: {out:?}");
    assert!(out.contains("rlev=0"), "got: {out:?}");
    assert!(out.contains("col0=reg"), "got: {out:?}");
    assert!(out.contains("col1=cat"), "got: {out:?}");
}

#[test]
fn set_index_multi_empty_cols_raises() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    try:\n        mi: DataFrame = df.set_index_multi(keys)\n        println(\"no-raise\")\n    except ValueError:\n        println(\"got-valueerror\")\n    return 0\n"
    );
    let out = run("set_index_multi_empty", &src);
    assert!(out.contains("got-valueerror"), "got: {out:?}");
}

#[test]
fn set_index_multi_missing_col_raises() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"missing\")\n    try:\n        mi: DataFrame = df.set_index_multi(keys)\n        println(\"no-raise\")\n    except ValueError:\n        println(\"got-valueerror\")\n    return 0\n"
    );
    let out = run("set_index_multi_missing", &src);
    assert!(out.contains("got-valueerror"), "got: {out:?}");
}

#[test]
fn set_index_multi_on_already_indexed_raises() {
    // After set_index single-col, set_index_multi must raise.
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    si: DataFrame = df.set_index(\"reg\")\n    keys: List[str] = []\n    keys.append(\"cat\")\n    try:\n        mi: DataFrame = si.set_index_multi(keys)\n        println(\"no-raise\")\n    except ValueError:\n        println(\"got-valueerror\")\n    return 0\n"
    );
    let out = run("set_index_multi_already_indexed", &src);
    assert!(out.contains("got-valueerror"), "got: {out:?}");
}

#[test]
fn index_nlevels_0_for_rangeindex() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    println(\"nlev=\" + str(df.index_nlevels()))\n    return 0\n"
    );
    let out = run("index_nlevels_0", &src);
    assert!(out.contains("nlev=0"), "got: {out:?}");
}

#[test]
fn index_nlevels_1_for_single_col_index() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    si: DataFrame = df.set_index(\"reg\")\n    println(\"nlev=\" + str(si.index_nlevels()))\n    return 0\n"
    );
    let out = run("index_nlevels_1", &src);
    assert!(out.contains("nlev=1"), "got: {out:?}");
}

#[test]
fn index_level_returns_column_or_none() {
    // Verify presence (not none) for levels in range, and none for out of range.
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    l0: Column? = mi.index_level(0i64)\n    if l0 is not none:\n        println(\"l0=set\")\n    l1: Column? = mi.index_level(1i64)\n    if l1 is not none:\n        println(\"l1=set\")\n    l5: Column? = mi.index_level(5i64)\n    if l5 is none:\n        println(\"l5=none\")\n    return 0\n"
    );
    let out = run("index_level_basic", &src);
    assert!(out.contains("l0=set"), "got: {out:?}");
    assert!(out.contains("l1=set"), "got: {out:?}");
    assert!(out.contains("l5=none"), "got: {out:?}");
}

#[test]
fn index_level_name_returns_str_or_none() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    n0: str? = mi.index_level_name(0i64)\n    if n0 is not none:\n        println(\"n0=\" + n0)\n    n1: str? = mi.index_level_name(1i64)\n    if n1 is not none:\n        println(\"n1=\" + n1)\n    n5: str? = mi.index_level_name(5i64)\n    if n5 is none:\n        println(\"n5=none\")\n    return 0\n"
    );
    let out = run("index_level_name_basic", &src);
    assert!(out.contains("n0=reg"), "got: {out:?}");
    assert!(out.contains("n1=cat"), "got: {out:?}");
    assert!(out.contains("n5=none"), "got: {out:?}");
}

#[test]
fn sort_index_multi_lex_ascending() {
    // Build a 2-level MultiIndex and verify the row ordering.  Levels:
    // [("east","a"), ("east","b"), ("west","a"), ("west","b"), ("east","a")]
    // After ascending sort by (reg, cat): east-a (rows 0+4), east-b (row 1),
    // west-a (row 2), west-b (row 3).  Check qty ordering.
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    so: DataFrame = mi.sort_index_multi(true)\n    qcol: ColumnI64? = so.get_column_i64(\"qty\")\n    if qcol is not none:\n        println(\"q0=\" + str(qcol.get(0i64)))\n        println(\"q1=\" + str(qcol.get(1i64)))\n        println(\"q2=\" + str(qcol.get(2i64)))\n        println(\"q3=\" + str(qcol.get(3i64)))\n        println(\"q4=\" + str(qcol.get(4i64)))\n    return 0\n"
    );
    let out = run("sort_index_multi_asc", &src);
    // Original (reg, cat) order: east-a (10), east-b (20), west-a (30),
    // west-b (40), east-a (50). Sorted ascending: east-a (10), east-a (50),
    // east-b (20), west-a (30), west-b (40).
    assert!(out.contains("q0=10"), "got: {out:?}");
    assert!(out.contains("q1=50"), "got: {out:?}");
    assert!(out.contains("q2=20"), "got: {out:?}");
    assert!(out.contains("q3=30"), "got: {out:?}");
    assert!(out.contains("q4=40"), "got: {out:?}");
}

#[test]
fn sort_index_multi_descending_reverses() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    so: DataFrame = mi.sort_index_multi(false)\n    qcol: ColumnI64? = so.get_column_i64(\"qty\")\n    if qcol is not none:\n        println(\"q0=\" + str(qcol.get(0i64)))\n        println(\"q4=\" + str(qcol.get(4i64)))\n    return 0\n"
    );
    let out = run("sort_index_multi_desc", &src);
    // Descending lex: reverse of ascending = [40, 30, 20, 50, 10]
    assert!(out.contains("q0=40"), "got: {out:?}");
    assert!(out.contains("q4=10"), "got: {out:?}");
}

#[test]
fn sort_index_multi_without_multiindex_raises() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    try:\n        so: DataFrame = df.sort_index_multi(true)\n        println(\"no-raise\")\n    except ValueError:\n        println(\"got-valueerror\")\n    return 0\n"
    );
    let out = run("sort_index_multi_no_mi", &src);
    assert!(out.contains("got-valueerror"), "got: {out:?}");
}
