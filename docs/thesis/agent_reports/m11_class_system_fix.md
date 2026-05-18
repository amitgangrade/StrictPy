# M11 fix pass — class-system overhaul + primitive ctor dispatch

**Brief**: fix the class-system cluster (BUGS_KNOWN §1/2/3 + new N1/N2)
plus the primitive-ctor dispatch bug surfaced by C5.

**Wall-clock**: ~41 minutes
**Tool uses**: 147
**Files modified**: `compiler/src/resolver.rs`, `compiler/src/ir.rs`,
`compiler/src/types.rs`, `shared/src/native.rs`, `vm/src/builtins.rs`,
`BUGS_KNOWN.md`, `STRICTPY_SPEC.md`. New: `vm/tests/m11_fixes.rs` (478
lines, 13 regression tests).

## Bugs closed

### BUG-015 — sealed dispatch drops to base method
- **Fix**: `compiler/src/ir.rs::lower_method_call` line ~1979 — devirt
  guard now requires `!is_open && !is_sealed`.
- **Test**: `sealed_base_dispatches_to_subclass_override`.

### BUG-016 — subclass field offsets alias parent's last field
- **Fix**: (a) added `payload_size: u32` to `ClassLayout` in
  `compiler/src/types.rs`. (b) `resolver.rs::layout_class` seeds offset
  cursor from `parent.payload_size` and inherits parent's `fields`
  verbatim. (c) IR pass-2 uses `payload_size` (padded to 8-byte words)
  for type-table `size`.
- **Tests**: `subclass_field_offsets_do_not_alias_parent_fields`,
  `subclass_with_three_inherited_fields_does_not_alias`.

### BUG-017 + N1 — vtable "mod 4 / cap 4"
The root cause was **THREE adjacent bugs**, not one:

1. **Subclass vtable didn't inherit parent methods** (`resolver.rs`).
   Sub vtables started empty; un-overridden inherited methods had
   `u32::MAX` in their slots. Fix: seed sub's `methods` from parent
   (skipping `__init__`); overrides replace entries in-place; slot
   indices stay stable across the chain.

2. **IR didn't walk up the chain for inherited fn_ids** (`ir.rs` pass-2).
   When resolving a method call's target, the IR only checked the
   receiver's own class. Fix: walk `cur → cur.base` looking up
   `Class.method` until found.

3. **The op_new class_id vs type_id collision** — the most interesting
   bug of M11. The M3-era VM `op_new` had a fallback: if the operand
   didn't index a known `type_id` directly, try indexing the type table
   by it as if it were a class_id. This worked while type_ids and
   class_ids never overlapped. **The 4th user class arrives with
   `class_id = 16`, which numerically equals Shape's `type_id = 16`**
   (type_ids start at 16 after the ~12 prelude classes). The "direct
   lookup" silently returned Shape's RuntimeType for the new class's
   `NEW`. Symptom: 4th sibling subclass got Shape's vtable, dispatch
   went through Shape's methods, and the "vtable mod 4" appearance was
   really a class-id collision making Pentagon look like Shape.

   **Fix**: `ir.rs::lower_call` now emits the runtime `type_id` directly
   on `Alloc` (from the `class_type_id` map) instead of the resolver's
   `class_id`.

- **Tests**: `vtable_supports_six_virtual_methods_with_override`,
  `subclass_can_inherit_method_without_override`,
  `natural_class_hierarchy_with_parent_fields_and_six_virtuals`.

### N2 — heap corruption on Pair + virtual call
- **Verified**: this was BUG-016 in disguise. With subclass fields
  aliasing offset 0, `Pair.car` overwrote the vtable pointer at
  `(obj + 0)`. First `p.tag()` loaded `Pair.car` (a heap pointer to a
  Number) as if it were a `RuntimeType*` and dereferenced `vtable[0]`
  from random memory.
- After fixing BUG-016, N2's repro runs cleanly.
- **Test**: `pair_with_class_ref_fields_dispatches_through_vtable`.

### Primitive ctor dispatch (i32/i64/f64/char)
- **Fix**: `compiler/src/ir.rs::lower_call` `SymbolKind::PrimType` arm
  now does per-arg-type dispatch (mirrors M10's `str(x)` fix). New
  `NativeFn::I64FromF64 = 29` added.
- **Tests**: `i32_of_i64_truncates_value`, `i64_of_i32_widens_value`,
  `f64_of_i64_widens_value`, `i64_of_f64_truncates_toward_zero`.

### str(f64) — already correct
- `vm/src/builtins.rs::format_f64` already emits `"3.0"` via `:.1` for
  integer-valued floats. No code change; added spec doc.

## Provisionally closed (post-M11 verification)

### BUG-026 — non-deterministic heap corruption in json_parse/calculator
**Post-fix observation**: ran `examples/calculator.spy` and
`examples/json_parse.spy` 5 times each after the M11 class-system fixes.
**Both completed cleanly all 5/5 runs**, where pre-M11 they were 0/3 and
0/3 clean respectively.

**Strong empirical evidence that BUG-026 was a manifestation of BUG-016.**
The non-determinism was the heap layout varying across runs; the
underlying trigger was always the same offset-aliasing causing the GC
to walk through a corrupted vtable pointer.

Provisionally closed pending a torture test (running each example 100×
in CI) to upgrade to "confirmed fixed."

### BUG-027 — position-sensitive crash from function ordering
Same root cause as BUG-026; same provisional closure.

## Still deferred to next round

- **BUG-028** — no implicit line continuation across `+`. Separate
  lexer enhancement.
- **Cleanup**: the VM's `op_new` fallback (`module.types.get(operand)`)
  is still there. Could be removed once the IR is verified to always
  emit `type_id`.

## Final test totals

Pre-M11: 189 passing. Post-M11: **201 passing, 0 failed, 1 ignored**.
+12 from `vm/tests/m11_fixes.rs`.

## Files modified

| File | Δ lines |
|---|---|
| `compiler/src/resolver.rs` | ~50 |
| `compiler/src/ir.rs` | ~150 |
| `compiler/src/types.rs` | +5 |
| `shared/src/native.rs` | +6 |
| `vm/src/builtins.rs` | +5 |
| `vm/tests/m11_fixes.rs` (NEW) | 478 |
| `BUGS_KNOWN.md` | rewritten — §1/2/3 closed, "Fixed in M11" section added |
| `STRICTPY_SPEC.md` | +13 (§9.1 doc on str(x) float convention + prim-ctor dispatch) |

## Why this milestone was so consequential

Per the agent's note: "The `op_new` class_id-vs-type_id collision was a
long-standing M3-era hack ('compiler emits class_id, VM falls back to
indexing type table by class_id') that worked only because class_ids and
type_ids never collided in the small examples. Once the 4th user class
arrived, its class_id (16) numerically matched Shape's type_id (16). This
is exactly the kind of latent bug stress-testing is designed to surface."

The class-system overhaul also closed the last load-bearing correctness
gap in the language. Programs using natural OO patterns — sealed
hierarchies, deep inheritance, fields on every class, virtual dispatch
on the base — now work correctly. The "tagged-union via kind: int"
workaround that every M10/M11 program had to reach for is no longer
required.
