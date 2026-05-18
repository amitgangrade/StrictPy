# M15 — try / except / finally + raise (BUG-025 close)

**Brief**: M10's KV-store agent surfaced BUG-025 — `open(missing, "r")`
aborted the program because no user code could catch the runtime
`IOError`. The proximate cause was that `Stmt::Try` and `Stmt::Raise`
IR lowerings were stubs (just walked the bodies) and the
`Throw/EnterTry/LeaveTry/Rethrow` opcodes returned
`Trap("not implemented (M5)")`. M15 wires the full pipeline end-to-end
and closes BUG-025.

**Wall-clock**: ~2 hours (read-through + design + implementation + tests
+ docs).
**Files changed**: 9 source files + 4 new tests/example/report.
**Tests**: 222 baseline + 10 new M15 + 2 new BUG-025 acceptance = **234 passing, 0 failing**.

## Representation chosen: lazy materialisation on bind

The brief offered two options for the exception value: eagerly allocate
a heap object every time something throws, or stay in the Rust-side
`VmError::UncaughtException` payload until a handler with `as e:` binds
it. I went with **lazy materialisation** because:

1. Most exceptions either escape (no handler matches → the existing
   "error to top of run_until → exit non-zero" path runs and the heap
   alloc would have been wasted) or are caught by an arm without an
   `as e:` binding (the body just wants to recover, not introspect).
   Only when `arm.bind_reg != DISCARD_REG` does
   `Interpreter::materialise_exception` allocate the 2-field heap object
   (16-byte header + 16-byte payload: `type_name: str` at offset 0,
   `message: str` at offset 8).
2. The native error sites in `vm/src/builtins.rs` and `vm/src/interp.rs`
   that already return `VmError::UncaughtException { type_name, message }`
   needed zero changes — `propagate_exception` does its match against
   the unboxed string, which the existing payload already carries.
3. User-side `raise IOError("msg")` lowers to a real heap allocation
   (`IROp::Alloc` + two `IROp::Store`s + `Terminator::Throw`), so the
   already-allocated object is what `Opcode::Throw` reads back to
   produce the `VmError::UncaughtException`. The handler's bind-reg
   materialise step then allocates a **second** copy. Acceptable cost
   for v0.1 — the explicit-`raise` path is the rare one, and unifying
   the two would require either (a) keeping the original alloc through
   `Throw` (forces register liveness across what was previously a
   terminator), or (b) tagging the materialise step to skip on
   round-trip (state plumbing). Not worth it yet.

## Bytecode opcodes added (or rather, activated)

The opcodes `Throw`, `EnterTry`, `LeaveTry`, `Rethrow` were reserved in
`shared/src/opcode.rs` since the project started but had been trapping
with `"not implemented (M5)"`. M15 implements all four.

**EnterTry encoding** (`compiler/src/codegen.rs::IROp::TryEnter` arm):

```
[u8 opcode]
[i32 finally_pc]                   // -1 sentinel if no finally
[u8 n_arms]
for each arm:
    [u32 filter_str_idx]           // constant-pool index of type-name; "Exception" = catch-all
    [i32 handler_pc]               // patched at finish()
    [u16 bind_reg]                 // u16::MAX (DISCARD_REG) if no `as e:`
```

`handler_pc` and `finally_pc` are registered with the existing
`Codegen.patches` table, so the same branch-offset resolution pass that
`Terminator::Branch` uses handles them automatically. **Zero new
codegen infrastructure** — reuses the M3 patch machinery in full.

**Throw** reads its operand register as a heap pointer; reconstructs
the `(type_name, message)` strings from offsets 0/8 past the header;
returns `VmError::UncaughtException` so the `run_until` catch loop
can deliver. **LeaveTry** pops the topmost handler frame.
**Rethrow** (used as END_FINALLY) re-raises the value in
`Interpreter.pending_exception` if `Some`, else no-ops.

