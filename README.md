# StrictPy

A statically typed dialect of Python, with a dedicated compiler and bytecode VM.

## Documents to read first

- **Writing StrictPy code?** Read **[LANGUAGE_GUIDE.md](LANGUAGE_GUIDE.md)** — the AI-friendly language reference. Every syntax form, every stdlib function, every gotcha, with examples. Designed so AI coding tools can generate idiomatic StrictPy without reading the compiler source.
- **Working on the compiler / VM internals?** Read **[STRICTPY_SPEC.md](STRICTPY_SPEC.md)** — the canonical 1800+ line implementation reference. Every design decision in the compiler/VM source traces back to a section of the spec.
- **Understanding the project's history / methodology?** Read **[THESIS.md](THESIS.md)** (mid-form technical thesis) and **[BLOG_POST.md](BLOG_POST.md)** (narrative version). The full per-milestone archive lives at `docs/thesis/`.
- **Resuming a session?** Read **[HANDOFF.md](HANDOFF.md)** — current state, in-flight work, integration recipes.
- **Latest frozen release?** **[RELEASE_NOTES_v0.2.md](RELEASE_NOTES_v0.2.md)** (tag `v0.2.0`).

## Workspace layout

```
.
├── STRICTPY_SPEC.md      Canonical language + VM specification
├── Cargo.toml            Workspace manifest
├── shared/               Definitions used by both compiler and VM
│   └── src/
│       ├── opcode.rs     Bytecode opcode enum (spec §13)
│       ├── file_format.rs `.spyc` header, constant tags, sentinels (spec §12)
│       └── type_tag.rs   Inline operand type tags (spec §13.3.6)
├── compiler/             compiler library (frontend → bytecode emitter)
├── vm/                   `spy` — single unified CLI (compile + run)
└── examples/             Sample StrictPy programs
```

## Implementation language

Rust, throughout. The two top-level rationales:

- **One language across the toolchain** keeps the dependency surface small and lets
  us share crates (notably `strictpy-shared`) without FFI glue.
- **Algebraic data types** are well suited to AST / IR work; the borrow checker
  catches most lifetime issues in the compiler.

The dispatch loop in the VM uses `unsafe` for direct-threaded dispatch (computed-goto
analogue via a function-pointer table). That `unsafe` is bounded to one module
(`vm/src/interp.rs`). If you want to swap the VM out for a C or Zig implementation
later, the `.spyc` format is fully specified and the shared crate is FFI-friendly.

## Building

```powershell
cargo build
cargo test
```

Run an example — `spy` accepts `.spy` sources directly (compile-if-stale +
run, with bytecode cached in `__spycache__/`) or precompiled `.spyc`
modules:

```powershell
# Python-style: source in, compile-and-run, cache to __spycache__/.
cargo run --bin spy -- examples/hello.spy

# Already-compiled .spyc bytecode runs the same way.
cargo run --bin spy -- examples/hello.spyc

# Compile only, no execute (analogue of `python -m py_compile`).
cargo run --bin spy -- --compile-only examples/hello.spy

# Inline mode (analogue of `python -c "..."`).
cargo run --bin spy -- -c 'fn main() -> i32:\n    println("hi")\n    return 0'
```

## Implementation status

Following the milestones in spec §19:

