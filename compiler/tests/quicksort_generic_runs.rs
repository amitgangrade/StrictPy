//! M17 acceptance test: `examples/quicksort_generic.spy` — one generic
//! `partition[T]` + `quicksort[T]` body, sorted twice from `main()`:
//! once over `List[i64]` (instantiation `quicksort__list_i64_i64_i64`,
//! `partition__list_i64_i64_i64`) and once over `List[f64]`. The
//! comparison ops (`<`) bind to integer / float instructions per
//! instantiation, which is the whole point of monomorphisation — before
//! M17 the IR lowered `xs[j] < pivot` once with `Ty::Var(0)`, fell back
//! to `ILt`, and silently mis-sorted floats.
//!
//! Asserts:
//!   - the program exits 0,
//!   - the i64 list comes out sorted ascending,
//!   - the f64 list comes out sorted ascending,
//!   - the divider `---` separates the two outputs.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn quicksort_generic_compiles() {
    let src_path = project_root().join("examples").join("quicksort_generic.spy");
    let src = fs::read_to_string(&src_path).expect("read quicksort_generic.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile quicksort_generic.spy: {e}"));
}

#[test]
fn quicksort_generic_sorts_both_i64_and_f64_from_one_body() {
    let src_path = project_root().join("examples").join("quicksort_generic.spy");
    let src = fs::read_to_string(&src_path).expect("read quicksort_generic.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile quicksort_generic.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("quicksort_generic.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run quicksort_generic");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // Split on the `---` divider into the two sorted runs.
    let parts: Vec<&str> = out.split("---\n").collect();
    assert_eq!(parts.len(), 2, "expected two segments separated by ---, got:\n{out}");

    let ints: Vec<i64> = parts[0]
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.parse::<i64>().expect("parse i64"))
        .collect();
    assert_eq!(
        ints,
        vec![0i64, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        "i64 sort came out wrong: {ints:?}\nstdout:\n{out}"
    );

    let floats: Vec<f64> = parts[1]
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.parse::<f64>().expect("parse f64"))
        .collect();
    assert_eq!(floats.len(), 6, "expected 6 floats, got {floats:?}");
    for w in floats.windows(2) {
        assert!(
            w[0] <= w[1],
            "f64 sort not ascending: {:?} before {:?}; full: {floats:?}",
            w[0], w[1]
        );
    }
}
