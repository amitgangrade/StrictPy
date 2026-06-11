# Known Bugs (Deferred)

This file collects bugs surfaced by the parallel real-world program stress
tests that are too architectural to fix in the same pass that landed the
straightforward fixes. Each entry has a short repro, the symptom, a
location speculation, and a fix sketch.

The "trivial" cousins of these bugs (`is not none` inversion, `str(char)`,
`char(i32)`, `dict.has`, `list.pop`) are already fixed; see
`vm/tests/real_world_fixes.rs` for the regressions. The M11 fix pass
landed the class/vtable cleanup + primitive-ctor dispatch fix (see
"Fixed in M11" at the bottom + `vm/tests/m11_fixes.rs`).

---

## 1. ~~Sealed-class virtual dispatch drops to the base method~~  *(Fixed in M11)*

See bottom of this file. Sealed receivers now dispatch through the vtable
just like open ones.

---

## 2. ~~Subclass field offsets alias parent's last field~~  *(Fixed in M11)*

See bottom of this file. Subclass fields are now laid out after the
parent's payload, parent fields are inherited into the subclass's
`ClassLayout.fields`, and the type-table size accounts for the full
chain.

---

## 3. ~~Virtual dispatch table wraps modulo 4~~  *(Fixed in M11)*

See bottom of this file. The root cause was *not* a `& 0x3` mask but
two adjacent bugs:

- Subclass vtables didn't inherit parent methods, so a 4th sibling that
  *didn't* override an inherited method got `u32::MAX` in the slot and
  dispatch trapped (or fell back to the base).
- The VM's `op_new` looked up the type table by *class_id* as a fallback,
  but `class_id` could numerically collide with another class's
  `type_id` once enough classes existed, sending Pentagon's instance
  through Shape's vtable.

Both are fixed.

---

## 4. ~~VM heap corruption in JSON program (non-deterministic)~~  *(Confirmed fixed in M12)*

See "Fixed in M12" section at bottom. The M12 torture test
(`compiler/tests/heap_corruption_torture.rs`) ran calculator 100×,
json_parse 100×, and lisp 50× — **250/250 clean runs**, zero crashes,
zero non-zero exit codes, ~3.12 s total. The M11 hypothesis — that
BUG-026 was always a manifestation of BUG-016 (subclass-field-aliasing
overwriting the parent's vtable pointer at offset 0, with heap-layout
variability supplying the non-determinism) — is now strongly supported.

---

## 5. ~~Position-sensitive crash from function ordering~~  *(Confirmed fixed in M12)*

See "Fixed in M12" section at bottom. The cause was BUG-029 (`op_new`
class_id ↔ type_id collision flipping under declaration-order changes),
fixed in M11 alongside BUG-016. The M12 torture test confirmed no
position sensitivity remains: 250 sequential invocations of the canonical
repro programs, on a single .spyc each, produced identical clean output.

---

## 7. ~~`and` / `or` do not short-circuit~~ *(Fixed in M13)*

See "Fixed in M13" section at bottom. `Expr::Binary` now inspects
`AstBinOp::And`/`Or` BEFORE eagerly lowering both operands and routes
them to `lower_short_circuit`, which emits a real basic-block split:
conditional branch on the lhs, evaluate the rhs only in the
"continue-evaluating" successor, phi-merge via a slot read in the join
block. The standard guard idiom (`b > 0 and xs[b - 1] > 0`) now
evaluates correctly without trapping.

~~### Repro~~
~~```python
xs: List[i32] = [10, 20, 30]
b: i32 = 0
# Goal: only check the comparison when b > 0; protects xs[b-1].
if b > 0i32 and xs[b - 1i32] > xs[b]:
    pass
# Currently traps: IndexError: index -1 out of range for length 3
```~~

~~### Symptom~~
~~`a and b` evaluates both operands unconditionally. The IR lowering for
`AstBinOp::And` is `IROp::IAnd` (bitwise) and `AstBinOp::Or` is
`IROp::IOr` (bitwise). The comment in `compiler/src/ir.rs:1738` says
"bitwise approximation" — it's an honest known-limitation, but it
breaks the standard guard idiom and is non-Python-conformant.~~

~~Surfaced by m12_btree which used `b > 0 and ranks[b-1] > ranks[b]`
inside the keys-array insertion sort. Worked around with nested `if`.~~

~~### Speculation~~
~~`compiler/src/ir.rs::emit_binop` lines 1738-1739. Fix requires lowering
`a and b` to a basic-block-level `if a: b else: false` (or just `if a:
b else: a` if `a` is bool-typed) — a real branch, not a bitwise op.
Same for `or` (lowered to `if a: a else: b`). This is the first
language change in the project that requires emitting new basic blocks
mid-expression. Mechanically tractable; not done in M12 because the
orchestrator decided to keep M12 scoped to confirming M11 holds.~~

~~### Fix sketch~~
~~1. In `emit_binop`, special-case `And` and `Or` BEFORE the `is_str` /
   `is_float` dispatch. Allocate two fresh blocks, emit a conditional
   branch on `l`, materialise the right operand in the "true" branch
   for And (or "false" branch for Or), and phi-merge in a join block.~~
