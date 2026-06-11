//! Conformance: bitwise/shift ops evaluate at the operand's declared width.
//!
//! Regression: codegen hardwired `&`, `|`, `^`, `<<`, `>>`, `~` to the
//! I32 opcodes regardless of operand type, so i64/u64 operands were
//! truncated to 32 bits (`2463534242 >> 1` printed -915716527 instead of
//! 1231767121, `a & 4294967295` went negative, and popcount loops like
//! `while v != 0: v = v >> 1` never terminated for v >= 2^31 because the
//! arithmetic shift kept replicating the wrongly-set sign bit). Fixed by
//! dispatching on operand width like the arithmetic ops (IAddI64 etc.).
//!
//! Expected outputs below are CPython's results for the same expressions
//! (verified against python3), which agree with two's-complement i64
//! semantics for all in-range values, including negative operands.
//! Each program is run through both engines: the Cranelift JIT
//! (`Interpreter::new`, jit feature default) and the pure interpreter
//! (`SharedVm::new` + `from_shared`, no JIT cache installed).

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
        // The JIT leg is only meaningful if the functions actually
        // compile; a decode failure would silently fall back to the
        // interpreter and test the same path twice.
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
fn i64_bitwise_matches_python() {
    let src = "\
fn main() -> i32:
    a: i64 = 2463534242
    println(str(a >> 1))
    println(str(a << 1))
    println(str(a & 4294967295))
    println(str(a | 6148914691236517205))
    println(str(a ^ 6148914691236517205))
    println(str(~a))
    c: i64 = 1
    println(str(c << 40))
    println(str(c << 62))
    return 0
";
    check(
        "i64_bitwise",
        src,
        "1231767121\n\
         4927068484\n\
         2463534242\n\
         6148914693426109943\n\
         6148914693152168439\n\
         -2463534243\n\
         1099511627776\n\
         4611686018427387904\n",
    );
}

#[test]
fn i64_negative_operands_match_python() {
    // Python's `>>` on negative ints floors toward -inf, i.e. an
    // arithmetic shift — `-1 >> k == -1` for any k, and `&` with a
    // positive mask extracts the two's-complement low bits.
    let src = "\
fn main() -> i32:
    n: i64 = -2463534243
    println(str(n >> 1))
    println(str(n & 4294967295))
    println(str(n << 2))
    m: i64 = -1
    println(str(m >> 5))
    println(str(m & 4294967295))
    println(str(m ^ 2463534242))
    return 0
";
    check(
        "i64_negative",
        src,
        "-1231767122\n\
         1831433053\n\
         -9854136972\n\
         -1\n\
         4294967295\n\
         -2463534243\n",
    );
}

#[test]
fn i64_popcount_loop_terminates() {
    // Pre-fix this looped forever: `v >> 1` was a 32-bit arithmetic
    // shift of a value whose (wrong) sign bit was set, so v never
    // reached 0.
    let src = "\
fn main() -> i32:
    v: i64 = 2463534242
    cnt: i32 = 0
    while v != 0:
        cnt = cnt + i32(v & 1)
        v = v >> 1
    println(str(cnt))
    return 0
";
    check("i64_popcount", src, "14\n");
}

#[test]
fn u64_shift_is_logical() {
    // `>>` on unsigned operands is a logical shift (UShrU64), so the
    // all-ones value halves to i64::MAX instead of staying -1.
    // Results stay below 2^63 because `str()` of a u64 in the top half
    // of the range prints as signed (a separate, pre-existing issue).
    let src = "\
fn main() -> i32:
    u: u64 = 18446744073709551615
    println(str(u >> 32))
    println(str(u & 255))
    println(str(u >> 1))
    h: u64 = 9223372036854775808
    println(str(h >> 63))
    println(str(h ^ u))
    return 0
";
    check(
        "u64_shift",
        src,
        "4294967295\n\
         255\n\
         9223372036854775807\n\
         1\n\
         9223372036854775807\n",
    );
}

#[test]
fn i32_bitwise_unchanged() {
    // i32 operands keep 32-bit semantics: arithmetic shift preserves the
    // sign bit and `~`/`&`/`^` operate on the 32-bit value.
    let src = "\
fn main() -> i32:
    b: i32 = -2147483648
    println(str(b >> 31))
    println(str(b >> 1))
    x: i32 = 1
    println(str(x << 3))
    y: i32 = 51966
    println(str(y & 255))
    println(str(~y))
    println(str(y ^ 43690))
    return 0
";
    check(
        "i32_bitwise",
        src,
        "-1\n\
         -1073741824\n\
         8\n\
         254\n\
         -51967\n\
         24660\n",
    );
}
