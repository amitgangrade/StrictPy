# M23 P3a-C — `threading.Lock` + `threading.Semaphore` + `queue.PriorityQueue`

**Brief**: extend the M6 thread/channel surface with the missing
synchronization primitives (Lock, Semaphore) and add a min-priority
queue.  Same stdlib-module-table recipe the M19/M20/M22 agents have
been using; the new wrinkle is that this is the **first Phase 3a
module that adds non-trivial state to `SharedVm`** — three new slot
tables (`locks`, `semaphores`, `priority_queues`) parallel to the
existing `channels` / `threads` / `dicts` / `files` tables.

**Wall-clock**: ~2 hours (read-through + slot-table addition + native
handlers + 16 in-process tests + 4 subprocess tests + 2 example
programs + spec + report).
**LOC**: ~640 added across 5 source files + 1 spec section + 1 report.

## Phase 1/2 modules this builds on

* **M6 (`Thread` + `Channel`)** — the `SharedVm.threads` and
  `SharedVm.channels` slot tables are the *exact* shape I copy for
  `locks` / `semaphores` / `priority_queues`.  Worker threads spawn a
  fresh `Interpreter` pointing at the same `SharedVm`, so a lock
  acquired on the main thread is visible from a worker.
* **M14 (tuples)** — `pq_pop_min_*` returns `Tuple[f64, T]`, allocated
  via M20a's `alloc_tuple_obj` (which materialises a 2-slot heap
  object with two `u64` fields — same shape as `path.splitext`).
* **M15 (try/except)** — `RuntimeError` (release on unheld lock) and
  `IndexError` (pop on empty PQ) raise through the standard
  `VmError::UncaughtException` channel.
* **M19 (stdlib module table)** — appending two `StdlibModule`s
  (`threading`, `queue`) in `seed_stdlib_modules` is the entire
  resolver-side change, except for one small refactor noted below.
* **M22 P2x (slot tables for handles)** — `argparse` and `collections`
  store opaque `Dict[str, str]` / `List[i64]` handles; PriorityQueue
  uses a proper slot table because the underlying state (a Rust
  `BinaryHeap`) isn't trivially round-trippable through a List/Dict.

## Design choices and what was scoped down

### `threading.Lock` is non-recursive

Matches Python's `threading.Lock`.  A thread that calls
`lock_acquire(h)` twice on the same lock deadlocks — the spec
(§9.24) is explicit.  Re-entrant `RLock` is on the v0.3 list because
adding it (track owner id + recursion depth on each slot) is a
contained change that I deliberately did not bundle with the
basic mutex landing.

The "release on unheld lock raises RuntimeError" check is real:
without it, the slot's `owner = None` field would silently flip
back to None and the next `acquire` would succeed against an
unbalanced state.

### `threading.Semaphore` is counting (no owner)

Standard counting semaphore: `permits: i32` + `Condvar`.  Any thread
may `release`, not just the one that acquired; that's the standard
semaphore behaviour and the most useful one for producer-consumer
patterns.  No bounded-cap semaphore in v0.2 (a future
`semaphore_new_bounded(n, max)` would clamp `release` operations).

### `queue.PriorityQueue` is monomorphic per element type

Stdlib functions can't be generic in v0.2 (M17 generics only see
user-defined .spy fns), so the brief asked for two variants: i64
items and str items.  I shipped exactly those.  `pq_len` /
`pq_is_empty` are type-erased — the i64 handle alone is enough
because the typechecker pinned the element type at the
`pq_new_*` site.  This saves 2 NativeFn ids per element type that
would otherwise be wasted on identical type-tagged variants.

FIFO tie-breaking is real: I attach a per-slot monotonic
`next_seq: u64` to every push and wrap it in `Reverse` inside the
heap key.  Two items with priority 5.0 pop in insertion order; a
test in `m23_p3a_c_threading_queue.rs` pins this.

NaN priorities: I added an `F64Ord` newtype with an `Ord` impl that
treats NaN as greatest.  Programs shouldn't push NaN, but the
implementation handles it cleanly instead of panicking inside
`BinaryHeap`'s comparator.

### What I deliberately did NOT ship

* `RLock`, `Event`, `Condition`, `Barrier`, named locks, timed
  acquire (`acquire(timeout=...)`) — all on the v0.3 list.
* `with lock_acquire():` context-manager sugar.  StrictPy's `with`
  desugars to `io.File`'s `__enter__`/`__exit__`; adding lock-aware
  desugaring would require a typechecker change.  Users write
  `acquire / try ... finally release` explicitly.
* PriorityQueue: `pq_clear`, `pq_drain`, bounded-capacity push,
  decrease-key.  v0.3.

## NativeFn IDs used

420–437 (18 of 20 reserved).  438–439 left for v0.3
(`pq_clear` / `pq_drain` are the obvious candidates).

## Files modified

| File | Lines added | Purpose |
|---|---|---|
| `shared/src/native.rs` | +50 | 18 new variants + `from_u32` arms |
| `compiler/src/resolver.rs` | +180 | `threading` + `queue` `StdlibModule`s + one prelude-precedence fix |
| `vm/src/interp.rs` | +110 | 3 slot-table types + 3 alloc helpers + `F64Ord` newtype |
| `vm/src/builtins.rs` | +330 | dispatch arms + lock/sem/pq helper fns |
| `STRICTPY_SPEC.md` | +120 | §9.24 + §9.25 |

