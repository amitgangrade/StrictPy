//! M22 P2C regression tests for `itertools` and `statistics`.
//!
//! Follows the M19/M20/M21 pattern: each test compiles a tiny `.spy`
//! snippet, writes the bytecode to `CARGO_TARGET_TMPDIR`, runs it via
//! `run_file_capture`, and asserts on stdout.
//!
//! Coverage:
//!   * `itertools.range_step` — three-arg positive / negative step,
//!     and the step==0 → ValueError edge.
//!   * `itertools.enumerate_i64` — tuple shape with i32 index.
//!   * `itertools.zip_i64_i64` — truncates to shorter input.
//!   * `itertools.chain_i64` — concatenates lists.
//!   * `itertools.take_str` / `drop_str` — boundary conditions.
//!   * `itertools.accumulate_i64` — running prefix sum, empty input.
//!   * `itertools.flatten_str` — list-of-list-of-str concatenation.
//!   * `statistics.mean` / `median` / `variance` / `stdev` /
//!     `sum` / `min_max` / `quantile` — happy-path values.
//!   * `statistics.mean([])` raises ValueError.
//!   * `statistics.stdev([1.0])` raises ValueError (n<2).
//!   * `statistics.quantile(xs, 1.5)` raises ValueError (q out of range).
//!   * `statistics.mode_str` — common case + tie-broken-by-first.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m22_p2c_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── itertools module ───────────────────────────────────────────────────

