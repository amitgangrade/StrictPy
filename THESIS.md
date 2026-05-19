# StrictPy — a statically typed Python dialect, compiler, and bytecode VM

*A 26-milestone, AI-orchestrated systems-language project*

**Project**: <https://github.com/amitgangrade/StrictPy>
**Archive**: [`docs/thesis/`](docs/thesis/) — quantitative record (per-milestone CSV, 38 verbatim agent reports, 34-entry bug catalog, 7 benchmark snapshots).
**Spec**: [`STRICTPY_SPEC.md`](STRICTPY_SPEC.md) — frozen at v0.1 on day one; the contract every subsequent milestone is checked against.
**Date span**: 2026-05-17 → 2026-05-19 (3 calendar days, ~50 hours of agent compute).
**Outcome**: A working compiler + bytecode VM + Cranelift JIT that beats CPython 3.12 by **4–17×** on all 16 cells of a 4-program benchmark suite, with **70 example programs**, **24 stdlib modules**, **586 passing tests**, and **one** open bug.

---

## Abstract

We built **StrictPy** — a statically typed Python dialect with its own
Rust toolchain (compiler, bytecode VM, Cranelift JIT) — over 3 calendar
days using an AI-orchestrated workflow. The implementation is ~31K
lines of Rust plus ~11K lines of example StrictPy code. fib(30)
runs in 13.1 ms versus CPython 3.12's 159.5 ms; on every cell of a
4-program benchmark suite StrictPy is 4–17× faster than CPython. The
JIT itself is ~1,400 lines of Cranelift integration, dramatically
smaller than dynamic-language JITs because mandatory static types
eliminate the type-profiling, inline-cache, and deoptimization
infrastructure that consumes most of the engineering effort in
PyPy-class implementations.

The project's second contribution is methodological: a 26-milestone
record of how Claude Code orchestrated agent tasks, what agent briefs
worked, what failed, and a 34-bug catalogue with root-cause analysis
showing that **17 of 34 bugs were found by running real programs
rather than by writing tests** — a result the archive preserves as
the auditable record of the stress-test ROI curve.

This document is a technical thesis aimed at compiler and systems
engineers. It synthesises the project archive into seven chapters
(Design, Implementation, Performance, Methodology, Findings,
Limitations, Conclusion) with file:line references throughout. The
underlying evidence — agent reports, benchmark snapshots, bug
catalogue, design-decision dossiers — lives in
[`docs/thesis/`](docs/thesis/) and is meant to be audited.

---

## 1. Introduction

### 1.1 The starting question

The project began with a question posed by the author to Claude:
*"Why is type information not used to generate more efficient
bytecode in Python?"* The honest answer has many layers — type hints
can lie, they are stored as strings, CPython's object model is the
real bottleneck — but the follow-up question was harder to dismiss:
*"What would it look like if you actually built a Python where the
types weren't optional?"*

This thesis is the record of trying. The empirical result was a
working compiler-VM-JIT toolchain that beats CPython 3.12 on every
benchmark cell. The methodological result was a structured record of
how AI agents can be coordinated to produce a non-trivial systems
artifact, with concrete patterns for what to brief explicitly and
where the architecture leaks.

### 1.2 What we built

StrictPy is Python's syntax with the following constraints:

- Every name has a mandatory type annotation.
- Numeric types are concrete: `i32`, `i64`, `f64`, `BigInt`. There is
  no untyped `int`.
- Nullability is explicit: `T?` means "T or None"; the type checker
  narrows `T?` to `T` inside `if x is not none:` branches.
- Classes are `final` by default; `open` is the keyword that permits
  subclassing. Single inheritance only.
- No `Any`, no `eval`, no monkeypatching, no `__dict__` mutation, no
  metaclasses, no decorators that synthesise runtime types.

These are not aesthetic choices. Every one of them eliminates a
category of dynamism that an AOT compiler would otherwise have to
defeat with runtime checks. The thesis claim is that **removing
dynamism doesn't just make optimization possible; it makes
optimization mechanical** — the JIT becomes a translation pass over
typed IR with no speculation, no guards, no inline caches, and no
deoptimization.

### 1.3 What this thesis contains

The remainder of this document is organised as seven chapters:

- **§2 Design** — the language constraints and the five load-bearing
  architectural decisions (`is_native` class flag, unified JIT ABI,
  nullable-unwrap dispatch, conservative GC with `in_jit` pause,
  per-function JIT opt-in with fixpoint disable). Spec-first
  methodology and what each decision bought.
- **§3 Implementation** — the 26-milestone trajectory, partitioned
  into five phases: foundations (M0–M9), correctness through stress
  testing (M10–M12), language completeness (M13–M17), stdlib
  (M19–M23), maturity and the unified CLI (M24–M25).
- **§4 Performance** — the M7→M8→M9 cliff (931 ms → 14.6 ms →
  13.5 ms on fib(30)), the M9→M25 plateau (16 milestones of feature
  work with no perf regression), and the architectural decisions
  that made the cliff cheap.
- **§5 Methodology** — the orchestrator + sub-agent process model;
  spec-first; hard acceptance criteria; parallel agents on disjoint
  files; worktree-isolated parallel agents (M22–M24); honest
  reporting bias; what failed and the corrections that worked.
- **§6 Findings** — seven generalisable patterns from the 34-entry
  bug catalogue, including the four-instance "placeholder IR
  lowering" pattern, the dose-dependent latent-hack accumulation
  (BUG-029), and the deterministic-sibling-unlocks-non-deterministic
  mystery pattern (BUG-026 → BUG-030 → BUG-016).
- **§7 Limitations** — what the archive supports versus what it
  doesn't, the deferred-to-v0.2 list, and the one remaining open
  bug.
- **§8 Conclusion** — what changes given the result, and where the
  evidence currently runs out.

The companion artifact [`BLOG_POST.md`](BLOG_POST.md) is a narrative
for a broader audience covering roughly the same material. The
[`docs/thesis/`](docs/thesis/) directory is the evidence layer — read
it like a paper's supplementary materials.

---

## 2. Design

### 2.1 Spec-first

The single highest-leverage decision in the project was producing the
language and VM specification before any code. M0 wrote
[`STRICTPY_SPEC.md`](STRICTPY_SPEC.md) — 1,813 lines covering grammar
(§4 EBNF), the type system (§5), static and dynamic semantics (§6,
§7), the memory and object model (§8), the bytecode file format
(§12), the opcode reference (§13), the VM (§14), GC, threading, FFI,
the error model, and an implementation roadmap that became the
milestone plan.

Without the spec, every subsequent agent task would have had to
re-litigate design choices. With it, briefs could point at a section:
"Implement the parser per spec §4.7 and §10.3; the conformance suite
in §20 is the acceptance test." This was not a hypothetical benefit
— the archive shows ~30 agent invocations across M0–M25, and almost
every brief includes a spec section reference. The spec absorbed the
churn that would otherwise have happened in agent context.

The spec was also a coordination mechanism. The bytecode format §12
defined a stable contract between the compiler and the VM crates, so
the M3 IR / codegen / bytecode agents could land in parallel with the
M4 loader / interpreter / GC agents — they all referenced the same
`.spyc` layout. This is the pattern that made the cross-crate
parallelism in §5.3 possible.

The cost of writing a spec first is one session. The cost of not
writing it is paid by every subsequent task, plus the design churn
that re-derives forgotten constraints.