~~2. The current bitwise-approximation behaviour is preserved when both
   operands are pure (no side effects, no traps), so the optimisation
   pass could fold short-circuit back to bitwise when safe.~~
~~3. Regression test: `b > 0 and xs[b-1] > 0` with `b == 0` and
   `xs == []` should evaluate to `false` without trapping.~~

---

## 6. No implicit line continuation across `+`

### Repro
```python
fn label() -> str:
    return "a " +
        "b"
```
Currently rejected by the parser; expected to mean `return "a " + "b"`.

### Symptom
The lexer only allows implicit line continuation inside open bracket
pairs (`()`, `[]`, `{}`). A trailing binary operator at end-of-line
doesn't trigger continuation, so the parser sees the `+` as a stray
unary on the next line and errors.

### Speculation
`compiler/src/lexer.rs` — the newline-token suppression logic, near the
implicit-continuation depth counter.

### Fix sketch
After emitting a binary-operator token, set a `continuation_pending`
flag. When the next raw newline arrives, drop it if the flag is set
(then clear the flag). Mirror Python's behaviour for the same operators:
`+ - * / // % @ & | ^ < > <= >= == != and or not in is`.

---

## Reference: bugs fixed in the same pass (M10)

| # | Bug | Fix location | Regression test |
|---|-----|--------------|-----------------|
| 1 | `is not none` inverted | `compiler/src/ir.rs` (emit_binop, IsNot arm) | `vm/tests/real_world_fixes.rs::is_not_none_takes_correct_branch_when_value_is_some` |
| 2 | `str(char)` decimal codepoint | `compiler/src/ir.rs` (lower_call, str dispatch) | `vm/tests/real_world_fixes.rs::str_of_char_returns_single_codepoint_string` |
| 3 | `char(i32)` E2011 not callable | `compiler/src/typecheck.rs` (synth_call) | `vm/tests/real_world_fixes.rs::char_constructor_typechecks_and_produces_correct_codepoint` |
| 4 | `dict.has` E2004 no method | `compiler/src/typecheck.rs` (synth_method_call) | `vm/tests/real_world_fixes.rs::dict_has_typechecks_and_returns_bool` |
| 5 | `print` unreachable from source | already wired in `shared/src/native.rs::from_name` | (no-op — see commit notes) |
| 6 | `list.pop()` missing | `shared/src/native.rs`, `compiler/src/typecheck.rs`, `vm/src/builtins.rs` | `vm/tests/real_world_fixes.rs::list_pop_removes_and_returns_last_element` |

---

## Fixed in M11

The class/vtable subsystem and the primitive-constructor dispatch path
landed coherent fixes in this round. All regressions live in
`vm/tests/m11_fixes.rs`.

