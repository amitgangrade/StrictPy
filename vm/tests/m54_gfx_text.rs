#![cfg(feature = "graphics")]
//! M54 integration tests for the GFX font/text functions (SDL_ttf).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m54_text_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn test_gfx_text_suite() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    std::env::set_var("SDL_AUDIODRIVER", "dummy");

    // 1. load_font succeeds
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    font: Font = gfx.load_font(\"vm/tests/fixtures/games/DejaVuSansMono.ttf\", 24i32)
    println(\"load_font ok\")
    gfx.free_font(font)
    return 0
";
        let p = compile_snippet("load_font", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0, "load_font failed: {out:?}");
        assert!(out.contains("load_font ok"), "expected ok; got: {out:?}");
    }

    // 2. text_size returns non-zero width/height
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    font: Font = gfx.load_font(\"vm/tests/fixtures/games/DejaVuSansMono.ttf\", 24i32)
    sz: Tuple[i32, i32] = gfx.text_size(font, \"Hello\")
    w: i32 = sz.0
    h: i32 = sz.1
    println(\"w=\" + str(w) + \",h=\" + str(h))
    gfx.free_font(font)
    return 0
";
        let p = compile_snippet("text_size", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0, "text_size failed: {out:?}");
        // Both dimensions must be positive
        assert!(out.contains("w=") && out.contains(",h="), "output: {out:?}");
        // Parse w and h from output like "w=65,h=32"
        let parts: Vec<&str> = out.trim().lines().last().unwrap_or("").splitn(2, ',').collect();
        if parts.len() == 2 {
            let w: i32 = parts[0].trim_start_matches("w=").parse().unwrap_or(0);
            let h: i32 = parts[1].trim_start_matches("h=").parse().unwrap_or(0);
            assert!(w > 0, "text width should be > 0, got {w}");
            assert!(h > 0, "text height should be > 0, got {h}");
        }
    }

    // 3. draw_text doesn't crash (dummy driver, no visible output)
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    win: Window = gfx.create_window(\"Text Test\", 320i32, 240i32)
    font: Font = gfx.load_font(\"vm/tests/fixtures/games/DejaVuSansMono.ttf\", 24i32)
    gfx.clear(win, 0i32, 0i32, 0i32)
    gfx.draw_text(win, font, \"Hello World\", 10i32, 10i32, 255i32, 255i32, 255i32)
    gfx.present(win)
    println(\"draw_text ok\")
    gfx.free_font(font)
    gfx.close_window(win)
    return 0
";
        let p = compile_snippet("draw_text", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0, "draw_text failed: {out:?}");
        assert!(out.contains("draw_text ok"), "expected ok; got: {out:?}");
    }

    // 4. load_font on missing file raises IOError
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    try:
        font: Font = gfx.load_font(\"vm/tests/fixtures/games/no_such_font.ttf\", 24i32)
        println(\"UNREACHED\")
    except IOError as e:
        println(\"caught IOError\")
    return 0
";
        let p = compile_snippet("missing_font", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
        assert!(out.contains("caught IOError"), "expected IOError on missing font; got: {out:?}");
    }

    // 5. free_font + use after free raises ValueError
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    font: Font = gfx.load_font(\"vm/tests/fixtures/games/DejaVuSansMono.ttf\", 24i32)
    gfx.free_font(font)
    try:
        sz: Tuple[i32, i32] = gfx.text_size(font, \"Hello\")
        println(\"UNREACHED\")
    except ValueError as e:
        println(\"caught ValueError\")
    return 0
";
        let p = compile_snippet("font_use_after_free", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
        assert!(out.contains("caught ValueError"), "expected ValueError on use-after-free; got: {out:?}");
    }

    // 6. double free raises ValueError
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    font: Font = gfx.load_font(\"vm/tests/fixtures/games/DejaVuSansMono.ttf\", 24i32)
    gfx.free_font(font)
    try:
        gfx.free_font(font)
        println(\"UNREACHED\")
    except ValueError as e:
        println(\"caught double free\")
    return 0
";
        let p = compile_snippet("font_double_free", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
        assert!(out.contains("caught double free"), "expected ValueError on double free; got: {out:?}");
    }
}
