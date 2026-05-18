//! M14 regression tests for tuples — type annotation, literals, field
//! access (`t.0` / `t.1` / ...), destructuring let, tuple return,
//! `str(tuple)` formatting, element-wise `==` / `!=`, and tuples with
//! class-ref elements.
//!
//! Before M14 every program that wanted to "return two things" had to
//! fall back to either a fresh single-purpose `final class` (over a
//! dozen of these scattered across the M10–M12 stress programs) or a
//! 1-element mutable `List[i64]` used as an out-parameter (the
//! `cursor: List[i64] = [0]` idiom in csv_aggregate / markov / json /
//! ...). The repro programs below all exercise the new tuple surface,
//! which makes both workarounds unnecessary.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m14_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── (1) 2-tuple literal + element access ───────────────────────────────

#[test]
fn two_tuple_literal_and_field_access() {
    let src = "\
fn main() -> i32:
    t: Tuple[i32, i32] = (3i32, 4i32)
    println(str(t.0) + \" \" + str(t.1))
    return 0
";
    let p = compile_snippet("two_tuple_literal", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit: {out:?}");
    assert!(out.contains("3 4\n"), "got: {out:?}");
}

// ── (2) 3-tuple with mixed types ────────────────────────────────────────

#[test]
fn three_tuple_with_mixed_element_types() {
    // Mixed primitive widths exercise the uniform 8-byte-per-slot layout
    // — i32 → 4 bytes loaded with TypeTag::I32, str → 8-byte pointer,
    // bool → 1 byte with TypeTag::Bool.
    let src = "\
fn main() -> i32:
    t: Tuple[i32, str, bool] = (1i32, \"two\", true)
    println(str(t.0))
    println(t.1)
    println(str(t.2))
    return 0
";
    let p = compile_snippet("three_tuple_mixed", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit: {out:?}");
    assert!(out.contains("1\n"), "got: {out:?}");
    assert!(out.contains("two\n"), "got: {out:?}");
    assert!(out.contains("true\n"), "got: {out:?}");
}

// ── (3) Tuple return type, then field access on the result ─────────────

#[test]
fn tuple_return_type_and_caller_field_access() {
    // The whole point: a function can naturally return two values
    // without an `out: List[T] = [...]` workaround or a one-off Pair
    // class. The caller binds the tuple and reads `.0` / `.1`.
    let src = "\
fn p() -> Tuple[i32, str]:
    return (42i32, \"hi\")

fn main() -> i32:
    t: Tuple[i32, str] = p()
    println(t.1)
    return 0
";
    let p = compile_snippet("tuple_return", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit: {out:?}");
    assert!(out.contains("hi\n"), "got: {out:?}");
}

// ── (4) Annotated destructuring let ────────────────────────────────────

#[test]
fn annotated_destructuring_let_binds_each_name() {
    let src = "\
fn p() -> Tuple[i32, str]:
    return (42i32, \"hi\")

fn main() -> i32:
    x: i32, y: str = p()
    println(str(x))
    println(y)
    return 0
";
    let p = compile_snippet("destructure_annotated", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit: {out:?}");
    assert!(out.contains("42\n"), "got: {out:?}");
    assert!(out.contains("hi\n"), "got: {out:?}");
}

// ── (5) Inferred destructuring let (no per-name annotations) ───────────

#[test]
fn inferred_destructuring_let_infers_from_rhs() {
    let src = "\
fn p() -> Tuple[i32, str]:
    return (42i32, \"hi\")

fn main() -> i32:
    x, y = p()
    println(y)
    return 0
";
    let p = compile_snippet("destructure_inferred", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit: {out:?}");
    assert!(out.contains("hi\n"), "got: {out:?}");
}

// ── (6) Tuple equality — `==` and `!=` element-wise ────────────────────

#[test]
fn tuple_equality_elementwise_eq_and_ne() {
    // Lower `t == u` to per-element compares ANDed together; `!=` is the
    // bitwise-not of that. Mirrors the M12 BUG-034 pattern for strings.
    let src = "\
fn main() -> i32:
    a: Tuple[i32, i32] = (1i32, 2i32)
    b: Tuple[i32, i32] = (1i32, 2i32)
    c: Tuple[i32, i32] = (1i32, 3i32)
    println(\"eq=\" + str(a == b))
    println(\"neq=\" + str(a == c))
    println(\"ne_true=\" + str(a != c))
    println(\"ne_false=\" + str(a != b))
    return 0
";
    let p = compile_snippet("tuple_equality", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit: {out:?}");
    assert!(out.contains("eq=true\n"), "got: {out:?}");
    assert!(out.contains("neq=false\n"), "got: {out:?}");
    assert!(out.contains("ne_true=true\n"), "got: {out:?}");
    assert!(out.contains("ne_false=false\n"), "got: {out:?}");
}

// ── (7) str(tuple) — spec-defined `"(a, b, ...)"` format ───────────────

#[test]
fn str_of_tuple_formats_python_ish() {
    // Bare-element format: no quotes on string elements, comma+space
    // separator, parens around the whole thing. See STRICTPY_SPEC §5.X.
    let src = "\
fn main() -> i32:
    t: Tuple[i32, str] = (1i32, \"x\")
    println(str(t))
    return 0
";
    let p = compile_snippet("str_of_tuple", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit: {out:?}");
    assert!(out.contains("(1, x)\n"), "got: {out:?}");
}

// ── (8) Tuple with a class-ref element ─────────────────────────────────

#[test]
fn tuple_with_class_ref_element_can_be_dereferenced() {
    // A class instance stored in a tuple slot must round-trip through
    // the heap correctly — field access `t.0.n` walks first into the
    // tuple object then into the Foo instance.
    let src = "\
final class Foo:
    n: i32

    fn __init__(self, n: i32) -> None:
        self.n = n

fn main() -> i32:
    t: Tuple[Foo, i32] = (Foo(5i32), 7i32)
    println(str(t.0.n))
    println(str(t.1))
    return 0
";
    let p = compile_snippet("tuple_class_ref", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit: {out:?}");
    assert!(out.contains("5\n"), "got: {out:?}");
    assert!(out.contains("7\n"), "got: {out:?}");
}
