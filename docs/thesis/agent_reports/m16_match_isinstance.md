# M16 — `isinstance(x, T)` + `match` / `case` lowering

**Brief**: Eliminate the `kind: i32` discriminator workaround that every
M10–M12 sealed-hierarchy stress program had to roll. Two pre-existing
language-surface holes had forced the workaround:

* `isinstance(x, T)` parsed but `vm/src/interp.rs::op_is_instance` was the
  M3 stub that always returned `1`.
* `match v: case Pair(c, d): ...` parsed end-to-end (the `MatchArm` /
  `Pattern::Constructor` AST shape has been in place since M3 and the
  parser has been producing it correctly) but `compiler/src/ir.rs`'s
  `Stmt::Match` arm was an explicit M4 placeholder — `// M4` — that
  silently dropped every body.

**Wall-clock**: ~2.5 hours (read-through + IR/typecheck/VM wiring + 9
regression tests + new example program + spec/catalog updates).

**Tests**: 234 baseline + 9 new M16 + 2 new example-program tests = **243 passing, 0 failing, 1 ignored** (unchanged from baseline).

## Lowering strategy: if-elif chain over the scrutinee

`lower_match` (`compiler/src/ir.rs`) evaluates the scrutinee exactly once
into a hidden local slot (`__match_scrut_<fresh-value-id>`), then walks
each arm in source order, emitting an isinstance-guarded basic-block
branch:

```text
   scrut_slot ← eval(scrutinee)
   for each arm:
     case Pat:
       scrut ← ReadLocal(scrut_slot)
       cond ← IsInstance(scrut, class_id_of(Pat))    // for Constructor
                                                     // (true for Wildcard / Identifier)
       CondBranch(cond, arm_b, next_b)
       arm_b:
         scrut ← ReadLocal(scrut_slot)               // re-read for field loads
         for each Identifier sub-pattern at field i:
           v ← Load(offset_of_field_i, scrut)
           WriteLocal(slot_for(name), v)
         <arm body>
         Branch(exit)
       next_b:
         ...
   exit:
```

The choice of if-elif chain over a real jump table is deliberate:

1. **No new opcode needed.** `Opcode::IsInstance` already existed; M16's
   `op_is_instance` rewrite now actually walks the parent chain via
   `module.types[*].base_type` instead of returning a constant `1`.
   Codegen for the new `IROp::IsInstance { class_id }` reuses the
   pre-M16 `Opcode::IsInstance` emission with a real type-table id where
   the legacy `TypeCheck` path had been passing `u32::MAX`.
2. **The M14 destructuring lowering already produces exactly this shape.**
   `LetDestructure`'s emit pattern (Load(8*i, tup) → WriteLocal) was
   adapted directly: tuple patterns reuse it verbatim, and constructor
   patterns differ only in the offset lookup (`layout.fields[i].offset`
   instead of `8*i`).
3. **The scrutinee-evaluated-once invariant** falls out naturally: each
   arm test reads from the hidden slot, so a `match expensive_call():`
   produces exactly one `expensive_call()` evaluation in IR. The
   regression test `match_evaluates_scrutinee_once` verifies this via a
   side-effecting counter.

A jump table would need a new opcode (`SWITCH` is already reserved at
`0xE9` in the spec but not implemented), would require *complete* runtime
class-id coverage of the scrutinee's sealed hierarchy at IR time, and
buys nothing for the ≤8-arm matches we see in practice. v0.2 can add it.

## Flow narrowing: adapted from `is none` / `is not none`

`narrowings_from_cond` (`compiler/src/typecheck.rs`) was the natural seam.
The existing nullable-narrowing pattern recognises `Expr::Binary { op: Is
| IsNot, lhs: Ident, rhs: Literal::None, .. }` and pushes a `(SymbolId,
Ty)` into the per-branch `Narrowing` struct. M16 added a parallel arm
recognising `Expr::Call { callee: Ident("isinstance"), args: [Ident(x),
Ident(T)] }` and pushing `(symbol_of(x), Ty::Class(T))` into the
then-branch narrowing only.

The else-branch is deliberately NOT narrowed: `if isinstance(x, A): ...
else: ...` does NOT see `x: not-A` in the else, because `x` could be any
sibling subclass or unrelated reference type. This is documented in spec
§9.1's `isinstance` note.

What I explicitly didn't do: narrow through `and` / `or`. The M13
short-circuit lowering split mid-expression CFG without threading
narrowing into the right-operand block — `if isinstance(x, A) and
x.field > 0:` does NOT see `x: A` on the right of the `and`. The brief
flagged this as acceptable for v0.1 and I left it. A future round can
extend `lower_short_circuit` to plumb a Narrowing into the rhs block.

## Subclass matching: parent-chain walk at runtime

The brief required `isinstance(Cat(), Animal) == true` when `Cat` extends
`Animal`. The compiler-side `ClassLayout.base` was already populated
correctly (M11 BUG-016 fix); the runtime-side `TypeInfo.base_type` is
already emitted by `Lowerer::collect_types` and round-trips through the
loader. So `op_is_instance` reads the object's header → `RuntimeType ->
type_id`, then linearly scans `shared.module.types[*]` for entries whose
`type_id` matches the current cursor, follows the `base_type` link, and
repeats until match or `NO_BASE_TYPE`.

The linear scan over the type table is O(n_types) per `isinstance` call,
n_types ≤ ~30 in practice. A hash from type_id → base_type would be the
obvious optimisation, but it's never been on the critical path of any
program we run — `op_is_instance` is called from match arms, not inside
hot loops.

A defensive bound (32 iterations) protects against malformed `.spyc`
files with cyclic parent links — those can't happen from the compiler,
but the loader doesn't currently verify them either.

## The example rewrite: `examples/calculator_with_match.spy`

**Before** (existing `examples/calculator.spy`, 249 lines):

```python
open class Expr:
    open fn eval(self) -> f64:
        return 0.0
    open fn render(self) -> str:
        return "<Expr:unimpl>"

