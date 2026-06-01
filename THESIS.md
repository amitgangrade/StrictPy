# StrictPy — a statically typed Python dialect, compiler, and bytecode VM

*A ~56-milestone, AI-orchestrated systems-language project (v0.2.0 frozen on day 5; v0.3 work began at M31 and continues through M56+M51 — a from-scratch Pandas-shaped data package (M37–M51), a browser-served DataFrame UI (M50), and a native-SDL2 desktop-games stack with two playable games (M52–M56)).*

**Project**: <https://github.com/amitgangrade/StrictPy>
**Archive**: [`docs/thesis/`](docs/thesis/) — quantitative record (per-milestone CSV, 56+ verbatim agent reports, 35-entry bug catalog, 8 benchmark snapshots).
**Spec**: [`STRICTPY_SPEC.md`](STRICTPY_SPEC.md) — frozen at v0.1 on day one; v0.2 release tagged at M30; v0.3 amendments in place through M39.
**Date span**: 2026-05-17 → 2026-06-01 (v0.2 frozen in the first 6 days at M30; v0.3 work — tabular package, desktop UI, games stack — continued across the following ~2 weeks).
**Outcome**: A working compiler + bytecode VM + Cranelift JIT that beats CPython 3.12 by **4–17×** on all 16 cells of the canonical 4-program benchmark suite and wins **28 of 30 cells** on an extended suite. **v0.2 frozen state** (at M30): 96 example programs (including a complete HTTP/1.1 + HTTPS web framework written in StrictPy user code), 36 stdlib modules, **35 bugs found, ALL 35 fixed (0 deferred)**, tagged as **v0.2.0**. **v0.3 work since**: generic classes (M31), async I/O event loop (M32), precise GC stack maps (M33), first stdlib classes — typed JsonValue tree (M34), four more stdlib classes via three parallel agents (M35), `StdlibItemKind::Class` infrastructure refactor (M36), and a from-scratch Pandas-shaped data package `tabular` (M37–M51) that grew from the common-80% of pandas to an index-aware, MultiIndex-capable, categorical-and-rolling-window-bearing DataFrame library with a comprehensive vs-pandas-3.0 benchmark suite, a browser-served interactive UI (M50 `tabular.serve`), and a memory-cost deep-dive (M48b). The same `gfx`/SDL2 stdlib that M52–M56 added makes StrictPy host **native 60-FPS desktop games** (Snake, Tetris) written in StrictPy user code. **1,102 passing tests** post-M51; **26 stdlib classes; 120 example programs**. A standout performance result: M48 measured StrictPy's categorical group-by losing ~12× to pandas, wrote a numeric target into the next brief, and M49 hit a **~194× speedup** (12.8 s → 66 ms), beating pandas's own Categorical fast-path by ~14× at high cardinality. **Lesson 1 methodology streak: 39 clean-commit agents** across the games stack (M28 → M56).

---

## Abstract

We built **StrictPy** — a statically typed Python dialect with its own
Rust toolchain (compiler, bytecode VM, Cranelift JIT) — over 6 calendar
days using an AI-orchestrated workflow. The implementation is ~42K
lines of Rust plus ~14K lines of example StrictPy code. fib(30)
runs in 13.1 ms versus CPython 3.12's 159.5 ms; on every cell of the
canonical 4-program benchmark suite StrictPy is 4–17× faster than
CPython, and on an extended 30-cell suite (5 additional pure-compute
programs + 5 stdlib-comparison programs) StrictPy wins 28 cells, ties
2, loses 0. The JIT itself is ~1,400 lines of Cranelift integration,
dramatically smaller than dynamic-language JITs because mandatory
static types eliminate the type-profiling, inline-cache, and
deoptimization infrastructure that consumes most of the engineering
effort in PyPy-class implementations.

The project's third contribution is empirical: a complete HTTP/1.1 +
HTTPS web framework (Sinatra/Flask-shaped, ~970 lines of framework
code plus a TODO API demo) written **in StrictPy user code, on top of
the language's own stdlib**, demonstrating that the surface is
sufficient to host real software (within 2× of Flask+gunicorn — the
gap is the async event loop, deferred). Stress-testing finding: the
1,500-LOC framework surfaced **zero new bugs in the networking
stdlib** — the first stress round in project history with zero finds.

The project's fourth contribution, added during the v0.3 phase, is a
from-scratch **Pandas-shaped data package** (`tabular`) shipped across
three consecutive single-agent milestones (M37 + M38 + M39, ~7,800 LOC
of native Rust handlers). It covers the common-80% of pandas
workflows: typed columns with per-column null masks, CSV+SQL I/O,
filter/sort, per-column aggregations including sample std/var/median,
hash-based group-by with shortcut aggregations + custom spec lists,
merge (all four join modes via hash-join), pivot (long→wide), and
melt (wide→long). The relevance: it demonstrates that the post-M36
`StdlibItemKind::Class` infrastructure scales to multi-class packages
without prelude bloat, and that the orchestrator+agent pattern can
ship ~2,500-LOC packages in a single milestone (three consecutive
times, all clean-commit per Lesson 1).

M40–M51 took `tabular` the rest of the way: a single- then
multi-column index that propagates through every operation (M40–M46),
a `ColumnCategorical` dtype and rolling-window aggregations (M47), a
comprehensive vs-pandas-3.0 benchmark harness (M48), a categorical
codes-hash optimization that turned a ~12× loss into a ~194× win
(M49), a localhost HTTP server that renders a DataFrame in a browser
tab with interactive filters/pivots/charts and zero new crate
dependencies (M50), a chainable `RollingWindow` (M51), and a
byte-level memory deep-dive that root-causes StrictPy's 4–5× peak-RSS
gap to an 8-byte-per-boolean null mask carried on every column (M48b).
The project's fifth contribution followed: M52–M56 added a `gfx`
stdlib over native SDL2 (plus pure-Rust audio and font rendering) and
two complete games — Snake and Tetris — written in StrictPy and
running as native 60-FPS windows, evidence that a statically typed
Python can host interactive desktop software, not just batch programs.

The project's second contribution is methodological: a ~56-milestone
record of how Claude Code orchestrated agent tasks, what agent briefs
worked, what failed, and a 35-bug catalogue with root-cause analysis
showing that **17 of 35 bugs were found by running real programs
rather than by writing tests** — a result the archive preserves as
the auditable record of the stress-test ROI curve. The Lesson 1
escalation (numerical thresholds in agent briefs) held across
**39 consecutive clean-commit agents** (M28 → M56). The longer run
also surfaced a *taxonomy of milestone shapes* that predicts commit
cadence (disjoint-handler / shared-infra / cross-dispatch /
net-new-feature; §5), a genuine **parallel-work collision** where two
agents independently built the same feature and the resolution was to
reconcile rather than force-push (§5), and a **delegate-blind** pattern
where a sandboxed sub-agent ships unverified code that the orchestrator
must build, test, and integrate (§5).

