//! Regression: string relational operators `<` / `<=` / `>` / `>=`.
//!
//! These had no `is_str` branch in the IR binop lowering and fell through to
//! the integer `ILt`/`ILe`/`IGt`/`IGe`, which compared the two string heap
//! pointers as `i64` — meaningless for content ordering (same bug class as
//! BUG-034 `str !=`, BUG-008 `is not`, BUG-039 `in`). Surfaced by the M63b
//! bounded-generics demo (`Pair2[str]`). Fixed by the `StrCmp` native +
//! lowering `a <relop> b` as `StrCmp(a, b) <relop> 0`.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn run(name: &str, src: &str) -> (i32, String) {
    let bytes = compile_source(format!("{name}.spy"), src)
        .unwrap_or_else(|e| panic!("{name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_strord_{name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    run_file_capture(&out).expect("run")
}

#[test]
fn str_relational_operators_compare_by_content() {
    // Each line prints "T" or "F" for the operator's result.
    let src = "\
fn b(x: bool) -> str:
    if x:
        return \"T\"
    return \"F\"

fn main() -> i32:
    lo: str = \"aaa\"
    hi: str = \"zzz\"
    eq: str = \"aaa\"
    # <
    println(b(lo < hi))   # T
    println(b(hi < lo))   # F
    println(b(lo < eq))   # F (equal)
    # <=
    println(b(lo <= hi))  # T
    println(b(hi <= lo))  # F
    println(b(lo <= eq))  # T (equal)
    # >
    println(b(hi > lo))   # T
    println(b(lo > hi))   # F
    println(b(lo > eq))   # F (equal)
    # >=
    println(b(hi >= lo))  # T
    println(b(lo >= hi))  # F
    println(b(lo >= eq))  # T (equal)
    # prefix ordering: \"ab\" < \"abc\"
    println(b(\"ab\" < \"abc\"))   # T
    println(b(\"abc\" < \"ab\"))   # F
    return 0
";
    let (code, out) = run("str_relational", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(
        out,
        "T\nF\nF\nT\nF\nT\nT\nF\nF\nT\nF\nT\nT\nF\n",
        "string relational results: {out:?}"
    );
}
