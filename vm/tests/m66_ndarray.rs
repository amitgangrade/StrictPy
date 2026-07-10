//! M66 (wave-3 Lane B) in-process regression tests for the `ndarray`
//! stdlib module.
//!
//! `NDArray` is a handle-backed native class (§9.52): f64 only, 1-D and
//! 2-D, copies-not-views, with the buffer living in a `SharedVm.ndarrays`
//! slot table.  These tests drive the full public surface — the nine
//! module constructors and all 48 methods — through compiled `.spy`
//! programs, asserting on captured stdout.
//!
//! Precision policy (from WAVE3_PLAN.md): exact equality for structural
//! ops (shape/reshape/transpose/get/masks/to_list/show), 1e-12 tolerance
//! for float math.  Float comparisons run inside the `.spy` program via
//! the `fclose` helper so the assertion is on a boolean, not on a
//! platform-dependent float-to-string rendering.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m66_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

/// Compile + run, asserting a clean exit; returns captured stdout.
fn run_ok(test_name: &str, src: &str) -> String {
    let p = compile_snippet(test_name, src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "{test_name}: nonzero exit; stdout: {out}");
    out
}

// A `fclose` helper prelude injected into programs that do float math.
const PRELUDE: &str = "\
fn fclose(a: f64, b: f64) -> bool:
    d: f64 = a - b
    if d < 0.0:
        d = -d
    return d < 0.000000000001
";

// ── Constructors ──────────────────────────────────────────────────────

