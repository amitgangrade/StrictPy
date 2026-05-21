# Session handoff — 2026-05-21

## Read this FIRST in the next session

Everything you need to resume is in:

1. **This file** — current state + pending work + integration recipes
2. **`docs/thesis/timeline.md`** — milestone-by-milestone narrative through M34
3. **`docs/thesis/stats/per_milestone.csv`** — quantitative ground truth
4. **`THESIS.md`** + **`BLOG_POST.md`** — synthesis documents through M34
5. **`RELEASE_NOTES_v0.2.md`** — v0.2.0 freeze-point summary
6. **Memory file**: `C:\Users\AG\.claude\projects\C--Users-AG-CascadeProjects-PythonCompiler\memory\project_strictpy.md`

## Current head

- Branch: `main`
- Latest commit: see `git log -1 --oneline`
- Tag: `v0.2.0` (commit `121483f`, pushed)
- Tests passing on main: **690** (post-M34); will jump to ~720+ once M35 lands

## Status snapshot

| Metric | Value |
|---|---:|
| Milestones complete on main | M0–M34 |
| **v0.2.0 release** | **Tagged at M30 (commit 121483f)** |
| Tests | 690 / 0 fail / 1 ignored |
| Bugs | 35 / 35 / **0 deferred** |
| Stdlib modules | 37 |
| Prelude classes | (Channel, Thread, File, Dict, Set, List) + 7 JsonValue tree (M34) |
| Example programs | 97 |
| Lesson 1 streak | 14 consecutive clean-commit agents (M28 → M34) |

## M35 — in flight when session ended

Three parallel worktree agents launched. They may have completed (check
notifications) or may still be running. Each worktree branch will be at
`.claude/worktrees/agent-<id>` with a separate git branch.

| Agent | Class | NativeFn IDs | Var prefix | Worktree agent ID |
|---|---|---|---|---|
| **P4-A** | `re.Pattern` (compiled regex) | 790-799 | `p4a_` | `aa88985588d8b7d0d` |
| **P4-B** | `sqlite3.Connection` + `Cursor` | 800-819 | `p4b_` | `af837e920c0c9dc30` |
| **P4-C** | `hashlib.Hasher` (streaming) | 820-829 | `p4c_` | `af3a46754248b7bbe` |

All three use the **M34 prelude-registration pattern** (no
`StdlibItemKind::Class` infrastructure — classes go in
`compiler/src/resolver.rs::seed_prelude` alongside Channel/Thread).
Each ships ≥6 tests + a demo + a spec subsection in the existing
module's section.

### Checking M35 agent status

```bash
# Worktree branches:
git branch -a | grep worktree-agent-

# Last commit on each worktree branch:
git log --oneline worktree-agent-aa88985588d8b7d0d -1   # P4-A re.Pattern
git log --oneline worktree-agent-af837e920c0c9dc30 -1   # P4-B sqlite3
git log --oneline worktree-agent-af3a46754248b7bbe -1   # P4-C Hasher

# Check uncommitted state in each:
cd .claude/worktrees/agent-<id> && git status --short
```

If any agent didn't commit (Lesson 1 streak break — first time in 14 agents!),
the orchestrator-side commit-on-behalf pattern is well-documented in past
commits. See e.g. `git show 595c2e6` (M27 P3c-A orchestrator commit-on-behalf)
or `git show 0c6c004` (M27 P3c-A worktree-side recovery).

### M35 integration recipe

The proven additive-diff pattern (works because M35 agents have disjoint
NativeFn ranges + distinct variable prefixes):

```bash
# Pre-M35 base (the M34 archive commit):
PRE_M35=475ab47

# For each agent, in order P4-C (smallest) → P4-A → P4-B:
for agent_id in af3a46754248b7bbe aa88985588d8b7d0d af837e920c0c9dc30; do
  echo "=== Integrating $agent_id ==="
  git diff $PRE_M35..worktree-agent-$agent_id > /tmp/p4.patch
  git apply --3way --whitespace=nowarn /tmp/p4.patch

  # If there are conflicts (likely keep-both pattern), Python auto-resolve:
  # python -c "..."  # the keep-both script from M27/M28 integrations

  # Build + test the agent's specific tests:
  cargo build --workspace --release
  # cargo test --release -p strictpy-vm --test m35_<name>

  # Commit
  git add -A
  git commit -m "M35 P4-<X>: ..."
done

git push origin main
```

**Expected M35 conflict shape**: each agent adds new prelude class
registrations near M34's JsonValue block in `compiler/src/resolver.rs`,
new NativeFn variants near M34's range (750-789 → P4-A 790-799 →
P4-B 800-819 → P4-C 820-829) in `shared/src/native.rs`, and new
match arms in `vm/src/builtins.rs`. Probably 0-2 manual brace fixes
between adjacent agents' match arms — the standard M27+ closing-brace
pattern. The distinctive `p4a_` / `p4b_` / `p4c_` prefixes should
prevent the M27 alignment hazard.

### M35 catalog update + thesis archive

After M35 lands, update:
- `docs/thesis/timeline.md` — add M35 section
- `docs/thesis/stats/per_milestone.md` + `.csv` — add M35 row
- `docs/thesis/agent_reports/README.md` — add m35_p4a/b/c entries
- Memory file's "Status as of end of M..." block

The pattern is established; mirror the M34 archive commit (`475ab47`).

## What comes after M35