## JIT carve-out (already in place)

`vm/src/decompile.rs::decode_function` (line 485) has a catch-all
`_ => return Err(DecodeError::Unsupported(opcode_name(oc)))` arm. The
unique-opcode-name helper at line 510 already groups our four opcodes:
`Opcode::Throw => "Throw"`, `Opcode::EnterTry | Opcode::LeaveTry |
Opcode::Rethrow => "TryCatch"`. So any function whose bytecode body
contains those opcodes returns `Err`, and `Jit::compile_module` (per
`docs/thesis/design_decisions/per_function_jit_opt_in.md`) silently
falls back to the bytecode interpreter for that function. This was
dormant before M15 — no codegen emitted those opcodes. I confirmed the
fallback by running `vm/tests/m15_try_except.rs` and
`vm/tests/m11_fixes.rs::m11_examples_compile_cleanly` with `--features
jit` both green.

## BUG-025 closing demo

`examples/safe_open.spy` main flow (~80 lines):

```python
fn try_read(path: str) -> str:
    try:
        f: io.File = open(path, "r")
        contents: str = f.read()
        f.close()
        return contents
    except IOError as e:
        println("safe_open: could not open " + path + " — " + e.message)
        return ""

fn main() -> i32:
    ensure_written("safe_open_a.txt", "alpha-contents\n")
    ensure_written("safe_open_c.txt", "gamma-contents\n")

    a: str = try_read("safe_open_a.txt")
    if len(a) > 0: println("opened a OK: " + a.slice(0i64, 5i64))

    b: str = try_read("safe_open_b_definitely_missing_m15.txt")
    if len(b) == 0: println("recovered from missing b")

    c: str = try_read("safe_open_c.txt")
    if len(c) > 0: println("opened c OK: " + c.slice(0i64, 5i64))

    # finally + propagation demo
    cleaned: i32 = 0i32
    try:
        try:
            raise IOError("simulated downstream failure")
        finally:
            cleaned = cleaned + 1i32
    except IOError as e:
        cleaned = cleaned + 1i32
        println("propagated past finally: " + e.message)
    println("cleaned=" + str(cleaned))
    return 0
```

Actual output (captured by `compiler/tests/safe_open_runs.rs`):

```
opened a OK: alpha
safe_open: could not open safe_open_b_definitely_missing_m15.txt — <OS diagnostic>
recovered from missing b
opened c OK: gamma
propagated past finally: simulated downstream failure
cleaned=2
```

Pre-M15: the same source aborts the program at the second `try_read`
with the same `IOError` message printed to stderr by `vm/src/main.rs`
and a non-zero exit code. The remaining four lines never print.

## New bugs found during the work

- **Resolver's `is_open=true` on exception classes** still applies post-M15.
  This means `IOError`-typed receivers go through the vtable. The
  vtables are empty (no methods), so this is harmless in practice, but
  it does mean the `IsInstance` opcode against an `IOError` always
  returns 1 (the M5 conservative stub) — handler dispatch correctly
  matches by type-name string, not by `IsInstance`, so user code is
  unaffected. Logged for the next thesis catalog update; not a
  user-visible bug.
- **`with open(path, "r") as f: io.File:` and try/except can't compose
  cleanly without restructuring.** The example originally tried to put
  the open inside a `with` and the read inside a separate `try`. The
  `with` block's auto-close (`io.File.close()`) runs unconditionally
  via the IR's `Stmt::With` lowering — which is *another* path that
  doesn't go through handler frames. So a `try: with open(...) as f`
  would not see `IOError` from the `open` call. Worked around in
  `safe_open.spy` by putting the open inside the try directly. Logged
  as a follow-up: spec §7.5.4 should call this out; the long-term fix
  is to lower `with` to a try/finally pair.
