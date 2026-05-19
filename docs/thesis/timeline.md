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

## M12 — Second stress round + torture test (2026-05-18)

**Goal**: validate the M11 class-system overhaul by stress-testing program
shapes M11 itself didn't run against, and upgrade BUG-026 / BUG-027 from
"provisionally closed" to "confirmed fixed."

Four parallel agents:

- **C7 / regex**: Thompson-NFA engine. `sealed class RegexNode` with 8
  final subclasses (Lit/Dot/Star/Plus/Opt/Alt/Concat/CharClass), 6 virtual
  methods on the base, subclass fields including class-typed refs. 15
  internal cases pass; 10 sequential runs produce byte-identical stdout.
  **Zero new bugs.** Pre-M11, this shape would have hit ≥4 of the open
  class-system bugs.

- **C8 / dijkstra**: shortest-path with `final class Graph` holding
  parallel `List[List[i32]]` / `List[List[f64]]` adjacency, plus
  `final class MinHeap` with recursive sift-up/sift-down methods. **Zero
  new bugs.** Confirmed that `for v: i32 in g.adj_node[u]:` desugars
  correctly when the iterable is a method-receiver expression.

- **C9 / btree**: in-memory B-tree (order 4) with `final class BNode`
  containing `List[BNode?]` children, recursive search/insert, and node
  splits that allocate fresh BNodes and rewire class-ref slots. Class
  system held up — three back-to-back runs byte-identical. But the
  program surfaced **two new bugs**:

  - **BUG-034**: `str != str` always returned `true` because `emit_binop`'s
    `Ne` arm had no `is_str` branch — fell through to `INe`, comparing
    heap-pointer u64s. Identical shape to BUG-008 (`is not` was emitting
    `RefEq` not `not RefEq`). Fixed inline in M12 with a 4-line patch
    mirroring the `IsNot` precedent. The bug had been latent since
    strings became first-class — every previous example happened to use
    `==` for string compares.

  - **BUG-035**: `and` and `or` are bitwise approximations, not
    short-circuit. Trips `IndexError: -1` on the standard guard idiom
    `b > 0 and xs[b-1] > xs[b]`. Source code comment in `ir.rs:1738` is
    honest ("bitwise approximation"). Deferred — needs IR basic-block
    branching to lower `a and b → if a: b else: false`. First language
    feature in the project that requires emitting new blocks mid-expr.

- **E / torture test**: `compiler/tests/heap_corruption_torture.rs` runs
  the canonical BUG-026/027 repros sequentially — 100× calculator, 100×
  json_parse, 50× lisp. **250/250 clean runs**, zero crashes, zero stderr
  noise, 3.12s total wall-clock. BUG-026 and BUG-027 are **CONFIRMED
  FIXED**. The M11 hypothesis — that BUG-026 was always a manifestation
  of BUG-016 with heap-layout variability supplying the non-determinism
  — is now strongly supported.

**Headline pattern**: pre-M11, every stress program was a bug catalogue
needing extensive workarounds. Post-M11, the regex and dijkstra agents
wrote their programs first-try in the natural shape with no workarounds.
"Zero new bugs" became a valuable confirmation result. M11 didn't just
fix the named bugs — it landed a coherent class system.

**Tests**: 201 → 206. **Bugs**: 29 → 31 found, 27 → 28 fixed, 2 → 3
deferred. **Benchmarks**: untouched (M12 was correctness/confirmation;
no codegen changes affecting perf).

**The new lesson worth a thesis paragraph**: *negative-form silent
miscompiles hide behind positive-form code conventions*. Both BUG-008
(`is not` inverted) and BUG-034 (`str !=` always true) sat in the
codebase from M2 onwards but only surfaced when a stress test organically
used the negative form. Mechanical lesson: any new comparison operator
needs test cases for both forms.

---

## M13–M17 — Language-completeness sprint (2026-05-18)

