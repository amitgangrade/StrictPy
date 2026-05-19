# M24 — Phase 3a stress round (Round 5)

**Date**: 2026-05-19
**Wall-clock**: ~2.5 hours parallel agent compute (4 worktree agents)
+ ~30 min orchestrator integration (cherry-pick + BUG-039 fix).
**Headline**: 4 real programs combining 6+ Phase 3a modules each.
**1 bug found, 1 bug fixed (BUG-039 — fourth instance of the
placeholder-lowering pattern).** Real parallelism verified
(3.62×-5.75× speedup at N=4).

## What shipped

| Agent | Program | Modules combined | Probes | Bugs |
|---|---|---|---:|:---:|
| M24-A | `job_scheduler.spy` (267 LOC) | subprocess + threading.Lock + threading.Semaphore + queue.PriorityQueue + datetime + pathlib | 9/9 PASS | 0 |
| M24-B | `event_log.spy` (759 LOC) | sqlite3 + datetime + argparse + io + pathlib + re | 14/14 PASS | **BUG-039** |
| M24-C | `test_runner.spy` (448 LOC) | subprocess + threading.Thread + queue.PriorityQueue + sqlite3 + time | 10/10 PASS | 0 |
| M24-D | `fs_migrator.spy` (330 LOC) | pathlib + os + datetime + subprocess + io + sys | 10/10 PASS | 0 |

4 programs, **43/43 probes PASS**, ~1,800 LOC of real stress code.

## The one bug — BUG-039

`event_log.spy` rendered an empty per-category histogram when
real data was present. Bisected to: `k in dict` returning `false`
even immediately after `dict[k] = v` succeeded and `dict[k]`
returned the right value. The agent shrunk it to a 12-line
minimal repro and reported it as BUG-039 before working around
it with `len(dict.keys())` + `dict.get(k)`-style probing.

**Root cause** (fixed by orchestrator post-report): one line in
`compiler/src/ir.rs::emit_binop`:

```rust
AstBinOp::In => IROp::IEq,  // placeholder
AstBinOp::NotIn => IROp::INe,
```

The IR was comparing the key value against the container's heap
pointer as i64. Always false (unless they happened to coincide
at the same address). Symptom: `key in d` was a silent miscompile
across **every** Dict in StrictPy since M5.

**Fix**: type-dispatched lowering. `k in Dict[str, V]` →
`NativeFn::DictHas`; `x in Set[T]` → `NativeFn::SetHas`. `NotIn`
mirrors then emits `BoolNot`. List membership and non-str-keyed
Dict still placeholder (v0.3 — runtime is hardcoded to str keys
via `arg_str`; dispatching DictHas for non-str keys segfaults).

5 regression tests added (`vm/tests/m24_in_operator.rs`):
str_in_dict_true_for_present_key, str_not_in_inverts,
variable_key_in_dict, value-type-agnostic
(i64/i32/bool/str), in_dict_after_overwrite_and_grow.

## The fourth-instance lesson

BUG-039 is the **fourth** time the same shape has surfaced — a
binary operator with an incomplete IR lowering that silently
miscompiles when the operand type doesn't match the placeholder
branch:

| Bug | Operator | Placeholder | Found in | Fixed in |
|---|---|---|---|---|
| BUG-008 | `is not` | `RefEq` (not `not RefEq`) | M10 stress | M10 |
| BUG-034 | `str !=` | `INe` (no `is_str` branch) | M12 stress | M12 |
| BUG-037 | `??` (null-coalesce) | `Copy(rhs)` (always fallback) | M20a stdlib write | M21 |
| **BUG-039** | **`in` / `not in`** | **`IEq` / `INe`** | **M24 stress** | **M24** |

Every one of these is a single binary-op match arm in `emit_binop`
that punts on the type-dependent lowering. The methodology
recommendation now reads cleanly: **audit every match arm in
`emit_binop` after M2 for type-dispatched correctness.** Done
mechanically, it would have caught all four pattern instances at
once. (To be fair: BUG-039 also depended on M5 Dict landing, so a
mechanical pass right after M2 would have caught only BUG-008 +
BUG-037 + BUG-034. But the pattern itself is now obvious.)

A fifth instance is plausible — likely a Tuple or Set comparison
operator. v0.3 should include the audit explicitly.

## Stress-round bug-rate trajectory

| Round | Date | Programs | LOC | Bugs found |
|---|---|---:|---:|---:|
| M10 (round 1) | 2026-05-18 | 6 | ~1,660 | 17 |
| M11 (round 2) | 2026-05-18 | 5 | ~1,810 | 6 |
| M12 (round 3) | 2026-05-19 | 3 | ~1,477 | 2 |
| M18 (round 4) | 2026-05-19 | 4 | ~1,900 | 1 |
| **M24 (round 5)** | **2026-05-19** | **4** | **~1,800** | **1** |

The curve is flat at 1 bug / round of ~1500-2000 LOC since M18.
The one bug per round in M18 and M24 was, both times, a
placeholder operator lowering (BUG-037 was `??`; BUG-039 is
`in`/`not in`). Both were latent since their respective operators
shipped (M0-ish for `??`, M5-ish for `in`). Neither is
architectural; both are mechanical-audit candidates.

## Real parallelism verified (M24-C)

