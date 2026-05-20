# M27 — Phase 3c stdlib (shared brief)

Read this file FIRST, then your task-specific brief.

## Context (post-M26 state)

- M0-M26 complete. **586 tests passing, 0 failed, 1 ignored.**
- **24 stdlib modules** across Phase 1+2+3a:
  - Phase 1: sys, os, path, io, time, random, math, json, re
  - Phase 2: argparse, collections, csv, base64, hashlib, itertools, statistics, struct, urllib_parse
  - Phase 3a: subprocess, pathlib, datetime, threading, queue, sqlite3
- M25 collapsed the toolchain to a single `spy` command (Python-analogous).
- M26 shipped an extended 30-cell benchmark suite; **28/2/0** vs CPython 3.12.10.
- Only BUG-028 (lexer line continuation across infix `+`) remains deferred.
- This is the **third parallel-worktree stdlib round** (after M22 and M23). The
  pattern is well-established: each agent works in an isolated git worktree,
  commits independently, the orchestrator cherry-picks all onto main and
  resolves the standard append-at-end conflicts.

## Phase 3c target: 9 new stdlib modules across 5 agents

This round closes the **filesystem ergonomics + compression/archive** gap.
Every M24 stress program documented one or more of these as workarounds
(`fs_migrator.spy` shelled out to `stat` because we had no `os.mtime`;
no `os.rmdir` so empty dirs were leaked; etc.). Plus `logging` — the
single highest-pain Phase 3d item — slots in cleanly here.

- **P3c-A**: `shutil` + `tempfile` — high-level filesystem ops + temp files
- **P3c-B**: `glob` + `fnmatch` — wildcard expansion + pattern matching
- **P3c-C**: `gzip` + `zlib` + `bz2` — compression streams
- **P3c-D**: `zipfile` + `tarfile` — archive formats
- **P3c-E**: `logging` — application logging (flat global logger v0.2)

## CRITICAL: NativeFn ID range discipline

Phase 3a used 350-449. Phase 3c reserves disjoint ranges per agent:

- **P3c-A** (shutil + tempfile): IDs **450-479** (30 ids)
- **P3c-B** (glob + fnmatch): IDs **480-499** (20 ids)
- **P3c-C** (gzip + zlib + bz2): IDs **500-519** (20 ids)
- **P3c-D** (zipfile + tarfile): IDs **520-549** (30 ids)
- **P3c-E** (logging): IDs **550-569** (20 ids)

Do NOT use IDs outside your range. If you need more, scope down or
report — do NOT reach into another agent's range.

## Read FIRST, in order

1. `docs/thesis/agent_reports/m23_p3a_a.md` and `m23_p3a_d.md` —
   recent worktree agents. Same shape as you.
2. `docs/thesis/milestones/m23_phase3a_stdlib.md` — most recent
   worktree-round milestone note (covers the standard cherry-pick
   conflicts and their resolutions).
3. `docs/thesis/milestones/m22_phase2_stdlib.md` — earlier worktree
   round; covers the standard append-at-end pattern.
4. `STRICTPY_SPEC.md` §6.7 (imports) and §9.6-§9.29 (existing 24
   stdlib modules). Your modules will be §9.30+ (pick any free
   numbers; the orchestrator renumbers on integration).
5. `compiler/src/resolver.rs::seed_stdlib_modules` — append your
   `modules.push(StdlibModule { ... })` after the existing 24.
6. `shared/src/native.rs` — `NativeFn` enum + `from_u32`. Append in
   your reserved ID range.
7. `vm/src/builtins.rs::dispatch` — append handlers.
8. Your task-specific brief.

## Methodology notes (hard-won from M22/M23/M24)

**Commit EARLY, before writing the long report.** The biggest pattern
failure of the last two rounds: agents finish substantive work, then
exhaust compute budget writing a 600-word report, and never reach
`git commit`. The orchestrator then has to commit on the agent's
behalf. To avoid this: **commit your work as soon as `cargo test`
passes, even if the report is still in draft.** You can amend the
commit later if needed.

