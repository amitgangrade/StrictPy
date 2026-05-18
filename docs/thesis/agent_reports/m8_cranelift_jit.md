# M8 — Cranelift AOT compilation

**Brief**: AOT-compile every IR function to native code via Cranelift at
module-load time. Goal: beat CPython on the benchmark suite.

**Wall-clock**: ~36 minutes
**Tool uses**: 140

## Result

**fib(30) went from 931 ms to 14.6 ms.** A 64× speedup vs the M7
interpreter, and 11× faster than CPython 3.12 (which runs fib(30) in 163 ms).

Benchmark tally flipped from `5W/3T/8L` (M7-fair) to `10W/2T/4L`.

| Benchmark | M7 (interp) | M8 (JIT) | M7→M8 | vs CPython |
|---|---:|---:|---:|---|
| fib(30) | 931 ms | 14.6 ms | **64×** | **11× faster** |
| fib(33) | 2410 ms | 35.5 ms | **68×** | **15× faster** |
| Mandelbrot | 25.4 ms | 12.5 ms | 2× | **4.6× faster** |
| quicksort(100K) | 660 ms | 679 ms | unchanged | 3× slower (M9 fixed) |
| dot(1M) | 604 ms | 478 ms | 1.3× | 1.9× slower (M9 fixed) |

## Architecture decisions locked in this task

1. **Unified ABI** (see `design_decisions/unified_jit_abi.md`).
2. **Per-function opt-in with fixpoint disable** (see
   `design_decisions/per_function_jit_opt_in.md`).
3. **Bytecode decompilation at VM load.** Rather than modify the `.spyc`
   format, the VM decodes the bytecode back into a typed op stream
   (`vm/src/decompile.rs`) and feeds that to the JIT. Compiler crate
   unchanged.
4. **`feature = "jit"`** (default on). `--no-default-features` builds the
   VM without Cranelift in the dependency tree — sanity check that the
   interpreter still works alone.

## Critical Windows-specific gotcha

`CallConv::SystemV` is WRONG on Windows-x86_64. Rust `extern "C"` uses
`WindowsFastcall`. Without `host_call_conv()` selecting the right
convention per platform, JIT'd code would link but crash on first call.
This gotcha would have been invisible on Linux CI; we caught it because
the dev environment was Windows.

## Coverage at end of M8

| Example | Functions JIT'd / total |
|---|---|
| hello.spy | 1/1 |
| fib.spy | 2/2 (fully JIT'd) |
| mandelbrot.spy | 2/2 (fully JIT'd) |
| producer.spy | 3/5 |
| wordcount.spy | 3/5 |
| dot.spy | 1/2 (main uses ListNew → interpreted) |
| tree.spy | 1/6 (every method uses LoadField → interpreted) |

The "1/6 on tree" and "1/2 on dot" results foreshadowed M9 — most of the
losing benchmarks were spending their time in interpreted code, not
JIT-bottleneck.

## Files created
- `vm/src/decompile.rs` — bytecode → typed op stream
- `vm/src/jit.rs` — Cranelift translator + trampoline
- `vm/tests/jit_coverage.rs` — per-example coverage telemetry
- `vm/Cargo.toml` — Cranelift 0.115 deps behind `feature = "jit"`