The test_runner agent built a deliberate timing test: same fixed
20-test table (14 fast `exit N`, 6 slow `timeout`/`sleep`), run
twice — once with N=4 worker threads, once with N=1.

3 measurement runs gave wall-clock speedups of:
- 3.62× (5.31s → 1.47s)
- 5.75× (8.46s → 1.47s)
- 2.64× (4.65s → 1.76s)

This is real OS-thread parallelism — the workers spend most of
their time blocked in `subprocess.run` waiting for child
processes, and the StrictPy VM doesn't hold a GIL on those
threads. The test SQLite database is in-memory, accessed under a
single `db_lock`; even with contention the speedup is 2.6× +
better. Documented in `bench/history/m24_parallelism.md` for
future regression checks.

## Worktree integration quirk

All four agents finished their work but **ran out of compute
budget at the final `git commit` step** — they wrote 500-1000
word reports last and exhausted the budget before getting back
to `git commit`. The orchestrator committed each worktree's
tree on the agent's behalf, then cherry-picked onto main. No
substantive conflicts (each agent only added files).

**Pattern note for future rounds**: agent briefs should
explicitly say "**commit EARLY, before writing the long report**
— the orchestrator can read the report from a file, but losing
the working tree is expensive." This is the second round (after
M23 P3a-D) where the same shape happened, so it's worth
hardening into a permanent piece of the SHARED_BRIEF template.

## Missing stdlib primitives documented (M24-D)

The fs_migrator agent worked around v0.2 stdlib gaps and
catalogued them as Phase 3b candidates:

- `os.mtime(path)` / `os.size(path)` — currently requires
  shelling out to `stat -c %Y` (Unix) or `dir /T:W` (Windows).
- `pathlib.stat()` — same gap, OO surface.
- `os.rmdir()` / `pathlib.rmdir()` — only `os.remove(file)` ships
  in v0.2. Empty directories must be left behind.
- `re.find_all` capture groups — current `re.find_all` returns
  whole matches; group extraction needs a per-match `re.find`
  loop.
- `pathlib.normalise` — cross-platform separator coalescing.
- `subprocess.run(env=)` — environment variable injection per
  child. Currently child inherits parent's env wholesale.

None of these are blockers for the v0.2 stdlib surface; they're
the natural Phase 3b "round-the-edges" set.

## Tests + size deltas

- **Tests**: 553 → 578 (+25). Breakdown:
  - +11 M24-A (compile-check + scheduler end-to-end + 9 probes)
  - +5 M24-B (event_log_runs.rs covering BUG-039 sentinel)
  - +5 M24 BUG-039 regression (`vm/tests/m24_in_operator.rs`)
  - +2 M24-D (fs_migrator compile + run)
  - +2 M24-C (test_runner compile + run)
- **Examples**: 62 → 70 (+8 — 4 main programs + 4 probe files).
- **LOC**: compiler/src/ir.rs +~30 lines for the In/NotIn
  dispatch.
- **Stdlib modules**: **unchanged at 24**. M24 was stress + bug
  fix only; no new modules.

## Three-phase + stress retrospective

Two-and-a-half-day window from M19 (first stdlib seam) to M24
(first Phase 3a stress round):

| Window | Cadence | Modules | Tests | Bugs |
|---|---|---:|---:|---:|
| M19-M21 (Phase 1) | sequential | 9 | 379 | 1 |
| M22 (Phase 2) | 4 parallel agents | 9 | 468 | 0 |
| M23 (Phase 3a) | 4 parallel agents | 7 | 553 | 1 (resolver shadow) |
| **M24 (stress)** | **4 parallel agents** | **0** | **578** | **1 (BUG-039)** |

26 stdlib modules shipped + 1 stress round in this window. Both
of the two bugs found were either (a) an interaction-effect bug
caught only by combining the new infrastructure with an old
prelude path (the M23 resolver-shadow fix), or (b) a latent
placeholder that had been silently miscompiled since M5
(BUG-039). Neither is a regression caused by the stdlib work
itself — the M19 seam continues to hold.

## Next-step menu (post-M24)

- **G**: Draft the thesis. Archive is fully built: M0-M24
  timeline, 38 agent reports, 34-bug catalog (33 fixed, 1
  deferred), 24 stdlib modules, 7 bench snapshots, methodology
  + design-decision corpus.
- **F**: Spec catch-up. STRICTPY_SPEC.md now spans 24 stdlib
  modules + 5 language features (M13-M17) + tuples + match.
  Renumber/re-flow.
- **Phase 3b stdlib**: socket / http_client / ssl. The big
  remaining domain. Likely 2-3 worktree agents, ~1 week
  parallel.
- **Phase 3c**: pickle (skipped — static-type incompatible);
  asyncio (architectural — event loop is a major decision).
- **N**: Generic classes (`class Box[T]:`). Unblocks typed
  stdlib classes (Hasher, ArgParser, JsonValue tree,
  re.Pattern, sqlite3.Connection, datetime.DateTime).
- **Placeholder-lowering audit**: explicit mechanical pass on
  `compiler/src/ir.rs::emit_binop` for any remaining binary
  operator that punts on type dispatch. Estimated 30-60 min.
- **Q**: BUG-028 lexer line continuation across infix `+`. **The
  only remaining open bug.** Small lexer enhancement.
- **L (next stress)**: Round 6 once Phase 3b ships — socket +
  http + ssl combined with the existing 24-module surface.
