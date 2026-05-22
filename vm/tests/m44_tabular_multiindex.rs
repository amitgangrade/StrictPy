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

// ───────────────────────────────────────────────────────────────────────
// Phase B: multi-column group_by promotion
// ───────────────────────────────────────────────────────────────────────

#[test]
fn two_level_group_by_sum_promotes_to_multiindex() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    gdf: GroupedDataFrame = df.group_by(keys)\n    s: DataFrame = gdf.sum()\n    println(\"rows=\" + str(s.length()))\n    println(\"cols=\" + str(s.ncols()))\n    println(\"nlev=\" + str(s.index_nlevels()))\n    cnames: List[str] = s.columns()\n    println(\"col0=\" + cnames[0i32])\n    n0: str? = s.index_level_name(0i64)\n    n1: str? = s.index_level_name(1i64)\n    if n0 is not none:\n        println(\"n0=\" + n0)\n    if n1 is not none:\n        println(\"n1=\" + n1)\n    return 0\n"
    );
    let out = run("two_lvl_sum_promotes", &src);
    // Groups: east-a (10+50=60), east-b (20), west-a (30), west-b (40) = 4 rows.
    assert!(out.contains("rows=4"), "got: {out:?}");
    assert!(out.contains("cols=1"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
    assert!(out.contains("col0=qty"), "got: {out:?}");
    assert!(out.contains("n0=reg"), "got: {out:?}");
    assert!(out.contains("n1=cat"), "got: {out:?}");
}

#[test]
fn two_level_group_by_mean_emits_columnf64_with_multiindex() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    gdf: GroupedDataFrame = df.group_by(keys)\n    m: DataFrame = gdf.mean()\n    println(\"cols=\" + str(m.ncols()))\n    println(\"nlev=\" + str(m.index_nlevels()))\n    qcol: ColumnF64? = m.get_column_f64(\"qty\")\n    if qcol is not none:\n        println(\"q_dt=\" + qcol.dtype())\n    return 0\n"
    );
    let out = run("two_lvl_mean", &src);
    assert!(out.contains("cols=1"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
    assert!(out.contains("q_dt=f64"), "got: {out:?}");
}

#[test]
fn three_level_group_by_size_three_levels() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, GroupedDataFrame, DataFrame
import tabular
fn main() -> i32:
    a_v: List[str] = []
    a_v.append(\"x\")
    a_v.append(\"x\")
    a_v.append(\"y\")
    a_v.append(\"y\")
    av: ColumnStr = tabular.col_str_simple(a_v)
    b_v: List[str] = []
    b_v.append(\"p\")
    b_v.append(\"p\")
    b_v.append(\"q\")
    b_v.append(\"q\")
    bv: ColumnStr = tabular.col_str_simple(b_v)
    c_v: List[str] = []
    c_v.append(\"l\")
    c_v.append(\"m\")
    c_v.append(\"l\")
    c_v.append(\"m\")
    cv: ColumnStr = tabular.col_str_simple(c_v)
    qty_v: List[i64] = []
    qty_v.append(1i64)
    qty_v.append(2i64)
    qty_v.append(3i64)
    qty_v.append(4i64)
    qty: ColumnI64 = tabular.col_i64_simple(qty_v)
    n: List[str] = []
    n.append(\"a\")
    n.append(\"b\")
    n.append(\"c\")
    n.append(\"qty\")
    cols: List[Column] = []
    cols.append(av)
    cols.append(bv)
    cols.append(cv)
    cols.append(qty)
    df: DataFrame = tabular.from_columns(n, cols)
    keys: List[str] = []
    keys.append(\"a\")
    keys.append(\"b\")
    keys.append(\"c\")
    gdf: GroupedDataFrame = df.group_by(keys)
    sz: DataFrame = gdf.size()
    println(\"rows=\" + str(sz.length()))
    println(\"cols=\" + str(sz.ncols()))
    println(\"nlev=\" + str(sz.index_nlevels()))
    return 0
";
    let out = run("three_lvl_size", src);
    // 4 unique (a, b, c) tuples.
    assert!(out.contains("rows=4"), "got: {out:?}");
    // size adds 1 column ("size").
    assert!(out.contains("cols=1"), "got: {out:?}");
    assert!(out.contains("nlev=3"), "got: {out:?}");
}

#[test]
fn two_level_group_by_agg_specs_promotes() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    gdf: GroupedDataFrame = df.group_by(keys)\n    specs: List[Tuple[str, str]] = []\n    specs.append((\"qty\", \"sum\"))\n    specs.append((\"qty\", \"max\"))\n    a: DataFrame = gdf.agg(specs)\n    println(\"cols=\" + str(a.ncols()))\n    println(\"nlev=\" + str(a.index_nlevels()))\n    onames: List[str] = a.columns()\n    println(\"col0=\" + onames[0i32])\n    println(\"col1=\" + onames[1i32])\n    return 0\n"
    );
    let out = run("two_lvl_agg_specs", &src);
    assert!(out.contains("cols=2"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
    assert!(out.contains("col0=qty_sum"), "got: {out:?}");
    assert!(out.contains("col1=qty_max"), "got: {out:?}");
}

#[test]
fn two_level_group_by_keys_returns_zero_cols_with_multiindex() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    gdf: GroupedDataFrame = df.group_by(keys)\n    k: DataFrame = gdf.keys()\n    println(\"rows=\" + str(k.length()))\n    println(\"cols=\" + str(k.ncols()))\n    println(\"nlev=\" + str(k.index_nlevels()))\n    return 0\n"
    );
    let out = run("two_lvl_keys", &src);
    // 4 unique (reg, cat) tuples; 0 regular columns; nlev=2.
    assert!(out.contains("rows=4"), "got: {out:?}");
    assert!(out.contains("cols=0"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn two_level_group_by_size_has_size_column() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    gdf: GroupedDataFrame = df.group_by(keys)\n    sz: DataFrame = gdf.size()\n    println(\"cols=\" + str(sz.ncols()))\n    println(\"nlev=\" + str(sz.index_nlevels()))\n    cn: List[str] = sz.columns()\n    println(\"col0=\" + cn[0i32])\n    return 0\n"
    );
    let out = run("two_lvl_size_col", &src);
    assert!(out.contains("cols=1"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
    assert!(out.contains("col0=size"), "got: {out:?}");
}

// ───────────────────────────────────────────────────────────────────────
// Phase C: minimal MultiIndex propagation through filter/head/tail/iloc
// ───────────────────────────────────────────────────────────────────────

#[test]
fn filter_preserves_multiindex() {
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, ColumnBool, GroupedDataFrame, DataFrame
import tabular
fn main() -> i32:
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
    df: DataFrame = tabular.from_columns(n, cols)
    keys: List[str] = []
    keys.append(\"reg\")
    keys.append(\"cat\")
    mi: DataFrame = df.set_index_multi(keys)
    mask_vs: List[bool] = []
    mask_vs.append(false)
    mask_vs.append(false)
    mask_vs.append(true)
    mask_vs.append(true)
    mask_vs.append(true)
    mask_ns: List[bool] = []
    mask_ns.append(false)
    mask_ns.append(false)
    mask_ns.append(false)
    mask_ns.append(false)
    mask_ns.append(false)
    mask: ColumnBool = tabular.col_bool(mask_vs, mask_ns)
    fdf: DataFrame = mi.filter(mask)
    println(\"rows=\" + str(fdf.length()))
    println(\"nlev=\" + str(fdf.index_nlevels()))
    return 0
";
    let out = run("filter_keeps_mi", src);
    // 3 rows pass the mask.
    assert!(out.contains("rows=3"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn head_preserves_multiindex() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    h: DataFrame = mi.head(2i64)\n    println(\"rows=\" + str(h.length()))\n    println(\"nlev=\" + str(h.index_nlevels()))\n    return 0\n"
    );
    let out = run("head_keeps_mi", &src);
    assert!(out.contains("rows=2"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn tail_preserves_multiindex() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    t: DataFrame = mi.tail(2i64)\n    println(\"rows=\" + str(t.length()))\n    println(\"nlev=\" + str(t.index_nlevels()))\n    qcol: ColumnI64? = t.get_column_i64(\"qty\")\n    if qcol is not none:\n        println(\"q0=\" + str(qcol.get(0i64)))\n        println(\"q1=\" + str(qcol.get(1i64)))\n    return 0\n"
    );
    let out = run("tail_keeps_mi", &src);
    assert!(out.contains("rows=2"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
    // Last 2 rows: qty=40, 50
    assert!(out.contains("q0=40"), "got: {out:?}");
    assert!(out.contains("q1=50"), "got: {out:?}");
}

#[test]
fn iloc_preserves_multiindex() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    sl: DataFrame = mi.iloc(1i64, 4i64)\n    println(\"rows=\" + str(sl.length()))\n    println(\"nlev=\" + str(sl.index_nlevels()))\n    return 0\n"
    );
    let out = run("iloc_keeps_mi", &src);
    // iloc(1, 4) → 3 rows
    assert!(out.contains("rows=3"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn group_by_then_filter_keeps_multiindex() {
    // The full "chained workflow": group_by -> sum -> filter still has MultiIndex.
    // group_by sum on (reg,cat) yields 4 rows: east-a=60, east-b=20, west-a=30,
    // west-b=40.  Filter to qty > 25: east-a, west-a, west-b = 3 rows.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, ColumnBool, GroupedDataFrame, DataFrame
import tabular
fn main() -> i32:
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
    df: DataFrame = tabular.from_columns(n, cols)
    keys: List[str] = []
    keys.append(\"reg\")
    keys.append(\"cat\")
    gdf: GroupedDataFrame = df.group_by(keys)
    s: DataFrame = gdf.sum()
    qcol: ColumnI64? = s.get_column_i64(\"qty\")
    if qcol is none:
        return 1
    mvs: List[bool] = []
    mns: List[bool] = []
    i: i64 = 0i64
    while i < s.length():
        ov: i64? = qcol.get(i)
        v: i64 = 0i64
        if ov is not none:
            v = ov
        if v > 25i64:
            mvs.append(true)
        else:
            mvs.append(false)
        mns.append(false)
        i = i + 1i64
    mask: ColumnBool = tabular.col_bool(mvs, mns)
    fdf: DataFrame = s.filter(mask)
    println(\"rows=\" + str(fdf.length()))
    println(\"nlev=\" + str(fdf.index_nlevels()))
    return 0
";
    let out = run("gby_then_filter_mi", src);
    assert!(out.contains("rows=3"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn sort_by_drops_multiindex_m44b_anchor() {
    // M44a contract: any row-transforming op OTHER than filter/head/tail/iloc
    // drops a MultiIndex back to RangeIndex.  M44b will lift this.
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    so: DataFrame = mi.sort_by(\"qty\", true)\n    println(\"nlev=\" + str(so.index_nlevels()))\n    return 0\n"
    );
    let out = run("sort_by_drops_mi", &src);
    assert!(out.contains("nlev=0"), "got: {out:?}");
}

#[test]
fn select_drops_multiindex_m44b_anchor() {
    // M44a contract: select drops a MultiIndex (M44b lifts this).
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    want: List[str] = []\n    want.append(\"qty\")\n    se: DataFrame = mi.select(want)\n    println(\"nlev=\" + str(se.index_nlevels()))\n    return 0\n"
    );
    let out = run("select_drops_mi", &src);
    assert!(out.contains("nlev=0"), "got: {out:?}");
}

#[test]
fn multi_col_group_by_no_longer_keeps_keys_as_columns() {
    // The M43→M44 contract flip — the keys are now levels, NOT regular columns.
    // We assert that the result has only qty as a regular column (cat and
    // reg are now MultiIndex levels), so the column list length is 1 and
    // its sole entry is "qty".
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    gdf: GroupedDataFrame = df.group_by(keys)\n    s: DataFrame = gdf.sum()\n    cnames: List[str] = s.columns()\n    println(\"ncols=\" + str(s.ncols()))\n    println(\"only=\" + cnames[0i32])\n    return 0\n"
    );
    let out = run("multi_no_keys_as_cols", &src);
    assert!(out.contains("ncols=1"), "got: {out:?}");
    assert!(out.contains("only=qty"), "got: {out:?}");
}
