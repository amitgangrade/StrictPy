//! Compile-only test for `examples/games/space_shooter.spy` (M57).
//!
//! Space Shooter boots an SDL window and runs an interactive ~60 FPS
//! loop until R/Esc or the window's close button — not runnable
//! end-to-end from cargo test.  This verifies the typechecker is happy
//! + the bytecode generator emits a valid `.spyc`.  Manual gameplay
//! testing happens outside CI.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn space_shooter_demo_compiles() {
    let src_path = project_root()
        .join("examples")
        .join("games")
        .join("space_shooter.spy");
    let src = fs::read_to_string(&src_path).expect("read space_shooter.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile space_shooter.spy: {e}"));
}
