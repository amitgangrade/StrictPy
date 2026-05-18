//! M13 regression tests for BUG-035 — `and` / `or` must short-circuit.
//!
//! Before M13, `compiler/src/ir.rs::emit_binop` lowered `AstBinOp::And`
//! to `IROp::IAnd` (bitwise) and `AstBinOp::Or` to `IROp::IOr`. Both
//! operands were eagerly evaluated before the bitwise op ran, which
//! meant the standard guard idiom
//!
//!     b > 0 and xs[b - 1] > 0
//!
//! trapped with `IndexError(-1)` whenever `b == 0`. Surfaced by the
//! M12 B-tree's insertion sort (see `docs/thesis/agent_reports/m12_btree.md`
//! "Bug B"). The fix in M13 inspects `AstBinOp::And`/`Or` in
//! `lower_expr` (BEFORE both operands get lowered) and routes them to
//! `lower_short_circuit`, which emits a real basic-block split with a
//! conditional branch on the lhs and merges the result via the
//! slot-based ReadLocal/WriteLocal phi pattern (mirroring the M3.5
//! loop-carried-locals fix).
//!
//! Coverage strategy: prove BOTH that the value semantics are correct
//! (truth-table cases) AND that the rhs is NOT evaluated when the lhs
//! decides the result (the "trap-on-rhs" cases). The latter is the
//! load-bearing property — pure value-only tests would have passed
//! under the old bitwise lowering too.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m13_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── (1) `and` short-circuits: rhs index would trap if evaluated ─────────

#[test]
fn and_short_circuits_when_lhs_is_false_protecting_index() {
    // This is the canonical BUG-035 repro from BUGS_KNOWN.md §7. Under
    // the old bitwise lowering `xs[b - 1i32]` (== `xs[-1]`) was always
    // evaluated and trapped with IndexError. With short-circuit `and`,
    // the rhs is skipped because `b > 0` is false.
    let src = "\
fn main() -> i32:
    xs: List[i32] = [10i32]
    b: i32 = 0i32
    r: bool = b > 0i32 and xs[b - 1i32] > 0i32
    println(\"r=\" + str(r))
    return 0
";
    let p = compile_snippet("and_short_circuits", src);
    let (code, out) = run_file_capture(&p).expect("must not trap IndexError(-1)");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(out.contains("r=false\n"), "got: {out:?}");
}

// ── (2) `or` short-circuits: rhs index would trap if evaluated ──────────

#[test]
fn or_short_circuits_when_lhs_is_true_protecting_index() {
    // Mirror of (1) for `or`: when the lhs is true, the rhs must NOT
    // be evaluated. Under the old bitwise lowering, `xs[b - 1i32]`
    // would trap.
    let src = "\
fn main() -> i32:
    xs: List[i32] = [10i32]
    b: i32 = 0i32
    r: bool = b == 0i32 or xs[b - 1i32] > 0i32
    println(\"r=\" + str(r))
    return 0
";
    let p = compile_snippet("or_short_circuits", src);
    let (code, out) = run_file_capture(&p).expect("must not trap IndexError(-1)");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(out.contains("r=true\n"), "got: {out:?}");
}

// ── (3) `and` with both operands true returns true ──────────────────────

#[test]
fn and_both_true_returns_true() {
    let src = "\
fn main() -> i32:
    r: bool = true and true
    println(\"r=\" + str(r))
    return 0
";
    let p = compile_snippet("and_both_true", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit: {out:?}");
    assert!(out.contains("r=true\n"), "got: {out:?}");
}

// ── (4) `and` with first false skips a trapping rhs ────────────────────

#[test]
fn and_first_false_skips_division_by_zero_in_rhs() {
    // `1i32 / 0i32 == 0i32` would trap with "division by zero" if it
    // ran. Under short-circuit semantics, the false lhs means the rhs
    // never executes. Asserts both: program exits 0 AND `r == false`.
    let src = "\
fn main() -> i32:
    z: i32 = 0i32
    r: bool = false and (1i32 / z == 0i32)
    println(\"r=\" + str(r))
    return 0
";
    let p = compile_snippet("and_first_false_skips_divzero", src);
    let (code, out) = run_file_capture(&p).expect("must not trap div-by-zero");
    assert_eq!(code, 0, "rhs must not execute; stdout: {out:?}");
    assert!(out.contains("r=false\n"), "got: {out:?}");
}

// ── (5) `or` with first true skips a trapping rhs ──────────────────────

#[test]
fn or_first_true_skips_division_by_zero_in_rhs() {
    // Mirror of (4) for `or`. Under short-circuit semantics, the true
    // lhs means the rhs never executes.
    let src = "\
fn main() -> i32:
    z: i32 = 0i32
    r: bool = true or (1i32 / z == 0i32)
    println(\"r=\" + str(r))
    return 0
";
    let p = compile_snippet("or_first_true_skips_divzero", src);
    let (code, out) = run_file_capture(&p).expect("must not trap div-by-zero");
    assert_eq!(code, 0, "rhs must not execute; stdout: {out:?}");
    assert!(out.contains("r=true\n"), "got: {out:?}");
}

// ── (6) Chained `and`: all 8 truth-table rows ──────────────────────────

#[test]
fn chained_and_all_eight_truth_combinations() {
    // `a and b and c` for every assignment of {a,b,c} ∈ {true,false}^3.
    // Result should be true iff all three are true.
    let src = "\
fn show(a: bool, b: bool, c: bool) -> None:
    r: bool = a and b and c
    println(str(a) + \" \" + str(b) + \" \" + str(c) + \" => \" + str(r))

fn main() -> i32:
    show(false, false, false)
    show(false, false, true)
    show(false, true,  false)
    show(false, true,  true)
    show(true,  false, false)
    show(true,  false, true)
    show(true,  true,  false)
    show(true,  true,  true)
    return 0
";
    let p = compile_snippet("chained_and_truth_table", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit: {out:?}");

    // Only the all-true row should be true; the other seven false.
    assert!(out.contains("false false false => false\n"), "got: {out:?}");
    assert!(out.contains("false false true => false\n"),  "got: {out:?}");
    assert!(out.contains("false true false => false\n"),  "got: {out:?}");
    assert!(out.contains("false true true => false\n"),   "got: {out:?}");
    assert!(out.contains("true false false => false\n"),  "got: {out:?}");
    assert!(out.contains("true false true => false\n"),   "got: {out:?}");
    assert!(out.contains("true true false => false\n"),   "got: {out:?}");
    assert!(out.contains("true true true => true\n"),     "got: {out:?}");
}
