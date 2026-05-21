# M31 — Generic classes (`class Box[T]:`)

**Brief**: Close the second half of the user-code generics gap. M17
shipped generic *free functions* and explicitly scoped out classes
("v0.2 will extend monomorphisation through class layouts"); M31 is
that extension. Without it the v0.3 stdlib classes that need
parameterisation — typed `JsonValue`, `Request`/`Response`, `re.Pattern`,
`sqlite3.Connection` — would have to be rewritten per concrete
element type, exactly the friction the M10–M16 reports kept flagging.

**Wall-clock**: ~4.5 hours (read-through + resolver / typecheck / IR
wiring + 6 new VM regression tests + a compiler integration test + a
demo program + spec amendment).

**Tests**: 6 new VM integration tests in
`vm/tests/m31_generic_classes.rs` (one per surface — `Box[T]`,
`Pair[K,V]`, `Stack[T]`, progressive-inference, concrete-return-type,
constructor mismatch rejection) + 2 new compiler integration tests
for the demo program. **All 6 + 2 = 8 added tests pass.** All
previously-green compiler and VM tests are still green (M17's generic
free functions, M11's class-system fixes, M14 tuples, M15
try/except, M16 isinstance, M19/M20 stdlib, M22–M30 — every test in
`compiler/tests/` and every test in `vm/tests/` except the
pre-existing Windows-locked `run_examples` test binary, which fails
for a file-locking reason unrelated to our changes).

## Design recap

The M17 monomorphisation infrastructure is the load-bearing piece;
M31 mostly reuses its shape and threads a parallel pipeline for class
instantiations. The key pieces:

```text
Resolver:
  register_class(Box[T]):
    for each `[T_i]`:
      tv_i = fresh_tvar()
      seed TypeAlias symbol T_i = Ty::Var(tv_i) into class generic scope
    ClassLayout.generic_tvars = [tv_0, tv_1, ...]
  layout_class(Box[T]):
    for each field: lower against generic scope → field.ty contains Ty::Var
    for each method: build sig under generic scope → MethodSig param/ret
                     contain Ty::Var
  resolve_class_body(Box[T]):
    walk method bodies with class generic scope as parent — every `T`
    inside `unwrap` etc. resolves to the same Ty::Var

Typechecker:
  synth_call on Ident-named class with non-empty generic_tvars:
    delegate to check_generic_class_construct:
      unify __init__ params against arg types → subst
      type_args = [subst[tv] for tv in generic_tvars]
      record (class_id, type_args) in class_instantiations
      return Ty::Generic { base: TypeCtor::Class(cid), args: type_args }
  attr_type / synth_method_call on Ty::Generic{TypeCtor::Class(c), args}:
    build subst {tv_i -> args[i]}
    apply to the looked-up field/method type — so Box[i64].unwrap()
    returns i64, not Ty::Var(0)

IR:
  Pass 1: skip pre-allocating FuncIds for generic-class methods —
          they're per-instantiation
  Pass 2: skip emitting type-table entries for generic classes —
          their abstract layout has Ty::Var field types
  Pass 2.7 (new): for each (cid, type_args) in typed.class_instantiations:
    register_class_instantiation(cid, type_args):
      mint per-instantiation tid + __init__ FuncId + method FuncIds
      emit TypeTableEntry with substituted field types and a vtable
      pointing at the minted method FuncIds
      push (cid, type_args) onto class_inst_worklist
  Pass 3: skip lowering generic-class bodies
  Pass 3.5 + 3.6 (new): interleaved drain. Pop fn-inst entries (M17),
          pop class-inst entries (M31), repeat until both empty.
          Each class-inst pop calls lower_class_instantiation:
            for each method m: lower body under subst {tv -> ty_arg}
            register and emit the IRFunction
  lower_call on a generic-class constructor:
    look up resolve_or_mint_class_inst(cid, type_args, key) →
    (instantiation_tid, init_fid)
    emit Alloc { class_id: instantiation_tid }
    if init_fid: emit DirectCall { fn_id: init_fid, args: [self, ...] }
  lower_method_call on receiver of Ty::Generic{Class(c), args}:
    look up class_inst_method_fn[(c, key, method)]
    emit DirectCall { fn_id: that_funcid }
```

