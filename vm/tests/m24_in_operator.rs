//! M24 regression for BUG-039: `key in d` previously always returned
//! `false` because the IR lowered `In` to `IROp::IEq` — comparing the
//! key against the container's heap pointer as i64. Always false for
//! any separately-allocated key. Found by the M24-B event_log stress
//! agent (probe 10's hour-bucket histogram silently produced zero
//! buckets because `bucket in seen` was always false).
//!
//! Same shape as:
//! - BUG-008 (`is not` was `RefEq` not `not RefEq`)
//! - BUG-034 (`str !=` had no `is_str` branch)
//! - BUG-037 (`??` was `Copy(rhs)`)
//!
//! Fourth instance of the placeholder-lowering pattern. Fixed in M24
//! by dispatching the `In` lowering on the RHS container type:
//!   - `key in Dict[str, V]` -> NativeFn::DictHas(dict, key)
//!   - `x   in Set[T]`       -> NativeFn::SetHas(set, x)
//!   - `x   in List[T]`      -> still placeholder (v0.3; needs
//!                              linear-scan native or inline loop)
//!   - `key in Dict[non-str, _]` -> still placeholder (M5 Dict runtime
//!                                  is hardcoded to str keys; non-str
//!                                  Dict is v0.3 entirely)
//!
//! Symmetric `NotIn` lowering: same dispatch + BoolNot.
//!
//! Repros preserved at examples/_probe_dict_in_*.spy from the M24-B
//! worktree.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m24_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn str_in_dict_returns_true_for_present_key() {
    let src = "\
fn main() -> i32:
    d: Dict[str, i64] = {}
    d[\"x\"] = 1i64
    d[\"y\"] = 2i64
    if \"x\" in d:
        println(\"x: yes\")
    else:
        println(\"x: NO\")
    if \"missing\" in d:
        println(\"missing: yes\")
    else:
        println(\"missing: no\")
    return 0
";
    let p = compile_snippet("in_dict_str", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("x: yes\n"), "BUG-039 regression: {out:?}");
    assert!(out.contains("missing: no\n"), "BUG-039 regression: {out:?}");
}

#[test]
fn str_not_in_dict_inverts_correctly() {
    let src = "\
fn main() -> i32:
    d: Dict[str, str] = {}
    d[\"k\"] = \"v\"
    if \"k\" not in d:
        println(\"k missing\")
    else:
        println(\"k present\")
    if \"absent\" not in d:
        println(\"absent\")
    else:
        println(\"unexpected present\")
    return 0
";
    let p = compile_snippet("not_in_dict_str", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("k present\n"), "stdout: {out:?}");
    assert!(out.contains("absent\n"), "stdout: {out:?}");
}

#[test]
fn variable_key_in_dict() {
    // The M24-B probe specifically guarded against constant-folding,
    // pinning the key via a variable. Verify both forms work.
    let src = "\
fn main() -> i32:
    d: Dict[str, i64] = {}
    d[\"hello\"] = 42i64
    k: str = \"hello\"
    if k in d:
        println(\"variable key found\")
    else:
        println(\"variable key NOT found\")
    return 0
";
    let p = compile_snippet("in_dict_var_key", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("variable key found\n"), "stdout: {out:?}");
}

#[test]
fn in_dict_value_types() {
    // The M24-B probe tested Dict[str, *] across i64/i32/bool/str
    // value types. Verify each.
    let src = "\
fn main() -> i32:
    di64: Dict[str, i64] = {}
    di64[\"a\"] = 1i64
    if \"a\" in di64: println(\"i64 ok\")

    di32: Dict[str, i32] = {}
    di32[\"a\"] = 1i32
    if \"a\" in di32: println(\"i32 ok\")

    dbool: Dict[str, bool] = {}
    dbool[\"a\"] = true
    if \"a\" in dbool: println(\"bool ok\")

    dstr: Dict[str, str] = {}
    dstr[\"a\"] = \"v\"
    if \"a\" in dstr: println(\"str ok\")
    return 0
";
    let p = compile_snippet("in_dict_value_types", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("i64 ok\n"), "stdout: {out:?}");
    assert!(out.contains("i32 ok\n"), "stdout: {out:?}");
    assert!(out.contains("bool ok\n"), "stdout: {out:?}");
    assert!(out.contains("str ok\n"), "stdout: {out:?}");
}

#[test]
fn in_dict_after_overwrite_and_grow() {
    // Membership tracking under churn: set, overwrite, add another,
    // check both.
    let src = "\
fn main() -> i32:
    d: Dict[str, i64] = {}
    d[\"k\"] = 1i64
    d[\"k\"] = 2i64
    d[\"j\"] = 3i64
    if \"k\" in d:
        println(\"k yes\")
    else:
        println(\"k no\")
    if \"j\" in d:
        println(\"j yes\")
    else:
        println(\"j no\")
    return 0
";
    let p = compile_snippet("in_dict_churn", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("k yes\n"), "stdout: {out:?}");
    assert!(out.contains("j yes\n"), "stdout: {out:?}");
}
