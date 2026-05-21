# Design decision: conservative mark-sweep GC + `in_jit` pause

**Milestone introduced**: M4 (basic GC), M9 (in_jit pause)
**Status**: in production; documented as a v1 compromise
**Trade-off**: correctness under any allocator pattern vs precision (and ability to collect during JIT'd execution)

## Two related choices

### 1. Conservative root scanning

The mark phase doesn't know which register slots in a stack frame hold
heap pointers vs unrelated integers. It treats EVERY 8-byte slot as a
potential pointer; if the bit pattern matches an address in the
allocated-objects table, the GC marks it live.

False positives are possible: an integer like `0x7FFE12340000` might
happen to alias a heap allocation and keep it alive past its useful
lifetime. False positives are *safe* (the worst case is wasted memory),
just imprecise.

False negatives are NOT possible — if the register actually holds a
pointer, conservative scan WILL find it.

### 2. The `in_jit: AtomicUsize` counter (M9 addition)

When JIT'd code calls a runtime helper that might allocate
(`rt_list_push` triggering a grow, `rt_alloc` creating a fresh instance),
the helper takes the heap lock. If GC ran inside that helper, it would
scan only the interpreter's frame register file — but the JIT'd function's
register state lives in real CPU registers, not in any scannable buffer.

A heap pointer held only in a JIT'd function's CPU register would be
invisible to GC, marked dead, and freed under the active JIT code.

The fix: `SharedVm.in_jit: AtomicUsize`. The interpreter's `op_call_direct`
increments before invoking JIT'd code; decrements after. `Heap::collect`
early-returns when the counter is non-zero. **GC is paused for the
lifetime of any JIT call chain.**

## Why conservative + paused

We needed *something* that worked under threading + JIT. The honest
alternatives:

**Precise GC** (the "proper" answer): emit stack maps at every safepoint,
listing exactly which registers and stack slots hold pointers. Requires
Cranelift to emit safepoint metadata (it can, but it's significant work).
Requires every native helper to insert safepoints. Requires the GC to walk
the stack via the maps. **Estimated 2-3K LOC of careful work.** Deferred
to a future milestone.

**Stop-the-world barrier at every helper call**: the helper releases the
heap to whomever wants it, GC walks, helper resumes. Doesn't actually
solve the problem — the JIT-side pointer in CPU registers is still
invisible.

**Move all allocation to the interpreter**: bounce every helper call
through a re-entry into interpreted code. Defeats the whole point of
JIT'ing past `CallNative`.

**Just don't collect when JIT is active**: the chosen path. Correct (no
use-after-free) but limited (heap can grow without bound during JIT'd
execution).

## What this means in practice

- Bench workloads (the 4 micro-benchmarks): heap stays well under the
  16 MB arena. Never observed an OOM or GC pressure issue.
- Real-world programs tested so far (csv_aggregate, game_of_life, sudoku,
  json_parse, markov, kvstore, brainfuck): all complete without OOM.
  The KV store ran 200+ commands with sustained allocation; heap usage
  stayed bounded (each command produces ~1 KB of garbage).
- **A long-running program** (web server, daemon, anything with hours of
  uptime) would eventually OOM because GC never runs while JIT'd code is
  active. This is the load-bearing limitation.

## When to revisit

When (a) someone writes a real long-running StrictPy program OR (b) someone
allocates aggressively in a JIT'd hot loop AND the heap pressure shows up.
Either signal is a clear "do precise stack maps now."

The replacement design is clear: Cranelift emits safepoint metadata at
every helper call site; the GC reads those maps; `in_jit` counter goes
away; collections proceed normally. The work is bounded; what's missing
is the time to do it carefully.

## Reference

- Code: `vm/src/gc.rs::Heap`, `vm/src/interp.rs::SharedVm.in_jit`
- Related: M9 brief in `agent_reports/`; `BUGS_KNOWN.md §4` documents the
  related heap-corruption bug that may be a consequence of this design.

## v0.3 update (M33) — superseded

**Status as of M33**: the `in_jit` pause is **gone**. Conservative root
scanning of interpreter frames remains, but the JIT now publishes a
per-thread *shadow stack* of register windows that the GC also scans;
the GC therefore runs even while JIT'd code is on the stack.

What changed concretely:

- `SharedVm.in_jit: AtomicUsize` removed. The `fetch_add` / `fetch_sub`
  brackets around `op_call_direct`'s JIT entry are gone.
- `Heap::collect`'s early-return on `in_jit > 0` is gone.
- New file `vm/src/stackmap_registry.rs`: thread-local
  `Vec<(buf, len)>` with `rt_shadow_push(buf, len)` /
  `rt_shadow_pop()` extern "C" helpers callable from JIT'd code.
- `vm/src/jit.rs`: every JIT'd function allocates one Cranelift
  explicit stack slot of `num_registers * 8` bytes (the shadow frame).
  Before each heap-allocating runtime helper call (`rt_alloc`,
  `rt_list_push`, `rt_list_new`, `rt_array_new`, `rt_virtual_call`,
  `CallDirect`, `CallNative` trampoline, `strictpy_alloc_str_const`)
  the JIT emits: spill every register variable to the shadow slot,
  call `rt_shadow_push(&shadow_slot, num_registers)`, run the call,
  then call `rt_shadow_pop()`.
- `Interpreter::maybe_collect` snapshots the per-thread shadow stack
  and feeds every published window into `Heap::collect`'s root scan.

### Why shadow stack instead of Cranelift `enable_safepoints`?

The cranelift-codegen 0.115 `UserStackMap` API is real and documented
(`declare_value_needs_stack_map`), but consuming the maps requires:

1. Reading `MachBufferFinalized::user_stack_maps()` and correlating each
   safepoint PC against the JIT'd code memory range.
2. Walking JIT'd Rust frames at collection time — either via the
   `backtrace` crate or a custom frame-pointer-chain walker — which is
   platform-specific (Windows x64 vs SysV ABIs differ in unwind info).
3. Maintaining a PC → stack-map registry keyed by raw return addresses
   that `cranelift-jit` doesn't currently surface as a stable
   `Range<usize>` on stable Rust.

Total estimate: 2-3k LOC of careful and platform-specific work.

The shadow stack ships the same correctness property — every heap
pointer reachable from a JIT'd frame at the moment of collection IS
rooted — for ~200 LOC of book-keeping, at the cost of:

- One spill-all-registers-to-stack-slot + one helper-call pair before
  each heap-allocating helper. Measured <5 ns per spill on x86_64; the
  helper-call overhead dwarfs the spill cost.
- Conservative false positives (we publish every register, not just
  those holding pointers) — but the GC's `alive` set already rejects
  integers that don't alias a live allocation, so this is the same
  trade-off the conservative interpreter scan already accepts.

The "real" Cranelift safepoint stack maps path remains in the v0.4
backlog. It would buy: precise (no-false-positive) register
enumeration, and the ability to add safepoints on long-loop back-edges
without the JIT having to emit a spill round trip per back-edge.

### Limitations still in scope

- **Long pure-compute JIT'd loops**: if a JIT'd loop has no allocation
  call, it also has no shadow-stack push, and other threads have to
  wait until the loop exits before GC can run. This is the same
  limitation Java had until ~2017 (back-edge polling safepoints).
- **Other threads' shadow stacks**: collection still drives from a
  single thread (whoever is currently calling `maybe_collect`); other
  threads' shadow stacks are not scanned. Combined with the heap mutex
  serialising allocations, the worst case is that a worker thread's
  pointer-in-register survives one extra collection cycle. Producer.spy
  has run cleanly with this for several milestones — the spec's
  concurrency model (no cross-thread mutation visibility without
  explicit channels) makes the case rarer than it might appear.
- **Moving / compacting collector**: still ruled out by the existing
  heap-layout invariants (vtable pointer at offset 0, `is_native`
  flag, `GcKind` on the type table). A v1.x rewrite.

### Reference

- Spec: `STRICTPY_SPEC.md` §15.7 ("Implementation status (v0.3)").
- Code: `vm/src/stackmap_registry.rs`,
  `vm/src/jit.rs::Translator::m33_safepoint_enter` /
  `m33_safepoint_leave`, `vm/src/interp.rs::maybe_collect`.
- Tests: `vm/tests/m33_precise_gc.rs`.
- Report: `docs/thesis/agent_reports/m33_precise_gc.md`.
