# P4 — allocation-light nullable (`T?`): findings

**Outcome: deferred / no code change. The niche-tagged primitive optional
representation the task proposes already exists.** This report documents the
investigation, why no further change is warranted, and the GC-safety reasoning
that makes the existing scheme sound.

## Task premise vs. reality

The P4 brief assumed primitive optionals (`i64?`, `f64?`, `bool?`, …) are
heap-boxed and that the win is to replace the box with a niche/tagged value so
`is none` / `??` / optional-return become register ops with no allocation.

Investigation shows that **is already the implementation**. `T?` for a primitive
`T` is represented as the raw value of `T`, with a single reserved bit pattern,
`NONE_SENTINEL = 0x8000_0000_0000_0000`, standing in for `none`. There is no
heap box anywhere on the path.

Evidence:

- `none` literal → `Opcode::ConstNone` → `op_const_none` writes the register
  `NONE_SENTINEL` (vm/src/interp.rs). In the JIT it is `Op::ConstNoneSentinel`,
  an `iconst` of the same value (vm/src/jit.rs ~line 1019). No allocation.
- `x is none` / `x is not none` → `IROp::RefEq` against the `ConstNone` value
  (compiler/src/ir.rs, `AstBinOp::Is` / `IsNot`). A register integer compare.
- `x ?? y` → `if x is none: y else: x`, lowered as a `RefEq` test + branch with
  the result threaded through a slot (compiler/src/ir.rs, `Expr::NullCoalesce`).
  No allocation; `y` is short-circuit-evaluated only when `x` is none.
- A function returning `i64?` returns the raw i64 (or the sentinel) in a
  register. There is no boxing op in the IR lowering for nullable returns; grep
  for boxing in compiler/src/ir.rs finds only the unrelated generic `Box[T]`
  test fixture.
- The native builtins already rely on this contract: dozens of nullable-returning
  natives return `NONE_SENTINEL` for absence (vm/src/builtins.rs — DictGet,
  ChannelTryRecv, fetchone, the M37/M38 tabular column accessors, etc.). The
  M34/M37 agent reports call out the "return the sentinel, not zero" rule.

So primitive `T?` is a niche-tagged register value today. There is no box to
remove and therefore no allocation-removal win available for ds_nullable.

## Where the ds_nullable 1.69× actually comes from

The benchmark (comprehensive_bench/v2/programs/ds_nullable.spy) is a 1M-iteration
loop making **two** direct function calls per iteration (`lookup(i)` and
`lookup(i+1)`), i.e. ~2M calls total, each returning `i64?`.

JIT is a default feature and compiles the whole program AOT-at-load, so both
`run` and `lookup` are native; `lookup` is reached via `Op::CallDirect`, which
lowers to a direct native call. The optionals stay in i64 registers throughout.

The residual cost vs. CPython is the **per-call ABI overhead in the JIT**, not
nullable representation: each `CallDirect` materialises an `alloca` argument
buffer, stores each argument into it, and brackets the call with
`m33_safepoint_enter` / `m33_safepoint_leave` (shadow-stack push/pop for precise
GC). Multiplied by ~2M calls, that bookkeeping dominates. Closing it is a
**call/inlining/safepoint** workstream, not a value-representation one — and it
is out of P4's scope and risk envelope.

## GC-safety of the existing niche (why it is sound, and why changing it is risky)

The collector (vm/src/gc.rs) is **conservative**: every u64 in a root window or
a scanned object slot is treated as a potential pointer and kept alive only if
its bit pattern exactly matches an entry in the `alive` map of real allocations.

The niche is safe in both directions:

1. **A tagged primitive optional is never mistaken for a pointer.**
   `NONE_SENTINEL = 0x8000_0000_0000_0000` has the top bit set. On x86-64 that
   is a non-canonical / kernel-half address; the system allocator only ever
   returns low-half, 8-aligned user addresses. The sentinel therefore can never
   be a key in `alive`, so `maybe_push` never traces it and sweep never treats
   it as a live object. (A non-none `i64?` payload like `5` is likewise just an
   integer; if it ever happened to alias a live address it would only
   conservatively *retain* that object — never a memory-safety bug.)

2. **A real pointer optional is still traced.** A `str?` / `List[T]?` that holds
   `Some(ptr)` carries the actual heap address in its bits, so it *is* in
   `alive` and is marked/traced exactly like a non-optional reference. `none`
   for a reference optional is the same sentinel, which is correctly skipped.

This is precisely why a different niche would be dangerous. Any scheme that
moved the none-marker (or packed a tag into low/canonical bits of the payload)
could (a) produce a none-marker that aliases a real allocation — silently
keeping garbage alive, or worse — or (b) make a real pointer optional
indistinguishable from a tagged primitive, so the collector fails to trace a
live object and frees it underneath a live reference. The current high-bit
sentinel sidesteps both because it lives in an address range the allocator can
never hand out. Given the conservative collector, the existing representation is
the *safe* niche; re-tagging would be the risky change. We therefore do not
touch it.

## ds_generics confirmation

ds_generics is at 1.04× (results_v2_main_postfix.json) — already ~parity, as the
brief notes (P1 + prior work closed it). Generic classes (`Box[T]`, etc.) do
allocate a heap object per instantiation by design (that is their semantics, and
the generic-class tests in vm/tests/m31_generic_classes.rs enforce round-trip
identity). There is no cheap boxing win here that preserves semantics, so per
the brief ("only invest here if you find a cheap boxing win, otherwise leave
it") it is left unchanged.

## Verification

- `cargo build --release` — clean.
- `cargo test -p strictpy-vm --test m21_null_coalesce --test real_world_fixes`
  — all 14 pass (covers `??` chaining / short-circuit, `is none`,
  `is not none`). Nullable semantics intact (no code changed).
- Note: `m33_precise_gc::recursive_allocation_does_not_leak_or_crash` overflows
  the stack in the **debug** test build on Windows (deep-recursion test, small
  default thread stack). It is pre-existing and unrelated to nullable — this
  branch makes **zero** code changes (`git diff` is empty aside from this doc).
- The benchmark harness (comprehensive_bench/v2/run_v2.py) executes the spy
  binary directly; running it was not possible in this sandbox. The cited
  baseline ds_nullable 1.69× / ds_generics 1.04× are from
  results_v2_main_postfix.json. No representation change was made, so these
  numbers are unchanged by this branch.

## Recommendation

Close P4 as "already implemented; deferred". The genuine ds_nullable headroom is
in the JIT call/safepoint path (inline small leaf callees like `lookup`, or
elide the shadow-stack push/pop for calls to JIT'd callees with no live heap
roots), which should be tracked as a separate call-overhead workstream rather
than risked as a value-representation change.
