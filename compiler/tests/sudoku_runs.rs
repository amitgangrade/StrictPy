//! Integration test for examples/sudoku.spy.
//!
//! Compiles the program, runs it through the VM with stdout captured,
//! and asserts:
//!   - exits 0
//!   - prints both "puzzle:" and "solution:" headers
//!   - the solved board's first row is `5 3 4 6 7 8 9 1 2` (the unique
//!     solution of the Wikipedia easy puzzle baked into the example)
//!   - the solution does not contain any blanks ('.' beyond the puzzle
//!     section)
//!   - the 3x3-box separator bars are present

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
fn sudoku_solves_classic_easy_puzzle() {
    let src_path = project_root().join("examples").join("sudoku.spy");
    let src = fs::read_to_string(&src_path).expect("read sudoku.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile sudoku.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("sudoku.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run sudoku");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    assert!(out.contains("puzzle:"), "missing puzzle header:\n{out}");
    assert!(out.contains("solution:"), "missing solution header:\n{out}");
    assert!(
        out.contains("------+-------+------"),
        "missing 3x3 box separator bars:\n{out}"
    );

    // The Wikipedia easy puzzle has a unique solution starting:
    //   5 3 4 | 6 7 8 | 9 1 2
    assert!(
        out.contains("row0=534678912"),
        "first-row signature missing — solver produced wrong board:\n{out}"
    );

    // The solution section (everything after "solution:") must contain
    // no blank cells. We check by isolating the solution block via the
    // `row0=` marker that terminates it.
    let sol_start = out.find("solution:").expect("solution header");
    let sig_pos = out.find("row0=").expect("row0 marker");
    let sol_block = &out[sol_start..sig_pos];
    assert!(
        !sol_block.contains(". "),
        "solution block still contains blanks (`. `):\n{sol_block}"
    );

    // Spot-check a known cell deep in the solved board. Row index 4,
    // col index 4 of the canonical solution is `5`.
    // The solution prints rows in order; we look for the row containing
    // the centre value alongside its neighbours.
    // Full solution row 4 is: 4 2 6 | 8 5 3 | 7 9 1
    assert!(
        sol_block.contains("4 2 6 | 8 5 3 | 7 9 1"),
        "middle row mismatch:\n{sol_block}"
    );
}
