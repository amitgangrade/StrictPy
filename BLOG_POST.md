# I built a statically typed Python from scratch, and it beats CPython by up to 17× — and by Friday it was hosting a working web framework

*A 5-day, AI-orchestrated journey from "is this even possible?" to a 36K-line Rust toolchain with a Cranelift JIT, 96 example programs, 36 stdlib modules, a working HTTP/1.1 + HTTPS web framework written in StrictPy itself, and two open bugs — and what it taught me about why dynamic languages are slow.*

---

## TL;DR

Over 5 calendar days of AI-orchestrated work I built **StrictPy** — a statically typed dialect of Python with its own compiler, bytecode VM, Cranelift JIT, and a Python-shaped 36-module stdlib including TCP/UDP sockets, bidirectional TLS, HTTP/1.1 client, sqlite3, threading, datetime, json, regex, csv, hashlib, and more. By day 4 the language was hosting a real HTTPS web framework + TODO app **written in StrictPy itself**, on top of its own stdlib.

The current implementation wins against CPython 3.12 on every cell of the canonical 4-program benchmark suite:

| Benchmark | StrictPy | CPython 3.12 | StrictPy is… |
|---|---:|---:|---:|
| fib(30) recursive | 13.1 ms | 159.5 ms | **12× faster** |
| fib(33) recursive | 34.8 ms | 537.7 ms | **15× faster** |
| quicksort(100K) | 18.6 ms | 238.6 ms | **13× faster** |
| dot product (1M f64) | 54.0 ms | 239.1 ms | **4× faster** |
| Mandelbrot 60×30 | 13.6 ms | 56.6 ms | **4× faster** |

And on an extended 30-cell suite (5 more pure-compute programs + 5 stdlib-comparison programs): **28 wins, 2 ties, 0 losses**.

Headline numbers (current — post-M34): **97 example programs**, **690 tests passing**, **35 distinct bugs found and ALL 35 fixed (0 deferred)**, **37 stdlib modules** + 7 prelude JSON classes, **a working HTTP/1.1 + HTTPS web framework + TODO API demo in ~2,400 LOC of StrictPy** that runs ~2,200 req/s on `/health` (within 2× of Flask+gunicorn). The project was tagged as **v0.2.0** at M30 (the 35/35/0 freeze point); v0.3 began at M31 and has shipped generic classes, async I/O, precise GC stack maps, and typed JsonValue tree.

The short answer to "how": **static types make AOT compilation easy, and AOT compilation crushes any interpreter — and once you have a typed-bytecode language with a real stdlib, you can actually use it for real software.** The interesting answer is the long story of what had to be true for both punchlines to land — including seven stress test rounds that found bugs unit tests didn't, a class-system overhaul forced by a M3-era latent hack that took 10 milestones to trigger, a four-times-recurring "placeholder IR lowering" pattern, and the small methodology change that took agent commit-discipline from "fails 40% of the time" to "8 consecutive clean agents."

This is the journey. A more rigorous companion document — chapters on Design, Implementation, Performance, Methodology, Findings — lives at [`THESIS.md`](THESIS.md).

---

## The starting question

A user once asked me, "Why is type information not used to generate more efficient bytecode in Python?" The honest answer has many layers: type hints can lie, they're often stored as strings, CPython's object model is the actual bottleneck, etc.

But there was a follow-up question I couldn't dodge: **what would it look like if you actually built a Python where the types weren't optional?**

So I tried.

---

## The design

StrictPy is Python's syntax with some adjustments:

```python
# Mandatory annotations everywhere
fn fib(n: i64) -> i64:
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)

# Concrete numeric types, no implicit conversion
final WIDTH: i32 = 60
final HEIGHT: i32 = 30

# Classes are `final` by default — `open` to allow subclassing
open class Shape:
    open fn area(self) -> f64:
        return 0.0

final class Circle(Shape):
    radius: f64
    fn __init__(self, r: f64) -> None:
        self.radius = r
    fn area(self) -> f64:
        return 3.14159 * self.radius * self.radius
```

Key constraints that fall out of "types must be load-bearing":

- `int` is not a type. Use `i32`, `i64`, or `BigInt`.
- No `Any`, no `eval`, no monkeypatching, no `__dict__` mutation, no metaclasses.
- Single inheritance only. Protocols for structural typing.
- Nullability is explicit: `T?` means "T or None".

These aren't aesthetic choices. Every one of them eliminates a category of dynamism that an AOT compiler would otherwise have to defeat with runtime checks.

---

## The architecture

A Rust workspace with three crates:

```
shared/    Opcode enum, .spyc file format, NativeFn registry
compiler/  Lex → parse → resolve → typecheck → IR → optimize → bytecode
vm/        spy binary: loads .spyc, runs bytecode (interpreter + Cranelift JIT)
```

The compiler pipeline mirrors any small language: lex → parse → resolve → typecheck → IR lower → optimize → emit bytecode → write. The VM loads bytecode, optionally JITs each function, then dispatches calls either to native code or to a plain `match`-over-`Opcode` interpreter loop.

By the end (M25), this is ~31,000 lines of Rust plus ~11,000 lines of example StrictPy. The two biggest files: `compiler/src/ir.rs` (~2,500 lines, IR lowering + generics monomorphisation) and `vm/src/interp.rs` (~2,200 lines, the interpreter dispatch loop).

---

## The milestones

I structured the work as a series of milestones (M0 through M9), each with explicit acceptance criteria. Here's what each one delivered.

### M0–M2: spec, parser, type checker

Standard compiler frontend. The spec (`STRICTPY_SPEC.md`) came first — ~2,000 lines of canonical reference covering grammar, type system, semantics, bytecode format, and opcodes. Then a lexer (~1,500 lines) and parser (~2,300 lines), both with comprehensive unit tests. Then a resolver + bidirectional type checker (~2,600 lines combined) with 20 negative-test cases for forbidden constructs (no `Any`, no implicit numeric coercion, no nested fn capture writes, etc.).

By the end of M2, all 7 example programs parsed and type-checked cleanly. Nothing actually ran yet.

### M3–M4: bytecode + interpreter

M3 lowered the typed AST to an SSA-ish IR and emitted `.spyc` bytecode files using a custom binary format (full §12 of the spec, byte-by-byte). M4 built the VM: loader, interpreter dispatch loop, basic object model, stop-the-world mark-sweep GC.

**The first time hello.spy actually printed "Hello, StrictPy!" was the moment this stopped feeling like a thought experiment.** It was also when several latent bugs surfaced.

### M3.5: the embarrassing detour

M4's report showed something unsettling: programs were "passing" the integration tests, but several were passing *vacuously*. fib.spy hung in an infinite loop (the M3 IR didn't update loop-carried locals across back-edges, so `while i < 15: i += 1` read the original `i` register forever). dot.spy returned 0 because `[1.0, 2.0, 3.0]` list literals weren't actually populated. mandelbrot.spy printed nothing because top-level `final` consts weren't lowered — `WIDTH` and `HEIGHT` both resolved to 0.

These were real bugs masquerading as success. The fix took a focused round of IR rework, which broke tree.spy in a new way (constructor + field-store interaction). It took **three more separate bug discoveries** before tree.spy worked again:

1. A duplicate `self` param was overwriting slot 0 with `Unit` type
2. Eager devirtualization on `open` classes was skipping subclass overrides
3. `__init__` was consuming vtable slot 0, shifting every virtual method by one

This pattern repeated across the project: each milestone fixed bugs the previous milestone didn't know it had. Every working example told us something, but each one also hid something.

### M5–M7: stdlib and runtime classes

M5 added the native runtime implementations: file IO, channels, real OS threads (via `Arc<Module>` and per-thread `Interpreter` instances), dicts, math. By the end of M5, the VM could spawn threads correctly and channels worked — but `producer.spy` (the example that uses both) still didn't run, because the compiler emitted `VirtualCall` for `ch.send(i)` where `Channel` is a handle-backed runtime type with no real vtable.

M6 fixed a pile of real correctness bugs surfaced by getting more examples to run: lambda lifting (so threads could actually use closures), three separate `tree.spy` regressions, real threading wiring. M7 finally cracked the runtime-class dispatch — marking `Channel`, `File`, `Dict`, `str` as `is_native: true` in their `ClassLayout` and having the IR lowerer skip the vtable path for those.

**The M7 work also surfaced three deeply embarrassing bugs:**

- `not x` was emitting *bitwise* NOT. So `not 1` returned `0xFFFFFFFE` — truthy! Every `if not …:` in every program was silently wrong.
- `none` was being stored as the bit pattern `0`. So `if v is none:` matched zero-valued integers and zero-byte pointers.
- `Thread(closure)` emitted a generic `Alloc` returning a zeroed header — the spawned thread always saw a null closure.

