# Methodology

How the StrictPy project was conducted. Relevant for the methodology chapter
of the thesis — particularly the AI-assisted-development angle.

## Process model

The entire project was executed through **Claude Code** (a CLI agent harness)
with a human orchestrator. The pattern that worked:

1. **Spec-first.** Before any code, M0 produced the full ~1,800-line
   language and VM specification. Every subsequent agent task could be
   briefed by pointing at a spec section. Without this, agents would have
   re-litigated design choices on every task.

2. **Milestone-grained agent tasks.** Each milestone was framed as one or
   more parallel agent invocations with explicit, machine-checkable
   acceptance criteria. Example M8 acceptance: "fib.spy must run ≥3× faster
   than current; quicksort tests must stay green." No vague acceptance
   ("improve the IR") ever produced a useful result.

3. **Parallel agents on disjoint files.** When work could be partitioned by
   file (lexer + parser + pretty-printer; or compiler-side + VM-side; or
   C agents writing new examples), agents ran in parallel. The
   orchestrator integrated at the end. Up to 5 agents in parallel; the
   bottleneck was usually agent latency, not parallelism.

4. **"Stop criteria" in every brief.** Each brief had explicit conditions
   under which the agent should report failure rather than paper over the
   problem. This caught at least three near-misses where an agent would
   otherwise have produced compiling-but-broken code.

5. **Honest reporting bias.** Briefs explicitly asked for "what was
   awkward, what was missing, what didn't work" with at least equal
   weight to "what shipped." This produced the bug catalog that became
   the most useful artifact.

## Coordination patterns

### Pattern: scaffold-then-fill

For M0–M2, the orchestrator wrote the scaffolding (workspace, shared crate,
all stub modules with proper types and method signatures). Agents then
filled in implementations. This allowed parallel work because the seams
were locked in advance.

### Pattern: parallel agents on disjoint files

```
M1: Lexer + Parser + Pretty-printer
    └─ 3 agents, all write to different files
       (lexer.rs / parser.rs / pretty.rs), all read shared types

M6: tree.spy fix + real threading
    └─ 2 agents, one in compiler/, one in vm/

M10: Real-world stress test
    └─ 4 agents in parallel, then a 5th fix-pass agent
       AB (compiler/VM modifications)
       C1, C2, C3 (only add new files under examples/ + tests/)
       Fix agent (cleanup pass for bugs C2/C3 found)
```

### Pattern: progressive disclosure

Long agent tasks were given known limitations of the system explicitly,
so the agent didn't waste time discovering them. Example from M10:
> Known stdlib gaps to expect:
> - No `for x in xs:` — use `while`
> - No `str.split(sep)` — write a splitter
> - No `sorted()` / `list.sort()` — write Lomuto
> - `try_recv` returns the same `none` sentinel for "empty" and "disconnected"

This was load-bearing — without it, agents would have wasted hours on
problems whose answers were already known.

### Pattern: snapshots before disruptive work

Before launching M9 (full JIT coverage), the orchestrator snapshotted the
M8 benchmark results into `bench/history/`. Before launching M10, snapshot
M9. This created the progression artifact that anchors the performance
narrative — the data would have been impossible to reconstruct after the
fact.

## Quantitative parameters

- **Number of agent task launches**: ~30 (covering M0–M10)
- **Typical task duration**: 5–60 minutes wall-clock
- **Total project wall-clock**: ~3 days
- **Total agent compute**: estimated ~40 hours across all milestones
- **Lines of code at end**: ~21K Rust + ~1.7K StrictPy + ~2.3K test code + ~5K markdown documentation
- **Bug discovery rate**:
  - M0–M9: ~12 bugs in 9 milestones (~1.3/milestone)
  - M10: ~17 bugs in one stress-test round
  - Stress testing was the highest-leverage bug discovery mechanism

## What worked

- **Spec as first artifact.** Cannot overstate this. The spec was referenced
  by hundreds of agent tasks; it never had to be re-explained.
- **Hard acceptance criteria.** "Tests pass" alone is meaningless. Every
  brief that demanded a specific output value or measurable benchmark
  delta produced trustworthy results.
- **Background-mode agents.** Long-running agents (60+ minutes) ran in the
  background while the orchestrator drafted the next milestone's brief.
  Roughly halved overall wall-clock.
- **"Stop and report" criteria.** Caught three near-misses where an agent
  would have shipped non-functional code with a green test suite.
- **Snapshotting before disruption.** Enabled the M7→M8→M9→M10 progression
  table that became the centerpiece of the performance narrative.

## What didn't work (and what we did instead)

- **Vague briefs.** A few early briefs said "implement the IR optimizer"
  with no acceptance criteria. The agent produced something that compiled
  and had passing tests but didn't move benchmarks. Lesson: every brief
  must specify a measurable outcome.

- **Trying to fix multiple architectural bugs in one agent task.** The
  M3.5 agent was asked to fix three M3-era bugs at once. It fixed two
  cleanly but broke a third (tree.spy). A focused single-bug brief would
  have been safer.

- **Optimistic test discipline.** Original M4 integration tests checked
  `exit_code == 0` only. Programs "passed" while producing wrong output.
  Fixed by requiring value-level assertions in every test brief from M5
  onwards.

- **Single-agent benchmarks.** Early benchmarking gave Python its
  parse+compile time but excluded StrictPy's compile time. Caught and
  fixed at M10-prep when the user noticed; the M7-unfair snapshot is
  preserved as evidence of how the measurement bug looked.

## The reproducibility story

The project is fully reproducible from `git clone`. To regenerate any
result:

```powershell
git clone https://github.com/amitgangrade/StrictPy
cd StrictPy
cargo build --release
cargo test --workspace --release    # 173 tests pass
python bench/harness.py             # regenerates BENCH_REPORT.md
```

Individual examples:
```powershell
./target/release/spyc.exe examples/fib.spy -o fib.spyc
./target/release/spy.exe fib.spyc
```

Each milestone's benchmark numbers are preserved in `bench/history/`.
The CSV in `docs/thesis/stats/per_milestone.csv` is the authoritative
quantitative record.

## What this archive does and doesn't claim

This archive supports claims like:
- "StrictPy's JIT made fib(30) 64× faster than its interpreter, with
  ~2,000 lines of Cranelift integration."
- "17 of 24 distinct bugs in the project were found by running real
  programs, not by writing tests."
- "Static-type-driven AOT compilation requires none of the speculation /
  deopt machinery a dynamic-language JIT needs."

It does NOT support claims like:
- "StrictPy is faster than CPython on real-world workloads" — the
  benchmark suite is 4 micro-benchmarks; we only know about those.
- "Static typing is more productive than dynamic typing" — single
  developer, single project, no controlled comparison.
- "AI-assisted development is generally faster" — single project, no
  baseline. Anecdotal at best.

The thesis must be careful about scope. The empirical claims this archive
supports are narrow and specific; broader generalizations require evidence
beyond this project.
