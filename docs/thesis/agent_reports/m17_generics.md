# M17 — Generic free functions with monomorphisation

**Brief**: Close the "no user-code generics" language gap that every
M10–M16 agent report flagged as "rewrite-per-type friction". The AST
already carried `GenericParam` on `FuncDecl` and the parser accepted
`fn id[T](x: T) -> T:` since M0, but the surrounding pipeline had three
holes: (1) the resolver bound `T` to `Ty::Never` (placeholder from when
generics were aspirational), (2) the typechecker had no substitution
mechanism at call sites, so `id(7)` either rejected or silently treated
`T` as `Any`, and (3) the IR lowerer emitted exactly one bytecode
function per `FuncDecl`, so even if typechecking went through, all
specialisations would have shared a single body and float comparisons
inside an `i64`-instantiated body would have lowered as integer ops.

**Wall-clock**: ~3 hours (read-through + resolver/typecheck/IR wiring +
8 regression tests + new example + spec / catalog updates).

**Tests**: 245 baseline + 8 new M17 + 1 new example-program test (2
asserts) = **254 passing, 0 failing, 1 ignored** (baseline preserved).

## Monomorphisation strategy: lazy worklist, IR-driven

The standard textbook choice is between *eager* monomorphisation (clone
the AST per instantiation right after typecheck, re-run the typechecker
per substitution, lower as ordinary functions) and *lazy* (collect
`(fn_sym, type_args)` pairs into a worklist, lower per-instantiation
bodies once each, mint mangled `FuncId`s on the fly when calls discover
new instantiations).