- [x] M0 — Spec frozen at v0.1
- [x] M1 — Lexer + parser + pretty-printer (all 7 examples lex / parse / round-trip)
- [x] M2 — Resolver + type checker (20-case conformance suite; all 7 examples typecheck)
- [x] M3 — IR lowering + simple opts + bytecode emission (.spyc writer)
- [x] M4 — Loader + interpreter + object model + stop-the-world mark-sweep GC
- [x] M5 — Native stdlib (math, file IO, channels, dicts, range, str helpers)
- [x] M6 — Real OS threading (Arc'd module + per-thread interp); compiler-side
  vtable + lambda fixes; generational GC deferred to M8
- [x] M7 — Runtime-class method dispatch for stdlib types (Channel / File / Dict /
  str), Dict subscripts, `with`-block desugaring. **All 7 examples now run
  end-to-end.** Plus several real correctness bugs caught and fixed (see
  "incidental fixes").
- [x] M8 — **Cranelift AOT compilation.** Each IR function with JIT-supported
  ops gets compiled to native code at module load. Fully JIT'd benchmarks
  beat CPython 3.12 by 5-15× (see `bench/BENCH_REPORT.md`). fib(30) went
  from 931ms → 14.6ms — a **64× speedup vs the M7 interpreter**, and
  **11× faster than CPython**.
- [x] M9 — **Full JIT coverage** for heap mutation (ArraySet/ListPush/ArrayNew),
  field access (LoadField/StoreField), allocation (New via runtime helper),
  and virtual calls. **StrictPy now beats CPython 3.12 on every benchmark
  cell** by 4-17×. fib stayed at 11×, quicksort 100K went from 3× slower
  to 12× faster, dot 1M went from 2× slower to 4× faster.
- [x] M10 — 6 real-world programs (csv_aggregate, game_of_life, sudoku,
  json_parse, markov, kvstore, brainfuck) surface 17 bugs. 11 fixed
  including `is not none` inverted, `str(char)` codepoint, `dict.has`,
  `char(i32)`, `list.pop`. Nullable-narrowing audit catches 4 more
  silent miscompiles in codegen. Stdlib gains `for x in xs:`,
  `str.split(sep)`, `sorted()`/`sort()`.
- [x] M11 — 5 more programs (lambda_calc, calculator, tictactoe,
  levenshtein, lisp). **Class system overhauled.** Closed BUG-015/016/017
  plus newly-found N1 (vtable >4 slots unreachable) and N2 (deterministic
  heap corruption on subclass-with-class-ref-fields + virtual call).
  Root cause for the vtable-mod-4 symptom turned out to be a
  **`class_id` vs `type_id` collision in `op_new`** — a latent M3-era
  hack that only worked while id ranges didn't overlap. Primitive ctors
  `i32(x)` / `i64(x)` / `f64(x)` / `char(x)` now dispatch by arg type
  (was silently truncating). **BUG-026/027 (heap corruption) provisionally
  closed** — calculator + json_parse now run 5/5 cleanly, were 0/3 before.
- [ ] M12 — line continuation across infix operators (BUG-028), precise
  stack-map GC (replaces M9 `in_jit` pause), `isinstance` / match
  exhaustiveness, exception handling codegen, generics in user code

### What actually runs

`cargo build --release` then any of the 7 examples:

```powershell
# M25+ unified CLI: spy <file.spy> compiles-if-stale and runs.
./target/release/spy.exe examples/hello.spy
./target/release/spy.exe examples/fib.spy
./target/release/spy.exe examples/dot.spy
./target/release/spy.exe examples/tree.spy
./target/release/spy.exe examples/mandelbrot.spy
./target/release/spy.exe examples/producer.spy
# wordcount needs an input.txt in the cwd:
echo "the quick brown fox the lazy dog the quick fox" > input.txt
./target/release/spy.exe examples/wordcount.spy

# Precompiled .spyc files (legacy two-step workflow) still run unchanged:
./target/release/spy.exe --compile-only examples/hello.spy -o hello.spyc
./target/release/spy.exe hello.spyc
```

All 7 produce real output:
- greeting, Fibonacci 0..610, dot product = 70.0, tree sum = 15, ASCII fractal
- producer/consumer (real OS threads sharing a channel) prints `got 0` through `got 99`
- wordcount prints `unique words: 6` plus a per-word frequency table

### Test totals

`cargo test --workspace` → **134 passing, 0 failing, 1 ignored** across 23 test binaries:
- 83 compiler unit tests (lexer 11 + parser 25 + pretty 19 + resolver 6 + typecheck 13 + ir/codegen/opts 9)
- 28 VM unit tests
- 20-case negative conformance + positive conformance
- Lex / parse / round-trip / typecheck / compile_examples integration over all 7 example files
- VM threading integration (8 concurrent workers on a shared channel)
- **All 7 example end-to-end runs verified**

### Incidental fixes from M7

While wiring runtime-class dispatch the agent uncovered three real correctness
bugs that had been producing wrong-looking-but-not-obviously-wrong behavior:

- **`not x` was emitting bitwise NOT** instead of logical negation —
  `not 1` returned `0xFFFFFFFE` (truthy!). Every `if not …:` was silently
  wrong. Fixed to emit `x == 0`.
- **`none` was stored as `0`** instead of the `NONE_SENTINEL` constant, so
  `if v is none:` matched zero-valued integers and zero-byte pointers.
- **`Thread(closure)` emitted a generic `Alloc`** returning a zeroed header,
  so spawned threads always saw a null closure.

### Known gaps remaining

- **Generational GC + precise stack maps** — deferred to M8. The current
  mark-sweep is conservative and stop-the-world; it's correct across threads
  but can keep dead objects alive through false-positive root scans.
- **Inheritance-stable vtables** — subclasses that *add* virtual methods get
  wrong slot numbers. Works for `tree.spy` because subclasses only override.
- **`for x in iter:`** — parser accepts; IR doesn't desugar to
  `__iter__`/`__next__`. Use `while` with an explicit index for now.
- **`try`/`except`** — parser accepts; codegen doesn't lower exception
  tables. No example uses exceptions.
- **JIT coverage for heap-mutating ops** — `ArraySet`, `ListPush`, `Alloc`,
  `LoadField`, `StoreField`, `VirtualCall` all fall back to the interpreter.
  Adding ~3 runtime helpers (alloc_list, list_push, array_set) and a vtable-
  pointer-load primitive would unblock quicksort, dot at large sizes, and
  every tree.spy method.
- **`try_recv` returns the same `none` sentinel for "empty" and
  "disconnected"** — benign race in the producer/consumer pattern; the
  consumer test allows any prefix ≥10 lines.
- `debug_dot_capture` — leftover debugging scratchpad, ignored to keep
  `cargo test` output clean.

## License

Dual-licensed under either of:

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall
be dual-licensed as above, without any additional terms or conditions.