**Use distinct loop-variable names in list-building handlers.** M23
P3a-D's cherry-pick mis-aligned two handlers at a shared `let sp =
alloc_string(...) as u64;` line because git's three-way merge
treated them as the same context. If your handler builds a list of
strings, use a distinctive name like `let p3c_<letter>_str_handle =
interp.alloc_string(...)` to break the alignment heuristic.

**`str` is the byte-buffer for binary data** (Phase 3c-specific).
gzip/zlib/bz2/zip/tar all deal in bytes. Following the M22 P2D
`struct` precedent, your str parameters carry binary data where
each codepoint is a byte (0-255). The runtime treats str as
UTF-8 internally; you're using it as `Vec<u8>` for these APIs.
There's no separate `bytes` type in v0.2. Document this clearly
in your spec section.

## Patterns established by Phase 1/2/3a (don't reinvent)

- **Module registration**: `modules.push(StdlibModule { name, items: vec![...] })`.
- **Function item**: `StdlibItem { name, kind: StdlibItemKind::Function { params, ret }, native_id: NativeFn::X as u32 }`.
- **Constant item**: `StdlibItem { name, kind: StdlibItemKind::Const { ty }, native_id: NativeFn::X as u32 }`.
- **Tuple-return from native**: `Interpreter::alloc_tuple_obj(elements)` (M20a).
- **List of strings return**: see `vm/src/builtins.rs::os_listdir` for the canonical pattern.
- **List of list of strings**: see `csv.read_file` (M22 P2A) or `sqlite3.query` (M23 P3a-D).
- **Opaque handle (i64)**: see `sqlite3.connect` returning i64 connection id (M23 P3a-D pattern). Use a SharedVm-side slot table.
- **Raising exceptions**: return `Err(VmError::UncaughtException { type_name: "ValueError".into(), message: "...".into() })`.
- **Adding a crate dep**: edit `vm/Cargo.toml` only (compiler/shared stay dep-free).
- **Per-instance state on `Interpreter`**: add a field (M19's `sys_argv_cache`, M20b's `random_lcg_state`, M23 P3a-C's `locks`/`semaphores`/`priority_queues` slot tables).

## v0.2 limits (don't waste time)

- **`match` is a hard keyword** (M16). Pick a different attr name if needed.
- **No stdlib classes** in v0.2. No `ZipFile` object; use opaque i64 handles
  + a SharedVm slot table (the M23 sqlite3 pattern).
- **No submodules** — name your module flat. `zipfile`, not `zip.file`.
- **No bytes type** — use `str` with each codepoint as a byte 0-255.
- **No closures across NativeFn boundary** — your handlers can't take user
  functions as args. No `logging` callback handlers; flat global-config only.
- **`with open(...) as f:`** does not route through try/except.
- **Nullable narrowing is per-expression**, not per-binding.

## File-ownership boundaries (parallel agents)

You may modify these files (parallel agents edit them too but the
orchestrator merges):
- `compiler/src/resolver.rs` (append your module registration after the
  existing 24)
- `shared/src/native.rs` (your reserved ID range)
- `vm/src/builtins.rs` (append handlers)
- `vm/src/interp.rs` (only if you need new Interpreter state — document)
- `vm/Cargo.toml` (only if adding crate deps — document)
- `STRICTPY_SPEC.md` (append §9.X using any free section numbers; the
  orchestrator renumbers on integration)

NEW files (no conflict):
- `examples/<your-module>_demo.spy` (one per module)
- `compiler/tests/<your-module>_demo_runs.rs` OR `vm/tests/m27_<your-module>.rs`
- `docs/thesis/agent_reports/m27_p3c_<letter>.md`

Do NOT touch:
- Existing examples or test files from M0-M26.
- Other agents' module registrations.
- BUGS_KNOWN.md / bugs/catalog.md / timeline.md / stats/* (orchestrator
  integrates).
- Any of the M22 / M23 stdlib module code.

## Suggested Rust crates (your choice — these are starting points)

- **P3c-A** (shutil/tempfile): `std::fs` + `tempfile` crate for tempdir/tempfile.
- **P3c-B** (glob/fnmatch): `glob` crate (or hand-roll); `globset` for fnmatch.
- **P3c-C** (gzip/zlib/bz2): `flate2` (gzip + zlib both in one crate);
  `bzip2` for bz2.
- **P3c-D** (zipfile/tarfile): `zip` crate; `tar` crate. Both pure-Rust.
- **P3c-E** (logging): no crate needed. A simple global logger with
  level filter + format string + optional file output is ~100 LOC.
  Don't use the `log` crate — it's caller-facing macros that don't
  fit StrictPy's flat NativeFn dispatch.

## STOP CRITERIA

- If you need more NativeFn IDs than your reserved range, scope down.
  Do NOT reach into another agent's range.
- If you find a bug in the M0-M26 surface, save a minimal repro at
  `examples/_probe_<thing>.spy` and report. Don't try to fix it
  yourself unless it's in your module's territory.
- If your module needs runtime infrastructure that doesn't exist
  (e.g. callback closures, true binary bytes), document and ship a
  reduced surface — don't add primitive runtime features.

## Acceptance criteria

1. `cargo build --workspace --release` succeeds in your worktree.
2. `cargo test --workspace --release` passes — 586 baseline preserved,
   plus your new tests.
3. At least one example program per module + integration tests.
4. Spec amendments for each module (any §9.X numbers — orchestrator
   renumbers).
5. **Commit EARLY** — as soon as cargo test passes, before writing
   the report. Then write the report and amend if needed.
6. Report at `docs/thesis/agent_reports/m27_p3c_<letter>.md`
   (~600-1000 words).

## Reporting

Mirror the M23 P3a-x report style. The thesis cares about:
- Which Phase 1/2/3a modules your new modules built on.
- Which design choices you made (and what you scoped down).
- Any incidental bugs found (the M22 trend was zero across 4 of 4;
  M23 found one resolver-shadow bug; M24 found BUG-039).
- Cross-platform notes (Windows vs Linux/macOS matters here for
  shutil/tempfile and zip/tar).
- Final test totals.

Begin by reading the M22 + M23 reports + this brief. Report when done.
