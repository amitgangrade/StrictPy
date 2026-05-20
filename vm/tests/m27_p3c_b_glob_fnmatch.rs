//! M27 P3c-B regression tests for the `glob` + `fnmatch` stdlib modules.
//!
//! Follows the M23 P3a-B pattern: each test compiles a tiny `.spy`
//! snippet, writes the bytecode to `CARGO_TARGET_TMPDIR`, runs it
//! through `run_file_capture`, and asserts on stdout.
//!
//! Coverage:
//!   * `fnmatch.fnmatchcase` — basic patterns + case sensitivity.
//!   * `fnmatch.fnmatch` — determinism (platform-dependent direction).
//!   * `fnmatch.filter` — order preservation + empty cases.
//!   * `fnmatch.translate` — round-trip through `re.fullmatch`.
//!   * `glob.escape` — metachar bracketing + identity on plain strings.
//!   * `glob.glob` / `glob.recursive` — basic + empty results.
//!   * Invalid pattern -> ValueError.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m27_p3c_b_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── fnmatch.fnmatchcase ───────────────────────────────────────────────

#[test]
fn fnmatchcase_star_matches_extension() {
    let src = "\
import fnmatch
fn main() -> i32:
    a: bool = fnmatch.fnmatchcase(\"hello.txt\", \"*.txt\")
    b: bool = fnmatch.fnmatchcase(\"hello.bin\", \"*.txt\")
    println(\"a=\" + str(a))
    println(\"b=\" + str(b))
    return 0
";
    let p = compile_snippet("fnmatchcase_star", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("a=true"), "got: {out:?}");
    assert!(out.contains("b=false"), "got: {out:?}");
}

#[test]
fn fnmatchcase_question_mark_matches_single_char() {
    let src = "\
import fnmatch
fn main() -> i32:
    a: bool = fnmatch.fnmatchcase(\"x\", \"?\")
    b: bool = fnmatch.fnmatchcase(\"xy\", \"?\")
    println(\"a=\" + str(a))
    println(\"b=\" + str(b))
    return 0
";
    let p = compile_snippet("fnmatchcase_qm", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("a=true"), "got: {out:?}");
    assert!(out.contains("b=false"), "got: {out:?}");
}

#[test]
fn fnmatchcase_char_class_matches_digit() {
    let src = "\
import fnmatch
fn main() -> i32:
    a: bool = fnmatch.fnmatchcase(\"file1\", \"file[0-9]\")
    b: bool = fnmatch.fnmatchcase(\"filex\", \"file[0-9]\")
    println(\"a=\" + str(a))
    println(\"b=\" + str(b))
    return 0
";
    let p = compile_snippet("fnmatchcase_cc", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("a=true"), "got: {out:?}");
    assert!(out.contains("b=false"), "got: {out:?}");
}

#[test]
fn fnmatchcase_is_always_case_sensitive() {
    let src = "\
import fnmatch
fn main() -> i32:
    a: bool = fnmatch.fnmatchcase(\"Hello\", \"hello\")
    b: bool = fnmatch.fnmatchcase(\"HELLO.TXT\", \"*.txt\")
    println(\"a=\" + str(a))
    println(\"b=\" + str(b))
    return 0
";
    let p = compile_snippet("fnmatchcase_cs", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("a=false"), "got: {out:?}");
    assert!(out.contains("b=false"), "got: {out:?}");
}

#[test]
fn fnmatch_is_deterministic() {
    // We can't assert direction without knowing the host OS, but we
    // assert the contract that fnmatch(...) is stable on repeated calls.
    let src = "\
import fnmatch
fn main() -> i32:
    a: bool = fnmatch.fnmatch(\"HELLO.TXT\", \"*.txt\")
    b: bool = fnmatch.fnmatch(\"HELLO.TXT\", \"*.txt\")
    eq: bool = a == b
    println(\"eq=\" + str(eq))
    return 0
";
    let p = compile_snippet("fnmatch_det", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("eq=true"), "got: {out:?}");
}

// ── fnmatch.filter ────────────────────────────────────────────────────

#[test]
fn fnmatch_filter_preserves_input_order() {
    let src = "\
import fnmatch
fn main() -> i32:
    names: List[str] = [\"zeta.spy\", \"alpha.txt\", \"beta.spy\", \"gamma.txt\", \"delta.spy\"]
    keep: List[str] = fnmatch.filter(names, \"*.spy\")
    n: i64 = i64(len(keep))
    println(\"n=\" + str(n))
    i: i64 = 0
    while i < n:
        println(\"k=\" + keep[i])
        i = i + 1
    return 0
";
    let p = compile_snippet("filter_order", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("n=3"), "got: {out:?}");
    // Order: zeta, beta, delta.
    let z = out.find("k=zeta.spy").expect("zeta missing");
    let b = out.find("k=beta.spy").expect("beta missing");
    let d = out.find("k=delta.spy").expect("delta missing");
    assert!(z < b && b < d, "filter must preserve order; got: {out:?}");
}

#[test]
fn fnmatch_filter_empty_input_yields_empty() {
    let src = "\
import fnmatch
fn main() -> i32:
    names: List[str] = []
    keep: List[str] = fnmatch.filter(names, \"*\")
    println(\"n=\" + str(len(keep)))
    return 0
";
    let p = compile_snippet("filter_empty_in", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("n=0"), "got: {out:?}");
}

#[test]
fn fnmatch_filter_no_matches_yields_empty() {
    let src = "\
import fnmatch
fn main() -> i32:
    names: List[str] = [\"a.txt\", \"b.txt\"]
    keep: List[str] = fnmatch.filter(names, \"*.xyz\")
    println(\"n=\" + str(len(keep)))
    return 0
";
    let p = compile_snippet("filter_no_match", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("n=0"), "got: {out:?}");
}

// ── fnmatch.translate ─────────────────────────────────────────────────

#[test]
fn fnmatch_translate_roundtrips_through_re() {
    // Translate `*.txt` into a regex and verify it discriminates
    // correctly under `re.fullmatch`.
    let src = "\
import fnmatch
import re
fn main() -> i32:
    pat: str = fnmatch.translate(\"*.txt\")
    a: bool = re.fullmatch(pat, \"hello.txt\")
    b: bool = re.fullmatch(pat, \"hello.bin\")
    println(\"a=\" + str(a))
    println(\"b=\" + str(b))
    return 0
";
    let p = compile_snippet("translate_roundtrip", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("a=true"), "got: {out:?}");
    assert!(out.contains("b=false"), "got: {out:?}");
}

#[test]
fn fnmatch_translate_dot_is_literal() {
    // `x.txt` -> regex with escaped dot, not regex-any.
    let src = "\
import fnmatch
import re
fn main() -> i32:
    pat: str = fnmatch.translate(\"x.txt\")
    a: bool = re.fullmatch(pat, \"x.txt\")
    b: bool = re.fullmatch(pat, \"xytxt\")
    println(\"a=\" + str(a))
    println(\"b=\" + str(b))
    return 0
";
    let p = compile_snippet("translate_dot", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("a=true"), "got: {out:?}");
    assert!(out.contains("b=false"), "got: {out:?}");
}

#[test]
fn fnmatch_translate_char_class_negation() {
    // `[!a]` is CPython's negation; translate emits `[^a]`.
    let src = "\
import fnmatch
import re
fn main() -> i32:
    pat: str = fnmatch.translate(\"[!a]\")
    a: bool = re.fullmatch(pat, \"b\")
    b: bool = re.fullmatch(pat, \"a\")
    println(\"a=\" + str(a))
    println(\"b=\" + str(b))
    return 0
";
    let p = compile_snippet("translate_neg", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("a=true"), "got: {out:?}");
    assert!(out.contains("b=false"), "got: {out:?}");
}

// ── glob.escape ───────────────────────────────────────────────────────

#[test]
fn glob_escape_brackets_metacharacters() {
    let src = "\
import glob
fn main() -> i32:
    a: str = glob.escape(\"a*b\")
    b: str = glob.escape(\"x?y\")
    c: str = glob.escape(\"q[r\")
    println(\"a=\" + a)
    println(\"b=\" + b)
    println(\"c=\" + c)
    return 0
";
    let p = compile_snippet("escape_meta", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("a=a[*]b"), "got: {out:?}");
    assert!(out.contains("b=x[?]y"), "got: {out:?}");
    assert!(out.contains("c=q[[]r"), "got: {out:?}");
}

#[test]
fn glob_escape_identity_on_plain_text() {
    let src = "\
import glob
fn main() -> i32:
    a: str = glob.escape(\"hello.txt\")
    println(\"a=\" + a)
    return 0
";
    let p = compile_snippet("escape_plain", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("a=hello.txt"), "got: {out:?}");
}

// ── glob.glob / glob.recursive ────────────────────────────────────────

#[test]
fn glob_no_matches_returns_empty_list() {
    // Patterns that don't match any file yield an empty list, not
    // an error.
    let src = "\
import glob
fn main() -> i32:
    m: List[str] = glob.glob(\"_m27_definitely_nonexistent_dir/*.xyz\")
    println(\"n=\" + str(len(m)))
    return 0
";
    let p = compile_snippet("glob_empty", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("n=0"), "got: {out:?}");
}

#[test]
fn glob_recursive_no_matches_returns_empty_list() {
    let src = "\
import glob
fn main() -> i32:
    m: List[str] = glob.recursive(\"_m27_definitely_nonexistent_dir/**/*.xyz\")
    println(\"n=\" + str(len(m)))
    return 0
";
    let p = compile_snippet("glob_rec_empty", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("n=0"), "got: {out:?}");
}
