# Session handoff — 2026-06-01 (post-M51, layered onto the M52-M56 games stack)

> **Milestone-ordering + collision note:** M51 (`tabular` RollingWindow +
> grab-bag) was finished AFTER the M52-M56 games stack — it's the delayed
> M49 follow-up. It also involved a **parallel-work collision**: an
> independent M51 RollingWindow was already pushed to origin/main
> (`71f697b`) plus an unrelated pivot_table categorical fix (`10584f5`).
> We kept the published RollingWindow as canonical and layered the four
> non-overlapping grab-bag features on top (re-ID'd to avoid the clash).
> The orchestrator's fuller standalone M51 is preserved at tag
> `m51-local-full` but is NOT on main. `git log` is authoritative.

## Read this FIRST in the next session

Everything you need to resume is in:

1. **This file** — current state + pending work + integration recipes
2. **`docs/thesis/timeline.md`** — milestone-by-milestone narrative through M53
3. **`docs/thesis/stats/per_milestone.csv`** — quantitative ground truth (M0-M53)
4. **`THESIS.md`** + **`BLOG_POST.md`** — synthesis documents (frozen at M34)
5. **`RELEASE_NOTES_v0.2.md`** — v0.2.0 freeze-point summary
6. **`LANGUAGE_GUIDE.md`** — single source of truth for AI tools writing
   StrictPy programs (refreshed post-M56; §11 has gotchas through §11.43, §12 has games walkthroughs §12.6/§12.7)
7. **`bench/TABULAR_BENCH_REPORT.md`** + `_M49.md` + `_M51.md` —
   the StrictPy vs pandas 3.0 comparison (M51 adds the `merge_cat_codes`
   codes-hash cell)
8. **`GAMES_PLAN.md`** — sequential plan for the M52-M58 desktop-games stack (Snake + Tetris done; Space Shooter next)
9. **Memory file**: `C:\Users\AG\.claude\projects\C--Users-AG-CascadeProjects-PythonCompiler\memory\project_strictpy.md`

## Current head

- Branch: `main`
- Latest commit: **M58 games polish** — sqlite high scores + fullscreen/vsync + event-pump fix
- Tests passing: **1106 / 0 fail / 1 ignored** (M58 adds 3 tests). NOTE: the `tabular.serve` tests (`m50a_tabular_serve.rs`) bind real TCP ports and **occasionally flake under the parallel full-workspace sweep** (port/timing contention) — they pass in isolation; not a regression.

## Status snapshot

| Metric | Value |
|---|---:|
| Milestones complete on main | M0–M58 **+ M51** (M51 landed after M56 — see ordering note above) |
| **v0.2.0 release** | **Tagged at M30 (commit 121483f)** |
| Tests | **1106** / 0 fail / 1 ignored (`cargo test --workspace --release`) |
| Bugs | 35 / 35 / **0 deferred** + **0 unresolved** — the Snake/Tetris/Shooter Windows input/frame-timing quirk is **FIXED in M58** (vsync-off + persistent event pump), **runtime-verified** by a desktop play-test of Space Shooter (plays smoothly); same shared `gfx` fix applies to Snake/Tetris. |
| Stdlib modules | 39 (no change) |
| Stdlib classes | **26** (M51's `RollingWindow`; M52-M54's Window/Event/Image/Sound/Music/Font; the M37-M47 tabular Column/DataFrame family) |
| Example programs | **122** (+1 M58: `examples/games/highscores.spy`) |
| Reference games | **3** — Snake (M55), Tetris (M56), Space Shooter (M57), all on the `gfx`/SDL2 stack — now with sqlite high scores + fullscreen (M58) |
| Lesson 1 streak | 41 clean-commit agents (M28 → M58). **M51 / M57 / M58 are asterisks** — delegate-blind agents (sandbox blocked builds); orchestrator-verified, not self-gated. M58's agent couldn't even commit (edits landed in the main tree). |
| Benchmark suites | 3 (tabular suite gained the M51 `merge_cat_codes` cell) |

## M58 — completed (this session — games polish, delegate-blind)

Games-polish milestone (per GAMES_PLAN §6). Delegated to a sub-agent whose
sandbox denied `cargo`/`git`/`python` entirely, so it wrote everything
blind AND couldn't commit — its edits landed uncommitted in the **main
working tree**, which the orchestrator then built/fixed/tested/committed.
One blind-code bug found + fixed on integration: `resolver.rs` used
`bool_ty` (out of scope in the gfx registration block) → changed to inline
`Ty::Primitive(PrimTy::Bool)`. After that, clean.

**Shipped:**
1. **High-score persistence (sqlite3 / M23).** `examples/games/highscores.spy`
   is the standalone reference (`save_score(db,game,name,score)` +
   `top_scores(db,game,n) -> List[Tuple[str,i64]]` over
   `scores(game,name,score,ts)`). **StrictPy v0.2 has no user-module
   import**, so the agent **inlined `hs_*` copies into each of the three
   games** rather than importing — game-over flow saves once (a
   `score_saved` bool guard) and renders top-5 via `gfx.draw_text`, db
   `strictpy_games.db` (cwd-relative). Also note: **`i64(str)` is a
   code-point cast, not a parse** — the prelude `parse_i64` is the right
   tool for the score column. Tests: `vm/tests/m58_highscores.rs` (real
   temp-file sqlite round-trip, asserts score-DESC + game filtering —
   passes) + `compiler/tests/highscores_demo_runs.rs` (compile-only).
2. **`gfx.set_fullscreen(win, bool)` + `gfx.set_vsync(win, bool)`** —
   NativeFn IDs **1190/1191**. `set_fullscreen` → `FullscreenType::Desktop/Off`;
   `set_vsync` → raw FFI `SDL_RenderSetVSync(canvas.raw(), 0/1)` (the
   `canvas.set_vsync` wrapper wasn't relied on), **best-effort no-op on
   nonzero rc** (dummy/software renderers can't toggle at runtime — so it
   doesn't abort CI). `ir.rs` needed NO change (gfx fns lower generically
   via `item.native_id`). Test `vm/tests/m58_gfx_polish.rs` (dummy driver).
3. **Event-pump thread-local fix.** `m52_gfx_poll_event` no longer
   recreates the SDL `EventPump` every frame; a `thread_local! M58_EVENT_PUMP`
   builds it once and reuses it (EventPump is `!Send`, so it can't live in
   the global `Mutex` SDL context — a thread-local is its correct home).
   Each game now calls `gfx.set_vsync(win, false)` right after
   `create_window`.

### The Windows input/frame-timing quirk — FIXED in M58 (runtime-verified)

**RESOLVED.** A desktop play-test of Space Shooter (2026-06-01) confirmed
smooth, continuous input + steady frame rate — the M58 mitigations fixed
the M55/M56 quirk. The fix is in the shared `gfx` layer + each game's
`set_vsync(false)` call, so Snake and Tetris are fixed by the same change.
The two causes and the shipped fixes (now confirmed effective):

- **Cause A — `present_vsync` blocking.** The renderer is built with
  `.present_vsync()` (builtins.rs `m52_gfx_create_window` ~23772), so
  `canvas.present()` blocks until the next vblank. Combined with the
  games' own `time.sleep_ms` pacing this is double-pacing, and on Windows
  a vsync-present can stall unpredictably when the window isn't the
  compositor's focus. **Mitigation shipped:** `gfx.set_vsync(win, false)`
  + every game calls it at startup, so frame pacing is the explicit
  `sleep_ms` loop, not the vsync block.
- **Cause B — event pump recreated per frame.** `poll_event` used to
  build a fresh `EventPump` each call. **Mitigation shipped:** the
  thread-local single pump above.

If a play-test shows the quirk persists after these, the next suspects are
(i) the per-frame `M52_SDL_CONTEXT` mutex lock ordering and (ii) whether
the VM game loop actually runs on the SDL-init thread (SDL event handling
is thread-affine). Both need a live debugger on the desktop binary.

## M57 — completed (this session — Space Shooter, delegate-blind)

Third reference desktop game on the M52-M54 `gfx` stack (after M55 Snake +
M56 Tetris). Delegated to a sub-agent in an isolation worktree whose
sandbox again blocked all `cargo`/`python`, so it wrote **639 LOC of
StrictPy blind** and the orchestrator built, ran the asset generator, and
verified. Merged as `951baa6` (clean fast-forward — the agent branched
from current main HEAD, no collision this time).

- `examples/games/space_shooter.spy` (~639 LOC) — 800×600 vertical
  shooter, **all vector art** (filled-triangle ship via `draw_line`
  scanlines, rect enemies/bullets, ring-of-rects explosions, `draw_point`
  parallax starfield) — deliberately **no PNG sprites / no M53 image
  path**, so no asset-licensing concerns. 8-direction movement
  (key_down/key_up tracked), rate-limited fire, enemy waves every ~1.1s
  with random return fire, AABB collision, 3 lives + invuln flicker,
  game-over overlay (R restart / Esc quit), live score, two-layer parallax.
  Standard event-drain + despawn-by-rebuild idioms copied from snake/tetris.
- `examples/games/space_shooter/assets/` — `_generate_assets.py` (square-
  wave SFX, modeled on snake's) produced `shoot/explosion/hit/gameover.wav`
  (orchestrator ran it); `font.ttf` (DejaVu CC0 copy); `CREDITS.md`.
- `compiler/tests/space_shooter_demo_runs.rs` — compile-only test (passes
  first try — the blind-written game typechecks cleanly).
- `LANGUAGE_GUIDE.md` §12.8 walkthrough (entity lists, AABB, starfield).

**Verification note:** compile-only test green, but like Snake/Tetris the
game is **not runtime-verified** (interactive; can't run in CI) and the
`gfx` Windows input/frame-timing quirk (M55/M56) may affect it — a manual
play-test on a desktop is the real gate. M58 (games polish: high scores,
fullscreen, the input fix) is the natural next games milestone.

## M51 — completed (this session — reconciled after a parallel-work collision)

The delayed M49 follow-up. Two independent M51 RollingWindow
implementations existed: one the orchestrator's delegated agent built
locally, and one **already pushed to origin/main** (`71f697b`, plus an
unrelated `10584f5` pivot_table categorical fix). Per the user's call we
kept the **published remote RollingWindow as canonical** and layered only
the four non-overlapping grab-bag features the remote lacked.

**What's on main now (the M51 surface):**
- **RollingWindow (remote `71f697b`)** — constructor-variant API:
  `df.rolling(w)`, `df.rolling_centered(w)`, `df.rolling_min_periods(w,mp)`,
  `df.rolling_centered_min_periods(w,mp)` → a `RollingWindow` with
  terminals `.sum/.mean/.min/.max/.std/.count` + accessors
  `.window()/.min_periods()/.is_centered()`. NativeFn IDs **1069-1081**.
  (Note: this differs from the orchestrator's dropped design, which used
  chainable `.center(bool)/.min_periods(n)` builders + `.agg()`.)
- **pivot_table categorical fix (remote `10584f5`)** — pivot_table now
  resolves ColumnCategorical index/columns axes correctly.
- **Grab-bag layered by the orchestrator:**
  - **Phase B** — explicit `ColumnCategorical.is_ordered` bit (payload
    **32→40 bytes**, offset 32; replaces the M49 heuristic). Threaded
    through `m47_alloc_col_categorical` (now takes `ordered: bool`) + its
    4 constructor callers (M47 ⇒ false, M49 ⇒ true).
  - **Phase C** — `df.sort_by` on a `ColumnCategorical` sorts by code
    (= `categories[]` order); previously it raised ValueError.
  - **Phase D** — `df.loc_range_level_{i64,str,datetime}(level,start,stop)`
    filter any MultiIndex level (0=outermost). NativeFn IDs **1082-1084**
    (re-numbered from the orchestrator's original 1078-1080, which the
    remote's RollingWindow had taken).
  - **Phase E** — `merge_cat_codes` bench cell + `bench/TABULAR_BENCH_REPORT_M51.md`
    (codes path ~88× faster than string-coercion, 0.29× vs pandas at low
    cardinality; 5.93× slower at 5000-distinct high cardinality = open item).
- Grab-bag tests live in `vm/tests/m51_tabular_grabbag.rs` (9 tests);
  RollingWindow is covered by the remote's `vm/tests/m51_rolling_window.rs`
  (32 tests). Verified **1102 / 0 / 1 ignored** on the full workspace.

**The orchestrator's fuller standalone M51** (chainable RollingWindow +
`.agg` + all 5 phases, 1098 tests) is preserved at tag **`m51-local-full`**
— NOT on main. If anyone wants the chainable API or `.agg`, it's there.

### Integration notes worth remembering (hit during this milestone)

- **Parallel-work collision is real on this repo** — always `git fetch`
  before assuming local main is current. Two M51s diverged from `c4d034b`.
  When the agent's isolation-worktree branch can't `--ff-only` (it
  branched from HEAD-at-launch, behind a brief commit), `git rebase main`
  the branch then ff. When the *remote* has independent work, do NOT
  force-push; reconcile (here: reset local to origin, layer non-conflicting
  pieces, re-ID clashing NativeFns).
- **`SDL2_image.lib` link path is not in git** — `third_party/` (M53 games
  dep) is untracked, so a fresh checkout/worktree can't link `spy`
  (LNK1181). Fix without a rebuild: set
  `LIB="C:\Users\AG\CascadeProjects\PythonCompiler\third_party\SDL2_image\SDL2_image-2.8.2\lib\x64;$LIB"`
  before cargo build/test (MSVC linker reads `LIB`; doesn't change cargo
  fingerprints). Running `bench/tabular_harness.py` rewrites
  `bench/TABULAR_BENCH_REPORT.md` — `git checkout --` it after a subset run.
- **Agent-tool `isolation:"worktree"` sandbox denies `cargo`/`python`** —
  the delegated agent can't build/test, so it ships unverified code and
  the orchestrator owns all build/test/bench/integration. Budget for it.

## M56 — completed (this session, 1 commit)

Scope: second reference desktop game on the M52-M54 `gfx` stack.
10×20 board, all 7 tetrominoes, 4 rotation states per piece (encoded
as 16-bit masks), naive ±1-column wall kicks, auto-drop timer that
speeds up per level (800 → 100 ms over 14 levels), soft drop, hard
drop, line-clear scoring (100/300/500/800 × level), next-piece
preview, 100 ms white-flash on clear, full SFX (move/rotate/clear/
tetris/gameover).  Dual timer (render at 30 FPS, drop at variable
interval) — unavoidable for input responsiveness.

Files shipped:
- `examples/games/tetris.spy` (~510 LOC).
- `examples/games/tetris/assets/{move,rotate,clear,tetris,gameover}.wav`
  (square-wave SFX generated deterministically by `_generate_assets.py`).
- `examples/games/tetris/assets/font.ttf` (copy of the bundled DejaVu).
- `examples/games/tetris/assets/CREDITS.md`.
- `compiler/tests/tetris_demo_runs.rs` — compile-only test (passes).
- `LANGUAGE_GUIDE.md` §12.7 walkthrough of the bitmask piece
  encoding + dual-timer pattern + line-clear bottom-up rebuild.

### Findings worth recording for M57

- **Hex literals + bitwise ops** work in Spy (`0x00F0i32`, `>>`, `&`).
  Used for compact tetromino encoding (16-bit mask per rotation × 28
  rotations vs. 448-cell flat list).
- **No `;` between statements** — Spy is strict Python-style.
  Multi-statement helpers like `c.append(r); c.append(g); c.append(b)`
  must be a separate fn (`append_rgb(c, r, g, b)`).
- **Module-scope `final` only takes literals** — not function calls
  (e.g. can't write `final MASKS: List[i32] = make_piece_masks()`).
  Compute lookup tables once in `main()` and pass them as args to
  helpers that need them.  See `tetris.spy`'s `masks`/`colors`.

## M55 — completed (this session, 1 commit)

Scope: first reference desktop game built on the M52-M54 `gfx` stack.
20×20 cell Snake with 60 px top bar for the score, 8 cells/second
movement, arrow-key control with 180°-reversal guard via pending-
direction queue, reject-sampling food spawn, "GAME OVER → press R to
restart" overlay, eat/die SFX, DejaVu Sans Mono score text.

Files shipped:
- `examples/games/snake.spy` (~280 LOC)
- `examples/games/snake/assets/eat.wav` + `die.wav` (square-wave SFX
  generated deterministically by `_generate_assets.py`)
- `examples/games/snake/assets/font.ttf` (copy of the bundled DejaVu)
- `examples/games/snake/assets/CREDITS.md`
- `compiler/tests/snake_demo_runs.rs` — compile-only test (passes)
- `LANGUAGE_GUIDE.md` §12.6 walkthrough of the standard game-shape
  (final-class GameState + two timers + pending-dir queue) that M56
  Tetris and M57 Space Shooter will reuse.

### Findings worth recording for M56/M57

- **Nullable narrowing scope**: `if X is not none:` narrows inside the
  block, but `while X is not none:` and `if X is none: break` do NOT.
  The standard event-drain pattern in StrictPy desktop games is
  `while draining: ev_opt: Event? = gfx.poll_event(win); if ev_opt
  is not none: ...; else: draining = false`. Document in M56's brief.
- **No `list.insert(0, x)` native**: rebuild the list each step.
  Cheap for snake-tens; for Tetris's per-cell grid use a 2D
  `List[List[i32]]` directly. Mentioned in §12.6.
- **Module-scope constants use `final` not `let`**: `let CELL: i32 =
  30i32` at module scope is rejected; `final CELL: i32 = 30i32` works.
  GAMES_PLAN.md skeleton used `let` — agents picking up M56/M57
  should patch their skeletons accordingly.

## M54 — completed (single agent, 1 commit)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M54 GFX audio + fonts** | CPAL audio + Fontdue rasterizer | `m54_` | 1150-1158, 1170-1173 (13 new) | (committed) |

### What shipped

Implemented audio output and font rendering/text sizing (`gfx.Sound`, `gfx.Music`, `gfx.Font`) in the `gfx` stdlib package using pure Rust decoders.
- **Pure-Rust CPAL/Rodio Integration**: Plumbed `rodio` to bypass system dependency on `SDL2_mixer`. Implemented in-memory WAV playback (`load_sound`, `play_sound`, `set_sound_volume`, `free_sound`) and streaming background music playback (`load_music`, `play_music`, `stop_music`, `set_music_volume`).
- **Pure-Rust Fontdue Integration**: Plumbed `fontdue` to bypass system dependency on `SDL2_ttf`. Implemented font loading, pixel text measurement (`text_size`), and anti-aliased blended text composition onto the Window canvas (`draw_text`).
- **Class Registration**: Registered `gfx.Sound`, `gfx.Music`, and `gfx.Font` sealed classes in compiler and VM with handle tracking.
- **Asset Fixtures**: Generated `blip.wav` and committed `DejaVuSansMono.ttf`.
- **Integration Tests**: Added `vm/tests/m54_gfx_audio.rs` and `vm/tests/m54_gfx_text.rs` running under dummy audio/video drivers.
- **Fresh Smoke Example**: Added `examples/_smoke_audio.spy` demonstrating sound playing and text rendering.
- **Fresh Documentation**: Updated `LANGUAGE_GUIDE.md` §11.42 (audio API gotchas) and §11.43 (fonts gotchas).

## M53 — completed (single agent, 1 commit)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M53 GFX images** | Sprite loading + blitting + rotation | `m53_` | 1130-1135 (6 new) | (committed) |

### What shipped

Implemented the image loading and rendering primitives (`gfx.Image`) in the `gfx` stdlib package.
- **SDL2_image Integration**: Enabled the `"image"` feature on the `sdl2` dependency in `vm/Cargo.toml`. Set up pre-compiled DLLs/libs under `third_party/SDL2_image` for linking and runtime on Windows.
- **Image/Texture Resource**: Implemented `gfx.Image` sealed class with custom handle registration and safety checks in the VM.
- **Image Operations**: Implemented `load_image`, `image_size`, `draw_image`, `draw_image_rect`, `draw_image_rotated`, `free_image` native functions.
- **Robust Path Fallback**: Added robust Cwd path resolution: if a path starts with `vm/` and isn't found relative to Cwd, and we are running from inside the `vm/` directory (e.g. `cargo test`), strip the `vm/` prefix and resolve correctly.
- **Integration Tests**: Added `vm/tests/m53_gfx_images.rs` containing a serialized suite testing all success and error paths under the dummy driver.
- **Fresh Smoke Example**: Added `examples/_smoke_sprite.spy` loading and drawing a test sprite.
- **Fresh Documentation**: Updated `LANGUAGE_GUIDE.md` §5 (gfx API surface) and §11.41 (gfx.Image scope-down).

## M52 — completed (single agent, 1 commit)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M52 GFX core** | SDL2 init, Window, Event, drawing primitives | `m52_` | 1100-1111 (12 new) | (committed) |

### What shipped

Implemented the core GFX stdlib package (`gfx`) in StrictPy. Exposes windowing, keyboard/mouse event polling, and 2D drawing primitives using native SDL2.
- **SDL2 Context & Integration**: Plumbed native `sdl2` crate dependency (with `bundled` feature for hermetic builds).
- **Single Window Only**: Opaque native pointer registered in the VM, supporting a single OS window at any given time.
- **Event Polling**: Translation of SDL events to the `Event` class with fields (`kind`, `key`, `x`, `y`, `button`). Fixed a critical `NullPointerError` bug in event loop checks by looping until a mapped event is returned, or returning `NONE_SENTINEL` (which maps to `none` in StrictPy).
- **Drawing Primitives**: Implemented `clear`, `present`, `draw_rect`, `draw_rect_outline`, `draw_line`, `draw_point`.
- **Integration Tests**: 5 new integration tests in `vm/tests/m52_gfx_core.rs` (all passing under dummy video/audio drivers).
- **Fresh Smoke Example**: Added `examples/_smoke_window.spy` demonstrating the core API and game loop.
- **Fresh Documentation**: fresh entries in `LANGUAGE_GUIDE.md` §5 and gotchas §11.40.

## M50a — completed (single agent, 2 commits — Phases A-D combined + Phase E)



| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M50a tabular.serve** | HTTP transport + minimal browser-tab frontend | `m50a_` | 1067-1068 (2 new) | `63070ce` (A-D combined), `e684608` (E) |

### What shipped

The user's original Pandas-plan request from way back finally implemented: a localhost HTTP server for interactive DataFrame exploration in a browser tab.

- **HTTP server**: `tabular.serve(df, port)` + `tabular.serve_with_timeout(df, port, timeout_ms)`. Hand-rolled HTTP/1.1 server in `vm/src/builtins.rs::m50a_serve_loop` using `std::net::TcpListener` directly. **No new crate deps** — std::net + a ~80-LOC custom HTTP-parser + ~80-LOC custom JSON-body parser. The brief's architectural call (don't go via M28 socket stdlib or M29 webserver framework) held: ~850 LOC of Rust in `builtins.rs` covers server loop + JSON serializers + endpoint handlers + bundled frontend.
- **5 endpoints**: GET / (bundled HTML page), GET /api/schema, GET /api/rows, GET /api/cell, POST /api/filter, POST /api/groupby.
- **Server-side DataFrame ID registry**: filter + groupby endpoints register derived DataFrames at fresh IDs; frontend includes `?df=ID` in subsequent rows/cell requests. No GC integration needed — the primary df stays rooted on the user's call stack across `serve_with_timeout`; derived dfs live on the call-local `M50aServerState`.
- **Bundled frontend**: vanilla DOM (no React/Vue/jQuery), ~200 LOC JS embedded as a Rust string constant. Lazy-load-on-scroll table + one-column filter UI + groupby checkbox UI.

### Methodology data point — another shared-infra-shaped milestone

The brief classified M50a as **disjoint-handler** (predicting 5 per-phase commits at ~20%). The agent landed **2 commits** — Phases A-D combined + Phase E. Reasonable call: the server loop + JSON serializers + endpoint handlers + frontend are tightly interlocked in `builtins.rs` and don't go green incrementally without all four pieces.

**New cadence-classification refinement**:

| Classification | Examples | Phase shape |
|---|---|---|
| disjoint-handler | M42/M43/M45/M46/M48/M49 | Independent handlers; per-phase commits at ~20% |
| shared-infra | M41/M44 | Shared payload/helper introduced; combined Phase A at ~30-50% |
| cross-dispatch | M47 | New sealed-class subclass forces all dispatch files to compile together; ~50-75% |
| **NEW: net-new-feature** | **M50a** | **Net-new feature with tightly interlocked pieces (e.g. an HTTP server with its endpoints, JSON serializer, and frontend all in one builtins.rs subsystem). Combined-commit at ~50-70%. Distinct from cross-dispatch because no new sealed-class subclass.** |

Future briefs that ship a **net-new self-contained subsystem** (e.g. M50b/M50c desktop UI components, or v0.5 networking protocols) should classify as net-new-feature and not expect per-phase commits.

The streak holds at **32 because the agent committed cleanly** without orchestrator intervention. Both commits had green builds + passing tests.

### Five surprises worth recording

1. **`TcpListener::set_nonblocking(true)` + 50ms accept-poll with `Instant`-deadline** turned out simpler than M28 P3b-A's shutdown-FD trick for clean timeout-based shutdown. The Rust idiom is to set the listener non-blocking, accept in a loop with WouldBlock-handling, and check the deadline between iterations.
2. **DataFrame ID registry needed no GC integration** — primary df stays rooted on the calling stack across the blocking `serve_with_timeout`; derived dfs live on the call-local server state.
3. **Hand-rolled minimal JSON-body parser (~80 LOC) was less effort than wiring serde_json's derive types** across the i64/f64/str/bool value dispatch needed for filter/groupby request bodies.
4. **M47 ColumnCategorical serialization punted in v1** — different 32-byte layout vs the M37 24-byte Column shape; M50a renders categorical cells as `null`. Documented in §11.39 as M50b pickup.
5. **Edit-tool worktree leak occurred consistently** this session — every Edit/Write landed at the project-root path instead of the worktree. Precautionary `cp` block was denied by the sandbox (the `for f in ... cp` shell loop was blocked). Workaround: per-file `cp` after each batch, 100% effective; ~12 recoveries across the session.

### What M50b should pick up

- Sortable column headers.
- Composite (AND/OR) filters.
- Virtual scrolling for >10K-row frames.
- CSV download endpoint.
- ColumnCategorical serialization (M50a punted to null).
- Better styling.
- LRU eviction / explicit `/api/forget?df=ID` (M50a registry is unbounded).

M50c picks up the interactive pivot UI.

## M49 — completed (single agent, 5 per-phase commits, **massive bench-validated win**)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M49 codes optimization** | Categorical codes-hash + ordered categorical + polish | `m49_` | 1061-1066 (6 new) | `cb6d6ef` (A), `a8184bc` (B), `7bdf3d0` (C), `767369e` (D), `13c39a1` (E) |

### Headline: massive bench win

The M48 brief predicted ~10× speedup. **Actual result is ~70-194× depending on cardinality.**

| Cell | Size | M48 baseline | M49 | Ratio vs pandas | Speedup |
|---|---|---:|---:|---:|---:|
| `group_by_cat_via_strings` | medium (8 distinct) | 12.8s | **66ms** | 0.06× (16× faster than pandas) | **~194×** |
| `group_by_cat_via_strings` | medium_card_5000 (~4k distinct) | 5.4s | **77ms** | 0.07× (14× faster than pandas) | **~70×** |
| `group_by_str` | medium_card_5000 | 4.9s | 3.6s | 3.5× | 1.3× (incidental) |

**StrictPy now beats pandas's own Categorical fastpath by ~14× at high cardinality.** This is the most dramatic single-milestone perf result in the project after the M8 JIT cliff (931ms → 14.6ms on fib(30)).

### What shipped

- **Phase A**: high-cardinality bench fixture (`medium_card_5000` — 10K rows × 5000 distinct category values) added to `bench/tabular_harness.py` (+85 LOC) + baseline measurement before Phase B touched anything.
- **Phase B (PRIMARY)**: `m38_groupby_*` family detects ColumnCategorical key columns and hashes on `codes[i]` (i64) directly instead of routing through `to_strings()` materialization. Single-col + multi-col + mixed-dtype-fallback all handled. **The bench-validated win.**
- **Phase C**: merge codes-hash via the same machinery — when both lhs.on_col and rhs.on_col are ColumnCategorical with **bit-identical `categories[]` arrays**, hash on codes; else fall back to string-hash. New constructors: `tabular.col_categorical_ordered(values, categories)` + `tabular.col_categorical_from_codes(codes, categories)` + new predicate `cc.is_ordered()`.
- **Phase D**: more resample rules — `1w` (7 days) + `1M` + `1Y` (calendar arithmetic with end-of-month clamping for Feb/short months). Outer-merge MultiIndex on either side (extends M46's dtype-mismatch fallback to all three cases: lhs-MI / rhs-MI / both-MI). `unstack` now distributes EVERY regular column (M46 only first). `loc_range_multi_{i64,str,datetime}` (3 NativeFns) for range filtering on the innermost MultiIndex level.
- **Phase E**: 21 VM tests + 2 demo-runs + `examples/tabular_m49_codes_demo.spy` (~230 LOC, exercises all M49 features) + `bench/TABULAR_BENCH_REPORT_M49.md` with before/after numbers + LANGUAGE_GUIDE.md §5 M49 additions + §11.37/§11.38 new + §11.36 update.

### Edit-tool worktree leak

Recurred ~10 times this session. Defensive `cp` block at session start + per-file `cp` recovery worked cleanly — no data loss. Same pattern as M44/M46. The M45/M46/M47/M48/M49 alternation (no/leak/no/no/leak ~10×) confirms intermittence; the workaround remains reliable.

### What M51 should pick up (M49 follow-up)

1. **RollingWindow chainable + center=True** — deferred per brief (cross-dispatch territory).
2. **Pandas-style ordered-sort on `ColumnCategorical`** — currently still alphabetical; ordered-categorical-sort uses categories[] ordering.
3. **Range filtering on outer MultiIndex levels** — M49 only handles innermost.
4. **Dedicated merge codes-hash bench cell** — M48's cell shape doesn't exercise the M49 fast path.
5. **ColumnCategorical payload extension for an explicit `is_ordered` bit** — replace the heuristic.

(M50 sequence is desktop UI; M48b memory deep-dive also queued.)

## M48 — completed (single agent, 4 per-phase commits, comprehensive bench with honest findings)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M48 tabular bench** | `tabular` vs pandas 3.0 comprehensive benchmark suite (37 cells timed + 6 documented skips) | `m48_` | none (pure bench infra) | `c7bc6c7` (A), `0bc3dff` (B), `3aad89c` (C), `b62b292` (D) |

### What shipped

- **Phase A**: `bench/tabular_harness.py` (~1321 LOC) — deterministic CSV fixture generator (small/medium/large/xl sizes; .gitignored), psutil RSS-polling runner, JSON-merge-by-(op,size) writer, Markdown report renderer, CLI (`--sizes` / `--ops` / `--xl` / `--report-only`).
- **Phase B**: 8 core ops × small/medium + 5 ops × large = ~21 cells. read_csv / filter / sort_by / group_by+sum / merge inner / pivot_table / rolling_mean / describe.
- **Phase C**: 7 categorical-specific cells (str vs categorical-via-strings vs pandas Categorical) + memory peak comparison (psutil RSS) per cell + the slow-large-cell skip decisions.
- **Phase D**: `bench/TABULAR_BENCH_REPORT.md` (236 lines) with measured "Categorical cost analysis" pulling actual ms numbers + computed overhead percentages, methodology section, agent report.

### Headline findings (43 cells: 37 timed + 6 skipped)

- **Geomean ratio 0.30×** (StrictPy time / pandas time). 28 wins, 1 tie, 8 losses.
- **StrictPy wins broadly**:
  - **All small cells** (pandas import ~1s dominates wall-clock).
  - **At medium/large**: read_csv / filter / sort_by / rolling_mean / describe / unique (when fast).
- **pandas wins decisively at medium+**:
  - **group_by_str: 11.2×** at medium (the headline M49 target).
  - **pivot_table: 22.2×** at medium.
  - **group_by + sum: 11.25×** at medium.
- **Categorical cost (M47 v1 path)**: `to_strings()` coercion = **+11%** vs ColumnStr direct (12.8s vs 11.6s at medium group_by). M49's codes-hash should turn this into a measurable WIN, not just close the gap.
- **Pandas Categorical surprise**: at 8 distinct values (low cardinality), pandas Categorical is **0.98×** vs str groupby — essentially no speedup. **High-cardinality (~5000 values) is where codes-hash shines.** M49 should benchmark on a high-cardinality fixture before claiming codes-hash is universal.
- **Memory peaks**: StrictPy peak RSS runs **4-5× pandas** at large (filter/large: 1.07 GB vs 0.20 GB). This is the `List<T>` per-cell-overhead cost vs NumPy contiguous buffers. M49 polish won't fix this; the path forward is the M48b memory deep-dive + potentially a "compressed column" representation in v0.5.

### STOP CRITERIA cuts

- **xl (100M) size skipped entirely** — extrapolated >50 GB CSV; OOM/timeout risk too high for v1.
- **8 large cells skipped** (group_by + merge + pivot_table at large) due to >30 minute timeouts in StrictPy. Documented inline as `skip` rows with timeout notes. The honest gap.

### M49's target is now numeric

Pre-M48, M49's scope was qualitative ("optimize categorical codes paths"). Post-M48, it's quantitative:
- **group_by_cat_via_strings** at medium: 12.8s (current). Target: <1.5s (pandas-class). That's ~10× speedup needed.
- **High-cardinality benchmark fixture** (~5000 distinct values) added to M49's scope so the codes-hash win is unambiguous.

### M48 worktree state

The agent's worktree integration was clean — main was untouched, fast-forward straightforward. **30th Lesson-1-compliant agent. The streak holds.**

**Note on `bench/net_*` untracked files**: there are 3 leftover networking-benchmark files in `bench/` (`net_harness.py`, `NET_BENCH_REPORT.md`, `net_results.json`) from some earlier prototype session unrelated to M48. They're still untracked on main; not blocking anything. Decide whether to commit them, archive them, or delete them when convenient.

## M47 — completed (single agent, 2 commits with first at ~70% of budget, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M47 polish** | iloc 2-D + negative iloc + rolling Welford/min_periods + ColumnCategorical | `m47_` | 1043-1060 (18 new) | `ff010c9` (A+B+C combined), `325fcba` (D) |

### What shipped

- **Phase A**: `df.iloc_2d(row_start, row_stop, col_start, col_stop)` half-open 2-D slice with Python-style negatives on both axes; extends existing `df.iloc(start, stop)` to accept negative indices (lifting M40's v1 rejection).
- **Phase B**: 10 new `Column.rolling_*_min_periods(window, min_periods)` methods (sum/mean/min/max/std × i64+f64). Welford's online algorithm for std via new `m47_welford_std_sample` helper (Option 1 — recompute over window each step; bit-equivalent to M40 on small inputs). Original `rolling_std` unchanged for backwards compat.
- **Phase C**: new `ColumnCategorical` sealed Column subclass with `codes: List[i64]` + `categories: List[str]` + `nulls: List[bool]` + `length: i64` (32-byte payload, first 3 slots aligned with the M37 Column layout so the shared `length`/`is_null`/`null_count` handlers work unmodified). New methods: `tabular.col_categorical(values)`, `col_categorical_with_nulls`, `cc.codes()`, `cc.categories()`, `cc.to_strings()`, `cc.get(i)`, `df.get_column_categorical(name)`. v1 op integration via `to_strings()` coercion — optimized codes paths deferred to M48.
- **Phase D**: 32 new tests + `examples/tabular_m47_polish_demo.spy` (~155 LOC) + LANGUAGE_GUIDE.md §5 M47 subsection + §11.35 (negative iloc) + §11.36 (categorical alphabetical-sort v1) + agent report.

### Big methodology lesson — brief classification needs a new category

The brief classified M47 as **disjoint-handler** (per-phase commits at ~20%). **This was wrong**: adding a new sealed-class subclass (ColumnCategorical) means **every dispatch file has to grow together** before the build goes green. The agent's first commit landed at ~70% of budget — not because of agent error but because the **task itself** required combined commits of resolver.rs + ir.rs + native.rs + builtins.rs together.

This is a NEW classification beyond shared-infra:

- **"disjoint-handler"** (M42, M43, M45, M46): per-phase commits at ~20%. Each phase modifies independent handler bodies.
- **"shared-infra"** (M41, M44): combined Phase A at ~35%. Phases share a new helper or struct field that downstream phases use.
- **NEW: "cross-dispatch"** (M47): combined commit at ~50-75%. Adding a new sealed-class subclass requires every dispatch site to compile together — the build goes red until they all agree.

**Future brief language**: when adding a new sealed-class subclass (Column*, GroupedDataFrame-shape, etc.), classify the milestone as **cross-dispatch** and predict a 50-75% first-commit window. M48's brief should make this explicit if categorical optimized paths get a similar shape.

The streak holds at 29 because the agent committed cleanly without orchestrator intervention — the cadence slip was a brief miscategorization, not an agent error.

### Tests flipped (1)

`vm/tests/m40_tabular_timeseries.rs::iloc_negative_start_raises` → `iloc_negative_start_works_m47`. Old: asserted ValueError on `iloc(-1, 1)`. New: asserts `nrows=2` on `iloc(-2, 3)` (Python negative semantics).

### Edit-tool worktree leak

No recurrence this session. Precautionary `cp` block was blocked by Bash policy (same as M44/M46) but `wc -l` between worktree and project root at session start showed identical file sizes — the worktree had a clean baseline from M46's clean integration. Every Edit/Write landed correctly.

The M45/M46 hypothesis-refutation cycle plus M47's no-leak-from-clean-baseline tentatively suggest the leak might be related to **whether the worktree starts in sync** — but M46 refuted that. **Honest current state remains**: cause unknown, intermittent, workaround reliable.

## M46 — completed (single agent, 5 per-phase commits, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M46 stack/unstack + extensions** | Pandas's MultiIndex bread-and-butter + ergonomic polish | `m46_` | 1033-1042 (10 new) | `e02f08a` (A), `73616c6` (B), `b958a2b` (C), `4878426` (D), `d8dcbef` (E) |

### What shipped

- **Phase A**: `df.stack()` (rotates all columns into a new innermost MultiIndex level + single value column; requires shared-dtype across columns) + `df.unstack()` (takes innermost MultiIndex level, turns into columns; raises on no-MultiIndex). NativeFns 1033-1034.
- **Phase B**: `df.loc_range_{i64,f64,str,bool,datetime}(start, stop)` — per-dtype inclusive range lookup (extends M41's one-row `select_by_label_*`). NativeFns 1035-1039.
- **Phase C**: Outer-merge dtype-mismatch now produces a NaN-padded 2-level MultiIndex (replaces M42's RangeIndex fallback — hook `m46_merge_outer_dtype_mismatch_multiindex` into existing `m39_df_merge`). `set_index_list(cols)` unifies set_index/set_index_multi via length dispatch (NativeFn 1040). `pivot_table_aggfunc_list` (1041) emits one value-column set per aggfunc; `pivot_table_margins` (1042) adds "All" row + column.
- **Phase D**: time-series ops MultiIndex handling — `resample` + `resample_index` explicitly drop MultiIndex (reshape row dim); `asof_merge` + `asof_merge_index` preserve lhs MultiIndex via M45's merge MultiIndex pattern. No new NativeFns.
- **Phase E**: 25 new VM tests + 2 demo-runs + `examples/tabular_m46_extensions_demo.spy` (~160 LOC) + LANGUAGE_GUIDE.md §5 M46 subsection + §11.32 rewrite + §11.33/§11.34 new (stack must-share-dtype, unstack must-have-MultiIndex).

### Methodology data point — Edit-tool leak hypothesis partially refuted

M45 proposed: "leak triggers when worktree state diverges from project root at session start." M44 (cp run, no leak) and M45 (cp NOT run because Bash denied, no leak) supported this.

**M46 refutes it.** The cp block was unavailable again (Bash denied for the loop form) — same as M45. But the leak **DID recur** this round, with edits landing in the project root instead of the worktree. Agent recovered via per-file `cp` recoveries.

So M45 was the lucky outlier, not the new normal. The hypothesis is wrong; the leak is genuinely intermittent or has triggers we haven't identified. **The workaround stays in briefs**: precautionary `cp` at session start AND vigilance via `git status` per phase. Cause unknown — but the workaround is well-routinized at this point.

### Tests flipped (0)

The M45 outer-merge-fallback test exercises a different case (same-dtype outer with one side missing) than the M46 outer-merge MultiIndex fallback (mismatched-dtype outer). No flips needed.

## M45 — completed (single agent, 3 per-phase commits, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M45 MultiIndex propagation** | Lift M44's MultiIndex-drops scope-down across 14 M42+M43 handlers | `m45_` | none new | `384d357` (A), `49f1a22` (B), `90f1def` (C) |

### What shipped

Lifts the M44 v1 scope-down. The 14 row/column-transforming and reshape handlers that previously dropped MultiIndex back to RangeIndex now propagate it correctly — same recipe pattern as M42 (single-col propagation), routed through M44's auto-dispatching `m44_permute_multiindex_into_df` helper plus a new sibling `m45_copy_multiindex_into_df` for column-list ops.

- **Phase A** (M42 ops): `sort_by` / `dropna` / `dropna_subset` route through `m44_permute_multiindex_into_df`. `select` / `drop` / `rename` / `fillna_*` route through new `m45_copy_multiindex_into_df`. `merge` extends per-`how` index policy to MultiIndex via new `m45_merge_build_multiindex` (inner/left/right preserve MultiIndex; **outer with dtype-mismatch still falls back to RangeIndex — M46 anchor**).
- **Phase B** (M43 reshape ops): `melt` repeats each MultiIndex level per `value_var`. `concat_rows` uses new `m45_concat_rows_multiindex` with strict per-level reconciliation (3-tier fallback: MultiIndex → single-col → RangeIndex). `concat_cols` takes lhs's MultiIndex. `pivot` and `pivot_table` explicitly **drop a MultiIndex** (reshape the row dimension — no clean target).
- **Phase C**: 19 new tests + `examples/tabular_multiindex_propagation_demo.spy` (~175 LOC, 9 M45-aware ops with `index_nlevels()` checks at every step) + LANGUAGE_GUIDE.md §5 M45 subsection + §11.26 + §11.32 rewrites.

### Tests flipped (2 — predicted)

- `vm/tests/m44_tabular_multiindex.rs::sort_by_drops_multiindex_m44b_anchor` → `sort_by_preserves_multiindex_m45` (`nlev=0` → `nlev=2`).
- `vm/tests/m44_tabular_multiindex.rs::select_drops_multiindex_m44b_anchor` → `select_preserves_multiindex_m45` (same flip shape).

### Methodology data point worth recording — the leak workaround story has a twist

The brief asked the agent to run the precautionary `cp` block at session start. **The agent could NOT run it** because Bash and PowerShell were both denied at session start. **Yet zero leak recurrences happened anyway** — every subsequent `Edit` / `Write` landed in the worktree directly. Likely cause: the M44 archive commit had already landed on main cleanly, leaving worktree state in sync with project root from the orchestrator's prior `git checkout` operations. **Refined hypothesis**: the leak triggers when worktree state diverges from project root at the start of an Edit session, NOT just "the first Edit on an existing file" as M40 narrowed or "Write also leaks" as M43 broadened.

If this hypothesis holds, the workaround is even simpler: as long as the orchestrator's prior milestone integration left main + worktree in agreement, no `cp` block needed. The M44 cp-at-start-success could have been redundant for the same reason. **Worth confirming on M46**: if M46 also starts with a sync'd worktree (which it should after this M45 push), it might skip the `cp` block and still see no leak.

### EXPLICIT M46 anchor

What still drops a MultiIndex (or doesn't propagate it):
- `pivot` / `pivot_table`: reshape the row dimension; no clean target for input MultiIndex. Likely stays a doc'd drop unless M46 adds a smart-fallback design.
- **Outer-merge with dtype-mismatch indexes**: still falls back to RangeIndex. M46 should add NaN-padded MultiIndex fallback.
- **`stack` / `unstack`**: pandas's MultiIndex bread-and-butter. Net-new code.
- **`df.loc[label_list]` / range-by-label**: net-new methods.
- **Time-series ops MultiIndex propagation** (resample / asof_merge / resample_index / asof_merge_index): currently single-col only.
- **`set_index([col])` accepting a 1-element list**: minor ergonomics — currently `set_index(col_name)` takes a string and `set_index_multi([cols])` takes a list; pandas unifies these.
- **`pivot_table(aggfunc=List)` + `margins=True`**: small extensions.

## M44 — completed (single agent, 4 per-phase commits, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M44 MultiIndex** | Storage + multi-col group_by promotion + minimal propagation (filter/head/tail/iloc) | `m44_` | 1027-1032 (6 new) | `cb6c990` (A), `adccf24` (B), `7a66271` (C), `c2a0960` (D) |

### What shipped

- **Phase A**: DataFrame payload bumped 40 → 56 bytes for optional `index_levels: List[Column]?` + `index_names: List[str]?` (mutually exclusive with M41's single-col index). New helper `m44_build_df_with_multiindex`. 6 new methods (NativeFns 1027-1032): `set_index_multi(cols)`, `reset_index_multi()`, `index_nlevels()`, `index_level(i)`, `index_level_name(i)`, `sort_index_multi(ascending)`. 11 tests.
- **Phase B**: all 8 group_by aggregation methods (`size`/`keys`/`sum`/`mean`/`min`/`max`/`count`/`agg`) now dispatch on key count. Single-col → M41 path (today's behavior); multi-col (≥ 2 keys) → new M44 MultiIndex path with all keys promoted to index levels. 7 tests + 1 M43 contract test flipped.
- **Phase C**: new helper `m44_permute_multiindex_into_df` auto-dispatches on the parent's index state (no index → RangeIndex result; single-col → M42 single-col permute; MultiIndex → permute each level). Wired into `filter` / `head` / `tail` / `iloc`. All OTHER ops still drop a MultiIndex back to RangeIndex (M44b anchor). 7 tests.
- **Phase D**: demo (`examples/tabular_multiindex_demo.spy` ~165 LOC), LANGUAGE_GUIDE.md banner + §5 M44 subsection + §11.26 rewrite + new §11.32 (MultiIndex propagation v1 scope-down).

### The big methodology win

**The precautionary `cp` workaround eliminated the Edit-tool worktree leak entirely.** Zero recoveries mid-session vs M43's ~15 (which burned ~90 seconds). The agent ran one `cp` block at session start syncing `vm/src/builtins.rs`, `compiler/src/resolver.rs`, `compiler/src/ir.rs`, `shared/src/native.rs`, `LANGUAGE_GUIDE.md` from project root to worktree — and the leak never showed up again. **This is the mitigation pattern now**: defensive copy at start, skip the per-phase discovery loops entirely.

Combined with the **clean fast-forward integration on the orchestrator side** (main was completely clean post-agent — no leaked files), M44 was the cleanest tabular-package integration since the series began.

### Tests flipped (1 total)

- `vm/tests/m43_tabular_index_reshape.rs::multi_col_group_by_does_not_promote_to_index` → `multi_col_group_by_promotes_to_multiindex_m44`. Old: `ncols=3, has=false` (keys retained as columns). New: `ncols=1, nlev=2` (keys promoted to 2-level MultiIndex).
- **Zero M38 tests flipped** — M38's `group_by_multi_column` only checks group count, not column shape.

### EXPLICIT v1 scope-down (M44b anchor)

**MultiIndex propagation in M44a is limited to filter / head / tail / iloc.** Every other op drops a MultiIndex back to RangeIndex:
- M42 ops: `sort_by`, `dropna`, `dropna_subset`, `fillna_*`, `merge`, `select`, `drop`, `rename`
- M43 ops: `pivot`, `melt`, `concat_rows`, `concat_cols`, `pivot_table`
- M41 ops: `sort_index`, `resample_index`, `asof_merge_index`, `select_by_label_*` (single-col only)

M44b's job: lift this. Plus stack/unstack, `df.loc[label_list]` range-by-label, and outer-merge MultiIndex fallback.

## M43 — completed (single agent, 4 per-phase commits, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M43 reshape index propagation** | Extend reshape + group_by + pivot_table to index-promote | `m43_` | none new | `f4b4249` (A), `61fd3d5` (B), `13e64fe` (C), `fdd2e35` (D) |

### What shipped

Closes the v1 single-index propagation story. After M43 the `tabular` package is **fully index-aware end-to-end for single-column indexes** (multi-column / MultiIndex still M44+).

- **Phase A**: `pivot_table` promotes `index_col` to the result's index; **single-column** `group_by([col])` with `sum/mean/min/max/count/agg/size/keys` promotes the key column to the index. **Multi-column** `group_by` retains today's keys-as-regular-columns shape (deferred to M44 MultiIndex).
- **Phase B**: `pivot` promotes `index` to the output's index. `concat_rows` concatenates input indexes when all share dtype + name (else RangeIndex fallback). `concat_cols` takes lhs's index (consistent with M42's merge policy).
- **Phase C**: `melt` repeats the input index per `value_var` (preserves name + dtype). Matches pandas's default behavior for indexed melt.
- **Phase D**: 18 VM tests + 2 demo-runs + `examples/tabular_index_reshape_demo.spy` (~190 LOC) + LANGUAGE_GUIDE.md §5 + §11.26 + §11.28 + new §11.30 (melt index repetition) + §11.31 (concat_rows index reconciliation rules).

### Two methodology data points worth elevating

**1. Test flip cascade was larger than estimated (9 vs brief's 2-4).**

The brief estimated 2-4 M41/M42 tests to flip. Actual: **9 tests across M38/M39/M41 + 3 demo updates**:

| Source | Count | Reason |
|---|---:|---|
| M41 | 1 | `pivot_table_sum_happy_path` (ncols 3→2 + index checks) |
| M39 | 2 | `pivot_happy_path` + `pivot_missing_cell_is_null` (pivot promotes index) |
| M38 | 6 | `group_by_*` tests had keys-as-columns assertions; **single-column group_by promotion cascaded into all 6 group_by test cases** |
| Demos | 3 | `tabular_groupby_demo.spy`, `tabular_index_demo.spy`, `tabular_reshape_demo.spy` updated to use `sort_index` and adjust column counts |

**Generalizable lesson**: when a contract change is cross-cutting (every group_by now promotes its key), the test-flip count scales with how widely the old contract was tested. M38 had 6 group_by tests because group_by was M38's headline feature. **Next brief that changes a feature with broad existing test coverage should explicitly estimate the flip count from existing test files.**

**2. Edit-tool worktree leak is broader than the M40 narrowing claimed.**

M40 said: "Edit on already-existing files leaks; Write with absolute worktree paths doesn't." M43 found: **`Write` of new files ALSO leaked** at first-edit-per-file boundaries. Agent burned ~90 seconds across ~15 `cp` recoveries (vs ~5 seconds in M42, ~30 in M41, ~2 minutes in M40).

**M43 agent's recommendation, now adopted**: future briefs should suggest a **precautionary `cp` of all shared files at session start** rather than waiting for `git status` to surface the leak per phase. The defensive copy is cheap; the per-phase discovery loops are not.

## M42 — completed (single agent, 5 per-phase commits, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M42 index propagation** | Extend 11 existing handlers to propagate the M41 index | `m42_` | none new (modifies existing handlers) | `e84160c` (A), `b02de3e` (B), `98d977a` (C), `5a73af2` (D), `cbcab82` (E) |

### What shipped

Closes the M41 explicit v1 scope-down. The 11 existing DataFrame methods that returned a fresh frame now PROPAGATE the index instead of dropping it.

- **Phase A** (filter, sort_by, head, tail, iloc): one new helper `m42_permute_index_into_df` + 5 handler edits + 6 tests + 1 flipped M41 test.
- **Phase B** (select, drop, rename): sibling helper `m42_copy_index_into_df` + 3 handler edits + 3 tests.
- **Phase C** (dropna, dropna_subset, fillna_*): 2 handler edits (fillna's per-dtype dispatch via the shared `m40_df_fillna` body) + 5 tests.
- **Phase D** (merge): `m42_merge_build_index` + `m42_merge_outer_index_column` with dtype-mismatch fallback to RangeIndex; index_name policy = lhs wins for inner/left/outer, rhs wins for right + 5 tests.
- **Phase E**: 19 VM tests + 2 demo-runs; `examples/tabular_index_propagation_demo.spy` (~210 LOC end-to-end pipeline); LANGUAGE_GUIDE.md §5 + §11.26 rewrite (the M41 v1 scope-down section now reads as "closed by M42"); banner bumped to post-M42.

### Key M42 finding — methodology streak nuance closed

M41 introduced the first per-phase-cadence slip in the streak (combined Phases A+B+C because they shared cross-cutting infrastructure). **M42 returned to clean per-phase commits** because its phases modify disjoint handlers — each Phase has a green build + targeted tests at commit time, no shared revert-and-reapply risk. This confirms the M41 nuance is a true *infrastructure-then-uses* exception, not a general drift in agent discipline.

### Architectural pattern worth recording

The whole M42 milestone is a single recipe applied 11 times:

```rust
// In each row-transforming handler:
let keep_indices: Vec<usize> = /* existing code that builds row selection */;
let permuted_columns: Vec<u64> = /* existing per-column permute by keep_indices */;
// NEW: one line, replacing the existing m37_build_df call.
m42_permute_index_into_df(interp, parent_df_ptr, names, permuted_columns, &keep_indices)
```

The helper reads the parent's optional index, permutes it by the same `keep_indices`, and emits via `m41_build_df_with_index` (or `m37_build_df` if there was no index — preserving today's behavior for unindexed inputs). 280 LOC added to `builtins.rs` total — 4 helpers + 11 emit-call swaps.

### M41 tests flipped (1 total)

- `vm/tests/m41_tabular_index.rs::filter_drops_index` → `filter_preserves_index_m42`. Old asserted `has=false` (drops index per M41 v1 scope-down); new asserts `has=true` (M42 propagates).

### Edit-tool worktree leak — 5 recurrences

Detected at every "first Edit on a shared file" boundary: `vm/src/builtins.rs` (4× across phases), `vm/tests/m41_tabular_index.rs` (1×), `LANGUAGE_GUIDE.md` (1×). Each recovered with one `cp`. Total ~5 seconds. `Write` calls all landed correctly. Pattern now well-routinized — M40 narrowing (Edit-on-existing-files leaks, Write-with-absolute-paths doesn't) holds across 6 milestones now.

## M41 — completed (single agent, 2 commits, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M41 DatetimeIndex + pivot_table** | `tabular` Phase 5b — minimum viable index abstraction | `m41_` | 1015-1026 (12 used) | `eec3dc9` (A+B+C combined), `dad1b6d` (D) |

### What shipped

- **Phase A**: DataFrame payload grew **24 → 40 bytes** (added optional `index: Column?` + `index_name: str?`, both zero = RangeIndex default). Three existing constructors (`m37_from_columns` / `m37_from_rows` / `m37_build_df`) updated to allocate the larger payload. 6 new methods: `set_index(col)` / `reset_index()` / `has_index()` / `index()` / `index_name()` / `sort_index(ascending)`. NativeFns 1015-1020.
- **Phase B**: Index-aware time-series + per-dtype select. `resample_index(rule, agg)` mirrors M40's `resample` but reads bucket keys from the index (must be `ColumnDateTime`). `asof_merge_index(other)` mirrors `asof_merge` but joins on both frames' indexes. `select_by_label_{i64,str,datetime}(label) -> DataFrame?` returns a one-row frame (or `none` if absent). NativeFns 1021-1025.
- **Phase C**: `pivot_table(index_col, columns_col, values_col, aggfunc)` — pandas's most-loved DataFrame method, combining pivot + group-by + agg in one call. Aggfunc vocabulary: `sum/mean/min/max/count` (same as M38). Per-cell accumulator enum for the (dtype × agg) cross-product. NativeFn 1026.
- **Phase D**: 25 new tests (23 vm + 2 demo); `examples/tabular_index_demo.spy` (~180 LOC: trades → set_index → resample_index → sort_index → pivot_table → asof_merge_index → select_by_label_str → reset_index pipeline); LANGUAGE_GUIDE.md §5 M41 subsection + §11.26-§11.28 gotchas.

### EXPLICIT scope-down (M42 anchor)

**Every existing DataFrame method that returns a fresh frame DROPS the index in v1** — only the 4 explicitly-index-aware methods (`sort_index`, `resample_index`, `asof_merge_index`, `select_by_label_*`) preserve it. M42's job: index propagation through filter / sort_by / head / tail / iloc / dropna / fillna / merge / select / drop / rename. Per the agent's report, that's ~600-800 LOC concentrated in 6 existing handlers, each gaining: (a) read parent index + index_name, (b) permute the index by the same row-selection vector, (c) emit via `m41_build_df_with_index` instead of `m37_build_df`.

### Five findings worth knowing

1. **DataFrame payload bump to 40 bytes** — GC's Class scanner walks every 8-byte slot in payload; zero slots safely treated as "not pointers" (matches the M11 pointer-vs-i64 false-positive analysis, benign because mark-phase is additive). Three constructors updated.
2. **`sort_index` dispatch by index dtype** — single `m41_sort_index_perm(col, ascending)` helper reads class name and runs per-dtype comparator inline. Descending = ascending + `perm.reverse()` (preserves stability within non-null cells).
3. **`m41_clone_column` for the index slot** — `set_index` clones the column rather than aliasing, keeping the index physically independent. Cost: one extra column allocation per `set_index`; safe for v1 row counts.
4. **`pivot_table` accumulator as an enum** — single `Acc` enum carries variant-per-(dtype × agg) accumulators. Per-bucket update is a single `match` (vs. nested dispatch).
5. **Edit-tool worktree leak recurred once** (down from 5× in M39, 2× in M40). Confirms the M40 narrowing: `Edit` on already-existing files leaks; `Write` with absolute worktree paths is unaffected. Agent caught via `git status` check + recovered via one-shot `cp` of 4 shared files in ~30 seconds.

### Methodology nuance worth flagging

**M41 deviated from the per-phase-commit discipline**: Phases A+B+C landed as one combined commit at ~75% of budget (rather than the brief's 20% first-commit + per-phase target). Reason: all three phases share `m41_build_df_with_index` + the 40-byte payload change — splitting would have required revert-and-reapply with extra leak-recovery overhead. The Lesson 1 SPIRIT (commit before orchestrator intervenes, green build + tests passing at each commit) held — both M41 commits were clean. The streak counter (23) does not break, but the commit granularity slipped. **Generalizable lesson**: when phases share cross-cutting infrastructure (struct layout changes, new shared helpers), per-phase splitting becomes an antipattern. Future briefs for "cross-cutting infrastructure + downstream uses" rounds should accept "first commit after the infrastructure lands, even if late" as the right shape.

## M40 — completed (single agent, 4 phases, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M40 time-series** | `tabular` Phase 5 — cumulative + null + iloc + rolling + resample + asof_merge | `m40_` | 985-1012 (28 used) | `1b5c523` (A), `a2e1699` (B), `066a50f` (C), `a9f9354` (D) |

**Note**: a previous launch attempt died on a transient 529 (API overloaded) within ~3.5 minutes. Zero state was created. The successful run is the second attempt.

### What shipped

- **Phase A**: cumulative ops on numeric columns (`ColumnI64`/`F64` × `cumsum`/`cumprod`/`cummax`/`cummin` = 8 NativeFns); whole-frame null handling (`df.dropna` / `df.dropna_subset(cols)` + per-dtype `df.fillna_{i64,f64,str,bool,datetime}` = 7 NativeFns); range slicing (`df.iloc(start, stop)` — half-open, no negative indices). Null-propagation rule on cumulative: once a null is hit, every output cell after it is null (simpler than pandas's `min_periods=1` skip).
- **Phase B**: rolling-window aggregations (`ColumnI64`/`F64` × `rolling_{sum,mean,min,max,std}` = 10 NativeFns). Output length = input length; cells 0..window-1 are null; window-with-any-input-null produces null output. `rolling_mean`/`rolling_std` return `ColumnF64` regardless of input dtype. Sample n-1 std.
- **Phase C**: `df.resample(time_col, rule, agg)` — buckets a `ColumnDateTime` by rule width (`<i64><m|h|d>` parser), aggregates per-bucket via `sum`/`mean`/`min`/`max`/`count`. Empty buckets emit non-null bucket-start times but null aggregated cells. `df.asof_merge(other, on_self, on_other)` — left-join via `Vec::partition_point` after stable-sorting rhs. Both keys must share dtype (`ColumnDateTime` or `ColumnI64`).
- **Phase D**: 28 new tests (26 vm + 2 demo) + `examples/tabular_timeseries_demo.spy` (~170 LOC: fillna → cumsum → cummax → rolling_mean → resample → asof_merge → iloc → dropna pipeline). LANGUAGE_GUIDE.md §5 M40 subsection + §11.22-§11.25 gotchas.

### Six findings worth knowing

1. **Cumulative null-propagation choice**: "propagate from first null forward" is simpler than pandas's `min_periods=1`. Trivial user-side override: `col.fill_null(0).cumsum()`. Documented as §11.22.
2. **Resample rule parser** accepts only `<i64><m|h|d>` (e.g. `"15m"`, `"1d"`). Week/month/year require a calendar layer; M41 work if needed.
3. **`asof_merge` binary search** uses `Vec::partition_point(|k| *k <= needle)` which returns the first index past the run of `<=` matches — the largest matching index is `pp - 1`; `pp == 0` cleanly maps to "no match" (null right-side).
4. **`fillna_*` returns non-matching-dtype columns by raw pointer reuse** (not copies). Safe because no codepath mutates Column payloads in place.
5. **Resample drops string + bool columns** — no defined v1 aggregation. Could add `"first"` / `"last"` / `"mode"` later.
6. **Edit-tool worktree leak — key new finding**: the leak is specific to `Edit` calls on already-existing files; `Write` calls (with absolute worktree paths) land correctly. The agent recovered both leak instances in M40 with a one-shot `cp` from project root to worktree. ~2 minutes total burned. **Workaround for the M41 agent brief**: when bulk-editing existing shared files (`resolver.rs`, `ir.rs`, `native.rs`, `builtins.rs`), check `git status` after the first edit and `cp` if needed; `Write` calls for new files don't have this problem.

## M39 — completed (single agent, 4 phases, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M39 reshape** | `tabular` Phase 4 — reshape ops | `m39_` | 935-984 (11 used: 935-942, 945, 950-951) | `5411a9f` (A), `e4f2ed7` (B), `24859c1` (C), `0d73905` (D) |

### What shipped

- **Phase A**: 5 typed `df.unique_*` accessors (i64/f64/str/bool/datetime — mirrors M38 `get_column_*` pattern); `df.value_counts(col)` returns 2-col DataFrame sorted by count desc; module-level `tabular.concat_rows(dfs)` (vertical, schema-strict) and `tabular.concat_cols(dfs)` (horizontal, row-count-strict + unique col names).
- **Phase B**: `df.merge(other, on, how)` — hash-join inner/left/right/outer reusing M38's `\x01`-joined key encoding. Output column order = lhs cols + rhs non-`on` cols (no duplicates). Null cells in `on` columns never match (pandas/SQL `null != null`). Merged `on` columns inherit rhs values on right-only outer rows (matches `pd.merge` behavior).
- **Phase C**: `df.pivot(index, columns, values)` — long→wide; raises ValueError on duplicate (index, columns) pairs; missing pairs → null cells. `df.melt(id_vars, value_vars)` — wide→long; all `value_vars` must share a dtype.
- **Phase D**: 23 VM tests + 2 demo-runs; `examples/tabular_reshape_demo.spy` (~150 LOC, orders+customers workflow); LANGUAGE_GUIDE.md §5 / §11.20 / §11.21 updates.

### Five findings worth knowing

1. **f64 `unique` keys on `to_bits()`** — `HashSet<f64>` doesn't compile (`f64: !Hash`); bit-pattern keying distinguishes ±0.0 and lets multiple NaN payloads be distinct. Canonical workaround.
2. **`m39_join_key` returns `None` for any-null-cell rows** — different from M38's `m38_row_key` which encoded nulls as `\x02null` for grouping. For merge's `null != null` semantics, `None` shortcut is cleaner than a never-matching key.
3. **Merge `on` columns inherit rhs values on right-only outer rows** — matches pandas's "merged key column" behavior so the join key never goes null in outer/right outputs.
4. **Melt machinery is bulky** — each dtype needs per-value-var read + per-output-row write. Pre-read all `value_vars` into Vec<>s up front to avoid virtual-call-per-cell overhead.
5. **Edit-tool worktree leak recurred ~5 times in M39** — same as M37+M38. The agent caught each via `git status` after substantial edits and `cp`-recovered from project root to worktree. **This is now a confirmed-recurring harness issue across 3 consecutive milestones**; orchestrator integration workaround (checkout-and-merge-ff) is reliable.

## M38 — completed (single big agent, 5 phases, no STOP CRITERIA cuts)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M38 round-out** | `tabular` aggregations + group-by | `m38_` | 880-934 | `8e2c045` (A), `f95fa0c` (B), `294a6d7` (C), `604a912` (D), `ec9d9d0` (E) |

### What shipped

- **Phase A**: typed `df.get_column_i64 / f64 / str / bool / datetime` accessors (resolves the M37 sealed-class-return-type finding); restored Phase C ops — `between / ne / ge / le` on i64+f64, `starts_with / ends_with` on str, `df.rename`.
- **Phase B**: per-column aggregations — `sum / mean / min / max / count / std / var / median` on numeric columns (with sample n-1 std/var); `min / max / count` on str + datetime; `count` on bool. Null-skipping semantics throughout.
- **Phase C**: `df.describe() -> DataFrame` (count/mean/std/min/max/50% for numeric; count only for non-numeric); `Column.fill_null(v)` per subclass (5 methods); `tabular.from_dict(d: Dict[str, Column])` constructor.
- **Phase D**: new `GroupedDataFrame` class (registered via M36 `StdlibItemKind::Class`); `df.group_by(cols) -> GroupedDataFrame`; `gdf.size / keys / sum / mean / min / max / count` shortcuts; `gdf.agg(specs: List[Tuple[str, str]])` custom aggregator. Hash-based with `\x01`-joined multi-column keys.
- **Phase E**: 25 new tests (23 VM + 2 demo); `examples/tabular_groupby_demo.spy` (~110 LOC); LANGUAGE_GUIDE.md §5/§6.2/§11.18/§11.19 updates.

### Four findings worth knowing

1. **`Dict` has no insertion order** — M5's `Dict` is a `HashMap`. `tabular.from_dict` lex-sorts column names by key. Documented as LANGUAGE_GUIDE.md §11.19.
2. **NaN propagation on f64 aggregations** — matches `numpy.sum` (NaN propagates) NOT `numpy.nansum` (skips NaN). Nulls ARE skipped; NaN values are NOT. Documented as §11.18.
3. **Null-keyed group bucket** — rows with a null in any group-key column go into a synthesized null-group bucket (pandas's `dropna=False` mode).
4. **Edit-tool worktree leak (recurring)**: same as M37 — the agent's Edit tool writes leaked into the project-root copy mid-implementation. The agent recovered with a `cp -r` patch. **Orchestrator workaround**: when integrating, ALWAYS check `git status` on main first; if main has partial modifications, `git checkout --` them and `git merge --ff-only` the worktree branch. The worktree branch HEAD is authoritative.

## M37 — completed (single big agent, 5 phases, integrated as fast-forward)

| Agent | Scope | Var prefix | NativeFn IDs | Commits |
|---|---|---|---|---|
| **M37 tabular** | First Pandas-shaped stdlib package | `m37_` | 830-877 | `0f40eaf` (A), `c01e3f1` (B), `2c74e39` (C), `1978346` (D), `895da03` (E) |

### What shipped

- **Module**: `tabular` (named to avoid `import pandas` confusion — see LANGUAGE_GUIDE.md §11.11)
- **6 classes**: sealed `Column` + 5 final subclasses (`ColumnI64` / `F64` / `Str` / `Bool` / `DateTime`) + `DataFrame`. **First stdlib package using the post-M36 canonical class-registration path** — classes registered via `StdlibItemKind::Class` in `seed_stdlib_modules`, NOT in `seed_prelude`. Validates the M36 refactor end-to-end.
- **NA semantics**: per-column `nulls: List[bool]` parallel to `values: List[T]`. Uniform across dtypes; no NaN sentinel games.
- **Phase A (~400 LOC)**: Column/DataFrame allocation + construction helpers (`tabular.col_i64`, etc.) + inspection (shape/columns/dtypes/get_column) + `df.show(n)` ASCII table.
- **Phase B (~300 LOC)**: `read_csv` / `write_csv` / `from_sql` (reuses M35 Cursor!) / `from_rows`. Schema-driven parsing; empty cells → null.
- **Phase C (~400 LOC)**: per-column comparison ops (i64+f64: `eq`/`gt`/`lt`; str: `eq`/`contains`; bool: `eq`; datetime: `eq`/`gt`/`lt`) producing null-aware ColumnBool masks; combinators `and_` / `or_` / `not_` / `count_true`; `df.filter` / `select` / `drop` / `head` / `tail` / `row`.
- **Phase D (~150 LOC)**: stable `df.sort_by(col, ascending)` with nulls-go-to-end, per-Column-type comparator dispatch.
- **Phase E (~150 LOC)**: 19 VM tests + 2 compiler integration tests + `examples/tabular_demo.spy` + LANGUAGE_GUIDE.md updates + agent report.

### STOP CRITERIA invoked

Phase C cut `between`, `ne`, `ge`, `le`, `starts_with` — saved ~10 NativeFn slots. The kept set covers the common 80% filtering cases.

### Three findings worth knowing

1. **`(*hdr).vtable` not `.ty`**: ObjectHeader field name caught the agent in early Phase A; documented.
2. **No `get_column(name) -> Column?`**: sealed-class return type can't be cleanly chosen at NativeFn time. Demo works around by holding typed Column references from construction. **M38 follow-up**: add typed `get_column_i64` / `get_column_str` / etc.
3. **No bare-name fallback for tabular classes**: confirms the M36 refactor's promise. Users MUST write `from tabular import DataFrame`; `import tabular` + `tabular.DataFrame` works only as an annotation type. This is the post-M36 canonical behavior — M34/M35 classes still have the legacy bare-name fallback for back-compat.

## M36 — completed (single agent, integrated as fast-forward)

| Agent | Scope | Var prefix | Commits |
|---|---|---|---|
| **M36 refactor** | `StdlibItemKind::Class` infrastructure | `m36_` | `e72c9fb` (A+B+C+D), `91b581e` (E + report) |

### Design call (worth knowing)

The agent did NOT delete the prelude bindings for the 11 stdlib classes
— every M34/M35 integration test reaches the class names by bare lookup
after just `import json` / `import re` / `import sqlite3` / `import hashlib`
(no `from … import` form). Removing the prelude bindings would have
regressed 39 tests. **M36 is a metadata refactor**: the 11 classes are
NOW also published through their home stdlib modules as
`StdlibItemKind::Class { class_id }` items, but the legacy prelude
bindings remain for back-compat. The infrastructure is in place for v0.4
stdlib classes to register module-scoped from the start.

Phase D added an explicit "still load-bearing for these 11 classes"
comment on the legacy "prelude wins" branch. A future agent that flips
the M34/M35 tests to explicit `from json import JsonValue` forms can
then delete the branch in one go.

### Key takeaway for v0.4 stdlib growth

When you add a new stdlib class, the new path is now:

```rust
// in seed_stdlib_modules (or a per-module helper):
items.push(StdlibItem {
    name: "Foo".into(),
    kind: StdlibItemKind::Class { class_id: foo_cid },
    ty: Ty::Class(foo_cid),
    native_id: 0,  // unused for Class variant
});
```

Do NOT add to `seed_prelude`. Users will import via `from foo_mod import Foo`
or use `foo_mod.Foo` after `import foo_mod`.

## M35 — completed (3 parallel agents, all integrated)

| Agent | Class | NativeFn IDs | Var prefix | Commit |
|---|---|---|---|---|
| **P4-A** | `re.Pattern` (compiled regex) | 790-799 | `p4a_` | `dd80ce2` |
| **P4-B** | `sqlite3.Connection` + `Cursor` | 800-819 | `p4b_` | `ad1200c` |
| **P4-C** | `hashlib.Hasher` (streaming) | 820-829 | `p4c_` | `e2d69bd` |

All three used the **M34 prelude-registration pattern** (no
`StdlibItemKind::Class` infrastructure — classes go in
`compiler/src/resolver.rs::seed_prelude` alongside Channel/Thread/
JsonValue). Each shipped tests + a demo + a spec subsection in the
existing module's section.

**Integration shape that worked**: 3 worktree branches diffed against
the pre-M35 base (`475ab47`), applied additively with `git apply --3way`,
manual conflict resolution at adjacent prelude/match-arm sites
(matches the M27+ pattern). The distinctive `p4a_` / `p4b_` / `p4c_`
prefixes prevented the M27 alignment hazard cleanly.

## `tabular` package state after M50a

The 14-milestone tabular series (M37-M50a) is now feature-complete
for v1 + most of v0.4 polish + the desktop UI HTTP transport. What
ships today:

- **19 stdlib classes**: sealed `Column` + 6 subclasses
  (I64/F64/Str/Bool/DateTime/Categorical) + `DataFrame` +
  `GroupedDataFrame` + 11 from M34/M35 (JsonValue + Pattern +
  Connection/Cursor + Hasher) (M37 / M44 / M47).
- **Full v1 surface**: filter / sort / head / tail / iloc / iloc 2-D /
  iloc-negative / select / drop / rename / dropna / fillna_* / merge
  (all 4 join modes) / pivot / melt / pivot_table / concat_rows /
  concat_cols / unique / value_counts / read_csv / write_csv /
  from_sql / show.
- **Time series**: cumulative ops (cumsum/cumprod/cummax/cummin) /
  rolling (sum/mean/min/max/std + min_periods variants + Welford std
  internal) / resample (1m/5m/15m/1h/1d/1w/1M/1Y with calendar
  arithmetic) / asof_merge.
- **DatetimeIndex + MultiIndex**: full propagation through 18 single-
  col-index methods + 14 MultiIndex methods. Multi-col `group_by`
  promotes to MultiIndex.
- **Aggregations + group-by**: sum/mean/min/max/count/std/var/median
  per column; `df.describe()`; hash-based `group_by` with
  **codes-hash optimization for ColumnCategorical** (M49's ~70-194×
  speedup); ordered categorical with `from_codes`.
- **Reshape**: stack / unstack (all columns distribute) / pivot_table
  with aggfunc-list + margins.
- **Outer-merge NaN-padded MultiIndex** for dtype-mismatched indexes
  on either or both sides.
- **Desktop UI HTTP transport**: `tabular.serve(df, port)` +
  `serve_with_timeout` (M50a) with 5 endpoints + bundled vanilla-DOM
  frontend.

**Bench reality check** (post-M49): geomean **0.30×** (StrictPy /
pandas) across 37 cells; 28 wins / 1 tie / 8 losses. On the
`group_by_cat_via_strings` cell M49 targeted, StrictPy is **14×
faster than pandas's own Categorical fastpath** at high cardinality
(77ms vs 1.04s). Memory peak runs 4-5× pandas at large (M48b memory
deep-dive queued).

## Priority queue (post-M50a)

1. **THESIS + BLOG_POST refresh to M50a** (small writing task, ~30-45 min).
   Both are at post-M39 currently. Concrete deltas for M40-M50a:
   - Tests: 794 → 1034 (+28 M40, +25 M41, +21 M42, +20 M43, +27 M44,
     +19 M45, +27 M46, +32 M47, +0 M48 (bench only), +23 M49, +18 M50a)
   - Stdlib classes: 18 → 19 (M47 added `ColumnCategorical`)
   - Examples: 103 → 113
   - Lesson 1 streak: 21 → 32
   - `tabular` coverage: common-80% (post-M39) → ~95% (post-M40) →
     single-col DatetimeIndex with full propagation (post-M43) →
     MultiIndex with minimal propagation (post-M44) → fully
     index-aware for both single-col AND MultiIndex (post-M45) →
     v1 surface functionally complete (post-M46) → **v0.4 polish
     mostly done** (post-M47 with iloc 2-D + negative iloc +
     rolling Welford/min_periods + categorical dtype).
   - **Methodology notes worth flagging in BLOG**: (a) the M41/M44
     shared-infra cadence exception; (b) the **M47 new
     classification: "cross-dispatch"** — adding a new sealed-class
     subclass requires every dispatch file to compile together,
     so first commit lands at 50-75% of budget, not 20%; (c) the
     M43 9-test-flip cascade lesson; (d) the precautionary-cp
     workaround; (e) the M45/M46 hypothesis-refutation cycle —
     leak cause remains unknown.

2. **M51 — RollingWindow chainable + center=True + categorical sort + bench follow-ups** (the natural M49 follow-up; M50 sequence is desktop UI track in parallel).
   - **RollingWindow chainable class** — `df.rolling(window) -> RollingWindow` shaped like M44's `GroupedDataFrame`. Methods `.mean() / .sum() / .std() / .min() / .max() / .agg(...)`. Builder methods `.center(true)` and `.min_periods(n)`. Classification: **cross-dispatch** (new sealed-class-ish — pattern depends on whether it's an actual sealed-class subclass or just a struct).
   - **center=True rolling alignment** — currently rolling windows are trailing; center option aligns window symmetrically around the output position. M47 + M49 brief deferred.
   - **Pandas-style ordered-sort on `ColumnCategorical`** — currently alphabetical; ordered-categorical-sort uses `categories[]` ordering (M49 added `is_ordered()` predicate; M51 wires the sort path).
   - **Range filtering on outer MultiIndex levels** — M49 added innermost-only `loc_range_multi_*`; M51 extends to outer levels.
   - **Dedicated merge codes-hash bench cell** — M48's `merge_cat_via_strings` cell doesn't exercise the M49 fast path; add a `merge_cat_codes` cell to TABULAR_BENCH_REPORT.
   - **ColumnCategorical explicit `is_ordered` bit** — M49 uses a heuristic; M51 extends the payload (or adds a sidecar bool) to replace it.
   - Estimated: ~1500-2000 LOC. Probably classified as **shared-infra** (RollingWindow is a new helper class touched by every rolling op) or possibly **cross-dispatch** if implemented as sealed subclass.

3. **M48b — Memory deep-dive** — ✅ **DONE** (report:
   `bench/TABULAR_MEMORY_REPORT_M48b.md`). Root-caused the 4–5× peak-RSS
   gap to two v1 simplifications: (a) the **null mask stored as
   `List[bool]` at 8 bytes/bool, carried on every column** (single biggest
   line item — 64× a bit-packed mask), and (b) the **`List<T>` uniform
   8-byte-slot** representation vs NumPy contiguous typed buffers; plus
   secondary **un-interned strings** and **2× list-capacity slack** on
   filter-style ops. Static byte model predicts ~4.3× for a mixed frame,
   matching the measured 4–5×. **Highest-leverage fix (queued for v0.5):
   pack the null mask** (bit/byte) → moves the gap toward ~2.5–3×; the
   holistic endgame is a packed-column representation. Investigation only;
   no code shipped.

4. **M50b — Desktop UI frontend polish** (the natural M50a follow-up).
   - ~~M50a: HTTP transport~~ **SHIPPED** (post-M50a state above).
   - **M50b**: sortable column headers; composite (AND/OR) filters;
     virtual scrolling for >10K-row frames; CSV download endpoint;
     ColumnCategorical serialization (M50a punted to null);
     better styling beyond "looks like a spreadsheet"; LRU eviction
     or explicit `/api/forget?df=ID` (M50a registry is unbounded).
   - **M50c**: interactive pivot UI; sortable group-by; chart
     rendering (basic histograms / line / bar via JS canvas or a
     small charting library bundled inline).
   - Cadence classification: **net-new-feature** (per M50a's
     classification refinement — net-new self-contained subsystem
     with interlocked pieces, ~50-70% first-commit window).

5. **M36 follow-up — flip M34/M35 tests to explicit imports + delete
   the legacy "prelude wins" branch.** Mechanical migration; ~39 test
   files. M37-M50a all confirmed the canonical path works in
   production. Low-priority cleanup; the legacy branch costs nothing
   while it sits.

*(The M45+ polish list — Welford std / min_periods / 1w-1M-1Y
resample / Categorical / iloc 2-D / negative iloc / pivot_table
margins+aggfunc-list — has SHIPPED across M46/M47/M49. center=True
+ df.rolling chainable are the only items remaining; both queued
under M51 above.)*

4. **Edit-tool worktree leak — cause still unknown; M45 hypothesis refuted by M46.**
   Recurred M37-M43 (7 consecutive milestones), narrowed in M40
   (Edit-on-existing-files), broadened in M43 (Write also affected).
   M44 fixed it operationally with a precautionary `cp` block at
   session start. **M45** saw zero leak recurrences even though the
   cp wasn't run (Bash denied) — leading to the M45 hypothesis that
   "the leak only triggers on worktree-divergence at session start."
   **M46 REFUTED this hypothesis**: same conditions as M45 (Bash
   denied for cp loop form, main was sync'd post-M45-push), but
   the leak DID recur. M45 was the lucky outlier, not a stable
   improvement.

   **Honest current state**: cause unknown. The leak is intermittent
   or triggered by something we haven't identified. The
   workaround stays well-routinized:
   - Precautionary `cp` block at session start if Bash is available.
   - Per-file `cp` recovery when symptoms appear mid-session
     (`git status` shows project-root diffs after Edits).
   - Orchestrator integration via `git checkout --` (modified
     files) + remove (untracked leaked files) + `git merge --ff-only`
     against the worktree HEAD — works regardless of how bad the
     leak got in-session.

   Harness root-cause investigation remains deprioritized because
   the workaround is reliable and cheap. The M45/M46 hypothesis-
   refutation cycle is recorded so future thinking doesn't claim
   we understand the leak — only that we can survive it.

5. **Real Cranelift safepoints** (replaces M33 shadow stack):
   `cranelift-jit 0.115` doesn't stably expose PC ranges; check if
   a newer cranelift-jit (0.116+ or trunk) exposes
   `MachBufferFinalized::pc_range_for_inst` or similar. If yes,
   this is a focused agent. If not, the shadow-stack approach is
   fine for now.

4. **Real `mio` event loop** (replaces M32 thread façade): swap
   `asyncio.spawn`'s thread-per-task implementation for a single-
   threaded event loop with state-machine coroutines or
   thread-coordinated tasks. Public surface unchanged.

5. **Rewrite the M29 framework using JsonValue + Pattern +
   Connection + Hasher**: clean LOC measurement of how much v0.3
   stdlib classes shrink user code. The M29 framework was ~2,400 LOC;
   estimated ~1,500-1,700 LOC post-rewrite (30-35% reduction). One
   focused agent.

6. **Phase 3d stdlib**: `traceback`, `enum`, `functools`, `uuid`,
   `secrets`. Smaller modules; the M27 parallel-worktree pattern
   handles them cleanly. 4-5 parallel agents.

7. **Bounded generics + variance + explicit type-arg syntax**:
   extends M31. The `Box[i64]()` explicit-arg form would let
   `asyncio.spawn[T]` work generically.

8. **User-defined exception subclasses**: parser already accepts
   `class MyError(Exception):`; resolver currently rejects. Small fix.

9. **HTTP/2** + **WebSockets**: separate v0.4 stdlib modules.

### Lower priority

- More benchmarks (extended suite already has 30 cells; the M29
  framework throughput could be added as cells)
- Generic methods on non-generic classes (currently scoped-out per
  M17)
- Recursive generic classes (currently scoped-out per M31)
- M34/M35 scope-down cleanup (the helper-vs-constructor double-NativeFn-ID
  thing is mildly ugly; could unify via a constructor-flavour flag
  on `StdlibItemKind::Function`)

## CRITICAL: keep `LANGUAGE_GUIDE.md` up to date

`LANGUAGE_GUIDE.md` (project root, refreshed post-M35) is the
**single source of truth** for AI coding tools writing StrictPy
programs. Every agent brief that touches **language syntax**,
**type system**, or **stdlib** MUST include:

> Update `LANGUAGE_GUIDE.md` to document the new feature in the
> appropriate section. The doc is the single source of truth for
> AI coding tools; if it's out of date, AI tools generate wrong
> code. See §13 "Maintaining this file" at the bottom of the
> guide for the per-feature update pattern.

When integrating an agent's worktree, verify the guide was updated;
if not, write the update yourself before pushing. The doc is what
makes StrictPy usable by other AI tools — losing freshness here
costs more than the integration time saves.

After v0.4 language/stdlib work, update:
- Version banner at the top ("Last refresh: post-M..")
- The relevant §3 / §4 / §5 / §10 sub-section
- A §11 entry if there's a gotcha worth flagging
- §12 examples if the new feature deserves a worked demo

## Methodology lessons that have held

Document these in any new agent brief:

1. **"FIRST commit before 60% of your time budget"** with explicit
   20%/40%/60%/80% checkpoint discipline. **32 consecutive clean
   agents** (M28 → M50a) — the streak is the strongest empirical
   data point in the project. M37-M40 each ran 4-5 phase commits
   across ~2100-2800 LOC milestones. M41 + M44 slipped to combined
   commits (shared-infra exception). M42 + M43 + M45 + M46 + M48 +
   M49 returned to clean per-phase commits (disjoint handlers).
   M47 introduced "cross-dispatch". **M50a introduced a fourth
   classification: "net-new-feature"** — combined commit at
   ~50-70% because net-new self-contained subsystems (an HTTP
   server with its endpoints + JSON serializer + frontend all in
   one builtins.rs subsystem) don't go green incrementally.

   **Four classifications established across M41-M50a:**
   - **disjoint-handler**: per-phase commits at ~20%
     (M42/M43/M45/M46/M48/M49)
   - **shared-infra**: combined Phase A at ~30-50% (M41/M44)
   - **cross-dispatch**: combined commit at ~50-75% (M47) — new
     sealed-class subclass forces all dispatch files to compile
     together
   - **net-new-feature**: combined commit at ~50-70% (M50a) —
     net-new self-contained subsystem with tightly interlocked
     pieces. Distinct from cross-dispatch because no new
     sealed-class subclass.

   M49 was the largest disjoint-handler milestone to date (5 clean
   per-phase commits, ~2700 LOC). M50a was the first net-new-feature
   milestone (~1580 LOC in 2 commits). Future brief language should
   classify accordingly. **M50b** (frontend polish) and **M50c**
   (pivot UI) are also net-new-feature shape. **M51** (RollingWindow
   chainable + categorical sort) classifies as cross-dispatch if
   RollingWindow is a sealed-class subclass, else shared-infra.

2. **Test-flip cascade lesson (M43)**: when a contract change is
   cross-cutting (every single-column group_by now promotes its
   key), the test-flip count scales with how widely the old contract
   was tested. M43 flipped **9 tests** vs the brief's 2-4 estimate —
   M38's 6 group_by tests cascaded because group_by was M38's
   headline feature. **Next brief that changes a feature with broad
   existing test coverage should explicitly grep existing tests
   for old-contract assertions and estimate the flip count from
   that, not from intuition.**

2. **Distinctive variable prefixes per agent** in shared files
   (resolver.rs, builtins.rs, interp.rs) — `p3b_a_` / `p3b_b_` /
   `p3c_a_` / `p3c_b_` / `p4a_` / `p4b_` / `p4c_` / etc. Avoids the
   M27 closing-brace alignment hazard that bit two M27 + M28
   integrations. M35 reconfirmed this works.

3. **Always diff against the pre-round common ancestor** when
   cherry-picking sequentially. NEVER `git diff main..worktree` if
   another worktree has already landed on main — produces
   reverse-deletions. The M28 P3b-B integration disaster (1806
   lines deleted) is the cautionary tale. M35 followed this
   discipline (pre-M35 base `475ab47`) and integrated cleanly.

4. **Auto-resolve "keep-both" Python script** for git-apply conflicts
   that produce simple `<<<<<<<` markers around purely additive
   blocks. Works for ~80% of multi-agent integrations.

5. **Scope-down discretion**: agents who hit STOP CRITERIA and ship
   a smaller working version are the most useful. M33 (shadow-stack
   instead of full Cranelift safepoints), M34 (prelude registration
   instead of `StdlibItemKind::Class`), and M35 ×3 (inheriting M34's
   prelude path rather than building module-level class infra) are
   the exemplars — each shipped working features that v0.4 can
   extend.

## Honest open items to revisit

- **`m33_precise_gc::recursive_allocation_does_not_leak_or_crash`**
  — Windows stack overflow under specific recursive-allocation load.
  Pre-existing flake noted by both M33 + M34 agents. Not blocking;
  may indicate the shadow-stack approach has overhead that recursive
  StrictPy code hits at depth. Investigate during the
  Cranelift-safepoints v0.4 work.

- ~~**The prelude is getting crowded**~~ — **CLOSED in M36** with the
  `StdlibItemKind::Class` refactor. M37-M50a all register classes
  module-scoped from the start. The M34/M35 prelude bindings remain
  for back-compat (the M36 honest-debt); flipping their tests to
  explicit imports is the low-priority cleanup item #5 in the
  priority queue above.

- **Async I/O perf delta**: M32 ships Shape A (thread-backed). The
  M29 framework's ~2× gap to Flask+gunicorn was supposed to be
  closed by async; Shape A doesn't close it (each spawned task is
  still an OS thread). The real perf win requires the v0.4 mio
  event loop. Worth measuring the gap explicitly with a "rewrite
  M29 framework using async" before/after benchmark.

- **`tabular` memory peak vs pandas**: M48 measured StrictPy peak
  RSS runs **4-5× pandas at large** (filter/large: 1.07 GB vs
  0.20 GB). Root cause: `List<T>` per-cell overhead vs NumPy
  contiguous buffers. **M48b** memory deep-dive queued as the
  investigation milestone; v0.5 packed-column work would be the
  remediation.

- **`tabular` large-cell timeouts**: M48 documented 8 large
  (1M-row) group_by/merge/pivot_table cells skipped due to
  >30 minute StrictPy timeouts. M49's codes-hash optimization
  reduced the categorical case dramatically but did not retest
  large; the path-not-categorical large cells likely still time
  out. Worth re-running with the M49 build to refresh the
  baseline.

- **`tabular.serve` desktop UI v1 limitations** (M50a documented):
  no HTTPS (localhost-only); vanilla JS frontend (no React/Vue);
  one-column filter at a time; ColumnCategorical cells render as
  null; unbounded derived-df registry. M50b polishes these.

## Useful one-liners

```bash
# Status summary
cd C:/Users/AG/CascadeProjects/PythonCompiler
git log --oneline -10
git status
git tag --list  # should show v0.2.0

# Quick smoke test (latest tabular work — adjust per milestone)
cargo build --workspace --release && \
  cargo test --release -p strictpy-vm --test m50a_tabular_serve && \
  cargo test --release -p strictpy-vm --test m49_tabular_codes && \
  cargo test --release -p strictpy-vm --test m47_tabular_polish

# Full test sweep (~5-7 min on Windows; reports total at end)
cargo test --workspace --release --no-fail-fast 2>&1 | grep -E "^test result:" | \
  awk '{passed+=$4; failed+=$6; ignored+=$8} END {print "passed:",passed,"failed:",failed,"ignored:",ignored}'

# Run the tabular vs pandas bench (~20+ min for full sweep including
# the slow large cells; use --sizes medium for fast confidence run):
python bench/tabular_harness.py --sizes medium

# Launch the desktop UI demo (open browser at http://localhost:8765):
./target/release/spy.exe examples/tabular_serve_demo.spy

# List active worktrees:
git worktree list

# Pre-M35 base (kept for reference; the M35 round did diff against this):
PRE_M35=475ab47
```

## Memory file location

```
C:\Users\AG\.claude\projects\C--Users-AG-CascadeProjects-PythonCompiler\memory\project_strictpy.md
```

Update the "Status as of end of M..." block when v0.4 lands. The
file is ~155 lines; keep additions concise.
