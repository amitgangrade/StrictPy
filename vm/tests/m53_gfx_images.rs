#![cfg(feature = "graphics")]
//! M53 integration tests for the GFX image functions.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m53_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn test_gfx_image_suite() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    std::env::set_var("SDL_AUDIODRIVER", "dummy");

    // 1. Test load_image and image_size
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    win: Window = gfx.create_window(\"Test Window\", 320i32, 240i32)
    img: Image = gfx.load_image(win, \"vm/tests/fixtures/games/red_square.png\")
    size: Tuple[i32, i32] = gfx.image_size(img)
    w: i32 = size.0
    h: i32 = size.1
    println(\"img_w=\" + str(w) + \", img_h=\" + str(h))
    gfx.free_image(img)
    gfx.close_window(win)
    return 0
";
        let p = compile_snippet("load_size", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
        assert!(out.contains("img_w=4, img_h=4"), "expected size 4x4; got: {out:?}");
    }

    // 2. Test draw_image, draw_image_rect, draw_image_rotated
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    win: Window = gfx.create_window(\"Test Window\", 320i32, 240i32)
    img: Image = gfx.load_image(win, \"vm/tests/fixtures/games/red_square.png\")
    gfx.clear(win, 0i32, 0i32, 0i32)
    
    # Draw simple
    gfx.draw_image(win, img, 10i32, 10i32)
    
    # Draw sub-rectangle
    gfx.draw_image_rect(win, img, 0i32, 0i32, 2i32, 2i32, 20i32, 20i32, 16i32, 16i32)
    
    # Draw rotated
    gfx.draw_image_rotated(win, img, 50i32, 50i32, 32i32, 32i32, 45.0f64)
    
    gfx.present(win)
    gfx.free_image(img)
    gfx.close_window(win)
    return 0
";
        let p = compile_snippet("draw_ops", src);
        let (code, _out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
    }

    // 3. Test non-existent file raises IOError
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    win: Window = gfx.create_window(\"Test Window\", 320i32, 240i32)
    try:
        img: Image = gfx.load_image(win, \"vm/tests/fixtures/games/does_not_exist.png\")
        println(\"UNREACHED\")
    except IOError as e:
        println(\"caught IOError\")
    gfx.close_window(win)
    return 0
";
        let p = compile_snippet("missing_file", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
        assert!(out.contains("caught IOError"), "expected IOError on missing file; got: {out:?}");
    }

    // 4. Test non-image file raises ValueError
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    win: Window = gfx.create_window(\"Test Window\", 320i32, 240i32)
    try:
        img: Image = gfx.load_image(win, \"vm/tests/fixtures/games/_generate_test_assets.py\")
        println(\"UNREACHED\")
    except ValueError as e:
        println(\"caught ValueError\")
    gfx.close_window(win)
    return 0
";
        let p = compile_snippet("invalid_format", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
        assert!(out.contains("caught ValueError"), "expected ValueError on invalid image format; got: {out:?}");
    }

    // 5. Test use after free raises ValueError
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    win: Window = gfx.create_window(\"Test Window\", 320i32, 240i32)
    img: Image = gfx.load_image(win, \"vm/tests/fixtures/games/red_square.png\")
    gfx.free_image(img)
    try:
        gfx.image_size(img)
        println(\"UNREACHED\")
    except ValueError as e:
        println(\"caught ValueError\")
    gfx.close_window(win)
    return 0
";
        let p = compile_snippet("use_after_free", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
        assert!(out.contains("caught ValueError"), "expected ValueError on use after free; got: {out:?}");
    }

    // 6. Test double free raises ValueError
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    win: Window = gfx.create_window(\"Test Window\", 320i32, 240i32)
    img: Image = gfx.load_image(win, \"vm/tests/fixtures/games/red_square.png\")
    gfx.free_image(img)
    try:
        gfx.free_image(img)
        println(\"UNREACHED\")
    except ValueError as e:
        println(\"caught double free\")
    gfx.close_window(win)
    return 0
";
        let p = compile_snippet("double_free", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
        assert!(out.contains("caught double free"), "expected ValueError on double free; got: {out:?}");
    }
}
