//! M61a regression tests: user callbacks across the NativeFn boundary.
//!
//! Surface added: the higher-order builtins `map` / `filter` / `reduce` /
//! `sorted_by` and the in-place `List.sort_by`, each of which receives a
//! StrictPy closure value and re-enters the interpreter to invoke it per
//! element. Before M61a, function values were first-class within StrictPy
//! but could not cross into a native stdlib function.
//!
//! Coverage:
//!   1. `map` with a top-level-shaped closure (no capture).
//!   2. `map` with a closure that *captures* an enclosing variable —
//!      proves real closures, not just top-level fns, cross the boundary.
//!   3. `filter` with a capturing predicate.
//!   4. `reduce` for sum (i64) and product, with a non-zero init.
//!   5. `reduce` building up a str accumulator (heap pointer accumulator,
//!      exercises the temp-root that keeps the running acc reachable).
//!   6. `sorted_by` over List[i64] by an identity-ish key, and over
//!      List[str] by a str key.
//!   7. In-place `xs.sort_by(key)` mutates the receiver.
//!   8. `sorted_by` over a list of user records (class values) by an
//!      i32→i64 key, stable on ties.
//!   9. A large input that forces several GC cycles mid-callback, to prove
//!      the in-flight list / closure / accumulator stay reachable.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn run(test_name: &str, src: &str) -> (i32, String) {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m61_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    run_file_capture(&out).expect("run")
}

// ── (1) map without capture ─────────────────────────────────────────────

#[test]
fn map_no_capture() {
    let src = "\
fn main() -> i32:
    xs: List[i64] = [1, 2, 3, 4]
    ys: List[i64] = map(fn(x: i64) -> i64: x * x, xs)
    for y: i64 in ys:
        println(str(y))
    return 0
";
    let (code, out) = run("map_no_capture", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "1\n4\n9\n16\n", "{out:?}");
}

// ── (2) map with a CAPTURING closure ────────────────────────────────────

#[test]
fn map_capturing_closure() {
    let src = "\
fn main() -> i32:
    factor: i64 = 7
    xs: List[i64] = [1, 2, 3]
    ys: List[i64] = map(fn(x: i64) -> i64: x * factor, xs)
    for y: i64 in ys:
        println(str(y))
    return 0
";
    let (code, out) = run("map_capturing", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "7\n14\n21\n", "{out:?}");
}

// ── (3) filter with a capturing predicate ───────────────────────────────

#[test]
fn filter_capturing_predicate() {
    let src = "\
fn main() -> i32:
    lo: i64 = 3
    xs: List[i64] = [1, 5, 2, 8, 3, 9, 4]
    kept: List[i64] = filter(fn(x: i64) -> bool: x > lo, xs)
    for y: i64 in kept:
        println(str(y))
    return 0
";
    let (code, out) = run("filter_capturing", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "5\n8\n9\n4\n", "{out:?}");
}

// ── (4) reduce: sum + product ───────────────────────────────────────────

#[test]
fn reduce_sum_and_product() {
    let src = "\
fn main() -> i32:
    xs: List[i64] = [1, 2, 3, 4, 5]
    s: i64 = reduce(fn(acc: i64, x: i64) -> i64: acc + x, xs, 0)
    p: i64 = reduce(fn(acc: i64, x: i64) -> i64: acc * x, xs, 1)
    println(str(s))
    println(str(p))
    return 0
";
    let (code, out) = run("reduce_sum_product", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "15\n120\n", "{out:?}");
}

// ── (5) reduce building a str accumulator (heap acc) ─────────────────────

#[test]
fn reduce_str_accumulator() {
    let src = "\
fn main() -> i32:
    xs: List[str] = [\"a\", \"b\", \"c\", \"d\"]
    joined: str = reduce(fn(acc: str, x: str) -> str: acc + x, xs, \"\")
    println(joined)
    return 0
";
    let (code, out) = run("reduce_str_acc", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "abcd\n", "{out:?}");
}

// ── (6) sorted_by over i64 and str ──────────────────────────────────────

#[test]
fn sorted_by_i64_and_str() {
    let src = "\
fn main() -> i32:
    xs: List[i64] = [5, 1, 4, 2, 3]
    a: List[i64] = sorted_by(xs, fn(v: i64) -> i64: v)
    for y: i64 in a:
        print(str(y))
    println(\"\")
    # Sort descending by negating the key.
    d: List[i64] = sorted_by(xs, fn(v: i64) -> i64: 0i64 - v)
    for z: i64 in d:
        print(str(z))
    println(\"\")
    ws: List[str] = [\"pear\", \"apple\", \"fig\", \"banana\"]
    bn: List[str] = sorted_by(ws, fn(w: str) -> str: w)
    for w: str in bn:
        print(w + \",\")
    println(\"\")
    return 0
";
    let (code, out) = run("sorted_by_i64_str", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "12345\n54321\napple,banana,fig,pear,\n", "{out:?}");
}

// ── (7) in-place sort_by ────────────────────────────────────────────────

#[test]
fn list_sort_by_in_place() {
    let src = "\
fn main() -> i32:
    ws: List[str] = [\"ccc\", \"a\", \"bb\", \"dddd\"]
    ws.sort_by(fn(w: str) -> i64: i64(w.len()))
    for w: str in ws:
        print(w + \",\")
    println(\"\")
    return 0
";
    let (code, out) = run("list_sort_by", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "a,bb,ccc,dddd,\n", "{out:?}");
}

// ── (8) sorted_by over user records, stable on ties ─────────────────────

#[test]
fn sorted_by_records_stable() {
    let src = "\
final class P:
    name: str
    age: i32
    fn __init__(self, name: str, age: i32) -> None:
        self.name = name
        self.age = age

fn main() -> i32:
    ps: List[P] = [P(\"carol\", 30), P(\"alice\", 42), P(\"bob\", 19), P(\"dave\", 30)]
    by_age: List[P] = sorted_by(ps, fn(p: P) -> i64: i64(p.age))
    for p: P in by_age:
        print(p.name + \",\")
    println(\"\")
    return 0
";
    let (code, out) = run("sorted_by_records", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    // bob(19), carol(30), dave(30) [stable: carol before dave], alice(42).
    assert_eq!(out, "bob,carol,dave,alice,\n", "{out:?}");
}

// ── (9) large input → GC pressure mid-callback ──────────────────────────

#[test]
fn callbacks_survive_gc() {
    // The callback allocates a fresh str on every call, so a big list forces
    // several collections while the in-flight result list / closure /
    // accumulator are only reachable through `temp_roots`. If rooting were
    // wrong, this would crash or produce garbage.
    let src = "\
fn main() -> i32:
    xs: List[i64] = []
    i: i64 = 0
    while i < 5000:
        xs.append(i)
        i = i + 1
    # map each i to its string, then reduce to total length.
    ss: List[str] = map(fn(x: i64) -> str: str(x), xs)
    total: i64 = reduce(fn(acc: i64, s: str) -> i64: acc + i64(s.len()), ss, 0)
    println(str(total))
    # filter evens, count them.
    evens: List[i64] = filter(fn(x: i64) -> bool: x % 2 == 0, xs)
    println(str(len(evens)))
    return 0
";
    let (code, out) = run("callbacks_gc", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    // sum of digit-lengths of 0..4999:
    //   0..9      : 10 nums  * 1 = 10
    //   10..99    : 90 nums  * 2 = 180
    //   100..999  : 900 nums * 3 = 2700
    //   1000..4999: 4000 nums* 4 = 16000
    //   total = 18890
    assert_eq!(out, "18890\n2500\n", "{out:?}");
}
