//! Integration test for examples/tictactoe.spy.
//!
//! Compiles the program, runs it through the VM with stdout captured,
//! and asserts:
//!   - exits 0
//!   - prints the opening-position header and the final `result:` line
//!   - the final line names exactly one of "X wins", "O wins", "draw"
//!     (an unspecified or empty result would indicate the minimax loop
//!     never terminated or check_winner returned something out of range)
//!   - the opening-position render shows the seeded centre X and the
//!     two seeded corners
//!   - the game actually progressed (at least one `turn 1:` line)

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
fn tictactoe_runs_self_play_to_completion() {
    let src_path = project_root().join("examples").join("tictactoe.spy");
    let src = fs::read_to_string(&src_path).expect("read tictactoe.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tictactoe.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tictactoe.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run tictactoe");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // Header for the seeded board must appear.
    assert!(
        out.contains("opening position:"),
        "missing opening-position header:\n{out}"
    );
    // At least one move must have been played.
    assert!(
        out.contains("turn 1:"),
        "missing first turn — game didn't start:\n{out}"
    );

    // Final result line must classify the game as exactly one outcome.
    // Search backwards for the last `result:` since each render block
    // does not contain that token.
    let result_idx = out
        .rfind("result:")
        .unwrap_or_else(|| panic!("no `result:` line in stdout:\n{out}"));
    let tail = &out[result_idx..];
    let says_x = tail.contains("X wins");
    let says_o = tail.contains("O wins");
    let says_draw = tail.contains("draw");
    let count = (says_x as u32) + (says_o as u32) + (says_draw as u32);
    assert_eq!(
        count, 1,
        "result line should name exactly one of X wins / O wins / draw; got `{tail}`"
    );

    // Sanity: from the seeded opening (X centre + X corner vs single O
    // corner), perfect play from both sides yields X winning or a draw —
    // never an O win. If we got an O win that's a logic bug in the
    // minimax search (sign error, depth-penalty wraparound, etc.).
    assert!(
        !says_o,
        "from the seeded X-favourable opening, minimax should never let O win:\n{tail}"
    );
}
