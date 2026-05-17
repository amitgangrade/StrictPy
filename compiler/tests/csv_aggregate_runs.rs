//! Integration test for the CSV aggregator example.
//!
//! Compiles `examples/csv_aggregate.spy`, materialises the sample CSV in
//! the cwd the program reads from, runs the .spyc through the VM's
//! `run_file_capture`, and asserts that the printed output contains the
//! expected per-category totals.

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
fn csv_aggregate_groups_and_sums_amounts() {
    // Compile the example to a temp .spyc.
    let src_path = project_root().join("examples").join("csv_aggregate.spy");
    let src = fs::read_to_string(&src_path).expect("read csv_aggregate.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile csv_aggregate.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("csv_aggregate.spyc");
    fs::write(&spyc_path, &bytes).expect("write csv_aggregate.spyc");

    // Materialise the CSV in a unique temp dir so we don't race with other
    // tests that chdir, and so the relative `sample.csv` lookup the program
    // does actually finds the right file.
    let work_dir = std::env::temp_dir().join("strictpy_csv_aggregate_test");
    let _ = fs::remove_dir_all(&work_dir);
    fs::create_dir_all(&work_dir).expect("mkdir work dir");
    let csv = "category,amount\n\
               groceries,12.50\n\
               gas,45.00\n\
               groceries,8.75\n\
               restaurants,32.00\n\
               gas,38.50\n\
               groceries,15.00\n\
               restaurants,18.75\n";
    // The string continuation above includes leading whitespace on each
    // continuation line, so strip them per-line before writing.
    let cleaned: String = csv
        .lines()
        .map(|l| l.trim_start().to_string())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(work_dir.join("sample.csv"), cleaned).expect("write sample.csv");

    // chdir so the example's `open(\"sample.csv\")` lookup hits our file.
    // This serialises with any other test that chdirs (e.g. wordcount); we
    // restore cwd before asserting so a failure can still find files.
    let original = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(&work_dir).expect("chdir to work dir");
    let outcome = std::panic::catch_unwind(|| run_file_capture(&spyc_path));
    let _ = std::env::set_current_dir(original);

    let result = outcome.expect("compile/run did not panic");
    let (code, out) = result.expect("csv_aggregate must run cleanly");
    assert_eq!(code, 0, "exit code; stdout was: {out:?}");

    // The header should be the first line.
    assert!(
        out.starts_with("category,total"),
        "missing header in stdout: {out:?}"
    );

    // Each category should appear with its summed total. We assert the
    // numbers liberally because StrictPy's `str(f64)` is shortest-round-trip
    // (83.5 not 83.50), and floating-point sums can have a tiny epsilon.
    assert!(out.contains("gas,"), "missing `gas`: {out:?}");
    assert!(out.contains("groceries,"), "missing `groceries`: {out:?}");
    assert!(out.contains("restaurants,"), "missing `restaurants`: {out:?}");

    // Numeric totals: 45.00 + 38.50 = 83.5; 12.50 + 8.75 + 15.00 = 36.25;
    // 32.00 + 18.75 = 50.75.
    assert!(out.contains("83.5"), "gas total missing: {out:?}");
    assert!(out.contains("36.25"), "groceries total missing: {out:?}");
    assert!(out.contains("50.75"), "restaurants total missing: {out:?}");

    // Sort order: alphabetical by category. `gas` < `groceries` < `restaurants`.
    let gas_pos = out.find("gas,").expect("gas line");
    let groc_pos = out.find("groceries,").expect("groceries line");
    let rest_pos = out.find("restaurants,").expect("restaurants line");
    assert!(
        gas_pos < groc_pos && groc_pos < rest_pos,
        "lines not alphabetically sorted: {out:?}"
    );
}
