#![cfg(feature = "graphics")]
//! M52 integration tests for the `gfx` module.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m52_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn test_gfx_core_suite() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    std::env::set_var("SDL_AUDIODRIVER", "dummy");

    // 1. test_gfx_init_and_window_size
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    win: Window = gfx.create_window(\"Test Window\", 320i32, 240i32)
    size: Tuple[i32, i32] = gfx.window_size(win)
    w: i32 = size.0
    h: i32 = size.1
    println(\"w=\" + str(w) + \", h=\" + str(h))
    gfx.close_window(win)
    return 0
";
        let p = compile_snippet("init_size", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
        assert!(out.contains("w=320, h=240"), "expected size 320x240; got: {out:?}");
    }

    // 2. test_gfx_drawing_primitives
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    win: Window = gfx.create_window(\"Test Drawing\", 100i32, 100i32)
    gfx.clear(win, 0i32, 0i32, 0i32)
    gfx.draw_rect(win, 10i32, 10i32, 20i32, 20i32, 255i32, 0i32, 0i32, 255i32)
    gfx.draw_rect_outline(win, 5i32, 5i32, 30i32, 30i32, 0i32, 255i32, 0i32, 255i32)
    gfx.draw_line(win, 0i32, 0i32, 100i32, 100i32, 0i32, 0i32, 255i32, 255i32)
    gfx.draw_point(win, 50i32, 50i32, 255i32, 255i32, 255i32, 255i32)
    gfx.present(win)
    gfx.close_window(win)
    return 0
";
        let p = compile_snippet("drawing", src);
        let (code, _out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
    }

    // 3. test_gfx_poll_event_empty
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    win: Window = gfx.create_window(\"Test Event\", 100i32, 100i32)
    ev: Event? = gfx.poll_event(win)
    if ev is none:
        println(\"polled: none\")
    else:
        println(\"polled: \" + ev.kind)
    gfx.close_window(win)
    return 0
";
        let p = compile_snippet("event_empty", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
        assert!(out.contains("polled:"), "expected polled status; got: {out:?}");
    }

    // 4. test_gfx_set_window_title
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    win: Window = gfx.create_window(\"Original Title\", 100i32, 100i32)
    gfx.set_window_title(win, \"New Title\")
    gfx.close_window(win)
    return 0
";
        let p = compile_snippet("title", src);
        let (code, _out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
    }

    // 5. test_gfx_double_close_raises_value_error
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    win: Window = gfx.create_window(\"Close Test\", 100i32, 100i32)
    gfx.close_window(win)
    try:
        gfx.close_window(win)
        println(\"UNREACHED\")
    except ValueError as e:
        println(\"caught ValueError\")
    return 0
";
        let p = compile_snippet("double_close", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
        assert!(out.contains("caught ValueError"), "expected ValueError on double close; got: {out:?}");
    }
}
