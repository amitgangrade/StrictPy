# Project timeline

A milestone-by-milestone narrative. For each, what was attempted, what was
delivered, what was discovered. Quantitative metrics in
[stats/per_milestone.md](stats/per_milestone.md); bug details in
[bugs/catalog.md](bugs/catalog.md).

---

## M0 — Specification (2026-05-17)

**Scope**: write the language and VM specification.

**Delivered**: `STRICTPY_SPEC.md` (1,813 lines) covering grammar, type system,
static + dynamic semantics, memory model, opcode table, file format, GC,
threading, FFI, error model, and an implementation roadmap.

**Decisions locked**: mandatory typing, no `Any`, no monkeypatching, single
inheritance, generics monomorphized, GIL-free VM, mark-sweep GC for v1
(generational deferred to M6+), Cranelift JIT planned for "M7" (became M8 in
practice). Spec was the contract for every subsequent milestone.

**Cost insight**: writing the spec first, even though it took a full session,
turned out to be the highest-leverage move of the project. Every agent task
afterwards could be briefed by pointing at a spec section. Without it the
project would have lost weeks to design churn.

---

## M1 — Lexer + parser + pretty-printer

**Scope**: source → AST + round-trip.

**Delivered**: lexer (1,512 lines), parser (2,365 lines), pretty-printer
(1,665 lines). 55 unit tests. Round-trip integration test confirming
`pretty(parse(pretty(parse(src)))) == pretty(parse(src))` for all 7 example
programs.

**Approach**: three parallel agents (lexer, parser, pretty-printer) working
on disjoint files. The lexer agent designed `TokenKind` first; parser and
pretty-printer worked against the shared `Token` and `ast::*` types defined
earlier in the scaffolding phase.

**Surprises**:
- Pretty-printer's idempotence-on-second-application caught a bug in the
  parser's handling of `**`-associativity that unit tests had missed.
- F-string lexing turned out more complex than the spec implied (nested
  braces, `{{`/`}}` escaping). The agent picked sensible defaults and
  documented them as `// TODO(spec)`.

---

## M2 — Resolver + type checker

**Scope**: scoping + bidirectional type checking + a negative-conformance
test suite.

**Delivered**: resolver (1,325 lines), type checker (1,319 lines), 20-case
negative-conformance suite for forbidden constructs (no `Any`, no implicit
numeric coercion, no nested-fn capture writes, multiple inheritance
rejected, final-class subclassing rejected, etc.).

**Approach**: two parallel agents (resolver+typechecker bundled; conformance
tests parallel). Conformance agent wrote tests with `#[ignore]` since the
resolver/typechecker didn't exist yet; orchestrator un-ignored after they
landed.

**Surprises**:
- Stdlib gaps were larger than expected. The 7 examples needed `Channel`,
  `Thread`, `io.File`, `str.slice`, `str.char_at`, `Dict.items()`, plus
  numeric conversion functions — none of which were in spec §9.1. Decision:
  extend the prelude (marked `// stdlib:`) rather than gate the milestone.
- Match exhaustiveness deferred — type checker accepts non-exhaustive
  matches silently. Documented as M11 work.

---

## M3 — IR + bytecode emission

**Scope**: typed AST → SSA-ish IR → `.spyc` bytecode.

**Delivered**: IR module (1,280 lines), codegen (692 lines), bytecode writer
(386 lines), three simple optimization passes (constant fold, copy
propagation, dead code).

**Surprises**:
- Per-function monomorphization of generic classes (`List[i64]`,
  `List[f64]`, `Channel[i32]`) added more complexity than expected.
- The agent introduced `NativeFn` as a stable u32-indexed registry of
  built-in functions. This decision turned out to be load-bearing for M5,
  M7, and M8 — see [design_decisions/](design_decisions/).
- Lambdas were lowered to a placeholder `ClosureNew { fn_id: u32::MAX }` —
  intentional punt, marked for later. Bit us in M6.

---

## M4 — Interpreter + GC

**Scope**: VM that loads and runs `.spyc` bytecode.

**Delivered**: loader, interpreter (~1,600 lines initially), object model,
basic mark-sweep GC. `hello.spy` ran end-to-end and printed "Hello, StrictPy!".