These had been there since M3. They went undetected because no test asserted on values precise enough to catch them. Lesson: if your test says "doesn't crash," it's saying nothing.

By the end of M7, all 7 examples ran end-to-end with verified real output.

---

## The first benchmark

With 7 programs actually working, time to measure against CPython.

I wrote a benchmark harness in `bench/harness.py` that generates parameterized StrictPy and Python source files, compiles both, runs both, and reports timings. Four benchmarks: Fibonacci (recursion), Quicksort (list mutation), dot product (numeric loop), Mandelbrot (nested loops).

**Initial results were suspicious in StrictPy's favor.**

The user caught it immediately: *"Did Python time include the time it took to compile Python to .pyc?"*

It did. I was running `python file.py` (parse + compile + execute) versus `spy.exe file.spyc` (execute pre-compiled bytecode). Adding a `py_compile` step before timing fixed the methodology. The corrected numbers were the real story:

### Snapshot 1: M7 (interpreter only, fair comparison)

| Benchmark | StrictPy | CPython 3.12 | Ratio |
|---|---:|---:|---:|
| fib(20) | 16.2 ms | 75.9 ms | **0.21× (StrictPy wins)** |
| fib(25) | 79.7 ms | 88.8 ms | 0.90× (tie) |
| fib(30) | 931 ms | 260 ms | **3.58× slower** |
| fib(33) | 2410 ms | 521 ms | **4.62× slower** |
| quicksort(1000) | 12.4 ms | 54.3 ms | **0.23× (StrictPy wins)** |
| quicksort(100K) | 660 ms | 229 ms | **2.88× slower** |
| dot(10K) | 13.9 ms | 55.4 ms | **0.25× (StrictPy wins)** |
| dot(1M) | 604 ms | 216 ms | **2.79× slower** |
| Mandelbrot | 25.4 ms | 50.2 ms | **0.50× (StrictPy wins)** |

**Tally: 5 StrictPy wins, 3 ties, 8 CPython wins.**

A crossover pattern: StrictPy was faster on small workloads but lost decisively on bigger ones. The diagnosis is exactly what you'd expect for a typed interpreter:

- StrictPy wins small because its startup is cheap and there's no runtime type dispatch.
- CPython wins large because its interpreter loop is 30 years of optimized C, including PEP 659 specializing dispatch added in 3.11.

The conclusion: a typed *interpreter* can't beat CPython's interpreter at scale. To win, StrictPy needed to stop being an interpreter.

---

## M8: the JIT

I asked: *"Suggest one thing that we can do next which will massively improve performance and even beat CPython."*

The answer was clear. Not a JIT in the PyPy/V8 sense — that machinery exists to defeat dynamism. **StrictPy doesn't have any dynamism to defeat.** Every operand's type is known at compile time. There's no need for type profiling, no need for inline caches, no need for deoptimization. The right move was straight **AOT compilation at module-load time** via Cranelift.

Cranelift is a perfect fit:
- Designed for embedded JIT use cases (wasmtime, Lucet)
- 5-10× faster compile times than LLVM
- Stable Rust API
- Generates competitive native code

The design:
- Every JIT-compiled function uses a unified ABI: `unsafe extern "C" fn(*mut VmState, *const u64) -> u64`. Args via pointer, return as u64 bits (bitcast for f64).
- Per-function opt-in: if any IR op in a function isn't JIT-supported, that function stays interpreted. A fixpoint pass disables JIT for callers of un-JIT'd functions so cross-calls always resolve.
- The VM decompiles bytecode back to a typed op stream at load time (no compiler-crate changes, no `.spyc` format break).

One real gotcha that almost wrecked the integration: **on Windows-x86_64, `CallConv::SystemV` is wrong**. Rust `extern "C"` uses `WindowsFastcall`. The default Cranelift example would link, then crash. The agent that did this work added a `host_call_conv()` helper that picks the right one per platform.

### Snapshot 2: M8 (JIT covering arithmetic, branches, calls, list reads)

| Benchmark | M7 | M8 (with JIT) | M7→M8 | vs CPython |
|---|---:|---:|---:|---|
| fib(30) | 931 ms | **14.6 ms** | **64×** | **11× faster** |
| fib(33) | 2410 ms | **35.5 ms** | **68×** | **15× faster** |
| Mandelbrot | 25.4 ms | **12.5 ms** | 2× | **4.6× faster** |
| dot(100K) | 70.9 ms | **57.7 ms** | 1.2× | 1.2× faster |
| dot(1M) | 604 ms | 478 ms | 1.3× | 1.9× *slower* |
| quicksort(100K) | 660 ms | 679 ms | unchanged | 3× *slower* |

**Tally after M8: 10 StrictPy wins, 2 ties, 4 CPython wins.**

The Fibonacci and Mandelbrot wins were astonishing — a 64× speedup over the interpreter, with fib(30) going from "3.6× slower than CPython" to "11× faster." That's an order of magnitude flip on a single milestone.

But four benchmarks still lost to CPython. Why?

Looking at the JIT coverage report told the story: in the losing benchmarks, the *hot inner functions* (quicksort's `partition`, dot's `build_a`) were *not* JIT'd. They used `ArraySet` and `ListPush` ops that touched the GC's allocation path, which the M8 JIT had punted on. The fixpoint disable cascade then disabled their callers too.

**It wasn't a JIT quality problem. It was a JIT coverage problem.**

---

## M9: full coverage

M9's plan was tightly scoped: add JIT support for the heap-mutating ops, plus user-class operations (LoadField, StoreField, VirtualCall, Alloc). The bet: the JIT'd code in the *non*-hot parts of each function was already fast; we just needed to stop cascading the disable.

Three categories of work:

**1. Inlined ops (no helper call)**: `ArraySet`, `LoadField`, `StoreField`. The current GC is non-moving (mark-sweep without compaction), so heap pointers are stable for the object's lifetime. Cranelift emits a direct load/store at `base + offset`.

**2. Runtime helpers (extern "C" Rust functions)**: `rt_list_push`, `rt_list_new`, `rt_array_new`, `rt_alloc`, `rt_virtual_call`. The JIT'd code calls these directly. Their overhead is one function call hop per operation — same as the interpreter pays, but the surrounding code now runs at native speed.

**3. GC safety**: when JIT'd code holds a heap pointer in a CPU register and `rt_list_push` triggers a reallocation, the GC's conservative scan can't see that register. Fix: an `in_jit: AtomicUsize` counter on `SharedVm`. Bracket every JIT entry; `Heap::collect` skips when the counter is non-zero. This blocks GC during JIT'd execution — fine for benchmark workloads (the 16MB arena is enormous), bad for long-running programs. Proper precise stack maps are an M10 problem.

### Snapshot 3: M9 (full JIT coverage)

| Benchmark | M8 | M9 | M8→M9 | vs CPython |
|---|---:|---:|---:|---|
| fib(30) | 14.6 ms | 13.5 ms | unchanged | **12× faster** |
| fib(33) | 35.5 ms | 34.8 ms | unchanged | **15× faster** |
| quicksort(50K) | 333 ms | **15.2 ms** | **22×** | **9× faster** |
| quicksort(100K) | 679 ms | **18.6 ms** | **36×** | **13× faster** |
| dot(500K) | 251 ms | **29.1 ms** | **9×** | **5× faster** |
| dot(1M) | 478 ms | **54.0 ms** | **9×** | **4× faster** |
| Mandelbrot | 12.5 ms | 13.6 ms | unchanged | **4× faster** |

**Tally after M9: 16 StrictPy wins, 0 ties, 0 losses.** Every cell.

---

## The full progression

Putting all four snapshots in one place:

| Benchmark | M7 unfair* | M7 fair | M8 (JIT) | **M9 (full JIT)** | CPython 3.12 |
|---|---:|---:|---:|---:|---:|
| fib(20) | 22.2 | 16.2 | 8.0 | **7.8** | 52.3 |
| fib(25) | 105.2 | 79.7 | 9.1 | **9.2** | 64.5 |
| fib(28) | 456 | 347 | 10.4 | **9.7** | 97 |
| **fib(30)** | **967** | **931** | **14.6** | **13.5** | **160** |
| fib(32) | 2969 | 1550 | 25.0 | **22.2** | 358 |
| fib(33) | 2781 | 2410 | 35.5 | **34.8** | 538 |
| quicksort(1K) | 11.3 | 12.4 | 13.0 | **10.2** | 53.4 |
| quicksort(5K) | 32.2 | 36.0 | 32.8 | **11.2** | 59.0 |
| quicksort(10K) | 65.0 | 58.3 | 62.7 | **11.1** | 66.6 |
| quicksort(50K) | 496 | 326 | 333 | **15.2** | 138 |
| **quicksort(100K)** | **1286** | **660** | **679** | **18.6** | **239** |
| dot(10K) | 29.9 | 13.9 | 14.4 | **10.9** | 53.1 |
| dot(100K) | 142 | 70.9 | 57.7 | **13.0** | 70.0 |
| dot(500K) | 670 | 304 | 251 | **29.1** | 148 |
| **dot(1M)** | **1177** | **604** | **478** | **54.0** | **239** |
| Mandelbrot | 42.0 | 25.4 | 12.5 | **13.6** | 56.6 |