This document is a technical thesis aimed at compiler and systems
engineers. It synthesises the project archive into eight chapters
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
- **§3 Implementation** — the 39-milestone trajectory, partitioned
  into nine phases (A–I): foundations (M0–M9), correctness through
  stress testing (M10–M12), language completeness (M13–M17),
  stdlib (M19–M23), maturity + benchmark expansion (M24–M27),
  networking + web framework + v0.2 freeze (M28–M30), v0.3 begins
  (M31–M34 — generics / async / GC / first stdlib classes), and
  Phase I — stdlib classes to a Pandas-shaped data package
  (M35–M39).
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

### 3.3 The 39 milestones in nine phases

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

**Phase F: benchmark expansion + filesystem/compression stdlib
(M26–M27, 2026-05-19–20).** M26 added an extended 30-cell
benchmark suite (5 pure-compute programs — n-queens, Sieve, matrix
multiply, binary tree, heap sort — plus 5 stdlib programs —
JSON round-trip, regex throughput, SHA-256, CSV parse, SQLite
insert+query). Result: **28 wins, 2 ties, 0 losses** vs CPython
3.12. Two empirical findings worth recording: the `btree` row
narrows monotonically as allocation pressure grows (0.23× → 0.71×
→ 1.13× from n=1k to n=10k — at large recursive-allocation counts,
StrictPy's `rt_alloc` + conservative GC overhead overtakes the JIT
win, the kind of cell precise stack maps would fix); and the
stdlib comparison, expected to land near 1× since both sides do
their actual work in C/Rust, instead favours StrictPy across
every cell because Python's ~50–70 ms process startup overhead
dominates short workloads. M27 then shipped **Phase 3c stdlib**
in a 5-parallel-worktree round: `shutil`, `tempfile`, `glob`,
`fnmatch`, `gzip`, `zlib`, `bz2`, `zipfile`, `tarfile`, `logging`
— 9 modules + ~95 NativeFns. The agents had high commit-discipline
variance (2 of 5 still failed to commit before budget exhaustion
despite the brief's "commit early" language), which led directly
to the methodology refinement in M28.

**Phase G: networking + hosting real software (M28–M29.5,
2026-05-20–21).** Three milestone-clusters in tight sequence:

- **M28** (3 parallel agents): `socket` (TCP/UDP raw + listen/
  accept + DNS), `ssl` (TLS-over-TCP via rustls — client side
  only), `http_client` (HTTP/1.1 via ureq). 3 stdlib modules,
  ~40 NativeFns. The biggest single domain gap closed since the
  M19 stdlib seam.
- **M28.5** (single focused agent): server-side TLS extension to
  the `ssl` module. 3 NativeFns (`ssl.load_server_config`,
  `ssl.accept_tls`, `ssl.free_server_config`). Closes the
  HTTPS-server gap.
- **M29 + M29.5** (two single-program stress tests): a complete
  HTTP/1.1 + HTTPS web framework + TODO API demo, written
  in StrictPy user code on top of the language's own stdlib.
  M29 shipped the framework (~1,446 LOC); M29.5 added the
  five "Tier 1" features that separate "demo" from "small-internal-
  API-grade" — HTTP keep-alive, chunked transfer encoding (both
  directions), `multipart/form-data` parsing, graceful shutdown,
  HTML error pages — bringing the total to ~2,443 LOC.

M28–M29.5 is the part of the project that most directly tests
the empirical claim of the work: "is the language now sufficient
to host real software?" The answer is yes (with documented v0.2
limitations: no async event loop, no HTTP/2, no WebSockets, no
production-grade password hashing in stdlib). M29 measured ~2,200
req/s for the framework's `/health` endpoint and ~1,500 req/s with
SQLite-backed `GET /api/todos` — within 2× of Flask+gunicorn, the
remaining gap being the async event loop (v0.3).

The **methodological** finding from Phase G is the most generalisable
one in the project: the M28 brief's strengthened "**FIRST commit
before 60% of budget**" language, with explicit checkpoint discipline
(20% / 40% / 60% / 80% / 95%), produced **3 of 3 committed agents**
in M28, then **1 of 1** in M28.5, then **1 of 1** in M29, then **1 of
1** in M29.5 — a five-agent winning streak after the 2 of 5 partial-
failure rate in M27. The streak then extended through M30 (2 agents),
M31 (1), M32 (1), M33 (1), and M34 (1) — **14 consecutive
clean-commit agents** over 4 calendar days. The brief change was
the only meaningful difference. The pattern generalises: numerical
thresholds in agent briefs move the needle where qualitative urgency
("commit early") doesn't.

**Phase H: v0.2 release + v0.3 architectural work (M30–M34,
2026-05-21).** After M30 closed the last two open bugs (BUG-028
lexer continuation + BUG-040 socket.close_listener), the project
reached its first "v0.2 frozen" state — 35 bugs found, all 35 fixed,
0 deferred — and was tagged as **v0.2.0** with a comprehensive
[`RELEASE_NOTES_v0.2.md`](RELEASE_NOTES_v0.2.md). v0.3 work then
began at M31 and shipped four of the priority items from the
THESIS.md §8.4 next-pass list in tight sequence:

- **M31 generic classes** (`class Box[T]:`, `Pair[K, V]`,
  `Stack[T]`). Extended the M17 worklist-driven monomorphisation
  infrastructure from free functions to classes. New IR Pass 2.7 +
  3.6 running to joint fixpoint with M17's existing passes.
  Per-instantiation type_id + method bodies via mangled names
  (`Box__i64`, `Pair__str_i32`). Existing M11 vtable infrastructure
  handles dispatch — zero new VM opcodes. Constructor-site type
  inference; no explicit `Box[i64]()` syntax (v0.4). +8 tests.
- **M32 async I/O** (`asyncio` stdlib module + async-socket
  variants). Shape A — thread-backed Future façade. New `Future[T]`
  exposed as a TypeCtor (joining Channel/Atomic/Dict/List) with
  special-cased `.await()` dispatch — agent's design call to NOT
  use M31's user-defined-generic-class machinery (~25 LOC of
  TypeCtor wiring vs the full monomorphisation worklist).
  `asyncio.spawn` launches an OS thread; the thread fills a
  `FutureSlot`; `Future.await()` blocks on a Condvar. Demo:
  `examples/async_echo_server.spy` (~115 LOC) handling 3 concurrent
  clients. v0.4 swaps the internals for a real mio/polling event
  loop without changing the public surface. +9 tests.
