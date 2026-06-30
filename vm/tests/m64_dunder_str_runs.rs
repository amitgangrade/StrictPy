//! Wave-2 Lane A regression tests: `str(obj)` / `print(obj)` on a user
//! class call its `__str__`, falling back to `__repr__`, and — when the
//! class defines neither — a synthesised default `ClassName(field=value, …)`
//! repr.
//!
//! Before this lane, both `str(obj)` and `print(obj)` lowered the instance
//! to `NativeFn::StrFromAny`, which reinterpreted the ObjectHeader *pointer*
//! as a StringRepr (or, failing the heap-pointer probe, as an i64) and
//! printed garbage. The deferral lived in `BUGS_KNOWN.md`
//! ("StrFromAny garbage for class").
//!
//! Coverage:
//!   1. `__str__` is used by both `str()` and `print()`.
//!   2. `__repr__` is the fallback when `__str__` is absent.
//!   3. Neither dunder → default field repr (`ClassName(f=v, …)`).
//!   4. Default repr recurses through nested fields (str / class / tuple).
//!   5. Virtual dispatch: an overridden `__str__` wins through a base-typed
//!      receiver; an inherited (non-overridden) `__str__` resolves to the
//!      parent's body.
//!   6. A field-less class with no dunder renders as `ClassName()`.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn run(test_name: &str, src: &str) -> (i32, String) {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m64_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    run_file_capture(&out).expect("run")
}

// ── (1) __str__ used by str() and print() ───────────────────────────────

#[test]
fn str_and_print_use_dunder_str() {
    let src = "\
final class Point:
    x: i64
    y: i64

    fn __init__(self, x: i64, y: i64) -> None:
        self.x = x
        self.y = y

    fn __str__(self) -> str:
        return \"Point(\" + str(self.x) + \", \" + str(self.y) + \")\"

fn main() -> i32:
    p: Point = Point(3, 4)
    println(p)
    println(str(p))
    return 0
";
    let (code, out) = run("dunder_str", src);
    assert_eq!(code, 0, "out: {out:?}");
    assert_eq!(out, "Point(3, 4)\nPoint(3, 4)\n", "out: {out:?}");
}

// ── (2) __repr__ fallback ───────────────────────────────────────────────

#[test]
fn str_falls_back_to_dunder_repr() {
    let src = "\
final class Money:
    cents: i64

    fn __init__(self, cents: i64) -> None:
        self.cents = cents

    fn __repr__(self) -> str:
        return \"Money<\" + str(self.cents) + \"c>\"

fn main() -> i32:
    m: Money = Money(1099)
    println(m)
    println(str(m))
    return 0
";
    let (code, out) = run("dunder_repr", src);
    assert_eq!(code, 0, "out: {out:?}");
    assert_eq!(out, "Money<1099c>\nMoney<1099c>\n", "out: {out:?}");
}

// `__str__` is preferred over `__repr__` when both exist.
#[test]
fn dunder_str_preferred_over_repr() {
    let src = "\
final class Both:
    n: i64

    fn __init__(self, n: i64) -> None:
        self.n = n

    fn __str__(self) -> str:
        return \"STR:\" + str(self.n)

    fn __repr__(self) -> str:
        return \"REPR:\" + str(self.n)

fn main() -> i32:
    b: Both = Both(7)
    println(b)
    println(str(b))
    return 0
";
    let (code, out) = run("dunder_both", src);
    assert_eq!(code, 0, "out: {out:?}");
    assert_eq!(out, "STR:7\nSTR:7\n", "out: {out:?}");
}

// ── (3) default field repr — neither dunder defined ─────────────────────

#[test]
fn default_repr_when_no_dunder() {
    let src = "\
final class Color:
    r: i64
    g: i64
    b: i64

    fn __init__(self, r: i64, g: i64, b: i64) -> None:
        self.r = r
        self.g = g
        self.b = b

fn main() -> i32:
    c: Color = Color(255, 128, 0)
    println(c)
    println(str(c))
    return 0
";
    let (code, out) = run("default_repr", src);
    assert_eq!(code, 0, "out: {out:?}");
    assert_eq!(
        out, "Color(r=255, g=128, b=0)\nColor(r=255, g=128, b=0)\n",
        "out: {out:?}"
    );
}

// ── (4) default repr recurses through str / nested fields ───────────────

#[test]
fn default_repr_recurses_through_fields() {
    let src = "\
final class Inner:
    tag: str

    fn __init__(self, tag: str) -> None:
        self.tag = tag

    fn __str__(self) -> str:
        return \"<\" + self.tag + \">\"

final class Outer:
    label: str
    count: i64
    flag: bool
    inner: Inner

    fn __init__(self, label: str, count: i64, flag: bool, inner: Inner) -> None:
        self.label = label
        self.count = count
        self.flag = flag
        self.inner = inner

fn main() -> i32:
    o: Outer = Outer(\"hi\", 5, true, Inner(\"deep\"))
    println(o)
    return 0
";
    let (code, out) = run("default_repr_nested", src);
    assert_eq!(code, 0, "out: {out:?}");
    // `str` field renders raw; bool via StrFromBool; nested class via its
    // own __str__.
    assert_eq!(
        out, "Outer(label=hi, count=5, flag=true, inner=<deep>)\n",
        "out: {out:?}"
    );
}

// ── (5) virtual dispatch through a base-typed receiver ──────────────────

#[test]
fn dunder_str_dispatches_virtually() {
    let src = "\
open class Animal:
    name: str

    fn __init__(self, name: str) -> None:
        self.name = name

    open fn __str__(self) -> str:
        return \"Animal:\" + self.name

final class Dog(Animal):
    fn __init__(self, name: str) -> None:
        self.name = name

    fn __str__(self) -> str:
        return \"Dog:\" + self.name

final class Cat(Animal):
    fn __init__(self, name: str) -> None:
        self.name = name

fn describe(a: Animal) -> None:
    println(a)

fn main() -> i32:
    d: Dog = Dog(\"Rex\")
    c: Cat = Cat(\"Felix\")
    println(d)
    println(c)
    describe(d)
    describe(c)
    return 0
";
    let (code, out) = run("dunder_virtual", src);
    assert_eq!(code, 0, "out: {out:?}");
    // d overrides; c inherits Animal's __str__. describe() takes a base-typed
    // receiver, so the override must still win at runtime (vtable dispatch).
    assert_eq!(
        out, "Dog:Rex\nAnimal:Felix\nDog:Rex\nAnimal:Felix\n",
        "out: {out:?}"
    );
}

// ── (6) field-less class with no dunder → ClassName() ───────────────────

#[test]
fn default_repr_empty_fields() {
    let src = "\
final class Unit:
    fn __init__(self) -> None:
        pass

fn main() -> i32:
    u: Unit = Unit()
    println(u)
    println(str(u))
    return 0
";
    let (code, out) = run("default_repr_empty", src);
    assert_eq!(code, 0, "out: {out:?}");
    assert_eq!(out, "Unit()\nUnit()\n", "out: {out:?}");
}
