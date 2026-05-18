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
