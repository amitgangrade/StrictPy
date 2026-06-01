//! M58 integration tests for the gfx polish natives
//! (`gfx.set_fullscreen` / `gfx.set_vsync`).
//!
//! Headless: SDL is driven against the `dummy` video driver so window
//! creation succeeds and fullscreen/vsync toggles run their code paths
//! without an actual display.  We assert the snippet exits 0 (no crash /
//! no raised ValueError).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m58_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn test_gfx_polish_suite() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    std::env::set_var("SDL_AUDIODRIVER", "dummy");

    // set_fullscreen(true) then (false), and set_vsync(true) then (false).
    let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    win: Window = gfx.create_window(\"M58 Polish\", 200i32, 150i32)
    gfx.set_fullscreen(win, true)
    gfx.set_fullscreen(win, false)
    gfx.set_vsync(win, true)
    gfx.set_vsync(win, false)
    gfx.clear(win, 0i32, 0i32, 0i32)
    gfx.present(win)
    gfx.close_window(win)
    println(\"polish ok\")
    return 0
";
    let p = compile_snippet("polish", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "polish snippet should exit 0; stdout: {out:?}");
    assert!(out.contains("polish ok"), "expected completion marker; got: {out:?}");
}
