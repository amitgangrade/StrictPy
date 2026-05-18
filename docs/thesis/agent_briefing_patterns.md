# Agent briefing patterns

Concrete patterns the orchestrator evolved across M0–M11 for briefing
sub-agents. Each pattern emerged in response to a real failure mode and
became standard for subsequent briefs. Recorded for the thesis methodology
chapter and as a guide for future work.

## Brief structure that worked

Every successful brief had this skeleton:

```
1. Context paragraph: what is the project, what milestone is this, why now
2. Files to read FIRST (in order): forces the agent to ground in real state
3. What's currently available / known: prevents wasted discovery
4. Known broken stuff (DON'T waste time): prevents agent from re-finding
   bugs already in BUGS_KNOWN.md
5. The actual task with explicit scope
6. Acceptance criteria (machine-checkable)
7. Constraints: what files NOT to modify (file ownership boundaries)
8. Stop criteria: what conditions warrant reporting failure rather than
   papering over
9. Reporting requirements: word limit + specific items to include
```

## Specific patterns that earned their keep

### "Files to read FIRST"

Every brief listed 3-5 files to read before doing anything else, in a
specific order. This prevented two failure modes:

- Agent reinventing knowledge that was already in another file (the spec,
  a sibling example, an existing implementation).
- Agent missing constraints documented elsewhere.

Cost: ~30 seconds of agent reading time per task. Benefit: avoided hours
of duplicated work and design churn.

### "Known broken stuff (DON'T waste time)"

Each brief explicitly listed bugs and limitations the agent would otherwise
discover and try to fix. Example from M11 C-agent briefs:

```
- Sealed-class virtual dispatch drops to base method. Use `open class` instead.
- Subclass field offsets alias parent's last field if parent has fields.
  Keep parent class field-less.
- `try_recv` returns the same `none` sentinel for "empty" and "disconnected".
- No implicit line continuation across trailing `+`. Use accumulators.
```

Without this, agents would have spent significant time on already-known
bugs. With it, they wrote workarounds and flagged the manifestations,
which is what we actually wanted from stress tests.

### Acceptance criteria as concrete observations

Vague: "fib should run faster"
Concrete: "`cargo run --release --bin spy -- fib.spyc` is at least 3× faster
than current. Currently ~931 ms; target ≤ 310 ms."

The concrete form caught at least three agent runs where the work compiled
and tested green but didn't actually move the metric. Without a concrete
target, the agent's self-report ("I optimized X") was not a trustworthy
signal of impact.

### Stop criteria

Every brief had at least one explicit "stop and report what's blocking"
condition. Example from the M8 Cranelift brief:

```
STOP CRITERION: if Cranelift integration takes more than ~half your time
budget and Phase 1 isn't running fib.spy correctly, document blockers
and stop. A partial implementation with the dependency added + the
framework in place + a stub `Jit` that always returns `None` (so
interpreter handles everything) is still a useful checkpoint.
```

This caught at least three near-misses where an agent was about to ship
non-functional code that would have looked successful from the green test
suite.

### File ownership boundaries

Parallel agents got explicit "DO NOT modify files under X; another agent
owns those." This made it possible to run 4-5 agents simultaneously without
merge-conflict-like failures.

Example coordination from M10:

```
AB (compiler/VM): modifies compiler/src/* and vm/src/*
C1, C2, C3 (programs): only ADD new files under examples/ + compiler/tests/
```

The C agents only created new files; the AB agent modified existing files.
Zero file-level conflicts.

### "Find bugs, not ship demos" framing

For the stress-test rounds (M10, M11), every brief included variants of:

> The point of this task is finding gaps, not just shipping a program.
> Treat this as a stress test of the language's stdlib and surface.

This shifted the agent's success metric from "code that runs" to "honest
report of what's broken." The M11 C6 lisp agent spent 60% of its time on
bug-hunting reductions; the resulting bug list was the most valuable
output of the round.

### Verbatim report preservation

Briefs explicitly requested specific items in the report ("paste 5-10
lines of stdout"; "list every NativeFn variant you added"; "minimal
repro for each new bug"). This made the reports usable as primary source
material for `docs/thesis/agent_reports/` without further processing.

## Anti-patterns observed early and abandoned

### Vague briefs ("improve the optimizer")

A few early briefs said "implement the IR optimizer" with no acceptance
criteria. The agent produced something that compiled and had passing tests
but didn't move benchmarks. Lesson: every brief must specify a measurable
outcome.

### Multi-bug fix briefs

The M3.5 agent was asked to fix three M3-era bugs at once. It fixed two
cleanly but broke a third (tree.spy). A focused single-bug brief would have
been safer. Subsequent fix-pass briefs grouped related bugs (same file or
same root cause) but tried not to span unrelated areas.

### "exit 0" as success signal

Original M4 integration tests checked `exit_code == 0` only. Programs
"passed" while producing wrong output. Fixed by requiring value-level
assertions in every test brief from M5 onwards.

### Letting agents pick the spec interpretation

When a spec section was ambiguous, early agents would pick a reasonable
default and proceed. This worked for low-impact decisions. For
load-bearing choices (e.g., "how should `none` be encoded"), the
orchestrator should have called the question explicitly before launching.
Reference: M7's discovery that `none` was stored as bit pattern `0`
silently corrupting `is none` checks — a defensible-at-the-time choice
that became a critical bug.

## When to use parallel vs sequential agents

**Parallel works when**:
- Files don't overlap (each agent owns disjoint files)
- The work is additive (creating new files, not modifying existing ones)
- Coordination cost would otherwise exceed the parallelism gain

**Sequential is better when**:
- Agents need to build on each other's work (M3 → M4 → M5)
- Files would overlap significantly
- The work has hard dependencies (can't write the parser before deciding the AST)

Across the project: M1, M6, M10 used parallel agents (3-5 in flight). All
other milestones were sequential single agents. The parallel rounds
delivered ~2× wall-clock speedup but had ~10× higher coordination
overhead (one parallel round needed ~2 hours of orchestrator time for
brief drafting + integration).

## Brief length

Successful briefs ranged from 300 lines (M11 fix pass) to 60 lines (small
focused bug fix). Length scaled with task complexity, not with hopes.
Brief padding (more "context" in hopes of better results) didn't help —
agents responded to the specifics, not the volume.