A 5-milestone chain that closed every "language feature missing" item
from the M10-M12 agent reports' "language-surface awkwardness" sections.
Each milestone was one focused agent task; commits sequenced because
all 5 features touched `ir.rs` and `typecheck.rs` and would have
conflicted in parallel. Total wall-clock: a single orchestrator
session; agent compute ~3 hours (M13) + ~40 min (M14) + ~52 min
(M15) + ~35 min (M16) + ~67 min (M17) ≈ 4 hours.

### M13 — Short-circuit `and`/`or` (BUG-035)

The smallest task. Previously `and`/`or` lowered to `IAnd`/`IOr`
(bitwise approximation). Tripped `IndexError(-1)` on the standard
guard idiom `b > 0 and xs[b-1] > 0`. Fix landed `lower_short_circuit`
in `compiler/src/ir.rs` — the project's **first mid-expression CFG
manipulation**. Reuses the M3.5 slot-based phi pattern: pre-seed result
slot with lhs, `CondBranch`, overwrite from rhs block only when control
flows there.

**Importantly, this pattern was the prerequisite for M15.** Try/except
needs identical machinery: handler block + finally block + phi-merge
on join. Sequencing M13 first was deliberate.

### M14 — Tuples + destructuring

The highest-frequency workflow win. Every M10-M12 stress program
listed "no tuples / no multi-return" in its awkwardness section
(workarounds: 1-element mutable lists as out-params; wrapper classes
with `fst`/`snd` fields). M14 closed it.

Representation: heap-allocated tuple objects as synthetic class
layouts (kind=3 in the type table). **Zero new VM opcodes** — reuses
`Alloc`/`Load`/`Store` and the existing class-tracing GC path.
Surface: `Tuple[T1, T2, ...]` types, `(a, b)` literals, `t.0`/`t.1`
field access, `let a, b = pair()` destructuring, return-position
tuples, element-wise `==`/`!=`, `str()` formatting.

Demo: `examples/dijkstra_with_tuples.spy` — `pop_min() -> Tuple[i32, f64]`
returns both vertex and priority. Eliminates the `visited[]` array
workaround the M12 dijkstra needed because `pop_min` could only return
one value.

Incidentally fixed an `assert(cond, msg)` IR-tuple-allocation crash that
would have surfaced as a regression in every example using asserts
with messages — caught because the tuple lowering tried to allocate
with `tid=u32::MAX` for the wrapper tuple.

### M15 — try/except/finally + raise (BUG-025 closed)

The biggest of the 5 by far. End-to-end wiring through parser →
resolver → typecheck → IR → codegen → bytecode → VM. The foundation
was favourable: `Stmt::Try` already in AST (parser accepts it; codegen
just drops it pre-M15), `VmError::UncaughtException { type_name, message }`
already propagated through native calls (IndexError on `xs[i]`, IOError
on `open()`, etc.). Pre-M15 these dropped to program-level abort; post-M15
they flow through handler frames.

Representation: **lazy materialisation on handler bind.** Exceptions
stay as a Rust `VmError` payload until an `as e:` arm forces
materialisation into a 2-field heap object. Most exceptions either
escape or are caught without binding, so eager alloc would be waste.

Four bytecode opcodes reserved since project start (Throw/EnterTry/
LeaveTry/Rethrow) finally activated.

JIT carve-out is **automatic** via `vm/src/decompile.rs`'s catch-all
`_ => Err(DecodeError::Unsupported(...))` arm. Any function with
try/raise opcodes silently falls back to the interpreter — mirrors
the M8 per-function JIT opt-in pattern. Zero JIT changes needed.

BUG-025 closing demo: `examples/safe_open.spy` — `try: f = open("missing.txt", "r") except IOError as e: ...`
now catches the file-not-found and prints a recovery message. Pre-M15
the same program aborts at the open call with non-zero exit.

Known follow-up: `with open(...) as f:` does not route through try/except
(the `with` desugaring bypasses the handler frame). Documented in §7.5.4
of the spec; the long-term fix is to desugar `with` to a try/finally pair.

### M16 — isinstance + match case Constructor() patterns