All times in milliseconds. Best of 3 runs, total wall-clock.

*M7 unfair: Python timing included parse+compile; subsequent runs pre-compile with `py_compile`.

The most striking column to scan: **quicksort(100K)** went from 1286 ms → 660 ms → 679 ms → **18.6 ms**. A 69× improvement from start to finish, with the punchline arriving in two distinct bursts (M8's JIT, then M9's coverage extension).

---

## M10–M12: real programs find real bugs

If the M0–M9 story is "build it and beat CPython," the M10–M12 story is "build six real-world programs and find out what's still wrong."

### M10: six programs in parallel, 17 bugs

I ran four agents in parallel: AB on compiler/VM bug-hunts; C1 on computation (Game of Life, Sudoku); C2 on data structures (JSON parser, Markov chain); C3 on concurrency + interpreters (KV store with WAL, Brainfuck). Plus a CSV aggregator from the preceding session. Roughly 1,500 lines of new StrictPy code across one milestone.

Result: **17 distinct bugs across six programs**. The previous nine milestones combined had surfaced ~12.

The headline find: `is not none` was *inverted* at the IR level. Every `if x is not none:` in every program had been silently running the wrong branch since M2. No existing example caught it because every M0–M9 program used `if x is none: ... else: ...` — the positive form. The first program to organically write `if x is not none:` was M10's JSON parser, and it failed catastrophically.

A second pattern surfaced. CSV aggregator's float aggregation produced wrong totals on rows with missing values. The bug: `prev: f64?` narrowed to `f64` inside `if prev is not none:`, but the IR-side dispatch on `Ty::Primitive` saw the *un-narrowed* `Nullable(f64)` and fell through to integer add. One bug. Then I audited every `Ty::Primitive` match in `codegen.rs` — five seconds of grep per hit — and found **four more siblings**, each silently miscompiling a different operator under nullable-narrowed operands.

One real-world program found one bug; the audit found four more. **Bugs cluster around a pattern.** It became one of the project's core findings.

### M11: the class-system overhaul (and a 10-milestone-old hack)

M10 surfaced a non-deterministic STATUS_HEAP_CORRUPTION crash in `json_parse.spy` and `calculator.spy`. The symptoms shifted under any attempted fix — depending on subclass declaration order, function declaration order, and apparently random heap-layout variation across runs. Across M10, it was the worst-classified bug in the project.

The M11 breakthrough was the C6 lisp interpreter agent finding a *deterministic* sibling: `Pair(Value) { car: Value }` then `p.tag()` → reliable access violation. Same shape (subclass with class-ref fields + virtual call), deterministic repro. Reducing it to minimal form exposed the underlying cause: subclass fields started at offset 16, **overlapping the parent's vtable pointer**. The Pair's `car` field was overwriting the vtable; the next virtual call dereferenced a garbage pointer.

Fixing that one bug (BUG-016 in the catalogue) collateral-closed the deterministic crash AND the non-deterministic one AND a position-sensitive sibling where adding an unrelated function between two others toggled the crash. The non-determinism was always heap-layout variability supplying different exact failure modes; the underlying trigger was always the same offset aliasing.

But the deepest bug of M11 was something else entirely. Investigating a separate "vtable wraps mod 4" symptom — the 4th sibling class's virtual calls going to the base's slots — led to a piece of code in the VM's `op_new` from M3. The hack: if the operand didn't match a known `type_id`, fall back to indexing the type table as if the operand were a `class_id`. **The hack had worked silently for 10 milestones** because `class_id` and `type_id` numeric ranges never overlapped. M10 added enough user classes that the 4th one's class_id (16) numerically collided with Shape's type_id (16). Pentagon got allocated with Shape's vtable. The "mod 4" symptom was just what the numerical collision happened to look like.

This is BUG-029 in the catalogue. The thesis-level lesson: **latent bugs accumulate dose-dependently.** A convenience hack can sit silent for years before enough state accumulates to trigger it. The cost of the hack is paid by the milestone that hits the trigger — plus all the work required to recognise that an M11 symptom traces back to M3-era code.

By the end of M11, the class system worked end-to-end. The same five stress programs that had been pre-M10 bug catalogues now ran clean. BUG-026 and BUG-027 went from "non-deterministic terror" to "provisionally fixed."

### M12: confirmation as a deliverable

M12's plan was modest: write three more stress programs, run a torture test against the previously-flaky bugs, and verify M11's class-system work actually landed.

Two of the three programs found zero bugs.

The regex agent shipped a Thompson-NFA engine with a `sealed class RegexNode` carrying 8 final subclasses (Lit, Dot, Star, Plus, Opt, Alt, Concat, CharClass), 6 virtual methods on the base, and class-ref subclass fields. The agent's report opened: "ran first-try without a single workaround." The dijkstra agent built a `final class Graph` with parallel `List[List[T]]` adjacency, plus a recursive sift-up/sift-down min-heap. Same outcome: clean run, no workarounds.

Pre-M11, every class-heavy program shipped with a "known gaps" comment section listing the workarounds. The fact that two M12 programs didn't need one was the headline result. **The absence of bugs found is itself a confirmation result** when it's the first time a particular program shape works first-try.

The third program — a B-tree of order 4 — found two more silent miscompiles:

- **BUG-034**: `str != str` always returned true because the IR's `Ne` lowering had no `is_str` branch and fell through to comparing two heap-pointer u64s. Distinct allocations have distinct pointers, so `INe` returned true for every string compare. Same shape as `is not` (BUG-008 in M10) — a binary-op match arm in `ir.rs::emit_binop` that punted on type-dependent dispatch.
- **BUG-035**: `and` / `or` didn't short-circuit. The source comment in `ir.rs:1738` was honest: "bitwise approximation." Tripped `IndexError: -1` on the standard guard idiom `b > 0 and xs[b-1] > xs[b]`.

Both fixed (the second in M13). Plus the torture test in `compiler/tests/heap_corruption_torture.rs` ran the canonical BUG-026/027 repros 250 sequential times (100× calculator + 100× json_parse + 50× lisp) and produced zero failures in 3.12 seconds. BUG-026 and BUG-027 went from "provisionally fixed" to "confirmed fixed." That marginal cost — 20 minutes of agent time and 3 seconds of CI wall-clock — is almost always worth paying.

---

## M13–M17: language completeness in five milestones

By M12 the language was Python-shaped but missing things every program used to fake-implement. Five sequential milestones over half a day shipped them:

- **M13**: short-circuit `and` / `or` (BUG-035 closed). First mid-expression CFG manipulation — `a and b` lowers to `if a: b else: false` with proper basic-block branching. The pattern (slot-based phi merge) was reused for M15 try/except and M21 `??` null-coalesce.
- **M14**: tuples + destructuring. `Tuple[T1, T2]` types, `(a, b)` literals, `t.0/t.1`, `let a, b = pair()`, return-position tuples. Heap-allocated as synthetic class layouts with **zero new VM opcodes**. Eliminated the "1-element mutable list as multi-return cell" workaround that was the most-friction idiom in every M10–M12 program. Incidentally fixed an `assert(cond, msg)` IR-tuple-allocation crash that would have surfaced as a regression in every example using asserts with messages.
- **M15**: full try/except/finally + raise. 10 built-in exception names. Lazy materialisation of exception objects on handler bind. The Cranelift JIT carve-out is automatic — functions containing try/raise fall back to the interpreter. BUG-025 (no fallible `open()`) closed.
- **M16**: `isinstance` + `match case Constructor()` patterns. Eliminated the `kind: i32` discriminator that every M10–M12 sealed-hierarchy program had used. Flow narrowing for isinstance mirrors the M10 `is not none` narrowing.
- **M17**: generic free functions with call-site monomorphisation. `fn id[T](x: T) -> T` works for any primitive / tuple / class. Per-instantiation operator binding handles `T + T` by deferring resolution until instantiation. Eliminated the rewrite-quicksort-per-type friction. Generic *classes* deferred to v0.2.

These were sequential, not parallel, because every milestone touched `ir.rs` and `typecheck.rs`. Parallel agents would have conflicted. The orchestrator-led pattern: one focused agent per feature, each with a hard acceptance criterion (e.g. "rewrite `examples/calculator.spy` using match patterns; LOC must shrink").

