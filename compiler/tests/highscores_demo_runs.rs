//! Compile-only test for `examples/games/highscores.spy` (M58).
//!
//! `highscores.spy` is the canonical reference for the shared high-score
//! persistence logic that the three games (Snake / Tetris / Space Shooter)
//! inline (StrictPy v0.2 has no user-module import, so each game carries
//! its own copy).  This test verifies the reference module typechecks +
//! bytecode-compiles.  Its `main()` exercises save_score + top_scores
//! against a real sqlite file when run by hand; CI only compiles it.
//!
//! Pattern matches `snake_demo_runs::snake_demo_compiles`.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn highscores_demo_compiles() {
    let src_path = project_root()
        .join("examples")
        .join("games")
        .join("highscores.spy");
    let src = fs::read_to_string(&src_path).expect("read highscores.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile highscores.spy: {e}"));
}
