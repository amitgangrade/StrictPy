//! Native string methods (P1): strip/lstrip/rstrip, find, replace,
//! startswith/endswith/contains. Each is one Rust call instead of a hand-rolled
//! O(n) per-char bytecode loop.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn run(name: &str, src: &str) -> (i32, String) {
    let bytes = compile_source(format!("{name}.spy"), src)
        .unwrap_or_else(|e| panic!("{name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_strmethods_{name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    run_file_capture(&out).expect("run")
}

#[test]
fn strip_family() {
    let src = "\
fn main() -> i32:
    println(\"[\" + \"  hi there  \".strip() + \"]\")
    println(\"[\" + \"\\t\\n x \\r\".strip() + \"]\")
    println(\"[\" + \"  ab  \".lstrip() + \"]\")
    println(\"[\" + \"  ab  \".rstrip() + \"]\")
    println(\"[\" + \"\".strip() + \"]\")
    println(\"[\" + \"nospace\".strip() + \"]\")
    return 0
";
    let (code, out) = run("strip", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "[hi there]\n[x]\n[ab  ]\n[  ab]\n[]\n[nospace]\n", "{out:?}");
}

#[test]
fn find_returns_codepoint_index_or_minus_one() {
    let src = "\
fn main() -> i32:
    println(str(\"hello world\".find(\"world\")))   # 6
    println(str(\"hello\".find(\"z\")))              # -1
    println(str(\"hello\".find(\"\")))               # 0
    println(str(\"héllo\".find(\"llo\")))            # 2 (code points)
    return 0
";
    let (code, out) = run("find", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "6\n-1\n0\n2\n", "{out:?}");
}

#[test]
fn replace_all_occurrences() {
    let src = "\
fn main() -> i32:
    println(\"a,b,c\".replace(\",\", \"-\"))
    println(\"aaa\".replace(\"a\", \"bb\"))
    println(\"no match\".replace(\"x\", \"y\"))
    return 0
";
    let (code, out) = run("replace", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "a-b-c\nbbbbbb\nno match\n", "{out:?}");
}

#[test]
fn startswith_endswith_contains() {
    let src = "\
fn b(x: bool) -> str:
    if x:
        return \"T\"
    return \"F\"

fn main() -> i32:
    println(b(\"Content-Length: 18\".startswith(\"Content-Length\")))  # T
    println(b(\"Content-Length: 18\".startswith(\"Host\")))            # F
    println(b(\"file.json\".endswith(\".json\")))                      # T
    println(b(\"file.json\".endswith(\".xml\")))                       # F
    println(b(\"hello world\".contains(\"o w\")))                      # T
    println(b(\"hello\".contains(\"z\")))                              # F
    return 0
";
    let (code, out) = run("predicates", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "T\nF\nT\nF\nT\nF\n", "{out:?}");
}

#[test]
fn idiomatic_header_parse() {
    // A small HTTP-header parse using the new methods (no hand-rolled loops).
    let src = "\
fn main() -> i32:
    raw: str = \"POST /x HTTP/1.1\\nContent-Length: 18\\nHost: localhost\\n\\nbody\"
    lines: List[str] = raw.split(\"\\n\")
    clen: i64 = -1
    i: i64 = 1
    while i < i64(len(lines)):
        line: str = lines[i].strip()
        if line.startswith(\"Content-Length:\"):
            clen = i64(len(line))
        i = i + 1
    println(str(clen))   # len of 'Content-Length: 18' = 18
    return 0
";
    let (code, out) = run("hdr", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "18\n", "{out:?}");
}