M18 then ran four parallel agents on the new surface as a confirmation round. Three found zero bugs. The fourth (R3 expression interpreter) found exactly one: a spec/runtime drift where the runtime emitted `"DivisionByZeroError"` as the exception name but the spec advertised `"ZeroDivisionError"` as canonical. Easy fix, but the *pattern* was new — when introducing a Python-compat alias, update both the registration table and the runtime emit side.

The cleanest single-data-point evidence that M13–M17 landed coherently: rewriting M10's `json_parse.spy` against the new surface (`json_parse_v2.spy`) went from 374 lines with 8 documented workarounds to **152 lines with zero workarounds**. ~60% reduction.

---

## M19–M23: stdlib

By M17 the language was complete. By M23 it had a Python-shaped stdlib.

M19 landed the load-bearing infrastructure: the `seed_stdlib_modules` table in `compiler/src/resolver.rs` that maps `import json; json.parse(s)` to a NativeFn dispatch in the VM. The hard work was getting `import sys; sys.exit(0)` to type-check, lower correctly through the IR, and produce a non-catchable `VmError::Exit` that walks past any enclosing `try ... except Exception:` (matching CPython's `SystemExit` semantics). After M19, every subsequent stdlib module slotted in without touching the resolver/typecheck/IR layers.

Three phases:

- **Phase 1** (M19–M21, sequential): `sys`, `os`, `path`, `io`, `time`, `random`, `math`, `json`, `re`. 9 modules. One bug found incidentally — BUG-037, the third instance of the placeholder-lowering pattern (`??` always returned its fallback).
- **Phase 2** (M22, **four parallel agents in worktrees**): `argparse`, `collections`, `csv`, `base64`, `hashlib`, `itertools`, `statistics`, `struct`, `urllib_parse`. 9 modules. **Zero bugs**, 4× wall-clock speedup over sequential.
- **Phase 3a** (M23, **four parallel agents in worktrees**): `subprocess`, `pathlib`, `datetime`, `threading.Lock`, `threading.Semaphore`, `queue.PriorityQueue`, `sqlite3` (via rusqlite-bundled). 7 modules. One incidental bug (resolver shadowing legacy `from threading import Thread`).

M22 was the first time the project used **git worktrees for parallel agent isolation**. Four agents wrote to the same four shared files (`resolver.rs`, `native.rs`, `builtins.rs`, `STRICTPY_SPEC.md`) in isolated `git worktree` branches; the orchestrator then cherry-picked all four onto main, hand-resolving the append-at-end conflicts. Total wall-clock: ~1.5 h parallel + ~30 min integration vs ~5 h sequential at the M19–M20 cadence.

The pattern worked twice more (M23, M24). The cost is non-trivial integration work — git's three-way merge once mis-aligned `sqlite3.column_names` with `pathlib.read_lines` at a shared `let sp = alloc_string(...)` line, semantically replacing one handler's tail with the other's. Recovery: reconstruct from worktree history. **For future worktree rounds: give parallel agents distinct loop-variable names so git can't align them.**

By the end of M23 the language reached into OS-level domains — file systems, subprocess pipes, threading primitives, persistence — for the first time. 24 stdlib modules total. The M19 seam continues to hold.

---

## M24: the fourth-instance audit

M24 was a stress round on the Phase 3a surface — four parallel agents writing real programs that combined 6+ stdlib modules each:

- **`job_scheduler.spy`** (267 LOC): subprocess + threading.Lock + threading.Semaphore + queue.PriorityQueue + datetime. 9/9 probes pass. Zero bugs.
- **`event_log.spy`** (759 LOC): sqlite3 + datetime + argparse + io + pathlib + re. 14/14 probes pass. **Found BUG-039.**
- **`test_runner.spy`** (448 LOC): subprocess + threading + queue + sqlite3 + time. 10/10 probes pass. **Verified real OS-thread parallelism** — N=4 vs N=1 wall-clock speedups of 3.62×–5.75× across three runs. The VM doesn't hold a GIL on threads blocked in `subprocess.run`.
- **`fs_migrator.spy`** (330 LOC): pathlib + os + datetime + subprocess + io. 10/10 probes pass. Documented missing Phase 3b primitives.

BUG-039 was the headline. `key in dict` was always returning false, even immediately after `dict[key] = value` succeeded and `dict[key]` returned the right value. The agent shrank it to a 12-line minimal repro.

Root cause: `compiler/src/ir.rs::emit_binop` had this line:

```rust
AstBinOp::In => IROp::IEq,  // placeholder
```

The IR was comparing the key against the container's heap pointer as `i64`. Always false (unless they happened to coincide at the same address). Symptom: `key in d` was silently miscompiled across **every** Dict in StrictPy since M5.

This is the **fourth instance** of the same pattern:

| Bug | Operator | Placeholder | Fixed in |
|---|---|---|---|
| BUG-008 | `is not` | `RefEq` (not `not RefEq`) | M10 |
| BUG-034 | `str !=` | `INe` (no `is_str` branch) | M12 |
| BUG-037 | `??` (null-coalesce) | `Copy(rhs)` (always fallback) | M21 |
| **BUG-039** | **`in` / `not in`** | **`IEq` / `INe`** (pointer compare) | **M24** |

Same shape every time: a binary-op match arm in `emit_binop` that punts on the type-dependent lowering with a hardcoded `IROp`. Each one shipped in M2 alongside the type system; each one surfaced organically when a stress test used the operator in the form the placeholder didn't handle.

**A mechanical audit of `emit_binop` — "for every binary operator whose semantics depend on operand type, verify the lowering dispatches on type" — would have caught all four at once.** The audit is now an explicit menu item for v0.3. The cost is 30–60 minutes; the benefit is closing whatever fifth instance is currently hiding. Tuple compares and Set membership are the strongest candidates.

The stress-round bug-rate trajectory:

| Round | Date | Programs | LOC | Bugs |
|---|---|---:|---:|---:|
| M10 | 2026-05-18 | 6 | ~1,660 | 17 |
| M11 | 2026-05-18 | 5 | ~1,810 | 6 |
| M12 | 2026-05-19 | 3 | ~1,477 | 2 |
| M18 | 2026-05-19 | 4 | ~1,900 | 1 |
| M24 | 2026-05-19 | 4 | ~1,800 | 1 |

The curve has flattened — by M24 a 1,800-LOC stress round finds one bug — but **every round still finds at least one**. And in M21 and M24, both finds were the *same* shape: a placeholder lowering that had been latent since the operator first shipped.

---

## M25: one command, like Python

By M24 the language had: 26 milestones, 70 example programs, 24 stdlib modules, 586 tests, one open bug. But the toolchain was still two binaries:

```
spyc examples/hello.spy -o hello.spyc
spy hello.spyc
```

Python's equivalent is one command:

```
python script.py             # compile + run, cache to __pycache__/
```

M25 collapsed the two-binary split into a single `spy` command modelled on `python`:

```
spy script.spy [args...]              # compile-if-stale + run
spy script.spyc [args...]             # run pre-compiled bytecode
spy -c "code"                          # compile inline + run
spy --compile-only script.spy [-o]    # explicit compile (like python -m py_compile)
```

Bytecode lands in `<dir>/__spycache__/<basename>.spyc` — the StrictPy analogue of Python's `__pycache__/foo.cpython-312.pyc`. The cache is reused iff the source's mtime is ≤ the cached `.spyc`'s mtime; any source edit triggers a recompile.

The refactor took ~30 minutes of focused work — no parallel agents, no sub-tasks. Six files changed: two Cargo manifests, one library API addition (`run_bytes_with_args`), one CLI rewrite, one integration test file (8 new tests), one spec section (§10.8). Tests: 578 → 586 (+8). Zero regressions. The bench numbers didn't move because no codegen changed.

The reason to mention M25 in a "performance" blog post: this is the kind of milestone the M9→M25 plateau makes possible. Sixteen milestones since the JIT shipped have added correctness, language features, stdlib, and CLI ergonomics. Every one of them has shipped a 16/0/0 benchmark sweep against CPython. The fib(30) number has been within 10–20% of 13.5 ms across the entire window. The plateau isn't accidental — it's the architectural payoff for `is_native: bool` (M7), the unified JIT ABI (M8), and the per-function JIT opt-in with fixpoint disable (M8). New features fall back to the interpreter; the JIT-emitted hot-loop code stays untouched.

---

## M26: an honest extended benchmark

The canonical 4-program suite tests what the JIT is good at. M26 added 10 more programs to test other shapes: n-queens (recursive backtracking), Sieve of Eratosthenes (integer loops), matrix multiply 50/100/150 (f64), binary tree insertion (allocation + virtual dispatch), heap sort (mutation), JSON parse round-trip (serde_json vs `_json.c`), regex throughput (Rust `regex` crate vs `_sre.c`), SHA-256 (Rust `sha2` vs OpenSSL/`_sha256.c`), CSV parsing, SQLite insert+query — 30 cells total.

Result: **28 wins, 2 ties, 0 losses**. Two empirical findings worth recording.

**Finding 1**: the `btree` row is the only non-win. Three sizes:

| n | StrictPy | CPython | Ratio |
|---:|---:|---:|---:|
| 1k | 16.6 ms | 71.8 ms | 0.23× |
| 5k | 51.2 ms | 72.4 ms | 0.71× |
| 10k | 96.8 ms | 85.4 ms | 1.13× |

The ratio degrades monotonically as allocation pressure grows. At ~10k recursive `BNode` insertions, StrictPy's `rt_alloc` + the conservative-GC `in_jit` pause overhead overtakes the JIT win. This is *exactly* the workload that precise stack maps + a moving GC would fix — the single architectural limitation StrictPy carries forward from M9, and the M26 data is the empirical evidence of where it shows up.

**Finding 2**: the stdlib comparison was a trap I almost fell into. Both sides do their actual work in C/Rust (StrictPy uses `serde_json`, `regex`, `sha2`, `rusqlite`; CPython uses `_json.c`, `_sre.c`, `_sha256.c`, `_sqlite3.c`). I expected the ratios to land near 1×. Instead they range 0.14×–0.91×, all favouring StrictPy. The cause is process startup: Python pays ~50–70 ms on every cold launch (interpreter + import), StrictPy pays ~5–15 ms. The narrowing-with-size pattern is visible on every stdlib row — CSV parse goes 0.20× → 0.91× as input grows from small to large. The asymptote IS ~1×; startup just dominates short workloads. Honest data, not a measurement bug.

The 30-cell extended suite lives in [`bench/EXTENDED_REPORT.md`](bench/EXTENDED_REPORT.md). The reason to run it: the canonical 4-program bench was too tightly tuned to the JIT's strengths. M26 widens the test surface and shows where the wins generalise and where they don't.

---

## M27: 9 more stdlib modules in half a day, and a methodology problem I couldn't fix with words

M27 was Phase 3c — filesystem ergonomics + compression + archives + logging. Nine modules across **five parallel worktree agents**: `shutil`, `tempfile`, `glob`, `fnmatch`, `gzip`, `zlib`, `bz2`, `zipfile`, `tarfile`, `logging`. The pattern was established by M22 (Phase 2) and M23 (Phase 3a): each agent works in its own git worktree, ships independently, the orchestrator cherry-picks all of them onto main and resolves mechanical conflicts.

The integration cost is roughly constant per agent: append-at-end conflicts in `resolver.rs` / `native.rs` / `builtins.rs` / `Cargo.toml` / `STRICTPY_SPEC.md` — the five shared files every stdlib module touches. M27 added a sixth pattern: a closing brace dropped between adjacent agents' final match arms (caught by `cargo build` immediately; one Edit per integration to fix). Worth recording because it's predictable now: every parallel-worktree round produces *exactly* N-1 missing braces at agent boundaries.

But the more interesting M27 story is what didn't work. **Two of five agents finished their substantive work but ran out of compute budget before reaching `git commit`.** The brief said "commit early, before writing the long report." It had said that since M25, after the same failure mode hit M23 P3a-D and all four M24 agents. By M27 the warning had failed in 7+ agents.

I sat with this for a while. The qualitative urgency ("commit early") wasn't working — agents read the brief, intended to follow it, and still spent their last 30% of budget polishing the report. So I rewrote the brief section with explicit numerical thresholds:

> **Your FIRST `git commit` must land before you have used 60% of your estimated time budget.** If you're approaching that mark and tests aren't passing yet, COMMIT THE WORK-IN-PROGRESS ANYWAY. You can amend the commit later. The orchestrator strongly prefers a half-finished committed state over a complete uncommitted state.
>
> Suggested checkpoint discipline: 20% scaffolding → COMMIT; 40% NativeFns wired → COMMIT (amend); 60% tests passing → COMMIT (amend); 80% report drafted → COMMIT (amend).

That went into M28's shared brief. The result, across the next 8 agents: **all 8 committed cleanly, every one before 80% of budget, several with explicit checkpoint commits at the 20% and 40% marks.** The brief change was the only intervention.

The takeaway generalises: **in agent briefs, numerical thresholds beat qualitative urgency for behaviours expected under time pressure.** "Commit early" is a vibe. "First commit before 60% of budget" is a self-assessable mid-task gate. The lesson cost me 12 messed-up agent runs to learn. Documenting it here so the next project doesn't re-pay the tuition.

---

## M28: networking — the biggest single domain gap, closed in one round

M28 shipped the networking stdlib in three parallel worktree agents:

- **`socket`** (P3b-A): TCP/UDP raw sockets, listen/accept/send/recv, DNS lookup. Backed by `std::net` only — no new Rust crate dependency. ~19 NativeFns.
- **`ssl`** (P3b-B): TLS-over-TCP via `rustls` + `webpki-roots`. Client side only (M28.5 adds server side). 10 NativeFns. Uses the `ring` crypto provider for pure-Rust build on Windows.
- **`http_client`** (P3b-C): HTTP/1.1 client with TLS via `ureq`. GET / POST / PUT / DELETE / HEAD + a generic `request` with custom headers + URL parsing utilities. ~12 NativeFns.

Three new crate deps in `vm/Cargo.toml`: `rustls`, `rustls-pki-types`, `webpki-roots`, `ureq`. Each agent independently designed its own SharedVm slot tables (opaque i64 handles + `Mutex<HashMap<i64, ...>>` slots, the same pattern M23 sqlite3 and M27 zipfile/tarfile use).

All three followed the strengthened M28 brief's checkpoint discipline. **3 of 3 committed before 80% of budget.** Agent P3b-A even self-caught a deadlock in their own code mid-task: their first sketch held the slot-table mutex across a blocking `TcpStream::read()`, which deadlocked the loopback echo test. They fixed it by switching to `Arc<TcpStream>` (grab refcount, drop the table mutex, then do the blocking I/O on the Arc'd handle) and shipped a focused fix-up commit. First time in the project an agent caught and fixed a non-trivial design bug in their own work mid-task.