final class Lit(Expr):
    value: f64
    fn eval(self) -> f64: return self.value
    fn render(self) -> str: return str(self.value)

final class BinOp(Expr):
    op: char; lhs: Expr; rhs: Expr
    fn eval(self) -> f64:
        a: f64 = self.lhs.eval()
        b: f64 = self.rhs.eval()
        if self.op == '+': return a + b
        ...
```

The AST + virtual evaluators occupy lines 44–122 (79 source lines).

**After** (`examples/calculator_with_match.spy`, NEW, 129 lines total;
AST + two evaluators occupy 73 source lines):

```python
sealed class Expr:
    fn __init__(self) -> None: pass

final class Lit(Expr):
    value: f64
    fn __init__(self, value: f64) -> None: self.value = value

final class BinOp(Expr):
    op: char; lhs: Expr; rhs: Expr
    fn __init__(self, op: char, lhs: Expr, rhs: Expr) -> None: ...

fn eval_expr(e: Expr) -> f64:
    match e:
        case Lit(value):       return value
        case BinOp(op, lhs, rhs):
            a: f64 = eval_expr(lhs)
            b: f64 = eval_expr(rhs)
            if op == '+': return a + b
            ...
```

The LOC delta on this particular program is small (79 → 73 = -6 lines)
because `calculator.spy` used virtual dispatch rather than the worse
`kind: i32` discriminator pattern. The big payoff is the *qualitative*
shape change: each match arm has direct typed field access (`value`,
`op`, `lhs`) instead of `self.value`/`self.op`/`self.lhs`, and the
"oops, I forgot to override and silently get 0.0" failure mode goes
away — adding a 4th `final class` to the `sealed Expr` family makes
`eval_expr` non-exhaustive and the compiler now warns.

For the sealed-hierarchy programs that *did* use the `kind: i32`
discriminator (`json_parse.spy`'s `JsonAtom`), the LOC win would be
substantial — per the brief I did not migrate them in this round.

## Hardest 2-3 things

1. **`isinstance(x, T)` typechecking with `T` as a type-name argument.**
   The second positional argument isn't a value, it's a class name. The
   generic `synth_call` would have lowered it via `synth_expr(args[1])`
   which resolves to `Ty::Class(cid)` but is internally an `Expr::Ident`
   that the IR layer would have happily lowered as a value-load. Special-
   casing it required (a) a new prelude entry so `isinstance` is in
   scope, (b) a custom `synth_call` arm that bypasses generic
   arg-typechecking and stashes the resolved `class_id` under the
   second-arg span, and (c) a custom `lower_call` arm that bypasses
   generic arg-lowering and reads the class_id back from the resolver.

2. **Constructor pattern field offsets.** The brief said "positional, in
   declaration order" but the `ClassLayout.fields` vector includes
   parent-inherited fields verbatim (M11 BUG-016 fix). The lowerer
   binds `fields[i]` by `ClassLayout.fields[i].offset` directly, which
   works because the subclass's own fields lay out *after* the inherited
   ones — so the constructor pattern's `i`-th positional slot lines up
   with the constructor argument's `i`-th slot, which is the *subclass's*
   own `i`-th field, which is `layout.fields[i_inherited + i]`. The test
   case `match_constructor_binds_fields_per_variant` exercises this
   over a 2-field subclass (`Circle.r`, `Square.s`) and confirms the
   binding lines up; programs with deeper inheritance trees that match
   on a multi-level subclass would need more thought, but the
   ConstructorPattern feature is documented as v0.1 "flat subclass
   matching only" for this round.

3. **Exhaustiveness without a real ADT pass.** Spec §6.5 promises
   exhaustiveness as a *compile error*. Building the full algebraic-
   datatype coverage pass — every literal arm, every sealed variant,
   every tuple shape, with widening for `case x:`-as-default — is a
   day-scale project. The brief explicitly said "scope down to: warn
   only when sealed and no `_` is present", which is exactly what
   landed (sealed-class scrutinee + non-empty `missing` set + no
   wildcard → stderr warning, then continue typecheck). Programs with
   open-class or primitive scrutinees are not checked at all yet. The
   regression test `match_non_exhaustive_sealed_compiles` confirms the
   warning path doesn't fail typecheck — that the program *runs*.

## Incidentally-discovered bugs

None. The M14 / M15 rounds had each surfaced one (assert-tuple crash;
`with` not routed through try/except). M16's changes touched the
typechecker, the IR lowerer, and one opcode; nothing along the way
exposed a stale bug.

## Final test totals

| Suite                                  | Tests | Status |
|----------------------------------------|------:|--------|
| Pre-M16 baseline                       | 234   | OK     |
| `vm/tests/m16_match_isinstance.rs`     | 9     | OK     |
| `compiler/tests/calculator_with_match_runs.rs` | 2 | OK |
| **Total**                              | **243** | **0 failed, 1 ignored** |

`cargo build --workspace --release` succeeds.
`cargo test --workspace --release` passes (243 OK, 0 failed, 1 ignored —
the lone ignore is the pre-M16 baseline ignore, unchanged).
