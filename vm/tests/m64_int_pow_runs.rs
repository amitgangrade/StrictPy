//! wave-2 Lane F: integer `**` (exponentiation) + out-of-range int literals.
//!
//! Two correctness fixes:
//!
//!   1. Integer `**` previously lowered to the `IMul` placeholder, so
//!      `2 ** 10` computed `2 * 10 == 20`. It now routes to the `IntPow`
//!      native (exponentiation by squaring) for integer bases and to
//!      `MathPow` (`f64::powf`) for float bases. Each program below is run
//!      through BOTH engines — the Cranelift JIT (with a decode-eligibility
//!      assertion so the JIT leg can't silently fall back to the interpreter)
//!      and the pure interpreter.
//!
//!   2. An integer literal that exceeds the i64 range used to truncate
//!      silently (`9223372036854775808` wrapped to `i64::MIN`). It is now a
//!      clean compile error (`E2073`); the negative-case test pins that.

use std::sync::{Arc, Mutex};

use strictpy_compiler::compile_source;
use strictpy_vm::interp::{Interpreter, SharedVm, Stdout};
use strictpy_vm::{builtins, loader};

struct Capture(Arc<Mutex<String>>);
impl Stdout for Capture {
    fn write_str(&mut self, s: &str) {
        self.0.lock().unwrap().push_str(s);
    }
}

fn run_engine(name: &str, src: &str, jit: bool) -> (i32, String) {
    let bytes = compile_source(format!("{name}.spy"), src)
        .unwrap_or_else(|e| panic!("{name}: compile error: {e}"));
    let module = loader::load(&bytes).expect("load");
    if jit {
        // The JIT leg is only meaningful if the functions actually compile;
        // a decode failure would silently fall back to the interpreter and
        // test the same path twice.
        for f in &module.functions {
            let nm = module
                .strings
                .get(f.name_idx as usize)
                .map(|s| s.as_str())
                .unwrap_or("?");
            strictpy_vm::decompile::decode_function(&module, f.code_offset, f.code_length)
                .unwrap_or_else(|e| panic!("{name}: fn {nm} not JIT-eligible: {e:?}"));
        }
    }
    let mut interp = if jit {
        Interpreter::new(module)
    } else {
        Interpreter::from_shared(SharedVm::new(module))
    };
    builtins::register(&mut interp);
    let buf = Arc::new(Mutex::new(String::new()));
    interp.set_stdout(Box::new(Capture(buf.clone())));
    let code = interp
        .run_main()
        .unwrap_or_else(|e| panic!("{name}: run error: {e}"));
    let out = buf.lock().unwrap().clone();
    (code, out)
}

fn check(name: &str, src: &str, expected: &str) {
    for jit in [true, false] {
        let engine = if jit { "jit" } else { "interp" };
        let (code, out) = run_engine(name, src, jit);
        assert_eq!(code, 0, "{name} [{engine}]: exit code, stdout: {out:?}");
        assert_eq!(out, expected, "{name} [{engine}]");
    }
}

#[test]
fn two_to_the_tenth_is_1024() {
    // The headline regression: `2 ** 10` was `2 * 10 == 20`.
    let src = "\
fn main() -> i32:
    p: i64 = 2 ** 10
    println(str(p))
    return 0
";
    check("pow_2_10", src, "1024\n");
}

#[test]
fn larger_power_fits_i64() {
    // 3 ** 19 == 1162261467; 2 ** 40 == 1099511627776 (both fit i64).
    let src = "\
fn main() -> i32:
    a: i64 = 3 ** 19
    println(str(a))
    b: i64 = 2 ** 40
    println(str(b))
    return 0
";
    check("pow_large", src, "1162261467\n1099511627776\n");
}

#[test]
fn pow_on_i32_operands_keeps_i32() {
    // 7 ** 4 == 2401 with i32 operands.
    let src = "\
fn main() -> i32:
    base: i32 = 7i32
    exp: i32 = 4i32
    r: i32 = base ** exp
    println(str(r))
    return 0
";
    check("pow_i32", src, "2401\n");
}

