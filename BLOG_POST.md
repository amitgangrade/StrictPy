# I built a statically typed Python from scratch, and it beats CPython by up to 17×

*A journey from "is this even possible?" to a 13K-line Rust toolchain with a Cranelift JIT — and what it taught me about why dynamic languages are slow.*

---

## TL;DR

Over two weeks of (heavily AI-assisted) work, I built **StrictPy** — a statically typed dialect of Python with its own compiler and bytecode VM. The current implementation wins against CPython 3.12 on every cell of a small benchmark suite:

| Benchmark | StrictPy | CPython 3.12 | StrictPy is… |
|---|---:|---:|---:|
| fib(30) recursive | 13.5 ms | 159.5 ms | **12× faster** |
| fib(33) recursive | 34.8 ms | 537.7 ms | **15× faster** |
| quicksort(100K) | 18.6 ms | 238.6 ms | **13× faster** |
| dot product (1M f64) | 54.0 ms | 239.1 ms | **4× faster** |
| Mandelbrot 60×30 | 13.6 ms | 56.6 ms | **4× faster** |

How did this happen? The short answer: **static types make AOT compilation easy, and AOT compilation crushes any interpreter.** The interesting answer is everything that had to be true before that punch line landed — and the embarrassing mistakes we made along the way.

This is the journey.

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
compiler/  spyc binary: .spy → .spyc bytecode
vm/        spy binary: loads .spyc, runs bytecode (interpreter + Cranelift JIT)
```

The compiler pipeline mirrors what any small language has: lex → parse → resolve → typecheck → IR lower → optimize → emit bytecode → write. The VM loads the bytecode, optionally JITs each function, then dispatches calls either to native code or to a plain `match`-over-`Opcode` interpreter loop.

By the end, this is ~13,000 lines of Rust. The two biggest files: `compiler/src/ir.rs` (~2,000 lines, IR lowering with monomorphization) and `vm/src/interp.rs` (~1,900 lines, the interpreter + opcode handlers).

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

## What I learned

### 1. Static types make AOT compilation trivial — the hard parts of JIT'ing Python simply don't apply.

PyPy is millions of lines of code. CPython's experimental JIT (3.13+) involves complex tier-up logic and copy-and-patch templates. Both spend most of their engineering effort *defeating* dynamism: profiling types at runtime, inserting guards, building inline caches, deoptimizing when types change.

The StrictPy JIT is one Rust file (~1,200 lines) plus a runtime helpers file (~200 lines). The translation from typed IR to Cranelift IR is largely mechanical pattern matching. **There's no profiling, no guards, no inline caches, no deopt.** Types are already known.

This is the single most important lesson: **dynamism has a price you pay forever**. It's paid every time CPython looks up a method, every time the interpreter dispatches an opcode, every time an integer overflows out of fastpath range. Removing dynamism doesn't just make optimization possible — it makes optimization *easy*.

### 2. The interpreter was the bottleneck, not the runtime.

Before M8, StrictPy was already a typed-bytecode VM. Every opcode knew its operand types. No boxed integers. No `__dict__` lookups. Yet it still lost to CPython by 3-5× on tight loops. The bottleneck wasn't the bytecode semantics; it was *interpreting* anything at all. CPython's interpreter loop is 30 years of C tuning. A from-scratch Rust `match` couldn't compete.

The fix wasn't to optimize the interpreter. It was to *bypass* the interpreter.

### 3. Tests that don't assert on values lie.

The pattern repeated four times across the project: a program would "pass" its integration test by exiting 0, but the actual computation was wrong (empty list, zero const, infinite loop, bitwise NOT instead of logical). Every time, the bug had been there for at least one milestone.

What caught the bugs was not adding more tests but adding *more specific* assertions. "exits 0" tells you almost nothing. "prints `fib(15) = 610`" tells you the recursion, integer arithmetic, string concat, and println all worked end-to-end.

### 4. AI-assisted development needs hard acceptance criteria.

I used Claude Code for this entire project, spawning sub-agents for each milestone. The pattern that worked: every agent task had an explicit, machine-checkable acceptance criterion. "fib.spy must print `fib(15) = 610` end-to-end." "All 7 examples must compile to a `.spyc` that starts with the magic bytes." "quicksort(100K) must beat CPython."

The pattern that didn't work: vague tasks like "improve the IR" or "fix the optimization passes." Without an external success signal, the agents would hand back work that compiled and tested green but didn't move the needle. Worse — sometimes they'd silently regress.

The discipline of "stop criterion: if your acceptance test fails, stop and report what's blocking rather than papering over with stubs" caught at least three near-misses.

### 5. Methodology matters more than micro-optimization.

The biggest single performance "improvement" in the project was actually the fairness fix to the benchmark harness — pre-compiling Python to `.pyc` so the comparison didn't include parse time. That changed some ratios by 30-50% without writing a single line of optimization code. Honest measurement beats clever optimization every time.

---

## What StrictPy still can't do

Don't take the benchmark wins too literally. StrictPy is not a production language. It's a research demonstration. The current limitations are substantial:

- **GC is paused during JIT'd execution** (the `in_jit` flag). Long-running programs would eventually OOM. Precise stack maps are the proper fix.
- **No exception handling.** Parser accepts `try`/`except`; codegen drops it.
- **No for-loops.** Use `while` with an index. Parser accepts `for x in iter:`; IR doesn't desugar it.
- **`producer.spy`** runs but its inner closures aren't JIT'd because `RefEq` and `ClosureNew` aren't yet supported.
- **Inheritance-stable vtables** aren't enforced — subclasses that *add* virtual methods (vs only overriding) get wrong slot numbers.
- **Strings are UTF-8 bytes**, no side index for fast non-ASCII code-point access.
- **`int` is `i64`** — no `BigInt` runtime yet despite the type being declared.

Most importantly: the benchmark suite is tiny (four programs). The wins are real but narrow. Real-world programs would expose bugs we haven't hit.

---

## What's next

The benchmark wins shift the interesting questions. The performance question is mostly answered: yes, statically typed Python can beat CPython, by a lot. The remaining questions are:

- **Is it usable?** Can you write a non-trivial program in StrictPy without hitting one of the limitations above?
- **Does the type system pay for itself in error catching?** The 20-case conformance suite catches structural errors at compile time that CPython would catch at runtime (if at all). How much does that matter in practice?
- **How does it interact with NumPy?** StrictPy's `List[f64]` is already a contiguous f64 buffer. A `numpy.ndarray` view is one cast away. The interesting comparison isn't StrictPy vs CPython — it's StrictPy vs CPython+NumPy.
- **Does it scale to a real codebase?** 13K lines is a toy. Even mypyc — the closest existing tool — has compiled mypy itself (~50KLOC). A real test would be compiling something like `black` or a small web framework.

I don't know the answers to any of these. But after watching fib(30) drop from 931ms to 13.5ms over two weeks of focused work, I'm convinced the design thesis is right: **the only good Python is a statically typed Python.**

---

## Reproducing this

The whole project, including the spec, source, tests, benchmarks, and historical snapshots, is in one tree:

```
PythonCompiler/
├── STRICTPY_SPEC.md       Canonical language spec
├── compiler/              spyc binary: .spy → .spyc
├── vm/                    spy binary: loader + interpreter + Cranelift JIT
├── shared/                Opcode + NativeFn definitions
├── examples/              Seven programs covering the language surface
├── bench/
│   ├── harness.py         Benchmark runner
│   ├── BENCH_REPORT.md    Current report
│   └── history/           Snapshots at each milestone (m7, m8, m9)
└── README.md
```

To reproduce the headline number:

```powershell
cargo build --release
./target/release/spyc.exe examples/fib.spy -o fib.spyc
./target/release/spy.exe fib.spyc
```

To re-run the full benchmark suite:

```powershell
cargo build --release
python bench/harness.py
```

To re-render the historical reports from snapshots without re-running:

```powershell
# Edit harness.py's BENCH_DIR/results.json source, then:
python bench/harness.py --report-only
```

Total wall-clock for the full project: roughly two weeks elapsed, ~25 hours of agent compute, ~$200 in API costs.

The most valuable artifact isn't the code — it's the `bench/history/` directory, which shows the gradual performance improvement that gets you from "3× slower than CPython on quicksort" to "13× faster on the same workload" in two well-defined optimization passes.

That's the lesson worth taking away: **the gap between a typed bytecode interpreter and CPython is roughly the gap between two interpreters**. The gap between native code and CPython is several orders of magnitude. The only thing standing between you and that gap is the engineering work to bridge it — and if your types are real, that work is dramatically simpler than you'd think.