Eliminates the `kind: i32` discriminator workaround used in every M10-M12
sealed-class program (json_parse, lisp, lambda_calc, calculator). Pre-M16
the natural form parsed but `Stmt::Match` was an M4-era placeholder in
`compiler/src/ir.rs:894` that dropped to nothing.

Surface: `isinstance(x, T)` with subclass-chain walking; flow narrowing
in if-bodies (`if isinstance(x, T): x.field_of_T`); `match` with
Constructor patterns, Tuple patterns, Wildcard, Identifier, and literal
patterns. Sealed-class exhaustiveness as a stderr **warning** (not error
— scoped down to avoid needing a real ADT pass).

The Constructor field-binding via `Load { offset: layout.fields[i].offset }`
**worked first-try thanks to M11's BUG-016 fix** (subclass field offsets
no longer alias parent fields). This is the kind of compound result that
makes the thesis: M11 + M16 ship a coherent class-system feature that
neither one alone delivers.

Demo: `examples/calculator_with_match.spy` — AST + 2 evaluators in 73
lines. Clearer to read than the original `calculator.spy`'s virtual-method
approach.

### M17 — Generics with call-site monomorphisation

Closes "rewrite-per-type friction" — the last item on the language-gap
list. Previously `fn id[T](x: T) -> T` parsed (the AST had `GenericParam`
on `FnDecl` since M1) but never monomorphised; the typechecker treated
`T` as a hole.

Strategy: **lazy monomorphisation via worklist.** Generic templates
never emit bytecode. Each `(fn_sym, type_args)` pair gets one bytecode
function with a deterministic mangled name (`id__i32`, `first__i32_str`,
etc.). Pass 2.6 (new) seeds the worklist from the typechecker's
instantiation set. Pass 3 skips generic templates. Pass 3.5 (new) drains
the worklist; transitive instantiations are minted on the fly in
`lower_call` and re-queued. Drains to fixed point.

Per-instantiation operator binding: `T + T` defers; when the substituted
instantiation re-typechecks, `i32 + i32` becomes `IAdd`, `str + str`
becomes `StrConcat`, etc.

Demo: `examples/quicksort_generic.spy` — 38 LoC `partition[T] +
quicksort[T]` sorting both `List[i64]` and `List[f64]` from one body.
~20% shorter than two hand-rolled copies at 2 types; the ratio scales
linearly with type count.

Out of scope (v0.2): generic classes, generic methods on non-generic
classes, bounded generics (`T: Comparable`), auto-inference from
return-type context.

Incidental finding: `TypedModule::instantiations` was declared as
`HashSet<(SymbolId, Vec<Ty>)>` in earlier scaffolding, but `Ty` doesn't
derive `Hash`/`Eq` — the set was never inserted into. Switched to
`Vec` + string-keyed dedup. No user-visible symptom.

### Sprint totals

- **Tests**: 206 → 255 (+49 across the chain).
- **Examples**: 23 → 28 (+5: dijkstra_with_tuples, safe_open,
  calculator_with_match, quicksort_generic, plus probe files).
- **Spec**: STRICTPY_SPEC.md gained sections on tuples (§5.5),
  try/except (§7.5), isinstance + match (§6.5), generics (§5.1.5).
- **Bugs**: 31 found, 30 fixed, 1 deferred (only BUG-028, the lexer
  line-continuation enhancement, remains).
- **Workarounds eliminated** (per agent reports): 1-element mutable
  lists, wrapper Pair classes, kind:i32 discriminators, virtual-method
  pseudo-dispatch where the natural form was a tuple/match, and
  rewrite-per-type for container algorithms.
- **Benchmarks**: untouched. 16/16 wins; fib(30) 13.1ms (~11× CPython).
  M13-M17 was language completeness, not perf work.

The language now feels meaningfully **Python-shaped**. The remaining
gaps (generic classes, exception subclassing, bounded generics,
proper `with → try/finally` desugaring) are all scoped to v0.2.

---

## M18 — Round 4 stress test (2026-05-18)

The first stress round to specifically target the M13-M17 surface. Four
parallel C-agents, file-disjoint, each picking a different combination
of the new features:

| Agent | Program | Result |
|---|---|---|
| R1 / algorithms_lib | Generic free-fn library (find_first, zip, unzip, binary_search, merge_sorted, group_by_first) | **Zero new bugs** |
| R2 / json_parse_v2 | JSON parser rewritten with M13-M17 surface (sealed + match + try/except + tuples) — direct comparison to M10's json_parse.spy | **Zero new bugs** |
| R3 / expr_interp | Small expression-language interpreter with sealed Expr/Value, recursive match-based eval, try/except for runtime errors | Found **BUG-036** (divzero name mismatch) |
| R4 / graph_lib | Generic graph algorithms (BFS, Dijkstra, topo-sort with cycle detection, shortest_path with try/except) — empirical worklist verification | **Zero new bugs**; verified M17 worklist drains 8 instantiations to fixpoint with 2 transitive |

**Headline numbers**:

- M10 (round 1) found 17 bugs in one round.
- M11 (round 2) found 6.
- M12 (round 3) found 2.
- M18 (round 4) found **1**.

The stress-test ROI curve has flattened sharply. This is itself a
load-bearing thesis result: post-M17 the language has settled to the
point where 1500 lines of stress-test code surface a single bug, and
that bug is a spec/runtime drift (the spec was honest about the legacy
exception name; the runtime never followed through on emitting the
canonical one) — not a silent miscompile or a heap corruption.

**BUG-036 in one sentence**: `try: 1/0 except ZeroDivisionError:` did
not catch, because the runtime emitted the legacy `DivisionByZeroError`
name and the arm-matcher in `vm/src/interp.rs:456` did exact-string
equality. Fixed by canonicalising the emit (4 sites) AND adding an
alias-aware match so the legacy filter still works.

**R2 headline (workaround inventory)**: the original M10 `json_parse.spy`
documented eight language workarounds in its 70-line header. All eight
are now obsolete. R2's `json_parse_v2.spy` writes the natural form
(sealed JsonValue with 6 subclasses; instance methods on Parser;
`List[Tuple[str, JsonValue]]` for objects; try/except for parse errors;
match for serialisation) and works on first compile. 374 lines → 152
lines, ~60% reduction. **The strongest single piece of evidence that
M13-M17 collectively shipped a coherent language.**

**R4 headline (worklist verification)**: the M17 monomorphisation
worklist is the most algorithmically complex feature added in the
sprint. R4's graph_lib was deliberately written to hammer it: BFS uses
`enumerate[T]`, Dijkstra uses `min_by[T]` which uses `safe_get[T]`,
shortest_path threads everything together. The agent counted **8
distinct monomorphic instantiations across 4 generic source functions**,
of which **2 (`safe_get__i32` and dispatch sites inside `min_by`/
`first_or_default`) were discovered only transitively** from inside
other generic bodies. Empirically confirms the Pass 2.6 → Pass 3.5
fixed-point drain.

**Probes archive**: 17 minimal-repro / edge-case probes from the four
agents are preserved at `docs/thesis/m18_round/probes/`. They document
v0.1 limits the user can grep for (isinstance flow-narrowing doesn't
compose through `and`; nested constructor patterns don't bind inner
identifiers; match-scrutinee-throws propagates correctly; `raise e`
re-raise works despite being out-of-scope in §7.5.6 — could be promoted
to spec).

**Tests**: 255 → 267 (+12).

---

## M19–M21 — Stdlib sprint (2026-05-19)

Five milestones over one orchestrator session shipped the import system
plus a usable Phase 1 stdlib: **sys, os, path, io, time, random, math,
json, re**. The language went from "everything is a bare-name native"
to "import json; json.parse(s)" — a real Python-shaped surface.

### M19 — Import machinery + sys (foundation)

The bulk of the load-bearing infrastructure. AST already had
`Import { path, items, alias }`; the parser already accepted both
`import x` and `from x import y, z`; the lexer had `KwImport` / `KwFrom`.
The lift was the resolver → typecheck → IR → VM plumbing plus the
stdlib registration table.

