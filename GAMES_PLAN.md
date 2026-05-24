# StrictPy Games Plan

**Purpose:** turn StrictPy into a language fit for writing real desktop games. Ship a `gfx` stdlib package that exposes windowing + drawing + input + audio, then build three reference games on top of it: **Snake**, **Tetris**, and a 2D **Space Shooter**.

**Audience:** another AI agent picking this up in a fresh session. Read this file end-to-end before writing code. Every milestone below has a self-contained brief; you can execute them sequentially without re-reading prior briefs after you finish each one.

**How to use this file:** treat each `### M52 …` / `### M53 …` heading as the brief for one focused agent run. Each brief tells you what to ship, what to touch, how to test, how to commit, and what to update in `LANGUAGE_GUIDE.md` so the next agent inherits a clean baseline.

---

## 1. Scope and goal

Build three working **desktop** games written in StrictPy (`.spy` source):

1. **Snake** — grid-based, single-player, food → grow → collide-with-self ends game.
2. **Tetris** — 10×20 grid, 7 tetrominoes with SRS-style rotation, line clears, level progression, next-piece preview.
3. **Space Shooter** — top-down/side-scrolling 2D, player ship, projectiles, enemy waves, score, lives.

All three must:

- Run as **native desktop windows** (not in a browser tab — see §3 for the architectural call).
- Play **sound effects** (short SFX) and ideally background **music**.
- Render **images** (sprites), or at minimum coloured rectangles for the Snake/Tetris grids.
- Read **keyboard input** at 60 FPS without perceptible latency.
- Run cross-platform — Windows, macOS, Linux — from the same Spy source.
- Persist **high scores** (sqlite via the existing M23 stdlib).

The reference games live in `examples/games/<name>.spy` with assets in `examples/games/<name>/assets/`.

---

## 2. What StrictPy has today (the relevant pieces)

Skim this so you know what you can lean on and where the new code goes.

### 2.1 Language + runtime

- Static types: `i32 / i64 / f64 / bool / str`, `None`, optionals (`T?`), `List[T]`, `Dict[K, V]`, `Tuple[A, B, …]`. Sealed and open classes with generics (M16/M17/M31). `match` / `isinstance` (M16). `try` / `except` (M15). Closures + threads + channels (M5/M6). Async I/O thread-backed (M32).
- Compiled to bytecode (`.spyc`), executed by the `spy` VM (`vm/src/lib.rs`); optional Cranelift JIT.
- VM-side primitives are exposed to Spy code through the `NativeFn` enum in `shared/src/native.rs`. Adding a stdlib function means: pick an ID, add it to the enum + the `from_u32` arm, write the handler in `vm/src/builtins.rs`, and register the binding in `compiler/src/resolver.rs::seed_stdlib_modules`.
- Stdlib classes are also registered through `seed_stdlib_modules` as `StdlibItemKind::Class { class_id }` (M36 refactor — see HANDOFF.md "M36" section).

### 2.2 Existing stdlib (the parts you'll reuse)

| Module | Notes |
|---|---|
| `math` (M20b) | `sqrt`, `sin`, `cos`, `floor`, `ceil`, `log2`, `gcd`, `factorial`, etc. NativeFns 200-229. |
| `random` (M20b) | `seed`, `randint`, `random`, `choice_{i64,f64,str}`, `shuffle_*`, `sample_*`. NativeFns 185-199. |
| `time` (M20b) | `now`, `now_ms`, `monotonic` (per-process Instant-anchored, fractional seconds), `sleep_s`, `sleep_ms`. NativeFns 175-180. **Critical for frame-time deltas.** |
| `io` (M5) | `open(path, mode)` → `File`, `read`, `write`, `close`. Use for asset paths that aren't handled by the gfx loader. |
| `os.path` (M20a) | `join`, `exists`, `dirname`, etc. Useful for resolving asset paths relative to the script. |
| `sqlite3` (M23 P3a-D, M35 class) | `Connection` + `Cursor`. Use for high-score persistence. |
| `json` (M20c) | Optional — for settings files. |
| `tabular` (M37-M50c) | Not relevant for games. |

### 2.3 What's missing for games (the entire reason this plan exists)

- **No window/graphics primitive.** No way to create an OS window, draw pixels, blit images. This is the central gap.
- **No keyboard/mouse event source.** Stdin reading exists but it's line-buffered and useless for game input.
- **No audio output.** No way to play a WAV or stream music.
- **No font rendering.** Need this for score displays and menus.
- **No realtime frame timing helper.** `time.monotonic` is fine to compute deltas; we don't need a separate frame-pacing primitive in v1.

Everything else (lists, dicts, math, random, file I/O, sqlite for high scores) StrictPy already has.

### 2.4 NativeFn ID accounting (where to put new IDs)

- Highest currently used ID is `M50aTabServeWithTimeout = 1068` (M50a, in `shared/src/native.rs`).
- M49 reserved IDs through 1066; M50a uses 1067-1068; the comment block at 1069-1090 was reserved for M50b/M50c follow-ups but **M50b/M50c shipped with zero new NativeFns** (everything went into the M50a serve loop dispatcher), so 1069-1090 are still free.
- **Reserve IDs 1100-1299 for the games stack** (`gfx` stdlib, audio, fonts). That's 200 slots, more than enough for v1 + room to grow. Document the range in `shared/src/native.rs` with a block comment, same style as the M50a block.
- Sub-ranges within 1100-1299 (suggested, adjust as you go):
  - **1100-1129**: `gfx` core (init, window, event polling, drawing primitives) — M52.
  - **1130-1149**: `gfx` images + sprite-sheet helpers — M53.
  - **1150-1169**: `gfx` audio (SFX + music) — M54.
  - **1170-1189**: `gfx` fonts + text rendering — M54 or M55 (small).
  - **1190-1199**: reserved for M58 polish (fullscreen, vsync toggle, gamepad).
  - **1200-1299**: reserved for v2 (3D, particles, networking-multiplayer).

---

## 3. Architectural decision: native SDL2 vs browser-based

There are two coherent paths to "desktop games in StrictPy." Pick one up front because they fork the entire stdlib design.

### Path A — Native SDL2 (recommended)

