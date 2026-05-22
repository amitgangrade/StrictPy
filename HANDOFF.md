# Session handoff — 2026-05-22 (post-M39)

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
- Latest commit: `0d73905` (M39 D: tabular reshape — tests + demo + LANGUAGE_GUIDE update + agent report)
- Tag: `v0.2.0` (commit `121483f`, pushed)
- Tests passing on main: **794** (+25 over M38)

## Status snapshot

| Metric | Value |
|---|---:|
| Milestones complete on main | M0–M39 |
| **v0.2.0 release** | **Tagged at M30 (commit 121483f)** |
| Tests | 794 / 0 fail / 1 ignored |
| Bugs | 35 / 35 / **0 deferred** |
| Stdlib modules | 38 |
| Stdlib classes | 18 (unchanged — M39 ships reshape ops as DataFrame methods, no new classes) |
| Example programs | **103** (+1 in M39: `tabular_reshape_demo.spy`) |
| Lesson 1 streak | **21 consecutive clean-commit agents** (M28 → M39 — M39 agent shipped all 4 phases clean with no STOP CRITERIA cuts) |

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

**Three unpushed commits on local main** (P4-C, P4-B, P4-A) — the
M35 round did not push. `git push origin main` to publish.

## What comes after M35

Per the THESIS §8.4 next-pass priority list + M34/M35 deferred items:

### Highest leverage (in order)

1. **THESIS + BLOG_POST refresh to M35-M39** (small writing task, ~30-60 min).
   Both are frozen at M34. Concrete deltas to fold in:
   - Tests: 690 → 794 (M35 +33; M36 pure refactor; M37 +21; M38 +25; M39 +25)
   - Stdlib classes: 7 → 18 (M35 +4; M36 promoted via `StdlibItemKind::Class`;
     M37 added 6 module-scoped; M38 added 1 GroupedDataFrame)
   - Stdlib modules: 37 → 38 (M37 added `tabular`; M38+M39 extended it)
   - Demo programs: 97 → 103
   - Lesson 1 streak: 14 → 21
   - **New thesis chapter**: "Pandas-shaped data package as v0.3 stdlib growth"
     — M37 ships Phase 1+2; M38 ships Phase 3 (aggregations + group-by);
     M39 ships Phase 4 (reshape). `tabular` now covers the common-80%
     of pandas workflows. Phase 5 (time series) is M40.

2. **M40 — tabular Phase 5: time series + cumulative ops** (the natural M39 follow-up).
   Per the original Pandas plan + M39's agent report follow-up list:
   - `DatetimeIndex` / index-aware operations on top of `ColumnDateTime`
   - `df.rolling(window).mean() / sum() / std()` — rolling-window aggregates
   - `df.resample("1H") / "1D" / etc.` — time-based resampling
   - `df.asof_merge(other, on)` — asof joins
   - `Column.cumsum / cumprod / cummax / cummin` — running aggregates
   - `df.dropna() / df.dropna(subset=cols)` — drop null-bearing rows
   - `df.fillna(value)` — whole-frame fill
   - `df.iloc(start, stop)` — explicit range slicing
   - Estimated ~1500-2000 LOC.

3. **M36 follow-up — flip M34/M35 tests to explicit imports + delete
   the legacy "prelude wins" branch.** Mechanical migration; ~39 test
   files. M37+M38+M39 all confirmed the canonical path works.

4. **Edit-tool worktree leak — investigate or work around at harness level.**
   Confirmed-recurring across M37 + M38 + M39 (3 consecutive milestones).
   Each time the agent's Edit/Write tool writes to project-root copies
   instead of the worktree. Current orchestrator-side workaround
   (checkout-and-merge-ff against worktree HEAD) is reliable but should
   not be permanent. Worth a focused investigation — does this happen
   with the harness's git-worktree path resolution? Is there a setup
   step missing? Single, no-coding session probably.

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
   20%/40%/60%/80% checkpoint discipline. **21 consecutive clean
   agents** (M28 → M39) — the streak is the strongest empirical
   data point in the project. M37 + M38 + M39 each ran 4-5 phase
   commits across ~2400-2800 LOC milestones without breaking the
   streak. Don't soften this language.

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