- **M33 precise GC stack maps** (shadow-stack fallback). Replaces
  the M9 conservative `in_jit: AtomicUsize` pause that blocked
  collection during JIT'd execution. New `vm/src/stackmap_registry.rs`
  with thread-local shadow stack; the JIT spills register variables
  into a per-function Cranelift stack slot and pushes the slot
  window before every heap-allocating runtime helper; `Heap::collect`
  consults the windows for precise root enumeration. Removes the
  `in_jit` field and its bracket calls. Full Cranelift
  `enable_safepoints` integration deferred to v0.4 (requires
  walking JIT'd Rust frames + correlating PC offsets that
  `cranelift-jit 0.115` doesn't expose stably). +4 tests.

**M32 + M33 was the first parallel-v0.3-agent round** — two
agents in separate worktrees, both modifying `vm/src/interp.rs`
and `vm/src/lib.rs`. M32 added `SharedVm.futures`; M33 removed
`SharedVm.in_jit`. **Zero cherry-pick conflicts at integration** —
git's three-way merge handled the orthogonal hunks automatically.
Cleanest parallel-agent integration in project history.

- **M34 typed JsonValue tree** — the first stdlib classes. Sealed
  `JsonValue` base + 6 final subclasses (JNull, JBool, JInt,
  JFloat, JString, JList, JObject). Closes the M29 framework
  agent's #1 documented ergonomics gap: the framework's POST body
  parser hand-walks `json.parse_to_string` output for ~50 LOC where
  a typed JsonValue tree drops it to ~10 LOC of `match`. Per the
  brief's STOP CRITERIA the agent took the scope-down option:
  registered the 7 classes in the **prelude** (alongside Channel,
  Thread, io.File) rather than building proper
  `StdlibItemKind::Class` infrastructure for module-level class
  items. The legacy "prelude wins" branch in the import resolver
  makes `from json import JsonValue` work transparently; module-
  scoped class registration is a pure refactor deferred to v0.4.
  Three findings: (a) `JList` storing `List[JsonValue]` worked
  first-try under M11 + M31 — the class system is stable enough for
  recursive types-with-self-in-a-List; (b) GC root scanning was
  free via existing `GcKind::Class` + `GcKind::List` traversals
  — no bespoke code for the recursive tree; (c) helper-vs-
  constructor needs two NativeFn IDs per shape (different arg
  conventions, caught by JNull's zero-arg case). +13 tests.

After M34, the v0.3 menu reads: real Cranelift safepoint stack
maps (replaces M33 shadow stack); real mio event loop (replaces
M32 thread façade); module-level class registration (replaces M34
prelude registration); bounded generics + variance + HKT + explicit
type-arg syntax for M31; remaining stdlib classes (`re.Pattern`,
`sqlite3.Connection`, `Hasher`, `logging.Logger`); Phase 3d stdlib
(`traceback`, `enum`, `functools`, `uuid`, `secrets`); user-defined
exception subclasses; HTTP/2; WebSockets.

**Phase I: from stdlib classes to a Pandas-shaped data package
(M35–M39, 2026-05-21 / 22).** The five milestones after M34 form
one coherent storyline: more stdlib classes via the M34 prelude
pattern (M35), the infrastructure refactor that resolves the
M34/M35 scope-down (M36), and a from-scratch Pandas-shaped data
package built on top of the new infrastructure across three
single-agent ~2,500-LOC milestones (M37 + M38 + M39). After M39
the language ships with a usable Pandas-shape `tabular` module
covering the common-80% of pandas workflows.

- **M35 four stdlib classes via three parallel agents.** P4-A
  `re.Pattern` (compiled regex with `compile-once-reuse` semantics,
  NativeFn IDs 790-799), P4-B `sqlite3.Connection` + `Cursor`
  (typed wrappers over the M23 P3a-D flat surface, IDs 800-819),
  P4-C `hashlib.Hasher` (streaming digests with `update` /
  `hexdigest` / `copy` / `reset` / `name`, IDs 820-829). All three
  worktree-isolated, distinctive `p4a_` / `p4b_` / `p4c_` variable
  prefixes, disjoint NativeFn ranges. Each committed cleanly per
  Lesson 1; integration was three sequential `git apply --3way`s
  against the pre-M35 base with the now-standard manual brace fix
  between adjacent agents' match arms. **Tests 677 → 723 (+33).**
  After M35 the prelude held 17 stdlib classes (6 base + 11 v0.3)
  — the next class family would have made it crowded.

- **M36 `StdlibItemKind::Class` infrastructure refactor.** Added a
  `Class { class_id: ClassId }` payload variant to the resolver's
  item-kind enum, chosen over an `Option<ClassId>` field to avoid
  touching the 345 existing `StdlibItem { … }` construction sites.
  Then published all 11 M34/M35 classes through their home stdlib
  modules (`json` / `re` / `sqlite3` / `hashlib`) as proper Class
  items. Extended the `from MOD import X as Y` resolver branch to
  bind aliased imports to a fresh `SymbolKind::Class` pointing at
  the same `ClassId`. **Honest scope-down**: the prelude bindings
  were RETAINED for back-compat because every M34/M35 integration
  test reaches class names by bare lookup after just `import json` /
  `import re` / etc. (no `from … import` form), and a hard prelude
  removal would have regressed all 39 of them. Phase D annotated
  the legacy "prelude wins" resolver branch with the explicit list
  of 11 classes it remains load-bearing for; a future agent
  migrating the tests to explicit imports can delete the branch in
  one go. **Tests unchanged at 723** (pure refactor). The unlock:
  the next stdlib package could register classes module-scoped from
  the start, without prelude pressure.

- **M37 `tabular` core (Phase 1+2 of the Pandas plan).** Single
  agent, 5 phase commits (A-E), ~2,800 LOC across 9 files — the
  largest single-agent milestone to date. **First stdlib package
  using the post-M36 canonical class-registration path**, no
  prelude additions: end-to-end validation of M36. 6 new classes
  registered via `StdlibItemKind::Class`: sealed `Column` base +
  5 final subclasses (`ColumnI64` / `ColumnF64` / `ColumnStr` /
  `ColumnBool` / `ColumnDateTime`) + `DataFrame`. NA semantics:
  per-column `nulls: List[bool]` parallel to `values: List[T]` —
  uniform across dtypes, no NaN sentinel games. Phase A core
  types + factories + `df.show(n)` ASCII table; Phase B I/O
  (`read_csv` / `write_csv` / `from_sql` — the SQL path reuses
  the M35 typed `Cursor` directly); Phase C per-column comparisons
  (`eq` / `gt` / `lt` on i64+f64; `eq` / `contains` on str;
  `eq` on bool; `eq` / `gt` / `lt` on datetime) producing
  null-aware `ColumnBool` masks + `df.filter` / `select` / `drop` /
  `head` / `tail`; Phase D stable `df.sort_by(col, ascending)`
  with nulls-at-end; Phase E 21 tests + a 130-LOC demo. Module
  name `tabular` (not `pandas`, to avoid `import pandas` confusion
  — real pandas can't import architecturally). STOP CRITERIA in
  Phase C cut `between` / `ne` / `ge` / `le` / `starts_with` (10
  NativeFn slots saved; M38 picks up). Three findings: (a)
  `(*hdr).vtable` not `.ty` — ObjectHeader field rename caught
  the agent in early Phase A; (b) no `get_column(name) -> Column?`
  because the sealed-class return type can't be cleanly chosen at
  NativeFn time (M38 fixes via typed accessors); (c) NO bare-name
  fallback for tabular classes — confirms the M36 refactor's
  promise. **Tests 723 → 744 (+21).**

- **M38 `tabular` round-out — aggregations + group-by (Phase 3
  of the Pandas plan).** Single agent, 5 phase commits, ~2,530
  LOC, **zero STOP CRITERIA cuts**. Phase A: typed
  `df.get_column_i64` / `f64` / `str` / `bool` / `datetime`
  accessors (resolves the M37 sealed-class-return finding —
  each is its own NativeFn because the return type is monomorphic)
  + restored M37-cut comparison ops + `df.rename`. Phase B:
  per-column aggregations — `sum` / `mean` / `min` / `max` /
  `count` / `std` / `var` / `median` on numeric (sample n-1
  std/var); `min` / `max` / `count` on str + datetime; `count`
  on bool. Null-skipping throughout. Phase C: `df.describe() ->
  DataFrame` (count/mean/std/min/50%/max for numeric, count for
  non-numeric); `Column.fill_null(v)` per subclass (5 methods);
  `tabular.from_dict(d: Dict[str, Column])`. Phase D: new
  `GroupedDataFrame` class registered via the M36 canonical path
  (second stdlib class on the canonical path after M37's 6
  classes); `df.group_by(cols) -> GroupedDataFrame`; shortcuts
  `size` / `keys` / `sum` / `mean` / `min` / `max` / `count`;
  custom `agg(specs: List[Tuple[str, str]])`. Hash-based with
  `\x01`-joined multi-column keys. Phase E: 25 new tests + a
  groupby demo + LANGUAGE_GUIDE updates. Four findings: (a) M5's
  `Dict` has no insertion order — `tabular.from_dict` lex-sorts
  column names; (b) NaN propagation on f64 aggregations matches
  `numpy.sum` (NaN propagates) NOT `numpy.nansum` — nulls ARE
  skipped but NaN values are NOT; (c) null-keyed group bucket
  follows pandas's `dropna=False` mode; (d) **Edit-tool worktree
  leak (recurring)** — the agent's Edit/Write tool writes
  occasionally land in the project-root copy of files instead of
  the worktree. First seen in M37, recurred in M38; orchestrator
  workaround is `git checkout --` main + `git merge --ff-only`
  the worktree HEAD. **Tests 744 → 769 (+25).**

- **M39 `tabular` reshape (Phase 4 of the Pandas plan).** Single
  agent, 4 phase commits, ~2,430 LOC, **zero STOP CRITERIA cuts**.
  After M39 the `tabular` module covers the common-80% of pandas
  workflows. Phase A: 5 typed `df.unique_*` accessors per dtype +
  `df.value_counts(col)` (2-col DataFrame sorted by count desc) +
  module-level `tabular.concat_rows(dfs)` (vertical, schema-strict)
  + `tabular.concat_cols(dfs)` (horizontal, row-count-strict +
  unique-column-name-strict). Phase B: `df.merge(other, on, how)`
  — hash-join with all 4 modes (`inner` / `left` / `right` /
  `outer`) reusing M38's `\x01`-joined per-cell key encoding. Null
  cells in `on` columns never match (pandas/SQL `null != null`);
  merged `on` columns inherit rhs values on right-only outer rows
  (matches pandas's "merged key column" behavior). Phase C:
  `df.pivot(index, columns, values)` (long→wide; raises on
  duplicate (index, columns) pairs; missing → null) + `df.melt(
  id_vars, value_vars)` (wide→long; all `value_vars` must share
  a dtype). Phase D: 25 new tests + a reshape demo +
  LANGUAGE_GUIDE.md §11.20 (null-join-keys gotcha) + §11.21
  (duplicate-pivot-key gotcha). Five findings: (a) f64 `unique`
  keys on `to_bits()` — `HashSet<f64>` doesn't compile
  (`f64: !Hash`); bit-pattern keying distinguishes ±0.0 and lets
  multiple NaN payloads be distinct; (b) `m39_join_key` returns
  `None` for any-null-cell rows (vs M38's null-bucketing) —
  short-circuit cleaner for merge's `null != null` semantics; (c)
  merged `on` columns inherit rhs values on right-only outer rows
  via the `rhs_fallback_idx` pluck path; (d) melt's per-dtype
  machinery is bulky — pre-read `value_vars` into Vec<>s up front
  to avoid virtual-call-per-cell overhead; (e) **Edit-tool
  worktree leak confirmed-recurring across 3 consecutive
  milestones** (M37 + M38 + M39, ~5 times in M39 alone) — now
  a methodology note in HANDOFF.md. **Tests 769 → 794 (+25).**

After M39, the v0.3+ menu narrows. The high-leverage open items
are: real Cranelift safepoint stack maps; real mio event loop;
the M36 honest-debt cleanup (migrate the 39 M34/M35 tests to
explicit imports, then delete the legacy "prelude wins" resolver
branch); the Edit-tool worktree leak investigation; `tabular`
Phase 5 (DatetimeIndex / rolling / resample / asof_merge /
cumulative / dropna / fillna / iloc range); and `tabular` Phase 6
(desktop UI — webview-served or Tauri/wry hybrid). Bounded
generics + user-defined exception subclasses + HTTP/2 + WebSockets
remain on the v0.4 list.

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
M27 621 tests     (+35 across Phase 3c — 9 modules)
M29 639 tests     (+18 across networking stack + web framework)
M30 656 tests     (+17 across last two bug closures — v0.2.0 freeze point)
M34 690 tests     (+34 across generic classes / async / GC / JsonValue)
M35 723 tests     (+33 across re.Pattern / sqlite3 typed / hashlib streaming)
M36 723 tests     (unchanged — pure StdlibItemKind::Class refactor)
M37 744 tests     (+21 across tabular core + IO + filter + sort)
M38 769 tests     (+25 across tabular aggregations + group-by)
M39 794 tests     (+25 across tabular reshape — merge/pivot/melt/concat)
```

The jump at M10 is the inflection point. M0–M9 added 134 tests via
linear feature growth; M10 added 39 in one milestone of stress
testing. The pattern continued: real programs forced regression
coverage faster than feature work did. By M29, the test suite is
~4.8× the size of M9, and the bug catalogue is ~3× — both driven
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
M27          19,500   14,000      720    11,500   ← Phase 3c stdlib
M29.5        19,850   15,350      720    13,400   ← networking + web framework
M34          20,760   17,800      725    13,290   ← v0.3 begins (generics/async/GC/JsonValue)
M35          21,100   18,450      725    13,587   ← +3 stdlib class families
M37          21,881   19,620      725    13,717   ← tabular Phase 1+2 — ~2,800 LOC milestone
M38          22,152   20,866      725    13,827   ← tabular Phase 3 — ~2,530 LOC milestone
M39          22,219   21,967      725    13,977   ← tabular Phase 4 — ~2,430 LOC milestone
```

The two largest deltas in the VM are the M8/M9 JIT work (+1,400 LOC
across jit.rs + jit_runtime.rs) and the M19–M27 stdlib batch (+~8,000
LOC across builtins.rs). The compiler's largest deltas are the M3 IR
+ codegen (+~3,000 LOC) and the M17 generics work (+~600 LOC in ir.rs
+ typecheck.rs). The shared crate is intentionally tiny: it holds
only the cross-crate contract (opcodes, file format, type tags,
NativeFn IDs).

The M37+M38+M39 `tabular` build-out adds another ~6,500 LOC to the
VM's `builtins.rs` alone (decode-then-allocate handler code across
6 dtypes × ~25 methods + dispatch), with a relatively small
proportional impact on the compiler (~700 LOC) — class registration
+ method-dispatch table, no new IR opcodes. This is the canonical
shape of a "stdlib package" milestone post-M36: most LOC lands in
the VM's handler code; the compiler-side surface is a thin
class-and-method registration block. The pattern is what allowed
three consecutive ~2,500-LOC milestones to ship clean in one
single-agent budget each.

The Examples-LOC trajectory has its own story: from M22 to M29.5 it
roughly doubled (9,218 → 13,400 LOC), but ~1,000 lines of that growth
is in a single program — `examples/webserver/todo_app.spy` (2,443
LOC after M29.5's round-out), the largest single .spy file in the
project and the most direct test of the language's ability to host
real software.

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

### 4.4 The M9 → M29 plateau

Twenty milestones since the JIT shipped. The benchmark numbers have
been essentially flat:

| Snapshot | fib(30) | W/T/L vs CPython |
|---|---:|---|
| M9 | 13.5 ms | 16/0/0 |
| M10 | 15.8 ms | 16/0/0 |
| M11 | 13.1 ms | 16/0/0 |
| M22 | 15.7 ms | 16/0/0 |
| M25 | 13.1 ms | 16/0/0 |
| M29 | 13.1 ms | 16/0/0 |
| M34 | 13.1 ms | 16/0/0 |

Cross-snapshot variance is ~10–20%, which is below the noise floor
of best-of-3 timing on a Windows workstation. **The JIT-emitted
hot-loop code has not been touched since M9 full-coverage landed.**
Every milestone since has either added correctness (M10–M12), added
language features (M13–M17), added stdlib (M19–M23, M27, M28, M28.5),
refactored non-perf-critical infrastructure (M25), or added
user-code stress tests (M24, M29, M29.5). The benchmark cells don't
exercise any of that surface, so they don't measure it.

This is a feature, not an accident. The design decisions in §2.3 are
structured to keep the JIT-emitted code stable as the language
grows. New language features (exceptions, generics, isinstance,
match) all introduce code paths that fall back to the interpreter
via the per-function JIT opt-in. The fixpoint disable then ensures
the bench's hot loops still get JIT'd (they don't touch any of those
features). The plateau is the architectural payoff for that
discipline.

### 4.5 The M26 extended suite — 30 cells across two regimes

M26 (2026-05-19) added 10 new benchmark programs (5 pure-compute,
5 stdlib-using) at 3 sizes each — 30 additional cells beyond the
canonical 16. The headline: **28 wins, 2 ties, 0 losses**. Full
results in [`bench/EXTENDED_REPORT.md`](bench/EXTENDED_REPORT.md);
two empirical findings worth recording in the thesis.

**Finding 1: the `btree` row narrows monotonically as allocation
pressure grows.** Three sizes:

| n | StrictPy | CPython | Ratio |
|---:|---:|---:|---:|
| 1k | 16.6 ms | 71.8 ms | 0.23× |
| 5k | 51.2 ms | 72.4 ms | 0.71× |
| 10k | 96.8 ms | 85.4 ms | 1.13× |

At ~10k recursive `BNode` insertions, StrictPy's `rt_alloc` + the
conservative-GC `in_jit` pause overhead overtakes the JIT win. This
is exactly the workload precise stack maps + a moving GC would fix.
The single non-win in the entire extended suite is structurally
predictable from the design decisions in §2.3 — and it is the
empirical justification for prioritising the precise-stack-map work
in v0.3.

**Finding 2: the stdlib comparison was expected to land near 1×,
but instead favours StrictPy across every cell.** Both sides do
their actual work in C/Rust (StrictPy: `serde_json`, `regex`,
`sha2`, `rusqlite`; CPython: `_json.c`, `_sre.c`, `_sha256.c`,
`_sqlite3.c`). The expected ratio was therefore ~1×. The measured
ratios are 0.14×–0.91×. The cause is process startup overhead:
Python pays ~50–70 ms on every cold launch (interpreter + import
of the stdlib module), StrictPy pays ~5–15 ms. The narrowing-with-
size pattern is visible on every stdlib row — e.g. CSV parse 0.20×
→ 0.91× as input grows from small to large. The asymptote IS ~1×,
the startup tax just dominates short workloads. **This is honest
data, not a measurement bug**; the M26 report documents it
explicitly.

### 4.6 Hosting real software — the M29 framework

The M29 web framework (§3.3 Phase G) is the most realistic
performance test in the project. Best-effort numbers, loopback only,
on a Windows 11 workstation:

| Endpoint | HTTP req/s | HTTPS req/s |
|---|---:|---:|
| `/health` (no I/O) | ~2,200 | ~800 |
| `GET /api/todos` (1 SQLite query) | ~1,500 | ~700 |
| `POST /api/todos` (1 SQLite insert) | ~1,100 | ~600 |

**Within 2× of Flask+gunicorn** for an equivalent workload — without
async I/O, without connection pooling, without a JIT warm-up loop,
on a 5-day-old language. The remaining ~2× gap is the async event
loop (v0.3). For a thread-per-connection synchronous model written
in user code on a typed-bytecode VM, the perf is competitive with
production Python web stacks.

### 4.7 Why this comparison is what it is

The headline "StrictPy beats CPython by 4–17×" claim is narrow in
three ways worth stating explicitly:

1. **The canonical suite is 4 programs; the extended suite adds 10
   more.** Real-world Python workloads include web frameworks
   (now partially represented by M29), scientific computing, ML
   training, and data-engineering pipelines. The wins generalise
   to "tight numeric loops, recursive small-int arithmetic,
   integer-keyed list mutation, stdlib calls dominated by startup";
   they do not generalise to "everything CPython does." Allocation-
   heavy workloads (M26 `btree` at large n) erode the JIT win.
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
  M23 P3a-D and all four M24 agents and 2 of 5 M27 agents, the agent
  finished the substantive work but exhausted the budget while
  writing the long report — `git commit` never happened. Orchestrator
  committed each worktree on the agent's behalf. M25+ pattern note:
  briefs initially said "commit EARLY, before the long report" —
  which kept failing. See §5.5.1 below for the resolution that
  actually worked.
- **Git three-way merge mis-aligning parallel handlers.** During the
  M23 P3a-D cherry-pick, git aligned `sqlite3.column_names` with
  `pathlib.read_lines` at a shared `let sp = alloc_string(...) as
  u64;` line. The merge result semantically replaced one handler's
  tail with the other's. Recovery: reconstruct from worktree
  history. Future worktree rounds should use distinct loop-variable
  names or distinct trailing comment markers to break the alignment
  heuristic. Implemented in M28+: agent briefs explicitly require
  per-agent variable-name prefixes (`p3b_a_`, `p3b_b_`, `p3b_d_`,
  etc.), which has worked cleanly across 6 subsequent agents.
- **Diff-against-current-main instead of against pre-round base** —
  M28 P3b-B integration disaster. After cherry-picking P3b-A onto
  main, the orchestrator generated P3b-B's diff via
  `git diff main..worktree`. That diff was computed against
  current-main (which had P3b-A) — so it contained REVERSE-DELETIONS
  of P3b-A's contributions. The first apply+commit deleted 1,806
  lines of already-landed work. Caught by inspecting `--stat`
  (deletion counts ≫ insertion counts is the smoke alarm). Recovery:
  `git reset --hard HEAD~1`, regenerate as
  `git diff <pre-round-base>..worktree`, re-apply. **Pattern lesson**:
  when sequentially cherry-picking parallel worktrees, always diff
  against the common ancestor, not against current-main.

### 5.5.1 The Lesson 1 escalation — explicit thresholds work

The "commit EARLY" briefing language failed in **7+ agents** across
M23, M24, and M27 — a sustained pattern that qualitative urgency was
not addressing. The M28 brief rewrote the section with explicit
numerical thresholds:

> **Your FIRST `git commit` must land before you have used 60% of
> your estimated time budget.** If you're approaching that mark and
> tests aren't passing yet, COMMIT THE WORK-IN-PROGRESS ANYWAY.
> You can amend the commit later. The orchestrator strongly prefers
> a half-finished committed state over a complete uncommitted state.
>
> Suggested checkpoint discipline: 20% scaffolding → COMMIT; 40%
> NativeFns wired → COMMIT (amend); 60% tests passing → COMMIT
> (amend); 80% report drafted → COMMIT (amend).

The result: **3 of 3 M28 agents** committed cleanly. Then **1 of 1
M28.5**. Then **1 of 1 M29**. Then **1 of 1 M29.5**. Then M30 (2
agents) + M31 + M32 + M33 + M34, all clean. Then M35 (3 parallel
agents). Then M36 (the infrastructure refactor). Then M37 + M38 + M39
— three consecutive ~2,500-LOC single-agent milestones, each
delivering 4-5 phase commits clean, **zero STOP CRITERIA cuts in M38
and M39, only Phase C ops in M37**. **21 agents in a row over 12
milestones followed the discipline cleanly**, including the three
largest single-agent milestones in the project's history.

The brief language was the only intervention that changed between
M27 and M28. The numerical threshold + named checkpoints replaced
"commit early" with a measurable expectation an agent can self-assess
against mid-task. **The pattern generalises**: in agent briefs,
numerical thresholds beat qualitative urgency for behaviours
the agent is expected to perform under time pressure. The lesson is
methodologically reusable beyond StrictPy.

**Validation at the upper end of agent scope**: M37 / M38 / M39 each
shipped ~2,500 LOC across 4-5 phase commits in a single agent budget.
The Lesson 1 streak held through all three. The implication for
single-agent scope ceiling — at least within StrictPy's idiom of
phase-decomposable stdlib packages — is now empirically tested up to
~2,800 LOC (M37) without a streak break. The orchestrator's per-phase
STOP CRITERIA discipline (drop the lowest-priority feature subset
rather than the milestone) is the cushion that prevents budget
overruns from breaking the streak. M37 used the cushion (cut some
Phase C ops); M38 and M39 didn't need to.

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

### 5.7 Three methodology results from the long tail (M40–M56)

The v0.3 marathon — fourteen tabular milestones, a desktop-UI track,
and a six-milestone games stack — produced three methodology findings
that the first 39 milestones did not.

**A taxonomy of milestone shapes predicts commit cadence.** Lesson 1
("first commit before 60% of budget") works, but *when* a milestone
first goes green depends on its structural shape, and the brief should
name that shape. Four classes emerged and were validated repeatedly:

- **disjoint-handler** — independent handler bodies; clean per-phase
  commits, first at ~20% (M42/M43/M45/M46/M48/M49).
- **shared-infra** — a struct-layout or shared-helper change every
  later phase depends on; combined first commit at ~30–50% (M41/M44 —
  e.g. the DataFrame payload growing 24→40→56 bytes for the index).
- **cross-dispatch** — a new sealed-class subclass forces every
  dispatch file (resolver / ir / native / builtins) to compile
  together; first green at ~50–75% (M47's `ColumnCategorical`).
- **net-new-feature** — a self-contained subsystem whose pieces only
  go green together; ~50–70% (M50a's HTTP server; the games).

M47 is the instructive case: its brief named the wrong shape
(disjoint-handler), the first commit landed at 70%, and that was a
brief-side mis-classification, not agent drift. Naming the shape up
front sets the right expectation and keeps the streak honest.

**Parallel-work collisions are a distinct failure mode from merge
conflicts.** In M51 a delegated sub-agent and an independent
contributor built the *same* feature — a chainable rolling-window class
— in parallel, both diverging from the same commit, and both landed a
complete, tested implementation. The push was rejected on a
non-fast-forward. The correct resolution was not to force-push one good
implementation over the other but to keep the already-published version
canonical and layer on only the non-overlapping work, re-numbering the
clashing native-function IDs. The lesson: when you delegate work that
someone else might also do, `git fetch` before assuming your local
branch is authoritative, and reconcile rather than overwrite.

**A sandboxed delegate inverts the verification contract.** Late
sub-agents ran in isolated worktrees whose sandbox denied every
`cargo`/`python` invocation. The agent wrote ~1,900 lines of Rust blind
(from close reading of the codebase) and it compiled on the first
orchestrator build — but it could not self-gate on a green build the
way Lesson 1 assumes. The pattern works, but the orchestrator, not the
agent, owns building, testing, benchmarking, and integration. This is a
strictly stronger demand on the orchestrator than the earlier
worktree-isolated agents, which could at least run their own tests.

---

## 6. Findings

### 6.1 The bug catalogue

[`docs/thesis/bugs/catalog.md`](docs/thesis/bugs/catalog.md) records
every distinct bug discovered across M0–M29.5. Summary by category:

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
| Stdlib semantics (network / concurrency) | 1 | 0 | 1 |
| **Total** | **35** | **33** | **2** |

Two open bugs:
- **BUG-028** — the lexer doesn't continue lines across a trailing
  `+` operator. Mechanically simple to fix; cost of the workaround
  (parentheses or accumulator variables) has been small enough that
  deferral never blocked anything.
- **BUG-040** (found in M29.5 framework round-out) —
  `socket.close_listener` does not unblock an in-flight
  `socket.accept`. The accept handler `Arc::clone`s the listener
  and drops the slot-table mutex before calling the blocking
  syscall, so closing the slot from another thread doesn't drop
  the underlying FD. User-code workaround (self-connect to wake
  the blocked accept) is ~15 LOC and is what the M29.5 framework's
  graceful-shutdown path uses; the proper stdlib-side fix
  (`Mutex<Option<TcpListener>>` slot or new
  `socket.shutdown_listener`) is v0.3.

Eight patterns from the catalogue generalise beyond StrictPy. Each
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
| M27 (round 6: Phase 3c stdlib stress) | 9 modules | ~3,500 | 1 (`bzip2` write-side hang on bad input) |
| **M29 (round 7: web framework)** | 1 | ~1,446 | **0** |
| M29.5 (round 7 follow-up: framework round-out) | 1 (~1,000 LOC additions) | ~1,000 | 1 (BUG-040 `close_listener`) |

The ROI curve flattens — by M18 the system is in steady state — but
**most rounds still found at least one bug**. The bugs found in late
rounds are not architectural; they are placeholder lowerings (BUG-037
`??`, BUG-039 `in`) or stdlib-semantic gaps (BUG-040 `close_listener`),
latent since the relevant primitive first shipped. They would not
have been found by feature-development testing.

**The M29 round is the first stress round in project history with
zero bug finds.** The result is structurally interpretable: M29
stress-tested the M28 + M28.5 networking surface, which was unusually
clean coming in — the agents had strong commit discipline (see §5.5
on the Lesson 1 escalation), and the integration produced fewer
manual fixes than M27. Tighter incoming surface, fewer latent issues
to surface later. M29.5 then **did** surface a bug (BUG-040), but on
a control-plane operation (graceful shutdown) that M29 hadn't
exercised — the same "real programs use APIs in combinations unit
tests don't" mechanism that the ROI table captures.

The mechanism: real programs use operators in combinations and
contexts that unit tests don't. A unit test for `dict.has(k)` checks
that it returns the right value. A real program — `event_log.spy`'s
histogram — uses `bucket in seen` as a guard for whether to
initialise a counter. The unit test never tests the `in` operator
because `in` has its own (broken) lowering; the real program does
because that's the natural Python idiom. The M29.5 graceful-shutdown
case is structurally identical: nothing in the M28 unit tests
exercised "close the listener from one thread while another is
blocked in accept" — but a real shutdown path does.

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

### 7.3 No open bugs (v0.2 + v0.3 state)

After M30 closed BUG-028 (lexer line continuation) and BUG-040
(`socket.close_listener` doesn't unblock blocked `accept`), the
project reached its first **zero-open-bugs** state. **35 bugs found,
35 fixed, 0 deferred.** This is the cleanest state in project history
and was the freeze point for the **v0.2.0 release tag** (2026-05-21).

Subsequent v0.3 work (M31 generic classes, M32 async I/O, M33
precise GC, M34 stdlib classes, M35 four more class families, M36
class-registration infrastructure refactor, M37+M38+M39 the
`tabular` Pandas-shaped data package across three single-agent
milestones) has found no new bugs in either the M0–M30 surface or
the new v0.3 features themselves. The bug catalogue summary table
is now:

| Category | Found | Fixed | Deferred |
|---|---:|---:|---:|
| (all categories) | **35** | **35** | **0** |

This is **794 tests passing, 0 failing, 0 open bugs, 9 milestones
after the v0.2.0 freeze**.

### 7.4 v0.2 feature gaps

The following were deferred from v0.1; some have shipped in v0.3
and the remainder is the v0.4 list:

- **Generic classes** (`class Box[T]:`). ~~Deferred.~~ **Shipped in
  M31** — `Box[T]` / `Pair[K, V]` / `Stack[T]` work via per-
  instantiation type_id + method bodies. The M17 worklist
  infrastructure extended to classes (new IR Pass 2.7 + 3.6).
  Constructor-site type inference; explicit `Box[i64]()` syntax
  remains v0.4.
- **User-defined exception subclasses.** Still deferred (v0.4).
  v0.1+ ships 10 built-in exception names; user-defined
  `class MyError(Exception):` is parsed but the resolver rejects it.
- **`with` → try/finally desugaring.** Still deferred. Workaround:
  explicit `try: with open(...) as f: ... except IOError:`.
- **Bounded generics** (`T: Comparable`). Still deferred (v0.4 work).
  Generics re-typecheck under substitution per instantiation, which
  is approximately correct but allows operations the source bound
  would have rejected.
- **Stdlib classes** — typed `JsonValue` tree, `re.Pattern`,
  `sqlite3.Connection` + `Cursor`, `hashlib.Hasher`. ~~Blocked on
  generic classes.~~ **All shipped in M34 + M35**, with M36 publishing
  them through proper module-scoped registration. The M29 framework
  rewrite using JsonValue / Pattern / Connection is a queued
  measurement task (HANDOFF.md priority list); estimated ~30-35%
  LOC reduction.
- **Async I/O / event loop.** **Shape A shipped in M32** (thread-
  backed Future façade, `asyncio.run` / `spawn` / `await` / `gather`
  + `socket.async_*`). The public surface is right; the internals
  spawn one OS thread per task. Closing the M29 framework's ~2× gap
  to Flask+gunicorn needs a real `mio` event loop — that's the v0.4
  work, no public-API impact.
- **HTTP/2 and WebSockets.** Still deferred to v0.4.
- **Production-grade password hashing** (bcrypt / argon2). Still
  deferred. v0.2+ ships `hashlib.sha256` — fine for content hashing,
  inappropriate for auth password storage. M35's `Hasher` streaming
  surface doesn't change the underlying primitives.
- **Phase 3d stdlib** — `traceback`, `enum`, `functools`, `uuid`,
  `secrets`. Still deferred. Each is small; the M27 parallel-worktree
  pattern handles them in 1-2 milestones.
- **NumPy / pandas integration.** Real NumPy + pandas still can't
  import (libpython dependency) — architectural; see
  [`docs/thesis/design_decisions/why_no_numpy_pandas.md`](docs/thesis/design_decisions/why_no_numpy_pandas.md).
  **The M37 + M38 + M39 `tabular` package is the native-reimplementation
  path** for the data-package shape: a Pandas-shaped DataFrame
  library, written in ~7,800 LOC of native Rust handlers,
  feature-comparable to a v0.0.1 pandas (typed columns + null masks
  + IO + filter/sort + aggregations + group-by + merge with all four
  join modes + pivot + melt). What's still missing in the data-package
  space: `DatetimeIndex`, rolling / resample / asof_merge / cumulative
  ops, `dropna` / `fillna` at frame scope, `iloc` range slicing
  (the M40 punch list); BLAS-backed numeric matrix ops; and a
  desktop UI layer (the M37-design Phase 6 — webview-served or
  Tauri/wry hybrid).

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

Two v0.3-era defects are characterized but unfixed. (1) **`tabular`
peak memory runs 4–5× pandas** at large sizes (M48b root-caused it:
the per-column null mask is a `List[bool]` at 8 bytes per boolean, plus
the VM's uniform-8-byte list slots vs NumPy's contiguous typed buffers,
plus un-interned strings; see [`bench/TABULAR_MEMORY_REPORT_M48b.md`](bench/TABULAR_MEMORY_REPORT_M48b.md)).
The fix (pack the null mask; eventually a packed-column representation)
is scoped but deferred to v0.5. (2) The M55/M56 **games have a Windows
input/frame-timing quirk** that took several patches and is not fully
resolved — the kind of latent issue that only surfaces when a human
actually plays the game at 60 FPS, and a reminder that "the compile-only
test passes" is not the same as "it plays correctly."

---

## 8. Conclusion

### 8.1 What the result changes

The headline empirical claim — that a statically typed Python
dialect with mandatory annotations can beat CPython 3.12 by 4–17×
on tight numeric workloads, with ~42K lines of Rust and ~1,400 lines
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
suite, a 103-program example corpus, a 35-entry bug catalogue, a
39-milestone timeline. The architectural claims it supports are
stronger: the five design decisions in §2.3 are each independently
justified by load-bearing milestones, and each is documented with
the alternative that was considered and rejected.

What the archive does NOT establish:

- That this development methodology is generally faster than
  human-only development. Single project, no baseline.
- That static typing is more productive than dynamic typing. Single
  developer, no controlled comparison.
- That StrictPy is faster than CPython on real workloads. 4 micro-
  benchmarks (canonical) + 10 (extended) + the M29 framework.
- That AI-orchestrated systems work scales to projects 10× larger.
  This was a ~42K-LOC project; the agent-task complexity at 400K
  LOC is unstudied. However, M37 + M38 + M39 — three consecutive
  ~2,500-LOC single-agent milestones, all clean — do establish that
  the per-agent scope ceiling is at least ~2,800 LOC for
  phase-decomposable stdlib work within this codebase's idiom.

### 8.4 What the next pass would do

After M39 — the v0.3+ menu narrows. The highest-leverage open items:

1. **Real Cranelift safepoint stack maps** — replaces the M33
   shadow-stack fallback. Requires walking JIT'd Rust frames and
   correlating PC offsets against `MachBufferFinalized` ranges that
   `cranelift-jit 0.115` doesn't stably expose. Either wait for the
   upstream API to stabilise or land it via a focused agent against
   the trunk.
2. **Real `mio` event loop** — replaces the M32 thread-backed
   Future façade. Public surface (`asyncio.spawn` / `await` /
   `gather`) unchanged; the internals swap from one OS thread per
   task to a single-threaded event loop with state-machine
   coroutines. Closes the M29 framework's ~2× gap to Flask+gunicorn.
3. **M36 honest-debt cleanup** — migrate the 39 M34/M35 integration
   tests to use explicit `from json import JsonValue` etc., then
   delete the legacy "prelude wins" resolver branch. Mechanical
   migration; the M36 Phase D comment lists exactly which classes
   the branch still serves.
4. **Edit-tool worktree leak investigation** — confirmed-recurring
   across M37 + M38 + M39 (3 consecutive milestones). The orchestrator-
   side workaround (`git checkout --` main + `git merge --ff-only`
   the worktree HEAD) is reliable but should not be permanent. Single
   no-coding session to diagnose the harness's git-worktree path
   resolution.
5. **`tabular` Phase 5** — DatetimeIndex / rolling / resample /
   asof_merge / cumulative ops (`cumsum` / `cumprod` / `cummax` /
   `cummin`) / `dropna` / `fillna` at frame scope / `iloc` range
   slicing. Estimated ~1,500-2,000 LOC; the M37/M38/M39 template
   applies directly.
6. **`tabular` Phase 6 — desktop UI**. Per the original M37 design
   discussion: webview-served (reuse the M29 web framework + browser
   tab) or Tauri/wry hybrid (native window wrapping a JS frontend).
   The compute backend is the same regardless; the JS frontend
   (AG Grid or Perspective.js) drives filter/pivot UI.
7. **Bounded generics + variance + explicit type-arg syntax** —
   extends M31. The `Box[i64]()` explicit form would let
   `asyncio.spawn[T]` work generically.
8. **User-defined exception subclasses** — parser already accepts
   `class MyError(Exception):`; resolver currently rejects. Small fix.
9. **HTTP/2 + WebSockets** — separate v0.4 stdlib modules.
10. **A larger benchmark suite** — Python stdlib workloads,
    allocation-heavy programs, multi-threaded contention, long-running
    with GC pressure. The M26 extended suite is a start; the M29
    framework's HTTP throughput could be added as cells; the
    `tabular` package's group-by + merge throughput on million-row
    frames is a natural next benchmark cluster.

### 8.5 The minimum the archive promises

The project archive at [`docs/thesis/`](docs/thesis/) is fully
reproducible from a `git clone`:

```powershell
git clone https://github.com/amitgangrade/StrictPy
cd StrictPy
cargo build --release
cargo test --workspace --release    # 690 tests pass (M34); v0.2.0 freeze was 656
python bench/harness.py             # regenerates BENCH_REPORT.md
python bench/harness.py --extended  # 30-cell extended suite
spy examples/fib.spy                # 13.1 ms for fib(30)
spy examples/webserver/todo_app.spy --port 8080  # the M29 web framework
```

The CSV at
[`docs/thesis/stats/per_milestone.csv`](docs/thesis/stats/per_milestone.csv)
is the quantitative ground truth. The benchmark JSONs in
[`bench/history/`](bench/history/) are the timestamped performance
record. The 56+ agent reports in
[`docs/thesis/agent_reports/`](docs/thesis/agent_reports/) are the
methodology evidence. The 35-bug catalogue at
[`docs/thesis/bugs/catalog.md`](docs/thesis/bugs/catalog.md) is the
correctness record.

The minimum claim this archive makes — and supports with evidence
that can be audited line by line — is that a small team or single
developer using AI orchestration can, in 6 calendar days, build a
working compiler-VM-JIT toolchain for a statically typed Python
dialect that beats CPython on a small benchmark suite, **host a
real HTTP/1.1 + HTTPS web framework written in that language on top
of its own stdlib**, and **ship a from-scratch Pandas-shaped
DataFrame package covering the common-80% of pandas workflows**,
with a disciplined enough record that the bugs found, the design
choices locked, and the decisions deferred are all individually
inspectable in retrospect.

The webserver framework is the empirical anchor for the stronger
claim. It is not a toy: 2,443 lines of StrictPy, eight integration
tests including HTTP keep-alive, chunked transfer encoding, multipart
uploads, HTTPS via rcgen-generated self-signed certs, graceful
shutdown, and HTML error pages. It runs at ~2,200 req/s on `/health`
— within 2× of Flask+gunicorn. The language was ready to host it
by day 4 of the project. **That is the thesis claim** — not the
benchmark numbers, not the methodology patterns; those are
supporting evidence. The headline is: a statically typed Python
dialect, started from scratch on Monday, was hosting a working web
framework by Friday.

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

*Author:* Amit Gangrade. *Orchestration:* Claude Code (Claude Opus 4.7). *Period:* 2026-05-17 to 2026-05-21.
