# M25 — Unified `spy` CLI (Python-analogous compile+run)

**Date**: 2026-05-19
**Wall-clock**: ~30 min orchestrator (single-conversation; no agents).
**Headline**: collapsed the two-binary `spyc` + `spy` toolchain into a
single Python-style `spy` command. `.spy` sources now compile-if-stale
+ run; cached bytecode lands in `__spycache__/` next to source;
inline `-c "code"` works; the two-binary split is no longer required
for any workflow.

## Motivation

Until M24 the toolchain was a strict separation:

```
spyc examples/hello.spy -o hello.spyc
spy hello.spyc
```

Python's equivalent is a single command:

```
python script.py             # compile + run, cache to __pycache__/
python script.pyc            # run cached
python -c "print(1+1)"       # inline
python -m py_compile foo.py  # compile-only
```

User asked for the Python-analogous ergonomics. Goal isn't elegance
for its own sake — every example invocation in the repo, the bench
harness, the integration tests, the README, and every blog/thesis
listing assumed two commands. One command is one cognitive unit; two
is a fence beginners trip over.

## What landed

| Item | Before | After |
|---|---|---|
| Compile a source | `spyc hello.spy -o hello.spyc` | `spy --compile-only hello.spy` (or implicit on run) |
| Run a source | (not possible directly) | `spy hello.spy` (compile-if-stale + run) |
| Run cached bytecode | `spy hello.spyc` | `spy hello.spyc` (unchanged) |
| Inline run | (not supported) | `spy -c "fn main() -> i32: ..."` |
| Bytecode cache | hand-managed by user | `__spycache__/<basename>.spyc` next to source |
| Number of binaries | 2 (`spyc` + `spy`) | 1 (`spy`) |

## Code changes

- **Removed**: `[[bin]] name = "spyc"` from `compiler/Cargo.toml`;
  deleted `compiler/src/main.rs`. The compiler library
  (`strictpy_compiler::compile_file` / `compile_source`) is unchanged
  and remains the canonical API for in-process tooling and tests.
- **Promoted** `strictpy-compiler` from a `[dev-dependencies]` entry
  to a regular `[dependencies]` entry in `vm/Cargo.toml`. The
  formerly-dev-dep cycle (`compiler ↔ vm`) is now a clean DAG:
  `compiler ← vm`, with `compiler` carrying `vm` only as a dev-dep
  for its integration tests.
- **Extended** `vm/src/lib.rs` with
  `run_bytes_with_args(bytes, argv0, args)`. `run_file_with_args`
  now delegates to it after reading the file. The new helper is
  what `-c` mode needs — it skips ever touching the filesystem.
- **Rewrote** `vm/src/main.rs` (~210 LOC) with a Python-style CLI:
    - Positional `SCRIPT` accepts both `.spy` and `.spyc` (dispatched
      by extension; anything else is an error).
    - `-c CODE` mode (mutually exclusive with `SCRIPT`).
    - `--compile-only` flag + optional `-o OUTPUT`.
    - Trailing args still flow to `sys.argv`.
    - Helpers: `cached_spyc_path(src)` → `<dir>/__spycache__/<basename>.spyc`;
      `needs_recompile(src, cache)` checks both metadata reads and
      `src_mtime > cache_mtime` (any read failure → recompile).
- **Added** `compiler/tests/m25_unified_cli.rs` (8 integration tests):
    1. `.spy` run produces `__spycache__/foo.spyc` and executes.
    2. `.spyc` run executes directly.
    3. `-c "code"` inline executes.
    4. `--compile-only` default output lands in `__spycache__/`.
    5. Stale cache (source mtime bumped via 1.5s sleep + rewrite)
       triggers recompile (`v1` → `v2` visible in stdout).
    6. Fresh cache is reused (cached `.spyc` mtime unchanged across
       two consecutive runs of the same unmodified source).
    7. Trailing args flow to `sys.argv[1..]`.
    8. Unknown extension (`.txt`) errors cleanly.

## Updated docs

- **`STRICTPY_SPEC.md` §10.8** (new section): full CLI spec including
  staleness rule and `__spycache__` semantics.