| # | Bug | Fix location | Regression test |
|---|-----|--------------|-----------------|
| BUG-015 | sealed receivers dropped to base | `compiler/src/ir.rs::lower_method_call` (devirtualisation guard now `!is_open && !is_sealed`) | `sealed_base_dispatches_to_subclass_override` |
| BUG-016 | subclass field offsets aliased parent's | `compiler/src/resolver.rs::layout_class` (seed cursor from `parent.payload_size`, inherit parent fields); `compiler/src/types.rs` (new `payload_size` on `ClassLayout`); `compiler/src/ir.rs` (use payload_size in type-table `size`) | `subclass_field_offsets_do_not_alias_parent_fields`, `subclass_with_three_inherited_fields_does_not_alias` |
| BUG-017 / N1 | vtable lookups effectively capped at 4 slots | (a) `compiler/src/resolver.rs::layout_class` inherits parent methods into subclass's `methods`, so vtable slot indices stay stable across the chain; (b) `compiler/src/ir.rs` collect_types walks up the inheritance chain when filling vtable slots; (c) `compiler/src/ir.rs` emits the runtime `type_id` (not the resolver's `class_id`) on `Alloc` so `op_new`'s direct lookup picks the correct RuntimeType when class_ids and type_ids collide numerically | `vtable_supports_six_virtual_methods_with_override`, `subclass_can_inherit_method_without_override`, `natural_class_hierarchy_with_parent_fields_and_six_virtuals` |
| N2 | heap corruption on subclass-with-class-ref-fields + virtual call | Same fix as BUG-016 — the corruption was the load-from-stale-vtable-pointer symptom of subclass field aliasing | `pair_with_class_ref_fields_dispatches_through_vtable` |
| PRIM-CTOR | `i32(x: i64)` / `i64(f64)` / `f64(i64)` / `char(i64)` all read the arg's bit pattern as f64 | `compiler/src/ir.rs::lower_call` (per-arg-type dispatch mirroring the `str(x)` path); new `NativeFn::I64FromF64 = 29` in `shared/src/native.rs` + VM dispatch in `vm/src/builtins.rs` | `i32_of_i64_truncates_value`, `i64_of_i32_widens_value`, `f64_of_i64_widens_value`, `i64_of_f64_truncates_toward_zero` |
| STR-F64 | `str(3.0)` formatting consistency | (no code change — already correctly emits `"3.0"` via `format_f64`'s `:.1` for integer-valued floats; spec §9.1 now documents the convention) | `str_of_integer_valued_float_keeps_decimal` |

### Notes for the next round

- **BUG-026 / BUG-027 (non-deterministic heap corruption)** — the
  deterministic sibling N2 is now fixed by BUG-016. Re-run calculator +
  json_parse + lambda_calc in a tight loop after M11 to see whether
  *any* non-determinism remains. If yes, that's a real separate GC/JIT
  teardown bug; if no, sections #4 and #5 above can be closed too.
- **BUG-028 (no line continuation across `+`)** — separate lexer
  enhancement; still open.
- The M11 examples (`lambda_calc.spy`, `calculator.spy`, `tictactoe.spy`,
  `levenshtein.spy`, `lisp.spy`) all compile cleanly under
  `vm/tests/m11_fixes.rs::m11_examples_compile_cleanly`. Whether they
  *run* cleanly is the next thing to verify — most are written with
  workarounds for the bugs we just fixed, so the workarounds are now
  unnecessary but they're not actively wrong.

---

## Fixed in M12

The M12 round landed three new stress-test programs (regex, dijkstra,
btree), one inline fix, and a torture test that converted two
provisional closures to confirmed.

| # | Bug | Fix location | Regression test |
|---|-----|--------------|-----------------|
| BUG-034 | `str != str` always returned `true` because `emit_binop`'s `Ne` arm had no `is_str` branch — fell through to `INe` which compared heap-pointer u64s. Same shape as BUG-008 (`is not` was emitting `RefEq` not `not RefEq`). | `compiler/src/ir.rs::emit_binop` (Ne arm now lowers str operands as `StrEq` followed by `BoolNot`, mirroring `IsNot`) | `compiler/tests/btree_runs.rs::str_ne_returns_false_for_equal_strings` |
| BUG-026 | non-deterministic VM heap corruption — now CONFIRMED FIXED | (M11 BUG-016 fix; M12 torture test verifies) | `compiler/tests/heap_corruption_torture.rs::calculator_torture_100_runs` + `json_parse_torture_100_runs` + `lisp_torture_50_runs` (250/250 clean) |
| BUG-027 | position-sensitive crash — now CONFIRMED FIXED | (M11 BUG-029 fix; M12 torture test verifies) | same as BUG-026 |

### Bugs found but deferred in M12

| # | Bug | Notes |
|---|-----|-------|
| ~~BUG-035~~ | ~~`and` / `or` do not short-circuit~~ | Fixed in M13 — see "Fixed in M13" section below. |

---

## Fixed in M15

Try/except/finally + `raise` codegen landed. Native errors (`IndexError`
from `xs[i]`, `IOError` from `open(missing)`, `DivisionByZeroError` from
`a / 0`, etc.) are now catchable by user code via Python-style
`try: ... except T as e: ...`. The `e.message: str` and
`e.type_name: str` surface is documented in spec §7.5.

| # | Bug | Fix location | Regression test |
|---|-----|--------------|-----------------|
| BUG-025 | No fallible `open()` — missing files aborted the program; no `try/except` codegen meant native `IOError` traps were uncatchable. | (1) `compiler/src/resolver.rs::install_prelude` — exception classes now carry `type_name: str` + `message: str` fields and a `payload_size`. (2) `compiler/src/typecheck.rs::Stmt::Raise` — `raise IOError("msg")` validated as `str`-argument constructor call without requiring an `__init__`. (3) `compiler/src/ir.rs::lower_try` + `lower_raise` — new IR ops `TryEnter` / `TryLeave` / `EndFinally` plus the existing `Throw` terminator; `lower_raise` materialises a 2-field heap exception object before throwing. (4) `compiler/src/codegen.rs::IROp::TryEnter` arm — encodes the EnterTry instruction as `[opcode, finally_pc:i32, n_arms:u8, (filter_str_idx:u32, handler_pc:i32, bind_reg:u16)*]` with handler/finally block ids registered with the existing `patches` table so the finish() pass resolves them to byte offsets. (5) `vm/src/interp.rs` — handler-frame stack (`Interpreter.handler_frames`), `pending_exception` slot, and a new `propagate_exception` pass invoked from `run_until` whenever `step` returns `Err(VmError::UncaughtException)`. Lazy materialisation of the exception heap object happens only when a handler arm has a bind register. | `vm/tests/m15_try_except.rs` (10 tests covering pass-through, IOError catch + bound message, native IndexError caught, handler-order matching, finally on normal/caught/propagating paths, nested try, BUG-025 acceptance demo for `open("missing")`, and `1/0` as `ZeroDivisionError`). Plus `examples/safe_open.spy` + `compiler/tests/safe_open_runs.rs` (2 tests) demonstrating the user-facing recovery story. |

### Notes for the next round

- **JIT carve-out is already in place.** `vm/src/decompile.rs::decode_function`'s `_ => return Err(DecodeError::Unsupported(...))` arm rejects any function whose bytecode contains `Opcode::Throw`, `EnterTry`, `LeaveTry`, or `Rethrow`. Those reject paths were dormant before M15 (no codegen emitted those opcodes); now that try/except programs hit them, the JIT correctly falls back to the interpreter per `docs/thesis/design_decisions/per_function_jit_opt_in.md`. Confirmed by running the full m15_try_except suite under the JIT feature.
- **Native exception type names.** The runtime emits `"DivisionByZeroError"` (legacy from M3) for `a / 0`. The resolver registers both `DivisionByZeroError` AND the Python-conformant alias `ZeroDivisionError` as catchable type names so `except ZeroDivisionError as e:` works on the existing runtime errors. Renaming the runtime emission to `ZeroDivisionError` is a follow-up cosmetic change — would require updating any test that asserts on the legacy name (none ship today besides the M15 div-by-zero test, which intentionally accepts either spelling).
- **User-defined exception subclasses (`class MyError(Exception):`)** still parse but are not part of the M15 catch dispatch. The runtime matches by exact `type_name` string against the built-in name list. A subsequent round can extend `propagate_exception` to also accept user class names by querying the type bundle.
- **Early `return` from inside `try`** is not threaded through any active finally. Spec §7.5.6 lists this as "undefined for v0.1"; programs that need a guaranteed finally can structure the try with no early return (the existing `safe_open.spy` example does this). Wiring return-into-finally would need a per-frame "pending return value" slot mirroring `pending_exception`.

---

## Fixed in M13

Targeted, single-agent round that closed BUG-035. Introduces the
project's first mid-expression CFG manipulation; the pattern is
intentionally reusable for future features (try/except inside an
expression will need the same shape).

| # | Bug | Fix location | Regression test |
|---|-----|--------------|-----------------|
| BUG-035 | `and` / `or` did not short-circuit — `Expr::Binary` eagerly lowered both operands and `emit_binop` dispatched to `IROp::IAnd`/`IOr` (bitwise). Guard idioms like `b > 0 and xs[b-1] > 0` trapped IndexError when the guard was false. | `compiler/src/ir.rs::lower_expr` (`Expr::Binary` arm now intercepts `And`/`Or` BEFORE lowering operands); new helper `compiler/src/ir.rs::lower_short_circuit` emits a basic-block split with a CondBranch on the lhs, evaluates the rhs only in the "continue-evaluating" successor, and phi-merges via a slot ReadLocal in the join block (same slot-based pattern as the M3.5 loop-carried-locals fix) | `vm/tests/m13_short_circuit.rs` — 6 tests: `and_short_circuits_when_lhs_is_false_protecting_index`, `or_short_circuits_when_lhs_is_true_protecting_index`, `and_both_true_returns_true`, `and_first_false_skips_division_by_zero_in_rhs`, `or_first_true_skips_division_by_zero_in_rhs`, `chained_and_all_eight_truth_combinations` |

### Notes for the next round

- The slot-based phi-merge worked transparently: no IR ops or VM
  opcodes needed to be added, and the existing `IROp::ReadLocal` /
  `WriteLocal` already cross basic-block boundaries (the same property
  that lets `while` loops carry locals across the back-edge). When
  try/except lands the catch handler will need the same shape:
  pre-write a sentinel into the result slot in the "normal" path,
  write the exception value in the catch handler, both predecessors
  flow into the join, ReadLocal in the join.
- The bitwise-approximation arm in `emit_binop` is preserved as a
  defensive backstop. No caller exercises it for user-visible code;
  removing it is a cleanup for a future agent.
- The B-tree workaround (nested `if` for the `b > 0 and ranks[b-1] >
  ranks[b]` insertion-sort condition) is now unnecessary but not
  actively wrong. Leaving it for a future stress-test refactor pass.

---

## Fixed in M16

Two language-gap closures in one round — `isinstance` and `match` / `case`.
Neither was a numbered bug; both were architectural gaps documented in the
M10–M12 agent reports as "language-surface awkwardness" (every sealed-
hierarchy stress program — `json_parse`, `lisp`, `lambda_calc`, `calculator`
— either rolled a `kind: i32` discriminator field or routed every operation
through virtual methods because there was no way to ask "is this thing
actually a `Pair`?" at runtime).

| # | Gap closed | Fix location | Regression test |
|---|------------|--------------|-----------------|
| M16-A | `isinstance(x, T)` was the stub `op_is_instance` from M3 that always returned `true` (`vm/src/interp.rs:1281` pre-M16). | (1) New `IROp::IsInstance { class_id: u32 }` in `compiler/src/ir.rs` (lowers to existing `Opcode::IsInstance` with a real type-table id). (2) `compiler/src/typecheck.rs::synth_call` recognises `isinstance` as a 2-arg builtin whose second arg names a class. (3) `compiler/src/typecheck.rs::narrowings_from_cond` extended to narrow `x` to `T` inside `if isinstance(x, T):` then-branches, mirroring the pre-existing `is not none` narrowing pattern. (4) `compiler/src/ir.rs::lower_call` short-circuits `isinstance(x, T)` before the generic arg-lower so the type-name argument isn't accidentally compiled as a value-expression. (5) `vm/src/interp.rs::op_is_instance` now reads the object's `ObjectHeader.vtable -> RuntimeType.type_id` and walks the parent chain via `shared.module.types[*].base_type` until it finds the target or hits `NO_BASE_TYPE`. | `vm/tests/m16_match_isinstance.rs` (4 isinstance tests + 5 match tests). |
| M16-B | `match v: case Pair(car, cdr): ...` parsed end-to-end since M3 but `compiler/src/ir.rs::Stmt::Match` was an empty M4 placeholder (commented `// M4`). Programs got no diagnostic — the arm body simply never ran. | (1) New `compiler/src/ir.rs::lower_match` — evaluates the scrutinee exactly once into a hidden local slot, then emits each arm as an if-elif test reading from that slot. Constructor patterns emit `IsInstance` against the class's type-table id then bind each Identifier sub-pattern via `Load { offset }` against the resolved field offset. Tuple patterns destructure unconditionally at `8 * i` offsets (the typechecker has already verified arity). (2) `compiler/src/typecheck.rs::Stmt::Match` now narrows the scrutinee per-arm and binds sub-pattern identifiers to the correct field/element types. (3) Exhaustiveness warning: a sealed-class match with missing variants and no wildcard prints a warning to stderr but does not fail typecheck (v0.1 — spec §6.5 documents the gap). | Same suite. |

### Notes for the next round

- **`isinstance` for protocols / primitives / generics** is not supported.
  `isinstance(x, Hashable)`, `isinstance(x, i32)`, `isinstance(x, List[i32])`
  all currently error out with "second argument must name a user class".
  Protocol membership in particular needs the itable walk (analogous to
  the vtable walk already done in `op_is_instance`).
- **Narrowing through `and` / `or`** isn't there. `if isinstance(x, A) and x.field > 0:`
  does NOT see `x: A` in the right operand of the `and`. The short-circuit
  CFG path landed in M13 didn't thread narrowing into the right-operand
  block. A future round can adapt `narrowings_from_cond` to also annotate
  the short-circuit join's intermediate block.
- **Nested constructor patterns** (`case Pair(Number(n), c):`) are NOT
  supported in v0.1. The lowerer only recognises `Identifier` and
  `Wildcard` as sub-patterns of `Constructor`/`Tuple`. Adding nested
  constructor patterns is a recursion in `lower_match` — straightforward
  but didn't fit in v0.1 scope.
- **Exhaustiveness is a *warning*, not an error.** A real algebraic-datatype
  pass would track every sealed-class hierarchy in the program and verify
  every variant appears (or a wildcard is present) — that's spec §6.5's
  promised behaviour. The current implementation only catches the
  "missing direct subclass" case for sealed types.
- **Or-patterns / guard clauses / range patterns / mapping patterns** are
  all explicitly deferred. The parser already accepts `case Pat if cond:`
  (the guard field on `MatchArm`); the IR lowerer currently ignores it.
- **The example rewrite** is `examples/calculator_with_match.spy` (NEW,
  129 lines, runs through `compiler/tests/calculator_with_match_runs.rs`).
  The pre-M16 `examples/calculator.spy` (249 lines) is left untouched as
  the workaround baseline. The match-form AST + evaluator is 73 source
  lines vs the virtual-method form's ~79 lines for the same surface — the
  win is qualitative more than quantitative on this particular program,
  because the original used virtual dispatch rather than the worse
  `kind: i32` discriminator pattern. The big payoff is for the sealed-
  hierarchy programs (`json_parse.spy`, `lisp.spy`, `lambda_calc.spy`)
  whose discriminator workarounds CAN now be deleted; per the brief, we
  do not migrate them in this round.

---

## Fixed in M17

One language-gap closure: **user-code generic free functions**. Every
M10–M16 agent report flagged "rewrite-per-type friction" — `partition` /
`quicksort` / `min_heap` / `linked_list` had to be hand-rolled per element
type because the AST already carried `GenericParam` on `FuncDecl` but the
resolver bound `T` to `Ty::Never`, the typechecker rejected calls (no
substitution mechanism), and the IR lowered each function exactly once.

| # | Gap closed | Fix location | Regression test |
|---|------------|--------------|-----------------|
| M17 | `fn id[T](x: T) -> T: return x` declared, called, and inferred at the call site; per-instantiation IR lowering with mangled FuncIds; transitive monomorphisation across generic-to-generic calls. | (1) `compiler/src/resolver.rs::build_function_sig` — each generic param gets a fresh `TypeVarId` recorded on the `FunctionSig`; the seed type-symbol carries `Ty::Var(tv)` instead of `Ty::Never`. (2) `compiler/src/typecheck.rs::check_generic_call` — interleaved synth-then-unify per arg, with `subst_ty` applied to each expected param type so already-solved vars switch to `check_expr` (giving int-literal width inference). Unification failures surface as `E2001`. (3) `compiler/src/typecheck.rs::check_binary` — `Ty::Var` operands typecheck as deferred (instantiation-specific). (4) `compiler/src/ir.rs::Lowerer::run` — Pass 2.6 seeds the worklist from fully-concrete typechecker instantiations; Pass 3 skips generic templates; Pass 3.5 drains the worklist via `lower_func_instantiation`, which applies a `Ty::Var(id) -> concrete` substitution at every `expr_ty(span)` and at every param/return type. (5) `compiler/src/ir.rs::lower_call` — generic callee dispatch: rebuild the substitution from the current call's argument types (already substituted by the enclosing body's subst), mangle, look up or mint a `FuncId`, emit `DirectCall`. (6) New mangle scheme in `compiler/src/typecheck.rs::mangle_args_key` (deterministic, debuggable, e.g. `quicksort__list_i64_i64_i64`). | `vm/tests/m17_generics.rs` (8 tests covering identity over 5 widths, multi-param tuple projection, `List[T]` arg, transitive monomorphisation, inference failure, instantiation-specific `+` over i32/str, tuple swap, generic over user class), and `compiler/tests/quicksort_generic_runs.rs` (the load-bearing demo). |

M17 closed: generic free functions with call-site monomorphisation.
Generic classes deferred to v0.2.

### Notes for the next round

- **Generic classes (`class Box[T]:`)** are deferred. The parser accepts the
  syntax and the resolver assigns a `ClassLayout.generics` field, but field-
  typed references to `T` aren't substituted at instantiation time. Closing
  this requires the same lazy-mono worklist treatment applied at the class-
  layout layer: one type-table entry per `(class_sym, type_args)` with
  substituted field types, and call sites to constructors must dispatch by
  type-args.

- **Bounds (`T: Comparable`)** are deferred. The parser accepts the syntax;
  the resolver ignores it. A future bounds system can either: (a) compile
  the bound to a deferred constraint that the per-instantiation typecheck
  resolves to a concrete method-set, or (b) restrict the body's operations
  to those declared by the bound's protocol, with no per-instantiation
  re-check. Option (b) is more conservative and probably the right v0.2
  target.

- **Auto-inference from return-type context** (`let x: i64 = id(0)`) is
  deferred. The call-site loop currently uses `synth_expr` whenever the
  expected param type contains an unbound var, so a bare `0` defaults to
  i32. Plumbing the let-binding's expected type into the generic-call
  inference would let `id(0)` solve `T := i64` from outside. Spec §10.4
  promises this; v0.1 doesn't deliver it.

- **Calls to generic methods on non-generic classes** (`class Foo: fn cast[T](self) -> T`)
  are NOT yet implemented. `synth_method_call` would need the same
  `check_generic_call` path; this round only wired free functions.

- **Quicksort over `List[str]`** doesn't work — `<` on `str` traps in the
  VM (no `StrLt` native). That's a pre-existing M9 limitation surfaced by
  the M17 demo, not a regression. A small follow-up can add `StrLt`/`StrLe`
  natives; until then the demo uses `List[i64]` and `List[f64]`.

- **One incidental bug found during M17 development**: the
  `TypedModule::instantiations` field was declared `HashSet<(SymbolId,
  Vec<Ty>)>` since (apparently) an earlier scaffolding pass — but `Ty`
  doesn't derive `Hash`/`Eq`, so the field was never populated and the
  declaration compiled only because no one inserted into it. Replaced
  with `Vec<(SymbolId, Vec<Ty>)>` + a parallel `HashSet<(SymbolId,
  String)>` keyed by `mangle_args_key` for dedup. No user-visible
  symptom; cleanup of half-built scaffolding.

- **Demo LOC**: `examples/quicksort_generic.spy` is 55 lines including a
  long docstring header (38 lines of code). It sorts both `List[i64]` and
  `List[f64]` from the same `quicksort[T]` / `partition[T]` bodies. The
  pre-M17 `examples/quicksort.spy` (35 lines, i64 only) is kept untouched
  per the brief. A hand-rolled two-type baseline would be ~65 lines of
  code (two copies of partition + quicksort + main) — so the generic
  version is roughly 40% shorter at *two* element types and the savings
  scale linearly per added type.

---

## Fixed in M18 (round 4 stress test)

Four parallel C-agents (R1 algorithms_lib, R2 json_parse_v2, R3 expr_interp,
R4 graph_lib) wrote ~1500 lines of code exercising the M13-M17 surface in
combination. Three found zero new bugs. R3 found exactly one — fixed inline.

| # | Bug | Fix location | Regression test |
|---|-----|--------------|-----------------|
| BUG-036 | `except ZeroDivisionError` did not catch `1/0` — runtime emitted legacy `DivisionByZeroError` and arm-match was exact-string. Spec/runtime drift. | `vm/src/interp.rs` (4 divzero emit sites changed to canonical `ZeroDivisionError`; new `exception_name_alias` maps legacy `DivisionByZeroError` filter to canonical for backward-compat) | `vm/tests/m18_divzero_alias.rs::except_zero_division_error_catches_canonical_name`, `..._legacy_name_still_catches`, `divzero_in_i32_path_emits_canonical_name` |

### Notes for the next round

- After M18 the only deferred bug is **BUG-028** (no implicit line
  continuation across infix `+`). That's the smallest remaining language
  gap; a focused agent can land it in <1 hour.
- M18's "absence of new bugs" finding (R1, R2, R4 all clean) is itself
  load-bearing thesis material. Stress-test ROI has flattened sharply
  since M11: M10 found 17 bugs / round, M11 found 6, M12 found 2, M18
  found 1. The language is settling.
- M18 R3's other probes (saved under `docs/thesis/m18_round/probes/`)
  document v0.1 limits the user can grep for: isinstance flow-narrowing
  doesn't compose through `and`; nested constructor patterns don't bind
  inner identifiers; match-scrutinee-throws propagates correctly; `raise
  e` re-raise from a caught variable works despite being out-of-scope in
  §7.5.6 (could be promoted to spec).

---

## Fixed in M19-M20-M21 (stdlib sprint)

Five milestones shipped the import system + Phase 1 stdlib. Eight modules
(sys / os / path / io / time / random / math / json / re — note path is
sibling to os not submodule), 70+ NativeFn variants. The integration
example `examples/minigrep.spy` exercises sys + os + io + re + time +
try/except + tuples in one program.

One incidental bug found and fixed during the sprint:

| # | Bug | Fix location | Regression test |
|---|-----|--------------|-----------------|
| BUG-037 | `x ?? fallback` always returned `fallback` — IR lowering for `Expr::NullCoalesce` was a placeholder that lowered both operands then emitted `IROp::Copy(rhs)` only. Same shape as BUG-008 (`is not` inverted) and BUG-034 (`str !=` always true): placeholder lowering for a binary operator that silently shipped the wrong value. | `compiler/src/ir.rs::lower_expr` Expr::NullCoalesce arm rewritten to mirror M13 `lower_short_circuit`: pre-seed result slot with lhs, test `RefEq(lhs, none)`, branch, evaluate rhs only in the "lhs was none" block, slot-based phi merge. Short-circuit semantics: rhs runs only when lhs is none. | `vm/tests/m21_null_coalesce.rs` (6 tests: returns lhs when not-none, returns rhs when none, doesn't evaluate rhs when lhs not-none, does evaluate rhs when lhs is none, works for str, chained `a ?? b ?? c`) |

### Notes for the next round

- After M21, **the only deferred bug is still BUG-028** (no implicit line
  continuation across infix `+`). One bug at the start of the stdlib
  sprint; one bug at the end. The Phase 1 stdlib bulk-add did not
  destabilise the language.
- The "placeholder IR lowering" pattern has now been hit three times
  (BUG-008, BUG-034, BUG-037). All three were silent miscompiles
  surfaced by stress-test programs organically using the operator.
  Mechanical lesson: audit `compiler/src/ir.rs` for `// placeholder`
  comments and Copy/passthrough lowerings for operators.

---

## Fixed post-M63 (bare-function-reference miscompile)

Found while chasing an "asyncio runtime crash" that turned out to predate
every suspect change: `target/release/spy scratch/apitest/az_c.spy`
(minimal `asyncio.run_unit(w)` with a bare top-level function name) died
instantly with 0xC0000005 / SIGSEGV and no output, on every branch tested
— including the merge-base before the string-perf work it was blamed on.

| # | Bug | Fix location | Regression test |
|---|-----|--------------|-----------------|
| BUG-041 | Passing a **bare module-scope function name** as a value (`asyncio.run_unit(w)`, `asyncio.spawn_i32(t)`, `map(double, xs)`, `f: fn() -> i32 = worker`) compiled but crashed with an access violation at runtime. Fourth instance of the "placeholder IR lowering" pattern (after BUG-008/034/037): `lower_expr`'s `Expr::Ident` arm had no case for module-scope functions, so the name fell through to the `IRConst::None` placeholder, which codegens to `ConstNone` = `NONE_SENTINEL` (`0x8000_0000_0000_0000`). The asyncio/threading/callback natives null-check the closure arg but `NONE_SENTINEL != 0`, so `extract_closure_target` dereferenced the sentinel as a `ClosureRepr` → instant AV before any output. Lambda-wrapped targets (`fn() -> i32: w()`) always worked, which is why every existing test passed while the LANGUAGE_GUIDE's own `asyncio.spawn_i32(do_work)` form crashed. | `compiler/src/ir.rs::lower_expr` Ident arm: a name found in `fn_id_by_name` now lowers to `ClosureNew { fn_id, n_captures: 0 }`, so every fn-typed value is uniformly a `ClosureRepr` pointer. Defense-in-depth: `vm/src/builtins.rs::extract_closure_target`, `interp.rs::call_callable`, and `interp.rs::op_closure_call` now reject `NONE_SENTINEL` / unaligned bit patterns with a catchable `TypeError` instead of dereferencing. | `vm/tests/m32_asyncio.rs::run_unit_accepts_bare_function_reference`, `::spawn_i32_accepts_bare_function_reference`, `::map_accepts_bare_function_reference`; end-to-end: `scratch/apitest/az_c.spy`, `scratch/apitest/az_d.spy`, `comprehensive_bench/v2/programs/sys_async_tasks.spy` (output byte-identical to its `.py` twin: `done=1000` / `sum=461500`) |

### Notes for the next round

- Generic functions referenced bare (no call) still lower to the
  placeholder — they have no single `FuncId` to wrap. They now raise the
  catchable `TypeError` instead of access-violating; proper support
  would need an instantiation-at-reference-site rule.
- This is placeholder-lowering instance #4. The BUG-037 lesson stands:
  audit `ir.rs` fallthroughs that silently produce `IRConst::None`.

---

## Fixed: producer.spy deadlock — try_recv early exit + blocking bounded send

BUG-044. `vm/tests/run_examples.rs::producer_runs` hung intermittently
(~50% of parallel-mode runs locally; reproduced on both ubuntu and
windows CI runners, each stalling until the job timeout). The example's
consumer polled `try_recv()` and broke on `none` — but `none` means
"empty" as well as "closed" (the documented M5 limitation), so under
CPU contention the consumer could poll between sends and exit early.
With the consumer gone, the producer filled the 16-slot bounded channel
and blocked forever in `send()` (the receiver half lives in the shared
channel table and is never dropped), so `t1.join()` never returned:
three threads parked on futexes. The test tolerated the *truncated
output* ("accept any prefix ≥ 10") but not this second-order hang.

Fixed by switching the consumer to the race-free drain protocol the
LANGUAGE_GUIDE documents: blocking `recv()` + `except
ChannelClosedError`. `ChannelTryRecv`'s semantics are unchanged (the
`channel_try_recv_empty_returns_none_sentinel` unit test pins them);
the empty/closed ambiguity remains a known wart for user code — see the
kvstore.spy header for the SHUTDOWN-sentinel alternative.
`producer_runs` now asserts the full 100-value drain.

---

## Fixed: silent no-op `del d[k]` (dict deletion unimplemented)

`del d[k]` on a `Dict[str, V]` parsed and type-checked but lowered to
**nothing** — `compiler/src/ir.rs` had `Stmt::Del { .. } => Some(())`, so
after `del d[k]` both `len(d)` and `d.get(k)` were unchanged. There was
also no `d.remove()` / `d.pop()` alternative, so the language had no way
to remove a dict key at all (the LRU-cache benchmark had to fake eviction
with two-generation segmented maps). Fifth instance of the
"placeholder IR lowering" pattern (after BUG-008/034/037/041), and the
worst form of it: a statement the spec grammar includes (§7.5 `del_stmt`)
that compiled to a no-op with no diagnostic.

| # | Bug | Fix location | Regression test |
|---|-----|--------------|-----------------|
| BUG-043 | `del d[k]` silently lowered to nothing; no dict-key removal existed anywhere in the language. | New `NativeFn::DictRemove = 1200` (`shared/src/native.rs`) implemented in `vm/src/builtins.rs` (removes the key from the side-table slot, returns 1/0 for present/absent — `len` reads the same side table so it stays consistent). `compiler/src/ir.rs` lowers `Stmt::Del` on a Dict-index target to it and dispatches the new `d.remove(k) -> bool` method via `resolve_native_method`; `compiler/src/typecheck.rs` checks the key against `K`, adds the `remove` synth entry, and **rejects every other `del` target** (plain names, list indices, attributes) with a type error instead of compiling a no-op. | `vm/tests/dict_remove.rs` (del removes entry / absent-key no-op / `remove` presence bool / re-insert after del / non-dict `del` targets are compile errors); unit: `vm/src/builtins.rs::tests::dict_remove_*` |

---

## Deferred: BUG-042 — subprocess.kill during wait() can never land

Surfaced by the BUG-041 verification run, which was the first full-suite
run in a long time with `target/release/spy` actually present:
`compiler/tests/job_scheduler_runs.rs` probes **silently skip** when the
release binary is missing (`compile_and_run` returns `None`), so
`probe_4_kill_from_other_thread` has been green-by-skip, not green.

- **Repro:** build `target/release/spy`, then
  `cargo test --release -p strictpy-compiler --test job_scheduler_runs probe_4`.
  Fails identically on clean `origin/main` (3825315) and on the BUG-041
  branch: `wait()` blocks the full 60s, exit code 0,
  `nonzero-exit=false` / `elapsed-under-5s=false`.
- **Cause:** `NativeFn::SubprocessWait` calls `subprocess_table_take`,
  which **removes** the `Child` from `SUBPROCESS_TABLE` before blocking
  in `child.wait()`. The killer thread's `subprocess.kill(h)` 200ms
  later finds no handle and raises IOError in the worker; the kill
  never reaches the OS process. Deterministic, not a flake.
- **Fix sketch:** keep the entry in the table during wait (store the
  raw pid / process handle alongside, or wrap the Child in
  `Arc<Mutex<...>>` and wait via `try_wait` polling or an OS-level
  waitid on the pid), so `kill` can signal a process that another
  thread is currently waiting on. Also worth making the probe tests
  fail loudly (not skip) when the spy binary is absent, so CI actually
  exercises them.
