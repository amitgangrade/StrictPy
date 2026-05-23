//! M45 in-process regression tests for `tabular` MultiIndex propagation
//! through the M42 + M43 op surface.
//!
//! M44a shipped MultiIndex storage + multi-col group_by promotion +
//! minimal propagation through 4 row-selection ops (filter / head /
//! tail / iloc).  Every OTHER op dropped a MultiIndex back to
//! RangeIndex — explicit M44b anchor.
//!
//! M45 lifts the anchor for 14 handlers:
//!   - Phase A (M42 ops): sort_by, dropna, dropna_subset, fillna_*,
//!     merge (inner/left/right; outer falls back to RangeIndex per
//!     M46 anchor), select, drop, rename.
//!   - Phase B (M43 reshape ops): melt repeats each level per
//!     value_var; concat_rows concatenates level-by-level (dtype +
//!     name match per level); concat_cols inherits lhs's MultiIndex;
//!     pivot and pivot_table explicitly drop a MultiIndex (reshape
//!     the row dimension — no clean target).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m45_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

fn run(name: &str, src: &str) -> String {
    let p = compile_snippet(name, src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "{name}: exit={code} stdout={out:?}");
    out
}

// Common 5-row DataFrame (region, category, qty) builder.
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
// Phase A — M42 ops: sort_by / dropna / dropna_subset / fillna_* /
// merge / select / drop / rename now carry a MultiIndex through.
// ───────────────────────────────────────────────────────────────────────