Add a single Rust crate dependency: [`sdl2`](https://crates.io/crates/sdl2). The `sdl2` crate is the canonical Rust binding for libSDL2 and covers windowing + 2D rendering + input + audio + mixer (via SDL_mixer) + image loading (via SDL_image) + TTF (via SDL_ttf).

**Pros**

- Real native window, real 60 FPS, real keyboard events with zero network latency.
- One mature C library that ships on every desktop OS. The Rust binding is well-trodden.
- Single-process model: game logic lives in Spy, runs on the same thread as the SDL event loop.
- Audio "just works" via SDL_mixer.
- No new transport/protocol design needed in StrictPy.

**Cons**

- New system dependency: libSDL2 (+ SDL2_image + SDL2_mixer + SDL2_ttf) must be installed on the user's machine. The `sdl2` crate has a `bundled` feature that statically links a vendored copy — use it to keep the build hermetic (same pattern as `rusqlite` with `bundled`).
- ~150 KB binary growth; first build is ~30s slower because the vendored libSDL2 has to compile.
- Adds C compilation to the StrictPy build (CMake + a C compiler). Acceptable — every modern dev machine has this; CI has it.

### Path B — Browser-based (rejected for this plan)

Reuse the M50a HTTP server pattern: boot a localhost server, serve a bundled HTML5 canvas frontend, frame-sync via WebSocket. Pros: no new Rust deps, reuses M50a infrastructure. Cons: HTTP polling tops out at ~30 FPS realistically; real 60 FPS needs WebSockets (queued but not built); audio routing is awkward (browser tab needs user-gesture interaction before playing audio per browser security policy); the "desktop game" really just runs in a browser tab. Not what the user asked for.

**Decision: take Path A (native SDL2).** If you reach a milestone where SDL2 turns out to be infeasible (Windows CI breakage, license concerns), fall back to Path B and document why — but don't pre-emptively go there.

### 3.1 Why SDL2 and not winit + wgpu

`winit` (window/event loop) + `wgpu` (GPU rendering) is the modern Rust gamedev stack, but it's 5+ crates, a steep wgsl-shader learning curve, and overkill for 2D pixel-art games. SDL2 gives you `RenderCopy(texture, src, dst)` in one line — wgpu makes you set up a render pipeline, a sampler, a bind group, and write a fragment shader to draw a sprite. For v1 of "Snake / Tetris / Space Shooter," SDL2 is the right tool. Revisit wgpu if v2 ships a 3D game.

### 3.2 Why not `macroquad` or `bevy`

`macroquad` is a great single-crate gamedev framework but it owns the main loop in a way that fights with StrictPy's interpreter (it expects you to `await` per frame; we don't have async-frame plumbing). `bevy` is an ECS engine — too opinionated for "write your game logic in Spy and call drawing primitives."

---

## 4. The new `gfx` stdlib package — API surface

This is the API surface to design toward. Every entry maps to a NativeFn handler in `vm/src/builtins.rs` and a `StdlibItem` registration in `compiler/src/resolver.rs`. Spy callers see them as `import gfx` + `gfx.foo(...)`.

The exact spelling can shift slightly during implementation — but **don't reshape the API silhouette without updating this section first** so the per-milestone briefs stay accurate.

### 4.1 Module + classes

- `gfx` — module. Registered through `seed_stdlib_modules` like `tabular`.
- `gfx.Window` — opaque handle for an OS window + renderer. Sealed class, internal payload holds the SDL handles.
- `gfx.Image` — opaque handle for a loaded texture. Sealed class.
- `gfx.Sound` — opaque handle for a short SFX (PCM in memory). Sealed class.
- `gfx.Music` — opaque handle for streaming background music. Sealed class.
- `gfx.Font` — opaque handle for a loaded TTF font.
- `gfx.Event` — open-ish class with fields:
  - `kind: str` — one of `"key_down" | "key_up" | "mouse_down" | "mouse_up" | "mouse_move" | "quit"`.
  - `key: str` — for key events; the key name (e.g. `"left"`, `"right"`, `"space"`, `"escape"`, `"a"`-`"z"`).
  - `x: i32`, `y: i32` — for mouse events.
  - `button: i32` — for mouse_down/up (1=left, 2=middle, 3=right).
- `gfx.Color` — could be a Tuple[i32, i32, i32, i32] for RGBA, or a class. Decide in M52 brief; the simpler choice is to take 4 i32 args directly (`gfx.draw_rect(win, x, y, w, h, r, g, b, a)`) and skip a `Color` class. This matches how the M50a HTML rendering works (no `Color` class in JS either).

### 4.2 Functions — by milestone

#### M52 — core (window + events + drawing primitives)

```python
import gfx
from gfx import Window, Event

gfx.init() -> i32
# Initialize SDL2 video + events subsystems.  Idempotent — calling
# twice is a no-op.  Returns 0 on success, nonzero on failure
# (raises IOError on hard failure).

gfx.create_window(title: str, width: i32, height: i32) -> Window
# Create an OS window with an attached hardware-accelerated 2D
# renderer.  Width/height in logical pixels.

gfx.close_window(win: Window) -> None
# Destroy the window + renderer.  After this call, every other
# gfx.* call against `win` raises ValueError.

gfx.poll_event(win: Window) -> Event?
# Return the next pending event (key press, mouse, window-close),
# or `none` if the queue is empty.  Non-blocking.  Call this in a
# loop at the top of each frame until it returns none.

gfx.clear(win: Window, r: i32, g: i32, b: i32) -> None
# Fill the back buffer with a solid colour.  Call at the start of
# each frame.

gfx.present(win: Window) -> None
# Flip the back buffer to the screen.  Call once per frame after
# all drawing.

gfx.draw_rect(win: Window, x: i32, y: i32, w: i32, h: i32,
              r: i32, g: i32, b: i32, a: i32) -> None
# Filled rectangle.  Alpha in 0..255.

gfx.draw_rect_outline(win: Window, x: i32, y: i32, w: i32, h: i32,
                      r: i32, g: i32, b: i32, a: i32) -> None
# 1-pixel outline.

gfx.draw_line(win: Window, x1: i32, y1: i32, x2: i32, y2: i32,
              r: i32, g: i32, b: i32, a: i32) -> None

gfx.draw_point(win: Window, x: i32, y: i32,
               r: i32, g: i32, b: i32, a: i32) -> None
# Single pixel.  Useful for starfields.

gfx.window_size(win: Window) -> Tuple[i32, i32]
# Returns (width, height).

gfx.set_window_title(win: Window, title: str) -> None
```

#### M53 — images

