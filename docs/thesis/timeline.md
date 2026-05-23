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

## M24 — Phase 3a stress round (2026-05-19)

First stress round on the Phase 3a surface. 4 parallel worktree-isolated
agents wrote ~1500 LOC of real programs that combine
subprocess + threading + queue + sqlite3 + datetime + pathlib in ways
the per-module unit tests didn't.

### Agents + outcomes

- **M24-A** (job_scheduler.spy): background scheduler combining
  subprocess + threading.Lock + queue.PriorityQueue + datetime. 9/9
  probes PASS. **0 bugs.**
- **M24-B** (event_log.spy): SQLite-backed event log CLI combining
  sqlite3 + datetime + argparse + io + pathlib + re. 14/14 probes
  PASS. **Found BUG-039** — `key in Dict[str, *]` always returned
  false. Plus a related segfault on `<i64> in Dict[i64, i64]`
  (separate latent issue: M5 Dict runtime is hardcoded to str keys).
- **M24-C** (test_runner.spy): parallel test runner combining
  subprocess + threading + queue + sqlite3 + time. 10/10 probes PASS.
  **Real parallelism verified** — 3 runs gave N=4/N=1 speedups of
  3.62×, 5.75×, 2.64×.
- **M24-D** (fs_migrator.spy): filesystem migration tool combining
  pathlib + os + datetime + subprocess + io. 10/10 probes PASS. **0
  bugs**, but **documented missing stdlib primitives** for Phase 3b:
  `os.mtime`, `os.size`, `pathlib.stat`, `os.rmdir`, `re.find_all`
  capture groups, `pathlib.normalise`, `subprocess` env-var injection.

### BUG-039 — fourth placeholder-lowering instance

The headline finding. `compiler/src/ir.rs::emit_binop` had
`AstBinOp::In => IROp::IEq, // placeholder` — comparing the key
against the container's heap pointer as i64. Always false for any
separately-allocated key.

This is the **fourth instance** of the placeholder-lowering pattern:

| Bug | Operator | Placeholder | Fixed in |
|---|---|---|---|
| BUG-008 | `is not` | `RefEq` (not `not RefEq`) | M10 |
| BUG-034 | `str !=` | `INe` (no `is_str` branch) | M12 |
| BUG-037 | `??` (null-coalesce) | `Copy(rhs)` | M21 |
| **BUG-039** | **`in` / `not in`** | **`IEq` / `INe`** | **M24** |

Same shape every time: a binary operator with a missing branch in
its IR lowering. Every operator whose semantics depend on operand
type needs to dispatch on type, not emit a hardcoded IROp.

Fix in M24: `In` lowering now dispatches on the RHS (container)
type. `key in Dict[str, V]` → `NativeFn::DictHas(dict, key)`;
`x in Set[T]` → `NativeFn::SetHas(set, x)`. `NotIn` mirrors then
emits `BoolNot`. List membership and non-str-keyed Dict still
placeholder (v0.3 work).

### Worktree integration quirk

All four agents finished their work but **ran out of compute budget
at the final `git commit` step**. The orchestrator committed each
worktree's tree on the agent's behalf, then cherry-picked onto main.
Pattern note: future agent briefs should explicitly call out "commit
EARLY, before writing the long report" — the agents wrote 500-1000
word reports last and exhausted budget before getting back to
`git commit`.

### Stress-round bug-rate trajectory

| Round | Programs | Bugs found |
|---|---|---:|
| M10 (round 1) | 6 | 17 |
| M11 (round 2) | 5 | 6 |
| M12 (round 3) | 3 | 2 |
| M18 (round 4) | 4 (M13-M17 surface) | 1 |
| **M24 (round 5)** | 4 (Phase 3a surface) | **1** |

The curve is flat at 1 bug per round of ~1000-1500 LOC since M18.
That's a stable signal: the language is in steady state — stress
tests still find things, but the things they find are localized
(BUG-037 was a placeholder, BUG-039 is a placeholder; not
architectural). The methodology section should call out the
"placeholder-lowering audit" as a mechanical pass that would have
caught all four pattern instances at once if run after M2.

### Tests + size

- **Tests**: 553 → 578 (+25 from new examples + BUG-039 regression).
- **Examples**: 62 → 70 (+8 — job_scheduler, event_log, test_runner,
  fs_migrator, plus probe files).
- **Stdlib modules**: unchanged at 24. M24 was stress + bug fix only.

---

## M25 — Unified `spy` CLI (2026-05-19)

User request: "Python has a single command to both compile and execute;
StrictPy has separate executables. Make StrictPy analogous to Python."

### Decisions reached before coding

Four clarifying questions resolved up front (rather than mid-implementation):