Design choices:
- **Stdlib modules are built-in** (registered in `seed_stdlib_modules`),
  not parsed from `.spy` files. User-defined modules and submodules
  (`import os.path`) are explicitly v0.3.
- **`VmError::Exit(i32)` is non-catchable** — `propagate_exception` only
  matches `UncaughtException`, so `sys.exit(0)` flows past any active
  try/except. Mirrors Python's `SystemExit ⊄ Exception`.
- **`sys.argv` lazy-materialises**: the heap `List[str]` is built on
  first access and cached in `Interpreter::sys_argv_cache`. Most
  programs that import sys don't actually read argv.

Hardest piece: the parser folds `Attr + (` into `MethodCall`, so
`sys.exit(0)` is NOT a `Call(Attr, [...])` — required four intercept
points (Ident-read, Attr-read, Call-with-Attr-callee, MethodCall-on-Ident)
instead of two. Documented in the M19 report for the M20 agents.

Three examples shipped: `echo.spy`, `sum_args.spy`, `print_env.spy`.

### M20a — os, path, io

23 new NativeFns (ids 140-174). `os` for env vars + filesystem
operations; `path` for pure-StrictPy path manipulation (Python's
`os.path` surface, ported as a sibling module since submodules aren't
in v0.2); `io` for stdin/stdout/stderr line-based IO (filling the gap
M19 left when sys.stdin/stdout/stderr were deferred).

New VM infrastructure: `Interpreter::alloc_tuple_obj` — a native
function can now return a tuple. `path.splitext` was the first user;
the IR's tuple `Load(offset)` doesn't consult the type pointer, so
allocating with a null type pointer works.

Cross-platform: `path.sep` via `cfg!(windows)`; `io.input()` strips
both `\r\n` and `\n`; env var case-sensitivity follows OS conventions.

**Incidentally found BUG-037**: `x ?? fallback` always returned
`fallback`. IR placeholder lowering. Same pattern as BUG-008 (`is not`
inverted) and BUG-034 (`str !=` always true). Worked around in M20a
tests; fixed inline in M21.

Four examples: `list_dir`, `env_dump`, `file_stats`, `echo_interactive`.

### M20b — time, random, math (module wrapper)

31 new NativeFns (175-212). `time` for clocks + sleep; `random` for
seeded LCG; `math` as a NEW module wrapping the existing prelude bare
names AND adding constants (`pi`, `e`, `tau`, `inf`, `nan`) plus new
functions (`log2`, `log10`, `gcd`, `factorial`, `is_nan`, `is_inf`).

`time.format_iso` hand-rolls Howard Hinnant's `civil_from_days`
(12 LOC, public domain) instead of pulling in `chrono` for one
function. Saved ~400KB binary size.

`random.choice` / `shuffle` / `sample` shipped as **monomorphic
per-type variants** (`_i64`, `_f64`, `_str`) rather than M17 generics —
integrating stdlib-registered functions with the M17 worklist would
have been deeper than M20b warranted. Generic stdlib functions are
v0.3.

Cross-platform `time.sleep_ms` granularity: ~1ms on Linux, ~15.6ms on
Windows (documented; test assertions are lenient ≥50ms).

Four examples: `fizzbuzz_v2`, `timer_demo`, `math_demo`, `sleep_test`.

### M20c — json, re (Phase 1 complete)

12 new NativeFns (213-217 + 220-226). `json` for JSON validation +
formatting; `re` for regex matching/search/replace/split. Used
Strategy A (native Rust re-implementation) via `serde_json` and
`regex` crates added only to `vm/Cargo.toml`.

`json` shipped the **validation-focused surface** (`parse_to_string`,
`minify`, `is_valid`, `pretty`, `escape`) — the typed `JsonValue`
surface would have needed stdlib-class registration infrastructure
that doesn't yet exist. M18's `examples/json_parse_v2.spy` remains the
canonical typed-parser demo.

`re.match` was renamed `re.fullmatch` because `match` is a hard
keyword in StrictPy (since M16 match/case) and the parser doesn't
allow keywords as attribute names. Contextual-keyword treatment is a
v0.3 candidate.

