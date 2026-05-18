# M22 — Phase 2 stdlib (shared brief)

Read this file FIRST, then your task-specific brief.

## Context (post-M21 state)

- M0–M21 complete. 379 tests passing. 8 stdlib modules. Only BUG-028
  remains deferred.
- Phase 1 stdlib is in: `sys`, `os`, `path`, `io`, `time`, `random`,
  `math`, `json`, `re`. Phase 2 adds: `argparse`, `collections`, `csv`,
  `base64`, `hashlib`, `itertools`, `statistics`, `struct`,
  `urllib_parse` (use underscore — `.` would suggest a submodule,
  which is v0.3).
- Stdlib modules are built-in (registered in `seed_stdlib_modules`),
  NOT parsed from `.spy` files. Submodules and user-defined modules
  are v0.3.
- This round is the **first parallel-agent stdlib round**. You are
  running in a git worktree isolated from siblings — you won't see
  their commits. The orchestrator merges all worktrees onto main at
  the end.

## CRITICAL: NativeFn ID range discipline

Phase 1 used IDs 130-249. Phase 2 reserves disjoint ranges per agent:
- **P2A** (argparse + collections + csv): IDs 250-289
- **P2B** (base64 + hashlib): IDs 290-309
- **P2C** (itertools + statistics): IDs 310-329
- **P2D** (struct + urllib_parse): IDs 330-349

Do NOT use IDs outside your assigned range. If you need more than your
range allows, scope down or report — DO NOT reach into another agent's
range.

## Read FIRST, in order

1. `docs/thesis/agent_reports/m19_import_sys.md` — registration pattern.
2. `docs/thesis/agent_reports/m20a_os_path_io.md` — sibling agent with
   most-similar shape; documents tuple-return-from-native + the M19
   parser-fold gotcha.
3. `docs/thesis/agent_reports/m20c_json_re.md` — most-recent agent;
   documents the `match`-as-keyword collision and the
   `serde_json`/`regex` crate-add pattern.
4. `docs/thesis/milestones/m19_m21_stdlib.md` — full milestone-cluster
   note. Read the "What shipped per milestone" table and the
   "placeholder-lowering pattern" section.
5. `STRICTPY_SPEC.md` — find §6.7 (imports) and §9.6-§9.14 (existing
   stdlib modules). Your modules will append §9.15-§9.23 (or wherever
   the numbering lands).
6. `compiler/src/resolver.rs::seed_stdlib_modules` — your new modules
   are registered here. APPEND your push() calls AFTER the existing
   ones (sys/os/path/io/time/random/math/json/re).
7. `shared/src/native.rs` — `NativeFn` enum + `from_u32`. Append your
   variants in your assigned ID range.
8. `vm/src/builtins.rs` — append your dispatch handlers.
9. Your task-specific brief.

## Patterns established by Phase 1 (don't reinvent)

- **Module registration** in `seed_stdlib_modules`: `modules.push(StdlibModule { name, items: vec![...] })`.
- **Function item**: `StdlibItem { name, kind: StdlibItemKind::Function { params, ret }, native_id: NativeFn::Foo as u32 }`.
- **Constant item**: `StdlibItem { name, kind: StdlibItemKind::Const { ty }, native_id: NativeFn::Foo as u32 }`.
- **NativeFn variants**: declare in `shared/src/native.rs::NativeFn` enum with explicit `= <id>`, add a matching arm in `from_u32`.
- **Dispatch**: handler in `vm/src/builtins.rs::dispatch` match. Use `arg_u64(args, n)` to read args; return `Ok(value)` or `Err(VmError::UncaughtException { type_name, message })`.
- **Tuple returns**: `Interpreter::alloc_tuple_obj(elements)` from M20a; works for any flat-tuple of u64-sized fields.
- **Raising exceptions**: return `Err(VmError::UncaughtException { type_name: "ValueError".into(), message: "...".into() })`. Existing M15 propagate_exception will route to handlers.
- **Cross-platform**: use `cfg!(windows)` and `std::path::Path::join` for OS-divergent code.

## Known v0.1 limitations (don't waste time on these)

- **`match` is a hard keyword** (M16). Don't use it as an attribute name. Pick a different name.
- **Nullable narrowing is per-expression**, not per-binding. After `if x is not none:`, a fresh `let y: T = x` may still fail — write an `unwrap_xxx` helper.
- **`with open(...) as f:` doesn't route through try/except**. Use explicit `try: with open(...): ... except IOError:`.
- **Generic classes don't exist** (`class Box[T]:` typechecks but doesn't work). Use generic FREE functions over `List[T]` / `Dict[K, V]`.
- **`??` null-coalesce was just fixed in M21**. It now works correctly. Use freely.
- **BUG-028**: no implicit line continuation across infix `+`. Use string accumulators.

## File-ownership boundaries (PARALLEL — DO NOT touch others')

You may modify these files (the parallel agents will edit the same
files, but in disjoint regions or at append-end; merge conflicts will
be resolved by the orchestrator):
- `compiler/src/resolver.rs` (append at the end of seed_stdlib_modules)
- `shared/src/native.rs` (your assigned ID range)
- `vm/src/builtins.rs` (append handlers; no overlap on IDs)
- `vm/src/interp.rs` (only if you NEED to add Interpreter state; document)
- `STRICTPY_SPEC.md` (append §9.X with a section number larger than existing — your brief will tell you which)

NEW files (no conflict possible):
- `examples/<your-name>.spy`
- `compiler/tests/<your-name>_runs.rs` OR `vm/tests/m22_<your-name>.rs`
- `docs/thesis/agent_reports/m22_<your-name>.md`

Do NOT touch:
- Existing examples or test files from M0-M21.
- Other agents' module registrations.
- `BUGS_KNOWN.md`, `docs/thesis/bugs/catalog.md`, `docs/thesis/timeline.md`,
  `docs/thesis/stats/per_milestone.csv` — orchestrator integrates.

## STOP CRITERIA

- If you need more NativeFn IDs than your assigned range, **scope down**
  by deferring some functions to v0.3. Do NOT reach into another agent's
  range.
- If you find that a Phase 1 module is missing something you need (e.g.
  `os.walk` doesn't exist but you want it for `argparse` to find config
  files), that's normal — work around with what's there.
- If you discover a bug in M19-M21 surface, **save a minimal repro** at
  `examples/_probe_<thing>.spy` and report. Don't try to fix it.

## Acceptance criteria

1. `cargo build --workspace --release` succeeds in your worktree.
2. `cargo test --workspace --release` passes — 379 baseline preserved,
   plus your new tests.
3. At least one example per module + integration tests.
4. Spec amendments for each module.
5. Report at `docs/thesis/agent_reports/m22_<your-name>.md`
   (~600-1000 words).

## Reporting

Mirror the M19/M20 report style. The thesis cares about:
- Which Phase 1 modules your new modules built on
- Which design choices you made (and what you scoped down)
- Any incidental bugs found (the placeholder-lowering trend is now at
  3 instances — does Phase 2 find a 4th?)
- Cross-platform notes
- Final test totals

Be specific about what's at the spec edge (deferred to v0.3 / Phase 3).
