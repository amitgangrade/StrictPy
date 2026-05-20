# M27 — Phase 3c stdlib (filesystem + compression + archives + logging)

**Date**: 2026-05-20
**Wall-clock**: ~2-3h parallel agent compute (5 worktree agents) +
~1h orchestrator integration (cherry-pick + manual conflict resolution
+ spec renumbering).
**Headline**: 9 stdlib modules shipped concurrently by 5 worktree-
isolated agents. The third parallel-worktree stdlib round (after M22
and M23). One incidental bug found and worked around (bzip2 write-side
decoder hangs on malformed input; P3c-C switched to read-side).

## What shipped

| Agent | Modules | NativeFn IDs | Spec sections | Worktree commit |
|---|---|---|---|---|
| P3c-A | `shutil` + `tempfile` | 450-472 (9 ids) | §9.30, §9.31 | `0c6c004` → main `595c2e6` |
| P3c-B | `glob` + `fnmatch` | 480-486 (7 ids) | §9.32, §9.33 | `b870b4c` → main `<integ>` |
| P3c-C | `gzip` + `zlib` + `bz2` | 500-510 (11 ids) | §9.34, §9.35, §9.36 | `c170ecb` → main `<integ>` |
| P3c-D | `zipfile` + `tarfile` | 520-535 (16 ids) | §9.37, §9.38 | `b974301` → main `<integ>` |
| P3c-E | `logging` | 550-560 (11 ids) | §9.39 | `f3605ee` → main `a4e4a97` |

**54 NativeFns total** in a contiguous block 450-569 (with deliberate
gaps reserved for v0.3 extensions). 4 new crate deps in
`vm/Cargo.toml`: `tempfile`, `glob`, `flate2`, `bzip2`, `zip`, `tar`
(+ unix-only `libc` for `shutil.disk_usage`).

## Three patterns from this round worth recording

### 1. The "commit early" briefing failed again

The SHARED_BRIEF explicitly said: *"commit your work as soon as cargo
test passes, even if the report is still in draft."* This was the
load-bearing methodology fix added after M23 P3a-D and all four M24
agents ran out of compute budget at the commit step.

Result: **2 of 5 M27 agents (P3c-A, P3c-E) still failed to commit.**
The orchestrator committed both worktrees on the agents' behalf. The
other 3 agents (P3c-B, P3c-C, P3c-D) did commit early as briefed.

Updated finding: explicit briefing instructions are necessary but
not sufficient. Some agents will read the brief, intend to follow
it, and still spend their last 30% of budget polishing the report
instead of committing. Future rounds should consider:

