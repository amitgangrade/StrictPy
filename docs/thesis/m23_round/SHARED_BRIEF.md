# M23 — Phase 3a stdlib (shared brief)

Read this file FIRST, then your task-specific brief.

## Context (post-M22 state)

- M0-M22 complete. **468 tests passing, 0 failed, 1 ignored.**
- **17 stdlib modules** (Phase 1+2): sys/os/path/io/time/random/math/
  json/re/argparse/collections/csv/base64/hashlib/itertools/statistics/
  struct/urllib_parse.
- Only BUG-028 (lexer line continuation across infix `+`) remains
  deferred.
- M22 ran 4 parallel worktree-isolated agents — this round is the
  same pattern. You are running in a git worktree; you won't see
  sibling commits. The orchestrator cherry-picks all four onto main
  at the end.

## Phase 3a target: 4 new stdlib modules

This round extends StrictPy into **system control, calendar time,
synchronization primitives, and persistence** — domains the current
17 modules don't touch.

- **P3a-A**: `subprocess` + `pathlib` — system control + OO paths
- **P3a-B**: `datetime` — calendar arithmetic + timezone-aware times
- **P3a-C**: `threading.Lock` + `threading.Semaphore` +
  `queue.PriorityQueue` — sync primitives extending M6 threads/channels
- **P3a-D**: `sqlite3` — database via FFI to libsqlite3

## CRITICAL: NativeFn ID range discipline

M22 used 250-347. Phase 3a reserves disjoint ranges per agent:

- **P3a-A** (subprocess + pathlib): IDs **350-389** (40 ids)
- **P3a-B** (datetime): IDs **390-419** (30 ids)
- **P3a-C** (threading + queue): IDs **420-439** (20 ids)
- **P3a-D** (sqlite3): IDs **440-469** (30 ids)

Do NOT use IDs outside your range. If you need more, scope down or
report — do NOT reach into another agent's range.

## Read FIRST, in order

1. `docs/thesis/agent_reports/m19_import_sys.md` — original
   registration pattern.
2. `docs/thesis/agent_reports/m22_p2c.md` and `m22_p2d.md` — most
   recent worktree agents. Same shape as you.
3. `docs/thesis/milestones/m22_phase2_stdlib.md` — milestone-cluster
   note covering the worktree-integration pattern.
4. `STRICTPY_SPEC.md` §6.7 (imports) and §9.6-§9.23 (existing 17
   stdlib modules). Your modules will be §9.24+.
5. `compiler/src/resolver.rs::seed_stdlib_modules` — append your
   `modules.push(StdlibModule { ... })` after the existing 17.
6. `shared/src/native.rs` — `NativeFn` enum + `from_u32`. Append in
   your reserved ID range.
7. `vm/src/builtins.rs::dispatch` — append handlers.
8. Your task-specific brief.

## Patterns established by Phase 1/2 (don't reinvent)

- **Module registration**: `modules.push(StdlibModule { name, items: vec![...] })`.
- **Function item**: `StdlibItem { name, kind: StdlibItemKind::Function { params, ret }, native_id: NativeFn::X as u32 }`.
- **Constant item**: `StdlibItem { name, kind: StdlibItemKind::Const { ty }, native_id: NativeFn::X as u32 }`.
- **Tuple-return from native**: `Interpreter::alloc_tuple_obj(elements)` (added M20a).
- **Raising exceptions**: return `Err(VmError::UncaughtException { type_name: "ValueError".into(), message: "...".into() })`.
- **Non-catchable exit**: `VmError::Exit(code)` (M19; for `sys.exit`-style total terminations).
- **Adding a crate dep**: edit `vm/Cargo.toml` only (compiler/shared stay dep-free).
- **Per-instance state on `Interpreter`**: add a field (M19's `sys_argv_cache`, M20b's `monotonic_start` / `random_lcg_state`, M20a's `alloc_tuple_obj` infra).

## v0.2 limits (don't waste time)

- **`match` is a hard keyword** (M16). Pick a different attr name.
- **No stdlib classes** in v0.2. Same blocker as M20c's typed JsonValue,
  M22's ArgParser. Use `Dict[str, str]`-shaped handles or sealed
  wrappers around primitive types if you need an opaque handle. v0.3
  will add stdlib-class registration.
- **No submodules** — name your module flat. `subprocess`, not
  `os.subprocess`.
- **No bytes type** yet — use the M22 `struct` trick (each str char
  is a codepoint 0..=255) if you need binary buffers.
- **`with open(...) as f:`** does not route through try/except. If
  your code needs cleanup-on-raise around a `with`, write
  `try: with open(...): ... except IOError:` explicitly.
- **Nullable narrowing is per-expression**, not per-binding. Write
  an `unwrap_xxx` helper if needed.
- **No closures across NativeFn boundary** — your handlers can't
  take user functions as args. If your design needs callbacks (e.g.
  `sqlite3.execute(sql, on_row=...)`), restructure to return a list
  the caller iterates.

## File-ownership boundaries (parallel agents)

You may modify these files (parallel agents edit them too but the
orchestrator merges):
- `compiler/src/resolver.rs` (append your module registration after the
  existing 17)
- `shared/src/native.rs` (your reserved ID range)
- `vm/src/builtins.rs` (append handlers)
- `vm/src/interp.rs` (only if you need new Interpreter state — document)
- `vm/Cargo.toml` (only if adding crate deps — document)
- `STRICTPY_SPEC.md` (append §9.X using whatever section numbers; the
  orchestrator renumbers on integration)

NEW files (no conflict):
- `examples/<your-module>_demo.spy`
- `compiler/tests/<your-module>_demo_runs.rs` OR `vm/tests/m23_<your-module>.rs`
- `docs/thesis/agent_reports/m23_p3a_<letter>.md`

Do NOT touch:
- Existing examples or test files from M0-M22.
- Other agents' module registrations.
- BUGS_KNOWN.md / bugs/catalog.md / timeline.md / stats/* (orchestrator
  integrates).
- Any of the M22 stdlib module code.

## STOP CRITERIA

- If you need more NativeFn IDs than your reserved range, scope down.
  Do NOT reach into another agent's range.
- If you find a bug in the M0-M22 surface, save a minimal repro at
  `examples/_probe_<thing>.spy` and report. Don't try to fix it
  yourself unless it's in your module's territory.
- If your module needs runtime infrastructure that doesn't exist
  (e.g. async event loop, raw bytes), document and ship a reduced
  surface — don't try to add a primitive runtime feature.

## Acceptance criteria

1. `cargo build --workspace --release` succeeds in your worktree.
2. `cargo test --workspace --release` passes — 468 baseline preserved,
   plus your new tests.
3. At least one example program per module + integration tests.
4. Spec amendments for each module (any §9.X numbers — orchestrator
   renumbers).
5. Report at `docs/thesis/agent_reports/m23_p3a_<letter>.md`
   (~600-1000 words).
6. Single commit in your worktree with a clear message.

## Reporting

Mirror the M22 P2x report style. The thesis cares about:
- Which Phase 1/2 modules your new modules built on.
- Which design choices you made (and what you scoped down).
- Any incidental bugs found (the M22 trend was zero across 4 of 4;
  Phase 3a is the test of whether that trend holds when modules go
  beyond pure-Rust into OS FFI / process spawn / FFI to C libs).
- Cross-platform notes (Windows vs Linux matters more for Phase 3
  than for Phase 1/2 — process spawn, signals, threading primitives
  all differ).
- Final test totals.

Begin by reading the M22 reports + this brief. Report when done.