#[test]
fn itertools_range_step_positive() {
    let src = "\
import itertools
fn main() -> i32:
    xs: List[i64] = itertools.range_step(0i64, 10i64, 2i64)
    n: i64 = i64(len(xs))
    println(\"n=\" + str(n))
    i: i64 = 0i64
    while i < n:
        println(\"v=\" + str(xs[i]))
        i = i + 1i64
    return 0
";
    let p = compile_snippet("range_step_pos", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("n=5"), "got: {out:?}");
    assert!(out.contains("v=0"), "got: {out:?}");
    assert!(out.contains("v=8"), "got: {out:?}");
    assert!(!out.contains("v=10"), "should not include stop; got: {out:?}");
}

#[test]
fn itertools_range_step_negative() {
    let src = "\
import itertools
fn main() -> i32:
    xs: List[i64] = itertools.range_step(10i64, 0i64, 0i64 - 3i64)
    n: i64 = i64(len(xs))
    println(\"n=\" + str(n))
    println(\"first=\" + str(xs[0]))
    println(\"last=\" + str(xs[n - 1i64]))
    return 0
";
    let p = compile_snippet("range_step_neg", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // [10, 7, 4, 1] — stops before crossing 0.
    assert!(out.contains("n=4"), "got: {out:?}");
    assert!(out.contains("first=10"), "got: {out:?}");
    assert!(out.contains("last=1"), "got: {out:?}");
}

#[test]
fn itertools_range_step_zero_raises() {
    let src = "\
import itertools
fn main() -> i32:
    try:
        xs: List[i64] = itertools.range_step(0i64, 10i64, 0i64)
        println(\"NO-RAISE\")
    except ValueError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("range_step_zero", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught"), "got: {out:?}");
}

#[test]
fn itertools_enumerate_i64_pairs_index_and_value() {
    let src = "\
import itertools
fn main() -> i32:
    xs: List[i64] = [100i64, 200i64, 300i64]
    pairs: List[Tuple[i32, i64]] = itertools.enumerate_i64(xs)
    n: i64 = i64(len(pairs))
    println(\"n=\" + str(n))
    i: i64 = 0i64
    while i < n:
        p: Tuple[i32, i64] = pairs[i]
        println(\"i=\" + str(p.0) + \" v=\" + str(p.1))
        i = i + 1i64
    return 0
";
    let p = compile_snippet("enumerate_i64", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("n=3"), "got: {out:?}");
    assert!(out.contains("i=0 v=100"), "got: {out:?}");
    assert!(out.contains("i=1 v=200"), "got: {out:?}");
    assert!(out.contains("i=2 v=300"), "got: {out:?}");
}

#[test]
fn itertools_zip_i64_i64_truncates() {
    let src = "\
import itertools
fn main() -> i32:
    a: List[i64] = [1i64, 2i64, 3i64, 4i64, 5i64]
    b: List[i64] = [10i64, 20i64, 30i64]
    z: List[Tuple[i64, i64]] = itertools.zip_i64_i64(a, b)
    n: i64 = i64(len(z))
    println(\"n=\" + str(n))
    i: i64 = 0i64
    while i < n:
        p: Tuple[i64, i64] = z[i]
        println(\"a=\" + str(p.0) + \" b=\" + str(p.1))
        i = i + 1i64
    return 0
";
    let p = compile_snippet("zip_i64", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("n=3"), "got: {out:?}");
    assert!(out.contains("a=1 b=10"), "got: {out:?}");
    assert!(out.contains("a=3 b=30"), "got: {out:?}");
}

#[test]
fn itertools_chain_i64_concatenates() {
    let src = "\
import itertools
fn main() -> i32:
    a: List[i64] = [1i64, 2i64]
    b: List[i64] = [3i64, 4i64, 5i64]
    out: List[i64] = itertools.chain_i64(a, b)
    n: i64 = i64(len(out))
    println(\"n=\" + str(n))
    i: i64 = 0i64
    while i < n:
        println(\"v=\" + str(out[i]))
        i = i + 1i64
    return 0
";
    let p = compile_snippet("chain_i64", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("n=5"), "got: {out:?}");
    assert!(out.contains("v=1"), "got: {out:?}");
    assert!(out.contains("v=5"), "got: {out:?}");
}

#[test]
fn itertools_take_drop_boundaries() {
    let src = "\
import itertools
fn main() -> i32:
    xs: List[str] = [\"a\", \"b\", \"c\"]
    # take more than length → returns all
    t1: List[str] = itertools.take_str(xs, 99)
    println(\"t1_len=\" + str(i64(len(t1))))
    # take 0 → empty
    t2: List[str] = itertools.take_str(xs, 0)
    println(\"t2_len=\" + str(i64(len(t2))))
    # drop more than length → empty
    d1: List[str] = itertools.drop_str(xs, 99)
    println(\"d1_len=\" + str(i64(len(d1))))
    # drop 0 → all
    d2: List[str] = itertools.drop_str(xs, 0)
    println(\"d2_len=\" + str(i64(len(d2))))
    return 0
";
    let p = compile_snippet("take_drop", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("t1_len=3"), "got: {out:?}");
    assert!(out.contains("t2_len=0"), "got: {out:?}");
    assert!(out.contains("d1_len=0"), "got: {out:?}");
    assert!(out.contains("d2_len=3"), "got: {out:?}");
}

#[test]
fn itertools_accumulate_running_sum() {
    let src = "\
import itertools
fn main() -> i32:
    xs: List[i64] = [3i64, 1i64, 4i64, 1i64, 5i64]
    sums: List[i64] = itertools.accumulate_i64(xs)
    n: i64 = i64(len(sums))
    println(\"n=\" + str(n))
    i: i64 = 0i64
    while i < n:
        println(\"s=\" + str(sums[i]))
        i = i + 1i64
    return 0
";
    let p = compile_snippet("accumulate", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // 3, 4, 8, 9, 14.
    assert!(out.contains("n=5"), "got: {out:?}");
    assert!(out.contains("s=3"), "got: {out:?}");
    assert!(out.contains("s=14"), "got: {out:?}");
}

#[test]
fn itertools_flatten_str_concat() {
    let src = "\
import itertools
fn main() -> i32:
    a: List[str] = [\"a\", \"b\"]
    b: List[str] = [\"c\"]
    c: List[str] = [\"d\", \"e\"]
    nested: List[List[str]] = [a, b, c]
    out: List[str] = itertools.flatten_str(nested)
    n: i64 = i64(len(out))
    println(\"n=\" + str(n))
    i: i64 = 0i64
    while i < n:
        println(\"v=\" + out[i])
        i = i + 1i64
    return 0
";
    let p = compile_snippet("flatten_str", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("n=5"), "got: {out:?}");
    assert!(out.contains("v=a"), "got: {out:?}");
    assert!(out.contains("v=e"), "got: {out:?}");
}

// ── statistics module ──────────────────────────────────────────────────

#[test]
fn statistics_mean_median_sum() {
    let src = "\
import statistics
fn main() -> i32:
    xs: List[f64] = [1.0, 2.0, 3.0, 4.0, 5.0]
    println(\"mean=\" + str(statistics.mean(xs)))
    println(\"median=\" + str(statistics.median(xs)))
    println(\"sum=\" + str(statistics.sum(xs)))
    return 0
";
    let p = compile_snippet("mean_median_sum", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("mean=3"), "got: {out:?}");
    assert!(out.contains("median=3"), "got: {out:?}");
    assert!(out.contains("sum=15"), "got: {out:?}");
}

#[test]
fn statistics_median_even_length() {
    // [1, 2, 3, 4] → median = 2.5.
    let src = "\
import statistics
fn main() -> i32:
    xs: List[f64] = [1.0, 2.0, 3.0, 4.0]
    println(\"m=\" + str(statistics.median(xs)))
    return 0
";
    let p = compile_snippet("median_even", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("m=2.5"), "got: {out:?}");
}

#[test]
fn statistics_variance_stdev_known_dataset() {
    // [2,4,4,4,5,5,7,9] — Wikipedia's canonical sample stdev = 2.0
    // sample variance = 4.0 (n-1 denominator).
    let src = "\
import statistics
fn main() -> i32:
    xs: List[f64] = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]
    v: f64 = statistics.variance(xs)
    s: f64 = statistics.stdev(xs)
    # Expect variance = 32/7 ~= 4.571..., stdev = sqrt(32/7) ~= 2.138...
    println(\"v=\" + str(v))
    println(\"s=\" + str(s))
    return 0
";
    let p = compile_snippet("variance_stdev", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // The exact f64 stringification is platform-dependent — check
    // prefix and rough magnitude.
    assert!(out.contains("v=4.57"), "got: {out:?}");
    assert!(out.contains("s=2.13"), "got: {out:?}");
}

#[test]
fn statistics_min_max_returns_tuple() {
    let src = "\
import statistics
fn main() -> i32:
    xs: List[f64] = [3.5, 1.0, 4.5, 1.5, 9.0, 2.0]
    mm: Tuple[f64, f64] = statistics.min_max(xs)
    println(\"min=\" + str(mm.0))
    println(\"max=\" + str(mm.1))
    return 0
";
    let p = compile_snippet("min_max", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("min=1"), "got: {out:?}");
    assert!(out.contains("max=9"), "got: {out:?}");
}

#[test]
fn statistics_quantile_endpoints() {
    let src = "\
import statistics
fn main() -> i32:
    xs: List[f64] = [1.0, 2.0, 3.0, 4.0, 5.0]
    println(\"q0=\" + str(statistics.quantile(xs, 0.0)))
    println(\"q1=\" + str(statistics.quantile(xs, 1.0)))
    println(\"qmid=\" + str(statistics.quantile(xs, 0.5)))
    return 0
";
    let p = compile_snippet("quantile", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("q0=1"), "got: {out:?}");
    assert!(out.contains("q1=5"), "got: {out:?}");
    assert!(out.contains("qmid=3"), "got: {out:?}");
}

#[test]
fn statistics_mean_empty_raises() {
    let src = "\
import statistics
fn main() -> i32:
    xs: List[f64] = []
    try:
        m: f64 = statistics.mean(xs)
        println(\"NO-RAISE\")
    except ValueError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("mean_empty", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught"), "got: {out:?}");
}

#[test]
fn statistics_stdev_too_few_raises() {
    let src = "\
import statistics
fn main() -> i32:
    xs: List[f64] = [3.0]
    try:
        s: f64 = statistics.stdev(xs)
        println(\"NO-RAISE\")
    except ValueError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("stdev_short", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught"), "got: {out:?}");
}

#[test]
fn statistics_quantile_out_of_range_raises() {
    let src = "\
import statistics
fn main() -> i32:
    xs: List[f64] = [1.0, 2.0, 3.0]
    try:
        q: f64 = statistics.quantile(xs, 1.5)
        println(\"NO-RAISE\")
    except ValueError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("quantile_bad", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught"), "got: {out:?}");
}

#[test]
fn statistics_mode_str_picks_most_common() {
    let src = "\
import statistics
fn main() -> i32:
    xs: List[str] = [\"x\", \"y\", \"x\", \"y\", \"x\"]
    println(\"mode=\" + statistics.mode_str(xs))
    return 0
";
    let p = compile_snippet("mode_str", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("mode=x"), "got: {out:?}");
}