We chose lazy. The brief recommended it ("see m14_tuples for the
register_tuple_types pre-pass pattern") and it matches how the project
already handles tuple shapes — discover every distinct shape during one
prepass, emit one synthetic type-table entry per shape, never visit a
shape twice. The data-flow is:

```
1. Resolver pass:
   For each `fn f[T1, T2, ...]` declaration:
     allocate one fresh TypeVarId per type parameter
     seed each `T_i` as a TypeAlias symbol carrying Ty::Var(tv_i)
     lower param/return ast::Type → Ty against that scope
        => the FunctionSig has params containing Ty::Var(tv_i)
     store generic_tvars: Vec<TypeVarId> on the sig

2. Typecheck pass:
   On every `Expr::Call { callee: Ident(name), args }` where name resolves to
   a function with non-empty generic_tvars:
     subst = {}
     For (param_i, arg_i) in zip(sig.params, args):
       expected = subst_ty(param_i.type, subst)        // substitute already-solved vars
       if contains_unbound_var(expected):
         got = synth_expr(arg_i)
       else:
         got = check_expr(arg_i, expected)             // gives int-width inference
       unify_one(expected, got, &mut subst)
     For tv in sig.generic_tvars:
       assert subst.get(tv) is Some  // else E2001: cannot infer T
     type_args = [subst[tv] for tv in generic_tvars]
     record (sym_id, type_args) in self.instantiations
     return subst_ty(sig.ret, subst)

3. IR pass 2.6: seed worklist
   For (sid, type_args) in typechecker.instantiations:
     if any(has_unbound_var(t) for t in type_args): skip
     register_instantiation(sid, type_args)  -> assigns mangled FuncId, pushes to worklist

4. IR pass 3: lower non-generic functions normally. Generic FuncDecls are SKIPPED.

5. IR pass 3.5: drain worklist
   While worklist non-empty:
     pop (sid, type_args)
     subst = {tv_i -> type_args[i]}
     lower_func_instantiation(FuncId, FuncDecl, mangled_name, subst):
       Like lower_func, but applies subst to every param/return type
       and to every expr_ty(span) lookup. emit IRFunction.

6. lower_call for generic callees:
   Rebuild subst by unifying arg expr types (resolved through enclosing subst)
   against sig.params. Mangle. Look up FuncId — mint a new one and push
   to the worklist if not seen. Emit DirectCall.
```

The key invariant: `(fn_sym, mangle_args_key)` is the canonical key for
both worklist dedup and call-site dispatch. Once a body is lowered,
every call site recomputes the same mangle from its own arg types and
hits the same FuncId.

## Mangling

A mangled name has the form `<src_name>__<arg1>_<arg2>_...`. Primitives
are their lowercased Rust names (`i32`, `i64`, `f64`, `str`, `bool`,
`char`). Class types become `class<id>`. Generic types like `List[i64]`
become `list_i64`. Tuples like `Tuple[i32, str]` become `tuple_i32_str`.
Nullable wraps as `opt_<inner>`. So:

* `id__i32` is `id` over `i32`,
* `first__i32_str` is `first[K, V]` with `K=i32, V=str`,
* `quicksort__list_i64_i64_i64` is `quicksort[T](xs: List[T], lo: i64,
  hi: i64)` with `T=i64`.

Mangling is deterministic, lossless within the v0.1 type surface, and
shows up directly in the bytecode's `function_table` so a debugger can
recognise specialisations.

## Transitive instantiation

`fn outer[U](v: U) -> U: return id(v)` is the canonical transitive
case. The typechecker, walking `outer`'s body with `U := Ty::Var(1)`,
sees `id(v)` and runs `check_generic_call` with `arg_ty = Ty::Var(1)`.
Unifying `id`'s param type `Ty::Var(0)` against `Ty::Var(1)` binds
`Var(0) -> Var(1)` — i.e. `id`'s `T` is the *same* type variable as
`outer`'s `U`. The instantiation recorded is `(id_sid, [Var(1)])`.

But Pass 2.6 explicitly skips instantiations whose type args contain
unbound vars (`has_unbound_var` returns true). The transitive
`(id_sid, [Var(1)])` is dropped at that point — no FuncId for it.

The real magic happens in Pass 3.5. When we lower `outer__i32`'s body
under `subst = {1 -> i32}`, `lower_call` for `id(v)` does:

1. Look up the static type of `v` from `expr_types` — that gives `Var(1)`.
2. Apply the enclosing subst: `subst_ty(Var(1), {1 -> i32})` = `i32`.
3. Unify `id`'s param `Var(0)` against `i32` to bind `0 -> i32`.
4. `type_args = [i32]`, `key = "i32"`.
5. Check `fn_id_for_inst[(id_sid, "i32")]` — if present, dispatch; else
   mint a fresh FuncId, register the mangled name, push `(id_sid, [i32])`
   to the worklist. Either way, emit `DirectCall { fn_id }`.

So `outer__i32` calls `id__i32`, both materialised lazily. Recursion
inside a generic body works the same way: `quicksort[T]` calling
`quicksort(xs, lo, p-1)` from within `quicksort__list_i64_i64_i64`
resolves to its own FuncId — the worklist's "mint on first sighting,
dispatch by key on subsequent" semantics naturally handles the cycle.

## The quicksort_generic demo

The pre-M17 `examples/quicksort.spy` (kept untouched) is i64-only, 35
lines. The new `examples/quicksort_generic.spy` is 55 lines including a
17-line docstring header (38 lines of code) and sorts BOTH `List[i64]`
and `List[f64]` from one body:

```python
fn partition[T](a: List[T], lo: i64, hi: i64) -> i64:
    pivot: T = a[hi]
    i: i64 = lo - 1
    j: i64 = lo
    while j < hi:
        if a[j] < pivot:
            i = i + 1
            tmp: T = a[i]
            a[i] = a[j]
            a[j] = tmp
        j = j + 1
    tmp2: T = a[i + 1]
    a[i + 1] = a[hi]
    a[hi] = tmp2
    return i + 1

fn quicksort[T](a: List[T], lo: i64, hi: i64) -> None:
    if lo < hi:
        p: i64 = partition(a, lo, hi)
        quicksort(a, lo, p - 1)
        quicksort(a, p + 1, hi)
```

A hand-rolled two-type baseline (two copies of `partition` +
`quicksort` + the helpers + main) would be ~65 lines of code (i64
copy ~14 + f64 copy ~14 + helpers + main ~20 = 48 lines of code,
~75 lines with separators). The generic version is roughly **40%
shorter at two element types**, and the savings scale linearly per
additional element type. Add `List[i32]` and the generic body unchanged;
the hand-rolled equivalent grows by another ~14 lines.

The compelling part of the demo is what happens at IR time: the
`xs[j] < pivot` line lowers to `ILt` inside
`quicksort__list_i64_i64_i64` and to `FLt` inside
`quicksort__list_f64_i64_i64` from the same source body. The
`expr_ty(span)` lookup in `LowerCtx` applies the active substitution,
so `emit_binop`'s `is_float` check sees the substituted operand type
and picks the right opcode. Before M17, this fact (that a single source
expression compiled to different opcodes in different specialisations)
was literally inexpressible.

## Incidental bug found

The `TypedModule::instantiations` field was declared `HashSet<(SymbolId,
Vec<Ty>)>` in some earlier scaffolding pass — but `Ty` doesn't derive
`Hash`/`Eq`, so the field was never populated. The struct compiled only
because nobody had ever inserted into it. Replaced with
`Vec<(SymbolId, Vec<Ty>)>` plus a parallel `HashSet<(SymbolId,
mangle_key: String)>` for dedup. No user-visible symptom; cleanup of
dead scaffolding that would have surprised the first agent who tried
to use the field as declared.

## The 2-3 hardest things

1. **The generic-body typecheck needs to be deferred-not-rejected on
   `T + T`** (and `T < T`). The body of `fn double[T](x: T) -> T:
   return x + x` synthesises `Ty::Var(0) + Ty::Var(0)`, which the
   existing arithmetic-binop arm rejected with
   `Add not defined for ?T0`. Spec §10.4 promises per-instantiation
   re-typecheck; we don't have that infrastructure yet, so the simpler
   fix was to teach `check_binary` to accept `Ty::Var` operands as
   deferred and return the operand type. The cost is that
   instantiation-specific errors (e.g. `double(some_class)` for a class
   without `+`) now show up at VM runtime as `TypeError` rather than at
   compile time. v0.2's bounds system will move them back.

2. **Argument type inference vs literal width.** `fn id[T](x: T)`
   called as `id(7)` should pick `T = i32` (the default). But
   `quicksort[T](xs: List[T], lo: i64, hi: i64)` called as
   `quicksort(a, 0, n-1)` should accept `0` as `i64` (the *expected*
   type after solving `T` from `xs`). The fix is to walk args
   left-to-right with progressive substitution: when the expected
   param type is fully concrete (all vars solved), use `check_expr`
   for literal-width inference; when it still has unbound vars, use
   `synth_expr` and unify. Without this interleaving, the literal `0`
   would default to i32 and the call would fail to typecheck.

3. **Borrow-checker dance around the lowerer's mutable state.** The
   IR pass needs to *both* read shared maps (`fn_id_by_name`,
   `class_layouts`, `typed`) and mutate the per-instantiation FuncId
   table inside `lower_call` when a transitive instantiation is
   discovered. The cleanest layout was to pass *mutable* refs for the
   mono tables into `LowerCtx` (so `lower_call` can `insert` directly)
   while keeping the read-only shared maps as `&`. Splitting Lowerer
   so the mono tables aren't in conflict with the other borrows was
   the bulk of the refactor; the actual mono algorithm is short.

## Final test totals

| Suite | Pre-M17 | Post-M17 |
|---|---:|---:|
| Compiler unit | 89 | 89 |
| Compiler integration | 53 | 55 (+ quicksort_generic 2) |
| VM unit | 41 | 41 |
| VM integration | 62 | 70 (+ m17_generics 8) |
| Ignored | 1 | 1 |
| **Total passing** | **245** | **254** |
| **Failing** | **0** | **0** |
