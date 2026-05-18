//! Integration test for examples/levenshtein.spy.
//!
//! Compiles the program, runs it through the VM with stdout captured,
//! and asserts:
//!   - exits 0
//!   - all 6 hardcoded cases pass (the program itself reports
//!     "summary: all 6 cases passed" iff every result matched)
//!   - the canonical kitten->sitting line is present with the right
//!     answer (= 3), confirming the DP filled in correctly
//!   - the empty-string cases produce the expected lengths
//!   - the identity case prints `= 0`
//!   - no individual case is tagged FAIL

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
fn levenshtein_matches_known_distances() {
    let src_path = project_root().join("examples").join("levenshtein.spy");
    let src = fs::read_to_string(&src_path).expect("read levenshtein.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile levenshtein.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("levenshtein.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run levenshtein");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // Canonical Wikipedia example.
    assert!(
        out.contains("\"kitten\" -> \"sitting\" = 3"),
        "kitten->sitting result missing or wrong:\n{out}"
    );
    // Empty-string handling, both directions. These exercise the base
    // row/column fill without ever indexing the empty string.
    assert!(
        out.contains("\"\" -> \"abc\" = 3"),
        "empty->abc result missing or wrong:\n{out}"
    );
    assert!(
        out.contains("\"abc\" -> \"\" = 3"),
        "abc->empty result missing or wrong:\n{out}"
    );
    // Anagram-ish short pair.
    assert!(
        out.contains("\"flaw\" -> \"lawn\" = 2"),
        "flaw->lawn result missing or wrong:\n{out}"
    );
    // Longer pair — exercises a meaningful DP fill.
    assert!(
        out.contains("\"intention\" -> \"execution\" = 5"),
        "intention->execution result missing or wrong:\n{out}"
    );
    // Identity case.
    assert!(
        out.contains("\"same\" -> \"same\" = 0"),
        "identity case missing or wrong:\n{out}"
    );

    // The runner appends `(expected N, ok)` or `(expected N, FAIL)` to
    // each line. No FAIL tag should appear in the entire output.
    assert!(
        !out.contains("FAIL"),
        "at least one case was tagged FAIL:\n{out}"
    );

    // And the program's own summary should confirm.
    assert!(
        out.contains("summary: all 6 cases passed"),
        "missing all-passed summary:\n{out}"
    );
}
