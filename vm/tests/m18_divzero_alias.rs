//! M18 regression for BUG-036: runtime divide-by-zero must be catchable
//! by either the canonical `ZeroDivisionError` (Python-compatible) name
//! OR the legacy `DivisionByZeroError` name. Pre-M18, the runtime emitted
//! the legacy name and arm-filter matching was exact-string, so the
//! canonical name didn't catch — found by the M18-R3 expr_interp stress
//! agent. Fixed by:
//!  1. Changing the four divzero emit sites in vm/src/interp.rs to use
//!     the canonical `ZeroDivisionError`.
//!  2. Adding `exception_name_alias` in vm/src/interp.rs so the legacy
//!     `DivisionByZeroError` filter still matches.
//!
//! See `docs/thesis/m18_round/probes/_probe_divzero_name.spy` for the
//! agent's minimal repro (archived alongside the M18 round artifacts).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m18_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn except_zero_division_error_catches_canonical_name() {
    let src = "\
fn main() -> i32:
    try:
        z: i64 = 1i64 / 0i64
        println(\"never: \" + str(z))
    except ZeroDivisionError as e:
        println(\"canonical: \" + e.type_name)
    return 0
";
    let p = compile_snippet("divzero_canonical", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(
        out.contains("canonical: ZeroDivisionError\n"),
        "ZeroDivisionError did not catch — BUG-036 regression. stdout: {out:?}"
    );
    assert!(!out.contains("never:"), "stdout: {out:?}");
}

#[test]
fn except_division_by_zero_error_legacy_name_still_catches() {
    let src = "\
fn main() -> i32:
    try:
        z: i64 = 1i64 / 0i64
        println(\"never: \" + str(z))
    except DivisionByZeroError as e:
        println(\"legacy: \" + e.type_name)
    return 0
";
    let p = compile_snippet("divzero_legacy", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    // The emitted type_name is the canonical one; the alias lets the
    // legacy filter match.
    assert!(
        out.contains("legacy: ZeroDivisionError\n"),
        "DivisionByZeroError filter did not catch via alias. stdout: {out:?}"
    );
}

#[test]
fn divzero_in_i32_path_emits_canonical_name() {
    // The four divzero emit sites cover i32, i64, u32, u64. Exercise i32
    // (the i64 path is already covered above).
    let src = "\
fn main() -> i32:
    try:
        z: i32 = 1i32 / 0i32
        println(\"never: \" + str(z))
    except ZeroDivisionError as e:
        println(\"i32: \" + e.type_name)
    return 0
";
    let p = compile_snippet("divzero_i32", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(
        out.contains("i32: ZeroDivisionError\n"),
        "i32 divzero did not emit ZeroDivisionError. stdout: {out:?}"
    );
}
