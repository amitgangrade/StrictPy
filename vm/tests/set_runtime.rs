//! Set runtime acceptance: literals, `add`/`has`/`length`, `len()`,
//! `in`/`not in`, and set comprehensions with real contents.
//!
//! Sets are DictRepr-backed ("dict-with-unit-value"): the compiler lowers a
//! set literal to DictNew + one SetAdd per element, and SetAdd/SetHas carry a
//! TypeTag operand so the VM canonicalises elements by value (integer value /
//! float bit pattern / string content) — see vm/src/builtins.rs::set_elem_key.
//!
//! Modeled on `vm/tests/m62a_comprehensions.rs`.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_set_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn int_set_add_has_len_dedupe() {
    let src = "\
fn main() -> i32:
    s: Set[i64] = {1i64, 2i64, 3i64}
    s.add(10i64)
    s.add(2i64)
    println(\"len=\" + str(len(s)))
    println(\"length=\" + str(s.length()))
    if s.has(10i64):
        println(\"has 10\")
    if 3i64 in s:
        println(\"in 3\")
    if 5i64 not in s:
        println(\"no 5\")
    return 0
";
    let p = compile_snippet("int_ops", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("len=4\n"), "stdout: {out:?}");
    assert!(out.contains("length=4\n"), "stdout: {out:?}");
    assert!(out.contains("has 10\n"), "stdout: {out:?}");
    assert!(out.contains("in 3\n"), "stdout: {out:?}");
    assert!(out.contains("no 5\n"), "stdout: {out:?}");
}

#[test]
fn str_set_keys_on_content_not_pointer() {
    // Two separately-allocated strings with equal content must collide —
    // the canonical key is the string content, not the heap pointer.
    let src = "\
fn main() -> i32:
    s: Set[str] = {\"a\", \"b\"}
    s.add(\"a\" + \"\")
    println(\"len=\" + str(len(s)))
    probe: str = \"\" + \"b\"
    if probe in s:
        println(\"has b\")
    if \"z\" not in s:
        println(\"no z\")
    return 0
";
    let p = compile_snippet("str_content", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("len=2\n"), "stdout: {out:?}");
    assert!(out.contains("has b\n"), "stdout: {out:?}");
    assert!(out.contains("no z\n"), "stdout: {out:?}");
}

#[test]
fn unsuffixed_int_literals_default_to_i64() {
    // `{0}` must synthesise Set[i64] (guide §2: `42` defaults to i64) and an
    // i64 annotation must accept unsuffixed elements.
    let src = "\
fn main() -> i32:
    s: Set[i64] = {0, 1, 2}
    s.add(7)
    println(\"len=\" + str(len(s)))
    if 2 in {1, 2}:
        println(\"inline ok\")
    return 0
";
    let p = compile_snippet("i64_default", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("len=4\n"), "stdout: {out:?}");
    assert!(out.contains("inline ok\n"), "stdout: {out:?}");
}

#[test]
fn float_set_keys_on_value_bits() {
    let src = "\
fn main() -> i32:
    f: Set[f64] = {0.5, 1.5, 0.5}
    println(\"len=\" + str(f.length()))
    if 1.5 in f:
        println(\"has 1.5\")
    if 2.5 not in f:
        println(\"no 2.5\")
    return 0
";
    let p = compile_snippet("float_ops", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("len=2\n"), "stdout: {out:?}");
    assert!(out.contains("has 1.5\n"), "stdout: {out:?}");
    assert!(out.contains("no 2.5\n"), "stdout: {out:?}");
}

#[test]
fn set_comprehension_builds_real_contents() {
    // {x % 2 for x in [1,2,3,4]} == {0, 1}: duplicates collapse and the
    // result answers membership queries (no more placeholder lowering).
    let src = "\
fn main() -> i32:
    xs: List[i64] = [1i64, 2i64, 3i64, 4i64]
    s: Set[i64] = {x % 2i64 for x: i64 in xs}
    println(\"len=\" + str(len(s)))
    if 0i64 in s and 1i64 in s:
        println(\"both present\")
    if 2i64 not in s:
        println(\"no 2\")
    return 0
";
    let p = compile_snippet("comprehension", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("len=2\n"), "stdout: {out:?}");
    assert!(out.contains("both present\n"), "stdout: {out:?}");
    assert!(out.contains("no 2\n"), "stdout: {out:?}");
}

#[test]
fn add_with_mismatched_element_type_is_rejected() {
    let src = "\
fn main() -> i32:
    s: Set[i64] = {1i64}
    s.add(\"x\")
    return 0
";
    let err = compile_source("set_bad_add.spy".to_string(), src)
        .expect_err("str element into Set[i64] must be a compile error");
    let msg = err.to_string();
    assert!(msg.contains("E2001"), "unexpected error: {msg}");
}

#[test]
fn membership_probe_type_is_checked_against_element_type() {
    // SetHas canonicalises by the static element type, so a mismatched probe
    // would silently answer false at runtime — it must be a compile error.
    let src = "\
fn main() -> i32:
    s: Set[i64] = {1i64}
    if \"a\" in s:
        println(\"oops\")
    return 0
";
    let err = compile_source("set_bad_probe.spy".to_string(), src)
        .expect_err("str probe into Set[i64] must be a compile error");
    let msg = err.to_string();
    assert!(msg.contains("E2001"), "unexpected error: {msg}");
}

#[test]
fn non_hashable_element_types_are_rejected() {
    // Class elements would fall back to pointer-identity keying; the
    // typechecker rejects them at set-construction time.
    let src = "\
class P:
    x: i32
    fn __init__(self, x: i32) -> None:
        self.x = x

fn main() -> i32:
    s: Set[P] = {P(1i32)}
    return 0
";
    let err = compile_source("set_bad_elem.spy".to_string(), src)
        .expect_err("class elements must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("Set elements must be"),
        "unexpected error: {msg}"
    );
}
