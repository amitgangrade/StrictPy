//! M61b regression tests: default argument values + keyword arguments.
//!
//! Surface added: parameters may declare trailing defaults
//! (`fn f(a: i32, b: i32 = 1)`), and call sites may pass arguments by
//! keyword (`f(a=1, b=2)`) after any positional arguments. The compiler
//! binds the call to the callee's parameters, fills omitted defaults, and
//! lowers the result into the unchanged positional call ABI — so this works
//! identically in the interpreter and the JIT.
//!
//! Coverage:
//!   1. default omitted vs. default overridden (free function).
//!   2. keyword arguments supplied out of positional order.
//!   3. a mix of positional + keyword arguments.
//!   4. a constructor with a defaulted field, omitted then supplied by keyword.
//!   5. a method with a default argument.
//!   6. a default expression that references a top-level `final` constant.
//!   NEGATIVE cases (compile errors):
//!     7. an unknown keyword argument (E2015).
//!     8. a missing required argument (E2017).
//!     9. a parameter bound twice (positionally + by keyword) (E2016).
//!    10. a positional argument after a keyword argument (E2018).
//!    11. a required parameter following a defaulted one in a decl (E2019).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn run(test_name: &str, src: &str) -> (i32, String) {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m61b_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    run_file_capture(&out).expect("run")
}

// ── (1) default omitted vs. default overridden ───────────────────────────

#[test]
fn default_omitted_and_overridden() {
    let src = "\
fn greet(name: str, greeting: str = \"hi\") -> str:
    return greeting + \", \" + name

fn main() -> i32:
    println(greet(\"alice\"))
    println(greet(\"bob\", \"hey\"))
    return 0
";
    let (code, out) = run("default_basic", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "hi, alice\nhey, bob\n", "{out:?}");
}

// ── (2) keyword arguments out of positional order ────────────────────────

#[test]
fn keyword_out_of_order() {
    let src = "\
fn box(width: i32, height: i32 = 1, label: str = \"box\") -> str:
    return label + \" \" + str(width) + \"x\" + str(height)

fn main() -> i32:
    println(box(3, label=\"grid\", height=4))
    return 0
";
    let (code, out) = run("kw_out_of_order", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "grid 3x4\n", "{out:?}");
}

// ── (3) mixed positional + keyword, default left to fill ─────────────────

#[test]
fn mixed_positional_and_keyword() {
    let src = "\
fn box(width: i32, height: i32 = 1, label: str = \"box\") -> str:
    return label + \" \" + str(width) + \"x\" + str(height)

fn main() -> i32:
    println(box(5, label=\"row\"))
    println(box(2))
    return 0
";
    let (code, out) = run("kw_mixed", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "row 5x1\nbox 2x1\n", "{out:?}");
}

// ── (4) constructor with a defaulted field ───────────────────────────────

#[test]
fn constructor_default_field() {
    let src = "\
final class Point:
    x: i32
    y: i32
    fn __init__(self, x: i32, y: i32 = 0) -> None:
        self.x = x
        self.y = y
    fn show(self) -> str:
        return \"(\" + str(self.x) + \", \" + str(self.y) + \")\"

fn main() -> i32:
    p: Point = Point(7)
    q: Point = Point(1, y=9)
    println(p.show())
    println(q.show())
    return 0
";
    let (code, out) = run("ctor_default", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "(7, 0)\n(1, 9)\n", "{out:?}");
}

// ── (5) method with a default argument ───────────────────────────────────

#[test]
fn method_default_and_keyword() {
    let src = "\
final class Counter:
    n: i32
    fn __init__(self, n: i32) -> None:
        self.n = n
    fn bump(self, by: i32 = 1) -> i32:
        self.n = self.n + by
        return self.n

fn main() -> i32:
    c: Counter = Counter(10)
    println(str(c.bump()))
    println(str(c.bump(5)))
    println(str(c.bump(by=100)))
    return 0
";
    let (code, out) = run("method_default", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "11\n16\n116\n", "{out:?}");
}

// ── (6) default expression referencing a top-level constant ──────────────

#[test]
fn default_uses_constant() {
    let src = "\
final BASE: i32 = 100

fn add(x: i32, y: i32 = BASE) -> i32:
    return x + y

fn main() -> i32:
    println(str(add(1)))
    println(str(add(1, 2)))
    return 0
";
    let (code, out) = run("default_const", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "101\n3\n", "{out:?}");
}

// ── (7) NEGATIVE: unknown keyword argument (E2015) ───────────────────────

#[test]
fn err_unknown_keyword() {
    let src = "\
fn greet(name: str, greeting: str = \"hi\") -> str:
    return greeting + name

fn main() -> i32:
    println(greet(\"a\", salutation=\"yo\"))
    return 0
";
    let e = compile_source("err_unknown_kw.spy".into(), src)
        .expect_err("expected a compile error for unknown keyword");
    let msg = format!("{e}");
    assert!(msg.contains("E2015"), "expected E2015, got: {msg}");
}

// ── (8) NEGATIVE: missing required argument (E2017) ──────────────────────

#[test]
fn err_missing_required() {
    let src = "\
fn box(width: i32, height: i32 = 1) -> i32:
    return width * height

fn main() -> i32:
    println(str(box(height=4)))
    return 0
";
    let e = compile_source("err_missing.spy".into(), src)
        .expect_err("expected a compile error for missing required arg");
    let msg = format!("{e}");
    assert!(msg.contains("E2017"), "expected E2017, got: {msg}");
}

// ── (9) NEGATIVE: parameter bound twice (E2016) ──────────────────────────

#[test]
fn err_duplicate_binding() {
    let src = "\
fn f(a: i32, b: i32 = 0) -> i32:
    return a + b

fn main() -> i32:
    println(str(f(1, a=2)))
    return 0
";
    let e = compile_source("err_dup.spy".into(), src)
        .expect_err("expected a compile error for duplicate binding");
    let msg = format!("{e}");
    assert!(msg.contains("E2016"), "expected E2016, got: {msg}");
}

// ── (10) NEGATIVE: positional after keyword (E2018) ──────────────────────

#[test]
fn err_positional_after_keyword() {
    let src = "\
fn f(a: i32, b: i32) -> i32:
    return a + b

fn main() -> i32:
    println(str(f(a=1, 2)))
    return 0
";
    // This may surface as a parse error or a binding error depending on the
    // grammar; either way it must NOT compile.
    let e = compile_source("err_posafterkw.spy".into(), src)
        .expect_err("expected a compile error for positional-after-keyword");
    let _ = e;
}

// ── (11) NEGATIVE: required param after defaulted in a decl (E2019) ───────

#[test]
fn err_default_order() {
    let src = "\
fn f(a: i32 = 1, b: i32) -> i32:
    return a + b

fn main() -> i32:
    return 0
";
    let e = compile_source("err_order.spy".into(), src)
        .expect_err("expected a compile error for bad default order");
    let msg = format!("{e}");
    assert!(msg.contains("E2019"), "expected E2019, got: {msg}");
}
