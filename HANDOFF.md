# Session handoff — 2026-05-21 (post-M37)

## Read this FIRST in the next session

Everything you need to resume is in:

1. **This file** — current state + pending work + integration recipes
2. **`docs/thesis/timeline.md`** — milestone-by-milestone narrative through M35
3. **`docs/thesis/stats/per_milestone.csv`** — quantitative ground truth
4. **`THESIS.md`** + **`BLOG_POST.md`** — synthesis documents (frozen at M34;
   needs an M35 refresh pass — see "What comes after M35" below)
5. **`RELEASE_NOTES_v0.2.md`** — v0.2.0 freeze-point summary
6. **`LANGUAGE_GUIDE.md`** — single source of truth for AI tools writing
   StrictPy programs (refreshed post-M35)
7. **Memory file**: `C:\Users\AG\.claude\projects\C--Users-AG-CascadeProjects-PythonCompiler\memory\project_strictpy.md`

## Current head

- Branch: `main`
- Latest commit: `895da03` (M37 E: tabular tests + demo + LANGUAGE_GUIDE.md update + agent report)
- Tag: `v0.2.0` (commit `121483f`, pushed)
- Tests passing on main: **744** (+21 over M36)

## Status snapshot

| Metric | Value |
|---|---:|
| Milestones complete on main | M0–M37 |
| **v0.2.0 release** | **Tagged at M30 (commit 121483f)** |
| Tests | 744 / 0 fail / 1 ignored |
| Bugs | 35 / 35 / **0 deferred** |
| Stdlib modules | **38** (+1 in M37: `tabular`) |
| Stdlib classes | 11 (M34/M35) + **6 from M37** (ColumnI64/F64/Str/Bool/DateTime + DataFrame) = 17 total. M37 classes registered module-scoped from the start via the post-M36 `StdlibItemKind::Class` path (no prelude binding). |
| Example programs | **101** (+1 in M37: `tabular_demo.spy`) |
| Lesson 1 streak | **19 consecutive clean-commit agents** (M28 → M37 — M37 agent committed cleanly per phase across 5 phases) |

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

**Three unpushed commits on local main** (P4-C, P4-B, P4-A) — the
M35 round did not push. `git push origin main` to publish.

## What comes after M35

Per the THESIS §8.4 next-pass priority list + M34/M35 deferred items:

### Highest leverage (in order)

1. **THESIS + BLOG_POST refresh to M35-M37** (small writing task, ~30-60 min).
   Both are frozen at M34. Concrete deltas to fold in:
   - Tests: 690 → 744 (M35 +33; M36 unchanged — pure refactor; M37 +21)
   - Stdlib classes: 7 → 17 (M35 +4; M36 promoted them via `StdlibItemKind::Class`;
     M37 added 6 more — the first using the canonical post-M36 path)
   - Stdlib modules: 37 → 38 (M37 added `tabular`)
   - Demo programs: 97 → 101
   - Lesson 1 streak: 14 → 19
   - **New thesis chapter**: "Pandas-shaped data package as v0.3 stdlib growth"

2. **M38 — tabular round-out + group-by** (the natural M37 follow-up).
   M37's STOP CRITERIA cut some Phase C ops; M38 picks them up + adds
   group-by/aggregate. Concrete punch list:
   - Typed `get_column_i64` / `get_column_str` / `get_column_f64` /
     `get_column_bool` / `get_column_datetime` on DataFrame (resolves
     the M37 "can't return sealed Column" finding)
   - Restore cut Phase C ops: `between` / `ne` / `ge` / `le` (i64+f64);
     `starts_with` / `ends_with` (str); `rename` on DataFrame
   - Aggregations per column: `sum / mean / min / max / count / std / var
     / median` (drop f64 NaN cells)
   - `df.describe() -> DataFrame` summary
   - `df.group_by([cols]) -> GroupedDataFrame` + `.agg({col: "sum"})` /
     `.sum() / .mean()` — hash-based aggregation
   - `Column.fill_null(value)` for each subclass
   - `tabular.from_dict(d: Dict[str, Column])` constructor

3. **M36 follow-up — flip M34/M35 tests to explicit imports + delete
   the legacy "prelude wins" branch.** The infrastructure is in place;
   migration is mechanical. ~39 test files plus a handful of examples
   need `from json import JsonValue` (etc.) added. After the flip, the
   "still load-bearing" comment Phase D added in `resolver.rs` becomes
   removable. M37 confirmed the canonical path works — now mechanical
   migration of legacy callers.

4. **Real Cranelift safepoints** (replaces M33 shadow stack):
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
   20%/40%/60%/80% checkpoint discipline. **19 consecutive clean
   agents** (M28 → M37) — the streak is the strongest empirical
   data point in the project. M37 ran 5 phase-commits across a
   ~2800-LOC milestone without breaking the streak. Don't soften
   this language.

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

- **The prelude is getting crowded**: M34 added 7 JsonValue classes,
  M35 added 4 more (Pattern + Connection + Cursor + Hasher). The
  prelude now hosts **17 stdlib classes** (6 base + 11 v0.3 stdlib).
  The `StdlibItemKind::Class` refactor is now urgent. Probably
  "before M40" rather than "before M50".

- **Async I/O perf delta**: M32 ships Shape A (thread-backed). The
  M29 framework's ~2× gap to Flask+gunicorn was supposed to be
  closed by async; Shape A doesn't close it (each spawned task is
  still an OS thread). The real perf win requires the v0.4 mio
  event loop. Worth measuring the gap explicitly with a "rewrite
  M29 framework using async" before/after benchmark.

## Useful one-liners

```bash
# Status summary
cd C:/Users/AG/CascadeProjects/PythonCompiler
git log --oneline -10
git status
git tag --list  # should show v0.2.0

# Quick smoke test (M35-specific)
cargo build --workspace --release && \
  cargo test --release -p strictpy-vm --test m35_re_pattern && \
  cargo test --release -p strictpy-vm --test m35_sqlite_class && \
  cargo test --release -p strictpy-vm --test m35_hashlib_streaming

# Full test sweep (~5 min on Windows; reports total at end)
cargo test --workspace --release --no-fail-fast 2>&1 | grep -E "^test result:" | \
  awk '{passed+=$4; failed+=$6; ignored+=$8} END {print "passed:",passed,"failed:",failed,"ignored:",ignored}'

# Pre-M35 base (kept for reference):
PRE_M35=475ab47

# List active worktrees:
git worktree list
```

## Memory file location

```
C:\Users\AG\.claude\projects\C--Users-AG-CascadeProjects-PythonCompiler\memory\project_strictpy.md
```

Update the "Status as of end of M..." block when v0.4 lands. The
file is ~155 lines; keep additions concise.