**The first major surprise**: programs were "passing" tests vacuously.
- `fib.spy` infinite-looped (M3 didn't update loop-carried locals)
- `dot.spy` returned 0 (list literals not populated)
- `mandelbrot.spy` printed nothing (top-level `final` consts not lowered)
- `tree.spy` worked, `hello.spy` worked, but only superficially

The integration tests asserted `exit_code == 0` only — not output values.
**Lesson learned**: tests that don't assert on values aren't tests, they're
smoke alarms with no battery. This shaped every subsequent test brief.

---

## M3.5 — IR bug fix detour

**Scope**: fix the three M3 bugs M4 surfaced.

**Delivered**: real IR changes for loop-carried locals (`ReadLocal`/
`WriteLocal` slot-based representation instead of pure SSA), list literal
population, top-level const lowering.

**The cascade**: M3.5's local-slot refactor BROKE `tree.spy`. Tree had been
working before; now it segfaulted. The fix surfaced 3 *separate* root
causes in constructor + field-store handling — see M6.

**Net result**: fib, dot, mandelbrot fixed (each now prints real output);
tree regressed. Net +3 examples actually computing. Pattern that recurred
throughout: each fix surfaces the next layer of bugs.

---

## M5 — Native stdlib

**Scope**: implementations for the prelude additions M2 declared.

**Delivered**: math functions, file IO, channels (sync_channel-based),
dicts (string-keyed), range (eagerly materialized to List[i64]). Threads
explicitly deferred to M6.

**Approach**: agent ran in parallel with M3.5. Since the two touched
disjoint crates (M3.5 in `compiler/`, M5 in `vm/`), zero conflicts.

---

## M6 — Real threading + tree fix + lambda lifting

**Scope**: two parallel sub-milestones.

**M6-A (compiler)**: diagnose tree.spy regression. Found **3 separate root
causes**:
1. Duplicate `self` param overwriting slot 0 with Unit type
2. Eager devirtualization on `open` classes, skipping subclass overrides
3. `__init__` consuming vtable slot 0, shifting every virtual method

Each was independent. Tree.spy now runs and prints `tree sum = 15`.
Lambda lifting was already correctly implemented by M3.5 — only verified.

**M6-B (vm)**: real OS threading via `Arc<Module>` + per-thread
`Interpreter` instances + `SendableClosure` extracted from `ClosureRepr`.
Verified with `vm/tests/threading.rs` (8 concurrent workers on one channel).

**Surprise**: on Windows-x86_64, `CallConv::SystemV` is wrong for Rust
`extern "C"` — uses `WindowsFastcall`. This bit M8 too.

---

## M7 — Runtime-class method dispatch

**Scope**: fix the gap blocking producer + wordcount (`ch.send()` /
`f.read()` / `dict.get()` were lowering to `VirtualCall` instead of
`CallNative`).

**Delivered**: `is_native: bool` flag on `ClassLayout`. IR lowerer skips
vtable dispatch for native classes, falls through to existing NativeFn
resolution. Plus dict subscripts (`d[k] = v`), `with`-block desugaring.

**Three incidental bugs found while wiring this up** — load-bearing
correctness issues silently present since earlier milestones:
1. `not x` was emitting *bitwise* NOT — every `if not …:` was wrong
2. `none` was stored as bit pattern `0` — `if v is none:` matched zero
   integers and zero-byte pointers
3. `Thread(closure)` emitted generic `Alloc` returning a zeroed header —
   spawned threads always saw null

After M7, all 7 example programs ran end-to-end with verified real output.

---

## M8 — Cranelift AOT compilation

**Scope**: beat CPython.

**Delivered**: full Cranelift integration. Each `IRFunction` whose ops are
all supported gets compiled to native code at module-load time. Falls back
to interpreter per-function for unsupported ops. Unified ABI:
`unsafe extern "C" fn(*mut VmState, *const u64) -> u64`.

**Result**: fib(30) went from 931 ms → 14.6 ms. **64× speedup vs the
interpreter, 11× faster than CPython 3.12.** Mandelbrot also flipped to
beating CPython 4.6×. Tally went from 5W/3T/8L to 10W/2T/4L.

**Decompilation approach**: rather than changing the `.spyc` format, the VM
decompiles bytecode back into a typed op stream at load time. Keeps the
compiler crate unchanged.

**Limitations carried forward**: ArraySet, ListPush, Alloc, LoadField,
StoreField, VirtualCall, ClosureNew all fell back to interpreter →
fixpoint-disabled their callers → quicksort and dot stayed slow.

---

## M9 — Full JIT coverage

**Scope**: cover the heap-mutating + class ops the M8 JIT punted on.

**Delivered**: runtime helpers (`rt_list_push`, `rt_list_new`, `rt_array_new`,
`rt_alloc`, `rt_virtual_call`) called from JIT'd code. Inlined `ArraySet`,
`LoadField`, `StoreField`. GC safety: `in_jit: AtomicUsize` counter pauses
GC during JIT'd execution (compromise — fine for benchmarks, blocks long-
running programs from collecting).

**Result**: all 4 remaining CPython wins flipped. **StrictPy beats CPython
on every cell, 4-17×.** fib(30): 13.5 ms (12× faster). quicksort(100K):
18.6 ms vs CPython's 239 ms (13× faster). dot(1M): 54 ms vs 239 ms
(4× faster).

---

## M10 — Real-world stress test

**Scope**: write 6+ real programs to find the next batch of bugs.

**Delivered**:
- 6 new programs: Game of Life, Sudoku, JSON parser, Markov chain, KV store
  with WAL, Brainfuck. Plus the CSV aggregator from a preceding session.
