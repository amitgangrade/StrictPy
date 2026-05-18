//! M16 regression tests: `isinstance(x, T)` + `match` / `case` lowering.
//!
//! Before M16:
//!   * `isinstance` typechecked through the generic Function fallback but
//!     emitted `Opcode::IsInstance` with a sentinel `u32::MAX` operand,
//!     which the VM short-circuited to "always true". Programs needing
//!     true runtime class identity rolled hand-coded `kind: i32`
//!     discriminator fields (documented in `examples/json_parse.spy`).
//!   * `Stmt::Match` parsed end-to-end but `ir.rs` had an M4-era empty
//!     handler — `case Pair(car, cdr):` simply fell through.
//!
//! Coverage:
//!   1. Basic `isinstance(x, Cat)` true/false.
//!   2. Subclass match via `isinstance(Cat(), Animal)`.
//!   3. Flow narrowing: `if isinstance(a, Cat): a.meow()` typechecks.
//!   4. Nullable receiver: `x: Cat? = Cat(); if isinstance(x, Cat): x.name`.
//!   5. `match` over sealed Shape hierarchy with field-binding patterns.
//!   6. `match` with a tuple pattern.
//!   7. Terminal `case _:` catches the unmatched residual.
//!   8. Scrutinee evaluated exactly once (counter side-effect).
//!   9. Exhaustiveness warning on a non-wildcard sealed match still
//!      typechecks (the warning goes to stderr but is not fatal in v0.1).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m16_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── (1) basic isinstance true/false ─────────────────────────────────────

