# M10-AB — Nullable-narrowing audit + stdlib gaps

**Brief**: (1) audit codegen for nullable-narrowing miscompiles like BUG-001;
(2) add `for x in xs:`, `str.split(sep)`, `sorted()`/`list.sort()`.

**Wall-clock**: ~70 minutes
**Tool uses**: 205

## Task 1 — nullable audit (the high-leverage half)

**Found 4 more silent miscompile sites** with the same pattern as BUG-001.
All in `compiler/src/codegen.rs`:

| Site | Bug ID | Symptom |
|---|---|---|
| `emit_op` operand_ty/ty | BUG-002 | Generic dispatch silently picked wrong width for `Nullable(_)` slots |
| F32/F64 dispatch | BUG-003 | Would emit F64 op for `Nullable(F32)` operand — wrong width |
| `int_cmp_op` | BUG-004 | Comparison emitted wrong-width compare for nullable int slots |
| `op_for_iadd/isub/imul/idiv` | BUG-005 | Typed add family silently emitted I64 op for `Nullable(I32)` |
| `ty_tag_for` | (defensive fix) | Field-store dispatch could leak nullable types into ty_tag tables |

The recurring pattern:

```rust
// Before (buggy):
if let Ty::Primitive(p) = operand_ty {
    if p.is_float() { ... } else { ... }
}

// After:
let ty = unwrap_nullable_ty(operand_ty);
if let Ty::Primitive(p) = ty {
    if p.is_float() { ... } else { ... }
}
```

5 new regression tests pin each fix. The bug surface in codegen turned out
to be **broader than expected** — 5 distinct dispatch sites, not just
`emit_binop` (which was the original CSV-aggregator-triggered bug).

## Task 2 — `for x in xs:` desugaring

`Stmt::For` was a placeholder in M3. Now desugars (for `List[T]` receivers)
to:

```python
__i: i64 = 0
__n: i64 = i64(len(xs))
while __i < __n:
    x: T = xs[__i]
    <body>
    __i = __i + 1
```

`examples/quicksort.spy`'s main output loop was updated to demonstrate.
Other receivers (e.g. range, dict iteration) fall back to a placeholder
with TODO for `__iter__`/`__next__` protocol.

## Task 3 — `str.split(sep) -> List[str]`

Three prior programs (wordcount, csv_aggregate, markov) had hand-written
state-machine splitters. Now it's `NativeFn::StrSplit = 28`.

## Task 4 — `sorted()` / `list.sort()`

`NativeFn::ListSort = 105` (in-place), `ListSorted = 106` (returns new
list). For v1: `List[i64]`, `List[f64]`, `List[str]`. NaN-tolerant float
ordering. Generic comparators are future work.

## Files modified (line counts)

| File | Δ lines |
|---|---|
| `compiler/src/codegen.rs` | +85 (5 dispatch fixes + 5 regression tests) |
| `compiler/src/ir.rs` | +190 (for-loop lowering + sort helpers + nullable unwrap) |
| `compiler/src/typecheck.rs` | +30 (sorted/str.split/list.sort cases) |
| `compiler/src/resolver.rs` | +8 |
| `shared/src/native.rs` | +20 (StrSplit + ListSort + ListSorted) |
| `vm/src/builtins.rs` | +170 |
| New: `vm/tests/_for_loop_smoke.rs` | 40 |
| New: `vm/tests/_split_and_sort_smoke.rs` | 70 |

## Test totals

138 → 158 (+20: 5 nullable-dispatch regressions in codegen tests, 9 split/sort
tests in vm builtins, 1 for-loop IR test, 1 for-loop smoke, 2 split/sort
smokes — plus some other counts shifted slightly due to parallel-agent
churn).

## Unexpected findings

1. **The audit's bug surface was 5×, not 1×.** Expected to find maybe one
   sibling to BUG-001. Found four. The same dispatch pattern was scattered
   across `codegen.rs`. When you find one nullable-narrowing miscompile,
   audit aggressively.

2. **Concurrent cargo from parallel agents held `run_examples-*.exe`
   open.** Windows linker file locks. Worked around by serial test runs.
   Coordination cost of parallel-agent runs.
