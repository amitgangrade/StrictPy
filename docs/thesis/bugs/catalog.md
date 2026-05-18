# Bug catalog

Every distinct bug discovered during the project. Severity, milestone
discovered, milestone fixed (or "deferred" with pointer to
[BUGS_KNOWN.md](../../../BUGS_KNOWN.md)).

## Summary by category

| Category | Found | Fixed | Deferred |
|---|---:|---:|---:|
| Silent miscompile (codegen) | 7 | 7 | 0 |
| Vacuous output (IR lowering punt) | 3 | 3 | 0 |
| Vtable / inheritance | 7 | 7 | 0 |
| Typechecker rejects valid code | 3 | 3 | 0 |
| Frontend operator semantics | 2 | 2 | 0 |
| Runtime memory / GC | 2 | 2 | 0 |
| Stdlib missing | 5 | 5 | 0 |
| Parser / lexer | 1 | 0 | 1 |
| Formatting / spec consistency | 1 | 1 | 0 |
| **Total** | **31** | **30** | **1** |

Post-M12 state (2026-05-18):

- BUG-026 / BUG-027 (non-deterministic heap corruption + position-
  sensitive crash) are now **CONFIRMED FIXED**. M12's torture test
  (`compiler/tests/heap_corruption_torture.rs`) ran 250 consecutive
  invocations of the canonical repros (100× calculator + 100× json_parse
  + 50× lisp) with zero failures. Pre-M11 these were 0/3 clean.
- BUG-034 (M12 find: `str != str` always true — silent miscompile of
  the same shape as BUG-008 `is not`) was fixed inline in M12.

Post-M13 state (2026-05-18):

- BUG-035 (`and` / `or` short-circuit) is now **fixed**. M13's targeted
  agent landed `compiler/src/ir.rs::lower_short_circuit`, the project's
  first mid-expression CFG manipulation. The standard guard idiom
  (`b > 0 and xs[b-1] > 0`) now evaluates correctly without trapping
  `IndexError`. Regression coverage at `vm/tests/m13_short_circuit.rs`
  (6 tests including BOTH value-semantics truth-table cases AND
  trap-on-rhs cases — the latter is what the old bitwise lowering
  would have failed).

Truly-deferred bugs after M13 are:
- BUG-025: no fallible `open()` (needs exception handling).
- BUG-028: no implicit line continuation across infix operators
  (needs lexer enhancement).

Post-M15 state (2026-05-18):

- BUG-025 (no fallible `open()`) is now **fixed**. M15 landed try/
  except/finally + `raise` codegen, the synthetic 2-field exception
  heap object (`type_name: str`, `message: str`), and the interpreter-
  side handler-frame propagation pass that turns existing native
  `VmError::UncaughtException` traps into catchable user-visible
  exceptions. The BUG-025 acceptance test
  (`vm/tests/m15_try_except.rs::open_of_missing_file_is_catchable_as_io_error`
  + `examples/safe_open.spy` + `compiler/tests/safe_open_runs.rs`)
  confirms `open("missing", "r")` no longer aborts the program. The
  Cranelift JIT carve-out is implicit: `vm/src/decompile.rs::decode_function`
  rejects any function containing `Throw / EnterTry / LeaveTry /
  Rethrow` opcodes, so functions using try/raise fall back to the
  interpreter automatically.
- Only BUG-028 (lexer line continuation) remains deferred.

Post-M16 state (2026-05-18 later):

- **M16 closed two long-standing language gaps**: `isinstance(x, T)`
  was stubbed-true from M3 (`vm/src/interp.rs::op_is_instance` returned
  1 regardless), and `match` / `case Constructor(...)` parsed end-to-end
  but the IR lowerer dropped every arm (`compiler/src/ir.rs::Stmt::Match`
  was an M4 placeholder). Together these forced every sealed-hierarchy
  stress program (`json_parse`, `lisp`, `lambda_calc`, `calculator`) to
  either roll a `kind: i32` discriminator field or route every operation
  through a virtual method on an `open class` base. Neither was a numbered
  bug; both were architectural gaps documented in the M10-M12 agent
  reports as "language-surface awkwardness".
