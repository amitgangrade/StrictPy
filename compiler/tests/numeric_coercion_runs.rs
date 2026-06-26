//! Lane A (numeric core) end-to-end tests.
//!
//! Compiles small StrictPy snippets (and the `numeric_coercion_demo.spy`
//! example) with the real compiler, runs the resulting bytecode in-process
//! through the VM, and asserts the printed output. Covers:
//!   * `/` is true division -> f64
//!   * `//` is integer (truncating) division
//!   * implicit numeric widening (i32->i64->f64) in mixed binops
//!   * bare integer literals default to i64

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

fn run(name: &str, src: &str) -> (i32, String) {
    let bytes = compile_source(format!("{name}.spy"), src)
        .unwrap_or_else(|e| panic!("{name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_numeric_{name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    run_file_capture(&out).unwrap_or_else(|e| panic!("{name}: run error: {e:?}"))
}

#[test]
fn true_division_yields_float() {
    let src = "\
fn main() -> i32:
    a: i64 = 7
    b: i64 = 2
    q: f64 = a / b
    println(str(q))
    return 0
";
    let (code, out) = run("true_div", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "3.5\n", "{out:?}");
}

#[test]
fn floor_division_keeps_integer() {
    let src = "\
fn main() -> i32:
    a: i64 = 7
    b: i64 = 2
    q: i64 = a // b
    println(str(q))
    r: i64 = -7 // 2
    println(str(r))
    return 0
";
    let (code, out) = run("floor_div", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    // `//` truncates toward zero per spec §7.2 (so -7 // 2 == -3, not -4).
    assert_eq!(out, "3\n-3\n", "{out:?}");
}

#[test]
fn mixed_int_width_widens_to_i64() {
    let src = "\
fn main() -> i32:
    small: i32 = 1000i32
    big: i64 = 2000000000000
    s: i64 = small + big
    println(str(s))
    return 0
";
    let (code, out) = run("widen_i32_i64", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "2000000001000\n", "{out:?}");
}

#[test]
fn int_plus_float_widens_to_f64() {
    let src = "\
fn main() -> i32:
    n: i64 = 10
    h: f64 = 0.5
    x: f64 = n + h
    println(str(x))
    c: i32 = 3i32
    p: f64 = 1.5
    y: f64 = c * p
    println(str(y))
    return 0
";
    let (code, out) = run("widen_int_float", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "10.5\n4.5\n", "{out:?}");
}

#[test]
fn bare_literal_defaults_to_i64() {
    // 1_000_000 * 1_000_000 = 1e12 overflows i32 but fits i64. With the
    // i64-default literal width this is computed exactly.
    let src = "\
fn main() -> i32:
    w: i64 = 1000000 * 1000000
    println(str(w))
    return 0
";
    let (code, out) = run("i64_default", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "1000000000000\n", "{out:?}");
}

#[test]
fn slash_equals_requires_float_target() {
    // `/=` is true division, so an integer target is a compile error
    // (the result would be f64). `//=` is the integer form.
    let src = "\
fn main() -> i32:
    x: i64 = 10
    x /= 2
    return 0
";
    let err = compile_source("slash_eq_int.spy".to_string(), src)
        .expect_err("`/=` on an i64 target must be a type error");
    let msg = format!("{err}");
    assert!(
        msg.contains("true (float) division") || msg.contains("//="),
        "unexpected error: {msg}"
    );
}

#[test]
fn numeric_coercion_demo_runs() {
    let src_path = project_root().join("examples").join("numeric_coercion_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read numeric_coercion_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile numeric_coercion_demo.spy: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push("strictpy_numeric_coercion_demo.spyc");
    fs::write(&out, &bytes).expect("write spyc");
    let (code, stdout) = run_file_capture(&out).expect("run numeric_coercion_demo");
    assert_eq!(code, 0, "stdout: {stdout:?}");
    assert!(stdout.contains("7 / 2 = 3.5"), "stdout:\n{stdout}");
    assert!(stdout.contains("7 // 2 = 3"), "stdout:\n{stdout}");
    assert!(stdout.contains("i32 + i64 = 2000000001000"), "stdout:\n{stdout}");
    assert!(stdout.contains("10 + 0.5 = 10.5"), "stdout:\n{stdout}");
    assert!(stdout.contains("3 * 1.5 = 4.5"), "stdout:\n{stdout}");
    assert!(stdout.contains("1e6 * 1e6 = 1000000000000"), "stdout:\n{stdout}");
}
