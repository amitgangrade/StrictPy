# M23 — Phase 3a stdlib (system control + calendar + sync + DB)

**Date**: 2026-05-19
**Wall-clock**: ~80 min parallel agent compute + ~45 min orchestrator
integration (cherry-pick + merge resolution).
**Headline**: 7 stdlib modules shipped concurrently by 4 worktree-
isolated agents. One incidental bug found and fixed (resolver
stdlib-module-name shadowing). The M19 seam continues to hold; the
language now reaches into OS-level domains for the first time.

## What shipped

| Agent | Modules | NativeFn IDs | Worktree commit |
|---|---|---|---|
| P3a-A | subprocess, pathlib | 350-389 (20 ids) | `3db0b68` → main `54d1238` |
| P3a-B | datetime | 390-411 (22 ids) | `a6e05e0` → main `beb99e2` |
| P3a-C | threading.Lock + Semaphore + queue.PriorityQueue | 420-437 (18 ids) | `9e2701f` → main `c36b0dd` |
| P3a-D | sqlite3 | 440-448 (9 ids) | `6d5b554` → main `bd198b1` |

75 NativeFns total. 1 new crate dep (rusqlite-bundled).

## Sequencing & integration

The 4 agents ran in parallel using worktree isolation (same pattern as
M22). Cherry-pick order on main: P3a-A → P3a-B → P3a-C → P3a-D.

P3a-D was a special case: the agent returned without committing
(rusqlite-bundled was still compiling at deadline). Orchestrator
committed worktree state on its behalf — the actual work was
complete, just not yet wrapped in a `git commit`.

### Unusual merge conflict (worth recording for thesis methodology)

During the P3a-D cherry-pick, git's three-way merge aligned the
`sqlite3.column_names` handler with the existing `pathlib.read_lines`
handler at a `let sp = interp.alloc_string(...) as u64;` line they
both contain. Result: pathlib_read_lines' tail (the
`unsafe { list_push }; Ok(lst); }` block) got semantically replaced by
sqlite3.column_names' tail. The build failure pointed at the merged
location; manual reconstruction of pathlib_read_lines from its
worktree-side history (`git show worktree-agent-...:vm/src/builtins.rs`)
restored the correct shape.

**Pattern**: when parallel agents write handlers with similar
structure (list-building loops over `alloc_string` results), git
mis-aligns them. Future worktree rounds should consider giving
agents distinct loop-variable names or distinct trailing comment
markers to break the alignment heuristic.

### Spec renumbering

All 4 agents independently picked §9.24+. Final ordering:
- §9.24 subprocess (P3a-A)
- §9.25 pathlib (P3a-A)
- §9.26 datetime (P3a-B)
- §9.27 threading (P3a-C)
- §9.28 queue (P3a-C)
- §9.29 sqlite3 (P3a-D)

## The incidental bug (P3a-C resolver shadow fix)

Registering `threading` as a stdlib module broke the existing
`from threading import Thread` prelude binding because the new-module
match arm in `compiler/src/resolver.rs::register_top_decls` errored on
items not found in the stdlib_modules table — but `Thread` *was*
already in scope via the legacy prelude. The "Pre-existing prelude
binding wins" fall-through only fired *after* a successful stdlib
lookup, which was the wrong order.

Four-line fix: when an imported item isn't in stdlib_modules but IS
already in scope (legacy prelude), continue silently. Found and fixed
by the P3a-C agent within its worktree before reporting back.

**This is the first prelude/stdlib interaction bug found in 19 stdlib
modules**. The pattern is now documented in P3a-C's agent report as a
gotcha for future stdlib additions whose module names match existing
prelude bindings.

## Three-phase summary

**24 stdlib modules** total over 5 milestones (M19-M23):

| Phase | Milestones | Modules | NativeFn IDs |
|---|---|---|---|
| Phase 1 (foundation) | M19, M20a, M20b, M20c | sys, os, path, io, time, random, math, json, re | 130-249 |
| Phase 2 (utilities) | M22 | argparse, collections, csv, base64, hashlib, itertools, statistics, struct, urllib_parse | 250-347 |
| **Phase 3a (system)** | **M23** | **subprocess, pathlib, datetime, threading, queue, sqlite3** | **350-449** |

The language now handles:
- CLI ergonomics (sys, argparse)
- Data processing (csv, json, itertools, statistics)
- Encoding/crypto (base64, hashlib, struct)
- Text/URL (re, urllib_parse)
- Filesystem + IO (os, path, io, pathlib)
- Time + calendar (time, datetime)
- **System control** (subprocess) — new in M23
- **Concurrency primitives** (threading, queue) — new in M23
- **Persistence** (sqlite3) — new in M23

## What v0.2 stdlib still doesn't have

Phase 3b territory:
- **`socket`** — TCP/UDP via OS FFI. The big remaining domain.
- **`http_client`** — builds on socket.
- **`ssl` / `tls`** — multi-week OpenSSL or rustls FFI.

Phase 3c+:
- **`pickle`** — won't ship; incompatible with static types.
- **`asyncio`** — needs event loop in the VM; major arch decision.
- **`numpy` / `pandas` / `pytorch`** — architectural (covered in the
  "import Python packages" answer).

Phase 4 (typed-class infrastructure):
- Stdlib classes — would unblock typed `JsonValue` tree, `argparse.ArgParser`,
  `re.Pattern`, `sqlite3.Connection`, `datetime.DateTime`, `Hasher`
  streaming. Five-plus existing modules get cleaner surfaces.
- Generic stdlib functions — would unblock `random.choice[T]`,
  `itertools` generics, `collections.Counter[K]` / `Deque[T]`,
  `queue.PriorityQueue[K, V]`.

## Next-step menu (post-M23)

- **G**: Draft the thesis. Archive is fully built out: M0-M23
  timeline, 34 agent reports, 33-bug catalog, 24 stdlib modules,
  bench history 7 snapshots.
- **F**: Spec catch-up. STRICTPY_SPEC.md now spans 24 stdlib modules.
  Renumber + re-flow.
- **L**: Round of stress tests on the Phase 3a surface. subprocess,
  threading.Lock, and sqlite3 are particularly bug-prone shapes
  (cross-platform process spawn; concurrency primitives; FFI to a
  large C library). Likely 1-3 incidental bugs.
- **Phase 3b stdlib** — socket / http_client / ssl. Gating sequence
  on networking.
- **N**: Generic classes (`class Box[T]:`). Unblocks typed stdlib classes.
- **Q**: BUG-028 lexer line continuation. Last open bug.
