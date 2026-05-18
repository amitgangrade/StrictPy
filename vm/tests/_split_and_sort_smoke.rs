//! End-to-end smoke for str.split + sorted(). Compiles a tiny inline
//! program that exercises both new natives and asserts the printed
//! output matches the expected sorted lines.
//!
//! real-world: csv_aggregate / wordcount / markov / kvstore all want
//! split + sort. This is the smallest possible reproducer that
//! covers both natives reaching the VM and round-tripping the data.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_inline(name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(name.into(), src)
        .unwrap_or_else(|e| panic!("compile {name}: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_inline_{}.spyc", name.replace('.', "_")));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn split_then_sort_then_for_iterate() {
    let src = r#"
fn main() -> i32:
    parts: List[str] = "banana,apple,cherry".split(",")
    parts.sort()
    for p: str in parts:
        println(p)
    return 0
"#;
    let p = compile_inline("split_sort.spy", src);
    let (code, out) = run_file_capture(&p).expect("split_sort.spy must run cleanly");
    assert_eq!(code, 0, "exit code; stdout was: {out:?}");
    // After sort the order should be: apple, banana, cherry.
    let lines: Vec<&str> = out.lines().collect();
    assert!(
        lines.iter().any(|l| *l == "apple"),
        "missing apple in {out:?}",
    );
    assert!(
        lines.iter().any(|l| *l == "banana"),
        "missing banana in {out:?}",
    );
    assert!(
        lines.iter().any(|l| *l == "cherry"),
        "missing cherry in {out:?}",
    );
    // Verify the relative order — sort must place apple before banana
    // before cherry.
    let pos_a = out.find("apple").expect("apple");
    let pos_b = out.find("banana").expect("banana");
    let pos_c = out.find("cherry").expect("cherry");
    assert!(pos_a < pos_b && pos_b < pos_c, "wrong sort order: {out:?}");
}

#[test]
fn sorted_returns_copy_does_not_mutate_input() {
    let src = r#"
fn main() -> i32:
    xs: List[i64] = [3, 1, 2]
    ys: List[i64] = sorted(xs)
    # xs must be unchanged; ys must be sorted.
    for v: i64 in xs:
        println("x=" + str(v))
    for v: i64 in ys:
        println("y=" + str(v))
    return 0
"#;
    let p = compile_inline("sorted_copy.spy", src);
    let (code, out) = run_file_capture(&p).expect("must run cleanly");
    assert_eq!(code, 0, "stdout was: {out:?}");
    // Original order: 3, 1, 2.
    let xs_lines: Vec<&str> = out
        .lines()
        .filter(|l| l.starts_with("x="))
        .collect();
    assert_eq!(xs_lines, vec!["x=3", "x=1", "x=2"], "xs mutated: {out:?}");
    // Sorted: 1, 2, 3.
    let ys_lines: Vec<&str> = out
        .lines()
        .filter(|l| l.starts_with("y="))
        .collect();
    assert_eq!(ys_lines, vec!["y=1", "y=2", "y=3"], "ys not sorted: {out:?}");
}