### 2.2 The language constraints

Every constraint StrictPy adds (relative to CPython) eliminates a
runtime check. Together they make AOT compilation mechanical:

| Constraint | Eliminated runtime mechanism |
|---|---|
| Mandatory annotations | Type profiling |
| Concrete numeric types (`i32`, `i64`, `f64`) | Boxed-int fastpath / overflow tagging |
| No `Any` | Dispatch tables; megamorphic call sites |
| No monkeypatching | Method resolution caches; class watchpoints |
| Single inheritance, `final`-by-default classes | C3 linearisation cost; megamorphic vcalls |
| Explicit nullability (`T?`) | Implicit None check on every read |
| No `eval` / `exec` | Cannot specialise; cannot const-fold globals |
| No metaclass / `__init_subclass__` | Class-creation hooks; instance shape tracking |

The full list of forbidden constructs lives in [STRICTPY_SPEC.md
§5.6](STRICTPY_SPEC.md). The point is not that any single removal
gives a 10× win — it is that the elimination is *compositional*. An
operator like `a + b` where both operands are statically known to be
`i64` can be lowered to a single Cranelift `iadd` instruction with no
guards, no PIC slot, no fallback path. The same expression in CPython
goes through `BINARY_OP` → `PyNumber_Add` → type slot lookup →
fastpath check → boxed-int alloc. Each layer is the runtime defending
against a dynamism StrictPy doesn't allow.

### 2.3 Five load-bearing design decisions

The archive preserves five decisions whose downstream cost was
significant enough to warrant a standalone dossier in
[`docs/thesis/design_decisions/`](docs/thesis/design_decisions/).
Each is summarised here with a pointer.

**Unified JIT ABI** ([dossier](docs/thesis/design_decisions/unified_jit_abi.md)).
Every JIT-compiled function uses the same Rust signature:
`unsafe extern "C" fn(*mut VmState, *const u64) -> u64`. Args via
pointer, return value as a `u64` bit pattern (bitcast for `f64`). The
alternative — per-function native ABIs matching Rust's `extern "C"`
register conventions — would have eliminated one load per argument
but required signature negotiation at every call site. The unified
ABI was load-bearing for the trivial interpreter↔JIT boundary: the
interpreter builds a `&[u64]` slice and calls `jit_fn(vm,
args.as_ptr())` with no marshalling. The cost is ~5–15% on the
tightest leaf functions; the benefit is one Cranelift `Signature`
object for the entire JIT.

**`is_native: bool` on ClassLayout**
([dossier](docs/thesis/design_decisions/is_native_class_flag.md)).
Channel, File, Dict, Thread, and `str` are not real user classes —
they are handles backed by native runtime state. Marking them with
`is_native: true` lets the IR lowerer skip the vtable path for
methods on those types and emit `CallNative` directly. This was the
M7 breakthrough that finally got `producer.spy` and `wordcount.spy`
running end-to-end; before M7, every `ch.send(i)` lowered to a
`VirtualCall` against a vtable that didn't exist.

**Conservative GC with `in_jit` pause**
([dossier](docs/thesis/design_decisions/conservative_gc_with_in_jit_pause.md)).
The mark-sweep GC scans Rust call frames conservatively (treats every
8-byte-aligned u64 in the stack as a possible pointer). This works
for the interpreter, but JIT'd code holds heap pointers in CPU
registers that conservative scanning cannot see. The current fix is
an `in_jit: AtomicUsize` counter on `SharedVm`: bracket every JIT
entry, refuse to collect when the counter is non-zero. This blocks
GC during JIT'd execution — fine for benchmarks (the 16MB arena is
huge), and a known limitation for long-running programs. Precise
stack maps are the proper fix, deferred to a future release.

**Unwrap nullable before type-specific dispatch**
([dossier](docs/thesis/design_decisions/nullable_unwrap_dispatch.md)).
Every IR-side dispatch site that switches on `Ty::Primitive(_)` MUST
first unwrap `Ty::Nullable(inner)`. The type checker narrows `T?` to
`T` inside `if x is not none:` branches, but the narrowed type lives
in a side table, not on the IR slot — so naive `if let Ty::Primitive(_)
= slot_ty` checks see the un-narrowed `Nullable` and fall through to
the default branch. This cost five silent-miscompile bugs in
`codegen.rs` (BUG-001 through BUG-005) before being canonicalised
into the discipline. Lesson §6.6 below makes this concrete.

**Per-function JIT opt-in with fixpoint disable**
([dossier](docs/thesis/design_decisions/per_function_jit_opt_in.md)).
The JIT supports a subset of IR ops; functions containing an
unsupported op stay interpreted. The trick is the fixpoint pass:
**callers of un-JIT'd functions also stay interpreted**. Without
this, every call from JIT'd code into the interpreter would pay a
mode-switch penalty. With it, cross-mode calls only happen at the
JIT/interpreter boundary, which the unified ABI makes trivial.

### 2.4 What spec freezing bought