Per the THESIS §8.4 next-pass priority list (and the M34 deferred
items + general v0.4 backlog):

### Highest leverage (in order)

1. **`StdlibItemKind::Class` infrastructure**: move M34 (JsonValue)
   + M35 (Pattern, Connection, Cursor, Hasher) class registrations
   from the prelude to module-scoped. Pure refactor — no API
   change. The "right thing" the M34/M35 agents deferred. Probably
   200-400 LOC in resolver.rs + typecheck.rs.

2. **Real Cranelift safepoints** (replaces M33 shadow stack):
   `cranelift-jit 0.115` doesn't stably expose PC ranges; check if
   a newer cranelift-jit (0.116+ or trunk) exposes
   `MachBufferFinalized::pc_range_for_inst` or similar. If yes,
   this is a focused agent. If not, the shadow-stack approach is
   fine for now.

3. **Real `mio` event loop** (replaces M32 thread façade): swap
   `asyncio.spawn`'s thread-per-task implementation for a single-
   threaded event loop with state-machine coroutines or
   thread-coordinated tasks. Public surface unchanged.

4. **Rewrite the M29 framework using JsonValue + Pattern +
   Connection**: clean LOC measurement of how much v0.3 stdlib
   classes shrink user code. The M29 framework was ~2,400 LOC;
   estimated ~1,500-1,700 LOC post-rewrite (30-35% reduction). One
   focused agent.

5. **Phase 3d stdlib**: `traceback`, `enum`, `functools`, `uuid`,
   `secrets`. Smaller modules; the M27 parallel-worktree pattern
   handles them cleanly. 4-5 parallel agents.

6. **Bounded generics + variance + explicit type-arg syntax**:
   extends M31. The `Box[i64]()` explicit-arg form would let
   `asyncio.spawn[T]` work generically.

7. **User-defined exception subclasses**: parser already accepts
   `class MyError(Exception):`; resolver currently rejects. Small fix.

8. **HTTP/2** + **WebSockets**: separate v0.4 stdlib modules.

### Lower priority

- More benchmarks (extended suite already has 30 cells; the M29
  framework throughput could be added as cells)
- Generic methods on non-generic classes (currently scoped-out per
  M17)
- Recursive generic classes (currently scoped-out per M31)
- M34 scope-down cleanup (the helper-vs-constructor double-NativeFn-ID
  thing is mildly ugly; could unify via a constructor-flavour flag
  on `StdlibItemKind::Function`)

## Methodology lessons that have held

Document these in any new agent brief:

1. **"FIRST commit before 60% of your time budget"** with explicit
   20%/40%/60%/80% checkpoint discipline. **14 consecutive clean
   agents** (M28 → M34) — the streak is the strongest empirical
   data point in the project. Don't soften this language.

2. **Distinctive variable prefixes per agent** in shared files
   (resolver.rs, builtins.rs, interp.rs) — `p3b_a_` / `p3b_b_` /
   `p3c_a_` / `p3c_b_` / `p4a_` / etc. Avoids the M27 closing-brace
   alignment hazard that bit two M27 + M28 integrations.

3. **Always diff against the pre-round common ancestor** when
   cherry-picking sequentially. NEVER `git diff main..worktree` if
   another worktree has already landed on main — produces
   reverse-deletions. The M28 P3b-B integration disaster (1806
   lines deleted) is the cautionary tale.

4. **Auto-resolve "keep-both" Python script** for git-apply conflicts
   that produce simple `<<<<<<<` markers around purely additive
   blocks. Works for ~80% of multi-agent integrations.

5. **Scope-down discretion**: agents who hit STOP CRITERIA and ship
   a smaller working version are the most useful. M33 (shadow-stack
   instead of full Cranelift safepoints) and M34 (prelude
   registration instead of `StdlibItemKind::Class`) are the
   exemplars — both shipped working features that v0.4 can extend.

## Honest open items to revisit

- **`m33_precise_gc::recursive_allocation_does_not_leak_or_crash`**
  — Windows stack overflow under specific recursive-allocation load.
  Pre-existing flake noted by both M33 + M34 agents. Not blocking;
  may indicate the shadow-stack approach has overhead that recursive
  StrictPy code hits at depth. Investigate during the
  Cranelift-safepoints v0.4 work.

- **The M34 scope-down note**: M34 chose prelude registration for
  JsonValue classes. M35 inherits this. If the prelude class table
  starts feeling crowded (we'll add 4 more classes in M35 → 11+
  total in the prelude beyond the original Channel/Thread/File/Dict/
  Set/List), the `StdlibItemKind::Class` refactor becomes urgent.
  Probably "before M40" rather than "before M50".

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

# Quick smoke test
cargo build --workspace --release && cargo test --release -p strictpy-vm --test m34_json_value

# Full test sweep (~5 min on Windows; reports total at end)
cargo test --workspace --release --no-fail-fast 2>&1 | grep -E "^test result:" | \
  awk '{passed+=$4; failed+=$6; ignored+=$8} END {print "passed:",passed,"failed:",failed,"ignored:",ignored}'

# Pre-M35 base for integration:
PRE_M35=475ab47

# List active worktrees:
git worktree list
```

## Memory file location

```
C:\Users\AG\.claude\projects\C--Users-AG-CascadeProjects-PythonCompiler\memory\project_strictpy.md
```

Update the "Status as of end of M..." block when M35 lands. The
file is ~150 lines; keep additions concise.
