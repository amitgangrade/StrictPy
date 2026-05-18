# Bug catalog

Every distinct bug discovered during the project. Severity, milestone
discovered, milestone fixed (or "deferred" with pointer to
[BUGS_KNOWN.md](../../../BUGS_KNOWN.md)).

## Summary by category

| Category | Found | Fixed | Deferred |
|---|---:|---:|---:|
| Silent miscompile (codegen) | 5 | 5 | 0 |
| Vacuous output (IR lowering punt) | 3 | 3 | 0 |
| Vtable / inheritance | 6 | 3 | 3 |
| Typechecker rejects valid code | 3 | 3 | 0 |
| Frontend operator semantics | 1 | 1 | 0 |
| Runtime memory / GC | 2 | 0 | 2 |
| Stdlib missing | 5 | 4 | 1 |
| Parser / lexer | 1 | 0 | 1 |
| **Total** | **26** | **19** | **7** |

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

#### BUG-015 — Sealed-class dispatch drops to base method ⚠️ DEFERRED
- **Found**: M10 (C2 stress test)
- **Symptom**: `b: SealedBase = SealedSub(); b.name()` returns base's value, not subclass override. Same code with `open class SealedBase` works.
- **Speculation**: `sealed` is being treated as "closed-world devirtualizable to base impl" — wrong. Sealed classes should still dispatch through vtable; the closed world only allows the typechecker to enumerate subclasses for exhaustiveness, not to skip dispatch.
- **Status**: deferred. See `BUGS_KNOWN.md §1`.

#### BUG-016 — Subclass field offsets alias parent's last field ⚠️ DEFERRED
- **Found**: M10 (C2 stress test)
- **Symptom**: `Sub(Base) { n: i32 }` where `Base { kind: i32 }` ends up with `sub.n` and `sub.kind` at the same offset.
- **Speculation**: `resolver.rs::layout_class` doesn't seed the offset cursor with the parent's size when laying out subclass fields.
- **Status**: deferred. See `BUGS_KNOWN.md §2`. **High severity** — corrupts state on every multi-field subclass.

#### BUG-017 — Vtable index wraps mod 4 ⚠️ DEFERRED
- **Found**: M10 (C2 stress test)
- **Symptom**: with ≥4 subclasses overriding the same base method, 4th sub → base impl, 5th → 1st sub, 6th → 2nd sub.
- **Speculation**: an `& 0x3` or equivalent mask somewhere in the vtable construction or lookup path.
- **Status**: deferred. See `BUGS_KNOWN.md §3`. Hard cap on inheritance depth — meaningful for real OO programs.

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

#### BUG-025 — No fallible `open()` ⚠️ DEFERRED
- **Found**: M10 (C3 KV store)
- **Symptom**: missing file at startup traps; no `Result[File, IOError]` return; can't try/except for file-not-found.
- **Status**: deferred. Needs exception handling (parser accepts try/except; codegen doesn't lower).

### Medium: runtime memory

#### BUG-026 — Non-deterministic VM heap corruption (JSON program) ⚠️ DEFERRED
- **Found**: M10 (C2 JSON parser)
- **Symptom**: STATUS_HEAP_CORRUPTION on Windows during teardown of programs with ~6 nested heap allocations. Crash is intermittent. Depends on:
  - Subclass declaration order in source (reordering classes changes crash behavior)
  - Function declaration order (probe 63: adding a free function between two unrelated functions toggles the crash)
- **Speculation**: M9's `in_jit` GC pause holds the heap pinned; teardown of the JIT module + GC walk over still-rooted objects + dropped JIT-compiled functions create a use-after-free.
- **Status**: deferred. See `BUGS_KNOWN.md §4`. **High severity** but hard to reduce further — non-deterministic, position-sensitive, likely needs deep VM debugging.

#### BUG-027 — Position-sensitive crash from function ordering ⚠️ DEFERRED
- **Found**: M10 (C2 — same bisect as BUG-026)
- **Symptom**: defining an unrelated `fn parse_num(x: i32) -> i32: return 0` between two other functions toggles whether the program crashes.
- **Speculation**: probably the same root cause as BUG-026 — function table indexed by source position somewhere it shouldn't be (a Span-keyed map?).
- **Status**: deferred. See `BUGS_KNOWN.md §5`.

### Frontend semantics

#### BUG-028 — No implicit line continuation across trailing `+` ⚠️ DEFERRED
- **Found**: M10 (C2 Markov)
- **Symptom**: `return "a " +\n    "b"` errors with E0001. Forces accumulator-style string building.
- **Status**: deferred. See `BUGS_KNOWN.md §6`. Lexer enhancement, mechanically simple but separate.

## Lessons from the catalog

1. **The biggest cluster of bugs (BUG-001 through BUG-005) was found by ONE
   real-world program and audited up.** Without CSV aggregator, the silent
   miscompile pattern would still be undetected. Stress testing has
   superlinear return on investment.

2. **No bug was found by the type checker rejecting bad source.** Every
   single bug was a problem with what the toolchain produced/executed, not
   what it accepted at the source level.

3. **`exits 0` is not a test.** BUG-009/010/011 all "passed" the M4
   integration tests because the tests only checked exit code. Every test
   brief since then has required value-level assertions.

4. **Bugs cluster.** BUG-001 → audit → BUG-002 through 005.
   BUG-012 → tree.spy regression → BUG-013 and 014.
   BUG-026 and 027 are almost certainly the same underlying issue.
   Pattern: when you find one, look hard for siblings.

5. **Deferred ≠ unimportant.** BUG-016 (subclass field aliasing) and
   BUG-017 (vtable mod-4) are both load-bearing correctness issues. They're
   deferred because the fixes are non-trivial, not because the bugs are
   tolerable. The risk of leaving these unfixed is that any future real
   program with serious class hierarchies will hit them.
