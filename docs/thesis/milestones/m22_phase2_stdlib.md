# M22 — Phase 2 stdlib (first parallel-agent stdlib round)

**Date**: 2026-05-19
**Wall-clock**: ~1.5 hours parallel agent compute + ~30 min orchestrator
integration (cherry-pick + spec renumber).
**Headline**: 9 stdlib modules shipped concurrently by 4 worktree-isolated
agents. Zero new bugs. The orchestrator integration was mechanical but
non-trivial: 4 cherry-picks with append-at-end conflicts on
`resolver.rs` / `native.rs` / `builtins.rs` / `STRICTPY_SPEC.md`.

## What shipped

| Agent | Modules | NativeFn IDs | Worktree commit |
|---|---|---|---|
| P2A | argparse, collections, csv | 250-280 (26 ids) | `afa3d80` → main `1e9c874` |
| P2B | base64, hashlib | 290-304 (9 ids) | `fde6c7f` → main `c1af848` |
| P2C | itertools, statistics | 310-329 (20 ids) | `c6ae967` → main `7e03c18` |
| P2D | struct, urllib_parse | 330-347 (18 ids) | `30d1548` → main `601aac5` |

73 NativeFns total in a contiguous block.

## Sequencing & integration

The 4 agents ran in parallel using the Agent tool's `isolation: worktree`
mode. Each agent worked in its own git worktree branch (`worktree-agent-<id>`)
and committed independently. They never saw each other's changes.

After all 4 reported complete, the orchestrator cherry-picked sequentially
on main, smallest first to minimise downstream conflict surface:

1. P2C (clean cherry-pick, landed first).
2. P2D: conflicts on the 4 shared files; resolved by appending P2D's
   content after P2C's.
3. P2B: conflicts on the same 4 files; P2B's content (290-304) inserted
   before P2C's range (310-329) in `from_u32`, appended elsewhere.
4. P2A: conflicts on the same 4 files (most extensive — P2A is the
   biggest of the 4); P2A's content (250-280) inserted before P2B in
   `from_u32`, appended elsewhere.

Spec section renumbering: all 4 agents independently chose §9.15+§9.16
(or §9.15-§9.17 for P2A). Final spec ordering after integration:
- §9.15 itertools (P2C)
- §9.16 statistics (P2C)
- §9.17 struct (P2D)
- §9.18 urllib_parse (P2D)
- §9.19 base64 (P2B)
- §9.20 hashlib (P2B)
- §9.21 argparse (P2A)
- §9.22 collections (P2A)
- §9.23 csv (P2A)

## Worktree isolation as a pattern

This was the first time the StrictPy thesis project used worktree
isolation for parallel agents touching the same files. The pattern:

**Pros:**
- True parallelism — 4 agents worked simultaneously for ~1.5 hours; the
  sequential version at the M19-M20 cadence would have been ~5 hours.
- Zero coordination during execution. Each agent runs the full
  build+test cycle in its own worktree without seeing siblings.
- Reproducible: each agent's commit is self-contained on its own branch.

**Cons:**
- Integration is non-trivial. Each cherry-pick on the shared files
  conflicted; the orchestrator hand-resolved 4 × 4 = 16 file conflicts
  + a 3-way spec section renumber.
- Spec section numbers needed renumbering; agents independently chose
  the same numbers (§9.15+) because they all extended the same point
  in the spec.
- Tool integration: the orchestrator needed to leave conflict markers
  alone in some intermediate states between cherry-picks; `git add -u`
  staged files that still had markers, and subsequent builds failed
  loudly. The recovery was always `Read → Edit to remove markers →
  rebuild`.

**Verdict**: Worth it. For the next round of "many independent stdlib
modules", parallel worktrees are clearly the right pattern. For
fewer, larger, deeply-coupled changes, sequential remains better.

## Zero-incidental-bug streak continues

| Round | Bugs found |
|---|---|
| M20a (os/path/io) | 1 (BUG-037 ?? null-coalesce, found incidentally) |
| M20b (time/random/math) | 0 |
| M20c (json/re) | 0 |
| M21 (BUG-037 fix + minigrep) | 0 (the fix itself; no new finds) |
| **M22 (9 stdlib modules)** | **0** |

Five consecutive sub-milestones with no incidental bug discovery. The
M19 stdlib-module-table seam is the load-bearing piece — once it landed,
new modules slot in without disturbing resolver/typecheck/IR. Two
phases (M19-M22) and 17 stdlib modules shipped in total, with one bug
found (BUG-037) — and that one was a pre-existing M0-era placeholder,
not a Phase 1/2 regression.

## Phase 2 totals

Phase 1 (M19-M21) + Phase 2 (M22) summary:

- **17 stdlib modules** total: sys, os, path, io, time, random, math,
  json, re (Phase 1) + argparse, collections, csv, base64, hashlib,
  itertools, statistics, struct, urllib_parse (Phase 2).
- **130+ NativeFn IDs** in use (130-347).
- **Crate deps in vm/Cargo.toml**: serde_json, regex, base64, sha1,
  sha2, md-5, hmac.
- **Tests**: 267 → 468 across M19-M22 (+201 over 7 commits).
- **Examples**: 32 → 55 (+23 example programs across the two phases).

## What v0.2 stdlib still doesn't have

Phase 3 territory (per the "what stdlibs to implement next" answer):
- **`socket`** — TCP/UDP. Needs OS socket FFI.
- **`http_client`** — depends on socket; chunked encoding, headers.
- **`ssl`** — OpenSSL FFI. Multi-week.
- **`subprocess`** — process spawn + pipe management.
- **`threading.Lock` / `Semaphore`** — extend M6 thread support.
- **`queue.PriorityQueue`** — generic over T.
- **`sqlite3`** — FFI to libsqlite3.
- **`datetime`** — timezone-aware. Bigger than `time` was.
- **Generic stdlib functions** (random.choice[T] / itertools.* over T) —
  needs stdlib-class integration with the M17 worklist.
- **Stdlib classes** — typed JsonValue, ArgParser/Args, re.Pattern.

Skipped permanently: pickle (static-type incompatible); asyncio (event
loop is a major arch decision); numpy/pandas/scipy/torch (architectural
— see "what would it take to import Python packages").

## Next-step menu (post-M22)

- **G**: Draft the thesis. Archive is now complete: M0-M22 timeline, 30
  agent reports, 33-bug catalog, 17 stdlib modules.
- **F**: Spec catch-up. STRICTPY_SPEC.md now spans 11 stdlib modules
  (§9.6-§9.23) but the original v0.1 organisation was M0-era. Renumber
  + re-flow.
- **J**: Migrate existing examples (lisp/calculator/lambda_calc) to use
  the M13-M17 surface + stdlib.
- **Phase 3 stdlib**: socket / http / ssl / subprocess / threading
  primitives / queue / sqlite3 / datetime.
- **N**: Generic classes (`class Box[T]:`). Unblocks typed stdlib classes
  (ArgParser, JsonValue tree, re.Pattern, Counter[K]).
- **Q**: BUG-028 lexer line continuation (the smallest remaining bug).