Two examples: `json_demo`, `regex_demo`. **Zero incidental bugs** —
the first M20-batch sub-milestone with no incidental finding.

### M21 — BUG-037 fix + integration example

Closed BUG-037 (`x ?? fallback` always-fallback) using the M13
`lower_short_circuit` pattern: pre-seed result slot with `lhs`, test
`RefEq(lhs, none)`, branch, evaluate `rhs` only in the is-none block,
slot-based phi merge. Critically, `rhs` is now evaluated ONLY when
`lhs IS none` (short-circuit), matching Python's `or`-fallback
expectation. 6 regression tests including rhs-must-not-trap and
rhs-must-execute paths.

Integration program: `examples/minigrep.spy` (~110 LOC) — a small
grep-like CLI tool exercising `sys + os + io + re + time + try/except
+ tuples` together. 5 integration tests (subprocess) covering: pattern
match on a file, missing-file recovery via IOError catch, bad-pattern
ValueError, usage on no args, line counting. **The strongest single
piece of evidence that the Phase 1 stdlib composes ergonomically.**

### Sprint totals

- **Tests**: 267 → 379 (+112 over 6 commits).
- **Examples**: 32 → 46 (+14: echo / sum_args / print_env / list_dir /
  env_dump / file_stats / echo_interactive / fizzbuzz_v2 / timer_demo /
  math_demo / sleep_test / json_demo / regex_demo / minigrep).
- **Bugs**: 32 → 33 found, 31 → 32 fixed, 1 → 1 deferred (still only
  BUG-028 lexer line-continuation).
- **NativeFn IDs used in sprint**: 130-249 (a contiguous block of 120
  added across 5 milestones).
- **Stdlib modules**: 8 (sys / os / path / io / time / random / math /
  json / re).
- **New crate deps**: `serde_json` and `regex`, both in `vm/Cargo.toml`
  only.

The "placeholder-lowering" pattern hit a third instance (BUG-037,
after BUG-008 and BUG-034). All three were silent miscompiles where a
binary operator's IR lowering shipped as a placeholder that the
typechecker accepted and no test had hit the non-trivial path.
Recurring lesson: audit `ir.rs` for `// placeholder` comments and
operators that just `Copy(operand)`.

The language is now **demonstrably usable** for CLI tools and
data-processing scripts: `minigrep.spy` opens files, handles missing
ones gracefully, runs regexes, writes formatted output with timing — a
real script in a Python-shaped language with no dynamism.

---

## M22 — Phase 2 stdlib (first parallel-agent stdlib round) (2026-05-19)

Phase 2 of the stdlib sprint. **9 modules shipped in parallel** via 4
worktree-isolated agents, ~1.5 hours of parallel agent compute. The
biggest wall-clock acceleration of any round since M11's class-system
overhaul, and the first time the project ran parallel stdlib agents.

Modules: **argparse, collections, csv, base64, hashlib, itertools,
statistics, struct, urllib_parse**. 73 new NativeFn IDs (250-347).

### Agent allocation

- **P2A** (argparse + collections + csv): 26 NativeFn IDs (250-280).
  Highest-ROI module — pre-M22 every CLI tool hand-parsed sys.argv.
- **P2B** (base64 + hashlib): 9 IDs (290-304). Added 5 crates to
  `vm/Cargo.toml`: base64, sha1, sha2, md-5, hmac.
- **P2C** (itertools + statistics): 20 IDs (310-329). Monomorphic
  per-type variants matching the M20b random.* pattern.
- **P2D** (struct + urllib_parse): 18 IDs (330-347). `str`-as-byte-buffer
  encoding (each char is a codepoint 0-255) for binary IO. Self-found
  and fixed an `OBJECT_HEADER_SIZE` mismatch (was 24, should be 16).

### The worktree-isolation pattern

First use of the Agent tool's `isolation: worktree` mode in this
project. Each agent ran in its own git worktree branch
(`worktree-agent-<id>`) and saw a clean repo at M22-round commit time;
they couldn't see each other's writes.