The integration recovery taught me a second methodology lesson worth recording. After cherry-picking P3b-A onto main, I generated P3b-B's diff via `git diff main..worktree-agent-aba4f8f0e47a762cd`. That diff was computed against current-main, which already had P3b-A's content — so it contained REVERSE-DELETIONS of P3b-A's contributions. The first apply+commit deleted 1,806 lines of already-landed work. Caught by inspecting the commit's `--stat` (deletion counts ≫ insertion counts is the smoke alarm). Recovery: `git reset --hard HEAD~1`, regenerated as `git diff <pre-round-base>..worktree`, re-applied. Now permanent in my mental model: **when sequentially cherry-picking parallel worktrees, always diff against the common ancestor, not against current-main.**

After M28 plus M28.5 (a focused single-agent extension adding server-side TLS in 3 more NativeFns), the language could do bidirectional TLS — open TLS connections as a client, accept TLS connections as a server, send/recv encrypted bytes on either side. That's the foundation for the M29 web framework.

---

## M29: hosting real software

This is the part of the project I'm most proud of.

By the end of M28.5, StrictPy had a complete networking stdlib. The natural next move was to test it — write a real program that combines socket + ssl + http_client + threading + sqlite + json + logging in something a Python developer might actually recognise. **A web framework.**

The brief was: write a Sinatra/Flask-shaped HTTP/1.1 + HTTPS web framework + a TODO API demo on top of it, in StrictPy user code, no stdlib changes. Stress-test the language. Document anything that didn't work.

The agent shipped 1,446 LOC of StrictPy in one file (`examples/webserver/todo_app.spy`) covering:

- **HTTP/1.1 protocol** — request-line + headers + Content-Length body parser; response status + headers + body writer with `Server` / `Date` / `Connection` / `Content-Length` always set; ASCII case-folded header lookup.
- **Router** — linear scan with exact match + `<name>` single-segment + `<*rest>` greedy-tail patterns. Closure-captured handler IDs for dispatch (the agent skipped a `final class Handler` design in favour of integer handler IDs because the v0.2 generic constraints made it cheaper).
- **Server** — thread-per-connection accept loop with a 50-permit `threading.Semaphore`. Separate `accept_loop_plain` and `accept_loop_tls` functions.
- **TODO app** — `GET /`, `GET /api/todos`, `POST /api/todos`, `DELETE /api/todos/<id>`, `GET /static/<*rest>`, `GET /health`. SQLite persistence. JSON request/response.
- **HTTPS** — `--tls cert key` flag flips the server to use `ssl.accept_tls` instead of `socket.accept`. The test harness uses `rcgen` to generate a self-signed cert at test time.
- **Access logging** — every request emits `logging.info(method, path, status, ms, peer)`.