The spec was frozen at v0.1 on M0 and amended in-place in subsequent
milestones (M16's match patterns added §6.5.1; M19's import machinery
added §6.7; M25's CLI added §10.8). The archive shows every amendment
as a milestone-level change; no design churn is hidden in the git
log. The cost of "the spec is the contract" was occasionally writing
something into the spec, finding it wrong in M11, and editing the
spec rather than living with a contradiction. Each such episode is
documented in the milestone notes.

---

## 3. Implementation

### 3.1 Workspace

Three crates in a single Rust workspace:

```
strictpy-compiler/  lex → parse → resolve → typecheck → IR → optimize → bytecode
strictpy-vm/        loader + interpreter + Cranelift JIT + GC + native stdlib
strictpy-shared/    Opcode enum, .spyc file format, NativeFn registry, type tags
```

At M25 (the end of the timeline this thesis covers), the workspace
totals: compiler 18,895 LOC, VM 12,397 LOC, shared 720 LOC. Plus
~11,200 LOC of StrictPy example programs and ~5,000 LOC of
integration tests. The single largest source file is
[`compiler/src/ir.rs`](compiler/src/ir.rs) (~2,500 lines, IR lowering
+ monomorphisation); the second is `vm/src/interp.rs` (~2,200 lines,
the interpreter dispatch loop).

The split between `compiler` and `vm` is enforced by the `.spyc` file
format defined in `strictpy-shared`. The compiler emits `.spyc`; the
VM reads it. There is no shared in-memory representation. This
contract is what enabled M3 (IR + bytecode emission) and M4 (loader
+ interpreter + GC) to land in parallel — they spoke to each other
through the byte format §12 of the spec, not through a Rust type
that would have required coordinated edits.

### 3.2 Pipeline

```
.spy source
   │
   ▼
┌────────────┐
│   Lexer    │ → token stream (handles indentation via INDENT/DEDENT)
│   Parser   │ → untyped AST
│  Resolver  │ → AST with name bindings; class layout
│ Typechecker│ → typed AST (bidirectional)
│  IR lower  │ → typed three-address SSA over basic blocks
│ Optimizer  │ → folded IR (const fold + DCE + copy prop)
│  Codegen   │ → typed bytecode (.spyc)
└────────────┘
   │
   ▼
.spyc on disk
   │
   ▼
┌────────────┐
│  Loader    │ → in-memory module
│ Decompile  │ → typed op stream (for JIT)
│  JIT      │ → Cranelift IR → native code (per-function)
│  Interp    │ → handles un-JIT'd functions + native calls
│   GC       │ → mark-sweep, paused during JIT'd execution
└────────────┘
```

The decompilation step (M8) is worth a note. The JIT could have read
the same IR the compiler produces, but that would have required the
compiler to write IR into `.spyc` alongside bytecode. Instead, the VM
decompiles bytecode back into a typed op stream at load time. The
`.spyc` format stayed unchanged; the JIT got its typed input; no
cross-crate ABI break. This is the kind of trade-off the spec-first
discipline made cheap: changing the file format would have required a
spec amendment; reading the existing format twice did not.

### 3.3 The 26 milestones in five phases

The full per-milestone narrative is in
[`docs/thesis/timeline.md`](docs/thesis/timeline.md). The phase
breakdown here is the thesis-level summary.

**Phase A: foundations (M0–M9, 2026-05-17).** Spec, frontend,
typechecker, IR, bytecode, interpreter, GC, threading,
runtime-class dispatch, Cranelift JIT, full JIT coverage. By the end
of M9, all 7 original example programs ran end-to-end and StrictPy
beat CPython 3.12 on every cell of the 16-cell benchmark suite. The
M3.5 sub-milestone is documented separately because M3's IR didn't
update loop-carried locals across back-edges — fib infinite-looped,
dot returned zero, mandelbrot printed nothing, and only `tree.spy`
and `hello.spy` worked. The integration tests of M4 asserted only
`exit_code == 0`; programs were "passing" while computing garbage.
This was the lesson that every subsequent test brief had to require
value-level assertions.

**Phase B: correctness through stress testing (M10–M12,
2026-05-18).** Real programs find real bugs. M10's six new programs
(csv_aggregate, game_of_life, sudoku, json_parse, markov, kvstore,
brainfuck) surfaced 17 bugs in a single round — more than M0–M9
combined had produced. M11's five further programs (lambda_calc,
calculator, tictactoe, levenshtein, lisp) found six more bugs,
including the BUG-029 latent hack from M3 that only triggered when
the 4th user class arrived with a class_id that numerically collided
with an existing type_id. M12 added three more programs (regex,
dijkstra, btree) plus a torture test that converted BUG-026/027's
provisional fix to confirmed. By the end of M12, 28 of 31 known bugs
were fixed; 1 was deferred (BUG-028 lexer line continuation); 2 were
in flight.

**Phase C: language completeness (M13–M17, 2026-05-18 later).** Five
language features in five milestones: short-circuit `and`/`or`
(M13); tuples and destructuring (M14); try/except/finally + raise
(M15); `isinstance` and `match case Constructor()` (M16); generic
free functions with call-site monomorphisation (M17). Each milestone
eliminated a category of workaround that had been clogging the
example programs. The M14 tuple work removed the "1-element mutable
list as multi-return cell" idiom; the M16 match work removed the
`kind: i32` discriminator that every M10–M12 sealed-hierarchy
program had used; the M17 generics work removed the
rewrite-quicksort-per-type friction. M18 ran a stress round
(R1 algorithms_lib, R2 json_parse_v2, R3 expr_interp, R4 graph_lib)
that confirmed the new surface composed cleanly: json_parse_v2 (152
LOC) replaced the M10 json_parse (374 LOC, 8 documented workarounds)
with zero workarounds.

**Phase D: stdlib (M19–M23, 2026-05-19).** A 6-milestone sprint
shipping the import system plus 24 stdlib modules in three sub-phases:
Phase 1 (M19–M21) sys / os / path / io / time / random / math / json
/ re; Phase 2 (M22) argparse / collections / csv / base64 / hashlib /
itertools / statistics / struct / urllib_parse; Phase 3a (M23)
subprocess / pathlib / datetime / threading / queue / sqlite3. M19
landed the load-bearing infrastructure: the `seed_stdlib_modules`
table in `compiler/src/resolver.rs` that maps `import X; X.foo(a)` to
a NativeFn dispatch. After M19, every subsequent stdlib module
slotted in without touching the resolver/typecheck/IR layers. M22
and M23 each ran four parallel agents in isolated git worktrees,
with the orchestrator cherry-picking onto main and resolving
mechanical conflicts in the four shared files (resolver.rs,
native.rs, builtins.rs, STRICTPY_SPEC.md). Total elapsed for
Phase 1+2+3a: ~half a day of parallel agent compute + ~2 hours of
orchestrator integration.

**Phase E: maturity (M24–M25, 2026-05-19 latest).** M24 stress-tested
the Phase 3a surface — four programs combining 6+ stdlib modules
each (job_scheduler, event_log, test_runner, fs_migrator) — and
found BUG-039, the fourth instance of the placeholder-lowering
pattern documented in §6.7 below. M25 collapsed the two-binary
`spyc` + `spy` toolchain into a single Python-analogous `spy`
command (`spy script.spy` compiles-if-stale and runs, with cache in
`__spycache__/`; `spy -c "code"` inline; `spy --compile-only` for
explicit compile workflows). The CLI refactor was the only
milestone that touched no language semantics; it shipped in a
single ~30-minute session.

### 3.4 Test count growth

The per-milestone CSV in
[`docs/thesis/stats/per_milestone.csv`](docs/thesis/stats/per_milestone.csv)
is the authoritative quantitative record. The headline:

```
M1   55 tests     (frontend)
M9  134 tests     (full JIT)
M10 173 tests     (+39 from real-world programs)
M17 255 tests     (+82 across 5 language features)
M22 468 tests     (+213 across stdlib sprint)
M25 586 tests     (+8 unified CLI integration tests)
```

The jump at M10 is the inflection point. M0–M9 added 134 tests via
linear feature growth; M10 added 39 in one milestone of stress
testing. The pattern continued: real programs forced regression
coverage faster than feature work did. By M25, the test suite is
~4× the size of M9, and the bug catalogue is ~3× — both driven
primarily by real-program work, not by feature growth.

### 3.5 LOC growth and what changed where

```
            Compiler   VM      Shared   Examples
M1            5,618    0          500    0
M9           13,200    7,300      720    202
M10          13,700    8,600      720    1,660    ← +1,458 LOC examples
M17          15,631    7,780      623    5,517    ← language features
M22          17,656   10,603      650    9,218    ← stdlib sprint
M25          18,895   12,397      720    11,200
```

The two largest deltas in the VM are the M8/M9 JIT work (+1,400 LOC
across jit.rs + jit_runtime.rs) and the M19–M23 stdlib batch (+~3,500
LOC across builtins.rs). The compiler's largest deltas are the M3 IR
+ codegen (+~3,000 LOC) and the M17 generics work (+~600 LOC in ir.rs
+ typecheck.rs). The shared crate is intentionally tiny: it holds
only the cross-crate contract (opcodes, file format, type tags,
NativeFn IDs).

---

## 4. Performance

### 4.1 The benchmark suite

Four programs, parameterised, each at 4 sizes:

- **Fibonacci** recursive — `fib(20)`, `fib(25)`, `fib(28)`, `fib(30)`,
  `fib(32)`, `fib(33)`. Tests call overhead and small-integer
  arithmetic.
- **Quicksort** — `quicksort(1K)`, `quicksort(5K)`, `quicksort(10K)`,
  `quicksort(50K)`, `quicksort(100K)`. Tests list mutation, indexed
  read/write, recursion.
