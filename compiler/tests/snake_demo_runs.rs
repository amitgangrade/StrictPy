//! Compile-only test for `examples/games/snake.spy` (M55).
//!
//! The game itself is interactive — it boots an SDL window and waits
//! for keyboard input until the user presses Escape or the window's
//! close button, so it can't be run end-to-end from cargo test.  The
//! coverage we get here is: typechecker is happy + bytecode generator
//! emits a valid `.spyc`.  Manual smoke testing of actual gameplay
//! lives outside CI (run the binary yourself).
//!
//! Pattern matches `tabular_serve_demo_runs::tabular_serve_demo_compiles`.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn snake_demo_compiles() {
    let src_path = project_root()
        .join("examples")
        .join("games")
        .join("snake.spy");
    let src = fs::read_to_string(&src_path).expect("read snake.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile snake.spy: {e}"));
}
