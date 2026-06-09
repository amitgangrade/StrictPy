//! In-place string-append optimisation (`s = s + e` / `s += e` on a
//! uniquely-owned `str` local lowers to `StrAppendInPlace`, amortised O(N)).
//!
//! The heavy emphasis here is **alias safety**: the compiler only emits the
//! in-place mutation when escape analysis proves `s` is never aliased. These
//! tests confirm both that the fast path is correct AND that any aliasing /
//! escape disqualifies it (so a shared string is never mutated underneath
//! another reference).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn run(name: &str, src: &str) -> (i32, String) {
    let bytes = compile_source(format!("{name}.spy"), src)
        .unwrap_or_else(|e| panic!("{name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_strinplace_{name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    run_file_capture(&out).expect("run")
}

#[test]
fn pure_accumulator_is_correct() {
    // Optimised path (s only self-appended + returned).
    let src = "\
fn build(n: i64) -> str:
    s: str = \"\"
    i: i64 = 0
    while i < n:
        s = s + \"ab\"
        i = i + 1
    return s

fn main() -> i32:
    r: str = build(4)
    println(r)                 # abababab
    println(str(len(r)))       # 8
    return 0
";
    let (code, out) = run("pure", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "abababab\n8\n", "{out:?}");
}

#[test]
fn aug_assign_form_is_correct() {
    let src = "\
fn main() -> i32:
    s: str = \"\"
    i: i64 = 0
    while i < 3:
        s += \"xy\"
        i = i + 1
    println(s)                 # xyxyxy
    return 0
";
    let (code, out) = run("aug", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "xyxyxy\n", "{out:?}");
}

#[test]
fn aliasing_disqualifies_inplace_and_stays_correct() {
    // `t = a` aliases the accumulator, so the optimisation must NOT fire;
    // appending to `a` afterwards must leave `t` untouched.
    let src = "\
fn main() -> i32:
    a: str = \"\"
    a = a + \"xy\"
    a = a + \"xy\"
    t: str = a
    a = a + \"ZZ\"
    println(t)                 # xyxy   (alias unchanged)
    println(a)                 # xyxyZZ
    return 0
";
    let (code, out) = run("alias", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "xyxy\nxyxyZZ\n", "{out:?}");
}

#[test]
fn list_capture_disqualifies_and_stays_correct() {
    // Storing the accumulator into a list captures it; later appends must not
    // mutate the captured copy.
    let src = "\
fn main() -> i32:
    s: str = \"\"
    s = s + \"a\"
    snapshots: List[str] = []
    snapshots.append(s)
    s = s + \"b\"
    snapshots.append(s)
    s = s + \"c\"
    println(snapshots[0])      # a
    println(snapshots[1])      # ab
    println(s)                 # abc
    return 0
";
    let (code, out) = run("listcap", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "a\nab\nabc\n", "{out:?}");
}

#[test]
fn passing_to_function_disqualifies_and_stays_correct() {
    // Passing `s` to a function could alias it; the value seen by the callee
    // must reflect the string at call time and not change under later appends.
    let src = "\
fn tag(x: str) -> str:
    return \"[\" + x + \"]\"

fn main() -> i32:
    s: str = \"\"
    s = s + \"p\"
    first: str = tag(s)
    s = s + \"q\"
    println(first)             # [p]
    println(tag(s))            # [pq]
    return 0
";
    let (code, out) = run("passfn", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "[p]\n[pq]\n", "{out:?}");
}

#[test]
fn non_ascii_append_is_correct() {
    let src = "\
fn main() -> i32:
    u: str = \"\"
    u = u + \"é\"
    u = u + \"ñ\"
    u = u + \"z\"
    println(u)                 # éñz
    println(str(len(u)))       # 3 (code points)
    return 0
";
    let (code, out) = run("unicode", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "éñz\n3\n", "{out:?}");
}

#[test]
fn two_independent_accumulators() {
    let src = "\
fn main() -> i32:
    a: str = \"\"
    b: str = \"\"
    i: i64 = 0
    while i < 3:
        a = a + \"a\"
        b = b + \"bb\"
        i = i + 1
    println(a + \"|\" + b)      # aaa|bbbbbb
    return 0
";
    let (code, out) = run("two", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "aaa|bbbbbb\n", "{out:?}");
}

#[test]
fn large_build_is_correct() {
    // The amortised-O(N) path over many appends; assert the final length.
    let src = "\
fn main() -> i32:
    s: str = \"\"
    i: i64 = 0
    while i < 20000:
        s = s + \"a\"
        i = i + 1
    println(str(len(s)))       # 20000
    return 0
";
    let (code, out) = run("large", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "20000\n", "{out:?}");
}
