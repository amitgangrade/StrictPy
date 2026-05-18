# M9 — full JIT coverage

**Brief**: extend the JIT to cover heap mutation, fields, virtual calls,
allocation. Goal: flip the remaining 4 cells where CPython beat StrictPy
(quicksort 50K/100K, dot 500K/1M).

**Wall-clock**: ~21 minutes
**Tool uses**: 99

## Result

**Every cell flipped.** Tally: 16/16 cells beat CPython. quicksort(100K)
went from 679 ms (3× slower than CPython) to 18.6 ms (13× faster) — a
36× speedup. dot(1M) went from 478 ms to 54 ms (8.8× faster vs M8;
4.4× faster vs CPython).

| Cell | M8 ratio | M9 ratio | Speedup |
|---|---:|---:|---|
| quicksort 50K | 2.35× slower | **0.11× (9× faster)** | 22× |
| quicksort 100K | 3.03× slower | **0.08× (13× faster)** | 36× |
| dot 500K | 1.62× slower | **0.20× (5× faster)** | 8× |
| dot 1M | 1.92× slower | **0.23× (4× faster)** | 8× |

## What landed (3 categories)

**Inlined ops (no helper call)**: `ArraySet`, `LoadField`, `StoreField`.
The GC is non-moving, so heap pointers are stable for the object's
lifetime. Cranelift emits a direct load/store at `base + offset`.

**Runtime helpers (`extern "C"` Rust)**:
- `rt_list_push` — grows the backing buffer on capacity exceed
- `rt_list_new` / `rt_array_new` — allocates ListRepr + initial backing
- `rt_alloc` — allocates user-class instance with vtable header
- `rt_virtual_call` — re-enters interpreter for vtable dispatch

**GC safety: the `in_jit: AtomicUsize` counter** (see
`design_decisions/conservative_gc_with_in_jit_pause.md`). When JIT'd code
calls a runtime helper that might allocate, GC must not run — heap
pointers in CPU registers are invisible to the conservative root scan.
The counter bracketed every JIT entry. Heap collection skips when
non-zero.

## Per-example coverage after M9

| Example | Functions JIT'd / total |
|---|---|
| hello.spy | 1/1 |
| fib.spy | 2/2 |
| mandelbrot.spy | 2/2 |
| **tree.spy** | **6/6** (was 1/6 in M8) |
| dot.spy | 2/2 (was 1/2) |
| producer.spy | 3/5 (RefEq, ClosureNew unsupported) |
| wordcount.spy | 4/5 (RefEq unsupported) |

## Rough edges left for future work

- **GC starvation under `in_jit`** — long-running programs leak.
- **VirtualCall round-trips the interpreter** via `rt_virtual_call`. A vtable
  storing JIT function pointers directly would let us emit `call_indirect`.
- **No bounds checks in JIT** — `ArraySet`/`ArrayGet` traps now segfault
  rather than raising `IndexError`.
- **NewInit fusion** — compiler emits `New + DirectCall(__init__)`; JIT
  compiles both fine but a fused op would skip one bytecode roundtrip.
- **Error propagation from JIT** — `rt_virtual_call` and native trampoline
  swallow `Err(_)` into 0. Need an out-of-band error slot.

## Why this milestone was fast (21 min)

M8 had locked in the architecture (unified ABI, decompilation approach,
per-function opt-in with fixpoint). M9 was *pure coverage extension* —
mechanical pattern-matching against existing translation patterns.
No new design decisions. The 21-minute wall-clock vs M8's 36 minutes
reflects how much of the M8 work was foundational.