After all four reported complete, the orchestrator cherry-picked the
four commits onto main in order P2C → P2D → P2B → P2A. The first
cherry-pick was clean; the next three conflicted on the four shared
files (`resolver.rs` / `native.rs` / `builtins.rs` / `STRICTPY_SPEC.md`).
Resolution was mechanical: each agent appended to the same point in
each file, so the merge was "keep both, in some order." Spec section
numbers had to be renumbered (all four agents independently picked
§9.15+).

Total integration overhead: ~30 minutes of orchestrator time + one
build per cherry-pick to confirm the resolution was syntactically
correct. **The wall-clock saving from parallel agents (~3.5 hours)
significantly exceeded the integration cost (~30 minutes).**

### Zero-incidental-bug streak

The 9 modules shipped with zero new bugs. Counting from M19:

| Sub-milestone | Modules | Bugs found |
|---|---|---:|
| M19 (sys) | 1 | 0 |
| M20a (os, path, io) | 3 | 1 (BUG-037, found incidentally) |
| M20b (time, random, math) | 3 | 0 |
| M20c (json, re) | 2 | 0 |
| M21 (BUG-037 fix + minigrep) | 0 | 0 |
| **M22 (9 modules)** | **9** | **0** |

Six consecutive sub-milestones shipping 18 stdlib modules with one
bug found — and that bug was a placeholder lowering from the M0-era
parser, not a Phase 1/2 regression. The M19 stdlib-module-table seam
proved to be the load-bearing infrastructure: once it landed, new
modules slot in without disturbing resolver/typecheck/IR.

### Tests + size

- **Tests**: 379 → 468 (+89 across the four agents).
- **Examples**: 46 → 55 (+9 example programs, one per module —
  argparse_demo, word_count, csv_demo, base64_demo, hashlib_demo,
  itertools_demo, stats_demo, struct_demo, url_demo).
- **Spec**: §9.15-§9.23 added (9 new module sections).

### Phase 1 + Phase 2 stdlib summary

17 stdlib modules total over 4 milestones (M19-M22):

| Module | Phase | Wall-clock |
|---|---|---|
| sys | M19 | ~3 hr (foundational) |
| os, path, io | M20a | ~2 hr |
| time, random, math | M20b | ~2.5 hr |
| json, re | M20c | ~1.5 hr |
| argparse, collections, csv | M22 P2A | ~1.5 hr (parallel) |
| base64, hashlib | M22 P2B | ~1 hr (parallel) |
| itertools, statistics | M22 P2C | ~1.5 hr (parallel) |
| struct, urllib_parse | M22 P2D | ~1.5 hr (parallel) |

The language is now usable for the kinds of scripts and tools real
Python users write: CLI parsing (argparse), data processing (csv +
itertools + statistics), encoding (base64 + hashlib + struct), web
(urllib_parse). Networking primitives (socket, http_client, ssl) are
the next big Phase 3 push.

---

## M23 — Phase 3a stdlib (system control + calendar + sync + DB) (2026-05-19)

Second parallel-agent stdlib round. 4 worktree-isolated agents shipped
7 modules in **~80 min parallel + ~45 min orchestrator integration**.
First round to reach into OS FFI (subprocess, threading primitives,
sqlite via rusqlite); the M19 stdlib seam absorbed it cleanly.

### Agent allocation

- **P3a-A** (subprocess + pathlib): 20 NativeFn IDs (350-389). Cross-
  platform process spawn via Rust's `std::process::Command`; pathlib as
  flat functions (typed `Path` class pending v0.3 stdlib-class
  registration).
- **P3a-B** (datetime): 22 NativeFn IDs (390-411). Calendar arithmetic
  on top of M20b's `time` epoch primitives. Hand-rolled
  `days_from_civil` (Howard Hinnant, public domain) to invert M20b's
  `civil_from_days`. **Real platform `local_offset_minutes` via FFI**
  (`GetTimeZoneInformation` on Windows; `localtime_r` on Unix) — no
  chrono crate dep, just inline `extern` bindings.
