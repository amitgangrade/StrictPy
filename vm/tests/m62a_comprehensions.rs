//! M62a acceptance: list / dict / set comprehensions.
//!
//! Comprehensions desugar to a fresh collection plus an index-counted loop
//! over a `List[T]` iterable that appends each (optionally filtered) element
//! (see `compiler/src/ir.rs::lower_comprehension`). The loop variable is
//! annotated exactly like the `for` statement (`for x: T in it`).
//!
//! Coverage:
//!   - list comprehension (map)
//!   - list comprehension with an `if` filter
//!   - dict comprehension (`{k: v for ...}`), probed via `in`
//!   - set comprehension — compiles + lowers (the VM has no set runtime yet,
//!     same as set *literals*, so we only assert it compiles & runs)
//!   - NEGATIVE: an ill-typed body is rejected at compile time
//!
//! Modeled on `vm/tests/m24_in_operator.rs`.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m62a_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn list_comprehension_maps_elements() {
    let src = "\
fn main() -> i32:
    xs: List[i64] = [1i64, 2i64, 3i64, 4i64]
    sq: List[i64] = [x * x for x: i64 in xs]
    println(\"len=\" + str(i64(len(sq))))
    println(\"sq0=\" + str(sq[0]))
    println(\"sq3=\" + str(sq[3]))
    return 0
";
    let p = compile_snippet("list_map", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("len=4\n"), "stdout: {out:?}");
    assert!(out.contains("sq0=1\n"), "stdout: {out:?}");
    assert!(out.contains("sq3=16\n"), "stdout: {out:?}");
}

#[test]
fn list_comprehension_with_filter() {
    // Keep only the even elements, then square them: [4, 16, 36].
    let src = "\
fn main() -> i32:
    xs: List[i64] = [1i64, 2i64, 3i64, 4i64, 5i64, 6i64]
    evens: List[i64] = [x * x for x: i64 in xs if x % 2i64 == 0i64]
    println(\"len=\" + str(i64(len(evens))))
    println(\"e0=\" + str(evens[0]))
    println(\"e1=\" + str(evens[1]))
    println(\"e2=\" + str(evens[2]))
    return 0
";
    let p = compile_snippet("list_filter", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("len=3\n"), "stdout: {out:?}");
    assert!(out.contains("e0=4\n"), "stdout: {out:?}");
    assert!(out.contains("e1=16\n"), "stdout: {out:?}");
    assert!(out.contains("e2=36\n"), "stdout: {out:?}");
}

#[test]
fn dict_comprehension_builds_map() {
    // {str(n): n*n for n in [2,3,4]}. Probe by `in` (deterministic; avoids
    // hash-iteration order).
    let src = "\
fn main() -> i32:
    xs: List[i64] = [2i64, 3i64, 4i64]
    sq: Dict[str, i64] = {str(n): n * n for n: i64 in xs}
    if \"3\" in sq:
        println(\"has 3\")
    else:
        println(\"NO 3\")
    if \"9\" not in sq:
        println(\"no key 9\")
    return 0
";
    let p = compile_snippet("dict_build", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("has 3\n"), "stdout: {out:?}");
    assert!(out.contains("no key 9\n"), "stdout: {out:?}");
}

#[test]
fn dict_comprehension_with_filter() {
    let src = "\
fn main() -> i32:
    xs: List[i64] = [1i64, 2i64, 3i64, 4i64]
    sq: Dict[str, i64] = {str(n): n for n: i64 in xs if n > 2i64}
    if \"3\" in sq:
        println(\"has 3\")
    if \"1\" not in sq:
        println(\"filtered out 1\")
    return 0
";
    let p = compile_snippet("dict_filter", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("has 3\n"), "stdout: {out:?}");
    assert!(out.contains("filtered out 1\n"), "stdout: {out:?}");
}

#[test]
fn set_comprehension_compiles() {
    // Sets have no VM runtime yet (set *literals* lower to a placeholder too),
    // so a set comprehension is only required to parse, type-check, and emit
    // valid bytecode. We assert it compiles cleanly; its runtime contents are
    // intentionally not exercised (see LANGUAGE_GUIDE §11.44).
    let src = "\
fn main() -> i32:
    xs: List[i64] = [1i64, 2i64, 3i64, 4i64]
    s: Set[i64] = {x % 2i64 for x: i64 in xs}
    println(\"set comp ok\")
    return 0
";
    let bytes = compile_source("set_comp.spy".to_string(), src)
        .expect("set comprehension should compile");
    assert!(!bytes.is_empty(), "expected non-empty bytecode");
}

#[test]
fn ill_typed_body_is_rejected() {
    // The body produces `str`, but the target is `List[i64]` — must be a
    // compile error (TYPE_COMPREHENSION_ELEM_MISMATCH / E2041).
    let src = "\
fn main() -> i32:
    ss: List[str] = [\"a\", \"b\"]
    bad: List[i64] = [w for w: str in ss]
    return 0
";
    let err = compile_source("ill_typed.spy".to_string(), src)
        .expect_err("ill-typed comprehension body should be rejected");
    let msg = format!("{err}");
    assert!(
        msg.contains("E2041") || msg.to_lowercase().contains("comprehension"),
        "expected a comprehension element-type error; got: {msg}"
    );
}

#[test]
fn filter_must_be_bool() {
    // A non-bool `if` filter is rejected (E2042).
    let src = "\
fn main() -> i32:
    xs: List[i64] = [1i64, 2i64]
    bad: List[i64] = [x for x: i64 in xs if x]
    return 0
";
    let err = compile_source("bad_filter.spy".to_string(), src)
        .expect_err("non-bool comprehension filter should be rejected");
    let msg = format!("{err}");
    assert!(
        msg.contains("E2042") || msg.to_lowercase().contains("bool"),
        "expected a filter-not-bool error; got: {msg}"
    );
}