- **Dot product (f64)** — `dot(10K)`, `dot(100K)`, `dot(500K)`,
  `dot(1M)`. Tests tight numeric loops over contiguous arrays.
- **Mandelbrot 60×30** — nested loops + complex arithmetic + branch
  on iteration count.

16 cells total. Each cell is run best-of-3 wall-clock; the StrictPy
side excludes compile time (the `.spyc` is pre-built); the CPython
side excludes parse+compile time (the `.pyc` is pre-built via
`py_compile`). Methodology details in
[`bench/harness.py`](bench/harness.py).

This suite is small. It does not exercise generic dispatch,
exception handling, the stdlib, or any allocation-heavy workload.
The wins it shows are real but narrow; we discuss the scope
limitations in §7.

### 4.2 The M7 fairness fix

The first benchmark run, in M7, was favourable to StrictPy. The
author then asked: *"Did Python time include the time it took to
compile Python to .pyc?"* It did. The original methodology ran
`python file.py` (parse + compile + execute) versus
`spy.exe file.spyc` (execute pre-compiled bytecode). Adding a
`py_compile` step before timing was the single largest "performance
improvement" in the project: some ratios changed by 30–50% without
writing a line of optimization code.

Both snapshots are preserved in
[`bench/history/`](bench/history/): `m7_pre_jit_unfair.json` is the
biased measurement, `m7_pre_jit_fair.json` is the honest one. The
unfair snapshot is kept on purpose as evidence of how the
measurement bug looked. The lesson — **honest measurement beats
clever optimization** — is the cleanest single-line takeaway from
the methodology chapter.

### 4.3 The M7 → M8 → M9 cliff

| Cell | M7 fair | M8 (JIT) | M9 (full JIT) | CPython 3.12 |
|---|---:|---:|---:|---:|
| fib(20) | 16.2 | 8.0 | **7.8** | 52.3 |
| fib(30) | **931** | **14.6** | **13.5** | **160** |
| fib(33) | 2,410 | 35.5 | **34.8** | 538 |
| quicksort(1K) | 12.4 | 13.0 | **10.2** | 53.4 |
| quicksort(100K) | **660** | **679** | **18.6** | **239** |
| dot(10K) | 13.9 | 14.4 | **10.9** | 53.1 |
| dot(1M) | **604** | **478** | **54** | **239** |
| Mandelbrot | 25.4 | 12.5 | 13.6 | 56.6 |

All times in milliseconds. Bold cells are the headline trajectory.

Two distinct events:

**M8 — Cranelift AOT.** Per-function compilation at module load
time. Coverage limited to integer/float arith, branches, calls, and
list reads. Result: fib(30) drops 64× (931 ms → 14.6 ms), beating
CPython by 11×. Mandelbrot flips. But quicksort and dot product
barely move — their hot inner functions (`partition`, `build_a`) use
`ArraySet` and `ListPush` ops the M8 JIT punted on, and the fixpoint
disable cascade then disabled their callers too. It was not a JIT
quality problem; it was a JIT coverage problem.

**M9 — full coverage.** Extended JIT to `ArraySet`, `ListPush`,
`ArrayNew`, `LoadField`, `StoreField`, `Alloc`, `VirtualCall`. Three
categories of work: (a) inlined ops where the GC is non-moving so
heap pointers are stable (`ArraySet`, `LoadField`, `StoreField`);
(b) Rust runtime helpers called from JIT'd code via the unified ABI
(`rt_list_push`, `rt_list_new`, `rt_array_new`, `rt_alloc`,
`rt_virtual_call`); (c) GC safety via the `in_jit: AtomicUsize`
counter discussed in §2.3. Result: all 4 remaining CPython wins
flipped. quicksort(100K) went from 679 ms to 18.6 ms — a 36× win
from a single milestone of coverage work.

The narrative line — that the M8 JIT was almost-but-not-quite enough,
and M9 closed the coverage gap — is preserved in two agent reports
([`m8_cranelift_jit.md`](docs/thesis/agent_reports/m8_cranelift_jit.md),
[`m9_full_jit_coverage.md`](docs/thesis/agent_reports/m9_full_jit_coverage.md))
and the matching benchmark snapshots.

### 4.4 The M9 → M25 plateau

Sixteen milestones since the JIT shipped. The benchmark numbers have
been essentially flat:

| Snapshot | fib(30) | W/T/L vs CPython |
|---|---:|---|
| M9 | 13.5 ms | 16/0/0 |
| M10 | 15.8 ms | 16/0/0 |
| M11 | 13.1 ms | 16/0/0 |
| M22 | 15.7 ms | 16/0/0 |
| M25 | 13.1 ms | 16/0/0 |

Cross-snapshot variance is ~10–20%, which is below the noise floor
of best-of-3 timing on a Windows workstation. **The JIT-emitted
hot-loop code has not been touched since M9 full-coverage landed.**
Every milestone since has either added correctness (M10–M12), added
language features (M13–M17), added stdlib (M19–M23), or refactored
non-perf-critical infrastructure (M25). The benchmark cells don't
exercise any of that surface, so they don't measure it.

This is a feature, not an accident. The design decisions in §2.3 are
structured to keep the JIT-emitted code stable as the language
grows. New language features (exceptions, generics, isinstance,
match) all introduce code paths that fall back to the interpreter
via the per-function JIT opt-in. The fixpoint disable then ensures
the bench's hot loops still get JIT'd (they don't touch any of those
features). The plateau is the architectural payoff for that
discipline.

### 4.5 Why this comparison is what it is

The headline "StrictPy beats CPython by 4–17×" claim is narrow in
three ways worth stating explicitly:

1. **The suite is 4 programs.** Real-world Python workloads include
   web frameworks, scientific computing, ML training, and
   data-engineering pipelines. None of those are represented. The
   wins generalise to "tight numeric loops, recursive small-int
   arithmetic, integer-keyed list mutation"; they do not generalise
   to "everything CPython does."
2. **The CPython baseline is the C interpreter.** CPython 3.13's
   experimental JIT (copy-and-patch tier-up) is not included. Recent
   PyPy comparison would also be informative; not done here.
3. **No NumPy.** StrictPy's `List[f64]` is already a contiguous f64
   buffer, so the interesting comparison is StrictPy vs CPython
   *with NumPy*. Not done.

The strong claim the archive does support is that **static types
make AOT compilation cheap.** The JIT is ~1,400 lines including
runtime helpers. PyPy is millions of lines. CPython's experimental
JIT involves complex tier-up logic. **The cost of beating CPython on
small workloads scales with the amount of dynamism you have to
defeat**, and StrictPy doesn't allow any.

---

## 5. Methodology

### 5.1 The orchestrator + sub-agent model

The entire project was executed through Claude Code, a CLI agent
harness, with a single human orchestrator. The pattern that worked:

- **One orchestrator session per phase**, holding the spec and
  archive in context; spawning sub-agents for each milestone or
  milestone-slice of work.
- **Sub-agents brief on disjoint files** when possible — lexer +
  parser + pretty-printer (M1) ran as three parallel agents because
  each wrote to a different file. Sub-agents on shared files run
  sequentially, or in worktrees (M22–M24).