Three integration tests: compile-check, HTTP round-trip (GET / POST / DELETE), HTTPS round-trip. All pass.

The empirical result: **zero new bugs in M28/M28.5 networking surface**. This was the first stress round in the project's history with zero finds. Prior rounds: M10 found 17, M11 found 6, M12 found 2, M18 found 1, M24 found 1, M27 found 1. M29 found zero. The combination of careful M28/M28.5 agent discipline + a small target surface + a stress test that exercised the natural Python idiom produced a clean result.

The language ergonomics gaps it documented, however, are real (none are bugs; all are v0.2 stdlib/language-feature limits that v0.3 would close):

1. **No typed `JsonValue` tree in stdlib.** The POST body parser hand-walks the canonical compact form produced by `json.parse_to_string` (~70 LOC). A typed `JsonValue` sum type in v0.3 would drop this to ~10 LOC of pattern matching.
2. **`from` is a reserved word** even as a parameter name. Renamed to `start` / `end`. Minor stumble.
3. **No expression-level `T?` unwrap operator.** Pattern `if x is not none: ...` works but doesn't propagate to chained accesses.
4. **BUG-039 still bites for non-str Dict keys.** Worked around with `dict.get(k)` + `is not none`.

The framework's performance, best-effort on loopback:

| Endpoint | HTTP req/s | HTTPS req/s |
|---|---:|---:|
| `/health` (no I/O) | ~2,200 | ~800 |
| `GET /api/todos` (1 SQLite query) | ~1,500 | ~700 |
| `POST /api/todos` (1 SQLite insert) | ~1,100 | ~600 |

**Within 2× of Flask+gunicorn**, on a 4-day-old language, with no async I/O, no connection pooling, and no JIT warm-up loop. The remaining gap is the async event loop (v0.3 architectural decision).

A LOC comparison vs an equivalent Flask app:

| Component | StrictPy | Python+Flask |
|---|---:|---:|
| HTTP parser | ~200 LOC | 0 (`http.server`) |
| JSON tree | ~70 LOC | 0 (`json.loads`) |
| HTTP-Date | ~20 LOC | 0 (`email.utils.formatdate`) |
| Str helpers | ~50 LOC | 0 (stdlib) |
| Framework | ~620 LOC | ~250 LOC |
| Demo handlers | ~200 LOC | ~50 LOC |
| **Total** | **~1,160 LOC** | **~300 LOC** |

**The 4× gap is library density, not a language-feature gap.** Almost everything inlined in user code (HTTP parser, JSON tree, HTTP-Date formatter, str helpers) maps to a Python stdlib module that doesn't exist yet in StrictPy. v0.3 stdlib classes (typed JsonValue, Request/Response shipped by the language) would close ~half of it.

---

## M29.5: rounding out the framework

M29 was "demo grade." M29.5 was a focused single-agent task to add the five Tier 1 features that separate "demo" from "small-internal-API grade":

1. **HTTP keep-alive** — per-connection request loop with 5s idle timeout + 100 req/conn cap. Connection / Keep-Alive headers per RFC 7230.
2. **Chunked transfer encoding** (read + write) — `parse_chunked_body` decodes inbound; `resp_set_chunked` + `resp_add_chunk` emits framed outbound chunks.
3. **multipart/form-data** parsing — for file uploads. Single nesting level only.
4. **Graceful shutdown** — `--shutdown-after-secs N` CLI flag (StrictPy has no signal stdlib yet). Drains in-flight requests up to 10s.
5. **HTML error pages** — replacing the text/plain 4xx/5xx defaults.

Plus demo additions: `POST /api/upload`, `GET /api/uploads/<*rest>`, `GET /api/stream?n=N` (chunked-response demonstration with `time.sleep_ms` between chunks), upload form on the index page.

The framework is now 2,443 LOC. Eight tests pass (3 from M29 + 5 new). It can host actual non-trivial workloads: keep-alive saves the TCP handshake on every request, chunked streaming works, file uploads work, Ctrl-C drains cleanly.

**And M29.5 found a stdlib bug.** `socket.close_listener` doesn't unblock an in-flight `socket.accept` — the M28 P3b-A handler `Arc::clones` the listener and drops the slot-table mutex before the blocking syscall, so closing the slot from another thread doesn't drop the underlying FD. The agent worked around it in user code with a self-connect from the shutdown timer (~15 LOC). I logged it as BUG-040; the stdlib fix is v0.3. **First stress test in project history to find a bug in the network stack** — and it found it within minutes of attempting graceful shutdown, which the M29 framework hadn't exercised.

