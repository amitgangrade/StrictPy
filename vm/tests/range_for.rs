//! Regression: `for i in range(...)` loops.
//!
//! Previously the for-loop IR lowering only handled `List[T]` receivers; any
//! other iterable (including `range()`, whose static type is `Range`, not
//! `List`) fell through to a run-once placeholder that executed the body a
//! single time with a garbage loop variable. The runtime `range()` also capped
//! at 1M elements. Fixed by lowering `for v in range(...)` directly to a lazy
//! integer counter loop (compiler/src/ir.rs): unbounded, no allocation,
//! handles range(stop) / range(start, stop) / range(start, stop, step) with a
//! negative step, and preserves break/continue semantics.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn run(name: &str, src: &str) -> (i32, String) {
    let bytes = compile_source(format!("{name}.spy"), src)
        .unwrap_or_else(|e| panic!("{name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_rangefor_{name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    run_file_capture(&out).expect("run")
}

#[test]
fn one_arg_range_iterates_fully() {
    let src = "\
fn main() -> i32:
    c: i64 = 0
    s: i64 = 0
    for i: i64 in range(5):
        c = c + 1
        s = s + i
    println(str(c))
    println(str(s))
    return 0
";
    let (code, out) = run("one_arg", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "5\n10\n", "{out:?}");
}

#[test]
fn two_and_three_arg_range() {
    let src = "\
fn main() -> i32:
    a: i64 = 0
    for k: i64 in range(2, 6):
        a = a + k
    println(str(a))                 # 2+3+4+5 = 14
    b: i64 = 0
    for m: i64 in range(0, 10, 2):
        b = b + m
    println(str(b))                 # 0+2+4+6+8 = 20
    return 0
";
    let (code, out) = run("two_three", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "14\n20\n", "{out:?}");
}

#[test]
fn negative_step_counts_down() {
    let src = "\
fn main() -> i32:
    out: str = \"\"
    for d: i64 in range(5, 0, -1i64):
        out = out + str(d)
    println(out)                    # 54321
    return 0
";
    let (code, out) = run("neg_step", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "54321\n", "{out:?}");
}

#[test]
fn empty_range_runs_zero_times() {
    let src = "\
fn main() -> i32:
    c: i64 = 0
    for z: i64 in range(0):
        c = c + 1
    for z2: i64 in range(5, 5):
        c = c + 1
    for z3: i64 in range(0, 10, -1i64):
        c = c + 1
    println(str(c))                 # 0
    return 0
";
    let (code, out) = run("empty", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "0\n", "{out:?}");
}

#[test]
fn large_range_is_not_capped() {
    // The old materialising range() trapped beyond ~1M elements; the counter
    // loop has no cap and allocates nothing.
    let src = "\
fn main() -> i32:
    total: i64 = 0
    for i: i64 in range(3000000):
        total = total + 1
    println(str(total))             # 3000000
    return 0
";
    let (code, out) = run("large", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "3000000\n", "{out:?}");
}

#[test]
fn break_and_continue_semantics() {
    let src = "\
fn main() -> i32:
    cb: i64 = 0
    for n: i64 in range(10):
        if n == 5:
            break
        if n % 2i64 == 0i64:
            continue
        cb = cb + n
    println(str(cb))                # 1 + 3 = 4 (continue still steps the counter)
    return 0
";
    let (code, out) = run("break_continue", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "4\n", "{out:?}");
}

#[test]
fn nested_range_loops() {
    let src = "\
fn main() -> i32:
    acc: i64 = 0
    for i: i64 in range(3):
        for j: i64 in range(3):
            acc = acc + i * 3i64 + j
    println(str(acc))               # sum 0..8 = 36
    return 0
";
    let (code, out) = run("nested", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "36\n", "{out:?}");
}