- Implementation: `IROp::IsInstance { class_id }` lowers to the existing
  `Opcode::IsInstance` opcode (now reading object header → vtable →
  `type_id` and walking `module.types[*].base_type` until match);
  `lower_match` evaluates the scrutinee into a hidden local slot and
  emits each arm as an isinstance-guarded basic-block test with field
  destructuring via the resolver's `ClassLayout.fields[i].offset`. Flow
  narrowing for `if isinstance(x, T):` mirrors the existing `is not none`
  narrowing in `narrowings_from_cond`.
- Carve-outs: only `Identifier` and `Wildcard` sub-patterns in
  `Constructor`/`Tuple` patterns are supported (nested constructor
  patterns deferred); `isinstance` accepts only user classes (not
  protocols, primitives, or generics); narrowing does NOT compose
  through `and`/`or`; exhaustiveness is a stderr warning, not an error.
- Tests: `vm/tests/m16_match_isinstance.rs` (9 tests),
  `compiler/tests/calculator_with_match_runs.rs` (2 tests),
  `examples/calculator_with_match.spy` (NEW — sealed-class AST + match
  evaluator demo).
- BUG-028 (lexer line continuation) is still the only deferred bug.

## Full catalog

### Critical: silent miscompiles

#### BUG-001 — Nullable-narrowing not unwrapped in binop dispatch
- **Found**: M10 (CSV aggregator stress test)
- **Symptom**: `prev: f64? = agg.get(k); if prev is none: ... else: agg[k] = prev + x` silently performed integer add over raw f64 bit patterns. Every float aggregation returned garbage that `str(f64)` printed as `0.0`.
- **Location**: `compiler/src/ir.rs::lower_binop` — checked `Ty::Primitive(p) if p.is_float()` without first unwrapping `Ty::Nullable(F64)`.
- **Fix**: unwrap `Ty::Nullable(inner)` before the float check. Six-line patch.
- **Status**: fixed in M10.