```python
gfx.load_image(win: Window, path: str) -> Image
# Load a PNG/JPG/BMP into a GPU texture.  Path resolved relative
# to current working directory.  Raises IOError if not found,
# ValueError if not a supported format.

gfx.image_size(img: Image) -> Tuple[i32, i32]

gfx.draw_image(win: Window, img: Image, dst_x: i32, dst_y: i32) -> None
# Blit the image at its native size.

gfx.draw_image_rect(win: Window, img: Image,
                    src_x: i32, src_y: i32, src_w: i32, src_h: i32,
                    dst_x: i32, dst_y: i32, dst_w: i32, dst_h: i32) -> None
# Sprite-sheet blit + scale.  src_* selects a sub-rectangle of the
# image; dst_* places + scales it on the window.

gfx.draw_image_rotated(win: Window, img: Image,
                       dst_x: i32, dst_y: i32, dst_w: i32, dst_h: i32,
                       angle_deg: f64) -> None
# Same as draw_image_rect with src = full image, plus rotation
# around the center of dst.  Used by Space Shooter for ship/enemy
# orientation.

gfx.free_image(img: Image) -> None
# Drop the texture.  Called automatically at GC; explicit free is
# for memory-tight games with many transient images.
```

#### M54 — audio + fonts + text

```python
gfx.audio_init() -> i32
# Open the audio device.  Defaults: 44100 Hz, 16-bit signed
# stereo, 2048-frame buffer.  Idempotent.

gfx.load_sound(path: str) -> Sound
# Load a WAV/OGG into memory.  For short SFX (<10s).

gfx.play_sound(sound: Sound) -> None
# Fire-and-forget playback.  Multiple simultaneous plays of the
# same sound mix correctly.

gfx.load_music(path: str) -> Music
# Load streaming music (MP3/OGG).  Only one music track plays at
# a time.

gfx.play_music(music: Music, loops: i32) -> None
# loops = -1 for infinite loop; 0 = play once; N = play N+1 times.

gfx.stop_music() -> None

gfx.set_music_volume(volume: i32) -> None
# 0..128 (SDL_mixer's range).

gfx.set_sound_volume(sound: Sound, volume: i32) -> None
# Per-sound volume (0..128).

gfx.load_font(path: str, point_size: i32) -> Font
# Load a TTF.  point_size in points.

gfx.draw_text(win: Window, font: Font, text: str, x: i32, y: i32,
              r: i32, g: i32, b: i32) -> None
# Render text as a one-shot texture.  For UI text that changes per
# frame this is wasteful but fine for v1.  Optimize via caching
# in v2.

gfx.text_size(font: Font, text: str) -> Tuple[i32, i32]
# Returns (w, h) of the rendered text without drawing it.  Used
# for centering.
```

#### M58 — polish (don't ship in M52-M57; queued)

- `gfx.set_fullscreen(win, enabled)`, `gfx.set_vsync(win, enabled)`.
- Gamepad: `gfx.poll_event` reports `"gamepad_button_down"` events with `button: str`.
- Texture cache for text-rendering perf.
- Particle helper (or just leave it as user code).

### 4.3 Spy code conventions for the games

- Use `time.monotonic()` (returns `f64` seconds since interpreter init) for frame-time deltas. Target 60 FPS by `time.sleep_ms(max(0, 16 - elapsed_ms))` at the end of each frame.
- Game state is a struct (open class). Game loop is a `while` loop with `poll_event` at the top, `update(state, dt)` in the middle, `render(state)` at the bottom.
- Entity collections are `List[Entity]`. Removal mid-iteration: build a new list of "alive" entities (immutable-style); cheap enough for the entity counts we'll have (<200).
- Floating-point positions for entities (`f64 x, y`); cast to `i32` at render time. Avoids jitter on slow movement.

---

## 5. Sequential milestones

