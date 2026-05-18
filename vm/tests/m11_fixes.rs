//! M11 regression tests for the class/vtable subsystem cleanup and the
//! primitive-constructor dispatch bug. Each test compiles a minimal
//! repro of a bug surfaced by the M11 stress-test round, runs it
//! through `run_file_capture`, and asserts on the stdout that proves
//! the fix.
//!
//! Bug index (cross-reference: BUGS_KNOWN.md):
//!
//!   BUG-015 — sealed-class virtual dispatch dropped to the base
//!             method. Fixed in `compiler/src/ir.rs::lower_method_call`
//!             by adding `!is_sealed` to the devirtualisation guard.
//!
//!   BUG-016 — subclass field offsets aliased the parent's fields
//!             because `layout_class` restarted the offset cursor at 0.
//!             Fixed in `compiler/src/resolver.rs::layout_class` by
//!             seeding the cursor from `parent.payload_size` and
//!             inheriting the parent's fields verbatim.
//!
//!   BUG-017 / N1 — vtable lookups appeared capped at 4 slots and
//!             inherited methods on a subclass that didn't override
//!             them resolved to `u32::MAX`. Fixed in
//!             `compiler/src/ir.rs::collect_types` by walking up the
//!             inheritance chain when looking up `Class.method` fn ids,
//!             and in `resolver::layout_class` by carrying the parent's
//!             method list into each subclass.
//!
//!   N2     — heap corruption on a subclass with class-reference fields
//!             plus a virtual call. Was a symptom of BUG-016 (the
//!             "vtable" pointer read during dispatch landed on a stale
//!             car field). Fixing BUG-016 fixed N2 — this file's
//!             `pair_with_class_ref_fields_dispatches_through_vtable`
//!             test is the deterministic repro.
//!
//!   PRIM-CTOR — `i32(j: i64)` always returned 0 because every numeric
//!             ctor fell through to `NativeFn::from_name(name)`, which
//!             returned `I32FromF64` regardless of the arg type. Fixed
//!             in `compiler/src/ir.rs::lower_call` by mirroring the
//!             per-arg-type dispatch that already existed for `str(x)`.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m11_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── BUG-016: subclass field offsets must not alias parent fields ───────

