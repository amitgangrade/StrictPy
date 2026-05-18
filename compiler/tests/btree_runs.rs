//! Integration test for examples/btree.spy.
//!
//! The B-tree exercises exactly the pattern that historically broke:
//! a final class with a `List[BNode?]` (class-ref nullable) field, plus
//! recursive method-style traversal during insert/search, plus splitting
//! that rewrites class-ref slots and allocates fresh BNode instances in
//! a loop.
//!
//! Post-M11 (BUG-015/016/017/N2 fixed, BUG-026/027 provisionally closed)
//! we expect this to run cleanly end-to-end. If it crashes with
//! STATUS_HEAP_CORRUPTION (0xC0000374) or produces non-deterministic
//! output, that's a regression and the headline finding for the M12
//! report.
//!
//! We use the in-process `run_file_capture` path (like sudoku_runs.rs)
//! rather than the out-of-process `spy.exe` path (like lisp_runs.rs)
//! BECAUSE M11 was supposed to make in-process teardown safe for this
//! shape. If in-process trips heap corruption, we've found a regression
//! and want the test to surface it loudly.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

/// Regression for BUG-034 (M12): `str != str` previously always returned
/// true because `compiler/src/ir.rs::emit_binop`'s `AstBinOp::Ne` arm had
/// no `is_str` branch and fell through to `INe`, which compared the two
/// heap-pointer u64s. Fixed by lowering `Ne` on str operands as `StrEq`
/// followed by `BoolNot` (mirroring the `IsNot`/BUG-008 precedent).
/// See `examples/_probe_str_ne.spy` for the standalone repro.
#[test]
fn str_ne_returns_false_for_equal_strings() {
    let src_path = project_root().join("examples").join("_probe_str_ne.spy");
    let src = fs::read_to_string(&src_path).expect("read _probe_str_ne.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile probe: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("_probe_str_ne.spyc");
    fs::write(&spyc_path, &bytes).expect("write probe");
    let (code, out) = run_file_capture(&spyc_path).expect("run probe");
    eprintln!("probe stdout:\n{out}");
    assert_eq!(code, 0);
    assert!(out.contains("a == b \u{2192} true"), "a==b should be true:\n{out}");
    assert!(out.contains("a != b \u{2192} false"), "a!=b should be false (BUG-034 fix):\n{out}");
    assert!(out.contains("a == c \u{2192} false"), "a==c should be false:\n{out}");
    assert!(out.contains("a != c \u{2192} true"), "a!=c should be true:\n{out}");
}

#[test]
fn btree_compiles() {
    let src_path = project_root().join("examples").join("btree.spy");
    let src = fs::read_to_string(&src_path).expect("read btree.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile btree.spy: {e}"));
}

#[test]
fn btree_passes_all_internal_tests() {
    let src_path = project_root().join("examples").join("btree.spy");
    let src = fs::read_to_string(&src_path).expect("read btree.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile btree.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("btree.spyc");
    fs::write(&spyc_path, &bytes).expect("write btree.spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run btree");
    eprintln!("STDOUT:\n{out}");
    assert_eq!(code, 0, "non-zero exit; stdout was:\n{out}");

    // Internal-invariant check fired and passed.
    assert!(
        out.contains("INVARIANT_OK"),
        "missing INVARIANT_OK marker:\n{out}"
    );

    // All sub-tests passed.
    assert!(
        out.contains("OK: 5/5"),
        "expected OK: 5/5, got:\n{out}"
    );

    // Spot-check: no test printed a FAIL line.
    assert!(
        !out.contains("FAIL"),
        "stdout contains a FAIL line:\n{out}"
    );
}

#[test]
fn btree_runs_deterministically_three_times() {
    // BUG-026/027 are about non-deterministic heap corruption on
    // programs of this shape (class with class-ref fields, virtual
    // calls, many small allocations). The bugs are provisionally
    // closed post-M11; this test upgrades to "confirmed" if it
    // passes 3 in a row. If even one run differs, we've found a
    // regression and the assert below will fire.
    let src_path = project_root().join("examples").join("btree.spy");
    let src = fs::read_to_string(&src_path).expect("read btree.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile btree.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("btree_det.spyc");
    fs::write(&spyc_path, &bytes).expect("write btree_det.spyc");

    let mut first: Option<String> = None;
    for attempt in 0..3 {
        let (code, out) = run_file_capture(&spyc_path).expect("run btree");
        assert_eq!(
            code, 0,
            "attempt {attempt}: non-zero exit; stdout:\n{out}"
        );
        match &first {
            None => first = Some(out),
            Some(prev) => assert_eq!(
                prev, &out,
                "attempt {attempt} stdout differs from first run — \
                 BUG-026/027 regression suspected"
            ),
        }
    }
}