Each milestone is a self-contained agent run. Tag them M52 onward (HANDOFF's last shipped milestone is M50c). M51 is the unrelated "RollingWindow chainable" item queued in HANDOFF — don't conflate them; the games stack is M52+.

| Milestone | Scope | Estimated LOC (Rust + Spy) | Cadence classification |
|---|---|---:|---|
| **M52 (Complete)** | `gfx` core: SDL2 init, Window, Event, drawing primitives | ~900 Rust + ~150 test Spy | **cross-dispatch + net-new-feature** |
| **M53 (Complete)** | `gfx` images + sprite sheets | ~400 Rust + ~80 test Spy | disjoint-handler |
| **M54 (Complete)** | `gfx` audio + fonts + text | ~600 Rust + ~80 test Spy | shared-infra (audio init shared by SFX + music; combined Phase A at ~35%) |
| **M55 (Complete)** | Snake game | ~400 Spy | net-new-feature (pure user code; one commit when it plays) |
| **M56** | Tetris game | ~600 Spy | net-new-feature |
| **M57** | Space Shooter | ~900 Spy | net-new-feature |
| **M58** | Polish: high scores via sqlite, fullscreen, settings persistence, restart-without-relaunch | ~300 Rust + ~200 Spy | shared-infra |

After M52-M58 ships you have three working games + the `gfx` stdlib + an honest demonstration that StrictPy can host real desktop applications. Total estimated effort: **~4500 LOC** across ~6-7 agent sessions.

---

## 6. Per-milestone agent briefs

These are written to be picked up cold. Each tells you what to build, what to touch, how to test, what to commit, what to update in `LANGUAGE_GUIDE.md`, and what's explicitly out of scope.

### M52 — `gfx` core (windows + events + drawing primitives)

**Branch:** `claude/m52-gfx-core-<random>`. (The orchestrator will set the branch name; develop on it, push to it.)

**Scope**

Ship the minimum that lets a Spy program open a window, poll a key event, and draw a coloured rectangle, then close cleanly. Concretely the API surface from §4.2 "M52 core" — `init`, `create_window`, `close_window`, `poll_event`, `clear`, `present`, `draw_rect`, `draw_rect_outline`, `draw_line`, `draw_point`, `window_size`, `set_window_title`. Plus the `Window` and `Event` classes.

**Files you'll touch**

- `Cargo.toml` (vm/Cargo.toml): add `sdl2 = { version = "0.37", features = ["bundled"] }` (or whatever the current major; pick the latest stable). The `bundled` feature compiles libSDL2 from vendored sources so the build is hermetic — same pattern as `rusqlite` already uses with `bundled`.
- `shared/src/native.rs`: add the M52 NativeFn variants (IDs 1100-1129; reserve the rest of 1100-1129 for any small follow-ups in this milestone). Add an explanatory block comment in the same style as the M50a block at line ~1300.
- `vm/src/builtins.rs`: add a new section at the end of the file (after the M50c code, before the tests module) — `// ── M52 — gfx stdlib (windows + events + drawing) ──`. Build it the same way M50a built `tabular.serve`: a module-local `Mutex<Option<Sdl>>` for the SDL context, per-Window structs holding the SDL handles, NativeFn handlers that look up by raw pointer.
- `vm/src/builtins.rs`: in the dispatch `match` (the giant one near the top — `pub(crate) fn dispatch`), add arms for each new NativeFn ID.
- `compiler/src/resolver.rs`: add a new stdlib module registration in `seed_stdlib_modules`. Pattern is the same as `tabular`. Register the `Window` and `Event` classes through `StdlibItemKind::Class` (M36 pattern); register the functions as `StdlibItemKind::Function`.
- `vm/tests/m52_gfx_core.rs`: new test file. See "Testing" below.
- `examples/games/_smoke_window.spy`: a 50-line script that opens a window, draws a red square, waits for the user to press Escape or close the window, then exits. The test harness can compile this to verify the source typechecks end-to-end (don't actually run it in CI — see "Testing" caveat).
- `LANGUAGE_GUIDE.md`: §5 — add a `gfx (M52)` subsection with the API table. §11 — add §11.40 "gfx scope-down" listing what's NOT in M52 (no images, no audio, no fonts, no fullscreen, no gamepad).

**Design calls worth making explicitly**

- **Single-window only in v1.** Multiple windows is a real-world need (separate game + level-editor windows) but adds complexity to event routing. Reject it for v1; document in §11.40.
- **SDL_INIT_VIDEO + SDL_INIT_EVENTS only in M52.** Audio init waits for M54 — splitting it keeps the M52 surface narrower.
- **`Window` payload layout:** opaque to Spy. Internally a 16-byte payload pointing at a Rust-side struct that owns the SDL `Window` + `Canvas<Window>`. Use the same `m34_alloc_class_obj` pattern M37-M50c uses. Drop hook in the GC: register a finalizer that calls `SDL_DestroyWindow` when the Spy `Window` object is collected. **If finalizers aren't wired into the GC** (check `vm/src/gc.rs`), require explicit `gfx.close_window(win)` and document in §11.40.
- **`Event` payload:** a small open class with all the fields predefined. Empty/zero values for fields not relevant to the event's `kind`. Don't try to be clever with variants — Spy users can just check `event.kind` and read the relevant fields.
- **Key naming:** lowercase ASCII for letters/digits (`"a"`, `"1"`), spelled-out names for everything else (`"left"`, `"right"`, `"up"`, `"down"`, `"space"`, `"escape"`, `"enter"`, `"shift"`, `"ctrl"`, `"alt"`, `"tab"`). Use SDL's `Keycode::name()` then lowercase. Document the canonical set in `LANGUAGE_GUIDE.md`.
- **Renderer choice:** `Canvas<Window>` with `accelerated()` + `present_vsync()`. Drops back to software if hardware unavailable.

**Testing**

Two challenges:

1. **SDL needs a display.** Cargo test in headless CI (no $DISPLAY) will fail unless SDL is told to use the dummy driver. Set `SDL_VIDEODRIVER=dummy` in the test process env. SDL's dummy driver is enough to verify window creation + event polling + drawing primitives (they all succeed; nothing's actually visible).
2. **Event polling is async to user.** Tests can't `gfx.create_window` then expect a real key event — there's no user. Use `SDL_PushEvent` to synthesize events.

Plan: write a Rust-side helper `m52_test_push_keydown_event(scancode: &str)` available only under `#[cfg(test)]` that synthesizes events into the SDL queue. Then in Spy test code, the harness can:

```rust
// In vm/tests/m52_gfx_core.rs (Rust test, not Spy):
#[test]
fn window_open_and_close() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    let mut i = test_interp();
    let title = i.alloc_string("test");
    let w = dispatch(&mut i, NativeFn::GfxCreateWindow as u32,
                     &[title as u64, 320, 240]).unwrap();
    assert_ne!(w, 0);
    let r = dispatch(&mut i, NativeFn::GfxCloseWindow as u32, &[w]).unwrap();
    assert_eq!(r, 0);
}
```

Cover at minimum:
- Init succeeds twice (idempotent).
- Window create + close.
- `window_size` returns what was passed.
- `clear` + `present` don't crash.
- `draw_rect` doesn't crash with various dimensions including (0, 0) and out-of-bounds.
- Drawing on a closed window raises ValueError.
- Event poll on empty queue returns `none`.
- (If you wire the synthetic-event push helper) synthetic key_down → poll_event returns an Event with the right `kind` and `key`.

**Commit cadence**

- Phase A: Cargo.toml + sdl2 crate compiles. (Don't commit — too granular.)
- Phase B: NativeFn enum + init/create_window/close_window/window_size. Commit.
- Phase C: event polling + clear/present/draw_rect/draw_line. Commit.
- Phase D: tests + LANGUAGE_GUIDE + example. Commit.

Each commit must have a clean `cargo build --workspace --release` and a passing `cargo test --release -p strictpy-vm --test m52_gfx_core`. Per HANDOFF Lesson 1: first commit before 60% of your time budget. M52 may slip slightly because adding a new C dependency + new sealed-class subclass is cross-dispatch + net-new-feature — combined first commit at ~50-60% is fine.

**Out of scope (do not ship in M52)**

- Image loading — M53.
- Audio — M54.
- Font rendering — M54.
- Fullscreen toggle — M58.
- Gamepad / joystick — M58.
- Multiple windows — out of v1.

---

### M53 — `gfx` images (sprite loading + blitting)

**Branch:** `claude/m53-gfx-images-<random>`.

**Scope**

Add the image API from §4.2 "M53": `load_image`, `image_size`, `draw_image`, `draw_image_rect`, `draw_image_rotated`, `free_image`. Add the `Image` sealed class. Add the `sdl2-image` (or whatever the current binding name is — likely `sdl2` with `image` feature flag) dependency.

**Files**

- `vm/Cargo.toml`: enable the `image` feature on the `sdl2` crate (or add `sdl2-image` if it's split).
- `shared/src/native.rs`: M53 NativeFn IDs in 1130-1149.
- `vm/src/builtins.rs`: M53 section after M52. Use the same `m34_alloc_class_obj` pattern for `Image`. Texture lifetime is tied to the `Window` it was loaded against (SDL's `Texture` borrows the `TextureCreator`); document this in the v1 doc (§11.41).
- `compiler/src/resolver.rs`: register `Image` + new functions.
- `vm/tests/m53_gfx_images.rs`: load a tiny test PNG from `vm/tests/fixtures/games/red_square.png` (commit a 4×4 red square — generate it with `python3 -c 'import struct; ...'` if needed, or use ImageMagick; check it in to the repo). Verify size, draw without crash, free.
- `examples/games/_smoke_sprite.spy`: window + load PNG + draw + present + sleep 1s + close.
- `LANGUAGE_GUIDE.md` §5 / §11.41 updates.

**Design calls**

- **Sprite-sheet pattern:** the games will use sprite sheets (one PNG with many sprites in a grid). The `draw_image_rect` API takes src + dst rectangles so users do their own slicing. Document the convention in §11.41.
- **Format support:** PNG + JPG + BMP. PNG is enough for everything; JPG for photo-style backgrounds. BMP is free if SDL_image is loaded; include it.
- **Color-key (transparency from a sentinel color):** skip. PNGs with alpha channel are the norm.

**Testing**

Same SDL_VIDEODRIVER=dummy trick. Load `vm/tests/fixtures/games/red_square.png`, verify `image_size` returns `(4, 4)`, call `draw_image` (no crash). Make sure `load_image` on a missing path raises IOError. Make sure `load_image` on a non-image file raises ValueError.

**Out of scope**

- Texture atlases (offline tooling). Users hand-pack.
- Image-format conversion at load time. SDL_image handles what it handles.

---

### M54 — `gfx` audio + fonts + text

**Branch:** `claude/m54-gfx-audio-text-<random>`.

**Scope**

Add §4.2 "M54": `audio_init`, `load_sound`, `play_sound`, `load_music`, `play_music`, `stop_music`, `set_music_volume`, `set_sound_volume`, `load_font`, `draw_text`, `text_size`. Add `Sound`, `Music`, `Font` sealed classes.

**Files**

- `vm/Cargo.toml`: enable `mixer` and `ttf` features on `sdl2`.
- `shared/src/native.rs`: audio IDs 1150-1169, font IDs 1170-1189.
- `vm/src/builtins.rs`: two new sections after M53. Audio uses SDL_mixer; TTF uses SDL_ttf.
- `compiler/src/resolver.rs`: register `Sound`, `Music`, `Font` + new functions.
- `vm/tests/m54_gfx_audio.rs` + `vm/tests/m54_gfx_text.rs`.
- `vm/tests/fixtures/games/blip.wav` (tiny — generate a 100ms sine wave via Python `struct` and check in; ~10 KB) and `vm/tests/fixtures/games/test.ttf` (commit a CC0 TTF font; **don't commit a copyrighted font**).
- `examples/games/_smoke_audio.spy`: load WAV, play, sleep 200ms, exit.
- `LANGUAGE_GUIDE.md` §5 / §11.42 (audio) + §11.43 (fonts) updates.

**Design calls**

- **Audio init defaults:** 44100 Hz / 16-bit signed / stereo / 2048-frame buffer. Document; expose overrides only if a game needs them (probably never).
- **Mixer channel count:** SDL_mixer defaults to 8 simultaneous SFX. Bump to 16 to avoid drops on noisy frames.
- **Music format:** OGG Vorbis is the safe pick (license-free, well-supported). MP3 also works if SDL_mixer was built with it. Document OGG as the recommendation.
- **Font CC0 source:** [DejaVu Sans Mono](https://dejavu-fonts.github.io/) — public domain. Check in `vm/tests/fixtures/games/DejaVuSansMono.ttf` (~300 KB) and the games can use it as their default.
- **TTF init:** SDL_ttf needs `Sdl2TtfContext`. Like SDL itself, init once and stash globally. Document idempotency.

**Testing for audio in CI**

The dummy SDL audio driver works: `SDL_AUDIODRIVER=dummy`. Load WAV, call play, verify no crash. Can't verify sound actually came out (no speaker in CI), but the loading + playback pipeline is exercised.

**Out of scope**

- 3D audio / spatial panning. Future.
- Audio recording. Future.
- Streaming SFX (long sounds via streaming, not in-memory). Use `load_music` instead.

---

### M55 — Snake game ✅ COMPLETE

Shipped on `main` (single commit on top of M54). Files: `examples/games/snake.spy` + `examples/games/snake/assets/{eat.wav, die.wav, font.ttf, CREDITS.md, _generate_assets.py}` + `compiler/tests/snake_demo_runs.rs` + LANGUAGE_GUIDE.md §12.6 walkthrough. Compile-only test passes. SFX generated deterministically by the Python helper; font is a copy of the bundled DejaVu CC0 fixture. Manual gameplay smoke-test (run `./target/release/spy examples/games/snake.spy` on a desktop) verifies arrow-key control + collision + restart.

**Branch:** `claude/m55-game-snake-<random>` *(historical brief follows; ignore if reading after M55 has shipped)*.

**Scope**

Ship `examples/games/snake.spy` — a complete, playable Snake game using only the M52-M54 `gfx` API. Includes:

- 20×20 grid, 30 px cells, 600×600 window plus a 60 px top bar for score.
- Snake starts 3 segments long, moves at 8 cells/second (configurable).
- Food spawns at a random empty cell after each eat.
- Arrow keys change direction; 180° turns rejected (no instant reverse).
- Eating food: grow by 1, increment score, play `eat.wav`.
- Collide with wall or self: play `die.wav`, show "Game Over — Press R to restart" overlay, R restarts, Escape quits.
- Top bar: score in TTF text.

**Files**

- `examples/games/snake.spy` (~400 LOC).
- `examples/games/snake/assets/eat.wav`, `die.wav` (CC0 — see "Asset sourcing" §8).
- `examples/games/snake/assets/font.ttf` (symlink or copy of the bundled DejaVu).
- `compiler/tests/snake_demo_runs.rs`: compile-only test (don't run; an actual game session needs a user). Pattern from `compiler/tests/tabular_serve_demo_runs.rs`.
- `LANGUAGE_GUIDE.md` §12 — add a "Snake walkthrough" subsection referencing the example.

**Code skeleton**

```python
"""snake.spy — minimal Snake game in StrictPy."""

import gfx
import time
import random
from gfx import Window, Event, Font, Sound

let CELL: i32 = 30
let GRID_W: i32 = 20
let GRID_H: i32 = 20
let TOPBAR_H: i32 = 60
let WIN_W: i32 = CELL * GRID_W
let WIN_H: i32 = TOPBAR_H + CELL * GRID_H
let FRAME_MS: i64 = 16  # ~60 FPS render
let STEP_MS: i64 = 125  # 8 steps per second

class GameState:
    snake: List[Tuple[i32, i32]]  # head first
    direction: Tuple[i32, i32]
    pending_direction: Tuple[i32, i32]
    food: Tuple[i32, i32]
    score: i64
    game_over: bool
    last_step_ms: i64

fn new_game() -> GameState:
    s = GameState()
    s.snake = [(10i32, 10i32), (9i32, 10i32), (8i32, 10i32)]
    s.direction = (1i32, 0i32)
    s.pending_direction = (1i32, 0i32)
    s.food = spawn_food(s.snake)
    s.score = 0i64
    s.game_over = false
    s.last_step_ms = time.now_ms()
    return s

# ... spawn_food / handle_input / step / render / main loop ...

fn main() -> i32:
    gfx.init()
    gfx.audio_init()
    win: Window = gfx.create_window("Snake", WIN_W, WIN_H)
    font: Font = gfx.load_font("examples/games/snake/assets/font.ttf", 24i32)
    eat: Sound = gfx.load_sound("examples/games/snake/assets/eat.wav")
    die: Sound = gfx.load_sound("examples/games/snake/assets/die.wav")
    state: GameState = new_game()
    running: bool = true
    while running:
        frame_start: i64 = time.now_ms()
        # Drain events
        e: Event? = gfx.poll_event(win)
        while e is not none:
            ...
            e = gfx.poll_event(win)
        # Step game logic
        now: i64 = time.now_ms()
        if not state.game_over and (now - state.last_step_ms) >= STEP_MS:
            step(state, eat, die)
            state.last_step_ms = now
        # Render
        gfx.clear(win, 30i32, 30i32, 40i32)
        render(win, font, state)
        gfx.present(win)
        # Pace
        elapsed: i64 = time.now_ms() - frame_start
        if elapsed < FRAME_MS:
            time.sleep_ms(FRAME_MS - elapsed)
    gfx.close_window(win)
    return 0
```

**Out of scope**

- High-score persistence — M58.
- Variable game speed / difficulty — keep STEP_MS constant in M55; M58 can add level-up.
- Particles when food eaten — fine without.

---

### M56 — Tetris

**Branch:** `claude/m56-game-tetris-<random>`.

**Scope**

`examples/games/tetris.spy` — a playable Tetris with all 7 tetrominoes, SRS-style rotation (4 states per piece), line clears, scoring, next-piece preview, level progression that speeds up on every 10 lines.

**Game spec**

- Board: 10 wide × 20 tall, 30 px cells. Window: 600×750 (300 px board + 300 px side panel + 30 px chrome).
- 7 tetrominoes (I/O/T/S/Z/J/L). Each has 4 rotation states; encode as 4×4 bit grids in code.
- Standard rotation system (SRS) is overkill for v1; use simple "rotate matrix in place" with naive wall kicks (try the rotation; if it collides, try shifted 1 left, then 1 right; if both collide, reject).
- Drop tick starts at 800 ms, decreases by 50 ms per level, floor at 100 ms.
- Soft drop (down arrow): 50 ms per cell.
- Hard drop (space): instant + small score bonus.
- Scoring: single = 100 * level, double = 300 * level, triple = 500 * level, tetris = 800 * level.
- Lines cleared: 10 per level.
- Next-piece preview in side panel.
- Game over when a new piece can't spawn at the top row.

**Files**

- `examples/games/tetris.spy` (~600 LOC).
- `examples/games/tetris/assets/move.wav`, `rotate.wav`, `clear.wav`, `tetris.wav` (the 4-line-clear celebration), `gameover.wav`, optional `music.ogg`.
- `examples/games/tetris/assets/font.ttf` (DejaVu).
- `compiler/tests/tetris_demo_runs.rs`.

**Design calls**

- **Tetrominoes as static data:** define them once at module level as `List[List[i32]]` (4-state rotation tables × cell grids).
- **Color per piece:** map piece type → RGB tuple at the top of the file.
- **Pieces rendered as filled rects with a 1-px inner border** — looks bevelled enough without sprites.
- **Music:** optional; ship a 30-second loop of CC0 chiptune if you can find one in the time budget. If not, skip music — the SFX are enough.

**Out of scope**

- Hold-piece (Q/E keys). Stretch goal in M58.
- T-spin detection / advanced scoring. Future.
- Visual line-clear animation. v1 just flashes white for 1 frame.

---

### M57 — Space Shooter

**Branch:** `claude/m57-game-spaceshooter-<random>`.

**Scope**

`examples/games/space_shooter.spy` — top-down side-scrolling 2D shooter with:

- Player ship sprite, arrow keys to move (8-directional), space to shoot.
- Player projectiles (3 per second cap, despawn at top edge).
- Enemy waves: spawned every 2 seconds at random x along top, fly down, occasionally shoot back.
- Enemy projectiles: simpler than player's, despawn at bottom.
- Collision detection (AABB) for player vs enemies, player vs enemy projectiles, player projectiles vs enemies.
- Lives (3); player respawns with brief invuln; game over after losing all 3.
- Score per enemy killed; high score persisted via sqlite (M58 dependency — or skip persistence and add in M58).
- Background: parallax-scrolling starfield (two layers of randomly-positioned points at different scroll speeds).
- Sound: shoot, explosion, hit, optional background music.

**Files**

- `examples/games/space_shooter.spy` (~900 LOC).
- `examples/games/space_shooter/assets/ship.png`, `enemy.png`, `bullet.png`, `enemy_bullet.png`, `explosion.png` (small CC0 sprites — Kenney space-shooter pack works).
- `examples/games/space_shooter/assets/shoot.wav`, `explosion.wav`, `hit.wav`, optional `music.ogg`.
- `examples/games/space_shooter/assets/font.ttf`.
- `compiler/tests/space_shooter_demo_runs.rs`.

**Design calls**

- **Entity representation:** use an open class `Entity` with `kind: str` (player|enemy|p_bullet|e_bullet|explosion), `x: f64`, `y: f64`, `vx: f64`, `vy: f64`, `w: i32`, `h: i32`, `hp: i32`, `age_ms: i64`. One `List[Entity]` per frame; despawn by filtering. ~50-200 entities at peak — Spy lists are plenty fast.
- **AABB collision:** straightforward — every player projectile checked against every enemy each frame. O(n*m) — fine for <50 of each.
- **Procedural waves:** keep simple — every 2 seconds spawn 1-3 enemies. Difficulty ramps by adjusting wave size/cadence over time.
- **Starfield:** init two `List[Tuple[f64, f64]]` of point positions; update each frame by `y += scroll_speed * dt`, wrap when off screen.
- **Sprite rotation:** the player ship can stay axis-aligned. Enemies can use `draw_image_rotated` to face down (180°) — easy way to demonstrate the API.

**Out of scope**

- Power-ups. Stretch in M58.
- Boss enemies. Future.
- Persistent enemy AI / pathfinding. Procedural spawn-and-fly-straight is enough.

---

### M58 — Polish

**Branch:** `claude/m58-games-polish-<random>`.

**Scope** (pick a subset based on time budget)

- **High-score persistence** via sqlite (M23): a `~/.strictpy_games.db` file shared across the three games. Schema: `(game, name, score, ts)`. Add UI to each game to enter player name on game over and display top-10.
- **Settings file:** JSON (M20c) for volume / fullscreen / key remap, stored at `~/.strictpy_games.json`.
- **Fullscreen toggle** (`gfx.set_fullscreen`, `gfx.set_vsync` natives — IDs in 1190-1199).
- **Tetris hold piece** (Q/E).
- **Space Shooter power-ups**: triple-shot, shield, extra life.
- **Optional gamepad support** (SDL2 already exposes it; `gfx.poll_event` emits `"gamepad_button_down"` events with the button name).

Don't try to ship all of these in one milestone. Pick 2-3 based on what feels most useful.

---

## 7. Test patterns

### 7.1 Headless SDL in CI

Every `vm/tests/m5[2-8]_*.rs` must set:

```rust
std::env::set_var("SDL_VIDEODRIVER", "dummy");
std::env::set_var("SDL_AUDIODRIVER", "dummy");
```

at the top of each test or in a shared helper.  This makes SDL initialize against null backends; window creation succeeds, drawing is a no-op, audio playback is a no-op — but every code path runs and any panic/segfault still surfaces.

### 7.2 Synthetic event injection

For testing event handling, build a `#[cfg(test)]` helper:

```rust
#[cfg(test)]
fn m52_test_push_key_event(scancode: SDL_Scancode, down: bool) {
    // SDL_PushEvent with a synthesized SDL_KeyboardEvent.
    // See SDL_Event docs for the union layout.
}
```

Call it from Rust-side tests to drive the event queue, then assert that `poll_event` returns the expected `Event`.

### 7.3 Game-source tests are compile-only

The three games (Snake/Tetris/Space Shooter) can't be "run" in CI — they're interactive and would block. The test for each game is a compile-only check (the file typechecks + bytecode-compiles). Pattern:

```rust
// compiler/tests/snake_demo_runs.rs
#[test]
fn snake_demo_compiles() {
    let src_path = project_root().join("examples").join("games").join("snake.spy");
    let src = fs::read_to_string(&src_path).expect("read snake.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile snake.spy: {e}"));
}
```

Manual smoke-test the games yourself before shipping each milestone. Document in the commit message that you've manually verified the game plays correctly.

### 7.4 Asset-fixture loading

Test assets live in `vm/tests/fixtures/games/`. Commit small CC0 fixtures (4×4 PNGs, 100ms WAVs, the bundled DejaVu TTF). Generate WAVs deterministically with a Python script if no CC0 source is available; keep one such generator in `vm/tests/fixtures/games/_generate_test_assets.py` so future devs can re-generate.

---

## 8. Asset sourcing — important for shipping legally

Every committed asset must be **public domain (CC0)** or **MIT/Apache-style permissive licence with attribution preserved**. Don't commit anything you can't verify the licence of.

Recommended sources:

- **Kenney.nl** — game-asset packs in CC0. Snake/Tetris/Space Shooter sprites all available. Use the "Space Shooter Redux" pack for M57.
- **OpenGameArt.org** — mixed licences; filter by CC0.
- **freesound.org** — short SFX in CC0 (filter explicitly).
- **DejaVu Sans Mono** — public domain font, ~300 KB. Use for all UI text across the three games.

For each asset, add an `examples/games/<game>/assets/CREDITS.md` listing the source URL, original author, licence. Even CC0 attribution is a courtesy.

Generate placeholder assets via Python where licence-clean sources aren't available — e.g. 100ms square-wave beeps for SFX:

```python
import struct, wave
def beep(path, freq=440, ms=100, sr=44100):
    n = int(sr * ms / 1000)
    with wave.open(path, 'wb') as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(sr)
        for i in range(n):
            v = int(32767 * (1 if (i * freq * 2 // sr) % 2 == 0 else -1))
            w.writeframes(struct.pack('<h', v))
```

Acceptable for early milestones; replace with nicer SFX in M58.

---

## 9. Methodology lessons to carry forward

These are paraphrased from HANDOFF.md's "Methodology lessons that have held" section. Apply them on every milestone.

### 9.1 Lesson 1 — commit early

> **First commit before 60% of your time budget.** Checkpoint at 20% / 40% / 60% / 80%.

A 32-agent streak (M28 → M50c) of clean commits says this works. M52 may slip to ~60% (cross-dispatch + net-new-feature combined) but every other milestone should fit clean per-phase commits.

### 9.2 Use distinctive variable prefixes per agent in shared files

`vm/src/builtins.rs` is shared across milestones. Use a per-milestone prefix on every internal helper:

- `m52_` for M52 helpers (`m52_alloc_window`, `m52_event_kind_str`, etc.).
- `m53_` for M53. Etc.

This prevents the M27 closing-brace alignment hazard if multiple agents ever land in parallel.

### 9.3 Cadence classification

From HANDOFF (refined through M50a):

- **disjoint-handler** — independent handler bodies; per-phase commits at ~20%.
- **shared-infra** — combined Phase A at ~30-50% (a new helper or struct field that downstream phases use).
- **cross-dispatch** — new sealed-class subclass forces all dispatch files to compile together; commit at ~50-75%.
- **net-new-feature** — net-new self-contained subsystem with tightly interlocked pieces; ~50-70%.

Per §5 table: M52 is **cross-dispatch + net-new-feature**, M53 is disjoint-handler, M54 is shared-infra, M55/M56/M57 are net-new-feature (all-Spy work — one commit per playable game), M58 is shared-infra.

### 9.4 LANGUAGE_GUIDE freshness

Per HANDOFF "CRITICAL: keep `LANGUAGE_GUIDE.md` up to date":

> Every agent brief that touches **language syntax**, **type system**, or **stdlib** MUST update `LANGUAGE_GUIDE.md` in the same commit.  The doc is the single source of truth for AI tools writing StrictPy programs; if it's out of date, AI tools generate wrong code.

For each `gfx` milestone:

- Bump the version banner at line ~3.
- Update §5 with the new module / functions / classes.
- Add a §11.4x subsection listing the milestone's deliberate v1 simplifications.
- For the games: §12 "End-to-end example programs" — link the new example.

### 9.5 Edit-tool worktree leak

Known intermittent harness issue (HANDOFF "Honest open items"). Workaround: precautionary `cp` block at session start for shared files (`builtins.rs`, `resolver.rs`, `native.rs`, `LANGUAGE_GUIDE.md`), `git status` check per phase, per-file `cp` recovery when symptoms appear. The orchestrator integration via `git checkout` + `git merge --ff-only` is reliable regardless.

---

## 10. File / directory layout

After M58 the tree should look like:

```
examples/
  games/
    snake.spy
    snake/
      assets/
        eat.wav
        die.wav
        font.ttf
        CREDITS.md
    tetris.spy
    tetris/
      assets/
        move.wav
        rotate.wav
        clear.wav
        tetris.wav
        gameover.wav
        music.ogg            # optional
        font.ttf
        CREDITS.md
    space_shooter.spy
    space_shooter/
      assets/
        ship.png
        enemy.png
        bullet.png
        enemy_bullet.png
        explosion.png
        shoot.wav
        explosion.wav
        hit.wav
        music.ogg            # optional
        font.ttf
        CREDITS.md
    _smoke_window.spy        # M52
    _smoke_sprite.spy        # M53
    _smoke_audio.spy         # M54

vm/
  src/
    builtins.rs              # M52/M53/M54 gfx code appended at end
  tests/
    fixtures/
      games/
        red_square.png
        blip.wav
        DejaVuSansMono.ttf
        _generate_test_assets.py
    m52_gfx_core.rs
    m53_gfx_images.rs
    m54_gfx_audio.rs
    m54_gfx_text.rs

compiler/
  tests/
    snake_demo_runs.rs
    tetris_demo_runs.rs
    space_shooter_demo_runs.rs

shared/
  src/
    native.rs                # NativeFn IDs 1100-1199 used

GAMES_PLAN.md                # this file
LANGUAGE_GUIDE.md            # §5 gfx + §11.40-§11.43 updates
HANDOFF.md                   # update with M52-M58 status after each ships
```

---

## 11. After-this checklist

After M58 ships, update:

- **HANDOFF.md** — add an "M52-M58 games stack" section to the top (similar shape to the existing M50a/M50b/M50c sections). Bump the test count + example count in the "Status snapshot" table. The 32-streak Lesson-1 counter becomes 32 + (number of clean M52-M58 commits).
- **THESIS.md / BLOG_POST.md** — overdue refresh anyway; the games-stack story makes a compelling addition ("StrictPy is now suitable for desktop games via the `gfx` stdlib + bundled SDL2").
- **LANGUAGE_GUIDE.md** §3 — add a "Why games?" intro line if you want; otherwise no change.
- **README.md** — add a "Games" section pointing at the three examples.
- **`docs/thesis/`** — if `timeline.md` exists, add M52-M58 entries; if `per_milestone.csv` exists, add rows. (Check whether these exist before assuming.)

---

## 12. Open questions to resolve as you go

Items that need a judgment call but don't block the start:

1. **GC finalizers for SDL handles.** If `vm/src/gc.rs` doesn't support finalizers, M52 must require explicit `gfx.close_window` / `gfx.free_image` / etc. Check first thing in M52; if no finalizer support, document in §11.40.
2. **Bundled SDL2 binary size.** `bundled` feature adds ~150 KB. Acceptable; if it bloats wildly past that, reconsider system-installed libSDL2.
3. **Linux audio backend.** SDL_mixer on Linux uses ALSA by default; some distros need PulseAudio. The `dummy` driver dodges this in CI, but real users may hit it. Document in `LANGUAGE_GUIDE.md` §11.42.
4. **Font fallback.** If the user passes a non-existent font path, raise IOError — but the games' top-level scripts should default to the bundled DejaVu if the user's path is missing. Decide per-game.
5. **macOS retina / DPI scaling.** SDL2 handles this if you call `sdl2::hint::set("SDL_HINT_VIDEO_HIGHDPI_DISABLED", "1")` or similar. For v1, disable HiDPI (logical-pixel rendering only). Document in §11.40.
6. **Game-loop sleep granularity on Windows.** `time.sleep_ms(1)` on Windows historically rounds to ~15 ms. May need `timeBeginPeriod(1)` from winapi for accurate frame pacing. Defer to M58 — most v1 games will be fine at 30-50 effective FPS.

---

## 13. How to read this file when picking up cold

You're a fresh agent. The user pointed you at `GAMES_PLAN.md`. Here's what to do:

1. Read §1-3 to understand the goal + architectural call.
2. Read §4 to know the API surface.
3. Find the next unshipped milestone in §5 (check `git log --oneline | grep -E "M5[2-8]"` to see what's done).
4. Read **only** the brief for that milestone in §6.
5. Follow it. Commit + push per §9's methodology.
6. When the milestone ships, update §5 to mark it done (just edit the table) and update `HANDOFF.md` with the new state.
7. Stop. Don't try to do two milestones in one session — context erodes and the per-milestone briefs assume a clean baseline.

If anything in this plan contradicts what you find in the codebase (an NativeFn ID is already taken, a stdlib pattern has changed, etc.), trust the codebase and update this plan in the same commit. Don't fight reality.

Good luck.