- **Every agent task has a machine-checkable acceptance criterion.**
  M8: "fib(30) must run ≥3× faster than the M7 baseline; quicksort
  tests must stay green." M10: "produce N new example programs;
  each must compile + run + assert on a specific output line; the
  parallel agents must not edit each other's files." Vague briefs
  ("improve the IR") never produced useful work.
- **"Stop and report" criteria in every brief.** Agents were
  explicitly authorised to halt and report rather than paper over a
  problem with stubs. This caught at least three near-misses where
  an agent would have produced compiling-but-broken code.

### 5.2 Agent task counts and elapsed time

The archive
[`docs/thesis/agent_reports/`](docs/thesis/agent_reports/) preserves
38 verbatim or condensed agent reports from M8 onwards. Including
the M0–M7 work whose reports were only partially reconstructed, the
total is ~50 agent invocations over 26 milestones. Wall-clock
elapsed: 3 calendar days (2026-05-17 to 2026-05-19); cumulative
agent compute: ~50 hours; orchestrator-attended time: ~25 hours.
The ratio is what one would expect from background-mode agents —
roughly 2× compute per attended hour.

### 5.3 Three parallel agent patterns evolved across the project

**Pattern A: parallel on disjoint files.** M1 ran three agents in
parallel — lexer, parser, pretty-printer — all referencing shared
type definitions but writing to different files. Zero coordination
cost. M6 ran two agents — `tree.spy` regression fix (compiler-side)
and real threading (VM-side) — because the two crates were
independent. M10 ran five agents — AB (compiler/VM modifications),
plus C1/C2/C3 (only-add-new-files in `examples/` and `tests/`), plus
a fix-pass agent — with strict file-ownership rules.

**Pattern B: orchestrator-led integration**. M11's class-system
overhaul was run as a single sequential agent because the bug fixes
all touched `resolver.rs` and `ir.rs`. Parallel agents would have
conflicted. The orchestrator wrote a detailed brief listing all the
M10-flagged architectural bugs as inputs, and the single agent
worked through them. The dossier on this pattern is the lesson:
**deeply-coupled changes do not benefit from parallelism.**

**Pattern C: worktree-isolated parallel agents.** M22 was the first
milestone to use git worktrees for parallel agents. Four agents
(argparse+collections+csv; base64+hashlib; itertools+statistics;
struct+urllib_parse) ran simultaneously in isolated `git worktree`
branches, each writing to the same four files
(`compiler/src/resolver.rs`, `shared/src/native.rs`,
`vm/src/builtins.rs`, `STRICTPY_SPEC.md`). When all four reported
complete, the orchestrator cherry-picked the four worktree commits
onto main, hand-resolving the append-at-end conflicts in those four
files plus the `STRICTPY_SPEC.md` §9.X renumbering. Total
wall-clock: ~1.5 h parallel + ~30 min integration vs ~5 h
sequential at the M19–M20 cadence. M23 and M24 used the same
pattern. The cost is non-trivial integration work; the benefit is
~3× wall-clock reduction for "many small independent additions."

The full discipline document is at
[`docs/thesis/agent_briefing_patterns.md`](docs/thesis/agent_briefing_patterns.md).

### 5.4 What worked

- **Spec-first.** Every brief could reference a section. Without
  the spec, agents would have re-litigated design choices.
- **Hard acceptance criteria.** "Tests pass" alone is meaningless.
  "fib(30) under 30 ms" or "json_parse.spy must round-trip the
  example input byte-identically" produced trustworthy results.
- **Snapshotting before disruption.** The
  [`bench/history/`](bench/history/) snapshots taken before M8, M9,
  M10, M11, M22 are the entire performance narrative. They would
  have been impossible to reconstruct after the fact.
- **Background-mode agents.** Long-running agents (60+ minutes) ran
  in the background while the orchestrator drafted the next
  milestone. Roughly halved overall wall-clock.
- **Honest-reporting bias.** Briefs explicitly asked for "what was
  awkward, what was missing, what didn't work" with at least equal
  weight to "what shipped." This produced the bug catalogue that
  became the most useful artifact.

### 5.5 What didn't work, and the corrections

- **Vague briefs.** Early "implement the IR optimizer" produced
  compiling, tested-green, benchmark-flat code. Lesson: every brief
  needs a measurable outcome.
- **Multi-bug single-agent tasks.** The M3.5 agent was asked to fix
  three M3-era bugs at once. It fixed two cleanly and broke a third
  (tree.spy). Subsequent fix-pass milestones used one-bug-per-agent.
- **Optimistic test discipline.** Original M4 integration tests
  checked `exit_code == 0` only. Programs "passed" while computing
  wrong values. Fixed from M5 onwards by requiring value-level
  assertions.
- **Single-side benchmarks.** Pre-M10 the Python comparison gave
  Python its parse+compile time but excluded StrictPy's. Caught at
  M10-prep by the user noticing; the unfair snapshot is preserved
  as evidence.
- **Agents running out of compute budget at the commit step.** In
  M23 P3a-D and all four M24 agents, the agent finished the
  substantive work but exhausted the budget while writing the long
  report — `git commit` never happened. Orchestrator committed each
  worktree on the agent's behalf. M25+ pattern note: briefs should
  say "commit EARLY, before the long report."
- **Git three-way merge mis-aligning parallel handlers.** During the
  M23 P3a-D cherry-pick, git aligned `sqlite3.column_names` with
  `pathlib.read_lines` at a shared `let sp = alloc_string(...) as
  u64;` line. The merge result semantically replaced one handler's
  tail with the other's. Recovery: reconstruct from worktree
  history. Future worktree rounds should use distinct loop-variable
  names or distinct trailing comment markers to break the alignment
  heuristic.

### 5.6 The honest-scope statement

The methodology archive supports claims like:

- "StrictPy's JIT made fib(30) 64× faster than its interpreter, with
  ~1,400 lines of Cranelift integration." ([`bench/history/m8_jit.md`](bench/history/m8_jit.md))
- "17 of 34 distinct bugs in the project were found by running real
  programs, not by writing tests." ([`docs/thesis/bugs/catalog.md`](docs/thesis/bugs/catalog.md))
- "Static-type-driven AOT compilation requires none of the
  speculation / deopt machinery a dynamic-language JIT needs." (§2)

It does NOT support claims like:

- "StrictPy is faster than CPython on real workloads." — 4 programs,
  not a workload.
- "Static typing is more productive than dynamic typing." — single
  developer, single project, no controlled comparison.
- "AI-assisted development is generally faster." — single project,
  no baseline. The 3-day calendar-time elapsed is anecdotal at best.

The archive is structured to make these scope distinctions auditable
rather than rhetorical.

---

## 6. Findings

### 6.1 The bug catalogue

[`docs/thesis/bugs/catalog.md`](docs/thesis/bugs/catalog.md) records
every distinct bug discovered across M0–M25. Summary by category:

| Category | Found | Fixed | Deferred |
|---|---:|---:|---:|
| Silent miscompile (codegen) | 9 | 9 | 0 |
| Vacuous output (IR lowering punt) | 3 | 3 | 0 |
| Vtable / inheritance | 7 | 7 | 0 |
| Typechecker rejects valid code | 3 | 3 | 0 |
| Frontend operator semantics | 2 | 2 | 0 |
| Runtime memory / GC | 2 | 2 | 0 |
| Stdlib missing | 5 | 5 | 0 |
| Parser / lexer | 1 | 0 | 1 |
| Formatting / spec consistency | 1 | 1 | 0 |
| Spec/runtime drift | 1 | 1 | 0 |
| **Total** | **34** | **33** | **1** |