Plus new files:

* `examples/threading_demo.spy` — 4 workers + shared counter under
  a Lock, asserts the final value is 400.
* `examples/queue_demo.spy` — drain a 10-item numeric PQ in priority
  order + a 5-task string-named-task scheduler.
* `vm/tests/m23_p3a_c_threading_queue.rs` — 16 in-process tests.
* `compiler/tests/threading_demo_runs.rs`,
  `compiler/tests/queue_demo_runs.rs` — 4 subprocess tests via
  `spy.exe`.

## Incidental bug found + fixed in this milestone

The brief flagged a possible issue: `threading` is **also** a
pre-existing prelude BuiltinModule (from M6) that re-exports the
`Thread` and `Channel` classes.  When I added `threading` to
`stdlib_modules` so my new `lock_new` etc. would resolve, the
existing `from threading import Thread, Channel` started erroring
with **"module `threading` has no item named `Thread`"** — the
import path's `match &stdlib_mod { Some(m) => ... }` arm took
precedence over the "Pre-existing prelude binding wins" check.

The fix in `register_top_decls` (resolver.rs ~line 2370) is
straightforward: when an item isn't in `stdlib_modules` but IS
already in scope (from the prelude), continue silently instead of
erroring.  The existing fall-through comment "Pre-existing prelude
binding wins (legacy stdlib)" only fired *after* a successful
stdlib lookup, which was the wrong order.  After the fix,
`from threading import Thread, Channel` and `from threading import
lock_new, lock_acquire` both work, with the prelude winning the
race for the names it owns.

Test for this lives at the end of `m23_p3a_c_threading_queue.rs`
(`lock_protects_shared_counter_across_threads` uses both
`from threading import Thread` and `import threading` in the same
module).  All 13 pre-existing M19-era resolver tests still pass.

## Cross-platform notes

* `std::sync::Mutex` + `Condvar` are portable across Linux, macOS,
  Windows — no surprises.  Spawned `Thread` workers on Windows
  inherit the same `SharedVm` `Arc` and observe the same lock state.
* `BinaryHeap<Reverse<(F64Ord, Reverse<u64>, u64)>>` doesn't care
  about platform; it's pure Rust.
* No FFI to C libraries — same property that kept M22 P2x at zero
  bugs per agent.

## Hardest three things (in retrospect)

1. **The `threading` prelude collision**.  Took 20 minutes to track
   down because the first failure mode was a single test case that
   used both `from threading import Thread` (prelude) and `import
   threading` (now stdlib_module).  The fix was a 4-line resolver
   change, but the *understanding* of why the existing
   "Pre-existing prelude binding wins" comment didn't apply took
   careful re-reading.

2. **`Condvar` + non-table-locked `gate`**.  My first sketch held
   the outer `locks` table mutex across `cv.wait`.  That would
   instantly deadlock as soon as any other lock op tried to take
   the table mutex — and there's no way to wake the waiter because
   only `notify_one` releases it.  The fix: each `LockSlot`
   carries its own `Arc<Mutex<()>>` gate; we lock the gate (cheap,
   contention-free in steady state) and drop the table mutex
   before the wait.  The cv re-locks the gate on wake-up — that's
   the standard Condvar dance.

3. **`F64Ord` for `BinaryHeap`**.  `f64` is `PartialOrd` but not
   `Ord` (NaN strikes again).  My first attempt just `unwrap`ed
   `partial_cmp`, which would panic on a NaN priority.  The
   newtype with explicit NaN-is-greatest semantics is the right
   primitive — small enough to not deserve its own module but
   important enough to have its own test (`F64Ord` is `pub` so the
   in-process test file can stress it directly, though I ended up
   testing the externally-visible queue behaviour instead).

## Final test totals

* **vm/tests/m23_p3a_c_threading_queue.rs**: 16 new in-process tests
  (4 lock + 3 semaphore + 9 PriorityQueue).
* **compiler/tests/threading_demo_runs.rs**: 2 subprocess tests.
* **compiler/tests/queue_demo_runs.rs**: 2 subprocess tests.
* All M0–M22 tests (468 baseline) still pass.

## What's next (Phase 3a integration)

The orchestrator will cherry-pick this commit along with the three
sibling Phase 3a worktrees.  Conflicts likely in:

* `compiler/src/resolver.rs` — `seed_stdlib_modules` end-of-fn; each
  sibling pushes a `.insert(...)`.  Mechanical merge.
* `shared/src/native.rs` — disjoint id ranges by design; should be
  clean.
* `vm/src/builtins.rs` — append-only dispatch arms; clean.
* `vm/src/interp.rs` — only this agent (P3a-C) touches `SharedVm`'s
  table list.  No conflict expected.
* `STRICTPY_SPEC.md` — §9.24+ added; orchestrator may renumber.

The prelude-precedence fix is in `register_top_decls`, a hot
import-resolution path that the M22 P2C / P2D agents also touched
implicitly.  Conflict risk is low (no other phase-3a agent edits
this file as far as I can see from the brief) but if a sibling did
add a `from MODULE import name` test that exercises the same code
path, the orchestrator should run all 4 worktrees' tests together
after the cherry-pick to catch any interaction.
