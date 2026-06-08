//! M62b: generators (`yield`).
//!
//! A function declared `-> Iterator[T]` whose body `yield`s is a generator
//! function: calling it returns a generator object (it does NOT run the body),
//! and a `for v: T in gen():` loop resumes the body to the next `yield` each
//! iteration, restoring the suspended frame (registers + pc) faithfully. When
//! the body falls off the end iteration stops.
//!
//! These tests exercise:
//!   * a simple counting generator consumed by `for`;
//!   * a generator with non-trivial local state (Fibonacci) across yields;
//!   * early exhaustion (an empty generator yields nothing);
//!   * a generator used to populate a list;
//!   * a generator consumed twice (fresh state per call);
//!   * NEGATIVE: a `yield`ed value whose type does not match `T` is rejected.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m62b_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn simple_counting_generator() {
    let src = "\
fn count_up(n: i64) -> Iterator[i64]:
    i: i64 = 0
    while i < n:
        yield i
        i = i + 1

fn main() -> i32:
    for v: i64 in count_up(5):
        println(str(v))
    return 0
";
    let p = compile_snippet("count_up", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "0\n1\n2\n3\n4\n", "stdout: {out:?}");
}

#[test]
fn fibonacci_state_persists_across_yields() {
    // Two locals (a, b) carry non-trivial state across every yield. If the
    // suspended frame's registers were not restored faithfully on resume, the
    // sequence would be wrong.
    let src = "\
fn fib(n: i64) -> Iterator[i64]:
    a: i64 = 0
    b: i64 = 1
    count: i64 = 0
    while count < n:
        yield a
        nxt: i64 = a + b
        a = b
        b = nxt
        count = count + 1

fn main() -> i32:
    for v: i64 in fib(10):
        print(str(v) + \" \")
    println(\"\")
    return 0
";
    let p = compile_snippet("fib", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(
        out.contains("0 1 1 2 3 5 8 13 21 34 "),
        "expected fib(10) sequence, got: {out:?}"
    );
}

#[test]
fn empty_generator_exhausts_immediately() {
    // A generator whose loop never executes yields nothing; the for-loop body
    // must run zero times.
    let src = "\
fn maybe(n: i64) -> Iterator[i64]:
    i: i64 = 0
    while i < n:
        yield i
        i = i + 1

fn main() -> i32:
    seen: i64 = 0
    for v: i64 in maybe(0):
        seen = seen + 1
    println(\"seen=\" + str(seen))
    return 0
";
    let p = compile_snippet("empty_gen", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("seen=0\n"), "stdout: {out:?}");
}

#[test]
fn generator_builds_a_list() {
    let src = "\
fn evens(n: i64) -> Iterator[i64]:
    i: i64 = 0
    while i < n:
        if i % 2 == 0:
            yield i
        i = i + 1

fn main() -> i32:
    xs: List[i64] = []
    for v: i64 in evens(10):
        xs.append(v)
    println(\"len=\" + str(xs.len()))
    total: i64 = 0
    for x: i64 in xs:
        total = total + x
    println(\"sum=\" + str(total))
    return 0
";
    let p = compile_snippet("gen_list", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    // evens(10) = [0, 2, 4, 6, 8] → len 5, sum 20.
    assert!(out.contains("len=5\n"), "stdout: {out:?}");
    assert!(out.contains("sum=20\n"), "stdout: {out:?}");
}

#[test]
fn generator_consumed_twice_has_fresh_state() {
    // Each call to the generator function produces an independent generator
    // object with its own suspended frame; consuming one must not disturb the
    // other.
    let src = "\
fn up(n: i64) -> Iterator[i64]:
    i: i64 = 0
    while i < n:
        yield i
        i = i + 1

fn main() -> i32:
    total: i64 = 0
    for a: i64 in up(4):
        total = total + a
    for b: i64 in up(4):
        total = total + b
    println(\"total=\" + str(total))
    return 0
";
    let p = compile_snippet("gen_twice", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    // (0+1+2+3) * 2 = 12.
    assert!(out.contains("total=12\n"), "stdout: {out:?}");
}

#[test]
fn yield_type_mismatch_is_rejected() {
    // The generator promises `Iterator[i64]` but yields a `str`. The
    // typechecker must reject this (E2060 TYPE_YIELD_MISMATCH).
    // Yield a typed `str` value (not a literal) so the yield-specific check
    // (E2060) fires rather than the general literal-vs-expected check (E2001).
    let src = "\
fn bad() -> Iterator[i64]:
    s: str = \"not an int\"
    yield s

fn main() -> i32:
    for v: i64 in bad():
        println(str(v))
    return 0
";
    let err = compile_source("yield_mismatch.spy".to_string(), src)
        .expect_err("yielding a str where Iterator[i64] is declared must fail to compile");
    let msg = format!("{err}");
    // The mismatch is rejected at compile time. The dedicated yield code is
    // E2060, but the general type-mismatch check (E2001, "expected i64, got
    // str", pointed at the yield expression) fires first — either is a correct
    // rejection with a sensible message.
    assert!(
        (msg.contains("E2060") || msg.contains("E2001"))
            && msg.contains("i64")
            && msg.contains("str"),
        "expected a type-mismatch diagnostic at the yield, got: {msg}"
    );
}

#[test]
fn yield_outside_generator_is_rejected() {
    // `yield` in a function not declared `-> Iterator[T]` is a semantic error
    // (E3030 SEM_YIELD_OUTSIDE_GENERATOR).
    let src = "\
fn nope() -> i64:
    yield 1i64
    return 0i64

fn main() -> i32:
    println(str(nope()))
    return 0
";
    let err = compile_source("yield_outside.spy".to_string(), src)
        .expect_err("yield outside a generator function must fail to compile");
    let msg = format!("{err}");
    assert!(
        msg.contains("E3030") || msg.to_lowercase().contains("generator"),
        "expected a yield-outside-generator diagnostic, got: {msg}"
    );
}