#[test]
fn constructors_array_and_shape() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.array([1.0, 2.0, 3.0])
    println(\"shape=\" + str(a.shape()[0]))
    println(\"size=\" + str(a.size()))
    println(\"ndim=\" + str(a.ndim()))
    println(\"show=\" + a.show())
    return 0
";
    let out = run_ok("ctor_array", src);
    assert!(out.contains("shape=3"), "got: {out:?}");
    assert!(out.contains("size=3"), "got: {out:?}");
    assert!(out.contains("ndim=1"), "got: {out:?}");
    assert!(out.contains("show=[1.0, 2.0, 3.0]"), "got: {out:?}");
}

#[test]
fn constructors_array2_and_ndim() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.array2([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]])
    println(\"rows=\" + str(a.shape()[0]))
    println(\"cols=\" + str(a.shape()[1]))
    println(\"ndim=\" + str(a.ndim()))
    println(\"size=\" + str(a.size()))
    return 0
";
    let out = run_ok("ctor_array2", src);
    assert!(out.contains("rows=2"), "got: {out:?}");
    assert!(out.contains("cols=3"), "got: {out:?}");
    assert!(out.contains("ndim=2"), "got: {out:?}");
    assert!(out.contains("size=6"), "got: {out:?}");
}

#[test]
fn constructors_array2_ragged_raises_value_error() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    try:
        a: NDArray = ndarray.array2([[1.0, 2.0], [3.0]])
        println(\"UNREACHED\")
    except ValueError:
        println(\"caught\")
    return 0
";
    let out = run_ok("ctor_array2_ragged", src);
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

#[test]
fn constructors_zeros_ones_full() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    z: NDArray = ndarray.zeros([2i64, 3i64])
    o: NDArray = ndarray.ones([4i64])
    f: NDArray = ndarray.full([2i64, 2i64], 7.5)
    println(\"z=\" + z.show())
    println(\"o=\" + o.show())
    println(\"f=\" + f.show())
    println(\"zsize=\" + str(z.size()))
    return 0
";
    let out = run_ok("ctor_zof", src);
    assert!(out.contains("z=[[0.0, 0.0, 0.0]\n [0.0, 0.0, 0.0]]"), "got: {out:?}");
    assert!(out.contains("o=[1.0, 1.0, 1.0, 1.0]"), "got: {out:?}");
    assert!(out.contains("f=[[7.5, 7.5]\n [7.5, 7.5]]"), "got: {out:?}");
    assert!(out.contains("zsize=6"), "got: {out:?}");
}

#[test]
fn constructors_zeros_3d_raises_value_error() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    try:
        a: NDArray = ndarray.zeros([2i64, 2i64, 2i64])
        println(\"UNREACHED\")
    except ValueError:
        println(\"caught\")
    return 0
";
    let out = run_ok("ctor_zeros_3d", src);
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

#[test]
fn constructors_arange_linspace_eye() {
    let src = format!(
        "{PRELUDE}\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    r: NDArray = ndarray.arange(0.0, 5.0, 1.0)
    println(\"r=\" + r.show())
    ls: NDArray = ndarray.linspace(0.0, 1.0, 5i64)
    lst: List[f64] = ls.to_list()
    if fclose(lst[0], 0.0):
        println(\"ls0_ok\")
    if fclose(lst[2], 0.5):
        println(\"ls2_ok\")
    if fclose(lst[4], 1.0):
        println(\"ls4_ok\")
    e: NDArray = ndarray.eye(3i64)
    println(\"e=\" + e.show())
    return 0
"
    );
    let out = run_ok("ctor_arange", &src);
    assert!(out.contains("r=[0.0, 1.0, 2.0, 3.0, 4.0]"), "got: {out:?}");
    assert!(out.contains("ls0_ok"), "got: {out:?}");
    assert!(out.contains("ls2_ok"), "got: {out:?}");
    assert!(out.contains("ls4_ok"), "got: {out:?}");
    assert!(
        out.contains("e=[[1.0, 0.0, 0.0]\n [0.0, 1.0, 0.0]\n [0.0, 0.0, 1.0]]"),
        "got: {out:?}"
    );
}

// ── Structural: reshape / transpose / flatten ─────────────────────────

#[test]
fn reshape_and_flatten() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.arange(0.0, 6.0, 1.0)
    b: NDArray = a.reshape([2i64, 3i64])
    println(\"b=\" + b.show())
    fl: NDArray = b.flatten()
    println(\"fl=\" + fl.show())
    println(\"flndim=\" + str(fl.ndim()))
    return 0
";
    let out = run_ok("reshape_flatten", src);
    assert!(out.contains("b=[[0.0, 1.0, 2.0]\n [3.0, 4.0, 5.0]]"), "got: {out:?}");
    assert!(out.contains("fl=[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]"), "got: {out:?}");
    assert!(out.contains("flndim=1"), "got: {out:?}");
}

#[test]
fn reshape_size_mismatch_raises_value_error() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.arange(0.0, 6.0, 1.0)
    try:
        b: NDArray = a.reshape([4i64])
        println(\"UNREACHED\")
    except ValueError:
        println(\"caught\")
    return 0
";
    let out = run_ok("reshape_mismatch", src);
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

#[test]
fn transpose_2d_and_1d_identity() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.array2([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]])
    t: NDArray = a.transpose()
    println(\"trows=\" + str(t.shape()[0]))
    println(\"tcols=\" + str(t.shape()[1]))
    println(\"t=\" + t.show())
    v: NDArray = ndarray.array([1.0, 2.0, 3.0])
    tv: NDArray = v.transpose()
    println(\"tv=\" + tv.show())
    return 0
";
    let out = run_ok("transpose", src);
    assert!(out.contains("trows=3"), "got: {out:?}");
    assert!(out.contains("tcols=2"), "got: {out:?}");
    assert!(out.contains("t=[[1.0, 4.0]\n [2.0, 5.0]\n [3.0, 6.0]]"), "got: {out:?}");
    assert!(out.contains("tv=[1.0, 2.0, 3.0]"), "got: {out:?}");
}

// ── Broadcasting: four success cases + the mismatch ValueError ────────

#[test]
fn broadcast_equal_shapes() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.array2([[1.0, 2.0], [3.0, 4.0]])
    b: NDArray = ndarray.array2([[10.0, 20.0], [30.0, 40.0]])
    println(\"add=\" + a.add(b).show())
    println(\"sub=\" + b.sub(a).show())
    println(\"mul=\" + a.mul(b).show())
    return 0
";
    let out = run_ok("bc_equal", src);
    assert!(out.contains("add=[[11.0, 22.0]\n [33.0, 44.0]]"), "got: {out:?}");
    assert!(out.contains("sub=[[9.0, 18.0]\n [27.0, 36.0]]"), "got: {out:?}");
    assert!(out.contains("mul=[[10.0, 40.0]\n [90.0, 160.0]]"), "got: {out:?}");
}

#[test]
fn broadcast_row_1d_against_2d_both_orders() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    m: NDArray = ndarray.array2([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]])
    v: NDArray = ndarray.array([10.0, 20.0, 30.0])
    println(\"mv=\" + m.add(v).show())
    println(\"vm=\" + v.add(m).show())
    return 0
";
    let out = run_ok("bc_row", src);
    assert!(out.contains("mv=[[11.0, 22.0, 33.0]\n [14.0, 25.0, 36.0]]"), "got: {out:?}");
    assert!(out.contains("vm=[[11.0, 22.0, 33.0]\n [14.0, 25.0, 36.0]]"), "got: {out:?}");
}

#[test]
fn broadcast_column_rows1_against_2d() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    m: NDArray = ndarray.array2([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]])
    c: NDArray = ndarray.array2([[10.0], [20.0]])
    println(\"mc=\" + m.add(c).show())
    return 0
";
    let out = run_ok("bc_col", src);
    assert!(out.contains("mc=[[11.0, 12.0, 13.0]\n [24.0, 25.0, 26.0]]"), "got: {out:?}");
}

#[test]
fn broadcast_mismatch_raises_value_error() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.array([1.0, 2.0, 3.0])
    b: NDArray = ndarray.array([1.0, 2.0])
    try:
        c: NDArray = a.add(b)
        println(\"UNREACHED\")
    except ValueError:
        println(\"caught\")
    return 0
";
    let out = run_ok("bc_mismatch", src);
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

#[test]
fn division_by_zero_is_ieee754() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.array([1.0, 0.0])
    b: NDArray = ndarray.array([0.0, 0.0])
    println(\"d=\" + a.div(b).show())
    return 0
";
    let out = run_ok("div_zero", src);
    // 1/0 -> inf, 0/0 -> nan (no raise).
    assert!(out.contains("d=[inf, nan]"), "got: {out:?}");
}

// ── Scalar ops + unary math ───────────────────────────────────────────

#[test]
fn scalar_ops_and_unary_math() {
    let src = format!(
        "{PRELUDE}\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.array([1.0, 4.0, 9.0])
    println(\"adds=\" + a.adds(1.0).show())
    println(\"muls=\" + a.muls(2.0).show())
    println(\"neg=\" + a.neg().show())
    println(\"sqrt=\" + a.sqrt().show())
    b: NDArray = ndarray.array([-2.0, 3.0])
    println(\"abs=\" + b.abs().show())
    p: NDArray = ndarray.array([2.0, 3.0])
    println(\"powf=\" + p.powf(2.0).show())
    e: NDArray = ndarray.array([0.0])
    el: List[f64] = e.exp().to_list()
    if fclose(el[0], 1.0):
        println(\"exp_ok\")
    lg: List[f64] = ndarray.array([1.0]).log().to_list()
    if fclose(lg[0], 0.0):
        println(\"log_ok\")
    return 0
"
    );
    let out = run_ok("scalar_unary", &src);
    assert!(out.contains("adds=[2.0, 5.0, 10.0]"), "got: {out:?}");
    assert!(out.contains("muls=[2.0, 8.0, 18.0]"), "got: {out:?}");
    assert!(out.contains("neg=[-1.0, -4.0, -9.0]"), "got: {out:?}");
    assert!(out.contains("sqrt=[1.0, 2.0, 3.0]"), "got: {out:?}");
    assert!(out.contains("abs=[2.0, 3.0]"), "got: {out:?}");
    assert!(out.contains("powf=[4.0, 9.0]"), "got: {out:?}");
    assert!(out.contains("exp_ok"), "got: {out:?}");
    assert!(out.contains("log_ok"), "got: {out:?}");
}

// ── Reductions (whole-array + per-axis) ───────────────────────────────

#[test]
fn reductions_whole_array() {
    let src = format!(
        "{PRELUDE}\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.array2([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]])
    if fclose(a.sum(), 21.0):
        println(\"sum_ok\")
    if fclose(a.mean(), 3.5):
        println(\"mean_ok\")
    if fclose(a.min(), 1.0):
        println(\"min_ok\")
    if fclose(a.max(), 6.0):
        println(\"max_ok\")
    if fclose(a.std(), 1.707825127659933):
        println(\"std_ok\")
    return 0
"
    );
    let out = run_ok("reduce_whole", &src);
    assert!(out.contains("sum_ok"), "got: {out:?}");
    assert!(out.contains("mean_ok"), "got: {out:?}");
    assert!(out.contains("min_ok"), "got: {out:?}");
    assert!(out.contains("max_ok"), "got: {out:?}");
    assert!(out.contains("std_ok"), "got: {out:?}");
}

#[test]
fn reductions_per_axis() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.array2([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]])
    println(\"s0=\" + a.sum_axis(0i64).show())
    println(\"s1=\" + a.sum_axis(1i64).show())
    println(\"m0=\" + a.mean_axis(0i64).show())
    println(\"m1=\" + a.mean_axis(1i64).show())
    return 0
";
    let out = run_ok("reduce_axis", src);
    assert!(out.contains("s0=[5.0, 7.0, 9.0]"), "got: {out:?}");
    assert!(out.contains("s1=[6.0, 15.0]"), "got: {out:?}");
    assert!(out.contains("m0=[2.5, 3.5, 4.5]"), "got: {out:?}");
    assert!(out.contains("m1=[2.0, 5.0]"), "got: {out:?}");
}

#[test]
fn mean_empty_raises_value_error() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    empty: List[f64] = []
    a: NDArray = ndarray.array(empty)
    try:
        m: f64 = a.mean()
        println(\"UNREACHED\")
    except ValueError:
        println(\"caught\")
    return 0
";
    let out = run_ok("mean_empty", src);
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

#[test]
fn argmin_argmax_flat_index() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.array([3.0, 1.0, 2.0, 5.0, 0.5])
    println(\"argmin=\" + str(a.argmin()))
    println(\"argmax=\" + str(a.argmax()))
    return 0
";
    let out = run_ok("argmin_argmax", src);
    assert!(out.contains("argmin=4"), "got: {out:?}");
    assert!(out.contains("argmax=3"), "got: {out:?}");
}

// ── Linalg: matmul + dot ──────────────────────────────────────────────

#[test]
fn matmul_2x3_by_3x2() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.array2([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]])
    b: NDArray = ndarray.array2([[7.0, 8.0], [9.0, 10.0], [11.0, 12.0]])
    c: NDArray = a.matmul(b)
    println(\"crows=\" + str(c.shape()[0]))
    println(\"ccols=\" + str(c.shape()[1]))
    println(\"c=\" + c.show())
    return 0
";
    let out = run_ok("matmul", src);
    assert!(out.contains("crows=2"), "got: {out:?}");
    assert!(out.contains("ccols=2"), "got: {out:?}");
    assert!(out.contains("c=[[58.0, 64.0]\n [139.0, 154.0]]"), "got: {out:?}");
}

#[test]
fn matmul_shape_mismatch_raises_value_error() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.array2([[1.0, 2.0], [3.0, 4.0]])
    b: NDArray = ndarray.array2([[1.0, 2.0, 3.0]])
    try:
        c: NDArray = a.matmul(b)
        println(\"UNREACHED\")
    except ValueError:
        println(\"caught\")
    return 0
";
    let out = run_ok("matmul_mismatch", src);
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

#[test]
fn dot_1d_and_length_mismatch() {
    let src = format!(
        "{PRELUDE}\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.array([1.0, 2.0, 3.0])
    b: NDArray = ndarray.array([4.0, 5.0, 6.0])
    if fclose(a.dot(b), 32.0):
        println(\"dot_ok\")
    c: NDArray = ndarray.array([1.0, 2.0])
    try:
        x: f64 = a.dot(c)
        println(\"UNREACHED\")
    except ValueError:
        println(\"caught\")
    return 0
"
    );
    let out = run_ok("dot", &src);
    assert!(out.contains("dot_ok"), "got: {out:?}");
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

// ── Element access + bounds errors ────────────────────────────────────

#[test]
fn get_set_and_row_col_slice() {
    let src = format!(
        "{PRELUDE}\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.array([10.0, 20.0, 30.0, 40.0])
    if fclose(a.get(1i64), 20.0):
        println(\"get_ok\")
    a.set(1i64, 99.0)
    if fclose(a.get(1i64), 99.0):
        println(\"set_ok\")
    println(\"slice=\" + a.slice(1i64, 3i64).show())
    m: NDArray = ndarray.array2([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]])
    if fclose(m.get2(1i64, 2i64), 6.0):
        println(\"get2_ok\")
    m.set2(0i64, 0i64, 100.0)
    if fclose(m.get2(0i64, 0i64), 100.0):
        println(\"set2_ok\")
    println(\"row=\" + m.row(1i64).show())
    println(\"col=\" + m.col(2i64).show())
    println(\"rowslice=\" + m.slice(1i64, 2i64).show())
    return 0
"
    );
    let out = run_ok("get_set", &src);
    assert!(out.contains("get_ok"), "got: {out:?}");
    assert!(out.contains("set_ok"), "got: {out:?}");
    assert!(out.contains("slice=[99.0, 30.0]"), "got: {out:?}");
    assert!(out.contains("get2_ok"), "got: {out:?}");
    assert!(out.contains("set2_ok"), "got: {out:?}");
    assert!(out.contains("row=[4.0, 5.0, 6.0]"), "got: {out:?}");
    assert!(out.contains("col=[3.0, 6.0]"), "got: {out:?}");
    assert!(out.contains("rowslice=[[4.0, 5.0, 6.0]]"), "got: {out:?}");
}

#[test]
fn get_out_of_bounds_raises_index_error() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.array([1.0, 2.0, 3.0])
    try:
        x: f64 = a.get(10i64)
        println(\"UNREACHED\")
    except IndexError:
        println(\"caught_get\")
    try:
        a.set(10i64, 5.0)
        println(\"UNREACHED2\")
    except IndexError:
        println(\"caught_set\")
    return 0
";
    let out = run_ok("get_oob", src);
    assert!(out.contains("caught_get"), "got: {out:?}");
    assert!(out.contains("caught_set"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

// ── Comparisons / masks / where / clip ────────────────────────────────

#[test]
fn masks_where_and_clip() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.array([1.0, 2.0, 3.0, 4.0])
    println(\"gt=\" + a.gt(2.0).show())
    println(\"lt=\" + a.lt(2.0).show())
    println(\"ge=\" + a.ge(2.0).show())
    println(\"le=\" + a.le(2.0).show())
    println(\"eq=\" + a.eq_mask(3.0).show())
    println(\"clip=\" + a.clip(2.0, 3.0).show())
    mask: NDArray = a.gt(2.0)
    hi: NDArray = ndarray.full([4i64], 100.0)
    lo: NDArray = ndarray.full([4i64], 0.0)
    println(\"where=\" + ndarray.where_mask(mask, hi, lo).show())
    return 0
";
    let out = run_ok("masks", src);
    assert!(out.contains("gt=[0.0, 0.0, 1.0, 1.0]"), "got: {out:?}");
    assert!(out.contains("lt=[1.0, 0.0, 0.0, 0.0]"), "got: {out:?}");
    assert!(out.contains("ge=[0.0, 1.0, 1.0, 1.0]"), "got: {out:?}");
    assert!(out.contains("le=[1.0, 1.0, 0.0, 0.0]"), "got: {out:?}");
    assert!(out.contains("eq=[0.0, 0.0, 1.0, 0.0]"), "got: {out:?}");
    assert!(out.contains("clip=[2.0, 2.0, 3.0, 3.0]"), "got: {out:?}");
    assert!(out.contains("where=[0.0, 0.0, 100.0, 100.0]"), "got: {out:?}");
}

// ── to_list round-trip + copy independence ────────────────────────────

#[test]
fn to_list_round_trip_and_copy() {
    let src = format!(
        "{PRELUDE}\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    src: List[f64] = [1.5, 2.5, 3.5]
    a: NDArray = ndarray.array(src)
    out: List[f64] = a.to_list()
    ok: bool = true
    i: i64 = 0i64
    while i < 3i64:
        if not fclose(out[i], src[i]):
            ok = false
        i = i + 1i64
    if ok:
        println(\"roundtrip_ok\")
    b: NDArray = a.copy()
    b.set(0i64, 99.0)
    if fclose(a.get(0i64), 1.5):
        println(\"copy_independent\")
    return 0
"
    );
    let out = run_ok("to_list", &src);
    assert!(out.contains("roundtrip_ok"), "got: {out:?}");
    assert!(out.contains("copy_independent"), "got: {out:?}");
}

// ── show() formatting for 1-D and 2-D ─────────────────────────────────

#[test]
fn show_formatting_1d_and_2d() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    v: NDArray = ndarray.array([1.0, 2.0, 3.0])
    println(\"v=\" + v.show())
    m: NDArray = ndarray.array2([[1.0, 2.0], [3.0, 4.0]])
    println(\"m=\" + m.show())
    return 0
";
    let out = run_ok("show_fmt", src);
    assert!(out.contains("v=[1.0, 2.0, 3.0]"), "got: {out:?}");
    assert!(out.contains("m=[[1.0, 2.0]\n [3.0, 4.0]]"), "got: {out:?}");
}

// ── free() then reuse raises ValueError ───────────────────────────────

#[test]
fn free_then_reuse_raises_value_error() {
    let src = "\
import ndarray
from ndarray import NDArray
fn main() -> i32:
    a: NDArray = ndarray.array([1.0, 2.0, 3.0])
    a.free()
    try:
        s: f64 = a.sum()
        println(\"UNREACHED\")
    except ValueError:
        println(\"caught_use\")
    try:
        a.free()
        println(\"UNREACHED2\")
    except ValueError:
        println(\"caught_double_free\")
    return 0
";
    let out = run_ok("free_reuse", src);
    assert!(out.contains("caught_use"), "got: {out:?}");
    assert!(out.contains("caught_double_free"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}