#[test]
fn isinstance_basic_true_and_false() {
    let src = "\
open class Animal:
    fn __init__(self) -> None:
        pass

final class Cat(Animal):
    fn __init__(self) -> None:
        pass

final class Dog(Animal):
    fn __init__(self) -> None:
        pass

fn main() -> i32:
    c: Cat = Cat()
    d: Dog = Dog()
    if isinstance(c, Cat):
        println(\"c is Cat\")
    if isinstance(d, Cat):
        println(\"d is Cat (BAD)\")
    if not isinstance(d, Cat):
        println(\"d is not Cat\")
    return 0
";
    let p = compile_snippet("isinstance_basic", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("c is Cat\n"), "expected positive: {out:?}");
    assert!(out.contains("d is not Cat\n"), "expected negative: {out:?}");
    assert!(!out.contains("BAD"), "got: {out:?}");
}

// ── (2) subclass matching via parent-chain walk ─────────────────────────

#[test]
fn isinstance_walks_inheritance_chain() {
    let src = "\
open class Animal:
    fn __init__(self) -> None:
        pass

final class Cat(Animal):
    fn __init__(self) -> None:
        pass

fn main() -> i32:
    c: Animal = Cat()
    if isinstance(c, Animal):
        println(\"Cat is Animal\")
    if isinstance(c, Cat):
        println(\"Cat is Cat\")
    return 0
";
    let p = compile_snippet("isinstance_subclass", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("Cat is Animal\n"), "got: {out:?}");
    assert!(out.contains("Cat is Cat\n"), "got: {out:?}");
}

// ── (3) flow narrowing — a.meow() needs Cat-typed `a` ───────────────────

#[test]
fn isinstance_narrows_in_if_body() {
    let src = "\
open class Animal:
    fn __init__(self) -> None:
        pass

final class Cat(Animal):
    name: str

    fn __init__(self, name: str) -> None:
        self.name = name

    fn meow(self) -> str:
        return self.name + \" says meow\"

fn speak(a: Animal) -> str:
    if isinstance(a, Cat):
        return a.meow()
    return \"generic\"

fn main() -> i32:
    println(speak(Cat(\"Felix\")))
    return 0
";
    let p = compile_snippet("isinstance_narrows", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("Felix says meow\n"), "got: {out:?}");
}

// ── (4) narrowing strips Nullable too ───────────────────────────────────

#[test]
fn isinstance_narrows_nullable_receiver() {
    let src = "\
final class Cat:
    name: str

    fn __init__(self, n: str) -> None:
        self.name = n

fn main() -> i32:
    x: Cat? = Cat(\"Misha\")
    if isinstance(x, Cat):
        println(x.name)
    return 0
";
    let p = compile_snippet("isinstance_nullable", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("Misha\n"), "got: {out:?}");
}

// ── (5) match Constructor patterns on sealed Shape hierarchy ────────────

#[test]
fn match_constructor_binds_fields_per_variant() {
    let src = "\
sealed class Shape:
    fn __init__(self) -> None:
        pass

final class Circle(Shape):
    r: f64

    fn __init__(self, r: f64) -> None:
        self.r = r

final class Square(Shape):
    s: f64

    fn __init__(self, s: f64) -> None:
        self.s = s

fn describe(sh: Shape) -> str:
    match sh:
        case Circle(r):
            return \"circle r=\" + str(r)
        case Square(s):
            return \"square s=\" + str(s)
        case _:
            return \"unknown\"
    return \"unreachable\"

fn main() -> i32:
    println(describe(Circle(2.5)))
    println(describe(Square(4.0)))
    return 0
";
    let p = compile_snippet("match_constructor", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("circle r=2.5\n"), "got: {out:?}");
    assert!(out.contains("square s=4"), "got: {out:?}");
}

// ── (6) tuple pattern ──────────────────────────────────────────────────

#[test]
fn match_tuple_pattern_destructures() {
    let src = "\
fn classify(p: (i32, i32)) -> str:
    match p:
        case (a, b):
            return \"sum=\" + str(a + b)
    return \"none\"

fn main() -> i32:
    println(classify((3, 4)))
    return 0
";
    let p = compile_snippet("match_tuple", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("sum=7\n"), "got: {out:?}");
}

// ── (7) terminal wildcard ───────────────────────────────────────────────

#[test]
fn match_wildcard_catches_residual() {
    let src = "\
sealed class Color:
    fn __init__(self) -> None:
        pass

final class Red(Color):
    fn __init__(self) -> None:
        pass

final class Green(Color):
    fn __init__(self) -> None:
        pass

final class Blue(Color):
    fn __init__(self) -> None:
        pass

fn name(c: Color) -> str:
    match c:
        case Red():
            return \"red\"
        case _:
            return \"other\"
    return \"unreachable\"

fn main() -> i32:
    println(name(Red()))
    println(name(Green()))
    println(name(Blue()))
    return 0
";
    let p = compile_snippet("match_wildcard", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("red\n"), "got: {out:?}");
    // Green + Blue both fall into `_`.
    let other_count = out.matches("other\n").count();
    assert_eq!(other_count, 2, "expected 2 'other' lines, got: {out:?}");
}

// ── (8) scrutinee evaluated exactly once ────────────────────────────────

#[test]
fn match_evaluates_scrutinee_once() {
    // A side-effecting function increments a counter on every call. After
    // the match, the counter must read exactly 1 even though the scrutinee
    // is conceptually referenced in every case test.
    let src = "\
sealed class Tag:
    fn __init__(self) -> None:
        pass

final class A(Tag):
    fn __init__(self) -> None:
        pass

final class B(Tag):
    fn __init__(self) -> None:
        pass

final class Counter:
    n: i32

    fn __init__(self) -> None:
        self.n = 0i32

    fn bump_and_get_a(self) -> Tag:
        self.n = self.n + 1i32
        return A()

fn main() -> i32:
    c: Counter = Counter()
    match c.bump_and_get_a():
        case A():
            println(\"hit A\")
        case B():
            println(\"hit B (BAD)\")
        case _:
            println(\"hit wildcard (BAD)\")
    println(\"calls=\" + str(c.n))
    return 0
";
    let p = compile_snippet("match_scrut_once", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("hit A\n"), "got: {out:?}");
    assert!(out.contains("calls=1\n"), "scrutinee evaluated more than once: {out:?}");
    assert!(!out.contains("BAD"), "got: {out:?}");
}

// ── (9) non-exhaustive sealed match still typechecks (warning-only) ────

#[test]
fn match_non_exhaustive_sealed_compiles() {
    // 3 sealed subclasses but only 2 listed and no wildcard — must still
    // compile (warning to stderr, not an error). Runtime sees the
    // unmatched case fall through to the post-match exit; the function
    // returns its default.
    let src = "\
sealed class T:
    fn __init__(self) -> None:
        pass

final class T1(T):
    fn __init__(self) -> None:
        pass

final class T2(T):
    fn __init__(self) -> None:
        pass

final class T3(T):
    fn __init__(self) -> None:
        pass

fn label(t: T) -> str:
    out: str = \"default\"
    match t:
        case T1():
            out = \"one\"
        case T2():
            out = \"two\"
    return out

fn main() -> i32:
    println(label(T1()))
    println(label(T2()))
    println(label(T3()))
    return 0
";
    let p = compile_snippet("match_non_exhaustive", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("one\n"), "got: {out:?}");
    assert!(out.contains("two\n"), "got: {out:?}");
    assert!(out.contains("default\n"), "unmatched T3 should hit default: {out:?}");
}