- Stdlib additions: `for x in xs:` desugaring, `str.split(sep)`, `sorted()`/
  `xs.sort()`, `list.pop()`, char + dict typecheck fixes.
- Nullable-narrowing audit fixed 4 more silent codegen miscompiles in the
  same pattern as the CSV-aggregator bug.

**Bugs found**: 17 distinct issues across 5 parallel agents.
- 11 critical or medium severity → **fixed in same milestone**
- 6 architectural → documented in `BUGS_KNOWN.md` for M11

**The single most consequential finding**: `is not none` was INVERTED at
the IR level. Every `if x is not none:` had been silently running the
wrong branch since the type system landed in M2. No existing example
caught it because they were all coded around the bug.

**Approach**: 4 agents in parallel (AB compiler/VM, C1 computation, C2
data structures, C3 concurrency+interp). Then a 5th fix-pass agent for the
critical bugs. The parallelism worked cleanly because agents had disjoint
file ownership — compiler/VM changes in AB, only-add-new-files in C1/C2/C3.

---

---

## M11 — Class-system overhaul

**Scope**: write 5 more real-world programs to stress-test the class system,
then fix the architectural bugs they surface plus the deferred entries in
`BUGS_KNOWN.md`.

**Round 1** — 3 parallel agents wrote:
- **lambda_calc.spy** (232 lines) — λ-calculus with `Var`/`Abs`/`App` AST, capture-avoiding substitution, divergence detection
- **calculator.spy** (249 lines) — recursive-descent arithmetic parser + evaluator
- **tictactoe.spy** (285 lines) — 9-cell board, minimax to depth 9
- **levenshtein.spy** (146 lines) — 2D DP edit distance
- **lisp.spy** (647 lines) — toy Lisp with closures, environments, builtins

**Bugs surfaced**:
- 3 known bugs from `BUGS_KNOWN.md` confirmed (sealed dispatch, field aliasing, vtable mod-4)
- **N1**: vtable cap at 4 total slots on the base class — sharpens BUG-017
- **N2**: deterministic heap corruption on subclass-with-class-ref-fields + virtual call
- **i32(i64) silent truncation** — `i32`, `i64`, `f64`, `char` ctors all dispatch to a fixed native id, ignoring arg type. Same pattern as M10's `str(char)` fix.
- **str(f64)** of whole number prints `"3.0"` not `"3"` (low severity, doc inconsistency)

**Round 2 (fix pass)** — single agent fixed:
- BUG-015 (sealed dispatch) — `lower_method_call` devirt now requires `!is_open && !is_sealed`
- BUG-016 (subclass field aliasing) — `layout_class` seeds offset from `parent.payload_size`; subclasses inherit parent fields
- BUG-017 + N1 (vtable cap) — **three converging sub-bugs**:
  - Subclass vtables didn't inherit parent methods
  - IR didn't walk up the chain for inherited fn_ids
  - **`op_new` class_id vs type_id collision** — long-standing M3 hack that worked only while ids didn't collide; the 4th user class (`class_id 16`) collided with Shape's `type_id 16`
- N2 — confirmed to be BUG-016 in disguise (subclass `car` field overwrote vtable pointer at offset 0)
- Primitive ctor dispatch — `lower_call` mirrors `str(x)` pattern for all prim ctors
- `str(f64)` — was already correct, just needed spec doc

**The surprise finding (worth a thesis paragraph)**: BUG-026 (non-deterministic
heap corruption in json_parse/calculator) was **also BUG-016 in disguise**.
After the M11 fix, both calculator and json_parse run **5/5 cleanly** where
they were 0/3 before. The non-determinism was heap layout varying across
runs; the underlying trigger was always the same offset-aliasing causing
the GC to walk through a corrupted vtable pointer.

**Tests**: 173 → 201 (+12 regression tests in `vm/tests/m11_fixes.rs` + 9
example integration tests + 7 reshuffled).

**Benchmarks**: still 16/16 wins, slight perf improvement (fib(33) 35.5ms → 30.6ms).

---

## What this trajectory shows

- **Bugs found scales with running real programs, not with writing tests.**
  M0–M9 added 134 tests and found ~12 bugs. M10 added one round of stress-
  testing and found ~17 more bugs.
- **A 4-program benchmark suite massively under-tested the language.**
  Every "real" program found something the bench suite hadn't.
- **Each bug fix tends to reveal a sibling.** CSV aggregator's nullable
  float bug → audit found 4 more nullable dispatch sites with the same
  pattern. The first sealed-class-dispatch issue was likely one of several.
- **Static types make AOT compilation easy.** The Cranelift integration
  was ~2,000 lines and produced 64× speedups. The hard parts of JIT'ing
  Python (type profiling, deopt, inline caches) simply don't apply.
- **Test discipline matters more than test count.** "exits 0" tests caught
  zero of the M3 vacuous-output bugs.