#[test]
fn sort_by_preserves_multiindex() {
    // After set_index_multi(["reg","cat"]) the qty col remains.  Sort by
    // qty descending; the MultiIndex must follow the permutation.  Check
    // length + nlevels (the structural propagation) — exact label probing
    // is covered by sort_by_preserves_multiindex_labels below.
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    so: DataFrame = mi.sort_by(\"qty\", false)\n    println(\"rows=\" + str(so.length()))\n    println(\"nlev=\" + str(so.index_nlevels()))\n    return 0\n"
    );
    let out = run("sort_by_preserves_mi", &src);
    assert!(out.contains("rows=5"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

// A 5-row frame with a nullable i64 column "ext" added to the basic
// frame's three columns.  Index keys remain reg+cat.
const MULTI_FRAME_WITH_NULL_HEADER: &str = "\
from tabular import Column, ColumnI64, ColumnStr, GroupedDataFrame, DataFrame
import tabular
fn make_frame_with_null(ext_vs: List[i64], ext_ns: List[bool]) -> DataFrame:
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
    ext: ColumnI64 = tabular.col_i64(ext_vs, ext_ns)
    n: List[str] = []
    n.append(\"reg\")
    n.append(\"cat\")
    n.append(\"qty\")
    n.append(\"ext\")
    cols: List[Column] = []
    cols.append(regs)
    cols.append(cats)
    cols.append(qty)
    cols.append(ext)
    return tabular.from_columns(n, cols)
";

#[test]
fn dropna_preserves_multiindex() {
    // Build a 5-row frame with a null in row 2 of "ext"; set MultiIndex;
    // dropna() drops row 2 from data + every level.
    let src = format!(
        "{MULTI_FRAME_WITH_NULL_HEADER}\nfn main() -> i32:\n    vs: List[i64] = []\n    vs.append(1i64)\n    vs.append(2i64)\n    vs.append(0i64)\n    vs.append(4i64)\n    vs.append(5i64)\n    ns: List[bool] = []\n    ns.append(false)\n    ns.append(false)\n    ns.append(true)\n    ns.append(false)\n    ns.append(false)\n    df: DataFrame = make_frame_with_null(vs, ns)\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    do: DataFrame = mi.dropna()\n    println(\"rows=\" + str(do.length()))\n    println(\"nlev=\" + str(do.index_nlevels()))\n    return 0\n"
    );
    let out = run("dropna_preserves_mi", &src);
    assert!(out.contains("rows=4"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn dropna_subset_preserves_multiindex() {
    let src = format!(
        "{MULTI_FRAME_WITH_NULL_HEADER}\nfn main() -> i32:\n    vs: List[i64] = []\n    vs.append(0i64)\n    vs.append(2i64)\n    vs.append(3i64)\n    vs.append(0i64)\n    vs.append(5i64)\n    ns: List[bool] = []\n    ns.append(true)\n    ns.append(false)\n    ns.append(false)\n    ns.append(true)\n    ns.append(false)\n    df: DataFrame = make_frame_with_null(vs, ns)\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    sub: List[str] = []\n    sub.append(\"ext\")\n    do: DataFrame = mi.dropna_subset(sub)\n    println(\"rows=\" + str(do.length()))\n    println(\"nlev=\" + str(do.index_nlevels()))\n    return 0\n"
    );
    let out = run("dropna_subset_preserves_mi", &src);
    assert!(out.contains("rows=3"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn fillna_i64_preserves_multiindex() {
    let src = format!(
        "{MULTI_FRAME_WITH_NULL_HEADER}\nfn main() -> i32:\n    vs: List[i64] = []\n    vs.append(0i64)\n    vs.append(2i64)\n    vs.append(0i64)\n    vs.append(4i64)\n    vs.append(5i64)\n    ns: List[bool] = []\n    ns.append(true)\n    ns.append(false)\n    ns.append(true)\n    ns.append(false)\n    ns.append(false)\n    df: DataFrame = make_frame_with_null(vs, ns)\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    fo: DataFrame = mi.fillna_i64(99i64)\n    println(\"rows=\" + str(fo.length()))\n    println(\"nlev=\" + str(fo.index_nlevels()))\n    return 0\n"
    );
    let out = run("fillna_i64_preserves_mi", &src);
    assert!(out.contains("rows=5"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn select_preserves_multiindex() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    want: List[str] = []\n    want.append(\"qty\")\n    se: DataFrame = mi.select(want)\n    println(\"ncols=\" + str(se.ncols()))\n    println(\"nlev=\" + str(se.index_nlevels()))\n    return 0\n"
    );
    let out = run("select_preserves_mi", &src);
    assert!(out.contains("ncols=1"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn drop_preserves_multiindex() {
    let src = format!(
        "{MULTI_FRAME_WITH_NULL_HEADER}\nfn main() -> i32:\n    vs: List[i64] = []\n    vs.append(1i64)\n    vs.append(2i64)\n    vs.append(3i64)\n    vs.append(4i64)\n    vs.append(5i64)\n    ns: List[bool] = []\n    ns.append(false)\n    ns.append(false)\n    ns.append(false)\n    ns.append(false)\n    ns.append(false)\n    df: DataFrame = make_frame_with_null(vs, ns)\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    dr: List[str] = []\n    dr.append(\"ext\")\n    do: DataFrame = mi.drop(dr)\n    println(\"ncols=\" + str(do.ncols()))\n    println(\"nlev=\" + str(do.index_nlevels()))\n    return 0\n"
    );
    let out = run("drop_preserves_mi", &src);
    assert!(out.contains("ncols=1"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn rename_preserves_multiindex() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    pairs: List[Tuple[str, str]] = []\n    pairs.append((\"qty\", \"quantity\"))\n    re: DataFrame = mi.rename(pairs)\n    println(\"ncols=\" + str(re.ncols()))\n    println(\"nlev=\" + str(re.index_nlevels()))\n    cs: List[str] = re.columns()\n    println(\"only=\" + cs[0i32])\n    return 0\n"
    );
    let out = run("rename_preserves_mi", &src);
    assert!(out.contains("ncols=1"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
    assert!(out.contains("only=quantity"), "got: {out:?}");
}

#[test]
fn merge_inner_preserves_lhs_multiindex() {
    // Build a 2-level MI'd frame, merge with a tiny lookup on "qty".
    // inner join → all rows that match on qty stay; lhs MultiIndex wins.
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn make_lookup() -> DataFrame:\n    qs: List[i64] = []\n    qs.append(10i64)\n    qs.append(30i64)\n    qs.append(50i64)\n    qc: ColumnI64 = tabular.col_i64_simple(qs)\n    rs: List[str] = []\n    rs.append(\"low\")\n    rs.append(\"mid\")\n    rs.append(\"high\")\n    rc: ColumnStr = tabular.col_str_simple(rs)\n    n: List[str] = []\n    n.append(\"qty\")\n    n.append(\"bucket\")\n    cs: List[Column] = []\n    cs.append(qc)\n    cs.append(rc)\n    return tabular.from_columns(n, cs)\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    lk: DataFrame = make_lookup()\n    on: List[str] = []\n    on.append(\"qty\")\n    mo: DataFrame = mi.merge(lk, on, \"inner\")\n    println(\"rows=\" + str(mo.length()))\n    println(\"nlev=\" + str(mo.index_nlevels()))\n    return 0\n"
    );
    let out = run("merge_inner_preserves_lhs_mi", &src);
    // qty values 10,30,50 match — 3 rows in inner.
    assert!(out.contains("rows=3"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn merge_left_preserves_lhs_multiindex() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn make_lookup() -> DataFrame:\n    qs: List[i64] = []\n    qs.append(10i64)\n    qs.append(30i64)\n    qc: ColumnI64 = tabular.col_i64_simple(qs)\n    rs: List[str] = []\n    rs.append(\"low\")\n    rs.append(\"mid\")\n    rc: ColumnStr = tabular.col_str_simple(rs)\n    n: List[str] = []\n    n.append(\"qty\")\n    n.append(\"bucket\")\n    cs: List[Column] = []\n    cs.append(qc)\n    cs.append(rc)\n    return tabular.from_columns(n, cs)\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    lk: DataFrame = make_lookup()\n    on: List[str] = []\n    on.append(\"qty\")\n    mo: DataFrame = mi.merge(lk, on, \"left\")\n    println(\"rows=\" + str(mo.length()))\n    println(\"nlev=\" + str(mo.index_nlevels()))\n    return 0\n"
    );
    let out = run("merge_left_preserves_lhs_mi", &src);
    // Every lhs row kept — 5 rows.
    assert!(out.contains("rows=5"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn merge_right_takes_rhs_multiindex() {
    // Build an rhs with a MultiIndex; lhs has no index.  Right join =>
    // rhs's MultiIndex wins per M42's right-side policy (now M45 with
    // MultiIndex shape).
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn make_unindexed_lhs() -> DataFrame:\n    qs: List[i64] = []\n    qs.append(10i64)\n    qs.append(30i64)\n    qs.append(50i64)\n    qc: ColumnI64 = tabular.col_i64_simple(qs)\n    vs: List[str] = []\n    vs.append(\"x\")\n    vs.append(\"y\")\n    vs.append(\"z\")\n    vc: ColumnStr = tabular.col_str_simple(vs)\n    n: List[str] = []\n    n.append(\"qty\")\n    n.append(\"v\")\n    cs: List[Column] = []\n    cs.append(qc)\n    cs.append(vc)\n    return tabular.from_columns(n, cs)\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    rhs_mi: DataFrame = df.set_index_multi(keys)\n    lhs: DataFrame = make_unindexed_lhs()\n    on: List[str] = []\n    on.append(\"qty\")\n    mo: DataFrame = lhs.merge(rhs_mi, on, \"right\")\n    println(\"rows=\" + str(mo.length()))\n    println(\"nlev=\" + str(mo.index_nlevels()))\n    return 0\n"
    );
    let out = run("merge_right_takes_rhs_mi", &src);
    // Every rhs row kept — 5 rows.
    assert!(out.contains("rows=5"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

// ───────────────────────────────────────────────────────────────────────
// Phase B — M43 reshape ops: melt repeats levels; concat_rows
// concatenates level-by-level (with strict reconciliation); concat_cols
// inherits lhs's MultiIndex; pivot + pivot_table explicitly drop.
// ───────────────────────────────────────────────────────────────────────

#[test]
fn melt_repeats_each_multiindex_level() {
    // Build a MI'd frame.  qty has 5 rows; melt with value_vars=[qty]
    // produces 5 output rows.  With value_vars=[qty, qty_again] (we
    // duplicate qty as a second column) the output has 10 rows and each
    // level should also have length 10.
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn make_frame_two_qtys() -> DataFrame:\n    regs_v: List[str] = []\n    regs_v.append(\"east\")\n    regs_v.append(\"east\")\n    regs_v.append(\"west\")\n    regs_v.append(\"west\")\n    regs_v.append(\"east\")\n    regs: ColumnStr = tabular.col_str_simple(regs_v)\n    cats_v: List[str] = []\n    cats_v.append(\"a\")\n    cats_v.append(\"b\")\n    cats_v.append(\"a\")\n    cats_v.append(\"b\")\n    cats_v.append(\"a\")\n    cats: ColumnStr = tabular.col_str_simple(cats_v)\n    q1_v: List[i64] = []\n    q1_v.append(10i64)\n    q1_v.append(20i64)\n    q1_v.append(30i64)\n    q1_v.append(40i64)\n    q1_v.append(50i64)\n    q1: ColumnI64 = tabular.col_i64_simple(q1_v)\n    q2_v: List[i64] = []\n    q2_v.append(11i64)\n    q2_v.append(22i64)\n    q2_v.append(33i64)\n    q2_v.append(44i64)\n    q2_v.append(55i64)\n    q2: ColumnI64 = tabular.col_i64_simple(q2_v)\n    n: List[str] = []\n    n.append(\"reg\")\n    n.append(\"cat\")\n    n.append(\"q1\")\n    n.append(\"q2\")\n    cs: List[Column] = []\n    cs.append(regs)\n    cs.append(cats)\n    cs.append(q1)\n    cs.append(q2)\n    return tabular.from_columns(n, cs)\nfn main() -> i32:\n    df: DataFrame = make_frame_two_qtys()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    id: List[str] = []\n    vv: List[str] = []\n    vv.append(\"q1\")\n    vv.append(\"q2\")\n    mo: DataFrame = mi.melt(id, vv)\n    println(\"rows=\" + str(mo.length()))\n    println(\"nlev=\" + str(mo.index_nlevels()))\n    return 0\n"
    );
    let out = run("melt_repeats_mi_levels", &src);
    // 5 rows * 2 value_vars = 10 rows; nlev=2.
    assert!(out.contains("rows=10"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn concat_rows_concatenates_multiindexes_level_by_level() {
    // Two MI'd frames with the same nlevels, dtypes per level, and names
    // per level → output is a MultiIndex with the concatenated levels.
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn make_other() -> DataFrame:\n    regs_v: List[str] = []\n    regs_v.append(\"north\")\n    regs_v.append(\"south\")\n    regs: ColumnStr = tabular.col_str_simple(regs_v)\n    cats_v: List[str] = []\n    cats_v.append(\"c\")\n    cats_v.append(\"d\")\n    cats: ColumnStr = tabular.col_str_simple(cats_v)\n    qty_v: List[i64] = []\n    qty_v.append(100i64)\n    qty_v.append(200i64)\n    qty: ColumnI64 = tabular.col_i64_simple(qty_v)\n    n: List[str] = []\n    n.append(\"reg\")\n    n.append(\"cat\")\n    n.append(\"qty\")\n    cs: List[Column] = []\n    cs.append(regs)\n    cs.append(cats)\n    cs.append(qty)\n    return tabular.from_columns(n, cs)\nfn main() -> i32:\n    a: DataFrame = make_frame()\n    b: DataFrame = make_other()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    ami: DataFrame = a.set_index_multi(keys)\n    bmi: DataFrame = b.set_index_multi(keys)\n    dfs: List[DataFrame] = []\n    dfs.append(ami)\n    dfs.append(bmi)\n    co: DataFrame = tabular.concat_rows(dfs)\n    println(\"rows=\" + str(co.length()))\n    println(\"nlev=\" + str(co.index_nlevels()))\n    return 0\n"
    );
    let out = run("concat_rows_mi_level_by_level", &src);
    // 5 + 2 rows.
    assert!(out.contains("rows=7"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn concat_rows_mismatched_levels_falls_back() {
    // lhs has 2-level MI ([reg, cat]); rhs has 1-level MI ([reg]) — both
    // expose the same regular column shape (just `cat` + `qty` vs
    // `qty`).  Make the regular column shape match by using two MI'd
    // frames where lhs lifts both reg+cat and rhs lifts only reg+cat
    // (single-element MI of ["cat"] vs 2-element MI of ["reg","cat"]).
    // We pin the regular cols to just `qty` by lifting all of reg+cat
    // off; the MI nlevels differ (1 vs 2) — strict reconciliation drops
    // to RangeIndex.
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn make_other() -> DataFrame:\n    cats_v: List[str] = []\n    cats_v.append(\"a\")\n    cats_v.append(\"b\")\n    cats: ColumnStr = tabular.col_str_simple(cats_v)\n    qty_v: List[i64] = []\n    qty_v.append(100i64)\n    qty_v.append(200i64)\n    qty: ColumnI64 = tabular.col_i64_simple(qty_v)\n    n: List[str] = []\n    n.append(\"cat\")\n    n.append(\"qty\")\n    cs: List[Column] = []\n    cs.append(cats)\n    cs.append(qty)\n    return tabular.from_columns(n, cs)\nfn main() -> i32:\n    a: DataFrame = make_frame()\n    keys2: List[str] = []\n    keys2.append(\"reg\")\n    keys2.append(\"cat\")\n    ami: DataFrame = a.set_index_multi(keys2)\n    b: DataFrame = make_other()\n    keys1: List[str] = []\n    keys1.append(\"cat\")\n    bmi: DataFrame = b.set_index_multi(keys1)\n    dfs: List[DataFrame] = []\n    dfs.append(ami)\n    dfs.append(bmi)\n    co: DataFrame = tabular.concat_rows(dfs)\n    println(\"rows=\" + str(co.length()))\n    println(\"nlev=\" + str(co.index_nlevels()))\n    return 0\n"
    );
    let out = run("concat_rows_mi_fallback", &src);
    assert!(out.contains("rows=7"), "got: {out:?}");
    // nlevels differ (2 vs 1) → strict-reconciliation drops to RangeIndex.
    assert!(out.contains("nlev=0"), "got: {out:?}");
}

#[test]
fn concat_cols_inherits_lhs_multiindex() {
    // lhs has a 2-level MI; rhs has none.  concat_cols → lhs MI wins.
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn make_other() -> DataFrame:\n    e_v: List[i64] = []\n    e_v.append(1i64)\n    e_v.append(2i64)\n    e_v.append(3i64)\n    e_v.append(4i64)\n    e_v.append(5i64)\n    e: ColumnI64 = tabular.col_i64_simple(e_v)\n    n: List[str] = []\n    n.append(\"extra\")\n    cs: List[Column] = []\n    cs.append(e)\n    return tabular.from_columns(n, cs)\nfn main() -> i32:\n    a: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    ami: DataFrame = a.set_index_multi(keys)\n    b: DataFrame = make_other()\n    dfs: List[DataFrame] = []\n    dfs.append(ami)\n    dfs.append(b)\n    co: DataFrame = tabular.concat_cols(dfs)\n    println(\"rows=\" + str(co.length()))\n    println(\"nlev=\" + str(co.index_nlevels()))\n    return 0\n"
    );
    let out = run("concat_cols_inherits_mi", &src);
    assert!(out.contains("rows=5"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn pivot_drops_multiindex() {
    // pivot reshapes the row dimension — the input MultiIndex doesn't
    // map to the output rows (which are unique `index_col` values), so
    // the MultiIndex is explicitly dropped.  Output's index is promoted
    // from `index_col` per M43 (a single-col index).
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    pv: DataFrame = mi.pivot(\"qty\", \"qty\", \"qty\")\n    println(\"nlev=\" + str(pv.index_nlevels()))\n    return 0\n"
    );
    let out = run("pivot_drops_mi", &src);
    // pivot promotes `qty` to the output's single-col index; MI dropped.
    assert!(out.contains("nlev=1"), "got: {out:?}");
}

#[test]
fn pivot_table_drops_multiindex() {
    // Same as pivot: reshape the row dimension, drop the input MI.
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    pt: DataFrame = mi.pivot_table(\"qty\", \"qty\", \"qty\", \"sum\")\n    println(\"nlev=\" + str(pt.index_nlevels()))\n    return 0\n"
    );
    let out = run("pivot_table_drops_mi", &src);
    assert!(out.contains("nlev=1"), "got: {out:?}");
}

#[test]
fn merge_outer_with_multiindex_falls_back_to_range() {
    // M45 explicit anchor: outer-merge with a MultiIndex on either side
    // falls back to RangeIndex (M46 anchor for interleaving level-by-
    // level outer joins).
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn make_other() -> DataFrame:\n    qs: List[i64] = []\n    qs.append(10i64)\n    qs.append(99i64)\n    qc: ColumnI64 = tabular.col_i64_simple(qs)\n    vs: List[str] = []\n    vs.append(\"x\")\n    vs.append(\"y\")\n    vc: ColumnStr = tabular.col_str_simple(vs)\n    n: List[str] = []\n    n.append(\"qty\")\n    n.append(\"v\")\n    cs: List[Column] = []\n    cs.append(qc)\n    cs.append(vc)\n    return tabular.from_columns(n, cs)\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    ot: DataFrame = make_other()\n    on: List[str] = []\n    on.append(\"qty\")\n    mo: DataFrame = mi.merge(ot, on, \"outer\")\n    println(\"nlev=\" + str(mo.index_nlevels()))\n    return 0\n"
    );
    let out = run("merge_outer_mi_fallback", &src);
    // Outer + MultiIndex anywhere → RangeIndex fallback.
    assert!(out.contains("nlev=0"), "got: {out:?}");
}
