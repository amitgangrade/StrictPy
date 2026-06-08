//! Regression: a unary-negated integer literal coerces to the expected width.
//!
//! `5` (a bare int literal) takes on the expected width in a typed context, but
//! `-1` is `Unary(Neg, Int)` — not a bare literal — so it previously missed the
//! literal-coercion path, defaulted to i32, and was rejected against an i64
//! context (`x: i64 = -1`, `range(a, b, -1)`, `f(-1)`). Fixed in typecheck by
//! pushing the expected type into a numeric-literal operand of unary `+`/`-`.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn run(name: &str, src: &str) -> (i32, String) {
    let bytes = compile_source(format!("{name}.spy"), src)
        .unwrap_or_else(|e| panic!("{name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_neglit_{name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    run_file_capture(&out).expect("run")
}

#[test]
fn negative_int_literal_coerces_to_i64() {
    let src = "\
fn takes(a: i64, b: i64) -> i64:
    return a + b

fn main() -> i32:
    x: i64 = -1
    println(str(x))                       # -1
    y: i64 = -1000000000000              # would overflow i32 -> must be i64
    println(str(y))
    println(str(takes(-3, -4)))           # -7  (negative literals as i64 args)
    return 0
";
    let (code, out) = run("neg_i64", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "-1\n-1000000000000\n-7\n", "{out:?}");
}

#[test]
fn negated_variable_keeps_its_type() {
    // Only *literal* operands are coerced; `-v` keeps v's real type.
    let src = "\
fn main() -> i32:
    v: i64 = 10
    println(str(-v))                      # -10
    f: f64 = -2.5
    println(str(f))                       # -2.5 (negative float literal)
    return 0
";
    let (code, out) = run("neg_var", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "-10\n-2.5\n", "{out:?}");
}
