//! M63b regression tests: bounded (constrained) generics.
//!
//! Surface added on top of M17 (generic free functions) and M31 (generic
//! classes): a single built-in *bound* may be declared on a type parameter,
//! `fn max2[T: Comparable](a: T, b: T) -> T` / `class Sorted[T: Comparable]`.
//!
//! Built-in bounds: `Comparable` (ordering), `Equatable` (==/!=),
//! `Printable` (str()). Only primitive / str / char types satisfy a bound —
//! there is no user trait/impl system in v1.
//!
//! Coverage:
//!   1. Bounded fn `max2[T: Comparable]` over i64, f64, str.
//!   2. `Equatable` bound enables `==` inside a generic body.
//!   3. Bounded generic class `Sorted[T: Comparable]`.
//!   4. NEGATIVE: a non-Comparable type arg (a user class) is rejected.
//!   5. NEGATIVE: `<` on an *unbounded* `[T]` is a compile error.
//!   6. NEGATIVE: ordering (`<`) on an `[T: Equatable]` (no ordering) is
//!      rejected.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m63b_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── (1) bounded fn over multiple primitive types ────────────────────────

#[test]
fn comparable_max_over_primitives() {
    let src = "\
fn max2[T: Comparable](a: T, b: T) -> T:
    if a < b:
        return b
    return a

fn main() -> i32:
    x: i64 = 5
    y: i64 = 11
    println(str(max2(x, y)))
    f: f64 = 9.5
    g: f64 = 2.25
    println(str(max2(f, g)))
    println(max2(\"alpha\", \"omega\"))
    return 0
";
    let p = compile_snippet("comparable_max", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("11\n"), "i64 max: {out:?}");
    assert!(out.contains("9.5\n"), "f64 max: {out:?}");
    assert!(out.contains("omega\n"), "str max: {out:?}");
}

// ── (2) Equatable bound enables == inside the body ──────────────────────

#[test]
fn equatable_bound_enables_eq() {
    let src = "\
fn same[T: Equatable](a: T, b: T) -> bool:
    return a == b

fn main() -> i32:
    p: i64 = 4
    q: i64 = 4
    if same(p, q):
        println(\"eq\")
    else:
        println(\"ne\")
    if same(\"a\", \"b\"):
        println(\"eq\")
    else:
        println(\"ne\")
    return 0
";
    let p = compile_snippet("equatable_eq", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("eq\n"), "int eq: {out:?}");
    assert!(out.contains("ne\n"), "str ne: {out:?}");
}

// ── (3) bounded generic class ───────────────────────────────────────────

#[test]
fn comparable_bounded_class() {
    let src = "\
final class Sorted[T: Comparable]:
    lo: T
    hi: T
    fn __init__(self, a: T, b: T) -> None:
        if a < b:
            self.lo = a
            self.hi = b
        else:
            self.lo = b
            self.hi = a
    fn largest(self) -> T:
        return self.hi

fn main() -> i32:
    a: i64 = 3
    b: i64 = 8
    s: Sorted[i64] = Sorted(a, b)
    println(str(s.largest()))
    ss: Sorted[str] = Sorted(\"yak\", \"ant\")
    println(ss.largest())
    return 0
";
    let p = compile_snippet("sorted_class", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("8\n"), "i64 class: {out:?}");
    assert!(out.contains("yak\n"), "str class: {out:?}");
}

// ── (4) NEGATIVE: non-Comparable type arg rejected ──────────────────────

#[test]
fn non_comparable_type_arg_is_rejected() {
    let src = "\
final class Widget:
    v: i32
    fn __init__(self, v: i32) -> None:
        self.v = v

fn max2[T: Comparable](a: T, b: T) -> T:
    if a < b:
        return b
    return a

fn main() -> i32:
    p: Widget = Widget(1)
    q: Widget = Widget(2)
    r: Widget = max2(p, q)
    return 0
";
    let r = compile_source("non_comparable.spy".to_string(), src);
    let err = r.expect_err("expected a bound-satisfaction error");
    let msg = format!("{}", err);
    assert!(
        msg.contains("satisfy bound") || msg.contains("Comparable") || msg.contains("E2015"),
        "expected unsatisfied-bound error, got: {msg}"
    );
}

// ── (5) NEGATIVE: `<` on an unbounded T is a compile error ──────────────

#[test]
fn unbounded_t_cannot_compare() {
    let src = "\
fn bad[T](a: T, b: T) -> bool:
    return a < b

fn main() -> i32:
    return 0
";
    let r = compile_source("unbounded_lt.spy".to_string(), src);
    let err = r.expect_err("expected an unbounded-comparison error");
    let msg = format!("{}", err);
    assert!(
        msg.contains("Comparable") || msg.contains("E2015") || msg.contains("bound"),
        "expected a bound-required error, got: {msg}"
    );
}

// ── (6) NEGATIVE: ordering on an Equatable-only T is rejected ────────────

#[test]
fn equatable_cannot_order() {
    let src = "\
fn bad[T: Equatable](a: T, b: T) -> bool:
    return a < b

fn main() -> i32:
    return 0
";
    let r = compile_source("equatable_lt.spy".to_string(), src);
    let err = r.expect_err("expected an ordering-not-allowed error");
    let msg = format!("{}", err);
    assert!(
        msg.contains("Comparable") || msg.contains("E2015") || msg.contains("bound"),
        "expected a Comparable-required error, got: {msg}"
    );
}
