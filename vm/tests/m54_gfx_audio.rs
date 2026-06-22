#![cfg(feature = "graphics")]
//! M54 integration tests for the GFX audio functions (SDL_mixer).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m54_audio_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn test_gfx_audio_suite() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    std::env::set_var("SDL_AUDIODRIVER", "dummy");

    // 1. audio_init succeeds (after gfx.init)
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    gfx.audio_init()
    println(\"audio_init ok\")
    return 0
";
        let p = compile_snippet("audio_init", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0, "audio_init failed: {out:?}");
        assert!(out.contains("audio_init ok"), "expected ok; got: {out:?}");
    }

    // 2. load_sound + play_sound + set_sound_volume don't crash
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    gfx.audio_init()
    snd: Sound = gfx.load_sound(\"vm/tests/fixtures/games/blip.wav\")
    gfx.set_sound_volume(snd, 64i32)
    gfx.play_sound(snd)
    println(\"play ok\")
    gfx.free_sound(snd)
    return 0
";
        let p = compile_snippet("load_play", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0, "load/play failed: {out:?}");
        assert!(out.contains("play ok"), "expected play ok; got: {out:?}");
    }

    // 3. load_sound on missing file raises IOError
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    gfx.audio_init()
    try:
        snd: Sound = gfx.load_sound(\"vm/tests/fixtures/games/does_not_exist.wav\")
        println(\"UNREACHED\")
    except IOError as e:
        println(\"caught IOError\")
    return 0
";
        let p = compile_snippet("missing_sound", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
        assert!(out.contains("caught IOError"), "expected IOError on missing file; got: {out:?}");
    }

    // 4. free_sound + use after free raises ValueError
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    gfx.audio_init()
    snd: Sound = gfx.load_sound(\"vm/tests/fixtures/games/blip.wav\")
    gfx.free_sound(snd)
    try:
        gfx.play_sound(snd)
        println(\"UNREACHED\")
    except ValueError as e:
        println(\"caught ValueError\")
    return 0
";
        let p = compile_snippet("sound_use_after_free", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
        assert!(out.contains("caught ValueError"), "expected ValueError on use-after-free; got: {out:?}");
    }

    // 5. double free raises ValueError
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    gfx.audio_init()
    snd: Sound = gfx.load_sound(\"vm/tests/fixtures/games/blip.wav\")
    gfx.free_sound(snd)
    try:
        gfx.free_sound(snd)
        println(\"UNREACHED\")
    except ValueError as e:
        println(\"caught double free\")
    return 0
";
        let p = compile_snippet("sound_double_free", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
        assert!(out.contains("caught double free"), "expected ValueError on double free; got: {out:?}");
    }

    // 6. load_music + play_music + stop_music + set_music_volume (WAV works as music too)
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    gfx.audio_init()
    mus: Music = gfx.load_music(\"vm/tests/fixtures/games/blip.wav\")
    gfx.set_music_volume(64i32)
    gfx.play_music(mus, 0i32)
    gfx.stop_music()
    println(\"music ok\")
    return 0
";
        let p = compile_snippet("load_music", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0, "load_music failed: {out:?}");
        assert!(out.contains("music ok"), "expected music ok; got: {out:?}");
    }

    // 7. load_music on missing file raises IOError
    {
        let src = "\
import gfx
fn main() -> i32:
    gfx.init()
    gfx.audio_init()
    try:
        mus: Music = gfx.load_music(\"vm/tests/fixtures/games/no_such_file.ogg\")
        println(\"UNREACHED\")
    except IOError as e:
        println(\"caught IOError\")
    return 0
";
        let p = compile_snippet("missing_music", src);
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0);
        assert!(out.contains("caught IOError"), "expected IOError on missing music; got: {out:?}");
    }
}
