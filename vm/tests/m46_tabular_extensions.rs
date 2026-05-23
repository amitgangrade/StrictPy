//! M46 in-process regression tests for the `tabular` extensions:
//!
//!  Phase A: stack / unstack — pandas's MultiIndex bread-and-butter.
//!  Phase B: df.loc_range_* per dtype — extends M41's label lookup.
//!  Phase C: outer-merge MultiIndex fallback (dtype mismatch) +
//!           set_index_list (1-element list unification) +
//!           pivot_table_aggfunc_list + pivot_table_margins.
//!  Phase D: time-series ops MultiIndex handling (resample drops MI;
//!           asof_merge preserves lhs MI via M45 routing).
//!
//! Variable prefix `m46_` in any test-local helpers.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m46_{test_name}.spyc"));
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
    let p = compile_snippet(name, src);
    let (_code, out) = run_file_capture(&p).expect("run");
    // We don't care about the exit code shape; just return what was printed
    // so the test can check for the expected exception text.
    out
}

// ── Common 5-row builder used across multiple tests ─────────────────────

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

// ════════════════════════════════════════════════════════════════════════
// Phase A — stack / unstack
// ════════════════════════════════════════════════════════════════════════

#[test]
fn stack_no_index_to_singlecol_index() {
    // 3 rows × 2 ColumnI64 columns (a, b) -> 6 rows × 1 value col + col-name index.
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn main() -> i32:
    av: List[i64] = []
    av.append(1i64)
    av.append(2i64)
    av.append(3i64)
    a: ColumnI64 = tabular.col_i64_simple(av)
    bv: List[i64] = []
    bv.append(10i64)
    bv.append(20i64)
    bv.append(30i64)
    b: ColumnI64 = tabular.col_i64_simple(bv)
    n: List[str] = []
    n.append(\"a\")
    n.append(\"b\")
    cs: List[Column] = []
    cs.append(a)
    cs.append(b)
    df: DataFrame = tabular.from_columns(n, cs)
    st: DataFrame = df.stack()
    println(\"rows=\" + str(st.length()))
    println(\"nlev=\" + str(st.index_nlevels()))
    return 0
";
    let out = run("stack_no_index", src);
    assert!(out.contains("rows=6"), "got: {out:?}");
    assert!(out.contains("nlev=1"), "got: {out:?}");
}

#[test]
fn stack_singlecol_index_to_multiindex() {
    // 5 rows × 1 reg(str) + 1 qty(i64) - but stack requires all reg cols
    // same dtype.  Use a 5-row x 2 ColumnI64 frame with a set_index on a 3rd col.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    av: List[i64] = []
    av.append(1i64)
    av.append(2i64)
    a: ColumnI64 = tabular.col_i64_simple(av)
    bv: List[i64] = []
    bv.append(10i64)
    bv.append(20i64)
    b: ColumnI64 = tabular.col_i64_simple(bv)
    rv: List[str] = []
    rv.append(\"x\")
    rv.append(\"y\")
    r: ColumnStr = tabular.col_str_simple(rv)
    n: List[str] = []
    n.append(\"a\")
    n.append(\"b\")
    n.append(\"reg\")
    cs: List[Column] = []
    cs.append(a)
    cs.append(b)
    cs.append(r)
    df: DataFrame = tabular.from_columns(n, cs)
    si: DataFrame = df.set_index(\"reg\")
    st: DataFrame = si.stack()
    println(\"rows=\" + str(st.length()))
    println(\"nlev=\" + str(st.index_nlevels()))
    return 0
";
    let out = run("stack_singlecol_index", src);
    assert!(out.contains("rows=4"), "got: {out:?}");
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn stack_dtype_mismatch_raises() {
    // The full 5-row frame has reg (Str), cat (Str), qty (I64) regular
    // columns.  Stack on this frame (no set_index) must raise because
    // the regular columns have mixed dtypes.
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    try:\n        st: DataFrame = df.stack()\n        println(\"no-raise\")\n    except ValueError as e:\n        println(\"raised\")\n    return 0\n"
    );
    let out = run_expect_err("stack_dtype_mismatch", &src);
    // Either "raised" (caught) or the runtime exception text would
    // surface — both indicate a successful raise.  We accept either.
    assert!(
        out.contains("raised") || out.contains("share a dtype"),
        "got: {out:?}"
    );
}

#[test]
fn stack_multiindex_input_adds_level() {
    // 2-level MultiIndex input (reg, cat) + 1 ColumnI64 col (qty) ->
    // stack should produce a 3-level MultiIndex (reg, cat, value-name)
    // and 5 rows × 1 col = 5 rows in the output.
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    st: DataFrame = mi.stack()\n    println(\"rows=\" + str(st.length()))\n    println(\"nlev=\" + str(st.index_nlevels()))\n    return 0\n"
    );
    let out = run("stack_multiindex_input", &src);
    assert!(out.contains("rows=5"), "got: {out:?}");
    assert!(out.contains("nlev=3"), "got: {out:?}");
}

#[test]
fn unstack_multiindex_to_singlecol() {
    // Build a 2-level MultiIndex df with stack first, then unstack
    // it.  Result should have nlev=1 (single-col index).
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    av: List[i64] = []
    av.append(1i64)
    av.append(2i64)
    a: ColumnI64 = tabular.col_i64_simple(av)
    bv: List[i64] = []
    bv.append(10i64)
    bv.append(20i64)
    b: ColumnI64 = tabular.col_i64_simple(bv)
    rv: List[str] = []
    rv.append(\"x\")
    rv.append(\"y\")
    r: ColumnStr = tabular.col_str_simple(rv)
    n: List[str] = []
    n.append(\"a\")
    n.append(\"b\")
    n.append(\"reg\")
    cs: List[Column] = []
    cs.append(a)
    cs.append(b)
    cs.append(r)
    df: DataFrame = tabular.from_columns(n, cs)
    si: DataFrame = df.set_index(\"reg\")
    st: DataFrame = si.stack()
    us: DataFrame = st.unstack()
    println(\"rows=\" + str(us.length()))
    println(\"nlev=\" + str(us.index_nlevels()))
    return 0
";
    let out = run("unstack_multiindex_to_singlecol", src);
    assert!(out.contains("rows=2"), "got: {out:?}");
    assert!(out.contains("nlev=1"), "got: {out:?}");
}

#[test]
fn unstack_3level_multiindex_drops_one() {
    // 3-level MI from stacking the multiindex frame.  Unstack drops the
    // innermost level -> 2-level MultiIndex.
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    st: DataFrame = mi.stack()\n    us: DataFrame = st.unstack()\n    println(\"nlev=\" + str(us.index_nlevels()))\n    return 0\n"
    );
    let out = run("unstack_3level_mi_drops_one", &src);
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn unstack_requires_multiindex() {
    // Frame with no index -> unstack must raise.
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    try:\n        us: DataFrame = df.unstack()\n        println(\"no-raise\")\n    except ValueError as e:\n        println(\"raised\")\n    return 0\n"
    );
    let out = run_expect_err("unstack_no_index_raises", &src);
    assert!(
        out.contains("raised") || out.contains("must have a MultiIndex"),
        "got: {out:?}"
    );
}

#[test]
fn stack_unstack_roundtrip() {
    // Build a wide frame, stack it, unstack it, check nrows match.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn main() -> i32:
    av: List[i64] = []
    av.append(1i64)
    av.append(2i64)
    av.append(3i64)
    a: ColumnI64 = tabular.col_i64_simple(av)
    bv: List[i64] = []
    bv.append(10i64)
    bv.append(20i64)
    bv.append(30i64)
    b: ColumnI64 = tabular.col_i64_simple(bv)
    rv: List[str] = []
    rv.append(\"x\")
    rv.append(\"y\")
    rv.append(\"z\")
    r: ColumnStr = tabular.col_str_simple(rv)
    n: List[str] = []
    n.append(\"a\")
    n.append(\"b\")
    n.append(\"reg\")
    cs: List[Column] = []
    cs.append(a)
    cs.append(b)
    cs.append(r)
    df: DataFrame = tabular.from_columns(n, cs)
    si: DataFrame = df.set_index(\"reg\")
    st: DataFrame = si.stack()
    us: DataFrame = st.unstack()
    println(\"orig_rows=\" + str(si.length()))
    println(\"roundtrip_rows=\" + str(us.length()))
    return 0
";
    let out = run("stack_unstack_roundtrip", src);
    assert!(out.contains("orig_rows=3"), "got: {out:?}");
    assert!(out.contains("roundtrip_rows=3"), "got: {out:?}");
}

// ════════════════════════════════════════════════════════════════════════
// Phase B — df.loc_range_* per dtype
// ════════════════════════════════════════════════════════════════════════

#[test]
fn loc_range_i64_keeps_inclusive_both_ends() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn make() -> DataFrame:
    iv: List[i64] = []
    iv.append(1i64)
    iv.append(3i64)
    iv.append(5i64)
    iv.append(7i64)
    iv.append(9i64)
    idx: ColumnI64 = tabular.col_i64_simple(iv)
    qv: List[i64] = []
    qv.append(10i64)
    qv.append(20i64)
    qv.append(30i64)
    qv.append(40i64)
    qv.append(50i64)
    q: ColumnI64 = tabular.col_i64_simple(qv)
    n: List[str] = []
    n.append(\"id\")
    n.append(\"qty\")
    cs: List[Column] = []
    cs.append(idx)
    cs.append(q)
    return tabular.from_columns(n, cs)
fn main() -> i32:
    df: DataFrame = make()
    si: DataFrame = df.set_index(\"id\")
    lr: DataFrame = si.loc_range_i64(3i64, 7i64)
    println(\"rows=\" + str(lr.length()))
    println(\"has=\" + str(lr.has_index()))
    return 0
";
    let out = run("loc_range_i64", src);
    assert!(out.contains("rows=3"), "got: {out:?}");
    assert!(out.contains("has=true"), "got: {out:?}");
}

#[test]
fn loc_range_f64_keeps_inclusive() {
    let src = "\
from tabular import Column, ColumnF64, ColumnI64, DataFrame
import tabular
fn make() -> DataFrame:
    iv: List[f64] = []
    iv.append(1.5)
    iv.append(2.5)
    iv.append(3.5)
    idx: ColumnF64 = tabular.col_f64_simple(iv)
    qv: List[i64] = []
    qv.append(10i64)
    qv.append(20i64)
    qv.append(30i64)
    q: ColumnI64 = tabular.col_i64_simple(qv)
    n: List[str] = []
    n.append(\"id\")
    n.append(\"qty\")
    cs: List[Column] = []
    cs.append(idx)
    cs.append(q)
    return tabular.from_columns(n, cs)
fn main() -> i32:
    df: DataFrame = make()
    si: DataFrame = df.set_index(\"id\")
    lr: DataFrame = si.loc_range_f64(2.0, 3.0)
    println(\"rows=\" + str(lr.length()))
    return 0
";
    let out = run("loc_range_f64", src);
    assert!(out.contains("rows=1"), "got: {out:?}");
}

#[test]
fn loc_range_str_lex_inclusive() {
    let src = "\
from tabular import Column, ColumnStr, ColumnI64, DataFrame
import tabular
fn make() -> DataFrame:
    iv: List[str] = []
    iv.append(\"alpha\")
    iv.append(\"beta\")
    iv.append(\"gamma\")
    iv.append(\"delta\")
    idx: ColumnStr = tabular.col_str_simple(iv)
    qv: List[i64] = []
    qv.append(1i64)
    qv.append(2i64)
    qv.append(3i64)
    qv.append(4i64)
    q: ColumnI64 = tabular.col_i64_simple(qv)
    n: List[str] = []
    n.append(\"id\")
    n.append(\"qty\")
    cs: List[Column] = []
    cs.append(idx)
    cs.append(q)
    return tabular.from_columns(n, cs)
fn main() -> i32:
    df: DataFrame = make()
    si: DataFrame = df.set_index(\"id\")
    lr: DataFrame = si.loc_range_str(\"alpha\", \"delta\")
    println(\"rows=\" + str(lr.length()))
    return 0
";
    let out = run("loc_range_str", src);
    // alpha, beta, delta (not gamma — gamma > delta lex).
    assert!(out.contains("rows=3"), "got: {out:?}");
}

#[test]
fn loc_range_bool_inclusive() {
    let src = "\
from tabular import Column, ColumnBool, ColumnI64, DataFrame
import tabular
fn make() -> DataFrame:
    iv: List[bool] = []
    iv.append(false)
    iv.append(true)
    iv.append(false)
    iv.append(true)
    idx: ColumnBool = tabular.col_bool_simple(iv)
    qv: List[i64] = []
    qv.append(1i64)
    qv.append(2i64)
    qv.append(3i64)
    qv.append(4i64)
    q: ColumnI64 = tabular.col_i64_simple(qv)
    n: List[str] = []
    n.append(\"id\")
    n.append(\"qty\")
    cs: List[Column] = []
    cs.append(idx)
    cs.append(q)
    return tabular.from_columns(n, cs)
fn main() -> i32:
    df: DataFrame = make()
    si: DataFrame = df.set_index(\"id\")
    lr: DataFrame = si.loc_range_bool(true, true)
    println(\"rows=\" + str(lr.length()))
    return 0
";
    let out = run("loc_range_bool", src);
    assert!(out.contains("rows=2"), "got: {out:?}");
}

#[test]
fn loc_range_datetime_inclusive() {
    let src = "\
from tabular import Column, ColumnDateTime, ColumnI64, DataFrame
import tabular
fn make() -> DataFrame:
    iv: List[i64] = []
    iv.append(100i64)
    iv.append(200i64)
    iv.append(300i64)
    iv.append(400i64)
    nsv: List[bool] = []
    nsv.append(false)
    nsv.append(false)
    nsv.append(false)
    nsv.append(false)
    idx: ColumnDateTime = tabular.col_datetime(iv, nsv)
    qv: List[i64] = []
    qv.append(1i64)
    qv.append(2i64)
    qv.append(3i64)
    qv.append(4i64)
    q: ColumnI64 = tabular.col_i64_simple(qv)
    n: List[str] = []
    n.append(\"id\")
    n.append(\"qty\")
    cs: List[Column] = []
    cs.append(idx)
    cs.append(q)
    return tabular.from_columns(n, cs)
fn main() -> i32:
    df: DataFrame = make()
    si: DataFrame = df.set_index(\"id\")
    lr: DataFrame = si.loc_range_datetime(150i64, 350i64)
    println(\"rows=\" + str(lr.length()))
    return 0
";
    let out = run("loc_range_datetime", src);
    assert!(out.contains("rows=2"), "got: {out:?}");
}

#[test]
fn loc_range_empty_range_returns_empty_frame() {
    let src = "\
from tabular import Column, ColumnI64, DataFrame
import tabular
fn make() -> DataFrame:
    iv: List[i64] = []
    iv.append(1i64)
    iv.append(2i64)
    iv.append(3i64)
    idx: ColumnI64 = tabular.col_i64_simple(iv)
    qv: List[i64] = []
    qv.append(10i64)
    qv.append(20i64)
    qv.append(30i64)
    q: ColumnI64 = tabular.col_i64_simple(qv)
    n: List[str] = []
    n.append(\"id\")
    n.append(\"qty\")
    cs: List[Column] = []
    cs.append(idx)
    cs.append(q)
    return tabular.from_columns(n, cs)
fn main() -> i32:
    df: DataFrame = make()
    si: DataFrame = df.set_index(\"id\")
    lr: DataFrame = si.loc_range_i64(100i64, 200i64)
    println(\"rows=\" + str(lr.length()))
    return 0
";
    let out = run("loc_range_empty", src);
    assert!(out.contains("rows=0"), "got: {out:?}");
}

#[test]
fn loc_range_raises_on_multiindex() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    mi: DataFrame = df.set_index_multi(keys)\n    try:\n        lr: DataFrame = mi.loc_range_i64(0i64, 99i64)\n        println(\"no-raise\")\n    except ValueError as e:\n        println(\"raised\")\n    return 0\n"
    );
    let out = run_expect_err("loc_range_raises_mi", &src);
    assert!(
        out.contains("raised") || out.contains("MultiIndex not supported"),
        "got: {out:?}"
    );
}

// ════════════════════════════════════════════════════════════════════════
// Phase C — outer-merge MultiIndex fallback + set_index_list + pivot_table
// ════════════════════════════════════════════════════════════════════════

#[test]
fn outer_merge_dtype_mismatch_produces_2level_multiindex() {
    // lhs has ColumnI64 index "lid"; rhs has ColumnStr index "rid".
    // Outer-merge on "tid" with mismatched index dtypes -> M46
    // 2-level NaN-padded MultiIndex.
    let src = "\
from tabular import Column, ColumnI64, ColumnStr, DataFrame
import tabular
fn make_lhs() -> DataFrame:
    lid: List[i64] = []
    lid.append(10i64)
    lid.append(20i64)
    lic: ColumnI64 = tabular.col_i64_simple(lid)
    tid: List[i64] = []
    tid.append(1i64)
    tid.append(2i64)
    tc: ColumnI64 = tabular.col_i64_simple(tid)
    n: List[str] = []
    n.append(\"lid\")
    n.append(\"tid\")
    cs: List[Column] = []
    cs.append(lic)
    cs.append(tc)
    df: DataFrame = tabular.from_columns(n, cs)
    return df.set_index(\"lid\")
fn make_rhs() -> DataFrame:
    rid: List[str] = []
    rid.append(\"R1\")
    rid.append(\"R2\")
    ric: ColumnStr = tabular.col_str_simple(rid)
    tid: List[i64] = []
    tid.append(2i64)
    tid.append(3i64)
    tc: ColumnI64 = tabular.col_i64_simple(tid)
    n: List[str] = []
    n.append(\"rid\")
    n.append(\"tid\")
    cs: List[Column] = []
    cs.append(ric)
    cs.append(tc)
    df: DataFrame = tabular.from_columns(n, cs)
    return df.set_index(\"rid\")
fn main() -> i32:
    lhs: DataFrame = make_lhs()
    rhs: DataFrame = make_rhs()
    on: List[str] = []
    on.append(\"tid\")
    mo: DataFrame = lhs.merge(rhs, on, \"outer\")
    println(\"nlev=\" + str(mo.index_nlevels()))
    println(\"rows=\" + str(mo.length()))
    return 0
";
    let out = run("outer_merge_dtype_mismatch_mi", src);
    assert!(out.contains("nlev=2"), "got: {out:?}");
    // 1 left-only (tid=1) + 1 matched (tid=2) + 1 right-only (tid=3) = 3.
    assert!(out.contains("rows=3"), "got: {out:?}");
}

#[test]
fn set_index_list_1_element_acts_as_single_col() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    si: DataFrame = df.set_index_list(keys)\n    println(\"nlev=\" + str(si.index_nlevels()))\n    println(\"has=\" + str(si.has_index()))\n    return 0\n"
    );
    let out = run("set_index_list_one", &src);
    assert!(out.contains("nlev=1"), "got: {out:?}");
    assert!(out.contains("has=true"), "got: {out:?}");
}

#[test]
fn set_index_list_multi_element_acts_as_multiindex() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    keys.append(\"reg\")\n    keys.append(\"cat\")\n    si: DataFrame = df.set_index_list(keys)\n    println(\"nlev=\" + str(si.index_nlevels()))\n    return 0\n"
    );
    let out = run("set_index_list_multi", &src);
    assert!(out.contains("nlev=2"), "got: {out:?}");
}

#[test]
fn set_index_list_empty_raises() {
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    keys: List[str] = []\n    try:\n        si: DataFrame = df.set_index_list(keys)\n        println(\"no-raise\")\n    except ValueError as e:\n        println(\"raised\")\n    return 0\n"
    );
    let out = run_expect_err("set_index_list_empty", &src);
    assert!(
        out.contains("raised") || out.contains("non-empty"),
        "got: {out:?}"
    );
}

#[test]
fn pivot_table_aggfunc_list_emits_twice_the_value_columns() {
    // pivot_table_aggfunc_list with 2 aggfuncs on the 5-row frame.
    // Vanilla pivot_table over (reg, cat, qty, "sum") yields 2 col_keys
    // (a, b).  With ["sum", "mean"] we expect 4 output columns:
    // a_sum, b_sum, a_mean, b_mean.
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    aggs: List[str] = []\n    aggs.append(\"sum\")\n    aggs.append(\"mean\")\n    pt: DataFrame = df.pivot_table_aggfunc_list(\"reg\", \"cat\", \"qty\", aggs)\n    println(\"cols=\" + str(pt.ncols()))\n    println(\"rows=\" + str(pt.length()))\n    return 0\n"
    );
    let out = run("pivot_table_aggfunc_list", &src);
    assert!(out.contains("cols=4"), "got: {out:?}");
    assert!(out.contains("rows=2"), "got: {out:?}");
}

#[test]
fn pivot_table_margins_adds_all_row_and_column() {
    // Body has 2 rows × 2 cols.  Margins -> 3 rows × 3 cols.
    let src = format!(
        "{MULTI_FRAME_HEADER}\nfn main() -> i32:\n    df: DataFrame = make_frame()\n    pt: DataFrame = df.pivot_table_margins(\"reg\", \"cat\", \"qty\", \"sum\")\n    println(\"cols=\" + str(pt.ncols()))\n    println(\"rows=\" + str(pt.length()))\n    return 0\n"
    );
    let out = run("pivot_table_margins", &src);
    assert!(out.contains("cols=3"), "got: {out:?}");
    assert!(out.contains("rows=3"), "got: {out:?}");
}
