# Design decision: unwrap nullable before type-specific dispatch

**Milestone introduced**: M10 (forced by BUG-001 through BUG-005)
**Status**: in production, with hard-earned scar tissue
**Trade-off**: explicit unwrap discipline vs invariant in the type system

## The pattern

Every IR-side dispatch that switches on `Ty::Primitive(_)` must first
unwrap `Ty::Nullable(inner)`:

```rust
fn unwrap_nullable_ty(ty: &Ty) -> &Ty {
    match ty {
        Ty::Nullable(inner) => inner,
        _ => ty,
    }
}

// Every site that looked like:
//   if let Ty::Primitive(p) = operand_ty { ... }
// became:
//   let ty = unwrap_nullable_ty(operand_ty);
//   if let Ty::Primitive(p) = ty { ... }
```

## Why this is needed

The type checker correctly narrows `T?` to `T` inside branches like:

```python
prev: f64? = agg.get(k)
if prev is none: ...
else: agg[k] = prev + amount   # `prev` here has type f64
```

But the narrowed type lives in a side table on `expr_types`, NOT on the IR
slot. The IR slot for `prev` still says `Ty::Nullable(f64)` — that's its
declared type. Any IR-side check on the slot's type therefore sees a
`Nullable`, not a primitive.

The naive check `if let Ty::Primitive(_) = slot_ty` returns false for
`Nullable(F64)`. Whatever default branch handles the "not a primitive" case
silently runs — for binop dispatch, that meant falling through to integer
add, emitting `IAddI64` over raw f64 bit patterns.

## The alternative considered

Make the type checker rewrite the IR slot's type to the narrowed type
inside the narrowed scope. This is more "correct" in the type-theoretic
sense — `prev`'s type IS `f64` in the else branch.

We didn't do this because:

1. Mutable per-scope IR slot types complicate SSA reasoning.
2. The unwrap is a tiny, mechanical pattern; centralizing it in a single
   helper is simpler than threading narrowed types through the IR.
3. The unwrap is also correct for non-narrowed `Nullable(T)` slots where
   the runtime value is non-null — those should dispatch on T anyway,
   because a primitive op on a null operand should trap regardless.

## The cost of NOT doing this

Five silent miscompile bugs across `codegen.rs`. All shipped to "passing
tests" because no test pattern matched `narrowed-nullable + type-specific
op`. CSV aggregator's float aggregation was the first program to trigger
the case in user code.

The audit pattern that found the other four bugs:

```
grep -r "Ty::Primitive" compiler/src/codegen.rs
# for each hit, check: can a Nullable(T) slot reach here?
# if yes: bug. fix by unwrapping.
```

The four bugs ranged from "obvious miscompile" (F32/F64 dispatch picking
F64 for `Nullable(F32)` operands) to "subtle: only triggers on field stores
with nullable type tags."

## Discipline going forward

Any new IR dispatch site that switches on type:

1. **Must** use `unwrap_nullable_ty` (or `unwrap_nullable_inner` from
   `types.rs`) on the operand_ty before matching.
2. Should have a regression test like
   `iadd_dispatches_i64_even_when_slot_is_nullable`.
3. Reviewer pattern: search for `Ty::Primitive` in the diff and verify
   each site has the unwrap.

## Reference

- Code: `compiler/src/codegen.rs` (5 dispatch sites), `compiler/src/ir.rs::emit_binop`
- Bug entries: BUG-001 through BUG-005 in `bugs/catalog.md`
- Regression tests: `compiler/src/codegen.rs::tests` (5 nullable-dispatch tests)