The single open bug is BUG-028: the lexer doesn't continue lines
across a trailing `+` operator. Mechanically simple to fix; cost of
the workaround (use parentheses or accumulator variables) is small
enough that deferral never blocked anything.

Seven patterns from the catalogue generalise beyond StrictPy. Each
gets a sub-section here.

### 6.2 Pattern: stress testing has superlinear ROI

M0–M9 added 134 tests through linear feature growth and surfaced ~12
bugs. M10 added one round of stress testing (6 parallel agents
writing new programs) and surfaced **17 bugs in a single milestone**.
The pattern continued:

| Round | Programs | LOC | Bugs |
|---|---:|---:|---:|
| M10 (round 1) | 6 | ~1,660 | 17 |
| M11 (round 2) | 5 | ~1,810 | 6 |
| M12 (round 3) | 3 | ~1,477 | 2 |
| M18 (round 4) | 4 | ~1,900 | 1 |
| M24 (round 5) | 4 | ~1,800 | 1 |

The ROI curve flattens — by M18 the system is in steady state — but
**every round still found at least one bug**. The bugs found in late
rounds are not architectural; they are placeholder lowerings (BUG-037
`??`, BUG-039 `in`) latent since the operator first shipped. They
would not have been found by feature-development testing because the
operators "worked" in the positive form.

The mechanism: real programs use operators in combinations and
contexts that unit tests don't. A unit test for `dict.has(k)` checks
that it returns the right value. A real program — `event_log.spy`'s
histogram — uses `bucket in seen` as a guard for whether to
initialise a counter. The unit test never tests the `in` operator
because `in` has its own (broken) lowering; the real program does
because that's the natural Python idiom.

### 6.3 Pattern: bugs cluster around a root cause

BUG-001 was a nullable-narrowing silent miscompile found by
`csv_aggregate.spy`. The fix-pass agent didn't stop at one bug — it
audited every `Ty::Primitive` match arm in `codegen.rs` and found
**four more siblings** (BUG-002 through BUG-005), each silently
miscompiling a different operator under nullable-narrowed operands.
The audit pattern is documented in
[`docs/thesis/design_decisions/nullable_unwrap_dispatch.md`](docs/thesis/design_decisions/nullable_unwrap_dispatch.md):

```
grep -r "Ty::Primitive" compiler/src/codegen.rs
# for each hit, check: can a Nullable(T) slot reach here?
# if yes: bug. fix by unwrapping.
```

Five seconds of grep per audit hit; five bugs found in one pass.

BUG-017 ("vtable wraps mod 4") turned out to be three converging
adjacent bugs: subclass vtables didn't inherit parent slots; the IR
didn't walk up the inheritance chain for inherited fn_ids; and the
infamous BUG-029 (§6.5) class_id↔type_id collision. None of them
alone would have produced the visible symptom; all three together
did. The fix had to address each one separately.

**The lesson: when you find one bug, look hard for siblings.** A
visible symptom is often a single observation from a multi-bug root
cause; fixing one bug at a time without auditing can produce
correctness "improvements" that don't actually change behaviour
because another sibling is still active.

### 6.4 Pattern: deterministic siblings unlock non-deterministic mysteries

BUG-026 was a non-deterministic STATUS_HEAP_CORRUPTION crash in
`json_parse.spy` and `calculator.spy`. It depended on subclass
declaration order, function declaration order, and apparently random
heap-layout variation across runs. Across M10, it was the worst-
classified bug in the project: the symptoms shifted under any
attempted fix.

The breakthrough was M11's C6 lisp interpreter agent finding
BUG-030 — `Pair(Value) { car: Value }` then `p.tag()` →
deterministic access violation. The same shape (subclass with
class-ref fields + virtual call) but with a deterministic repro.
Reducing BUG-030 to its minimal form exposed BUG-016 (subclass field
aliasing) as the root cause: subclass fields started at offset 16,
overlapping the parent's vtable pointer. Fixing BUG-016 collateral-
fixed BUG-030 (deterministic) AND BUG-026 (non-deterministic) AND
BUG-027 (position-sensitive crash from function ordering).

The non-determinism was always heap-layout variability supplying
different exact failure modes; the underlying trigger was always
the same offset aliasing. **Without the deterministic sibling, the
fix would have been guesswork against shifting symptoms.**

M12's torture test (`compiler/tests/heap_corruption_torture.rs`)
then ran 250 sequential invocations of the canonical repros (100×
calculator + 100× json_parse + 50× lisp) and produced zero failures
in 3.12 s. BUG-026/027 went from "provisionally closed" to
"confirmed fixed." The marginal cost of "provisional → confirmed"
was ~20 minutes of agent time and ~3 s of CI wall-clock per future
run. That trade is almost always worth making.

### 6.5 Pattern: latent bugs accumulate dose-dependently

BUG-029 — the `op_new` class_id↔type_id collision — was the
most thesis-quality bug of the entire project. The shape:

1. M3 introduced a hack in the VM's `op_new`: if the operand didn't
   match a known type_id directly, fall back to indexing the type
   table as if it were a class_id.
2. The hack worked silently for 10 milestones because class_id and
   type_id numeric ranges never overlapped.
3. At M10, the 4th user class arrived. Its class_id was 16. Shape's
   type_id was also 16. The fallback fired, returned the wrong
   RuntimeType, and the program allocated a Pentagon with Shape's
   vtable.
4. The visible symptom was "vtable wraps mod 4" — Pentagon's
   virtual method calls went to Shape's slots.

Two conditions were needed: 10 milestones of accumulated state and a
specific numeric collision. Either alone would have left the bug
dormant.

**The lesson: latent bugs accumulate dose-dependently.** Convenience
hacks that paper over a missing case (a fallback, a default branch,
a permissive parse) can work silently for years before enough state
accumulates to trigger them. The cost of the hack is paid by the
milestone that hits the trigger, plus all the diagnostic work
required to reach the M3-era hack from an M10 symptom.

Two practical recommendations follow from this pattern:

1. **Treat fallback branches as suspicious code that requires
   regression coverage.** If the fallback exists, write a test
   that exercises it deliberately. If you can't, the fallback is
   probably wrong.
2. **When a bug's symptom looks like a numerical curiosity ("mod 4",
   "every 256 calls", "starts at the 4th class"), suspect a
   collision between two id spaces that share a numeric range.**

### 6.6 Pattern: silent miscompiles hide behind positive-form conventions

BUG-008: `is not none` was emitting `RefEq` not `not RefEq`. Every
`if x is not none:` had been silently running the wrong branch since
M2. No existing example caught it because they were all coded around
the bug — they used `if x is none: ... else: ...` (positive form
inverted).

BUG-034: `str != str` always returned true because `emit_binop`'s
`Ne` arm had no `is_str` branch — fell through to `INe`, comparing
two heap-pointer u64s. Every program using inequality compare on
strings was wrong. No prior example tripped it because every prior
example used `==` for string compare.

Both bugs sat latent from when the operator first shipped. Both
were found by stress tests that organically used the *negative*
form: BUG-008 by `json_parse.spy`, BUG-034 by `btree.spy` building
`FAIL got=X want=Y` output lines.