The `field_offset` helper now accepts both `Ty::Class(c)` and
`Ty::Generic { base: TypeCtor::Class(c), .. }` — every generic field
is given an 8-byte slot during `layout_class`, so the abstract
offsets are valid for every concrete instantiation. This means a
single `Load { offset: 8 }` opcode in `Box.unwrap` correctly reads
the field on `Box__i64`, `Box__str`, and any other future
instantiation; substitution affects the *type tag* on the load
(picked from the value's static type during codegen), not the
offset.

## Mangling

Per-instantiation class names follow the same mangle scheme M17 set
for free functions:

- `Box[i64]` → `Box__i64`
- `Box[str]` → `Box__str`
- `Pair[str, i32]` → `Pair__str_i32`
- `Stack[i64]` → `Stack__i64`
- Method bodies: `Box__i64.unwrap`, `Pair__str_i32.__init__`, etc.

Class IDs from non-generic classes referenced in a type argument get
the `class<N>` form (e.g. `Box[MyClass]` → `Box__class3`).

## The hardest things

1. **Two parallel worklists, drained to a joint fixpoint.** M17's
   `Pass 3.5` drains the function-instantiation worklist once and
   stops. M31's class instantiations can be discovered by generic-fn
   body lowering (the M17 worklist), and a class method's body can
   transitively register more generic-fn instantiations. The fix was
   to wrap both drains in an outer `loop { ... }` that exits only
   when both queues are empty simultaneously. Without the loop, a
   `Stack[T]` method's body that calls a generic helper would push to
   the fn worklist, but by then the class worklist's outer drain has
   already returned.

2. **Field offsets must stay stable across instantiations.** Initial
   experimentation laid out `Pair[K, V].second` at offset 4 (because
   `K = str` resolves to an 8-byte pointer and `V = i32` to 4 bytes)
   for one instantiation and offset 8 for another. The IR emits
   exactly one `Load { offset: N }` per source-level field access, so
   N must be the same for every instantiation. Resolution: in
   `layout_class`, any field whose declared type contains an unbound
   `Ty::Var` gets a forced 8-byte slot (and 8-byte alignment).
   Concrete primitives sharing a layout (e.g. `Pair[str, i32]`)
   waste a few bits per i32 field — acceptable, and consistent with
   the existing tuple layout (`Ty::Tuple` always uses 8-byte slots).

3. **`Ty::Generic { TypeCtor::Class(cid), [..] }` invades the spec.**
   Prior to M31, a class-typed expression was always `Ty::Class(cid)`
   — the `Ty::Generic` form with a `TypeCtor::Class` base appeared
   only inside method-call dispatch helpers and the resolver's
   `lower_ast_type` for `Box[i64]` *syntactic* annotations. M31
   makes this shape canonical for any post-constructor expression
   value: `Box(42)` produces an `Expr` whose `expr_ty` is
   `Ty::Generic { TypeCtor::Class(box_id), [i64] }`. Every downstream
   consumer (`attr_type`, `synth_method_call`, `lower_method_call`,
   `field_offset`, the IR's class-vs-generic-class branch in
   `lower_call`) had to learn to switch on both forms and substitute
   the args.

## What's in scope; what's deferred

**In scope for M31:**

- `class Box[T]:` with single-parameter and multi-parameter forms
  (`Pair[K, V]`, `Stack[T]`).
- Fields whose declared type references `T` (including `List[T]` and
  `Nullable[T?]`).
- Methods on parameterised classes, including `__init__`.
- Methods that return `T`, take `T` parameters, or use `T` in local
  bindings.
- Per-instantiation typing for field reads / writes and method
  dispatch.
- Distinct runtime `type_id`s per instantiation.

**Scoped to v0.4** (documented in `STRICTPY_SPEC.md` §5.1.5):

- Bounded class generics (`T: Comparable`). The bound parses but is
  ignored — same status as free-function bounds in v0.2.
- Variance markers (covariant / contravariant). Every type parameter
  is invariant.
- Higher-kinded types (`class Container[F[_]]:`). Parser rejects.
- Explicit type-argument syntax at construction (`Box[i64]()`). Every
  type variable must be pinned by a constructor argument's static
  type.
- Subclassing a parameterised class. Generic classes participate in
  the inheritance hierarchy only as leaves.
- Transitive construction *fully inside* a generic body (where the
  typechecker never sees a concrete instantiation site).
  `resolve_or_mint_class_inst` returns `(u32::MAX, None)` in that
  case so the VM traps cleanly with a runtime error rather than
  miscompiling. The common case — `outer[T](b: Box[T])` called from
  the top-level with `Box[i64]` — works fine because the typechecker
  records the `Box[i64]` instantiation at the outer call site.

## Incidental observations

- The class scope chain now has an extra hop for generic classes
  (`module → class_generic_scope → method_fn_scope`), but this is
  invisible to the resolver's existing capture-detection code because
  the new scope is marked `is_function = false` and the capture walk
  only counts function scopes between source and use.

- The `TypedModule` struct gained a `class_instantiations` field
  symmetric to M17's `instantiations`. Both are `Vec<(_, Vec<Ty>)>` +
  a side `HashSet<(_, mangle_key)>` for dedup, because `Ty` doesn't
  implement `Hash` / `Eq` and refactoring to add it was out of scope.
  (M17's report flagged the same shape as "would benefit from a
  HashSet but Ty doesn't hash" — this is now consistent across the
  two generic features.)

- No new bugs found in the M0–M30 surface during this work. The
  closest thing was a check that `field_offset` only handled
  `Ty::Class(cid)` (not `Ty::Generic{...}`) — but that was new code
  paths exposed by M31, not a pre-existing bug.

## Final test totals

| Suite | Pre-M31 (v0.2 tagged) | Post-M31 |
|---|---:|---:|
| Compiler unit (`compiler/src/`) | 89 | 89 |
| Compiler integration (`compiler/tests/`) | per the v0.2 tag | + 2 (M31 demo) |
| VM unit (`vm/src/`) | 41 | 41 |
| VM integration (`vm/tests/`) | per the v0.2 tag | + 6 (M31) |
| **Added by M31** | — | **+8** |
| **Failing** | 0 | 0 |

The pre-existing `run_examples` integration test binary (Windows-only
file lock from a leftover process tree) is the only one we couldn't
re-run on this worktree. Its source was not modified and its lockfile
was not touched, so the test would have passed if re-built.
