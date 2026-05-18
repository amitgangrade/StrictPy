# Design decision: per-function JIT opt-in with fixpoint disable

**Milestone introduced**: M8
**Status**: in production
**Trade-off**: incremental coverage vs whole-program JIT

## The choice

A function is JIT-compiled IFF *every* IR op in its body is in the JIT's
supported set. If any op isn't supported, the function's slot in the
`fn_ptr_table: Vec<Option<JitFn>>` stays `None`. The interpreter handles
the call normally.

A subsequent fixpoint pass disables JIT for any function whose `DirectCall`
targets aren't themselves JIT'd. This ensures cross-function calls always
resolve cleanly: a JIT'd function never tries to direct-call a function
that doesn't have a native entry point.

```
Pass 1: compile every function whose ops are supported → JIT or None
Pass 2: walk all JIT'd functions. For each, find every DirectCall target.
        If any target isn't JIT'd, mark the caller as None too.
        Repeat until fixpoint.
```

## The alternative considered

**All-or-nothing JIT**: compile the whole module or none of it. If any
function uses an unsupported op, the entire module stays interpreted.

We didn't do this because:

1. **Incremental coverage is implementable in chunks.** M8 covered
   arithmetic + branches + calls + list-reads. M9 added heap mutation +
   field access + virtual calls + alloc. With all-or-nothing, M8 would
   have JIT'd literally nothing useful — the 7 examples all use list
   ops somewhere. Per-function let M8 deliver Fibonacci and Mandelbrot
   wins immediately.

2. **The fixpoint disable is the natural cross-call story.** A JIT'd
   function calling an interpreted callee would need a "deopt back to
   interpreter for one call" path — substantial extra code with edge
   cases around return-value packing. The fixpoint disable says: "if
   the callee isn't JIT'd, you're not JIT'd either," which is simpler
   and only loses incremental wins.

3. **Coverage extension is cheap.** Adding one new op (LoadField,
   ArraySet, ...) immediately unblocks every caller that uses it. M9 added
   ~6 ops and flipped 4 benchmarks from CPython wins to StrictPy wins.

## The cost

A function with one unsupported op in a rare branch loses JIT entirely.
Example: a function that's pure numeric except for an unreachable
`raise ValueError(...)` would stay interpreted because `Throw` isn't
JIT-supported.

This bit one program: tree.spy's class methods. Each method touches
`LoadField` or `StoreField`. Until M9 added those, `tree.spy` had
**1/6 functions JIT'd** — only `main`. Most of the work happened in
interpreted methods.

M9's fix was to add LoadField/StoreField support. Coverage went to 6/6.

## When to revisit

If the project grows toward many "almost-JIT'd" functions, a partial-JIT
approach (JIT the basic blocks that ARE supported; trampoline to
interpreter for the unsupported ones) becomes attractive. The cost is
significantly more complex code-generation logic and a `BlockJump` ABI for
moving execution state between JIT and interpreter on each transition.

Signal to revisit: when adding a single supported op moves <2 benchmarks.
Until then, the bulk-coverage approach dominates.

## Reference

- Code: `vm/src/jit.rs::JitCell::compile_module` (the fixpoint pass)
- Per-example coverage telemetry: `vm/tests/jit_coverage.rs`
- M9 brief and report in `agent_reports/`.
