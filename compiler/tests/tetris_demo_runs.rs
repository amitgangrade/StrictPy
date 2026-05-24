//! Compile-only test for `examples/games/tetris.spy` (M56).
//!
//! Tetris boots an SDL window and waits for keyboard input until R/Esc
//! or the window's close button — not runnable end-to-end from cargo
//! test.  This verifies the typechecker is happy + bytecode generator
//! emits a valid `.spyc`.  Manual gameplay testing happens outside CI.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn tetris_demo_compiles() {
    let src_path = project_root()
        .join("examples")
        .join("games")
        .join("tetris.spy");
    let src = fs::read_to_string(&src_path).expect("read tetris.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tetris.spy: {e}"));
}
