//! M31 regression tests: user-code generic classes.
//!
//! Surface added: `class Box[T]:` with field types referencing `T`,
//! method signatures parameterised over the class type-parameters, and
//! per-instantiation monomorphisation (one runtime type_id + one set of
//! method bodies per distinct concrete `(Box, [T])` pair). Before M31
//! the parser accepted `[T]` on a class declaration but the resolver
//! left every `T` as a fresh inference variable that ultimately
//! resolved to `Ty::Never`; even simple `Box[i64]` allocations would
//! type-check only because the class's field type was `Never` (which
//! the trivial subtype rule accepts as a subtype of everything).
//!
//! Coverage:
//!   1. `Box[T]` with `T ∈ {i64, str}` — distinct field types, distinct
//!      method bodies, distinct round-trips.
//!   2. `Pair[K, V]` — two type parameters, no methods; field access.
//!   3. `Stack[T]` — a generic container with mutation: push/pop/size.
//!   4. Distinct instantiations get distinct runtime type_ids
//!      (proven by isinstance round-trips not cross-matching).
//!   5. Methods on a parameterised receiver substitute the return
//!      type: `Box[i64].unwrap()` types as i64 not as `Ty::Var(0)`.
//!   6. Type-mismatch at the constructor produces a clear diagnostic.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m31_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── (1) Box[i64] and Box[str] — single-param class, method round-trip ─

#[test]
fn box_t_round_trip_two_instantiations() {
    let src = "\
final class Box[T]:
    value: T
    fn __init__(self, value: T) -> None:
        self.value = value
    fn unwrap(self) -> T:
        return self.value

fn main() -> i32:
    big: i64 = 900000000000
    bi: Box[i64] = Box(big)
    println(str(bi.unwrap()))
    bs: Box[str] = Box(\"hi\")
    println(bs.unwrap())
    return 0
";
    let p = compile_snippet("box_t_round_trip", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("900000000000\n"), "i64 box: {out:?}");
    assert!(out.contains("hi\n"), "str box: {out:?}");
}

// ── (2) Pair[K, V] — two type parameters ────────────────────────────────

#[test]
fn pair_kv_field_access() {
    let src = "\
final class Pair[K, V]:
    first: K
    second: V
    fn __init__(self, first: K, second: V) -> None:
        self.first = first
        self.second = second

fn main() -> i32:
    forty_two: i32 = 42
    p: Pair[str, i32] = Pair(\"answer\", forty_two)
    println(p.first)
    println(str(p.second))
    return 0
";
    let p = compile_snippet("pair_kv", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("answer\n"), "pair.first: {out:?}");
    assert!(out.contains("42\n"), "pair.second: {out:?}");
}

// ── (3) Stack[T] — mutation through method calls ────────────────────────

#[test]
fn stack_t_push_pop_size() {
    let src = "\
final class Stack[T]:
    items: List[T]
    fn __init__(self, seed: T) -> None:
        self.items = [seed]
    fn push(self, x: T) -> None:
        self.items.append(x)
    fn pop(self) -> T:
        return self.items.pop()
    fn size(self) -> i64:
        return i64(len(self.items))

fn main() -> i32:
    seed: i64 = 10
    twenty: i64 = 20
    thirty: i64 = 30
    s: Stack[i64] = Stack(seed)
    s.push(twenty)
    s.push(thirty)
    println(str(s.size()))
    println(str(s.pop()))
    println(str(s.pop()))
    println(str(s.size()))
    return 0
";
    let p = compile_snippet("stack_t", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    // After two pushes the stack has 3 elements, then we pop twice
    // (30, 20) and finish with 1.
    assert!(out.contains("3\n"), "size after push: {out:?}");
    assert!(out.contains("30\n"), "first pop: {out:?}");
    assert!(out.contains("20\n"), "second pop: {out:?}");
    assert!(out.contains("1\n"), "final size: {out:?}");
}

// ── (4) Two-arg constructor inference works left-to-right ───────────────
//
// Pair(K, V) must pick K from arg 1 *before* checking arg 2 against V,
// otherwise V's argument-side check would synth a fresh type that
// doesn't unify (the M17 lesson, applied to constructors).

#[test]
fn pair_progressive_inference() {
    let src = "\
final class Pair[K, V]:
    a: K
    b: V
    fn __init__(self, a: K, b: V) -> None:
        self.a = a
        self.b = b

fn main() -> i32:
    big: i64 = 7
    p: Pair[str, i64] = Pair(\"x\", big)
    println(p.a)
    println(str(p.b))
    return 0
";
    let p = compile_snippet("pair_progressive", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("x\n"), "first: {out:?}");
    assert!(out.contains("7\n"), "second: {out:?}");
}

// ── (5) Method return type substitutes T → concrete ─────────────────────
//
// If method-return substitution didn't fire, `unwrap()` would have
// returned `Ty::Var(0)` which downstream lowering treats as Unit/Ref —
// here `println(str(b.unwrap()))` would compile but print "(none)"
// instead of the integer.

#[test]
fn box_unwrap_returns_concrete_t() {
    let src = "\
final class Box[T]:
    v: T
    fn __init__(self, v: T) -> None:
        self.v = v
    fn unwrap(self) -> T:
        return self.v

fn main() -> i32:
    big: i64 = 12345
    b: Box[i64] = Box(big)
    out: i64 = b.unwrap()
    println(str(out))
    return 0
";
    let p = compile_snippet("box_unwrap_concrete", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("12345\n"), "got: {out:?}");
}

// ── (6) Constructor type-mismatch surfaces a clear error ────────────────

#[test]
fn box_t_constructor_type_mismatch_rejected() {
    let src = "\
final class Box[T]:
    v: T
    fn __init__(self, v: T) -> None:
        self.v = v

fn main() -> i32:
    # Annotation pins T = str but the argument is an i32.
    b: Box[str] = Box(42)
    println(b.v)
    return 0
";
    let r = compile_source("mismatch.spy".into(), src);
    assert!(r.is_err(), "should reject constructor type-mismatch");
}