#### BUG-002 through BUG-005 — Nullable-narrowing audit siblings
- **Found**: M10 (AB agent's audit, triggered by BUG-001)
- **Location**: `compiler/src/codegen.rs`:
  - F32/F64 dispatch (would silently pick F64 for `Nullable(F32)`)
  - `int_cmp_op` (wrong width for nullable int compares)
  - `op_for_iadd/isub/imul/idiv` typed-bytecode dispatch
  - `ty_tag_for` field-store dispatch
  - `emit_op` operand_ty/ty in general
- **Symptom**: same shape as BUG-001 — narrowed nullable slots silently dispatch to wrong typed opcode.
- **Fix**: each site adds `unwrap_nullable_ty()` before the primitive check. 60 lines total + 5 regression tests.
- **Status**: all fixed in M10.

#### BUG-006 — `not x` was emitting bitwise NOT
- **Found**: M7
- **Symptom**: `not 1` returned `0xFFFFFFFE` — truthy. Every `if not …:` in every program was silently running the wrong branch.
- **Location**: codegen's `IROp::BoolNot` mapped to `INotI32` (bitwise) instead of `(x == 0)`.
- **Fix**: emit `ConstI32(0) + IEqI32`.
- **Status**: fixed in M7.

#### BUG-007 — `none` was stored as bit pattern `0`
- **Found**: M7 (while implementing runtime-class dispatch)
- **Symptom**: `op_const_none` loaded literal `0` into the register. `if v is none:` then matched zero-valued integers and zero-byte pointers — both indistinguishable from the actual none sentinel.
- **Location**: VM's `op_const_none` opcode handler.
- **Fix**: load `NONE_SENTINEL` (high-bit-set u64) — the constant M5 had defined for `try_recv` and `dict.get` return values.
- **Status**: fixed in M7.

#### BUG-008 — `is not none` is INVERTED
- **Found**: M10 (C2 agent's JSON parser stress test)
- **Symptom**: `x: i32? = 42; if x is not none: println("A") else: println("B")` prints "B". The condition is inverted at the IR level. Most severe correctness bug found in the project — easy to write, silently wrong, every program using `is not none` is broken.
- **Why no test caught it earlier**: every example was hand-written using `if x is none: ... else: ...` workarounds.
- **Location**: `compiler/src/ir.rs::emit_binop` `IsNot` arm.
- **Fix**: emit logical NOT of the equality check, not just the equality.
- **Status**: fixed in M10.

### Critical: vacuous output

#### BUG-009 — Loop-carried locals not updated across loop back-edges
- **Found**: M4
- **Symptom**: `while i < n: ...; i = i + 1` runs the body once with the initial `i`, then loops forever reading the same initial value. `fib.spy` infinite-looped.
- **Location**: M3's IR builder used a `locals` map but the loop-header condition read captured the ORIGINAL SSA value at lowering time, not the post-write value.
- **Fix (M3.5)**: introduced `ReadLocal { slot }` / `WriteLocal { slot }` IR ops with stable per-local slot indices. Each read re-fetches from the slot.
- **Status**: fixed in M3.5.

#### BUG-010 — List literals not populated
- **Found**: M4
- **Symptom**: `a: List[f64] = [1.0, 2.0, 3.0]` produced an empty list. `dot.spy` returned 0.
- **Location**: IR lowerer for `Expr::List` emitted `ListNew` but not the per-element `ListPush`.
- **Fix (M3.5)**: emit `ListNew + ListPush*N`.
- **Status**: fixed in M3.5.

#### BUG-011 — Top-level `final` const declarations not lowered
- **Found**: M4
- **Symptom**: `final WIDTH: i32 = 60` resolved to 0 at runtime. `mandelbrot.spy` printed nothing because outer loops never entered.
- **Location**: IR lowerer had no handler for `TopDecl::Const`.
- **Fix (M3.5)**: constant-fold consts inline at use sites (literal-init only).
- **Status**: fixed in M3.5.

### Critical: vtable / inheritance

#### BUG-012 — Duplicate `self` param overwriting slot 0
- **Found**: M6-A (debugging tree.spy regression from M3.5)
- **Symptom**: methods on user classes saw `self` as a Unit-typed null pointer; field stores went to wrong addresses.
- **Location**: parser already records `self` as `f.params[0]`, but IR was prepending ANOTHER implicit `self` whose type was `Infer` → `Unit`.
- **Fix**: skip `f.params[0]` in IR when receiver is already present.
- **Status**: fixed in M6.

#### BUG-013 — Eager devirtualization on `open` classes
- **Found**: M6-A
- **Symptom**: virtual methods on subclasses of `open` classes were never reached — calls were lowered to direct calls on the base.
- **Location**: `lower_method_call` devirtualized any class whose layout was visible.
- **Fix**: only devirtualize when `!layout.is_open`. Open classes must go through vtable to see subclass overrides.
- **Status**: fixed in M6.

#### BUG-014 — `__init__` consuming vtable slot 0
- **Found**: M6-A
- **Symptom**: subclasses with overridden methods got the wrong dispatch — `Leaf.sum()` called `Leaf.__init__()` instead.
- **Location**: `__init__` was being kept in `ClassLayout.methods` for typecheck-arity reasons, but it also took vtable slot 0, shifting every virtual method by one.
- **Fix**: skip `__init__` both when building the vtable and when computing dispatch slots. Keep it in `methods` for typecheck.
- **Status**: fixed in M6.

#### BUG-015 — Sealed-class dispatch drops to base method
- **Found**: M10 (C2 stress test)
- **Symptom**: `b: SealedBase = SealedSub(); b.name()` returns base's value, not subclass override. Same code with `open class SealedBase` works.
- **Root cause**: the devirt branch in `lower_method_call` required only `!is_open` — sealed classes fell through and were direct-called to base.
- **Fix**: `compiler/src/ir.rs::lower_method_call` now requires `!is_open && !is_sealed` for devirt. Sealed classes dispatch through vtable like open ones.
- **Status**: fixed in M11. Test: `sealed_base_dispatches_to_subclass_override`.

#### BUG-016 — Subclass field offsets alias parent's last field
- **Found**: M10 (C2 stress test)
- **Symptom**: `Sub(Base) { n: i32 }` where `Base { kind: i32 }` ends up with `sub.n` and `sub.kind` at the same offset.
- **Root cause**: `resolver.rs::layout_class` started the offset cursor at `header_size` (16) regardless of parent — subclass fields overlapped parent fields.
- **Fix**: added `payload_size: u32` to `ClassLayout`. `layout_class` now seeds the offset cursor from `parent.payload_size` and inherits the parent's `fields` verbatim. IR pass-2 uses `payload_size` (padded to 8-byte words) for the type-table `size`.
- **Status**: fixed in M11. **High severity** — was corrupting state on every multi-field subclass. Tests: `subclass_field_offsets_do_not_alias_parent_fields`, `subclass_with_three_inherited_fields_does_not_alias`.

#### BUG-017 — Vtable index wraps mod 4 (and N1: vtable cap at 4 slots)
- **Found**: M10 (C2 stress test) — original symptom: 4th sibling override goes to base. **Sharpened in M11** (C6 lisp interpreter) — actually a hard cap of 4 total slots on the base class.
- **Root cause**: NOT a `& 0x3` mask. Was **three converging adjacent bugs**:
  1. Subclass vtables didn't inherit parent method slots — un-overridden inherited methods had `u32::MAX` in their slots.
  2. IR didn't walk up the inheritance chain when resolving inherited fn_ids.
  3. **`op_new` class_id vs type_id collision** (see BUG-029). Long-standing M3-era hack where if `op_new`'s operand didn't match a known type_id directly, it fell back to indexing the type table as if it were a class_id. Worked silently for 10 milestones until the 4th user class arrived with `class_id 16` colliding with Shape's `type_id 16`.
- **Fix**: each of the three sub-bugs fixed individually in M11. IR now emits runtime `type_id` (not resolver `class_id`) on `Alloc`.
- **Status**: fixed in M11. Tests: `vtable_supports_six_virtual_methods_with_override`, `subclass_can_inherit_method_without_override`, `natural_class_hierarchy_with_parent_fields_and_six_virtuals`.

### Medium: typechecker rejects valid code

#### BUG-018 — `char(i32)` rejected with E2011
- **Found**: M10 (C3 Brainfuck)
- **Symptom**: `char(72)` fails typecheck even though `NativeFn::CharFromI32 = 23` exists, is dispatched, and the VM implements it.
- **Location**: `typecheck.rs::synth_call` numeric-ctor allow-list omits `char`.
- **Fix**: add `char` to the allow-list.
- **Status**: fixed in M10.

#### BUG-019 — `str(c: char)` returns codepoint as decimal
- **Found**: M10 (C3 + C2 both reported)
- **Symptom**: `str('h')` prints `"104"` instead of `"h"`.
- **Location**: IR lowerer routes every `str(x)` to `NativeFn::StrFromAny` which falls back to integer formatting. `NativeFn::StrFromChar = 10` exists but is unreachable.
- **Fix**: dispatch by arg type in `lower_call`.
- **Status**: fixed in M10.

#### BUG-020 — `dict.has(k)` rejected with E2004
- **Found**: M10 (C3)
- **Symptom**: `d.has(k)` fails typecheck even though `NativeFn::DictHas` exists and is implemented.
- **Location**: `synth_method_call` for Dict didn't list `has`.
- **Fix**: add Dict.has entry.
- **Status**: fixed in M10.

### Medium: stdlib missing

#### BUG-021 — No `list.pop()`
- **Found**: M10 (C3 Brainfuck — was building jump tables and wanted to remove the last entry)
- **Status**: fixed in M10. Added `NativeFn::ListPop = 107`.

#### BUG-022 — No `str.split(sep)`
- **Found**: M10 (every string-handling program hand-wrote a splitter — wordcount, csv_aggregate, markov)
- **Status**: fixed in M10. Added `NativeFn::StrSplit = 28`.

#### BUG-023 — No `sorted()` / `list.sort()`
- **Found**: M10 (csv_aggregate had to reimplement Lomuto quicksort for List[str] because the existing `quicksort.spy` is List[i64]-only and there are no generics in user code)
- **Status**: fixed in M10. Added `NativeFn::ListSort = 105`, `ListSorted = 106` for i64/f64/str.

#### BUG-024 — No `for x in xs:` loops
- **Found**: M10 (every example written so far used `while i < len(xs): … i += 1` manually)
- **Status**: fixed in M10. IR-level desugaring to indexed while-loop for List[T] receivers.

#### BUG-025 — No fallible `open()` ✅ FIXED IN M15
- **Found**: M10 (C3 KV store)
- **Symptom**: missing file at startup traps; no `Result[File, IOError]` return; can't try/except for file-not-found.
- **Root cause**: The runtime had been returning `VmError::UncaughtException { type_name: "IOError", ... }` from native `open(missing)` since M5 (see `vm/src/builtins.rs:373`). What was missing was the user-visible plumbing: (a) `Stmt::Raise` and `Stmt::Try` IR lowerings were stubs that dropped handlers and traversed all bodies in source order; (b) the `Throw/EnterTry/LeaveTry/Rethrow` opcodes in `shared/src/opcode.rs` were reserved but unimplemented in `vm/src/interp.rs::step`; (c) exception class layouts in `compiler/src/resolver.rs` had empty `fields`, so `e.message` / `e.type_name` had nothing to read.
- **Fix (M15)**: Wired the full pipeline.
  1. **Resolver**: every built-in exception class now carries `type_name: str` (offset 0) + `message: str` (offset 8) fields and `payload_size=16`. Added `ZeroDivisionError`, `AssertionError`, `RuntimeError`, `ChannelClosedError` to the recognised set.
  2. **Typechecker**: `Stmt::Raise { exc: Call(Ident(ExcName), [msg]) }` now validates `msg` against `str` directly, bypassing the normal class-constructor path (built-in exceptions have no `__init__`).
  3. **IR**: new `IROp::TryEnter { arms, finally_block }`, `IROp::TryLeave`, `IROp::EndFinally`. `lower_try` emits TryEnter at the start of the body, TryLeave at the end of the body, Branch through finally to the merge block, plus orphan blocks (entered only via VM exception dispatch) for each handler arm and the finally block. `lower_raise` allocates a 2-field heap exception object using the existing `IROp::Alloc` + two `IROp::Store` ops, then emits `Terminator::Throw`.
  4. **Codegen**: encodes EnterTry as `[opcode, finally_pc:i32, n_arms:u8, (filter_str_idx:u32, handler_pc:i32, bind_reg:u16)*]`. Handler / finally block ids are registered with the existing branch-`patches` table so the finish() pass resolves them to byte offsets.
  5. **Interpreter**: `Interpreter.handler_frames: Vec<HandlerFrame>` is the per-thread handler stack; `pending_exception: Option<(String, String)>` carries an in-flight exception through a finally block. `run_until` wraps each `step` call in a catch — on `Err(VmError::UncaughtException)`, `propagate_exception` walks `handler_frames` top-down looking for a matching arm (filter == `"Exception"` or filter == `type_name`); on match it pops the requisite call frames + handler frames, materialises the exception heap object only if the arm has a bind register, and jumps to the handler pc. On no-match but with a finally, it stashes the pending exception and jumps to the finally pc; `EndFinally` (`Opcode::Rethrow`) re-raises if pending is set.
- **Status**: fixed in M15. Tests: `vm/tests/m15_try_except.rs` (10 tests) + `compiler/tests/safe_open_runs.rs` (2 tests) + `examples/safe_open.spy` (the BUG-025 demo).

### Medium: runtime memory

#### BUG-026 — Non-deterministic VM heap corruption (JSON program) — confirmed fixed in M12
- **Found**: M10 (C2 JSON parser)
- **Symptom**: STATUS_HEAP_CORRUPTION on Windows during teardown of programs with ~6 nested heap allocations. Crash is intermittent. Depends on:
  - Subclass declaration order in source (reordering classes changes crash behavior)
  - Function declaration order (probe 63: adding a free function between two unrelated functions toggles the crash)
- **Sharpened in M11**: C4 calculator agent confirmed the crash can happen BEFORE the first println reaches the OS — not just at teardown. C6 lisp agent found N2 (BUG-030), the *deterministic* sibling.
- **Root cause**: same as BUG-016. Subclass field aliasing overwrites the vtable pointer at offset 0; the GC then walks corrupted pointers. Non-determinism is heap layout varying across runs; trigger is always the same offset-aliasing.
- **Verification (M11, provisional)**: ran `examples/calculator.spy` and `examples/json_parse.spy` 5 times each after the M11 BUG-016 fix. Both completed cleanly all 5/5 runs, where pre-M11 they were 0/3 each.
- **Verification (M12, confirmed)**: `compiler/tests/heap_corruption_torture.rs` ran 100× calculator + 100× json_parse + 50× lisp — **250/250 clean**, zero crashes, ~3.12s total. Pre-M11 these were 0/3.
- **Status**: confirmed fixed in M12.

#### BUG-027 — Position-sensitive crash from function ordering — confirmed fixed in M12
- **Found**: M10 (C2 — same bisect as BUG-026)
- **Symptom**: defining an unrelated `fn parse_num(x: i32) -> i32: return 0` between two other functions toggles whether the program crashes.
- **Root cause**: same as BUG-026 — the M3-era `op_new` class_id ↔ type_id collision (BUG-029) also flips under declaration-order changes. Pentagon as 4th vs 5th subclass triggered different fallback resolutions.
- **Status**: confirmed fixed in M12 by the same torture test as BUG-026.

### Frontend semantics

#### BUG-028 — No implicit line continuation across trailing `+` ⚠️ DEFERRED
- **Found**: M10 (C2 Markov)
- **Symptom**: `return "a " +\n    "b"` errors with E0001. Forces accumulator-style string building.
- **Status**: deferred. See `BUGS_KNOWN.md §6`. Lexer enhancement, mechanically simple but separate.

### M11-only finds

#### BUG-029 — `op_new` class_id vs type_id collision
- **Found**: M11 (class-system fix agent, debugging BUG-017 symptom)
- **Symptom**: 4th user class allocated through the wrong RuntimeType — e.g. `Pentagon()` was constructed using `Shape`'s vtable. Manifested as the "vtable wraps mod 4" appearance of BUG-017, plus heap corruption (BUG-026/027) when the wrong-vtable instance was later used.
- **Root cause**: M3-era hack in VM `op_new` — if the operand didn't index a known `type_id` directly, fall back to indexing the type table by it as if it were a `class_id`. The fallback worked silently for 10 milestones because `type_id` and `class_id` numeric ranges never overlapped. Once 12+ prelude classes + 4 user classes were registered, the 4th user class's `class_id` (16) numerically collided with Shape's `type_id` (16). The fallback returned the WRONG type.
- **Fix**: `compiler/src/ir.rs::lower_call` now emits the runtime `type_id` directly on `Alloc` instead of the resolver's `class_id`. (The VM fallback is still in place as defense-in-depth but no caller exercises it any longer.)
- **Status**: fixed in M11. This was THE most informative bug of the project — a latent compatibility hack from M3 that took 10 milestones of accumulated state to trigger. Direct lesson for the thesis: "this is exactly the kind of bug stress-testing is designed to surface" (M11 agent's words).

#### BUG-030 — Deterministic heap corruption on subclass-with-class-ref-fields + virtual call (was "N2")
- **Found**: M11 (C6 lisp interpreter)
- **Symptom**: `final class Pair(Value) { car: Value }` then `p: Value = Pair(...)` then `p.tag()` → access violation. Number-typed values work; only Pair (with class-ref fields) crashes.
- **Root cause**: BUG-016 in disguise. Subclass field offsets aliased the vtable pointer at offset 0; `Pair.car` overwrote the vtable pointer. First `p.tag()` loaded `Pair.car` (a heap pointer to a Number) as if it were a `RuntimeType*` and dereferenced random memory.
- **Fix**: subsumed by BUG-016 fix in M11. After fixing BUG-016, BUG-030's repro runs cleanly.
- **Status**: fixed in M11. **The deterministic sibling that enabled BUG-026's root-cause analysis** — without the C6 agent finding this, BUG-026 would still be a non-deterministic mystery.
- **Test**: `pair_with_class_ref_fields_dispatches_through_vtable`.

#### BUG-031 — Primitive ctors `i32`/`i64`/`f64`/`char` ignore arg type
- **Found**: M11 (C5 Levenshtein)
- **Symptom**: `j: i64 = 3; k: i32 = i32(j)` silently writes 0, not 3. Levenshtein on `""→"abc"` returned 0 instead of 3.
- **Root cause**: same architectural pattern as M10's BUG-019 (str(char)). `NativeFn::from_name("i32")` returns `I32FromF64` unconditionally; IR's `lower_call` only does per-arg-type dispatch for `str(x)`. So `i32(i64_var)` dispatches to `I32FromF64`, which reinterprets the i64 bit pattern as f64 — small ints look like denormal f64s and truncate to 0.
- **Fix**: `compiler/src/ir.rs::lower_call` `SymbolKind::PrimType` arm now mirrors `str(x)`'s per-arg dispatch for all primitive ctors. New `NativeFn::I64FromF64 = 29` added.
- **Status**: fixed in M11. Severity: medium-high — `len()` returns i64, so any list-length-to-i32 conversion was silently zeroing. Tests: `i32_of_i64_truncates_value`, `i64_of_i32_widens_value`, etc.

#### BUG-032 — `str(f64)` of whole number prints `"3.0"` not `"3"` — false alarm
- **Found**: M11 (C4 calculator)
- **Symptom**: `str(3.0)` prints `"3.0"`. CSV aggregator's comment claimed "shortest round-trip"; users may expect bare integer form for integer-valued floats.
- **Resolution**: not actually a bug. `vm/src/builtins.rs::format_f64` already emits `"3.0"` via `:.1` formatting — that IS the shortest round-trip form ("3" would be ambiguous between i64 and f64 if round-tripped through `parse_f64`). Updated spec §9.1 to document the convention explicitly.
- **Status**: documented, no code change needed.

#### BUG-033 — Vtable cap at 4 total slots on the base class (was "N1")
- **Found**: M11 (C6 lisp interpreter)
- **Symptom**: `open class Value` with 6 virtual methods → calling slot ≥4 traps "vtable slot N out of range" at runtime.
- **Root cause**: same as BUG-017 sub-bug (a) — subclass vtables didn't inherit parent method slots, so slots ≥4 were unallocated.
- **Fix**: subsumed by BUG-017 fix in M11.
- **Status**: fixed in M11. Tests: `vtable_supports_six_virtual_methods_with_override`.

### M12-only finds

#### BUG-034 — `str != str` always returns true
- **Found**: M12 (C9 B-tree stress test — every search-result FAIL line printed even when the value matched).
- **Symptom**: For two `str` operands `a, b`, `a != b` always evaluates to `true`, including when `a == b` also evaluates to `true`. Programs using `s != ""` as a "missing" sentinel silently went through the missing branch every time. Programs using positive `==` form were unaffected; this is why it took until M12 to surface.
- **Root cause**: `compiler/src/ir.rs::emit_binop` had a `StrEq` branch for `Eq` (added when strings became first-class) but no matching branch for `Ne`. `Ne` on str fell through to `INe`, which compared the two heap-pointer u64s — distinct allocations always have distinct pointers, so `INe` returned `true` for every string compare. Exact same shape as BUG-008 (`is not` had been emitting `RefEq` not `not RefEq`).
- **Fix**: lower `AstBinOp::Ne` on str operands as `StrEq` followed by `BoolNot`, mirroring the `IsNot` precedent. ~4 lines.
- **Status**: fixed in M12. Test: `compiler/tests/btree_runs.rs::str_ne_returns_false_for_equal_strings`. Minimal repro preserved at `examples/_probe_str_ne.spy`.
- **Severity**: medium. Every program comparing strings for inequality was wrong. Manifested in M12 only because btree was the first stress test that built fail/pass output lines from a string compare; calculator/lisp/json_parse all used numeric or virtual-method discrimination.

#### BUG-035 — `and` / `or` do not short-circuit
- **Found**: M12 (C9 B-tree stress test — `while b > 0 and ranks[b-1] > ranks[b]` traps with `IndexError: index -1` after one iteration of an insertion sort).
- **Symptom**: `a and b` and `a or b` evaluate both operands unconditionally. Programs that use the standard guard idiom (`if bounds_ok and xs[i] > ...`) trap when the right operand is only safe under the left guard.
- **Root cause**: `compiler/src/ir.rs::emit_binop` lowered `AstBinOp::And` to `IROp::IAnd` (bitwise) and `AstBinOp::Or` to `IROp::IOr`. The source comment was honest: "bitwise approximation". Load-bearing-correct for pure-operand uses, broke for guarded indexing.
- **Fix**: `compiler/src/ir.rs::lower_expr` (`Expr::Binary` arm) now intercepts `And`/`Or` BEFORE eagerly lowering both operands and routes them to the new `lower_short_circuit` helper. The helper allocates a result slot, lowers the lhs in the current block, pre-writes the lhs into the slot, emits a `CondBranch` (rhs-block on the "continue-evaluating" side, merge on the "short-circuit" side), evaluates the rhs in its own block and overwrites the slot, then branches to merge. The merge block reads the slot. Phi-merge uses the same slot-based ReadLocal/WriteLocal pattern as the M3.5 loop-carried-locals fix — no new IR ops or VM opcodes needed. **First mid-expression CFG manipulation in the project**; pattern is reusable for future features (try/except inside an expression will use the same shape).
- **Status**: fixed in M13. Tests: `vm/tests/m13_short_circuit.rs` (6 tests including BOTH value-semantics truth-table cases AND trap-on-rhs cases — `1 / 0` in the rhs of `false and (...)` / `true or (...)` must NOT execute. The trap cases are the load-bearing assertions; pure value-only tests would have passed under the old bitwise lowering too).

## Lessons from the catalog

1. **The biggest clusters of bugs were each found by ONE real-world
   program and audited up.** Without CSV aggregator, BUG-001 through
   BUG-005 (nullable-narrowing dispatch siblings) would still be silent.
   Without the C6 lisp interpreter, the class-system overhaul (BUG-015/
   016/017 + BUG-029/030/033) would still be deferred. Stress testing
   has superlinear return on investment.

2. **No bug was found by the type checker rejecting bad source.** Every
   single bug was a problem with what the toolchain produced/executed, not
   what it accepted at the source level.

3. **`exits 0` is not a test.** BUG-009/010/011 all "passed" the M4
   integration tests because the tests only checked exit code. Every test
   brief since then has required value-level assertions.

4. **Bugs cluster.** BUG-001 → audit → BUG-002 through 005.
   BUG-012 → tree.spy regression → BUG-013 and 014. BUG-017
   ("vtable mod 4" symptom) turned out to be THREE adjacent bugs (subclass
   vtable inheritance + IR chain walk + the `op_new` class_id/type_id
   collision in BUG-029). Pattern: when you find one, look hard for
   siblings — the root cause is often several bugs collapsed onto one
   visible symptom.

5. **Deterministic siblings unlock non-deterministic mysteries.** BUG-026
   was a non-deterministic heap corruption that defied easy debugging for
   a full milestone. M11's C6 agent found BUG-030 — the *deterministic*
   sibling. Reducing BUG-030 to a minimal repro exposed BUG-016 as the
   underlying cause, which then explained BUG-026 too. **Without the
   deterministic sibling, the fix would have been guesswork against
   shifting symptoms.**

6. **Latent bugs accumulate dose-dependently.** BUG-029 (the `op_new`
   class_id/type_id collision) was an M3-era hack that worked silently
   for 10 milestones because nothing in those milestones exercised it.
   It only triggered once the 4th user class arrived (class_id 16) AND
   that number happened to collide with a prelude type_id. Both
   conditions were needed; both came together in M10/M11 stress tests.

7. **Deferred ≠ unimportant.** BUG-016 (subclass field aliasing) and
   BUG-017 (vtable mod-4) were marked deferred in M10 because their fixes
   looked architectural. When M11 finally fixed them, they collateral-
   fixed BUG-026 and BUG-027 (the non-deterministic heap corruption that
   had been the project's worst-classified bug). **The marginal cost of
   leaving a load-bearing correctness bug "deferred" is paid by every
   subsequent program that has to work around it OR that hits its
   manifestations.**

8. **Confirmation results are valuable.** The M12 round added 3 stress
   programs (regex, dijkstra, btree). Two found zero bugs — and that
   was the headline. Pre-M11, every class-heavy program (json_parse,
   calculator, lisp) was a bug catalogue with extensive workaround
   sections. The regex agent's report — "sealed hierarchy with 8
   subclasses, 6 virtual methods, class-ref subclass fields, ran first
   try without a single workaround" — is empirical evidence that the
   M11 class-system overhaul actually landed. **Stress tests without
   workarounds are themselves a quantitative measurement of language
   maturity.**

9. **Torture tests are how provisional fixes become confirmed fixes.**
   BUG-026 and BUG-027 were marked "provisionally closed" at end-of-M11
   with the empirical evidence 5/5 clean runs. The M12 torture test
   (`heap_corruption_torture.rs`) ran 250 sequential invocations across
   three canonical repros in 3.12s — and produced zero failures. The
   marginal cost of "provisional → confirmed" was ~20 minutes of agent
   time and ~5 lines of regression-test wall-clock per CI run. That
   trade is almost always worth making.

10. **Silent miscompiles can hide behind positive-form code conventions.**
    BUG-034 (`str != str` always true) sat in the codebase from the
    moment strings became first-class, but no program tripped it until
    M12 because every prior example happened to use `==` for string
    compares (positive form). The B-tree was the first program that
    built `FAIL got=X want=Y` output via inequality compare, which
    rendered the bug visible. Same shape as BUG-008 (`is not` was
    `RefEq` not `not RefEq`) — both stayed hidden until a program
    organically used the negative form. Lesson: write tests that
    exercise BOTH equality forms for any new comparison operator.
