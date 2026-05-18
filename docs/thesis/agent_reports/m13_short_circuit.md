# M13 — `and` / `or` short-circuit (BUG-035)

**Brief**: lower `a and b` to `if a: b else: false` and `a or b` to
`if a: a else: b` at the IR level. First mid-expression CFG
manipulation in the project — every prior bool op was single-block.

**Wall-clock**: ~25 minutes
**Files changed**:
- `compiler/src/ir.rs` (+86 / −5)
- `vm/tests/m13_short_circuit.rs` (new, 6 tests)
- `BUGS_KNOWN.md` (close §7, add "Fixed in M13" section)
- `docs/thesis/bugs/catalog.md` (BUG-035 status, summary table totals)

## The transformation

Before: `Expr::Binary { op, lhs, rhs, span }` eagerly lowered both
operands then dispatched on `op` in `emit_binop`. For `And`/`Or` the
dispatch picked `IROp::IAnd` / `IROp::IOr` (bitwise), so the rhs was
always evaluated even when the lhs already decided the result.
Comment in `emit_binop` was honest: `// bitwise approximation`.

After: `lower_expr`'s `Expr::Binary` arm inspects `op` BEFORE lowering
operands. For `And` | `Or` it dispatches to a new helper:

```rust
fn lower_short_circuit(
    fb: &mut FuncBuilder,
    ctx: &mut LowerCtx,
    op: AstBinOp,
    lhs: &Expr,
    rhs: &Expr,
    ty: Ty,
) -> ValueId {
    let l = lower_expr(fb, ctx, lhs);                  // lhs in current block
    let slot_name = format!("__sc_{}", fb.slot_ty.len());
    let slot = fb.alloc_slot(&slot_name, ty.clone());
    fb.emit_write_local(slot, l);                      // pre-seed result with l
    let rhs_b = fb.new_block();
    let merge = fb.new_block();
    let (t_target, f_target) = match op {
        AstBinOp::And => (rhs_b, merge),
        AstBinOp::Or  => (merge, rhs_b),
        _ => unreachable!(),
    };
    fb.terminate(Terminator::CondBranch { cond: l, t: t_target, f: f_target });
    fb.switch_to(rhs_b);
    let r = lower_expr(fb, ctx, rhs);                  // rhs in its own block
    fb.emit_write_local(slot, r);
    fb.terminate(Terminator::Branch { target: merge });
    fb.switch_to(merge);
    fb.emit_read_local(slot)                           // phi via slot
}
```

The bitwise arm in `emit_binop` is preserved as a defensive backstop;
no user-visible caller exercises it any longer.

## Phi-merge pattern

I copied the **slot-based pattern** from the M3.5 loop-carried-locals
fix (`ReadLocal { slot }` / `WriteLocal { slot }` with stable per-local
slot indices, in `FuncBuilder::emit_read_local` / `emit_write_local`).
The pattern works transparently across basic-block boundaries — that's
the very property that lets `while` loops carry locals across the
back-edge. No new IR ops or VM opcodes were needed.

The alternative would have been an explicit `Phi` instruction in
`IROp`, but (a) the IR has no phi today, (b) the slot pattern is what
the rest of the codebase uses for cross-block values, and (c) consistency
with `Stmt::If`/`Stmt::While` matters more than minor IR purity.

Slot names are uniquified by current `slot_ty.len()` (`__sc_{N}`) so
nested short-circuit expressions (`a and b and c`, `a or (b and c)`)
don't alias each other.

## Test results

```
cargo test --workspace --release
```

All 45 test binaries green, **212 passed; 0 failed; 1 ignored** (the
existing M12 `probe_str_ne_bug_repro` is the lone ignored test;
unrelated to BUG-035). Pre-M13 baseline was 206; +6 from the new
`m13_short_circuit.rs` test file. New tests cover:
- guard idiom (`b > 0 and xs[b-1] > 0` with `b == 0`) — must NOT trap
- mirror for `or` (`b == 0 or xs[b-1] > 0`)
- `true and true` → `true`
- `false and (1/0 == 0)` — rhs must NOT execute; exit 0; `r == false`
- `true or (1/0 == 0)` — rhs must NOT execute; exit 0; `r == true`
- chained `a and b and c` for all 8 truth-table assignments

Cargo build (`--workspace --release`): clean, no new warnings.

## Gotchas — load-bearing notes for the try/except agent

This pattern is the template the try/except lowering will need to
reuse. Three things tripped me up that the next agent should know:

1. **Don't put the operand-lowering inside `emit_binop`.** `emit_binop`
   takes `l` and `r` already-lowered `ValueId`s — by the time you're
   in it, the rhs has already been evaluated in the wrong block.
   The dispatch has to be at the AST-node level (in `Expr::Binary`'s
   arm of `lower_expr`), BEFORE either operand is touched. Same will
   be true for `try: <expr> except: <handler>` if it's ever expression
   form: handler must be lowered in a fresh block, not in the same
   block as the try-body.

2. **Pre-seed the result slot BEFORE the CondBranch.** This is the
   "phi predecessor" for the short-circuit path. If you only write the
   slot in the rhs block, the merge block reads an uninitialised slot
   when control comes via the short-circuit edge. (The slot has a
   default value but it's not what you want.)

3. **`fb.terminate` is idempotent.** It only sets the terminator if
   the current block's terminator is still `Unreachable`. So calling
   `terminate` then `switch_to` is safe; calling `terminate` after
   switching to a block that's already terminated is a no-op. Not a
   pitfall in this fix (every block I created had a unique unset
   terminator), but worth knowing.

The fix is ~50 lines of net new code plus the helper's doc comment.
The bitwise-approximation arm in `emit_binop` is left in place as a
defensive backstop — removing it is a cleanup for a future agent.

## Confirming the original repro

The B-tree's insertion-sort condition
`b > 0 and ranks[b-1] > ranks[b]` (the program that originally
surfaced this bug in M12) is now expressible with the natural `and`;
the nested-`if` workaround in `examples/btree.spy:404` is no longer
necessary but is left in place — it's not actively wrong and the
M12 agent's report explicitly calls it out as a workaround. A future
stress-test refactor pass can unwind the workarounds.