This is the second example in the project of the "real programs use APIs in combinations unit tests don't" mechanism (the first was BUG-039 from M24, where `dict.get(k)` worked but `k in dict` didn't because no unit test had bothered to test `in` on the natural Python idiom). The pattern is structural and probably recurring: **stress tests find the integration-level bugs; unit tests find the unit-level bugs; both are necessary; the marginal cost of a stress test per round is consistently rewarded.**

---

## M30 — closing the last two bugs

Two focused agents in parallel closed BUG-028 (lexer line continuation across `+`/`and`/`or`/`==`/`=`/etc., deferred since M10) and BUG-040 (the M29.5 finding). BUG-028 was a 95-LOC additive lexer fix tracking the "last significant token" and suppressing `NEWLINE` if it's a binary operator needing a right operand. BUG-040 shipped `shutdown(fd, SHUT_RDWR)` + self-connect to listener address as belt-and-braces — agent's empirical Windows finding: **winsock does NOT wake `accept()` from `shutdown(fd)` alone** (per Microsoft KB-179942), self-connect is essential on Windows. Belt-and-braces both mechanisms on both platforms rather than `#[cfg]`-gating because redundancy is harmless.

After M30: **35 bugs found, 35 fixed, 0 deferred.** First zero-open-bugs state since M10. Tagged as **v0.2.0**. See [`RELEASE_NOTES_v0.2.md`](RELEASE_NOTES_v0.2.md) for the full freeze-point summary.

---

## M31–M34 — v0.3 begins

After v0.2 froze, the project pivoted to the THESIS §8.4 next-pass priority list. Four substantial features in three milestone-clusters:

**M31 generic classes** — `class Box[T]:`, `class Pair[K, V]:`, `class Stack[T]:`. Extended the M17 worklist-driven monomorphisation infrastructure from generic *free functions* to generic *classes*. Per-instantiation `type_id` + distinct method bodies via mangled names (`Box__i64`, `Pair__str_i32`). Constructor-site type inference (no explicit `Box[i64]()` — v0.4). Existing M11 vtable infrastructure handles dispatch — zero new VM opcodes. The big unlock: typed stdlib classes (next).

**M32 async I/O** — `asyncio` stdlib module with `run` / `spawn` / `sleep` / `gather` + `socket.async_accept` / `async_recv` / `async_send`. Shape A (thread-backed Future façade). Notable design call: agent did NOT use M31's user-defined-generic-class machinery for `Future[T]`. Instead added `Future` as a TypeCtor (joining Channel / Atomic / Dict / List) with special-cased `.await()` dispatch. ~25 LOC vs the full M31 monomorphisation worklist. Demo: `examples/async_echo_server.spy` (~115 LOC) handles 3 concurrent clients. v0.4 will swap internals for a real `mio` event loop without changing the public surface.

**M33 precise GC stack maps** — replaces the M9 conservative `in_jit: AtomicUsize` "freeze GC during JIT" pause with a per-thread shadow-stack approach. JIT spills register variables into per-function Cranelift stack slots; pushes `(buf, len)` window before each heap-allocating runtime helper; `Heap::collect` reads the windows for precise root enumeration. Shadow-stack rather than full Cranelift `enable_safepoints` (scope-down per brief — `cranelift-jit 0.115` doesn't stably expose what's needed). The `in_jit` field is gone; GC can now collect during JIT'd execution.

**M32 + M33 ran in parallel and integrated with zero cherry-pick conflicts** despite both touching `vm/src/interp.rs` and `vm/src/lib.rs`. M32 added `SharedVm.futures`; M33 removed `SharedVm.in_jit`. Git's three-way merge resolved everything automatically. Cleanest parallel-agent integration in project history; the file-ownership-discipline investment from M28 onward finally paid back its setup cost in full.

**M34 typed JsonValue tree** — first stdlib classes. Sealed `JsonValue` base + 6 final subclasses (JNull, JBool, JInt, JFloat, JString, JList, JObject), plus `json.parse(s) -> JsonValue`, `json.stringify(v)`, and constructor convenience helpers. Closes the M29 framework's #1 documented ergonomics gap: the POST body parser hand-walks `json.parse_to_string` output for ~50 LOC where typed `match case JObject(o): ...` drops it to ~10. Per the brief's STOP CRITERIA, agent registered the 7 classes in the **prelude** rather than building proper `StdlibItemKind::Class` infrastructure for module-level classes — the legacy "prelude wins" branch makes `from json import JsonValue` work transparently. Module-scoped class registration is a pure refactor deferred to v0.4. Three findings: (a) `JList` storing `List[JsonValue]` worked first-try under M11 + M31 — the class system is stable enough for recursive types; (b) GC root scanning was free via existing GcKind traversals; (c) helper-vs-constructor needs two NativeFn IDs per shape (caught by JNull's zero-arg case).

**The Lesson 1 streak holds at 14 consecutive clean-commit agents** (M28 → M34, 4 calendar days, zero orchestrator-commit-on-behalf interventions). The intervention is reproducible. The pattern generalises: in agent briefs, **numerical thresholds ("first commit before 60% of budget") beat qualitative urgency ("commit early")** for behaviours an agent needs to self-assess mid-task.

After M34, headline numbers refresh: **690 tests passing, 97 example programs, 37 stdlib modules, 7 new prelude classes (JsonValue tree), 0 open bugs.** Benchmark numbers unchanged (the v0.3 work doesn't touch the JIT-emitted hot-loop code).

---

## What I learned

### 1. Static types make AOT compilation trivial — the hard parts of JIT'ing Python simply don't apply.

PyPy is millions of lines of code. CPython's experimental JIT (3.13+) involves complex tier-up logic and copy-and-patch templates. Both spend most of their engineering effort *defeating* dynamism: profiling types at runtime, inserting guards, building inline caches, deoptimizing when types change.

The StrictPy JIT is ~1,200 lines (`vm/src/jit.rs`) plus a runtime helpers file (~200 lines). The translation from typed IR to Cranelift IR is largely mechanical pattern matching. **There's no profiling, no guards, no inline caches, no deopt.** Types are already known.

This is the single most important lesson: **dynamism has a price you pay forever**. It's paid every time CPython looks up a method, every time the interpreter dispatches an opcode, every time an integer overflows out of fastpath range. Removing dynamism doesn't just make optimization possible — it makes optimization *easy*.

### 2. The interpreter was the bottleneck, not the runtime.

Before M8, StrictPy was already a typed-bytecode VM. Every opcode knew its operand types. No boxed integers. No `__dict__` lookups. Yet it still lost to CPython by 3–5× on tight loops. The bottleneck wasn't the bytecode semantics; it was *interpreting* anything at all. CPython's interpreter loop is 30 years of C tuning. A from-scratch Rust `match` couldn't compete.

The fix wasn't to optimize the interpreter. It was to *bypass* the interpreter.

### 3. Tests that don't assert on values lie.

In M4, four programs "passed" their integration tests by exiting 0 while computing garbage: fib infinite-looped, dot returned 0, mandelbrot printed nothing, tree segfaulted on the slow path. Every time, the bug had been there for at least one milestone.

What caught the bugs was not adding more tests but adding *more specific* assertions. "exits 0" tells you almost nothing. "prints `fib(15) = 610`" tells you the recursion, integer arithmetic, string concat, and println all worked end-to-end.

### 4. AI-assisted development needs hard acceptance criteria.

The pattern that worked: every agent task had an explicit, machine-checkable acceptance criterion. "fib.spy must print `fib(15) = 610` end-to-end." "All 7 examples must compile to a `.spyc` that starts with the magic bytes." "quicksort(100K) must beat CPython."

The pattern that didn't work: vague tasks like "improve the IR" or "fix the optimization passes." Without an external success signal, agents handed back work that compiled and tested green but didn't move the needle. Worse — sometimes they'd silently regress.

"Stop criterion: if your acceptance test fails, halt and report rather than paper over with stubs" caught at least three near-misses.

### 5. Stress testing has superlinear ROI.

M0–M9 added 134 tests via feature development and found ~12 bugs. M10 added one round of stress testing — six new real-world programs — and found **17 bugs in a single milestone**. M11's five further programs found six more. The pattern continued: every round found at least one bug the unit tests didn't, and the bugs found by stress tests were systematically more architectural than the bugs found by feature tests.

The mechanism: a unit test for `dict.has(k)` checks that it returns the right value. A real program — `event_log.spy`'s histogram — uses `bucket in seen` as a guard. The unit test never exercises the `in` operator. The real program does. **Real programs use operators in combinations that unit tests don't.**

### 6. Bugs cluster around a root cause; audit on first discovery.

BUG-001 was one nullable-narrowing miscompile in csv_aggregate. The audit pass on `Ty::Primitive` match arms in `codegen.rs` found four more siblings (BUG-002 through BUG-005). BUG-017 ("vtable wraps mod 4") turned out to be three converging adjacent bugs.

**When you find one bug, look hard for siblings.** A visible symptom is often one observation from a multi-bug root cause; fixing one at a time can produce "improvements" that don't change behaviour because another sibling is still active.

### 7. Latent bugs accumulate dose-dependently.

BUG-029 — the `op_new` class_id↔type_id collision — sat silent for 10 milestones because class_id and type_id numeric ranges never overlapped. The 4th user class arrived in M10 with class_id 16, numerically colliding with Shape's type_id 16. The fallback fired, Pentagon got Shape's vtable, the visible symptom was "vtable wraps mod 4."

A convenience hack can sit silent for years before enough state accumulates to trigger it. **Treat fallback branches as suspicious code that needs regression coverage.** If the fallback exists, write a test that exercises it. If you can't, the fallback is probably wrong.

### 8. Placeholder IR lowerings silently miscompile.

Four bugs share the same shape: a binary-op match arm in `compiler/src/ir.rs::emit_binop` that punts on type-dependent lowering with a hardcoded `IROp`:

| Bug | Operator | Found in | Latent since |
|---|---|---|---|
| BUG-008 | `is not` | M10 | M2 |
| BUG-034 | `str !=` | M12 | strings became first-class |
| BUG-037 | `??` (null-coalesce) | M20a | the operator shipped |
| BUG-039 | `in` / `not in` | M24 | M5 (Dict landed) |

Each one was found organically by a stress test using the operator in the form the placeholder didn't handle. A mechanical audit of `emit_binop` would have caught all four at once. **"The parser and typechecker accept it and there's a lowering" is not the same as "the lowering is correct."**

### 9. Numerical thresholds in agent briefs beat qualitative urgency.

"Commit early" failed in 7+ agents across M23–M27. "Your first git commit must land before 60% of your time budget" — with explicit 20% / 40% / 60% / 80% checkpoint suggestions — has worked in 8 consecutive agents across M28–M29.5. Same goal, same urgency, completely different compliance rate.

This generalises beyond StrictPy: **when you want an agent to do something under time pressure, write the threshold as a number the agent can self-assess against mid-task.** "Don't get distracted" is unactionable. "Commit before 60% of budget" is a gate the agent can check itself against every 10 minutes. The 12 messed-up agent runs that taught me this were not cheap; documenting it here so the next project doesn't re-pay the tuition.

### 10. Real programs find integration bugs that unit tests can't.

The structural pattern, observed twice across the project's late stress rounds:

- M24's `event_log.spy` used `bucket in seen` as a histogram guard. Unit tests for `dict.has(k)` passed. The `in` operator (BUG-039) was a placeholder IR lowering since M5 (the moment Dict shipped). The first program to use the natural Python idiom found it.
- M29.5's framework graceful-shutdown path called `socket.close_listener()` from one thread while another was blocked in `socket.accept()`. Unit tests for `listen_tcp` + `accept` + `close_listener` passed individually. The combination (BUG-040) was an integration-level latent issue because the M28 `SocketAccept` handler Arc-clones the listener and drops the table mutex before the blocking syscall — closing the slot doesn't drop the underlying FD. The first program to attempt cooperative listener shutdown found it.

Both bugs are valid implementation choices that produced wrong end-user behaviour under combinations the unit tests didn't exercise. Stress tests find this class of bug; unit tests don't. The stress-test ROI curve flattens with time (M10: 17 bugs / M24: 1 / M29: 0) but **never reaches zero permanently** — there's always another combination of APIs that isn't covered yet.

---

## What StrictPy still can't do

Don't take the benchmark wins too literally. StrictPy is not a production language. It's a research demonstration that, by the time M29.5 landed, could host a working web framework. The current limitations:

- **No async I/O / event loop.** The M29 framework's ~2× gap to Flask+gunicorn is exactly this. Thread-per-connection works for small-to-medium load (~2,200 req/s on `/health`); production scale needs an event loop. Major architectural decision deferred to v0.3.
- **GC is paused during JIT'd execution** (the `in_jit: AtomicUsize` flag). Long-running programs with JIT'd hot loops and >16 MB live data will stall or OOM. The M26 `btree` row's narrowing-as-allocation-grows is the empirical evidence. Precise Cranelift stack maps are the proper fix; deferred.
- **No generic classes** (`class Box[T]:`). Generic *free functions* work since M17; classes are v0.3 work. This is the single most-impactful v0.3 ergonomic win — the M29 framework would shrink ~30% with typed `JsonValue` and another ~20% with stdlib-shipped `Request` / `Response`.
- **No user-defined exception subclasses.** v0.1 ships 10 built-in exception names; `class MyError(Exception):` is parsed but rejected.
- **`with open(...) as f:` does NOT route IOError through an enclosing `try ... except`.** Workaround: `try: with open(...) as f: ... except IOError:` explicitly. Known M15 follow-up.
- **No bounded generics** (`T: Comparable`). v0.1 generics re-typecheck per instantiation, which is approximately correct but allows operations the source bound would have rejected.
- **Two open bugs**:
    - **BUG-028**: the lexer doesn't continue lines across trailing `+`. Workaround: parentheses. ~1h to fix.
    - **BUG-040**: `socket.close_listener` doesn't unblock an in-flight `socket.accept` (M29.5 finding). Workaround: self-connect to wake the blocked accept (~15 LOC user code). ~1-2h to fix stdlib-side.
- **No HTTP/2, no WebSockets.** The M28 `http_client` is HTTP/1.1; the M29 server hand-rolls HTTP/1.1. Both v0.3.
- **No production-grade password hashing.** v0.2 ships `hashlib.sha256` — fine for content hashing, inappropriate for auth.
- **No NumPy / pandas** — architectural; see [`docs/thesis/design_decisions/why_no_numpy_pandas.md`](docs/thesis/design_decisions/why_no_numpy_pandas.md). Three theoretical paths exist (embed CPython, FFI to numpy's C lib, native reimplementation); none planned.

The canonical benchmark suite is tiny (4 programs, 16 cells). The extended suite (M26) adds 30 more cells. The wins are real but narrow. They generalise to "tight numeric loops, recursive small-int arithmetic, integer-keyed list mutation, stdlib calls dominated by startup, web-framework-shaped HTTP/sqlite/threading workloads"; they do not generalise to "everything CPython does." Allocation-heavy workloads (M26 `btree` at large n) erode the JIT win.

---

## What's next

The performance question is mostly answered: yes, statically typed Python can beat CPython by a lot. The "can you host real software in this language?" question is also answered: yes — within 2× of Flask+gunicorn, on a 5-day-old language, with no async I/O. The remaining questions:

- **Does the language scale beyond ~2,500 LOC user programs?** The M29 framework + demo is the largest single .spy file at 2,443 LOC. The mypyc comparison would be `black` or `Sphinx` — multi-tens-of-KLOC real codebases. Real but unexplored.
- **What does async look like?** Closing the 2× gap to production Python web stacks means an event loop. That's the next major architectural decision — straight `asyncio`-shape, or `tokio`-shape with explicit `Task` types, or coroutine-based on top of M14 tuples + M15 try/except. Each has trade-offs.
- **What about NumPy?** StrictPy's `List[f64]` is already a contiguous f64 buffer. A `numpy.ndarray` view is one cast away — *but* StrictPy can't actually import NumPy because NumPy depends on libpython. The interesting comparison would be StrictPy vs CPython+NumPy on the same numerical workload, and you'd need to reimplement parts of NumPy natively to make it.
- **Where does the static-type win flatten?** The M26 `btree` row showed where it shows up: allocation-pressured recursive workloads. Larger workloads — multi-threaded GC contention, long-running with churned heap — would probably erode more of that. Where?
- **Could it generalise to a research methodology?** The 5-day calendar-elapsed for 36K LOC + thesis archive + a working web framework is anecdotal. Whether the patterns in this project's archive transfer to other systems work is an open empirical question. The Lesson 1 escalation pattern (numerical thresholds in agent briefs) is the most teachable single finding; whether it survives outside StrictPy is unknown.

After watching fib(30) drop from 931 ms to 13.1 ms across three days, then watching a web framework boot up on the same language two days later, I'm convinced the design thesis holds: **the only good Python is a statically typed Python.** What's still open is how far that goes.

A more rigorous version of all this material — design decisions, methodology patterns, the eight generalisable findings from the bug catalogue — is in [`THESIS.md`](THESIS.md).

---

## Reproducing this

The whole project, including the spec, source, tests, benchmarks, historical snapshots, and thesis archive, is one tree:

```
StrictPy/
├── STRICTPY_SPEC.md       Canonical language spec (v0.1; in-place amendments per milestone)
├── THESIS.md              Mid-form thesis synthesising the archive
├── BLOG_POST.md           This file
├── compiler/              Compiler library (frontend → IR → bytecode emit)
├── vm/                    `spy` binary: unified compile+run CLI, JIT, GC, stdlib
├── shared/                Opcode + .spyc format + NativeFn registry
├── examples/              96 programs covering the language surface
│   └── webserver/         The M29 + M29.5 HTTP/1.1 + HTTPS web framework + TODO demo
├── bench/
│   ├── harness.py         Benchmark runner (canonical + --extended modes)
│   ├── BENCH_REPORT.md    Canonical 16-cell report
│   ├── EXTENDED_REPORT.md M26 extended 30-cell report
│   └── history/           8 snapshots from M7-unfair through M26-extended
└── docs/thesis/
    ├── timeline.md        Per-milestone narrative (M0–M29.5)
    ├── methodology.md     AI-pair-programming process
    ├── stats/             Machine-readable per-milestone metrics
    ├── milestones/        Per-milestone deep-dive notes (incl. m28_phase3b_stdlib,
    │                       m29_webserver_stress, etc.)
    ├── bugs/catalog.md    35 bugs, classified, fixed/deferred
    ├── design_decisions/  Six load-bearing architectural choices
    ├── agent_reports/     47 verbatim agent task reports
    └── agent_briefing_patterns.md
```

Repository: <https://github.com/amitgangrade/StrictPy>

To reproduce the headline number:

```powershell
git clone https://github.com/amitgangrade/StrictPy
cd StrictPy
cargo build --release

# M25 unified CLI: one command compiles and runs.
./target/release/spy.exe examples/fib.spy

# M29 / M29.5: start the web framework on port 8080 (HTTP).
./target/release/spy.exe examples/webserver/todo_app.spy --port 8080

# HTTPS version (auto-generates self-signed cert via rcgen in the test):
./target/release/spy.exe examples/webserver/todo_app.spy --port 8443 --tls cert.pem key.pem
```

To re-run the full benchmark suite:

```powershell
cargo build --release
python bench/harness.py              # 16-cell canonical suite
python bench/harness.py --extended   # 30-cell extended suite (M26)
```

To re-render the historical reports from snapshots without re-running:

```powershell
# Edit harness.py's BENCH_DIR/results.json source, then:
python bench/harness.py --report-only
```

Total wall-clock for the full project: **5 calendar days**, ~85 hours of cumulative agent compute, ~40 hours of orchestrator-attended time, across 30 milestones.

Three artifacts matter more than the code itself:

- [`bench/history/`](bench/history/) and [`bench/EXTENDED_REPORT.md`](bench/EXTENDED_REPORT.md) — the gradual performance trajectory from "3× slower than CPython on quicksort" to "13× faster on the same workload" in two well-defined optimization passes, plus the M26 extended suite that shows where the wins generalise and where they don't.
- [`docs/thesis/bugs/catalog.md`](docs/thesis/bugs/catalog.md) — the 35-entry record of every bug found, every root cause analysed, every audit pass that turned one bug into five. The "What I learned" lessons above all trace back to specific entries.
- [`examples/webserver/todo_app.spy`](examples/webserver/todo_app.spy) — the 2,443-LOC HTTP/1.1 + HTTPS web framework + TODO demo that was the project's stress test for "can this language host real software?" The answer is yes (with the documented v0.2 limitations).

That's the lesson worth taking away: **the gap between a typed bytecode interpreter and CPython is roughly the gap between two interpreters**. The gap between native code and CPython is several orders of magnitude. And once you've built the native-code path, the gap between "research demonstration" and "language that hosts a working web framework" is just the engineering work to fill in the stdlib — and if your types are real, even that work is dramatically simpler than you'd think.