#[test]
fn pow_on_i64_operands() {
    // i64 base/exp variables (not literals) — exercises the runtime native,
    // not constant folding.
    let src = "\
fn main() -> i32:
    base: i64 = 5
    exp: i64 = 6
    r: i64 = base ** exp
    println(str(r))
    return 0
";
    check("pow_i64", src, "15625\n");
}

#[test]
fn pow_exponent_zero_is_one() {
    let src = "\
fn main() -> i32:
    a: i64 = 5 ** 0
    println(str(a))
    b: i64 = 0 ** 0
    println(str(b))
    return 0
";
    check("pow_zero_exp", src, "1\n1\n");
}

#[test]
fn float_base_pow_is_floating_point() {
    // A float base routes to `MathPow` (f64::powf), not the integer native.
    let src = "\
fn main() -> i32:
    x: f64 = 2.0 ** 10.0
    println(str(x))
    return 0
";
    check("pow_float", src, "1024.0\n");
}

#[test]
fn negative_integer_exponent_is_clean_runtime_error() {
    // `int ** -k` would be a float in Python; until a float/BigInt result
    // path exists, it is a clean catchable ValueError rather than a silent
    // wrong answer. Interpreter-only: a function containing `try/except` is
    // (by design) not JIT-eligible (the M15 Throw/EnterTry carve-out), so the
    // both-engines `check` harness — which asserts JIT eligibility — doesn't
    // apply here.
    let src = "\
fn main() -> i32:
    try:
        e: i64 = -2
        r: i64 = 2 ** e
        println(str(r))
    except ValueError as ex:
        println(\"caught: negative exponent\")
    return 0
";
    let (code, out) = run_engine("pow_neg_exp", src, false);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "caught: negative exponent\n", "{out:?}");
}

#[test]
fn int_literal_above_i64_max_is_compile_error() {
    // 9223372036854775808 == i64::MAX + 1. Previously truncated silently to
    // i64::MIN; now a clean E2073 compile error.
    let src = "\
fn main() -> i32:
    x: i64 = 9223372036854775808
    println(str(x))
    return 0
";
    let err = compile_source("lit_overflow.spy".to_string(), src)
        .expect_err("an i64-overflowing integer literal must be a compile error");
    let msg = format!("{err}");
    assert!(
        msg.contains("E2073") && msg.contains("out of range"),
        "unexpected error (want E2073 out-of-range): {msg}"
    );
    assert!(
        msg.contains("BigInt"),
        "error should mention BigInt is not yet supported: {msg}"
    );
}

#[test]
fn i64_min_literal_is_accepted() {
    // `-9223372036854775808` (== i64::MIN) must still compile: the magnitude
    // 9223372036854775808 is out of range on its own, but the negated value
    // is exactly i64::MIN.
    let src = "\
fn main() -> i32:
    x: i64 = -9223372036854775808
    println(str(x))
    return 0
";
    check("i64_min_lit", src, "-9223372036854775808\n");
}

#[test]
fn int_pow_demo_example_runs() {
    let src_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/int_pow_demo.spy");
    let src = std::fs::read_to_string(src_path).expect("read int_pow_demo.spy");
    let (code, out) = run_engine("int_pow_demo", &src, false);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("2 ** 10 = 1024"), "stdout:\n{out}");
    assert!(out.contains("3 ** 19 = 1162261467"), "stdout:\n{out}");
    assert!(out.contains("7 ** 4 = 2401"), "stdout:\n{out}");
    assert!(out.contains("5 ** 0 = 1"), "stdout:\n{out}");
    assert!(out.contains("0 ** 0 = 1"), "stdout:\n{out}");
    assert!(out.contains("2.0 ** 10.0 = 1024.0"), "stdout:\n{out}");
}