- **Embedding a "commit checkpoint" requirement** that's mechanically
  verifiable mid-task (e.g. "your first commit must land within 60%
  of estimated budget; abort if not").
- **Auto-committing worktree state** at fixed intervals during the
  agent's session, independent of the agent's explicit `git commit`
  calls. This is an orchestrator-side automation that would treat
  the agent's worktree as ephemeral state and snapshot it
  periodically.

### 2. Keep-both auto-resolution works for purely additive at-end blocks

After M23's manual cherry-pick conflict resolution and the brief's
mention of git's three-way merge alignment heuristic, the M27
orchestrator switched to a different strategy: `git apply --3way`
per agent, then auto-resolve conflicts via a Python script that
takes both sides ("ours" then "theirs") of every conflict marker.

This worked cleanly for P3c-A's spec section, Cargo.toml entries,
and shared/src/native.rs's `from_u32` arms — all locations where the
diff is purely additive at the end of an existing list.

**It failed in two places per integration**:
1. `compiler/src/resolver.rs` interleaved conflicts where the two
   agents' module blocks shared boilerplate (`kind:
   StdlibItemKind::Function`, `ty: fn_ty(...)`). The keep-both
   resolution produced syntactically valid Rust that nevertheless
   dropped half of one agent's items.
2. `vm/src/builtins.rs` had the same shape at the match-arm
   boundary between two agents' new arms. The keep-both dropped a
   closing `}` between the previous arm's last expression and the
   next arm's match pattern, causing a "this file contains an
   unclosed delimiter" compile error.

Resolution for both: extract the agent's full block from their
worktree copy of the file, then manually insert at the correct
anchor in main's file. The closing-brace issue was always a single
one-line `Edit` after the build error surfaced.

### 3. The spec section collision

The brief said "any §9.X numbers — orchestrator renumbers". P3c-A
chose §9.30/9.31 (shutil/tempfile) and P3c-D *also* chose §9.30/9.31
(zipfile/tarfile). Both shipped that way; the orchestrator's
integration moved P3c-D to §9.37/§9.38.

This is the second round in a row where independent agents picked the
same section numbers (M22 had all four agents choose §9.15+ for their
modules). The orchestrator renumber step is now standard.

## Crate deps growth

vm/Cargo.toml grew from 11 deps (post-M23) to 17 deps (post-M27):

Pre-M27: anyhow, thiserror, clap, byteorder, serde_json, regex,
base64, md-5, sha1, sha2, hmac, rusqlite, plus cranelift family.

Post-M27 adds: `tempfile` (P3c-A), `glob` (P3c-B), `flate2` + `bzip2`
(P3c-C), `zip` + `tar` (P3c-D). Plus `libc` (Unix-only) for
`shutil.disk_usage`.

Cold-build cost increased moderately (~30-40s); incremental builds
unchanged.

## What v0.2 stdlib still doesn't have

After M27 the language has 33 stdlib modules. Remaining gaps for v0.2:

**Phase 3b** (networking — biggest single domain):
- `socket` — TCP/UDP via `std::net`
- `ssl` / `tls` — `rustls` integration
- `http_client` — request/response on top of socket
- `urllib_request` — higher-level wrapper

**Phase 3d** (utility & debugging — partially started by M27 P3c-E):
- `traceback` — format_exc, format_tb (improves StrictPy's currently-
  one-line exception output)
- `enum` — named constant classes (needs minor language support for
  value-bound class members)
- `functools` — `reduce`, `partial`, `lru_cache`, `cmp_to_key`
- `uuid` — UUID v4/v7
- `secrets` — crypto-secure random

**v0.3 stdlib classes** (Phase 4):
- Stdlib classes — would unblock typed `JsonValue` tree,
  `argparse.ArgParser`, `re.Pattern`, `sqlite3.Connection`,
  `datetime.DateTime`, streaming `Hasher`, `logging.Logger` /
  `Handler` / `Formatter`.
- Generic stdlib functions — `random.choice[T]`, `itertools` over T,
  `collections.Counter[K]` / `Deque[T]`, `queue.PriorityQueue[K, V]`.

## Three-phase + M27 retrospective

Stdlib milestones run-rate to date:

| Window | Cadence | Modules | NativeFns |
|---|---|---:|---:|
| M19-M21 (Phase 1) | sequential | 9 | 130-249 |
| M22 (Phase 2) | 4 parallel agents | 9 | 250-347 |
| M23 (Phase 3a) | 4 parallel agents | 7 | 350-449 |
| **M27 (Phase 3c)** | **5 parallel agents** | **9** | **450-569** |

33 stdlib modules shipped across 4 stdlib rounds. The 5-agent shape
of M27 was the largest concurrent round yet; the worktree pattern
held up but the integration cost was higher than M22/M23 (more
shared-file conflicts, more keep-both edge cases).

## Tests + size

- **Tests**: 586 → ~640+ (M27 added ~50 new tests across 5 agents).
  Full sweep pending verification on integration branch.
- **Examples**: 70 → 79 (+9 new demo programs).
- **Stdlib modules**: 24 → 33.

## Next-step menu (post-M27)

- **G**: Draft the thesis. Archive is fully built through M27.
- **Phase 3b**: networking (`socket` + `ssl` + `http_client`). The
  big remaining domain. Likely 2-3 worktree agents, ~1 week of
  parallel + integration.
- **Phase 3d**: utility/debugging stdlib (`traceback`, `enum`,
  `functools`, `uuid`, `secrets`). Smaller, parallel-friendly.
- **N**: Generic classes (`class Box[T]:`). Unblocks typed stdlib
  classes.
- **Placeholder-lowering audit**: 30-60 min mechanical pass on
  `compiler/src/ir.rs::emit_binop`.
- **Q**: BUG-028 lexer line continuation. The last open bug.
- **An orchestrator-side auto-commit harness**: snapshot worktree
  state every N minutes during agent execution so the "agent
  exhausts budget before committing" failure mode stops happening.
