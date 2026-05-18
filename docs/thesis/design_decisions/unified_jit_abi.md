# Design decision: unified JIT ABI

**Milestone introduced**: M8
**Status**: in production
**Trade-off**: simplicity vs per-call overhead

## The choice

Every JIT-compiled function uses the same Rust signature:

```rust
unsafe extern "C" fn(vm: *mut VmState, args: *const u64) -> u64
```

- `vm` — opaque pointer to the VM's shared state (heap, type tables, etc.)
- `args` — pointer to the first argument slot in a contiguous `u64` array
- Return value — bit-pattern of the result, transmuted from `i64` directly
  or from `f64` via `f64::to_bits()`

Every function reads its arguments by loading from `args[i]` at entry, and
returns a `u64`. There is no per-function native signature.

## The alternative considered

Use Cranelift's `Signature` builder to emit per-function ABIs that pass
i32/i64 in GP registers and f32/f64 in XMM registers (System V on Linux,
Windows fastcall on Windows). This would match Rust's `extern "C"`
conventions exactly and avoid the load-per-argument overhead.

## Why the unified ABI won

1. **Interpreter↔JIT boundary becomes trivial.** The interpreter's
   `CallDirect` handler builds a `&[u64]` slice of arguments and calls
   `jit_fn(&mut vm_state, args.as_ptr())`. No marshaling. No per-function
   ABI generation. No knowledge of the callee's signature.

2. **JIT-to-JIT calls also use the same ABI.** A JIT'd function calling
   another JIT'd function doesn't need a different code path. Cranelift
   emits a normal `call` with the unified signature; the callee reads its
   args from the pointer.

3. **Per-function ABI would have required signature negotiation.** Cranelift
   needs the full `Signature` (param types + return type + calling
   convention) at the time it emits a `call`. With per-function ABIs, the
   JIT would need to either reconstruct the callee's signature from the IR
   on every call site, or maintain a parallel signature table. Both are
   substantial bookkeeping.

4. **The cost is small in absolute terms.** One extra load per argument is
   ~1ns on modern hardware. For a function with 4 args called 1M times,
   that's 4ms of overhead — invisible relative to the actual work.

## Where the cost shows up

For very small functions called in tight loops, the per-arg load IS visible.
The fib benchmark would have been marginally faster with native ABI. Rough
estimate: 5–15% in the tightest cases. The fact that fib(30) still beats
CPython by 11× means the overhead is well within the margin we already
have.

## When to revisit

If a future workload is dominated by tiny leaf functions called millions of
times, and the JIT becomes the bottleneck instead of the workload itself,
per-function ABIs become attractive. Until then, the simplicity wins.

## Reference

- Code: `vm/src/jit.rs::JitTranslator::translate_function`
- M8 agent brief that locked this in: see `agent_reports/m8_cranelift_jit.md`