| Decision | Choice |
|---|---|
| Binary name | **Single `spy` only** — remove `spyc` entirely; `spy --compile-only` covers compile-only workflows |
| Cache location | **`__spycache__/foo.spyc`** next to source (Python's `__pycache__` shape) |
| Staleness rule | **Source mtime > cache mtime** (Python's rule) |
| Inline `-c "code"` | **Yes, include now** (matches `python -c`) |

### What landed

```
spy SCRIPT [ARGS...]              # compile-if-stale + run
spy -c CODE [ARGS...]             # compile inline + run
spy --compile-only SCRIPT [-o OUT]
```

- `compiler/Cargo.toml` `[[bin]] name = "spyc"` removed; `compiler/src/main.rs`
  deleted. Library API (`compile_file` / `compile_source`) unchanged.
- `strictpy-compiler` promoted from `[dev-dependencies]` to
  `[dependencies]` in `vm/Cargo.toml`. The previously circular
  dev-dep edge (`compiler ↔ vm`) becomes a clean DAG: `compiler ← vm`.
- `vm/src/lib.rs` gains
  `run_bytes_with_args(bytes, argv0, args)`; `run_file_with_args`
  now delegates to it. `-c` mode never touches the filesystem.
- `vm/src/main.rs` rewritten with clap-driven Python-style CLI
  (~210 LOC); helpers for `cached_spyc_path` and `needs_recompile`.
- `compiler/tests/m25_unified_cli.rs` adds 8 integration tests
  covering all four CLI shapes + stale-cache recompile + fresh-cache
  reuse + trailing args + unknown-extension error.

### Tests + size

- **Tests**: 578 → 586 (+8 M25 tests). Zero regressions.
- **VM LOC**: +~150 (new main.rs CLI surface).
- **Compiler LOC**: -32 (deleted main.rs).
- **Examples**: unchanged at 70. M25 ships no new `.spy` programs.
- **Stdlib modules**: unchanged at 24.
- **Bench**: not re-run (no codegen change; pure I/O glue).

### Why a one-conversation refactor, not parallel agents

Touched 6 files with high cross-coupling: two Cargo manifests, lib API,
CLI binary, integration tests, plus spec + README. Parallel agents
would each need the same global context, and the integration cost
would have dominated the parallelism gain. A focused single-threaded
session got it through in ~30 min including the test fixture
debugging (Rust raw string vs line-continuation indentation).

### Caveats deferred to v0.3

- Cross-process cache write race (two simultaneous `spy hello.spy`
  invocations can both write the same `.spyc`; bytes are identical so
  benign, but on Windows the second can briefly fail).
- Read-only source directory falls back to a temp-dir cache in
  Python; StrictPy currently errors instead.
- `.spyc` cache key doesn't yet include a build identifier; clearing
  `__spycache__/` after a major upgrade is currently manual.

---

## M26 — Extended benchmark suite (2026-05-19 latest)

10 additional benchmark tests (5 pure-compute + 5 stdlib) added on top
of the canonical 4-program / 16-cell suite. Generators in
`bench/harness.py`; rendered to `bench/EXTENDED_REPORT.md`; raw timings
in `bench/history/m26_extended.json`.

**Headline (30 cells)**: 28W / 2T / 0L vs CPython 3.12.10. Median pure-
compute ratio 0.15×, median stdlib ratio 0.25×. Two interesting
empirical findings, both honest data:

1. The btree row narrows monotonically as allocation pressure grows
   (0.23× → 0.71× → 1.13× at n=10k). At ~10k recursive insertions,
   StrictPy's `rt_alloc` + conservative GC overhead overtakes the JIT
   win. Cell precise stack maps + a moving GC would fix.
2. The stdlib comparison was expected to land near 1× since both sides
   do the work in C/Rust. Instead all 15 stdlib cells go to StrictPy
   with the narrowing-at-scale pattern visible on every test (CSV
   0.20× → 0.91×, SQLite 0.19× → 0.75×). Python's ~50–70 ms startup
   overhead amortises out as the work grows; what remains is the true
   relative cost of the bindings.

No language/compiler changes; new pure-bench infrastructure only.

---

## M46 — `tabular` stack/unstack + df.loc range + outer-merge MultiIndex + extensions (2026-05-23)

The cleanup-and-polish milestone after M45 closed the v1
MultiIndex propagation story. Adds stack/unstack (pandas's
MultiIndex bread-and-butter), df.loc range-by-label per dtype,
outer-merge MultiIndex fallback (replaces M42's RangeIndex
fallback for dtype-mismatched outer joins), time-series ops
MultiIndex handling, and small ergonomic extensions. After M46
the `tabular` v1 surface is **functionally complete** except for
v0.4 polish (rolling Welford std, categorical, df.iloc 2-D, etc.).

Single agent, 5 separable per-phase commits (disjoint-handler
cadence — first commit at ~20% of budget), ~2358 LOC,
**zero STOP CRITERIA cuts**.

### Surface

- **Phase A — stack/unstack**:
  - `df.stack() -> DataFrame`: all regular columns become an
    innermost MultiIndex level + a single value column. Requires
    shared-dtype across columns (same constraint as M43's `melt`).
    Documented in §11.33.
  - `df.unstack() -> DataFrame`: innermost MultiIndex level
    pivots out to columns. Requires input MultiIndex; raises on
    no-index or single-col index. Documented in §11.34.

- **Phase B — `df.loc` range-by-label per dtype** (extends M41's
  one-row `select_by_label_*` to inclusive ranges):
  - `df.loc_range_i64(start: i64, stop: i64) -> DataFrame`
  - `df.loc_range_f64(start: f64, stop: f64) -> DataFrame`
  - `df.loc_range_str(start: str, stop: str) -> DataFrame`
  - `df.loc_range_bool(start: bool, stop: bool) -> DataFrame`
  - `df.loc_range_datetime(start: i64, stop: i64) -> DataFrame`
    (epoch-ms)

- **Phase C — outer-merge MultiIndex fallback + set_index unification +
  pivot_table extensions**:
  - Outer-merge dtype-mismatch now produces a NaN-padded 2-level
    MultiIndex (level 0 = lhs key with NaN where lhs has no match;
    level 1 = rhs key with NaN where rhs has no match). Replaces
    M42's RangeIndex fallback. Hook `m46_merge_outer_dtype_mismatch_
    multiindex` into existing `m39_df_merge` / `m45_merge_build_
    multiindex`.
  - `df.set_index_list(cols: List[str])`: unifies M41's
    `set_index(name)` and M44's `set_index_multi(cols)` via length
    dispatch (1 element → single-col; ≥2 → MultiIndex; empty →
    raises). Pandas-style ergonomic.
  - `df.pivot_table_aggfunc_list(...)`: emits one set of value
    columns per aggfunc name in the list.
  - `df.pivot_table_margins(...)`: adds "All" row + "All" column
    with the aggfunc applied across the slice.

- **Phase D — time-series ops MultiIndex handling**:
  - `resample` + `resample_index`: explicitly drop MultiIndex
    (reshape the row dimension — no clean target).
  - `asof_merge` + `asof_merge_index`: preserve lhs MultiIndex
    via M45's merge MultiIndex pattern.

- **Phase E**: 25 VM tests + 2 demo-runs +
  `examples/tabular_m46_extensions_demo.spy` (~160 LOC) +
  LANGUAGE_GUIDE.md §5 M46 subsection + §11.32 rewrite +
  §11.33/§11.34 new (stack must-share-dtype, unstack
  must-have-MultiIndex).

### NativeFn IDs (10 new)

1033-1042: stack, unstack, loc_range × 5 dtypes, set_index_list,
pivot_table_aggfunc_list, pivot_table_margins. The outer-merge
fallback is internal to existing merge handler — no new NativeFn.

### Methodology data point — Edit-tool leak hypothesis refuted

M45 had proposed: "the leak only triggers when worktree state
diverges from project root at session start." Evidence: M44 (cp
run, no leak) and M45 (cp NOT run because Bash denied, no leak).

**M46 refutes the hypothesis.** Same conditions as M45 (Bash
denied for the cp loop form, main was sync'd post-M45-push) — but
the leak DID recur. M45 was the lucky outlier, not a stable
improvement.

**Honest current state after M46**: cause unknown; the leak is
intermittent or has triggers we haven't identified. The
workaround is well-routinized — precautionary cp at session
start if Bash available, per-file cp recovery mid-session,
orchestrator `git checkout --` + `git merge --ff-only` against
worktree HEAD. Harness root-cause investigation stays
deprioritized because the workaround is reliable and cheap, but
we should no longer claim we understand the leak's mechanism —
only that we can survive it.

The M45/M46 hypothesis-refutation cycle is itself a methodology
data point: **don't extrapolate from one milestone of evidence**
when the underlying phenomenon is intermittent. M45's three-data-
point support (M40 narrowing + M44 cp-worked + M45 no-cp-still-no-leak)
felt compelling but generalized too quickly.

### Tests flipped (0)

The M45 outer-merge-fallback test exercises a different case
(same-dtype outer with one side missing) than M46's outer-merge
MultiIndex fallback (mismatched-dtype outer). No flip required —
M45's test continues passing alongside M46's new behavior.

### Tests + size

- Tests: 934 → 961 (+27 net: 25 new VM in
  `vm/tests/m46_tabular_extensions.rs`, 2 new demo-runs in
  `compiler/tests/tabular_m46_extensions_demo_runs.rs`).
- Examples: 109 → 110
  (`examples/tabular_m46_extensions_demo.spy` ~160 LOC).
- Stdlib classes: unchanged at 18. NativeFn IDs: 1033-1042 (10
  new).
- LOC: `vm/src/builtins.rs` +1008 (stack/unstack handlers + 5
  loc_range_* + 3 pivot_table extensions + set_index_list + the
  outer-merge MultiIndex hook + asof_merge MultiIndex propagation),
  `vm/tests/m46_tabular_extensions.rs` +815 (new), demo + demo-runs
  +258 (new), `shared/src/native.rs` +54, `compiler/src/resolver.rs`
  +57, `compiler/src/ir.rs` +11, `LANGUAGE_GUIDE.md` +57. Total
  ~2358 LOC.

### Lesson 1 streak: 28 consecutive clean agents

(M28 → M46). **Ten consecutive `tabular` package agents shipped
clean** — M37 / M38 / M39 / M40 / M41 / M42 / M43 / M44 / M45 /
M46. The M41/M42/M43/M44/M45/M46 sextet validates the
cadence-classification pattern (shared-infra vs disjoint-handler)
across multiple milestones; M46 ran 5 clean phase commits.

### After M46: `tabular` v1 surface is functionally complete

What's done (M37-M46 inclusive):
- Sealed Column hierarchy + DataFrame core + IO + filter + sort
- Aggregations + group-by (single-col → single-col index;
  multi-col → MultiIndex)
- Reshape (unique/value_counts/concat/merge/pivot/melt/pivot_table)
  with index propagation
- Time series (cumulative/null/iloc/rolling/resample/asof_merge)
  with index handling
- DatetimeIndex (M41-M43) with full propagation through 21 methods
- MultiIndex (M44-M45) with full propagation through 18 methods +
  multi-col group_by promotion
- **stack/unstack + df.loc range + outer-merge MultiIndex
  fallback + set_index unification + pivot_table extensions
  (M46)**

What's deferred to M47+ (v0.4 polish):
- Rolling-window optimizations (Welford incremental sum-of-squares
  for rolling_std stability, `min_periods` argument, `center=True`
  window alignment).
- Categorical column dtype (memory-efficient group-by keys;
  faster equality).
- `df.iloc[rows, cols]` 2-D indexing (currently row-range only).
- Negative-index support for `iloc`.
- More resample rules (`1w` / `1M` / `1Y` — needs calendar
  arithmetic layer).
- `df.rolling(window).agg(...)` chainable fluent API.
- Desktop UI (the M37-design Phase 6 — webview-served or
  Tauri/wry hybrid).

---

## M45 — `tabular` full MultiIndex propagation through M42 + M43 ops (2026-05-23)

Lifts the M44 v1 scope-down. The 14 row/column-transforming and
reshape handlers that previously dropped MultiIndex back to
RangeIndex now propagate it correctly. After M45 the `tabular`
package is **fully index-aware for both single-col AND MultiIndex
inputs end-to-end** — the v1 propagation story is complete.

Single agent, 3 separable per-phase commits (disjoint-handler
work — first commit at ~20% of budget, matching M42 and M43's
cadence), ~1300 LOC, **zero STOP CRITERIA cuts**.

### Surface

**Phase A — M42 ops MultiIndex propagation**:

- `sort_by` / `dropna` / `dropna_subset`: route emit through
  M44's auto-dispatching helper `m44_permute_multiindex_into_df`
  (which dispatches: no index → m37_build_df; single-col →
  m42_permute_index_into_df; MultiIndex → permute each level +
  m44_build_df_with_multiindex).
- `select` / `drop` / `rename` / `fillna_*`: route through new
  sibling helper `m45_copy_multiindex_into_df` (no row permutation
  — just copies the index).
- `merge`: extends per-`how` index policy to MultiIndex via new
  `m45_merge_build_multiindex`. Inner/left/right preserve MultiIndex
  on the respective side. **Outer with dtype-mismatch still falls
  back to RangeIndex** (M46 anchor — replace with NaN-padded
  MultiIndex per pandas).

**Phase B — M43 reshape ops MultiIndex propagation**:

- `melt`: repeats each MultiIndex level per `value_var` (extends
  M43's single-col index repetition).
- `concat_rows`: new helper `m45_concat_rows_multiindex` with
  strict per-level reconciliation. **3-tier fallback**: if all
  inputs share MultiIndex shape (same level count + dtype-per-level
  + name-per-level) → concatenate level-by-level; else if all
  inputs share single-col index → M43 path; else → RangeIndex.
- `concat_cols`: takes lhs's MultiIndex (consistent with M42
  merge / M43 single-col policy).
- `pivot` and `pivot_table`: explicitly **drop a MultiIndex**.
  Both reshape the row dimension — the input MultiIndex labels
  don't have a clean target in the output. Same as today's
  RangeIndex-fallback shape; just documented now.

**Phase C**: 17 VM tests + 2 demo-runs +
`examples/tabular_multiindex_propagation_demo.spy` (~175 LOC, 9
M45-aware ops with `index_nlevels()` checks at every step) +
LANGUAGE_GUIDE.md §5 M45 subsection + §11.26 rewrite
("fully index-aware end-to-end") + §11.32 rewrite (drop list
now empty for M42+M43 ops; only pivot/pivot_table explicit drops
remain + the deferred-to-M46 items).

### Tests flipped (2 — exactly as predicted)

- `vm/tests/m44_tabular_multiindex.rs::sort_by_drops_multiindex_m44b_anchor`
  → `sort_by_preserves_multiindex_m45` (`nlev=0` → `nlev=2`).
- `vm/tests/m44_tabular_multiindex.rs::select_drops_multiindex_m44b_anchor`
  → `select_preserves_multiindex_m45` (same flip shape).

This is the second time a milestone has flipped tests (after M42's
historic first); each flip is precise (1 line of assertion per
test). The "drops X" → "preserves X" pattern is now a known
contract-change motif.

### Methodology data point worth recording — leak hypothesis refined

The brief asked the agent to run the precautionary `cp` block at
session start. **The agent could not run it** because Bash and
PowerShell were both denied at session start. **Yet zero leak
recurrences happened anyway** — every subsequent Edit/Write
landed in the worktree directly.

Combined with M44's clean orchestrator-side integration (main was
completely clean post-agent, no source modifications to reset),
the **refined hypothesis** is:

> The Edit-tool worktree leak triggers when worktree state
> diverges from project root at the start of an Edit session, NOT
> just "first Edit on existing file" (M40 narrowing) or "first
> Edit/Write on any file" (M43 broadening). If the previous
> milestone's orchestrator-side integration left main + worktree
> in agreement, subsequent Edits land correctly.

If this hypothesis holds, M44's precautionary `cp` may have been
redundant for the same reason. **M46 will confirm**: if it also
starts with a sync'd worktree (which it should after this M45
push), it should skip the cp block and still see no leak.

The workaround stays in briefs as a defensive measure (cheap;
harmless if not needed). Harness root-cause investigation remains
deprioritized — the workaround is reliable.

### Tests + size

- Tests: 915 → 934 (+19 net: 17 new VM in
  `vm/tests/m45_tabular_multiindex_propagation.rs`, 2 new
  demo-runs in `compiler/tests/tabular_multiindex_propagation_demo_runs.rs`,
  2 M44 tests flipped).
- Examples: 108 → 109
  (`examples/tabular_multiindex_propagation_demo.spy` ~175 LOC).
- Stdlib classes: unchanged at 18. NativeFn IDs: unchanged.
- LOC: `vm/src/builtins.rs` +287 (3 new helpers + handler edits),
  `vm/tests/m45_tabular_multiindex_propagation.rs` +338 (new),
  `examples/tabular_multiindex_propagation_demo.spy` +199 (new),
  `compiler/tests/tabular_multiindex_propagation_demo_runs.rs`
  +105 (new), `LANGUAGE_GUIDE.md` +48, agent report +101.
  Total ~1078 LOC.

### Lesson 1 streak: 27 consecutive clean agents

(M28 → M45). **Nine consecutive `tabular` package agents shipped
clean** — M37 / M38 / M39 / M40 / M41 / M42 / M43 / M44 / M45.
The M41/M42/M43/M44/M45 quintet validates the cadence-classification
pattern (shared-infra vs disjoint-handler) across multiple
milestones.

### After M45: what's left for `tabular`

What's done (M37-M45 inclusive):
- Sealed Column hierarchy + DataFrame core + IO + filter + sort
- Aggregations + group-by (single-col → single-col index;
  multi-col → MultiIndex)
- Reshape (unique / value_counts / concat / merge / pivot / melt /
  pivot_table) with index propagation
- Time series (cumulative / null handling / iloc / rolling /
  resample / asof_merge) single-col-index aware
- DatetimeIndex (M41-M43) with full propagation through 21 methods
- MultiIndex (M44-M45) with full propagation through 18 methods +
  multi-col group_by promotion

What's deferred to M46:
- **Outer-merge MultiIndex fallback** — replace M42's RangeIndex
  fallback for dtype-mismatched outer joins with NaN-padded
  MultiIndex.
- **`stack` / `unstack`** — pandas's MultiIndex bread-and-butter.
- **`df.loc[label_list]` / range-by-label** — extends M41's
  `select_by_label_*` from one-row to range + multi-key.
- **Time-series ops MultiIndex propagation** — `resample` /
  `asof_merge` / `resample_index` / `asof_merge_index` are
  single-col-index-only today.
- **`set_index([col])` accepting 1-element list** — ergonomics.
- **`pivot_table(aggfunc=List)` + `margins=True`** — small
  extensions.

What's deferred to M47+:
- Rolling-window optimizations (Welford for std, min_periods,
  center)
- Categorical column dtype
- `df.iloc[rows, cols]` 2-D indexing
- Negative-index support for iloc
- More resample rules (1w / 1M / 1Y — needs calendar layer)
- Desktop UI (the M37-design Phase 6 — webview-served or
  Tauri/wry hybrid)

---

## M44 — `tabular` MultiIndex (M44a: storage + multi-col group_by promotion + minimal propagation) (2026-05-22)

The headline architectural lift after M41-M43 closed the v1
single-index story. Adds nested indices for multi-column group_by
results. Single agent, 4 separable per-phase commits,
~1500-2000 LOC, **zero STOP CRITERIA cuts**. Explicitly scoped as
M44a; full propagation + stack/unstack + outer-merge MultiIndex
fallback + loc range stay as M44b.

The cleanest tabular-package integration in the series — main was
completely clean post-agent (no Edit-tool leaks). The agent's
precautionary `cp` workaround eliminated the leak entirely; see
"Methodology wins" below.

### Surface (extends `tabular` module)

**Phase A — MultiIndex storage + accessors + sort_index_multi**:
- DataFrame payload bumped 40 → 56 bytes for optional
  `index_levels: List[Column]?` + `index_names: List[str]?`
  (mutually exclusive with M41's single-col `index` / `index_name`).
- New constructor `m44_build_df_with_multiindex` parallels
  `m41_build_df_with_index` for the MultiIndex case.
- 6 new methods (NativeFns 1027-1032):
  `set_index_multi(cols: List[str]) -> DataFrame`,
  `reset_index_multi() -> DataFrame`,
  `index_nlevels() -> i64` (0/1/N),
  `index_level(i: i64) -> Column?`,
  `index_level_name(i: i64) -> str?`,
  `sort_index_multi(ascending: bool) -> DataFrame` (lexicographic
  across levels, stable).

**Phase B — Multi-column group_by promotion**:
- All 8 group_by aggregation methods (`size` / `keys` / `sum` /
  `mean` / `min` / `max` / `count` / `agg`) dispatch on
  `group_keys.length()`:
  - length 1 → M43's single-col promotion (today's behavior).
  - length ≥ 2 → new M44 MultiIndex path. All keys promoted to
    index levels; regular columns are just the aggregated values.

**Phase C — Minimal MultiIndex propagation (filter / head / tail / iloc)**:
- New helper `m44_permute_multiindex_into_df` auto-dispatches:
  - No index → `m37_build_df` (RangeIndex, today's behavior).
  - Single-col index → `m42_permute_index_into_df` (M42 path).
  - MultiIndex → permute each level by `keep_indices`, emit via
    `m44_build_df_with_multiindex`.
- 4 row-selection handlers (filter / head / tail / iloc) call the
  new helper at their emit site.

**Phase D**: 25 VM tests + 2 demo-runs +
`examples/tabular_multiindex_demo.spy` (~165 LOC) +
LANGUAGE_GUIDE.md §5 M44 subsection + new §11.32 (MultiIndex
propagation v1 scope-down) + §11.26 rewrite.

### EXPLICIT v1 scope-down (M44b anchor)

**MultiIndex propagation in M44a is limited to filter / head /
tail / iloc.** Every other op drops a MultiIndex back to
RangeIndex:
- M42 ops: sort_by, dropna, dropna_subset, fillna_*, merge,
  select, drop, rename
- M43 ops: pivot, melt, concat_rows, concat_cols, pivot_table
- M41 ops: sort_index, resample_index, asof_merge_index,
  select_by_label_* (these are single-col-only by design)

M44b's job: lift this restriction. Plus stack/unstack, `df.loc
[label_list]` range-by-label, outer-merge MultiIndex fallback
(replaces M42's RangeIndex fallback for dtype-mismatched indexes).

### Two methodology wins worth recording

**1. The precautionary `cp` workaround eliminated the Edit-tool
worktree leak entirely.**

After 7 consecutive milestones (M37-M43) seeing the leak with
varying severity (M40 ~2 min recovery / M41 ~30s / M42 ~5s /
**M43 ~90s across ~15 cp recoveries**), M44's brief recommended a
**precautionary `cp` of all shared files at session start**
syncing `vm/src/builtins.rs`, `compiler/src/resolver.rs`,
`compiler/src/ir.rs`, `shared/src/native.rs`, `LANGUAGE_GUIDE.md`
from project root to worktree.

The agent ran the `cp` block once at session start. **Zero
recoveries needed mid-session.** Plus, the orchestrator-side
integration saw zero leak — main was completely clean post-agent
(no source modifications to reset before the fast-forward
merge). The cleanest tabular-package integration in the series.

This is the standard mitigation pattern now. Future briefs that
involve bulk Edits to shared files will include the precautionary-
cp block. Harness-side root-cause investigation is deprioritized;
the workaround is cheap and effective.

**2. The shared-infra cadence exception worked cleanly when
classified explicitly.**

M41 was the first milestone to slip on per-phase cadence (combined
Phase A+B+C at ~75% of budget) because all three phases shared
`m41_build_df_with_index` + the 40-byte payload change. M44 is
structurally similar: Phase A introduces the new payload field +
constructor + 6 methods that every subsequent phase uses. **The
brief explicitly classified M44 as shared-infra and predicted a
30-50% first-commit window**.

Agent landed first commit at ~35% of budget — squarely inside
the predicted window. The M41/M42/M43/M44 quartet now confirms:
the shared-infra/disjoint-handler classification, when made
explicit in the brief with a first-commit threshold band, gets
the agent to the right cadence reliably.

### Tests flipped (1 total)

`vm/tests/m43_tabular_index_reshape.rs::multi_col_group_by_does_not_promote_to_index`
→ `multi_col_group_by_promotes_to_multiindex_m44`. Old:
`ncols=3, has=false` (keys retained as regular columns). New:
`ncols=1, nlev=2` (keys promoted to a 2-level MultiIndex).

**Zero M38 tests flipped** — M38's only multi-col group_by test
(`group_by_multi_column`) checks group count only, not column
shape.

### Tests + size

- Tests: 888 → 915 (+27 net: 25 new VM in
  `vm/tests/m44_tabular_multiindex.rs`, 2 new demo-runs in
  `compiler/tests/tabular_multiindex_demo_runs.rs`).
- Examples: 107 → 108
  (`examples/tabular_multiindex_demo.spy` ~165 LOC).
- Stdlib classes: unchanged at 18 (6 new methods, no new classes).
- LOC: `vm/src/builtins.rs` +622 (payload bump + new constructor +
  helpers + 6 method handlers + group_by rewrites + 4 emit-call
  swaps), `compiler/src/resolver.rs` +50 (DataFrame layout +
  method sigs), `compiler/src/ir.rs` +7 (dispatcher entries),
  `shared/src/native.rs` +40 (NativeFns 1027-1032), plus new
  tests / example / report.
- NativeFn IDs: 1027-1032 used (6 of the 50 reserved slots from
  the M40 era).

### Lesson 1 streak: 26 consecutive clean agents

(M28 → M44). **Eight consecutive `tabular` package agents shipped
clean** — M37 / M38 / M39 / M40 / M41 / M42 / M43 / M44, spanning
~10,000+ LOC of native Rust handler code across two architectural
extensions (M41 single-col index, M44 MultiIndex).

### After M44: what's left for `tabular`

What's done (M37-M44 inclusive):
- Sealed Column hierarchy + DataFrame core + IO + filter + sort
- Aggregations + group-by (single-col promoted to index)
- Reshape (unique / value_counts / concat / merge / pivot / melt /
  pivot_table)
- Time series (cumulative / null handling / iloc / rolling /
  resample / asof_merge)
- DatetimeIndex (single-col) with full propagation through 21
  methods
- MultiIndex with multi-col group_by promotion + minimal
  propagation (4 ops)

What's deferred to M44b:
- Full MultiIndex propagation through M42 + M43 ops (~14 handlers)
- `stack` / `unstack`
- `df.loc[label_list]` range-by-label
- Outer-merge MultiIndex fallback

What's deferred to M45+:
- Rolling-window optimizations (Welford for std, min_periods,
  center)
- Categorical column dtype
- `df.iloc[rows, cols]` 2-D indexing
- Negative-index support for iloc
- More resample rules (1w / 1M / 1Y — needs calendar layer)
- `pivot_table margins=True` + `aggfunc=list`
- Desktop UI (the M37-design Phase 6 — webview-served or
  Tauri/wry hybrid)

---

## M43 — `tabular` reshape index propagation (closes v1 single-index story) (2026-05-22)

Closes the v1 single-index propagation. M42 propagated the index
through 11 row/column-transforming methods; M43 finishes by making
the remaining "still drops index" methods (pivot_table, group_by +
agg, pivot, melt, concat_rows, concat_cols) index-aware. Single
agent, 4 separable per-phase commits, ~1715 LOC mostly modifying
existing handlers, **zero STOP CRITERIA cuts**. After M43 the
`tabular` package is **fully index-aware end-to-end for
single-column indexes**; MultiIndex remains M44+ work.

### Surface

- **Phase A**:
  - `pivot_table(index_col, columns_col, values_col, aggfunc)` —
    `index_col` becomes the result's index instead of a column.
  - **Single-column** `group_by([col])` with all aggregations
    (`sum / mean / min / max / count / agg / size / keys`) — the
    single group-key column becomes the result's index.
  - **Multi-column** `group_by([col1, col2])` retains today's
    keys-as-columns shape (deferred to M44 MultiIndex). Documented
    as the v1 contract.

- **Phase B**:
  - `pivot(index, columns, values)` — `index` value becomes the
    result's index (parallels `pivot_table` in Phase A).
  - `concat_rows(dfs)` — concatenates input indexes when all share
    dtype + name. Falls back to RangeIndex if any df lacks an
    index, or dtypes mismatch, or names mismatch.
  - `concat_cols(dfs)` — lhs's index wins (consistent with M42's
    merge policy).

- **Phase C**: `melt(id_vars, value_vars)` — if the input has an
  index, the output's index is the input's index **repeated
  `len(value_vars)` times** (one label per produced row, matching
  pandas's default). Preserves index name + dtype.

- **Phase D**: 18 VM tests + 2 demo-runs +
  `examples/tabular_index_reshape_demo.spy` (~190 LOC) +
  LANGUAGE_GUIDE.md §5 + §11.26 (now "fully index-aware for
  single-column indexes") + §11.28 + new §11.30 (melt index
  repetition) + §11.31 (concat_rows index reconciliation rules).

### Two methodology data points worth recording

**1. The test-flip cascade was 9 vs the brief's 2-4 estimate.**

The brief expected 2-4 M41/M42 tests to flip. Actual: 9 tests
across M38, M39, M41, plus 3 demo updates.

| Source | Flips | Reason |
|---|---:|---|
| M41 | 1 | `pivot_table_sum_happy_path` (ncols 3→2 + index checks) |
| M39 | 2 | `pivot_happy_path` + `pivot_missing_cell_is_null` |
| **M38** | **6** | All 6 group_by test cases asserted keys-as-columns; single-column group_by promotion cascaded into every one |
| Demos | 3 | `tabular_groupby_demo.spy` / `tabular_index_demo.spy` / `tabular_reshape_demo.spy` updated for new index column counts + sort_index calls |

**Generalizable lesson**: when a contract change is cross-cutting
(every group_by now promotes its key), the test-flip count scales
with how widely the old contract was tested in the milestone that
shipped it. M38's 6 group_by tests cascaded because group_by was
M38's headline feature. **Next brief that changes a broadly-tested
feature should grep existing tests for old-contract assertions
and estimate the flip count from that, not from intuition.**

**2. The Edit-tool worktree leak is broader than the M40 narrowing
claimed.**

M40-M42 thought the leak was Edit-on-existing-files only (Write
with absolute paths unaffected). M43 found **Write of new files
ALSO leaked** at first-edit-per-file boundaries. Recovery time:
M40 ~2 min, M41 ~30s, M42 ~5s, **M43 ~90s across ~15 cp recoveries**.
M43 agent recommended (and HANDOFF.md now adopts): **precautionary
`cp` of all shared files at session start** rather than per-phase
`git status` discovery loops. Defensive copy is cheap; per-phase
discovery is not. Cause unknown — not Edit-specific as the M40
hypothesis suggested; needs a focused harness investigation.

### Three findings worth noting

1. **Single-column detection** in group_by promotion is a single
   read of `group_keys.length()` at the top of each handler. If
   1, promote to index; if N, today's behavior. Tiny additional
   surface area (~3 lines per of 8 group_by handlers).
2. **`concat_rows` index reconciliation** required two distinct
   checks (dtype-match AND name-match). The agent went with strict
   reconciliation — if either differs, fall back to RangeIndex.
   No "best-effort" concatenation in v1; pandas's behavior is to
   raise on incompatible indexes, which is also a valid choice but
   would have broken more existing tests.
3. **`melt` index repetition** preserves dtype + name through the
   N × len(value_vars) row expansion. A vec-of-i64 input index
   becomes a longer vec-of-i64 output index with each input
   element repeated.

### Tests + size

- Tests: 868 → 888 (+20 net: 18 new VM in
  `vm/tests/m43_tabular_index_reshape.rs`, 2 new demo-runs in
  `compiler/tests/tabular_index_reshape_demo_runs.rs`, 9 M38/M39/M41
  flipped — old assertions deleted, new added).
- Examples: 106 → 107
  (`examples/tabular_index_reshape_demo.spy` ~190 LOC).
- Stdlib classes: unchanged at 18. NativeFn IDs: unchanged.
- LOC: `vm/src/builtins.rs` +295 (handler edits + 1 new helper
  `m43_concat_rows_index`), `vm/tests/m43_tabular_index_reshape.rs`
  +925 (new), `examples/tabular_index_reshape_demo.spy` +188 (new),
  `compiler/tests/tabular_index_reshape_demo_runs.rs` +96 (new),
  `LANGUAGE_GUIDE.md` +59, plus demo + test updates. Total ~1715
  LOC.

### Lesson 1 streak: 25 consecutive clean agents

(M28 → M43). **Seven consecutive `tabular` package agents shipped
clean** — M37 / M38 / M39 / M40 / M41 / M42 / M43, with the M41
shared-infra nuance and the M42 / M43 disjoint-handler return.

### After M43: the `tabular` v1 single-index surface is closed

What's done:
- 11 row/column-transforming methods propagate the index (M42).
- 4 explicitly-index-aware methods shape the index (M41:
  sort_index / resample_index / asof_merge_index /
  select_by_label_*).
- 6 reshape + group methods propagate or promote the index (M43:
  pivot_table / single-col group_by / pivot / melt / concat_rows /
  concat_cols).

What's deferred to M44+:
- **MultiIndex** — currently the index is a single column. Real
  pandas's nested indices for stack/unstack/groupby.agg-multikey.
  Headline missing piece; substantial architectural lift.
- **Multi-column group_by promotion** — waits for MultiIndex.
- **Outer-merge MultiIndex fallback** — replaces M42's current
  RangeIndex fallback for dtype-mismatched indexes.
- **`df.loc[label_list]` / range-by-label** — currently
  `select_by_label_*` is one-row only.
- **`set_index([col1, col2])`** — waits for MultiIndex.
- **`stack` / `unstack`** — pandas's MultiIndex bread-and-butter.

---

## M42 — `tabular` index propagation through existing methods (2026-05-22)

Closes the M41 explicit v1 scope-down. The 11 existing DataFrame
methods that returned a fresh frame (filter / sort_by / head /
tail / iloc / select / drop / rename / dropna / dropna_subset /
fillna_* / merge) now **propagate the index** instead of dropping
it. Single agent, **5 separable per-phase commits** (Phase A
through Phase E), ~700-1000 LOC mostly modifying existing handlers,
**zero STOP CRITERIA cuts**.

### The pattern — one helper applied 11 times

The whole milestone is a single recipe:

```rust
// Before M42: each handler ended with
m37_build_df(interp, names, permuted_columns)
//
// After M42: each handler ends with
m42_permute_index_into_df(interp, parent_df_ptr, names,
                          permuted_columns, &keep_indices)
```

The helper reads the parent's optional index, permutes it by the
same `keep_indices` vector the handler already builds, and emits
via `m41_build_df_with_index` (or `m37_build_df` if the parent had
no index — preserving today's behavior for unindexed inputs). 280
LOC added to `vm/src/builtins.rs` total: 4 helpers + 11 emit-call
swaps. **No new NativeFn IDs** — M42 modifies existing handlers,
not adds.

### Surface (extends `tabular` module's existing methods)

- **Phase A** (filter, sort_by, head, tail, iloc): `m42_permute_index_into_df`
  + 5 handler edits. The row-selection vectors already exist in
  each handler; M42 plumbs the index through them.
- **Phase B** (select, drop, rename): sibling helper
  `m42_copy_index_into_df` (no permutation needed — these methods
  don't touch rows) + 3 handler edits.
- **Phase C** (dropna, dropna_subset, fillna_*): 2 handler edits
  (fillna's per-dtype dispatch via the shared `m40_df_fillna`
  body — one edit threads through all 5 `fillna_*` variants).
- **Phase D** (merge): `m42_merge_build_index` +
  `m42_merge_outer_index_column`. **Index policy per `how`**: lhs
  wins for inner / left / outer; rhs wins for right. Outer with
  dtype mismatch falls back to RangeIndex (v1 simplification;
  pandas's NaN-padded MultiIndex is M43+ work).
- **Phase E**: 19 VM tests + 2 demo-runs +
  `examples/tabular_index_propagation_demo.spy` (~210 LOC
  end-to-end pipeline that set_indexes a frame, filters, sorts,
  drops nulls, fills, merges, projects, and verifies the index
  threaded through everything) + LANGUAGE_GUIDE.md §5 + §11.26
  rewrite (the M41 v1 scope-down section is now "closed by M42").

### Three findings worth recording

1. **The M41/M42 cadence contrast confirms the streak nuance**. M41
   slipped to combined Phase A+B+C because all three phases shared
   `m41_build_df_with_index` + the 40-byte payload (shared
   infrastructure → splitting becomes revert-and-reapply). M42
   returned to clean per-phase commits because its phases modify
   disjoint handlers (each phase touches a different subset of
   handlers, no shared revert risk). **Generalizable lesson now
   confirmed across two milestones**: brief language should call
   out "shared-infra" vs "disjoint-handler" so the agent aims at
   the right cadence.
2. **Merge index policy is per-`how`** — lhs wins for inner / left
   / outer; rhs wins for right. Outer-with-dtype-mismatch falls
   back to RangeIndex (no NaN-padded MultiIndex in v1). Documented
   in §11.29.
3. **Edit-tool worktree leak hit 5 times in M42** (back up from 1×
   in M41) because M42 made many small Edits to `builtins.rs`
   across 5 phase commits — each "first Edit on a shared file"
   per-phase triggered the leak. Each recovered via a single `cp`
   (~5 seconds total). Cause narrowing from M40 (Edit-on-existing-
   files leaks; Write with absolute paths doesn't) confirmed across
   6 milestones now.

### M41 tests flipped (1)

`vm/tests/m41_tabular_index.rs::filter_drops_index` was renamed to
`filter_preserves_index_m42` with its assertion flipped from
`has_index == false` to `has_index == true`. The test now verifies
the M42 behavior on the same input shape it used to verify the M41
v1 scope-down. **This is the first test in the project's history
that an agent intentionally flipped to verify a behavior change**
— all prior test work was additive.

### Tests + size

- Tests: 847 → 868 (+21 net: 19 new VM tests in
  `vm/tests/m42_tabular_index_propagation.rs`, 2 new demo-runs in
  `compiler/tests/tabular_index_propagation_demo_runs.rs`, 1 M41
  test flipped — old assertion deleted, new assertion added).
- Examples: 105 → 106
  (`examples/tabular_index_propagation_demo.spy` ~210 LOC).
- Stdlib classes: unchanged at 18. NativeFn IDs: unchanged
  (no new methods).
- LOC: `vm/src/builtins.rs` +283 (the 4 helpers + 11 emit-call
  swaps), `vm/tests/m42_tabular_index_propagation.rs` +940 (new),
  `examples/tabular_index_propagation_demo.spy` +210 (new),
  `compiler/tests/tabular_index_propagation_demo_runs.rs` +99
  (new), `LANGUAGE_GUIDE.md` +58, `docs/thesis/agent_reports/
  m42_tabular_index_propagation.md` +103. Total ~1693 LOC.

### Lesson 1 streak: 24 consecutive clean agents

(M28 + M28.5 + M29 + M29.5 + M30×2 + M31 + M32 + M33 + M34 + M35×3 +
M36 + M37 + M38 + M39 + M40 + M41 + M42). **Six consecutive
`tabular` package agents shipped clean** — M37 / M38 / M39 / M40 /
M41 / M42.

### After M42: the `tabular` v1 single-index surface is closed

What's done:
- 11 row/column-transforming methods preserve the index (M42).
- 4 explicitly-index-aware methods shape the index (M41:
  sort_index / resample_index / asof_merge_index /
  select_by_label_*).

What still drops the index (M43 anchor list):
- `pivot_table` — `index_col` should become the result's index.
- `group_by` + agg — group-key column should become the result's
  index (single-column first; multi-column requires MultiIndex).
- `pivot` / `melt` — case-by-case design.
- `concat_rows` / `concat_cols` — case-by-case (likely take lhs's
  index).

What's deferred to M44+:
- MultiIndex — currently the index is a single column. Real Pandas
  nested indices for stack/unstack/groupby.agg.
- `df.loc[label_list]` / range-by-label.
- Outer-merge MultiIndex fallback (replaces M42's RangeIndex
  fallback for dtype-mismatched indexes).

---

## M41 — `tabular` Phase 5b: DatetimeIndex (minimum viable) + pivot_table (2026-05-22)

Phase 5b of the Pandas plan. After M40 deferred DatetimeIndex to
keep scope manageable, M41 ships the minimum viable index
abstraction plus pandas's most-loved DataFrame method. Single
agent, 2 commits (the first per-phase-cadence slip in the streak —
see "Methodology nuance" below), ~2193 LOC across 9 files, **zero
STOP CRITERIA cuts**.

The architectural change worth highlighting: **DataFrame payload
grew 24 → 40 bytes** to carry an optional `index: Column?` +
`index_name: str?` (both zero = the existing RangeIndex default).
Three existing DataFrame constructors (`m37_from_columns`,
`m37_from_rows`, `m37_build_df`) updated to allocate the larger
payload. The GC's Class scanner walks every 8-byte slot in the
payload — zero slots are safely treated as "not pointers" (matches
the M11 pointer-vs-i64 false-positive analysis; benign because the
mark-phase is purely additive).

### Surface (extends `tabular` module)

- **Phase A — index storage + accessors + sort**: `df.set_index(col)` /
  `reset_index()` / `has_index()` / `index() -> Column?` /
  `index_name() -> str?` / `sort_index(ascending)`. `set_index`
  clones the column rather than aliasing (keeps the index physically
  independent — costs one extra column allocation, safe for v1 row
  counts).

- **Phase B — index-aware time-series + per-dtype select-by-label**:
  - `df.resample_index(rule, agg)`: mirrors M40's `resample` but
    reads bucket keys from the index (must be `ColumnDateTime`).
    Output preserves its own bucket-start index — one of the four
    methods that propagate the index.
  - `df.asof_merge_index(other)`: mirrors `asof_merge` but joins
    on both frames' indexes. Output preserves self's index.
  - `df.select_by_label_{i64, str, datetime}(label) -> DataFrame?`:
    returns a one-row frame or `none`. Duplicate labels return the
    first matching row (documented in §11.27).

- **Phase C — `pivot_table`**: `df.pivot_table(index_col,
  columns_col, values_col, aggfunc)`. Pandas's pivot + group-by +
  agg in one call. Per-cell accumulator implemented as a single
  `Acc` enum (variant per dtype × agg) so the per-bucket update is
  a single `match` (vs. nested dispatch). Aggfunc vocabulary
  `sum/mean/min/max/count` (same as M38). Output uses RangeIndex
  (per the explicit v1 scope-down).

- **Phase D**: 23 VM tests + 2 demo-runs + `examples/tabular_index_
  demo.spy` (~180 LOC: trades → set_index → resample_index →
  sort_index → pivot_table → asof_merge_index → select_by_label_str
  → reset_index pipeline) + LANGUAGE_GUIDE.md §5 M41 subsection +
  §11.26-§11.28 gotchas.

### EXPLICIT scope-down (M42 anchor)

**Every existing DataFrame method that returns a fresh frame
DROPS the index in v1** — only the 4 explicitly-index-aware methods
(sort_index, resample_index, asof_merge_index, select_by_label_*)
preserve it. M42's job: index propagation through
filter / sort_by / head / tail / iloc / dropna / fillna / merge /
select / drop / rename. Per the M41 agent's report, ~600-800 LOC
concentrated in 6 existing handlers; each gains: (a) read parent
index + index_name, (b) permute the index by the same row-selection
vector, (c) emit via `m41_build_df_with_index` instead of
`m37_build_df`. Permutation logic is identical to what's already
in those handlers — the only new line is the index-permute + emit
call.

### Five findings worth recording

1. **DataFrame payload bump to 40 bytes** — GC scanner walks every
   8-byte slot; zero slots safely treated as "not pointers" via the
   M11 pointer-vs-i64 false-positive analysis (benign — mark-phase
   is additive). Three constructors updated; existing callers don't
   need to know about the larger payload.
2. **`sort_index` dispatch by index dtype** — single
   `m41_sort_index_perm(col, ascending)` helper reads class name +
   runs per-dtype comparator inline. Descending = ascending +
   `perm.reverse()` (preserves stability within non-null cells).
3. **`m41_clone_column` for the index slot** — `set_index` clones
   the column rather than aliasing. Cost: one extra column
   allocation per `set_index`; safe for v1 row counts.
4. **`pivot_table` accumulator as an enum** — single `Acc` enum
   carries variant-per-(dtype × agg) accumulators. Per-bucket
   update is a single `match` (vs. nested dispatch).
5. **Edit-tool worktree leak recurred once** (down from 5× in M39,
   2× in M40). Confirms the M40 narrowing: `Edit` on already-
   existing files leaks; `Write` with absolute paths is unaffected.
   Agent caught via `git status` + `cp`-recovered in ~30 seconds.

### Methodology nuance worth flagging

M41 deviated from the per-phase-commit discipline: Phases A+B+C
landed as one combined commit at ~75% of budget (rather than the
brief's 20% first-commit + per-phase target). Honest reason: all
three phases share `m41_build_df_with_index` + the 40-byte payload
change — splitting would have required revert-and-reapply with
extra leak-recovery overhead. The Lesson 1 SPIRIT (commit before
orchestrator intervenes, green build + tests passing at each
commit) held; both M41 commits were clean. **The streak (23) does
not break**, but commit granularity slipped.

**Generalizable lesson**: when phases share cross-cutting
infrastructure (struct layout changes, new shared helpers),
per-phase splitting becomes an antipattern. Future briefs for
"cross-cutting infra + downstream uses" rounds should accept
"first commit after the infrastructure lands, even if late" as
the right shape — and explicitly tell the agent that
infrastructure-then-uses can land as one commit. This is the
**first explicit nuance to the Lesson 1 brief language since the
M28 escalation**.

### Tests + size

- Tests: 822 → 847 (+25: 23 in `vm/tests/m41_tabular_index.rs`,
  2 in `compiler/tests/tabular_index_demo_runs.rs`).
- Examples: 104 → 105 (`examples/tabular_index_demo.spy` ~180 LOC).
- Stdlib classes: unchanged at 18 (M41 adds methods + an optional
  DataFrame field, no new classes).
- LOC: `vm/src/builtins.rs` +1033, `compiler/src/resolver.rs` +84,
  `shared/src/native.rs` +76, `compiler/src/ir.rs` +43, plus new
  tests / example / report. Total ~2193 LOC.
- NativeFn IDs: 1015–1026 used (12 of the 50 reserved slots); 38
  remain for M42+.

### Lesson 1 streak: 23 consecutive clean agents

(M28 + M28.5 + M29 + M29.5 + M30×2 + M31 + M32 + M33 + M34 + M35×3 +
M36 + M37 + M38 + M39 + M40 + M41). **Five consecutive `tabular`
package agents shipped clean** — M37 / M38 / M39 / M40 / M41,
covering Phases 1–5b of the Pandas plan.

### After M41: the v0.3 narrows further

What remains of the originally-scoped v0.3 work:

- **M42 — index propagation through existing methods**: the M41
  anchor. ~600-800 LOC across 6 existing handlers.
- **Real Cranelift safepoint stack maps**: waiting on
  `cranelift-jit` API stability.
- **Real `mio` event loop**: replaces M32 thread-backed Future
  façade; closes the M29 framework's ~2× gap to Flask+gunicorn.
- **Edit-tool worktree leak investigation**: cause narrowed in
  M40 to Edit-on-existing-files; harness investigation is
  more tractable now.
- **`tabular` Phase 6 desktop UI**: webview-served or Tauri/wry
  hybrid.

The `tabular` package now spans Phases 1–5b of the original Pandas
plan. M42 closes the index propagation gap; M43+ picks up
categorical columns, rolling-window Welford optimizations, more
resample rules, and `df.iloc[rows, cols]` 2-D indexing.

---

## M40 — `tabular` Phase 5: time series + cumulative + null + iloc (2026-05-22)

Phase 5 of the Pandas plan. After M37+M38+M39 shipped the
common-80% surface (types + IO + filter + sort + aggregations +
group-by + reshape), M40 closes the time-series and null-handling
ops that real workflows hit constantly. Single agent, 4 phase
commits, ~2175 LOC, **zero STOP CRITERIA cuts**. **DatetimeIndex
deferred** — would require adding `index: Column` to DataFrame +
index-bearing variants of every existing op (~2-3× M40 scope);
M40's time-series ops take a column-name argument instead.

A previous launch attempt died on a transient 529 (API overloaded)
within ~3.5 minutes before any work was done. The successful
implementation is the second attempt. Worth flagging as a
methodology note: transient API failures are not state hazards
because no commits or worktree state are created if the agent
crashes before its first tool call.

### Surface (extends `tabular` module)

- **Phase A — cumulative + null handling + range slicing**:
  - Per-column cumulative on numeric: `ColumnI64.cumsum() / cumprod() /
    cummax() / cummin()`; same 4 on `ColumnF64` (8 NativeFns).
    Null-propagation rule: once a null is hit, every output cell
    after it is null (simpler than pandas's `min_periods=1`).
  - Whole-frame null: `df.dropna()` + `df.dropna_subset(cols)`; per-
    dtype `df.fillna_i64(v) / fillna_f64(v) / fillna_str(v) /
    fillna_bool(v) / fillna_datetime(v)` (7 NativeFns).
  - Range slicing: `df.iloc(start, stop)` — half-open, no negative
    indices in v1.

- **Phase B — rolling windows**: `ColumnI64` / `ColumnF64` ×
  `rolling_sum / rolling_mean / rolling_min / rolling_max /
  rolling_std` (10 NativeFns). Output length = input; cells
  0..window-1 are null (matches pandas's `min_periods=window`);
  any null in a window produces null output. Mean and std return
  `ColumnF64` regardless of input dtype. Sample n-1 std.

- **Phase C — time-series ops**:
  - `df.resample(time_col, rule, agg) -> DataFrame`. Rule parser
    accepts `<i64><m|h|d>` (e.g. `"15m"`, `"1d"`); week/month/year
    need a calendar layer (M41). Aggregations: `sum` / `mean` /
    `min` / `max` / `count`. Empty buckets emit non-null bucket-
    start times but null aggregated cells. String + bool source
    columns are silently dropped.
  - `df.asof_merge(other, on_self, on_other) -> DataFrame`. Left-
    join where each self row matches the largest other row with
    `other[on_other] <= self[on_self]`. Uses `Vec::partition_point`
    for O(log n) per-row matching after stable-sorting rhs. Both
    keys must share dtype (`ColumnDateTime` or `ColumnI64`).

- **Phase D**: 26 VM tests + 2 demo-runs + `examples/tabular_
  timeseries_demo.spy` (~170 LOC: fillna → cumsum → cummax →
  rolling_mean → resample → asof_merge → iloc → dropna pipeline)
  + LANGUAGE_GUIDE.md §5 M40 subsection + §11.22-§11.25 gotchas.

### Six findings worth recording

1. **Cumulative null-propagation choice** — "propagate from first
   null forward" is simpler than pandas's `min_periods=1` and
   trivially overridable user-side via `col.fill_null(0).cumsum()`.
   Documented as §11.22.
2. **Resample rule parser** is `<i64><m|h|d>` only. Week / month /
   year would need a calendar layer and don't fit a single-rule-
   width bucket model anyway. v1 has explicit `ValueError` messages
   for unrecognized rule formats.
3. **`asof_merge` binary search** uses
   `Vec::partition_point(|k| *k <= needle)` which returns the first
   index past the run of matches — the largest matching index is
   `pp - 1`, and `pp == 0` cleanly maps to "no match" (null right-
   side). Stable sort over rhs ensures duplicate keys preserve
   original row order.
4. **`fillna_*` pass-through** — non-matching-dtype columns are
   returned by raw pointer reuse (not copied). Safe because no
   codepath mutates Column payloads in place.
5. **Resample drops string + bool columns** — no defined v1
   aggregation for them. Could add `"first"` / `"last"` / `"mode"`
   later; v1 keeps the aggregation set numeric-only + `count`.
6. **Edit-tool worktree leak — key new finding**: confirmed-
   recurring across M37+M38+M39+M40 (now 4 milestones), but M40
   **narrowed the cause**. The leak is specific to `Edit` calls on
   already-existing files — `Write` calls (which take absolute
   worktree paths) land correctly. Agent recovered M40 leaks in
   ~2 minutes total via `cp` from project root to worktree. The
   interim workaround for M41+ briefs is to check `git status`
   after bulk Edits to shared files (resolver.rs / ir.rs /
   builtins.rs / native.rs); `Write` for new files is unaffected.
   This is the first time we've narrowed the leak's cause across
   four milestones of observation.

### Tests + size

- Tests: 794 → 822 (+28: 26 in `vm/tests/m40_tabular_timeseries.rs`,
  2 in `compiler/tests/tabular_timeseries_demo_runs.rs`).
- Examples: 103 → 104 (`examples/tabular_timeseries_demo.spy`
  ~170 LOC).
- Stdlib classes: unchanged at 18 (M40 adds methods, not classes).
- LOC: `vm/src/builtins.rs` +939, `shared/src/native.rs` +120,
  `compiler/src/resolver.rs` +114, `compiler/src/ir.rs` +61, plus
  new tests / example / report. Total ~2175 LOC.
- NativeFn IDs: 985–1012 used (28 of the 50 reserved slots); 22
  remain for v0.4.

### Lesson 1 streak: 22 consecutive clean agents

(M28 + M28.5 + M29 + M29.5 + M30×2 + M31 + M32 + M33 + M34 + M35×3 +
M36 + M37 + M38 + M39 + M40). **Four consecutive `tabular` package
agents shipped clean** — M37 / M38 / M39 / M40, each 4–5 phase
commits across ~2100–2800 LOC.

### After M40: the v0.3 narrows

What remains of the originally-scoped v0.3 work:

- **DatetimeIndex** — the architecturally substantial piece deferred
  from M40. M41 anchor.
- **Real Cranelift safepoint stack maps** — replaces M33 shadow
  stack; waiting on `cranelift-jit` API stability.
- **Real `mio` event loop** — replaces M32 thread-backed Future
  façade; closes the M29 framework's ~2× gap to Flask+gunicorn.
- **Edit-tool worktree leak investigation** — now narrowed to Edit-
  on-existing-files; the harness investigation gets easier with
  this data point.
- **`tabular` Phase 6 desktop UI** — webview-served or Tauri/wry
  hybrid. The compute backend is settled; the UI is the open
  surface.

The `tabular` package now spans Phases 1-5 of the original Pandas
plan. Phase 6 (desktop UI) is the headline remaining design
question; everything else is incremental.

---

## M39 — `tabular` Phase 4: reshape ops (2026-05-22)

Phase 4 of the Pandas plan. After M37+M38 shipped core types + IO +
filter + sort + aggregations + group-by, M39 ships reshape: unique
per dtype, value_counts, concat (rows + cols), merge with all four
hash-join modes, pivot, and melt. **Zero STOP CRITERIA cuts** —
every brief item shipped. After M39 the `tabular` module covers the
common-80% of pandas workflows.

Single agent, 4 phase commits (one fewer than M37/M38's 5 — combined
tests + demo + docs into the final Phase D).

### Surface (extends `tabular` module)

- **Phase A**: 5 typed `df.unique_*` accessors (i64/f64/str/bool/datetime)
  mirroring the M38 `get_column_*` shape; `df.value_counts(col) -> DataFrame`
  (2-col: value + count, sorted by count desc); module-level
  `tabular.concat_rows(dfs)` (vertical, schema-strict) and
  `tabular.concat_cols(dfs)` (horizontal, row-count-strict +
  unique-column-name-strict).
- **Phase B**: `df.merge(other, on, how)` — hash-join with all 4
  modes (`inner` / `left` / `right` / `outer`). Reuses M38's
  `\x01`-joined per-cell key encoding. Output columns = lhs cols +
  rhs non-`on` cols (no duplicates). Null cells in `on` columns
  never match (pandas/SQL `null != null`).
- **Phase C**: `df.pivot(index, columns, values)` (long→wide; raises
  on duplicate (index, columns) pairs; missing pairs → null cells);
  `df.melt(id_vars, value_vars)` (wide→long; all `value_vars` must
  share a dtype).
- **Phase D**: 23 VM tests + 2 demo-runs tests; `examples/tabular_
  reshape_demo.spy` (~150 LOC orders+customers workflow);
  LANGUAGE_GUIDE.md §5 + §11.20 + §11.21 updates.

### Five findings worth recording

1. **f64 `unique` keys on `to_bits()`** — `HashSet<f64>` doesn't
   compile (`f64: !Hash`); bit-pattern keying distinguishes ±0.0
   and lets multiple NaN payloads be distinct. Canonical workaround
   and also gives bitwise-identical first-occurrence semantics.
2. **`m39_join_key` returns `None` for any-null-cell rows** —
   different from M38's `m38_row_key` which encoded nulls as
   `\x02null` for grouping. For merge's `null != null` semantics,
   short-circuiting to None is cleaner than emitting a key that
   can never match anything.
3. **Merge `on` columns inherit rhs values on right-only outer rows**
   — pandas's "merged key column" behavior. The `rhs_fallback_idx`
   path in `m39_pluck_column` fills the `on` column from the rhs
   cell when the lhs side is None, so the key column never goes
   null in outer/right outputs.
4. **Melt's column-buffer machinery is bulky** — each dtype needs
   per-value-var read + per-output-row write. Pre-read all
   `value_vars` columns into per-var `Vec<>`s up front; do the
   `(row, var)` emit in one loop. Less elegant than a closure
   approach but avoids virtual-call-per-cell overhead.
5. **Edit-tool worktree leak recurred ~5 times in M39** — same as
   M37+M38. Agent caught each via `git status` showing no diff
   after substantial edits; `cp`-recovered from project root to
   worktree. **This is now confirmed-recurring across 3 consecutive
   milestones** — methodology note: parallel-worktree agents are
   reliable on commit discipline but unreliable on file-write-target
   discipline. Orchestrator integration workaround (checkout-and-
   merge-ff against worktree HEAD) is reliable and documented in
   HANDOFF.md.

### Tests + size

- Tests: 769 → 794 (+25: 23 in `vm/tests/m39_tabular_reshape.rs`,
  2 in `compiler/tests/tabular_reshape_demo_runs.rs`).
- Examples: 102 → 103 (`examples/tabular_reshape_demo.spy` ~150 LOC).
- Stdlib classes: unchanged at 18 (M39 adds methods, not classes —
  all reshape ops return existing `DataFrame` or `Column<T>`).
- LOC: `vm/src/builtins.rs` +1101, `compiler/src/resolver.rs` +67,
  `shared/src/native.rs` +61, `compiler/src/ir.rs` +43, plus new
  tests / example / report. Total ~2430 LOC.
- NativeFn IDs: 935-942 (Phase A) + 945 (Phase B) + 950-951
  (Phase C). 39 of the 50 reserved slots remain for v0.4.

### Lesson 1 streak: 21 consecutive clean agents

(M28 + M28.5 + M29 + M29.5 + M30×2 + M31 + M32 + M33 + M34 + M35×3 +
M36 + M37 + M38 + M39). Three consecutive Pandas-package agents
shipped clean.

### After M39: the `tabular` package landscape

The `tabular` module is now feature-comparable to a v0.0.1 pandas:

- **M37**: 6 classes (sealed Column + 5 subclasses + DataFrame),
  factories, IO (CSV + SQL), filter/select/drop/head/tail, sort_by
- **M38**: typed `get_column_*`, restored cmp ops, aggregations
  (sum/mean/min/max/count/std/var/median), `describe`, `fill_null`,
  `from_dict`, `GroupedDataFrame` + group-by + agg shortcuts
- **M39**: `unique_*`, `value_counts`, `concat_rows`/`concat_cols`,
  `merge` (inner/left/right/outer hash-join), `pivot`, `melt`

M40 picks up Phase 5 (DatetimeIndex / rolling / resample / asof_merge
/ cumsum/cumprod/cummax/cummin / dropna / fillna / iloc range
slicing). Phase 6 (desktop UI — webview-served or Tauri/wry hybrid)
follows after Phase 5 lands.

---

## M38 — `tabular` round-out: aggregations + group-by (2026-05-22)

Round-out of the M37 `tabular` module. Picks up the M37 STOP CRITERIA
debt and adds aggregations + hash-based group-by — the foundation for
M39's pivot/melt/join. Single big agent, 5 phase commits, **zero
STOP CRITERIA cuts**. Largest single-agent milestone with full
feature delivery to date.

### Surface (extends `tabular` module)

- **Phase A**: typed `df.get_column_i64 / f64 / str / bool / datetime`
  accessors (resolves the M37 sealed-class-return-type finding) + restored
  Phase C ops (`between` / `ne` / `ge` / `le` on numeric, `starts_with` /
  `ends_with` on str, `df.rename`).
- **Phase B**: per-column aggregations — `sum / mean / min / max /
  count / std / var / median` on numeric; `min / max / count` on str
  + datetime; `count` on bool. Sample n-1 std/var. Null-skipping
  throughout; NaN propagation on f64 (matches `numpy.sum` not
  `numpy.nansum`).
- **Phase C**: `df.describe() -> DataFrame` (count/mean/std/min/50%/max
  for numeric; count for non-numeric); `Column.fill_null(v)` per
  subclass (5 methods); `tabular.from_dict(d: Dict[str, Column])`
  constructor.
- **Phase D**: new `GroupedDataFrame` class registered via M36 path
  (no prelude bloat — 2nd stdlib class on the canonical M36 path
  after M37's 6 classes); `df.group_by(cols) -> GroupedDataFrame`;
  shortcuts `size / keys / sum / mean / min / max / count`; custom
  `agg(specs: List[Tuple[str, str]])`. Hash-based with `\x01`-joined
  multi-column keys.
- **Phase E**: 25 new tests + `examples/tabular_groupby_demo.spy`
  (~110 LOC) + LANGUAGE_GUIDE.md §5 / §6.2 / §11.18 / §11.19 updates.

### Four findings worth recording

1. **`Dict` has no insertion order** — M5's `Dict` is a `HashMap`.
   `tabular.from_dict` lex-sorts column names by key. Documented as
   LANGUAGE_GUIDE.md §11.19.
2. **NaN propagation on f64 aggregations** — matches `numpy.sum` (NaN
   propagates) NOT `numpy.nansum` (skips NaN). Nulls ARE skipped; NaN
   values are NOT. Documented as §11.18.
3. **Null-keyed group bucket** — rows with a null in any group-key
   column go into a synthesized null-group bucket (pandas's
   `dropna=False` mode).
4. **Edit-tool worktree leak (recurring methodology issue)** — same as
   M37, the agent's Edit tool writes leaked into the project-root
   copy of files mid-implementation. Agent recovered with a `cp -r`
   patch from worktree to project-root. Orchestrator workaround now
   recorded in HANDOFF.md: during integration, `git checkout --`
   main's partial modifications and `git merge --ff-only` the worktree
   branch. **This is the first non-Lesson-1 issue to repeat across
   M37 + M38 — worth a methodology note**: parallel-worktree agents
   are reliable on the *commit* discipline but unreliable on the
   *file-write-target* discipline when both worktree and project root
   are open in the orchestrator's view.

### Tests + size

- Tests: 744 → 769 (+25: 23 in `vm/tests/m38_tabular_ops.rs`, 2 in
  `compiler/tests/tabular_groupby_demo_runs.rs`).
- Examples: 101 → 102 (`examples/tabular_groupby_demo.spy` ~110 LOC).
- Stdlib modules: unchanged at 38 (extends M37's `tabular`).
- Stdlib classes: 17 → 18 (`GroupedDataFrame`).
- LOC: `vm/src/builtins.rs` +1246, `compiler/src/resolver.rs` +271,
  `shared/src/native.rs` +184, `compiler/src/ir.rs` +94, plus new
  tests / example / report. Total ~2530 LOC.

### Lesson 1 streak: 20 consecutive clean agents

(M28 + M28.5 + M29 + M29.5 + M30×2 + M31 + M32 + M33 + M34 + M35×3 +
M36 + M37 + M38). Two consecutive ~2500+ LOC big-bang milestones
delivered clean.

### M37 + M38 together: the v0.3 stdlib "package" template

What started as M34's "ship one class family in the prelude" pattern
has now scaled to a full Pandas-shaped data package:

- **M34**: JsonValue (7 classes, prelude — scope-down infrastructure)
- **M35**: Pattern + Connection + Cursor + Hasher (4 classes,
  prelude — parallel three-agent round)
- **M36**: `StdlibItemKind::Class` infrastructure
- **M37**: `tabular` core (6 classes, module-scoped — first canonical
  v0.3 stdlib package, Phase 1+2 of Pandas plan)
- **M38**: `tabular` round-out (1 class, module-scoped — Phase 3 of
  Pandas plan)

M39 picks up Phase 4 (pivot / melt / join). The shape now scales
linearly: one focused agent per package phase, the M36 canonical
class-registration path means no resolver coordination needed,
NativeFn IDs disjoint by 50 per round, distinctive variable
prefixes per round.

---

## M37 — `tabular` stdlib module (first Pandas-shaped package) (2026-05-21)

The first stdlib package for tabular data — a from-scratch native
DataFrame library following the M34 sealed-class pattern. Module
name `tabular` (not `pandas`, to avoid the confusion of `import
pandas` not meaning real pandas — see LANGUAGE_GUIDE.md §11.11).

**Significance**: first stdlib package to register classes via the
post-M36 `StdlibItemKind::Class` canonical path. No prelude bloat;
classes available only via `from tabular import DataFrame, ColumnI64`
or `tabular.DataFrame` as annotation type. End-to-end validation of
the M36 infrastructure refactor.

### Surface (`tabular` stdlib module)

6 classes registered module-scoped:

- `sealed class Column` + 5 final subclasses: `ColumnI64` /
  `ColumnF64` / `ColumnStr` / `ColumnBool` / `ColumnDateTime`
- `final class DataFrame`

**NA semantics**: each Column stores `values: List[T] +
nulls: List[bool]` of equal length. Uniform across dtypes; no NaN
sentinel games; integrates with `is not none` narrowing on the
typed accessor methods (`ColumnI64.get(i) -> i64?`).

### Phases (5 commits, all clean)

- **Phase A** — `tabular.col_*` factories, `from_columns`, `from_rows`;
  inspection methods (`shape / columns / dtypes / get_column /
  has_column`); ASCII `show()` table.
- **Phase B** — I/O: `read_csv` / `write_csv` / `from_sql` (reuses
  M35 typed Cursor!) / `from_rows`. Schema-driven parsing; empty
  cells → null.
- **Phase C** — per-column comparisons (i64+f64: `eq` / `gt` / `lt`;
  str: `eq` / `contains`; bool: `eq`; datetime: `eq` / `gt` / `lt`)
  producing null-aware ColumnBool masks; mask combinators
  (`and_` / `or_` / `not_` / `count_true` / `count_false` /
  `count_null`); `df.filter` / `select` / `drop` / `head` /
  `tail` / `row`.
- **Phase D** — stable `df.sort_by(col, ascending)` with
  nulls-at-end (pandas default), per-Column-type comparator dispatch.
- **Phase E** — 19 VM tests + 2 compiler integration tests +
  `examples/tabular_demo.spy` (~130 LOC) + LANGUAGE_GUIDE.md §5
  + §6.2 + §11 updates + agent report.

### STOP CRITERIA invoked in Phase C

Cut `between` / `ne` / `ge` / `le` (i64+f64), `starts_with` (str),
`DataFrame.rename`. Saved ~10 NativeFn slots. Kept set covers the
common 80% of filtering use cases; M38 picks the rest up.

### Three findings worth knowing

1. **`(*hdr).vtable` not `.ty`**: the ObjectHeader field rename
   caught the agent in early Phase A. Build errors were clean and
   pointed straight at the issue.
2. **No `get_column(name) -> Column?`**: the sealed-class return
   type can't be cleanly chosen at NativeFn time. Demo works
   around this by holding typed Column references from construction.
   M38 will add typed `get_column_i64` / `get_column_str` / etc.
3. **No bare-name fallback for tabular classes**: confirms the
   M36 refactor's promise. Users MUST write `from tabular import
   DataFrame`; bare-name access only works after explicit import.
   This is the post-M36 canonical behavior — M34/M35 classes
   still have the legacy bare-name fallback for back-compat.

### Tests + size

- Tests: 723 → 744 (+21: 19 in `vm/tests/m37_tabular.rs`, 2 in
  `compiler/tests/tabular_demo_runs.rs`).
- Examples: 100 → 101 (`examples/tabular_demo.spy` ~130 LOC).
- Stdlib modules: 37 → 38 (`tabular`).
- LOC: `vm/src/builtins.rs` +1170, `compiler/src/resolver.rs` +300,
  `shared/src/native.rs` +180, `compiler/src/ir.rs` +80, plus new
  tests / example / report. Total ~2800 LOC — the largest single-
  agent milestone to date. (Most was straightforward decode-then-
  allocate handler code in `builtins.rs`; not load-bearing logic.)

### Lesson 1 streak: 19 consecutive clean agents

(M28 + M28.5 + M29 + M29.5 + M30×2 + M31 + M32 + M33 + M34 +
M35×3 + M36 + M37). M37 ran 5 phase-commits across the largest
single-agent milestone to date without breaking the streak.
First commit (Phase A green) at ~25% of budget.

### Methodology contribution

M37 establishes the **scale-out template** for v0.3 stdlib packages
that grow beyond a single class: one focused agent ships 5 phases
sequentially behind the M36 canonical registration path, with
per-phase STOP CRITERIA so the milestone ships a smaller-but-
complete subset if budget runs out. The shape works for the next
Pandas-shape phase (M38 group-by + aggregations), the deferred
Phase 5 time-series work, and the eventual GUI binding milestone.

---

## M36 — `StdlibItemKind::Class` refactor (2026-05-21)

Single focused agent closes the M34/M35 scope-down debt. Pure
infrastructure refactor — no public API change, no test regressions.
The 11 stdlib classes shipped in M34 + M35 are now properly published
through their home stdlib modules; the prelude bindings remain for
back-compat. Lesson 1 streak: 18.

### What changed

1. **`StdlibItemKind::Class { class_id: ClassId }` variant** added.
   Enum-with-payload, not a `class_id: Option<ClassId>` field — the
   field-flavour would have required adding `class_id: None` to all
   345 existing `StdlibItem { … }` construction sites. The variant
   is additive; zero changes to existing sites.

2. **All 11 M34/M35 classes** (JsonValue + 6 subclasses, Pattern,
   Connection + Cursor, Hasher) now appear as `Class` items on
   their home modules (`json`, `re`, `sqlite3`, `hashlib`).

3. **`from MOD import X as Y` route** extended to handle Class
   items: aliased imports bind `Y` as a fresh `SymbolKind::Class`
   pointing at the same `ClassId`. Non-aliased imports continue to
   no-op via the legacy "prelude wins" branch.

4. **Phase D annotation**: the legacy branch now carries an explicit
   list of the 11 classes it remains load-bearing for. A future agent
   migrating the M34/M35 tests to `from json import JsonValue`
   forms can delete the branch in one go.

### The honest scope-down

The original brief framed M36 as "move classes OUT of the prelude".
The agent flagged early that every M34/M35 integration test reaches
class names by bare lookup after just `import json` / `import re` /
etc. — no `from … import` form. A hard prelude removal would have
regressed 39 tests. The agent's call (honoring the STOP CRITERIA):
ship the metadata refactor that unblocks v0.4 stdlib growth, keep
the prelude bindings for now, mark the legacy branch for future
removal. This is exactly the M33/M34 scope-down shape — ship working
infrastructure that the next-round agent can extend cleanly.

### Tests + size

- Tests: 723 (unchanged from M35 — pure refactor).
- `compiler/src/resolver.rs`: +156 / −14 lines.
- `LANGUAGE_GUIDE.md`: ~25 lines of prose updates (§3.12 Imports,
  §4.3 Class types, §5 preamble, §6.2 Prelude classes, version banner).
- No changes to `vm/src/`, `shared/src/`, examples, or any test files.

### Significance

The infrastructure is in place for v0.4 stdlib growth. Adding a
Pandas-shaped library (or any 10+-class stdlib package) no longer
requires touching the prelude — `StdlibItem { kind: Class { class_id }, … }`
on the home module is the canonical path. The "prelude is crowded"
risk that HANDOFF.md flagged as urgent-before-M40 is now mitigated
in shape, even if not yet in deletion. Migration of the existing
11 classes is mechanical and can happen incrementally.

### Lesson 1 streak: 18 consecutive clean agents

(M28 + M28.5 + M29 + M29.5 + M30×2 + M31 + M32 + M33 + M34 + M35×3 + M36).
M36's agent committed in 2 clean phases (code + docs). No
orchestrator-commit-on-behalf intervention.

---

## M35 — Four more stdlib classes (parallel round) (2026-05-21)

Follow-up to M34. Three parallel worktree agents shipped four new
prelude-registered stdlib classes — extending the M34 pattern
without coordinating on infrastructure. Cleanest M27-style parallel
round to date: distinctive variable prefixes prevented the
closing-brace alignment hazard, and all three agents committed
cleanly to their worktree branches (Lesson 1 streak: 17).

### Surface

| Agent | Class | NativeFn IDs | Var prefix | New methods |
|---|---|---:|---|---|
| P4-A | `re.Pattern` (compiled regex) | 790-799 | `p4a_` | matches / find / find_all / replace / replace_all / split / source |
| P4-B | `sqlite3.Connection` + `Cursor` | 800-819 | `p4b_` | Connection: execute / query / last_insert_rowid / changes / close. Cursor: fetchone / fetchall / column_names / row_count. |
| P4-C | `hashlib.Hasher` (streaming) | 820-829 | `p4c_` | update / hexdigest / digest / copy / reset / name |

All three are **prelude-registered** (alongside Channel / Thread /
io.File / JsonValue). The flat surfaces remain — `re.find(p, s)`,
`sqlite3.connect(path)`, `hashlib.sha256(data)` all work unchanged.
The new shape adds: `re.compile(s) -> Pattern`, `sqlite3.open(path)
-> Connection`, `hashlib.new("sha256") -> Hasher`.

### Why three at once worked cleanly

Per the M34 archive: the prelude-registration pattern unblocked a
parallel round of class-adders that doesn't coordinate on
infrastructure. Each agent owned a disjoint NativeFn range
(790-799, 800-819, 820-829), a disjoint slot table on `SharedVm`,
and a distinctive `p4a_` / `p4b_` / `p4c_` variable prefix in
shared files (resolver.rs, builtins.rs, ir.rs). Integration via
`git apply --3way` against the pre-M35 base (`475ab47`):

- P4-C applied cleanly (smallest diff first).
- P4-B applied cleanly after P4-C.
- P4-A required two manual fixes at the keep-both block
  boundaries (the closing-brace pattern, now standard since M27)
  but the additive diff itself was 1144 lines clean.

**Three commits, three Lesson-1-compliant agents, one integration
session**. The M27 disasters (1806-line reverse-deletion, double-
brace impl/mod boundary) did not recur — pre-round-base diffing
plus distinctive prefixes are now battle-tested across two parallel
v0.3 rounds (M32+M33 and M35).

### Tests + size

- Tests: 690 → 723 (+33: 10 in vm/tests/m35_re_pattern.rs + 2 in
  re_pattern_demo_runs.rs, 11 in m35_sqlite_class + 2 in
  sqlite_class_demo_runs, 6 in m35_hashlib_streaming + 2 in
  hashlib_streaming_demo_runs).
- Examples: 97 → 100 (+3: examples/re_pattern_demo.spy,
  examples/sqlite_class_demo.spy, examples/hashlib_streaming_demo.spy).
- Stdlib modules: unchanged at 37 (re / sqlite3 / hashlib all
  pre-existed; the new shape extends them).
- Prelude classes: +4 (Pattern, Connection, Cursor, Hasher) for a
  cumulative 11 v0.3 stdlib classes in the prelude on top of the
  6 base classes (Channel, Thread, io.File, Dict, Set, List).

### The prelude is now crowded

The `StdlibItemKind::Class` refactor M34 deferred is now urgent.
17 stdlib classes in the prelude (6 base + 11 v0.3) is more than
the legacy "prelude wins" branch in the import resolver was
designed for. Probably "before M40" rather than "before M50".
Estimated 200-400 LOC in resolver.rs + typecheck.rs; pure refactor,
no public API change.

### Lesson 1 streak: 17 consecutive clean agents

(M28 + M28.5 + M29 + M29.5 + M30×2 + M31 + M32 + M33 + M34 + M35×3).
First commit per agent: P4-A ~45%, P4-B ~50%, P4-C ~40% of budget.
The strengthened brief language has now produced 17 clean commits
across 5 calendar days.

### Methodology contribution

M35 is the cleanest **scale-out replica** of a v0.3 stdlib pattern
in project history: M34 invented prelude-registration for one
class family (JsonValue + 6 subclasses); M35 applied it three more
times in parallel with no infrastructure change. This is the shape
of how the v0.3 stdlib will likely grow until `StdlibItemKind::Class`
lands — small focused class adders, each using the previous
agent's archive as the template.

---

## M34 — Typed JsonValue tree (first stdlib classes) (2026-05-21)

The first stdlib classes shipped in v0.3. Closes the #1 ergonomics
gap documented in the M29 framework agent's report: the framework's
POST body parser hand-walks `json.parse_to_string` output for ~50
LOC where a typed JsonValue tree drops it to ~10.

### Surface

- `sealed class JsonValue` + 6 final subclasses (JNull, JBool, JInt,
  JFloat, JString, JList, JObject) registered in the prelude
- `json.parse(s) -> JsonValue` and `json.stringify(v) -> str`
- Constructor convenience helpers (`json.j_*`)
- Methods: `JList.length / get / items`; `JObject.get / has / keys
  / length`
- Existing `json.parse_to_string` / `is_valid` / `minify` /
  `pretty` / `escape` all preserved (backwards-compatible).

### Design choice — scope-down to prelude registration

Per the brief's STOP CRITERIA: agent registered the 7 classes in
the **prelude** (alongside Channel, Thread, io.File) rather than
building proper `StdlibItemKind::Class` infrastructure for
module-level class items. The legacy "prelude wins" branch in the
import resolver makes `from json import JsonValue` work transparently.
The infrastructure refactor is a pure implementation cleanup — no
API change — deferred to v0.4.

This was the right call: it shipped JsonValue in 3 commits (~50%
budget) while leaving the harder infrastructure question for an
agent that doesn't also have to design 7 classes simultaneously.

### Three findings worth recording

1. **JList storing `List[JsonValue]` worked first-try under M11 +
   M31** — no fallback to opaque handles needed. The class system
   is now stable enough that "recursive type with self in a List"
   just works.
2. **GC root scanning was free**: existing `GcKind::Class` traces
   8-byte slots; `GcKind::List` traces list elements; the recursive
   JsonValue tree needs no bespoke code paths.
3. **Helper-vs-constructor needs two NativeFn IDs per shape**: the
   M11 class constructor convention takes args from object slots;
   helper functions take args from the NativeCall stack. JNull's
   zero-arg case caught this; the agent added separate
   `JsonJNullCtor` (used by `JNull()`) and `JsonJNull` (used by
   `json.j_null()`) NativeFns.

### Tests + size

- Tests: 677 → 690 (+13: 11 in vm/tests/m34_json_value.rs, 2 in
  json_typed_demo_runs.rs).
- Examples: +1 (`examples/json_typed_demo.spy`, ~140 LOC).
- Stdlib modules: unchanged at 37 (json was already there; just
  extended).
- Prelude classes: 7 new (JsonValue + 6 subclasses).

### Lesson 1 streak: 14 consecutive clean agents

(M28 + M28.5 + M29 + M29.5 + M30×2 + M31 + M32 + M33 + M34). First
commit at ~50% of budget. The strengthened brief language has now
produced 14 clean commits across 4 calendar days.

### Next: M35 parallel round

The M34 prelude-registration pattern enables a parallel round of
class-adders to follow without coordinating on infrastructure:
M35 will ship `re.Pattern` (compiled regex), `sqlite3.Connection`
+ `Cursor`, and streaming `Hasher` in 3 parallel agents.

---

## M32 + M33 — async I/O + precise GC stack maps (2026-05-21)

The second and third v0.3 features per THESIS.md §8.4 priority list,
shipped in parallel as two independent worktree agents on disjoint
VM subsystems. **First parallel-v0.3-agent round; zero cherry-pick
conflicts at integration.**

### M32 — async I/O (Shape A: thread-backed Future façade)

Adds a complete async API surface with thread-backed implementation.
v0.4 will swap the internals for a real mio/polling event loop
without changing the public surface.

- New `asyncio` stdlib module (NativeFn IDs 700-714): `run_i32` /
  `run_unit` / `spawn_{i32,i64,str,bool,unit}` / `sleep` /
  `gather_{2,3,4}_{i32,str}`
- New `Future[T]` as a `TypeCtor` (joining `Channel`/`Atomic`/`Dict`/
  `List`) with special-cased `.await()` / `.is_ready()` dispatch.
  Agent's design call: did NOT use M31's user-defined-generic-class
  machinery — ~25 LOC of TypeCtor wiring vs the full M31
  monomorphisation worklist. Same shape as existing stdlib generic
  types.
- New async-variant socket functions (IDs 720-722): `async_accept`,
  `async_recv`, `async_send` returning `Future[...]`
- Demo: `examples/async_echo_server.spy` (~115 LOC) composes 5+ async
  primitives. Integration test verifies 3 concurrent clients.
- Internal: `SharedVm.futures` slot table with
  `Mutex<FutureState>` + `Condvar` per slot; spawned OS thread fills
  the slot on completion; `Future.await()` blocks on the Condvar.
- 9 new tests (7 VM + 2 compiler integration), all green.

### M33 — precise GC stack maps (shadow-stack fallback)

Replaces the M9 `in_jit: AtomicUsize` "freeze GC during JIT'd
execution" pause with a per-thread shadow-stack approach that lets
the collector see register-resident pointers in JIT'd frames.

- Shape: shadow-stack fallback, not full Cranelift `enable_safepoints`
  (the brief's STOP CRITERIA explicitly authorized this scope-down
  — Cranelift's safepoint API requires walking JIT'd Rust frames
  and correlating PC offsets against MachBufferFinalized ranges that
  `cranelift-jit 0.115` doesn't expose stably).
- New `vm/src/stackmap_registry.rs`: thread-local `Vec<(buf, len)>`
  with `rt_shadow_push` / `rt_shadow_pop` extern "C" helpers callable
  from JIT'd code.
- `vm/src/jit.rs`: each JIT'd function allocates a per-function
  Cranelift stack slot sized to its register file. Before every
  heap-allocating runtime helper (`rt_alloc`, `rt_list_*`,
  `rt_array_new`, `rt_virtual_call`, `CallDirect`, etc.) the JIT
  spills every register variable into the slot and pushes the window;
  pops after the helper returns.
- `Heap::collect` consults the shadow stack windows for precise root
  enumeration in JIT'd frames.
- The `in_jit: AtomicUsize` field, its bracket calls in
  `op_call_direct`, and the early-return in `maybe_collect` are all
  removed.
- 4 new tests (vm/tests/m33_precise_gc.rs) — including the M26
  `btree` shape (recursive 5,000-allocation workload).

### Why M32 + M33 in parallel worked

Both agents launched from the same pre-M32 base (893326a) and were
explicitly briefed to avoid each other's territory:

- M32 owned `asyncio`/`socket` natives, `Future` TypeCtor, parser
  tweak for `.await()` as method name, `SharedVm.futures` slot table
- M33 owned `vm/src/{gc,jit,stackmap_registry}.rs`, removed
  `SharedVm.in_jit` field

The shared files (`vm/src/interp.rs`, `vm/src/lib.rs`) had
orthogonal additions vs. removals — M32 added `SharedVm.futures`,
M33 removed `SharedVm.in_jit`. Git's three-way merge resolved them
automatically; **zero manual edits** at cherry-pick time, the
cleanest parallel-agent integration in project history.

### Lesson 1 streak now at 13 consecutive clean agents

(M28 + M28.5 + M29 + M29.5 + M30×2 + M31 + M32 + M33). The
strengthened brief language has now produced 13 clean commits
across 4 calendar days with zero orchestrator-commit-on-behalf
interventions needed.

### Tests + size

- Tests: 664 → 677 (+13: 9 from M32 + 4 from M33).
- Examples: +1 (`async_echo_server.spy`).
- Stdlib modules: 36 → 37 (asyncio).
- Bug catalogue: unchanged at 35/35/0 — no new bugs found in either
  round.

### What this means for the v0.3 trajectory

After M31 + M32 + M33, the top three items of the v0.3 priority list
are complete. The v0.4 menu now reads:
- Real Cranelift safepoint stack maps (replaces M33's shadow stack)
- Real mio-based event loop (replaces M32's thread façade)
- Bounded generics, variance, HKT, explicit type-arg syntax (extends
  M31's generic classes)
- Stdlib classes built on M31 generic classes — typed JsonValue,
  Request/Response, etc.
- Phase 3d stdlib (traceback, enum, functools, uuid, secrets)
- HTTP/2, WebSockets, async ssl/sqlite/http_client variants

---

## M31 — Generic classes (first v0.3 feature) (2026-05-21)

The first item on the THESIS.md §8.4 v0.3 priority list shipped: generic
classes (`class Box[T]:`, `class Pair[K, V]:`, `class Stack[T]:`).
Extends M17's worklist-driven monomorphisation infrastructure from
free functions to classes.

Surface: field types, method param/return types, and method-body
locals all parameterised over T. Constructor-site type inference (no
explicit `Box[i64]()` syntax — that's a v0.4 follow-up). Distinct
runtime type_id + distinct method bodies per instantiation (mangling:
`Box__i64`, `Pair__str_i32`). Existing M11 vtable infrastructure
handles dispatch — zero new VM opcodes.

Implementation: new `ClassLayout.generic_tvars` field; new
`class_generic_scope` in resolver; `field_offset` accepts both
`Ty::Class(c)` and `Ty::Generic{TypeCtor::Class(c), ..}`; field
offsets forced to 8-byte slots when type contains unbound `Ty::Var`;
new IR Pass 2.7 (seed class instantiations + emit per-instantiation
`TypeTableEntry`) and Pass 3.6 (drain class-inst worklist, lower
method bodies), running to joint fixpoint with M17's free-function
worklist inside an outer loop.

Scope-downs to v0.4 (documented in spec §5.1.5):
- Bounded class generics (`T: Comparable`)
- Variance markers, HKT, explicit type-argument syntax
- Subclassing a parameterised class
- Fully-internal transitive construction the typechecker never sees
  (clean VM trap path via `u32::MAX`)

Tests: 656 → 664 (+8). Agent followed Lesson 1 discipline cleanly
— 4 commits, first inside the 15%-of-budget window. Lesson 1 streak
now at **11 consecutive clean agents**.

Why this matters for v0.3: unblocks typed stdlib classes (typed
`JsonValue` tree, `Request`/`Response`, `re.Pattern`, etc.) that
would shrink the M29 web framework ~30% in LOC.

---

## M30 — Last two open bugs closed (2026-05-21)

Two focused parallel agents closed the last two open bugs in the
project. **35 found / 35 fixed / 0 deferred** — the cleanest "v0.2
frozen" state possible. First time at zero open bugs since M10.

### Agents

- **M30 BUG-028** — lexer line continuation across infix `+`/`and`/
  `or`/`==`/`=`/`+=`/etc. Frontend-only fix in `compiler/src/lexer.rs`
  (~95 LOC). Track last significant token; suppress NEWLINE if it's
  a binary operator needing a right-hand operand. Trigger set
  documented in spec §3.2; deliberately excludes `:` / `,` / `.` /
  `->` / `@` / unary `not`/`~`. 11 new regression tests.
- **M30 BUG-040** — `socket.close_listener` now wakes blocked
  `accept()`. Option C from the catalog (cleanest API): extended
  `close_listener` semantics, no new NativeFns. Implementation uses
  TWO mechanisms unconditionally — `shutdown(fd, SHUT_RDWR)` +
  self-connect to listener address (with wildcard-bind rewrite to
  loopback). Agent empirically found that Windows winsock does NOT
  wake `accept` from `shutdown` alone (KB-179942) — self-connect is
  essential on Windows. 1 new regression test with 5s watchdog
  (pre-fix would hang `cargo test`).

### Two findings worth recording

**The cross-platform shutdown finding** (BUG-040 agent): Linux
`shutdown(fd, SHUT_RDWR)` wakes a blocked `accept()` immediately.
Windows winsock does not — `accept()` keeps blocking. The canonical
Windows fix is to self-connect to the listener's address with a
short timeout; the throwaway connection gets accepted and the
accept returns. The agent ran the test, hit the 5s watchdog on
Windows, and added the self-connect as a belt-and-braces fix that
works uniformly across platforms. This is reproducible from the
Microsoft KB-179942 article but is the kind of platform-specific
landmine that doesn't show up in cross-platform Rust abstractions
(`std::net::TcpListener` doesn't expose any shutdown surface; you
have to drop down to raw FD/socket handles).

**The Lesson 1 streak holds at 10**: both M30 agents committed
cleanly on their worktree branches with no orchestrator
commit-on-behalf needed. The strengthened brief language (first
commit before 60% of budget) is now battle-tested across **10
consecutive clean agents** spanning M28 + M28.5 + M29 + M29.5 + M30.
The intervention is reproducible.

### Tests + size

- Tests: 639 → 651 (+12).
- Examples: unchanged at 96+.
- Stdlib modules: unchanged at 36.
- Bug catalogue: **35 found, 35 fixed, 0 deferred** (post-M30).

### The "v0.2 frozen" interpretation

After M30, the language has no known correctness bugs. The
remaining items on the "what v0.2 can't do" list are unimplemented
v0.3 features (generic classes, async event loop, precise GC stack
maps, stdlib classes, HTTP/2, WebSockets, server-side TLS mutual
auth, NumPy integration). None of those are bugs — they are
documented gaps with explicit rationale.

The minimum claim for a clean v0.2 release is now achievable.

---

## M29 — Webserver framework stress test (2026-05-20)

The largest single-program stress test of the project to date. A
complete HTTP/1.1 + HTTPS web framework (Sinatra/Flask-shaped) plus a
real TODO API app — ~1,446 LOC of StrictPy in one file
(`examples/webserver/todo_app.spy`). Optionally HTTPS via the new
M28.5 server-side TLS.

### Headline finding: zero new bugs in M28/M28.5 networking

**First stress round in project history that found zero bugs in the
target surface.** A 1500-LOC program exercising socket.accept +
socket.recv + socket.send + socket.close + bidirectional TLS across
50 concurrent connections, with thousands of requests in the
performance probe, surfaced no networking-stack bugs. Compare to M10
(17 bugs), M11 (6), M12 (2), M18 (1), M24 (1), M27 (1).

Two contributing factors: smaller target surface than prior rounds (3
networking modules + threading + sqlite + json/csv); plus unusually
disciplined agents in M28/M28.5 (P3b-A self-caught a deadlock,
P3b-B/D had clean Lesson 1 discipline) — tighter incoming surface,
less integration drift, fewer latent issues to surface later.

### Stress-test findings — language ergonomics (not bugs)

4 v0.2 gaps surfaced by building the framework, all documented:

1. **No typed JsonValue tree in stdlib** — the biggest pain. POST body
   parser hand-walks canonical compact form (~70 LOC); a typed sum
   type in v0.3 drops this to ~10 LOC of pattern matching.
2. **`from` is a reserved word** even as a parameter name. Renamed
   to `start`/`end`. Could be tightened (only conflicts in import
   context).
3. **No expression-level `T?` unwrap operator.** Workaround:
   `if x is not none: ...`. v0.3 ergonomics.
4. **BUG-039 still bites for non-str Dict keys** — already deferred.

These are **library-density gaps, not language-feature gaps**.

### What it exercises (every major piece in one program)

- Networking: M28 socket + M28 ssl client + M28.5 ssl server +
  M28 http_client (in the test harness)
- Concurrency: M6 Thread + M23 threading.Lock + threading.Semaphore
- Storage: M23 sqlite3 (CRUD on a todos table)
- Data: M22 json + urllib_parse + M28 http_client.urlencode
- Observability: M27 logging
- Time: M20b time.monotonic + hand-rolled HTTP-Date
- Language: M11 classes, M14 tuples, M15 try/except, §8.6 closures

### Performance (ballpark)

| Endpoint | HTTP | HTTPS |
|---|---:|---:|
| /health (no I/O) | ~2200 req/s | ~800 req/s |
| GET /api/todos (1 SQLite query) | ~1500 req/s | ~700 req/s |
| POST /api/todos (1 SQLite insert) | ~1100 req/s | ~600 req/s |

**Within 2× of Flask + gunicorn** without async I/O, JIT warmup, or
connection pooling. The remaining gap is the async event loop (v0.3).

### Methodology: Lesson 1 escalation continues to deliver

The agent followed the strengthened brief perfectly: 4 commits, all
before 80% of budget; first commit (framework skeleton) at ~15%. The
M28 Lesson 1 escalation now has 4 data points (P3b-A, P3b-B, P3b-D,
M29) of clean checkpoint discipline. Numerical thresholds work.

### LOC comparison

| Component | StrictPy | Python+Flask |
|---|---:|---:|
| Framework | ~620 LOC | ~250 LOC |
| HTTP parser | ~200 LOC | 0 (stdlib `http.server`) |
| JSON tree | ~70 LOC | 0 (stdlib `json.loads`) |
| HTTP-Date | ~20 LOC | 0 (`email.utils.formatdate`) |
| Str helpers | ~50 LOC | 0 (stdlib) |
| Demo handlers | ~200 LOC | ~50 LOC |
| **Total** | **~1,160 LOC** | **~300 LOC** |

The 4× gap is library density, not language-feature gap. v0.3 stdlib
classes (typed JsonValue, Request/Response) would close ~half of it.

### Tests + size

- Tests: 634 → 647 (+13).
- Examples: 85 → 87.
- Stdlib modules: unchanged at 36 — M29 builds on existing surface.

### The single-sentence finding

**StrictPy has enough surface, today, to build a non-trivial real
program — and the language survived the test cleanly. The remaining
gaps are library density, not architectural.**

---

## M28.5 — Server-side TLS (2026-05-20)

Closes the v0.2 networking gap. Single focused agent extends the M28
`ssl` module with server-side TLS so StrictPy can build HTTPS servers
by composing `socket.listen_tcp` + `ssl.accept_tls` + the existing
client-side `ssl.send`/`recv`/`close`.

3 new NativeFns in the 610-612 range (P3b-B's reserved space): 
`SslLoadServerConfig`, `SslAcceptTls`, `SslFreeServerConfig`. New
crate dep: `rustls-pemfile`. §9.41 amended in place — no new spec
section number.

Design: Option A from the brief — parallel `tls_server_streams` table
next to the existing client-side `tls_streams`. Server handles
allocated from id range ≥ 1,000,000; existing
`ssl.send`/`recv`/`close`/`peer_addr` handlers extended in-place to
dispatch on handle value. Zero edits to P3b-B's client-side logic.

Patch applied cleanly with no conflicts and no manual brace fixes —
the agent's clean discipline (distinctive `p3b_d_` prefix on all
locals + canonical closing-brace shape + additive-only changes to
shared dispatch handlers) eliminated the orchestrator overhead that
M27 and M28 P3b-B/C had needed.

Tests: 634 → 636 (+2: `https_server_demo_compiles` + `_runs_via_spy_exe`).

---

## M28 — Phase 3b stdlib (2026-05-20)

The biggest single domain remaining at end of M27: networking.
3 parallel worktree agents shipped 3 modules — socket (TCP/UDP raw
+ listen/accept), ssl (TLS-over-TCP via rustls), http_client
(HTTP/1.1 via ureq).

### Agents

| Agent | Modules | NativeFns | New deps |
|---|---|---:|---|
| P3b-A | socket | 19 (570-588) | none (std::net) |
| P3b-B | ssl | 10 (600-609) | rustls + rustls-pki-types + webpki-roots |
| P3b-C | http_client | ~12 (620-649) | ureq |

### The Lesson 1 escalation actually worked

The brief's strengthened language — "**Your FIRST `git commit` must
land before you have used 60% of your estimated time budget**" —
moved the needle where 7+ prior agents had failed:

- P3b-A: 2 commits (initial + self-fixed deadlock at ~40% of budget)
- P3b-B: 2 commits (green-build checkpoint at ~30%, final at end)
- P3b-C: 1 commit (committed before final test verification)

3 of 3 agents shipped committed work, vs 3 of 5 (M27) and 0 of 4
(M24). Explicit numerical thresholds in agent briefs move the needle
where qualitative urgency ("commit early") doesn't.

### Two integration disasters worth recording

**Disaster 1**: P3b-B's diff generated against current-main (which
already had P3b-A's content) contained REVERSE-DELETIONS of P3b-A's
work. The first commit deleted 1806 lines. Caught by inspecting the
`--stat` output. Recovery: `git reset --hard HEAD~1` + regenerate
diff against the PRE-M28 base (c4fe0ce). **Pattern lesson**: when
sequentially cherry-picking parallel worktrees, always diff against
the common ancestor, not against current-main.

**Disaster 2**: The keep-both auto-resolution placed P3b-B's `ssl`
StdlibModule block AFTER the closing `}` of `seed_stdlib_modules`.
Compile errors: "no method `seed_prelude` found for `Resolver`"
because methods after the misplaced block fell outside the impl
scope. Recovery: extract the ssl block from the worktree's
resolver.rs, reset main's file, Python-scripted insertion before
the function's closing brace.

### The familiar closing-brace fix (third round in a row)

`vm/src/builtins.rs` again needed 2 missing-`}` fixes between
adjacent agents' match arms — plus a NEW variant: where P3b-B's
`mod ssl_no_verify` ended and P3b-C's helper functions began, the
keep-both dropped TWO closing braces (one for `impl
ServerCertVerifier for NoVerify`, one for the `mod` block). Pattern
escalation noted: every M27+ integration adds N-1 missing braces
between adjacent agents.

### What the language can now do

After M28, the language has 36 stdlib modules and reaches into:
TCP/UDP sockets, TLS, HTTPS clients, SQLite, threading, subprocess,
filesystem (full surface — shutil + tempfile + glob + pathlib +
os + io), compression + archives, JSON + regex + CSV, argparse +
collections + itertools + statistics + struct, hashing + base64 +
urllib_parse, datetime + time + random + math, logging. That is
"everything a non-async CLI tool, log scraper, or API client
needs" — the explicit gaps are async I/O, HTTP/2/WebSockets,
generic classes, user-defined exception subclasses.

### Tests + size

- **Tests**: 621 → ~640 (+~20 new).
- **Examples**: 79 → 85 (+6).
- **Stdlib modules**: 33 → 36.

---

## M27 — Phase 3c stdlib (2026-05-20)

Filesystem ergonomics + compression + archives + logging — 9 stdlib
modules shipped concurrently by 5 worktree-isolated agents (the
largest concurrent stdlib round to date). NativeFn IDs 450-569; spec
§9.30-§9.39.

### Agents + outcomes

| Agent | Modules | NativeFns | New crate deps |
|---|---|---:|---|
| P3c-A | shutil + tempfile | 9 | tempfile, libc (unix) |
| P3c-B | glob + fnmatch | 7 | glob |
| P3c-C | gzip + zlib + bz2 | 11 | flate2, bzip2 |
| P3c-D | zipfile + tarfile | 16 | zip, tar |
| P3c-E | logging | 11 | (none — std::io + std::fs only) |

### Three patterns recorded by this round

1. **"Commit EARLY" still doesn't get followed.** Brief language is
   necessary but not sufficient — 2 of 5 M27 agents still ran out of
   compute mid-test-build before committing (same failure mode as
   M23 P3a-D + all four M24 agents despite the brief's explicit warning).
   Orchestrator committed both worktrees on the agents' behalf. The
   pattern recommendation now: auto-snapshot worktree state at
   intervals from the orchestrator side, independent of agent's
   explicit commits.

2. **Keep-both auto-resolution has a one-line failure mode.** The
   M27 orchestrator switched from manual cherry-pick resolution to
   `git apply --3way` + Python script that takes both sides of every
   conflict marker. This worked cleanly for purely additive at-end
   blocks (spec sections, Cargo.toml entries, from_u32 match arms),
   but failed in two places per integration: resolver.rs interleaved
   module-block conflicts, and vm/src/builtins.rs match-arm-boundary
   missing-`}` errors. Each required one manual fix per integration.

3. **Spec section collisions are now standard.** P3c-A and P3c-D
   both independently picked §9.30 + §9.31. M22 had all 4 agents
   pick §9.15+. Orchestrator renumber step is now expected, not
   exceptional.

### Tests + size

- **Tests**: 586 → ~640+ (M27 added ~50 new tests across 5 agents).
- **Examples**: 70 → 79 (+9 new demo programs across 9 modules).
- **Stdlib modules**: 24 → 33.
- **vm/Cargo.toml deps**: 11 → 17 (+ unix-only libc).

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