**Lesson: any new comparison operator needs both forms tested
explicitly.** Code conventions naturally drift toward one form;
silent miscompiles of the other form go undetected for as long as
the convention holds.

### 6.7 Pattern: placeholder IR lowerings silently miscompile

The most generalisable finding from the catalogue. Four bugs share
the same shape:

| Bug | Operator | Placeholder lowering | Found in | Fixed in |
|---|---|---|---|---|
| BUG-008 | `is not` | `RefEq` (not `not RefEq`) | M10 | M10 |
| BUG-034 | `str !=` | `INe` (no `is_str` branch) | M12 | M12 |
| BUG-037 | `??` (null-coalesce) | `Copy(rhs)` (always fallback) | M20a | M21 |
| BUG-039 | `in` / `not in` | `IEq` / `INe` (pointer compare) | M24 | M24 |

Each is a binary-op match arm in `compiler/src/ir.rs::emit_binop`
that punts on the type-dependent lowering with a hardcoded `IROp`.
Each is found organically by a stress test that uses the operator in
the form the placeholder doesn't handle.

The pattern shipped in M2 alongside the type system and surfaced
across four separate milestones over the next 23 milestones. A
mechanical audit of `emit_binop` — "for every binary operator whose
semantics depend on operand type, verify the lowering dispatches on
type" — would have caught all four at once.

The audit is now an explicit menu item for v0.3. The cost is
30–60 minutes; the benefit is closing whatever fifth instance of
the pattern is currently hiding (Tuple compares and Set membership
are the strongest candidates).

**The thesis-level takeaway: "the parser and typechecker accept it
and there's a lowering" is not the same as "the lowering is
correct." Placeholder branches in widely-shared dispatch tables are
particularly invisible because the dispatch architecture hides them
from grep.**

### 6.8 Pattern: confirmation is a deliverable

The M12 round shipped three stress programs. **Two of three found
zero bugs.** This was the headline.

Pre-M11, every class-heavy program (json_parse, calculator, lisp)
shipped with extensive workaround sections in its source — the
"known gaps" comment block. The M12 regex agent's report opened:
"sealed hierarchy with 8 subclasses, 6 virtual methods, class-ref
subclass fields, ran first-try without a single workaround." Same
for dijkstra: parallel `List[List[T]]` fields, recursive methods,
clean run. **Stress tests without workarounds are themselves a
quantitative measurement of language maturity** — the class system
overhaul of M11 actually landed, not provisionally landed.

The pattern recurs in M18 (R1 algorithms_lib and R2 json_parse_v2
both zero-bug); M22 (all four parallel stdlib agents zero-bug); M23
(three of four zero-bug); M24-A, M24-C, M24-D (zero-bug, three of
four). The "absence of bugs found" is itself a publication-quality
result when it is the *first time* a particular program shape works
first-try.

The marginal cost of running a confirmation round is one or two
agent invocations; the credibility upgrade — provisional → confirmed
— is worth far more than that.

### 6.9 Pattern: deferred ≠ unimportant

BUG-016 (subclass field aliasing) was marked "deferred" at end-of-M10
because its fix looked architectural. When M11 finally fixed it,
the fix collateral-closed BUG-026 and BUG-027 (the non-deterministic
heap corruption that had been the worst-classified bug in the
project) — see §6.4.

**The marginal cost of leaving a load-bearing correctness bug
"deferred" is paid by every subsequent program that has to work
around it OR by every milestone that hits its manifestations.** A
single architectural bug can produce many surface-symptom bugs;
fixing it early prevents the surface bugs entirely. The triage rule
that worked in practice: "deferred" should mean "the workaround is
acceptable for the current milestone but the fix has a milestone
slot," not "we'll get to it." Each deferred bug should have a named
milestone owner from the moment it lands in the catalogue.

---

## 7. Limitations

The wins this thesis describes are narrow in specific, auditable
ways. Listed here for honesty.

### 7.1 Benchmark scope

The 4-program / 16-cell benchmark suite tests tight numeric loops,
list mutation, integer recursion, and small-vector floating-point
work. It does NOT test exception handling, generic dispatch, the
stdlib, large-allocation workloads, multi-threaded contention,
network I/O, or anything that exercises StrictPy's "Python-shaped"
surface (M13–M25 work). The 16/0/0 win against CPython 3.12 is
real but applies only to the cells measured.

Reasonable next steps: extend the bench to use the stdlib (e.g. JSON
serialization round-trip, regex match throughput, CSV parsing, SQLite
queries); add allocation-heavy workloads; add long-running programs
that would stress the conservative-GC `in_jit` pause. None of these
are done.

### 7.2 The GC pause during JIT

The `in_jit: AtomicUsize` counter is correct for benchmark workloads
(the 16 MB arena rarely fills) and wrong for long-running programs
that need GC to make progress during JIT'd execution. The proper
fix is precise stack maps generated by Cranelift's safepoint
infrastructure — substantial work, deferred. Until that lands, any
StrictPy program with >16 MB of live data and JIT'd hot loops will
either OOM or stall.

### 7.3 The single open bug

BUG-028: the lexer doesn't continue lines across a trailing `+`
operator (`return "a " + \n "b"` errors with E0001). Workaround:
parentheses (`return ("a " + \n "b")`) or accumulator variables.
Mechanically simple to fix; the cost of the workaround is small
enough that the deferral never blocked anything. Listed here so
readers know what the "33 of 34 fixed" number refers to.

### 7.4 v0.2 feature gaps

The following are deferred to a v0.2 release:

- **Generic classes** (`class Box[T]:`). The M17 generics
  infrastructure handles free functions only. Generic classes need
  resolver-side template instantiation and IR-level class-id
  rewriting; deferred because v0.1 stdlib classes don't require it.
- **User-defined exception subclasses.** v0.1 ships 10 built-in
  exception names (`Exception`, `ValueError`, `IOError`,
  `ZeroDivisionError`, etc.); user-defined `class MyError(Exception):`
  is parsed but the resolver rejects it.
- **`with` → try/finally desugaring.** A `with open(...) as f:`
  inside a `try ... except IOError:` does NOT route the IOError
  through the except. Workaround: explicit
  `try: with open(...) as f: ... except IOError:`. Known M15
  follow-up.
- **Bounded generics** (`T: Comparable`). v0.1 generics
  re-typecheck under substitution per instantiation, which is
  approximately correct but allows operations the source bound
  would have rejected. v0.2 work.
- **Phase 3b stdlib** — `socket`, `http_client`, `ssl`. The big
  remaining stdlib domain.