- **P3a-C** (threading.Lock + Semaphore + queue.PriorityQueue):
  18 NativeFn IDs (420-437). Three new `SharedVm` slot tables (locks,
  semaphores, priority_queues) following the channels/threads/dicts
  pattern. Found **one incidental bug**: registering `threading` as a
  stdlib module broke the existing `from threading import Thread`
  prelude binding because the new-module-match arm errored before
  reaching the legacy-prelude fall-through. Four-line resolver fix.
- **P3a-D** (sqlite3): 9 NativeFn IDs (440-448) via the `rusqlite`
  crate with the `bundled` feature (libsqlite3.c links statically;
  no system SQLite dep). Connections as i64 handles into
  `SharedVm.sqlite_connections`. All result cells stringified
  (typed rows are v0.3 when `bytes` lands).

### Integration cost

Cherry-pick order P3a-A → B → C → D, smallest-conflict-first. Each
subsequent cherry-pick added conflicts in the same 4 files
(`resolver.rs`/`native.rs`/`builtins.rs`/`STRICTPY_SPEC.md`), plus
P3a-C and P3a-D each added a new field to `SharedVm` (so `interp.rs`
conflicted on the 3-field block).

One unusual conflict: P3a-D's `sqlite3.column_names` handler got
git-aligned with HEAD's `pathlib.read_lines` at a shared `let sp =
interp.alloc_string(...) as u64;` line — the surrounding loop bodies
look similar. Required manual reconstruction of pathlib_read_lines's
tail (the `unsafe { list_push }; Ok(lst); }` lines) before the sqlite3
section could be appended.

Spec section renumbering: agents independently picked §9.24+. Final
ordering: §9.24 subprocess, §9.25 pathlib, §9.26 datetime, §9.27
threading, §9.28 queue, §9.29 sqlite3.

### Three sub-milestones with consistent zero-bug streak; one find

| Sub-milestone | Modules | Bugs found |
|---|---|---:|
| P3a-A (subprocess + pathlib) | 2 | 0 |
| P3a-B (datetime) | 1 | 0 |
| **P3a-C (threading + queue)** | 3 | **1** (resolver shadow fix) |
| P3a-D (sqlite3) | 1 | 0 |

The trend since M20 (one incidental bug per 2-4 sub-milestones)
holds. The M19 stdlib-module-table is still the load-bearing
infrastructure that lets new modules slot in without disturbing
resolver/typecheck/IR — except in M23 P3a-C, where the new module
name happened to collide with a legacy prelude binding. Documented
fix; future stdlib modules avoid the same pitfall.

### Phase 1 + 2 + 3a stdlib summary

**24 stdlib modules** total over 5 milestones (M19-M23):

- M19: sys
- M20a: os, path, io
- M20b: time, random, math
- M20c: json, re
- M22: argparse, collections, csv, base64, hashlib, itertools,
  statistics, struct, urllib_parse
- **M23: subprocess, pathlib, datetime, threading, queue, sqlite3**

The language now reaches into:
- CLI ergonomics (sys, argparse)
- Data processing (csv, json, itertools, statistics)
- Encoding/crypto (base64, hashlib, struct)
- Text/regex (re, urllib_parse)
- Filesystem + IO (os, path, io, pathlib)
- Time + calendar (time, datetime)
- System control (subprocess)
- Concurrency primitives (threading, queue)
- Persistence (sqlite3)

Networking (socket, http_client, ssl) is the next big gap — Phase 3b.

### Tests + size

- **Tests**: 468 → 553 (+85 across the four agents).
- **Examples**: 55 → 62 (+7 — subprocess_demo, pathlib_demo,
  datetime_demo, threading_demo, queue_demo, sqlite_demo).
- **Spec**: §9.24-§9.29 added (6 new module sections).

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
- **Confirmation is a deliverable.** M12 added 3 stress programs; 2 found
  zero bugs and the headline was the absence. Pre-M11 every class-heavy
  program was a bug catalogue. The M12 regex agent's first-try natural-shape
  program is itself the proof that M11 actually landed.