- **The interpreter's `op_load_field` does a NULL check first.** The
  exception heap object I materialise has a null `RuntimeType*` (we
  don't have a wired `Exception` type in the type bundle, only in the
  compiler's type table). Field loads on `e.message` therefore go
  through `LoadField`'s offset+tag path which doesn't deref the type
  pointer — works correctly. But if a future pass starts using the
  vtable pointer (e.g. `e.toString()` virtual dispatch on Exception),
  this will trap. Documented in spec §7.5.2.

## Final test totals

```
$ cargo test --workspace --release
   ...
   234 passed; 0 failed; 1 ignored
```

The 1 ignored test is the pre-existing `probe_str_ne_bug_repro` from
M12 (unrelated). 222 → 234 = +12: 10 in `vm/tests/m15_try_except.rs`
and 2 in `compiler/tests/safe_open_runs.rs`. Every other test binary
unchanged.

## Three hardest things

1. **Block-id patching for the new EnterTry operand stream.** EnterTry
   is the first instruction in the project whose immediate operands
   include MULTIPLE block-relative i32 offsets within a single opcode
   payload. The existing `Codegen.patches` table is keyed by `(byte
   position, target block id)`, and `finish()` resolves each entry to
   `target_offset - pc_after_operand`. Initially I tried to do per-arm
   patches as separate vec entries — that worked. But I had to
   convince myself that the `pc_after` baseline is computed correctly:
   the offset is relative to the byte AFTER the i32 slot it sits in,
   not after the whole EnterTry instruction. The existing
   `Terminator::CondBranch` arm already handles this (it does two
   patches in one terminator). Once I followed that pattern, it just
   worked.

2. **Handler-frame depth bookkeeping during call unwinding.** When an
   exception fires inside a nested function call, we need to (a) find
   the handler frame whose `frame_depth` matches a caller's depth, (b)
   pop *every* call frame above that depth, (c) pop *every* handler
   frame whose depth is above-or-equal to the matched one. Initially I
   only popped the matched handler frame, which left dangling handler
   entries from any try-inside-the-callee that hadn't been LeaveTry'd
   (because the callee never got the chance — its frame was unwound).
   Fixed by the defensive `while let Some(top) = self.handler_frames.last()
   { if top.frame_depth >= frame_depth { ... pop ... } else { break; } }`
   loop in `propagate_exception`. The "wait for the matching depth, then
   stop after popping that one" extra-step prevents popping frames from
   the *target* call frame's own outer try (which is still active).
   The borrow checker also wanted me to snapshot `(frame_depth,
   matched, finally_pc)` before mutating — done via a `match` block
   that ends the borrow before any `self.handler_frames.pop()`.

3. **The exception-class `__init__` typecheck barrier.** Built-in
   exception classes have no `__init__` method (and they shouldn't —
   they're not user-extensible in v0.1). But `raise IOError("msg")` is
   parsed as `Stmt::Raise { exc: Expr::Call { callee: Ident("IOError"),
   args: [...] } }`. The typechecker's `synth_call` path for
   `Ty::Class(cid)` requires a matching `__init__` arity, or errors with
   "class `X` has no __init__" when args.len() != 0. The cleanest fix
   would have been to add a minimal `__init__(message: str)` to each
   exception class layout, but that would force `lower_raise` to either
   dispatch through `DirectCall` (which would need an emitted function
   body — none exists for built-ins) OR special-case the alloc + skip
   the call. I went with **intercepting the type-check at `Stmt::Raise`**:
   when the exc expression matches the `Call(Ident(ExcName), [msg])`
   shape with a recognised built-in name, validate `msg: str` directly
   and stash the callee's type as `Ty::Class(cid)` so IR's `lower_raise`
   can find the class id via `expr_types` and emit `Alloc`. This keeps
   the constructor-call sugar in the source while avoiding any need to
   synthesise a real `__init__` function. The same pattern can be reused
   when user-defined exception subclasses land — we'd just extend
   `is_builtin_exception_name` to also accept names that resolve to a
   subclass of `Exception`.