#[test]
fn subclass_field_offsets_do_not_alias_parent_fields() {
    let src = "\
open class Base:
    kind: i32 = 0

final class Sub(Base):
    n: i32

    fn __init__(self, n: i32) -> None:
        self.kind = 200
        self.n = n

fn main() -> i32:
    s: Sub = Sub(42)
    println(\"kind=\" + str(s.kind))
    println(\"n=\" + str(s.n))
    return 0
";
    let p = compile_snippet("bug016_subclass_fields", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(
        out.contains("kind=200\n"),
        "parent field clobbered by subclass field; got: {out:?}",
    );
    assert!(
        out.contains("n=42\n"),
        "subclass field clobbered by parent field; got: {out:?}",
    );
}

#[test]
fn subclass_with_three_inherited_fields_does_not_alias() {
    // Multiple parent fields of mixed widths — exercises the natural
    // alignment that `payload_size` accounts for.
    let src = "\
open class P:
    a: i32 = 0
    b: i64 = 0
    c: bool = false

final class C(P):
    x: i32
    y: i64

    fn __init__(self, x: i32, y: i64) -> None:
        self.a = 11
        self.b = 22
        self.c = true
        self.x = x
        self.y = y

fn main() -> i32:
    o: C = C(33, 44)
    println(\"a=\" + str(o.a))
    println(\"b=\" + str(o.b))
    println(\"x=\" + str(o.x))
    println(\"y=\" + str(o.y))
    return 0
";
    let p = compile_snippet("bug016_mixed_widths", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(out.contains("a=11\n"), "got: {out:?}");
    assert!(out.contains("b=22\n"), "got: {out:?}");
    assert!(out.contains("x=33\n"), "got: {out:?}");
    assert!(out.contains("y=44\n"), "got: {out:?}");
}

// ── BUG-017 / N1: vtable supports more than 4 virtual methods ────────

#[test]
fn vtable_supports_six_virtual_methods_with_override() {
    let src = "\
open class Value:
    fn m0(self) -> i32: return 0
    fn m1(self) -> i32: return 1
    fn m2(self) -> i32: return 2
    fn m3(self) -> i32: return 3
    fn m4(self) -> i32: return 4
    fn m5(self) -> i32: return 5

final class A(Value):
    fn m0(self) -> i32: return 100
    fn m1(self) -> i32: return 101
    fn m2(self) -> i32: return 102
    fn m3(self) -> i32: return 103
    fn m4(self) -> i32: return 104
    fn m5(self) -> i32: return 105

fn main() -> i32:
    v: Value = A()
    println(str(v.m0()))
    println(str(v.m1()))
    println(str(v.m2()))
    println(str(v.m3()))
    println(str(v.m4()))
    println(str(v.m5()))
    return 0
";
    let p = compile_snippet("bugN1_six_vtable_slots", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    // All six overrides must reach the subclass body.
    for v in &[100, 101, 102, 103, 104, 105] {
        let needle = format!("{v}\n");
        assert!(out.contains(&needle), "missing {needle:?} in: {out:?}");
    }
}

#[test]
fn subclass_can_inherit_method_without_override() {
    // Before BUG-017's fix, an inherited slot was filled with u32::MAX
    // and dispatch trapped on the first inherited call.
    let src = "\
open class P:
    fn name(self) -> str: return \"P\"
    fn tag(self) -> i32: return 1

final class A(P):
    fn name(self) -> str: return \"A\"
    # `tag` deliberately NOT overridden — must inherit P.tag

fn main() -> i32:
    p: P = A()
    println(p.name())     # \"A\"
    println(str(p.tag())) # 1 (inherited)
    return 0
";
    let p = compile_snippet("bugN1_inherit_unoverridden", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(out.contains("A\n"), "got: {out:?}");
    assert!(out.contains("1\n"), "got: {out:?}");
}

// ── N2: subclass with class-ref fields dispatches correctly ──────────

#[test]
fn pair_with_class_ref_fields_dispatches_through_vtable() {
    // This used to crash with STATUS_HEAP_CORRUPTION on the first
    // p.tag() call — the runtime read what it thought was the vtable
    // pointer but actually got Pair.car's heap address (because car
    // aliased the parent's vtable slot at offset 0). With BUG-016
    // fixed, the dispatch finds the real vtable and reaches Pair.tag.
    let src = "\
open class Value:
    fn tag(self) -> i32: return 0

final class Number(Value):
    n: i64
    fn __init__(self, n: i64) -> None: self.n = n
    fn tag(self) -> i32: return 1

final class Pair(Value):
    car: Value
    cdr: Value?
    fn __init__(self, c: Value, cd: Value?) -> None:
        self.car = c
        self.cdr = cd
    fn tag(self) -> i32: return 3

fn main() -> i32:
    n: Value = Number(10)
    p: Value = Pair(n, none)
    println(str(p.tag()))    # 3
    println(str(n.tag()))    # 1
    return 0
";
    let p = compile_snippet("bugN2_pair_class_ref_fields", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(out.contains("3\n"), "Pair.tag did not reach subclass; got: {out:?}");
    assert!(out.contains("1\n"), "Number.tag did not reach subclass; got: {out:?}");
}

// ── BUG-015: sealed-class virtual dispatch ────────────────────────────

#[test]
fn sealed_base_dispatches_to_subclass_override() {
    let src = "\
sealed class SealedBase:
    fn name(self) -> str: return \"BASE\"

final class SealedSub(SealedBase):
    fn name(self) -> str: return \"SUB\"

fn main() -> i32:
    b: SealedBase = SealedSub()
    println(b.name())
    return 0
";
    let p = compile_snippet("bug015_sealed_dispatch", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(
        out.contains("SUB\n"),
        "sealed receiver dispatched to base instead of the subclass override; got: {out:?}",
    );
    assert!(
        !out.contains("BASE\n"),
        "base method must NOT run when receiver is a SealedSub; got: {out:?}",
    );
}

// ── Primitive ctor dispatch ──────────────────────────────────────────

#[test]
fn i32_of_i64_truncates_value() {
    let src = "\
fn main() -> i32:
    j: i64 = 3
    k: i32 = i32(j)
    println(\"k=\" + str(k))
    return 0
";
    let p = compile_snippet("primctor_i32_from_i64", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(
        out.contains("k=3\n"),
        "i32(i64) used to reinterpret the bit pattern as f64 → 0; got: {out:?}",
    );
}

#[test]
fn i64_of_i32_widens_value() {
    let src = "\
fn main() -> i32:
    j: i32 = 42
    k: i64 = i64(j)
    println(\"k=\" + str(k))
    return 0
";
    let p = compile_snippet("primctor_i64_from_i32", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(out.contains("k=42\n"), "got: {out:?}");
}

#[test]
fn f64_of_i64_widens_value() {
    let src = "\
fn main() -> i32:
    j: i64 = 7
    f: f64 = f64(j)
    println(\"f=\" + str(f))
    return 0
";
    let p = compile_snippet("primctor_f64_from_i64", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(
        out.contains("f=7.0\n"),
        "f64(i64) must produce a real f64 with one trailing decimal; got: {out:?}",
    );
}

#[test]
fn i64_of_f64_truncates_toward_zero() {
    let src = "\
fn main() -> i32:
    f: f64 = 3.9
    k: i64 = i64(f)
    println(\"k=\" + str(k))
    return 0
";
    let p = compile_snippet("primctor_i64_from_f64", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(
        out.contains("k=3\n"),
        "i64(f64) must truncate toward zero; got: {out:?}",
    );
}

// ── str(f64) consistency ─────────────────────────────────────────────

#[test]
fn str_of_integer_valued_float_keeps_decimal() {
    let src = "\
fn main() -> i32:
    println(str(3.0))
    println(str(3.14))
    return 0
";
    let p = compile_snippet("str_f64_integer", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(
        out.contains("3.0\n"),
        "str(3.0) must keep one trailing decimal so it round-trips as a float; got: {out:?}",
    );
    assert!(out.contains("3.14\n"), "got: {out:?}");
}

// ── Natural class hierarchy sanity ───────────────────────────────────

// ── M11 example smoke tests (do they compile + parse cleanly?) ──────
//
// These programs were originally written by the M11 stress-test agents
// and either crashed at runtime or worked around the bugs we just fixed.
// At minimum, they should compile cleanly post-M11. Running them is
// out-of-scope for this fix round because some still hit BUG-026/027
// (non-deterministic heap corruption) which is a separate dedicated
// debugging task.
#[test]
fn m11_examples_compile_cleanly() {
    use strictpy_compiler::compile_source;
    let dir = {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop();
        p.push("examples");
        p
    };
    for name in &[
        "lambda_calc.spy",
        "calculator.spy",
        "tictactoe.spy",
        "levenshtein.spy",
        "lisp.spy",
    ] {
        let src_path = dir.join(name);
        let src = std::fs::read_to_string(&src_path).expect("read example");
        let bytes = compile_source(src_path.display().to_string(), &src)
            .unwrap_or_else(|e| panic!("compile {name}: {e}"));
        assert!(!bytes.is_empty(), "{name} compiled to zero bytes");
    }
}

#[test]
fn natural_class_hierarchy_with_parent_fields_and_six_virtuals() {
    // Combined sanity: parent has fields, four subclasses each override
    // six virtual methods, and dispatch goes through a base-typed
    // receiver in every case.
    let src = "\
open class Shape:
    fn area(self) -> f64: return 0.0
    fn perim(self) -> f64: return 0.0
    fn name(self) -> str: return \"shape\"
    fn corners(self) -> i32: return 0
    fn is_round(self) -> bool: return false
    fn render(self) -> str: return \"<shape>\"

final class Square(Shape):
    side: f64

    fn __init__(self, side: f64) -> None:
        self.side = side

    fn area(self) -> f64: return self.side * self.side
    fn perim(self) -> f64: return self.side * 4.0
    fn name(self) -> str: return \"square\"
    fn corners(self) -> i32: return 4
    fn is_round(self) -> bool: return false
    fn render(self) -> str: return \"<square>\"

final class Circle(Shape):
    radius: f64

    fn __init__(self, r: f64) -> None:
        self.radius = r

    fn area(self) -> f64: return self.radius * self.radius * 3.14
    fn perim(self) -> f64: return self.radius * 2.0 * 3.14
    fn name(self) -> str: return \"circle\"
    fn corners(self) -> i32: return 0
    fn is_round(self) -> bool: return true
    fn render(self) -> str: return \"<circle>\"

final class Triangle(Shape):
    base_len: f64
    height: f64

    fn __init__(self, b: f64, h: f64) -> None:
        self.base_len = b
        self.height = h

    fn area(self) -> f64: return self.base_len * self.height * 0.5
    fn perim(self) -> f64: return self.base_len * 3.0
    fn name(self) -> str: return \"triangle\"
    fn corners(self) -> i32: return 3
    fn is_round(self) -> bool: return false
    fn render(self) -> str: return \"<triangle>\"

final class Pentagon(Shape):
    side: f64

    fn __init__(self, s: f64) -> None:
        self.side = s

    fn area(self) -> f64: return self.side * self.side * 1.72
    fn perim(self) -> f64: return self.side * 5.0
    fn name(self) -> str: return \"pentagon\"
    fn corners(self) -> i32: return 5
    fn is_round(self) -> bool: return false
    fn render(self) -> str: return \"<pentagon>\"

fn show(s: Shape) -> None:
    println(s.name() + \" / corners=\" + str(s.corners()) + \" / area=\" + str(s.area()))

fn main() -> i32:
    a: Shape = Square(4.0)
    b: Shape = Circle(2.0)
    c: Shape = Triangle(3.0, 4.0)
    d: Shape = Pentagon(1.0)
    show(a)
    show(b)
    show(c)
    show(d)
    return 0
";
    let p = compile_snippet("natural_hierarchy", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(out.contains("square / corners=4 / area=16"),
            "got: {out:?}");
    assert!(out.contains("circle / corners=0"), "got: {out:?}");
    assert!(out.contains("triangle / corners=3"), "got: {out:?}");
    assert!(out.contains("pentagon / corners=5"), "got: {out:?}");
}