- **NumPy / pandas integration.** Three theoretical paths exist
  (embed CPython, FFI to numpy's C lib, native reimplementation);
  none planned. The architectural reasoning is in
  [`docs/thesis/design_decisions/why_no_numpy_pandas.md`](docs/thesis/design_decisions/why_no_numpy_pandas.md).

### 7.5 Methodology caveats

The "AI-orchestrated development" methodology was used on one
project by one developer. The 3-calendar-day elapsed time is real,
but it is anecdotal: there is no controlled comparison with a
human-only build of the same artifact. Claims about generalisable
productivity ratios are not supported by this archive — only claims
about *this* project's elapsed time and bug-discovery patterns are.

The 38 agent reports preserved in
[`docs/thesis/agent_reports/`](docs/thesis/agent_reports/) document
specific tasks, specific briefs, and specific outcomes. They show
what worked for *these* briefs on *this* codebase. Whether the
patterns transfer to other projects is an open empirical question.

### 7.6 Implementation defects acknowledged but not fixed

The M25 milestone note records three minor cache-hygiene issues with
the unified CLI that are deferred to v0.3: a cross-process race when
two `spy hello.spy` invocations concurrently rewrite the same
`__spycache__/hello.spyc`; no fallback when the source directory is
read-only; and the cache key doesn't include a build identifier so a
major upgrade requires manually clearing `__spycache__/`. None block
normal use; all are listed in [`docs/thesis/milestones/m25_unified_cli.md`](docs/thesis/milestones/m25_unified_cli.md).

---

## 8. Conclusion

### 8.1 What the result changes

The headline empirical claim — that a statically typed Python
dialect with mandatory annotations can beat CPython 3.12 by 4–17×
on tight numeric workloads, with ~31K lines of Rust and ~1,400 lines
of JIT code — is a single data point. But it is a data point that
constrains an argument:

- **Whatever PyPy is spending its millions of LOC on, most of it is
  defeating dynamism**. The StrictPy JIT does none of that work and
  emits competitive native code via a straightforward IR translation
  pass. The cost of dynamism is paid in JIT complexity, not just
  runtime.
- **CPython's interpreter loop is genuinely good**. The M7-fair
  snapshot — interpreted StrictPy losing to CPython by 3–5× on the
  same benchmarks where M8 won by 11× — is the evidence. The fix
  was not "write a better interpreter"; it was "stop interpreting."
- **AOT compilation of typed bytecode is mechanical.** Cranelift's
  IR maps to typed StrictPy IR ~1:1 for the supported ops. Adding
  coverage was incremental — M8 covered arith/branches/calls/reads;
  M9 added mutation/fields/vcalls — and produced a clean step
  function in benchmark wins.

### 8.2 What the result doesn't change

CPython is not threatened. Real-world Python workloads — web
frameworks, scientific computing, ML, data engineering — depend on
the dynamism this project explicitly forbids. NumPy, pandas, scipy,
PyTorch all link against libpython, use refcounting, implement the
Python C API. StrictPy cannot run them. The interesting comparison
is not "StrictPy vs CPython"; it is "StrictPy's design space (typed
Python) vs CPython's design space (untyped Python with optional
hints)." Each has costs the other doesn't pay.

### 8.3 Where the evidence runs out

The archive is structured to make these distinctions explicit. The
empirical claims it supports are narrow: a 4-program benchmark
suite, a 70-program example corpus, a 34-entry bug catalogue, a
26-milestone timeline. The architectural claims it supports are
stronger: the five design decisions in §2.3 are each independently
justified by load-bearing milestones, and each is documented with
the alternative that was considered and rejected.

What the archive does NOT establish:

- That this development methodology is generally faster than
  human-only development. Single project, no baseline.
- That static typing is more productive than dynamic typing. Single
  developer, no controlled comparison.
- That StrictPy is faster than CPython on real workloads. 4 micro-
  benchmarks.
- That AI-orchestrated systems work scales to projects 10× larger.
  This was a ~31K-LOC project; the agent-task complexity at 300K
  LOC is unstudied.

### 8.4 What the next pass would do

If a v0.2 series were to continue, the highest-leverage items are
visible from the archive:

1. **Precise stack maps for the GC** — closes the in_jit pause
   limitation; enables long-running programs.
2. **Generic classes** — unblocks typed stdlib surfaces
   (`JsonValue` tree, `re.Pattern`, `sqlite3.Connection`,
   `datetime.DateTime`, streaming `Hasher`).
3. **Phase 3b stdlib** — socket / http_client / ssl. The big
   remaining domain.
4. **The placeholder-lowering audit** — 30–60 minutes,
   mechanically catches the fifth-instance bug whose existence the
   pattern in §6.7 predicts.
5. **A larger benchmark suite** — Python stdlib workloads, allocation-
   heavy programs, multi-threaded contention, long-running with GC
   pressure.

### 8.5 The minimum the archive promises

The project archive at [`docs/thesis/`](docs/thesis/) is fully
reproducible from a `git clone`:

```powershell
git clone https://github.com/amitgangrade/StrictPy
cd StrictPy
cargo build --release
cargo test --workspace --release    # 586 tests pass
python bench/harness.py             # regenerates BENCH_REPORT.md
spy examples/fib.spy                # 13.1 ms for fib(30)
```

The CSV at
[`docs/thesis/stats/per_milestone.csv`](docs/thesis/stats/per_milestone.csv)
is the quantitative ground truth. The benchmark JSONs in
[`bench/history/`](bench/history/) are the timestamped performance
record. The 38 agent reports in
[`docs/thesis/agent_reports/`](docs/thesis/agent_reports/) are the
methodology evidence. The 34-bug catalogue at
[`docs/thesis/bugs/catalog.md`](docs/thesis/bugs/catalog.md) is the
correctness record.

The minimum claim this archive makes — and supports with evidence
that can be audited line by line — is that a small team or single
developer using AI orchestration can, in 3 calendar days, build a
working compiler-VM-JIT toolchain for a statically typed Python
dialect that beats CPython on a small benchmark suite, with a
disciplined enough record that the bugs found, the design choices
locked, and the decisions deferred are all individually inspectable
in retrospect.

That last clause is the part this thesis most cares about: not the
benchmark wins, not even the language design, but the **shape of the
archive**. The discipline that produced it is reproducible. The
patterns in §6 are generalisable. The methodology in §5 is teachable.

Whatever StrictPy itself becomes, the archive is the thing the next
project copies.

---

*Companion artifacts:*

- [`STRICTPY_SPEC.md`](STRICTPY_SPEC.md) — canonical language and VM specification (frozen v0.1; in-place amendments documented per milestone)
- [`BLOG_POST.md`](BLOG_POST.md) — narrative version for a broader audience
- [`README.md`](README.md) — build instructions, what runs today
- [`BUGS_KNOWN.md`](BUGS_KNOWN.md) — deferred-bug catalogue (currently: BUG-028 only)
- [`bench/BENCH_REPORT.md`](bench/BENCH_REPORT.md) — current benchmark rendering (regenerated by `python bench/harness.py`)
- [`docs/thesis/timeline.md`](docs/thesis/timeline.md) — per-milestone narrative with key events
- [`docs/thesis/methodology.md`](docs/thesis/methodology.md) — how the project was conducted
- [`docs/thesis/agent_briefing_patterns.md`](docs/thesis/agent_briefing_patterns.md) — concrete brief patterns
- [`docs/thesis/agent_reports/`](docs/thesis/agent_reports/) — 38 verbatim/condensed agent task reports
- [`docs/thesis/bugs/catalog.md`](docs/thesis/bugs/catalog.md) — every bug, classified, fixed/deferred
- [`docs/thesis/design_decisions/`](docs/thesis/design_decisions/) — six load-bearing architectural choices
- [`docs/thesis/stats/per_milestone.csv`](docs/thesis/stats/per_milestone.csv) — machine-readable per-milestone metrics
- [`docs/thesis/milestones/`](docs/thesis/milestones/) — per-milestone deep-dive notes (M12, M13–M17, M18, M19–M21, M22, M23, M24, M25)

*Author:* Amit Gangrade. *Orchestration:* Claude Code (Claude Opus 4.7). *Period:* 2026-05-17 to 2026-05-19.