- **`STRICTPY_SPEC.md` §9.6 `sys.argv`**: `argv[0]` description
  updated to "the script path the user typed" rather than "the .spyc
  path", and clarifies `-c` mode sets `argv[0] = "-c"` (matching
  CPython).
- **`README.md`**: workspace layout, build section, and "what
  actually runs" section all updated. All seven canonical example
  invocations collapsed from two-command (`spyc ... -o ...; spy
  ...`) to one (`spy hello.spy`).

## Why a single conversation, no parallel agents

This was a focused refactor with high cross-file coupling (Cargo
manifests, public lib API, CLI binary, eight integration tests,
spec section, README). Parallel agents would each have needed the
same global context; the integration cost would have dominated. A
single-thread session got it through cleanly in ~30 min.

## Test results

- **Tests before**: 578 passing, 0 failing, 1 ignored.
- **Tests after**: 586 passing, 0 failing, 1 ignored. (+8 M25 tests.)
- **No regressions** in any of the 24 stdlib-module test files, the
  language-feature tests (M13-M17), the JIT tests, the GC tests, the
  M10/M11/M12/M18/M24 stress tests, or the threading/sqlite/
  subprocess tests. Every test that invoked `spy.exe` with a `.spyc`
  path continued to work because that path is still supported.
- **Benchmark stability**: not re-run for M25 (no codegen change;
  CLI surface is pure I/O glue around the unchanged compiler+VM).
  `bench/history/` snapshots remain valid through M24.

## Known minor caveats (none blocking)

- **Cross-process cache write race**: if two `spy hello.spy` invocations
  run simultaneously on the same source and both find the cache stale,
  both will write `__spycache__/hello.spyc`. The last writer wins and
  the produced bytes are identical, so this is benign — but on
  Windows the second writer can briefly fail with "file in use" if
  the first is still holding the handle. Mitigation deferred to v0.3
  (atomic `rename`-based cache write).
- **Permission errors creating `__spycache__/`**: if the source
  directory is read-only, the first `.spy` run fails with an i/o
  error instead of falling back to a temp-dir cache. Python falls
  back; StrictPy currently does not. Deferred.
- **No `.spyc` magic-number versioning yet**: a cached `.spyc`
  produced by an older `spy` build will be loaded blindly. The
  loader's existing format-version check guards against incompatible
  bytecode, but the cache key doesn't know about it — clearing
  `__spycache__/` is currently a manual step. Will pair with the
  cache atomicity work in v0.3.

## What this unlocks

- **Onboarding**: README "first command after `cargo build --release`"
  is now `spy examples/hello.spy`. No mental model of `.spyc` needed
  before the first run.
- **Shell scripting**: `spy -c '...'` makes one-liners viable for
  smoke-testing.
- **Editor integrations**: a "run current file" command in an IDE
  now points at exactly one binary with one argument.
- **Removes a class of common confusion** ("which command was the
  compiler again?"). Pre-M25 each blog/thesis listing had to spell
  out both halves.

## Stats row

| M | Tests | Compiler LOC | VM LOC | Examples LOC | Bugs found | Bugs fixed | Bench |
|---|---:|---:|---:|---:|---:|---:|:---:|
| **M25** | **586** | **18,895** | **12,397** | **11,200** | 0 | 0 | 16/0/0 |

## Next-step menu (post-M25)

- **G**: Draft the thesis. Archive remains in steady state.
- **F**: Spec catch-up. STRICTPY_SPEC.md is now honest about the
  M25 CLI surface (§10.8 added).
- **Phase 3b stdlib**: socket / http_client / ssl. The big
  remaining domain.
- **Cache hygiene** (small follow-up to M25): atomic `rename`-based
  cache writes; magic-number versioning in `__spycache__` paths
  (Python uses `foo.cpython-312.pyc`).
- **Placeholder-lowering audit**: explicit pass on
  `compiler/src/ir.rs::emit_binop`. 30-60 min.
- **N**: Generic classes (`class Box[T]:`).
- **Q**: BUG-028 lexer line continuation. **The only remaining
  open bug.**
