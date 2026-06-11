//! Regression tests for dict key deletion (`del d[k]` / `d.remove(k)`).
//!
//! Before NativeFn::DictRemove existed, the IR lowered `Stmt::Del` to
//! nothing — `del d[k]` parsed and type-checked but compiled to a no-op,
//! so `len(d)` and `d.get(k)` were unchanged afterwards, and there was no
//! `remove`/`pop` alternative. That silently broke every eviction-shaped
//! pattern (LRU caches, session expiry). These tests pin down:
//!
//!   1. `del d[k]` really removes the entry (len / get / `in` all agree),
//!   2. `del d[k]` on an absent key is a quiet no-op,
//!   3. `d.remove(k) -> bool` reports whether the key was present,
//!   4. a deleted key can be re-inserted (cache-eviction round trip),
//!   5. `del` on anything that is NOT a Dict entry is a compile error
//!      (a silent no-op `del` is worse than a missing feature).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_compiler::error::CompileError;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_dict_remove_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn del_removes_dict_entry() {
    let src = "\
fn main() -> i32:
    d: Dict[str, i64] = {}
    d[\"a\"] = 1
    d[\"b\"] = 2
    del d[\"a\"]
    println(str(len(d)))
    v: i64? = d.get(\"a\")
    if v is none:
        println(\"a-gone\")
    else:
        println(\"a-still-there\")
    if \"a\" in d:
        println(\"in-says-present\")
    else:
        println(\"in-says-absent\")
    println(str(d[\"b\"]))
    return 0
";
    let p = compile_snippet("del_removes_entry", src);
    let (code, out) = run_file_capture(&p).expect("must run cleanly");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(out.contains("1\n"), "len(d) must be 1 after del; got: {out:?}");
    assert!(out.contains("a-gone"), "d.get must miss after del; got: {out:?}");
    assert!(out.contains("in-says-absent"), "`in` must miss after del; got: {out:?}");
    assert!(out.contains("2\n"), "untouched key must survive; got: {out:?}");
}

#[test]
fn del_absent_key_is_quiet_noop() {
    let src = "\
fn main() -> i32:
    d: Dict[str, i64] = {}
    d[\"keep\"] = 9
    del d[\"never-inserted\"]
    println(str(len(d)))
    println(str(d[\"keep\"]))
    return 0
";
    let p = compile_snippet("del_absent_noop", src);
    let (code, out) = run_file_capture(&p).expect("must run cleanly");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(out.contains("1\n"), "len unchanged by absent-key del; got: {out:?}");
    assert!(out.contains("9\n"), "existing entry unchanged; got: {out:?}");
}

#[test]
fn remove_method_reports_presence() {
    let src = "\
fn main() -> i32:
    d: Dict[str, i64] = {}
    d[\"x\"] = 1
    if d.remove(\"x\"):
        println(\"first-true\")
    else:
        println(\"first-false\")
    if d.remove(\"x\"):
        println(\"second-true\")
    else:
        println(\"second-false\")
    println(str(len(d)))
    return 0
";
    let p = compile_snippet("remove_reports_presence", src);
    let (code, out) = run_file_capture(&p).expect("must run cleanly");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(out.contains("first-true"), "remove of present key → true; got: {out:?}");
    assert!(out.contains("second-false"), "remove of absent key → false; got: {out:?}");
    assert!(out.contains("0\n"), "dict empty after remove; got: {out:?}");
}

// The LRU-cache shape that motivated the fix: evict, then reuse the slot.
#[test]
fn deleted_key_can_be_reinserted() {
    let src = "\
fn main() -> i32:
    cache: Dict[str, str] = {}
    cache[\"k\"] = \"old\"
    del cache[\"k\"]
    cache[\"k\"] = \"new\"
    v: str? = cache.get(\"k\")
    if v is not none:
        println(v)
    println(str(len(cache)))
    return 0
";
    let p = compile_snippet("reinsert_after_del", src);
    let (code, out) = run_file_capture(&p).expect("must run cleanly");
    assert_eq!(code, 0, "exit code; stdout: {out:?}");
    assert!(out.contains("new"), "re-inserted value must win; got: {out:?}");
    assert!(out.contains("1\n"), "exactly one live entry; got: {out:?}");
}

// `del` targets we cannot actually delete must be rejected at compile
// time, not lowered to a no-op.

fn assert_type_error(name: &str, src: &str) {
    match compile_source(format!("{name}.spy"), src) {
        Err(CompileError::Type { .. }) => {}
        Err(other) => panic!("{name}: expected a type error, got: {other}"),
        Ok(_) => panic!("{name}: must be rejected at compile time, but compiled"),
    }
}

#[test]
fn del_on_plain_name_is_compile_error() {
    assert_type_error("del_ident", "\
fn main() -> i32:
    x: i64 = 1
    del x
    return 0
");
}

#[test]
fn del_on_list_index_is_compile_error() {
    assert_type_error("del_list_index", "\
fn main() -> i32:
    xs: List[i64] = [1, 2, 3]
    del xs[0]
    return 0
");
}

#[test]
fn dict_remove_wrong_arity_is_compile_error() {
    assert_type_error("remove_arity", "\
fn main() -> i32:
    d: Dict[str, i64] = {}
    d.remove()
    return 0
");
}
