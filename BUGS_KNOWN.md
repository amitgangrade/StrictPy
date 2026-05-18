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

## 4. VM heap corruption in JSON program (non-deterministic)

### Symptom
A program that allocates ~6 nested `Parser`/`JsonAtom` objects sometimes
crashes with `STATUS_HEAP_CORRUPTION` on Windows during teardown. Whether
it crashes depends on:
  - the declaration order of unrelated subclasses,
  - the declaration order of unrelated free functions
    (C2's probe 63: inserting a no-op function between two siblings
    toggled the crash).

The M11 round produced a *deterministic* sibling of this (N2: subclass
with class-ref fields + virtual call crashes on the first call) which
turned out to be BUG-016 (subclass field offsets aliased the vtable
pointer at offset 0). Fixing BUG-016 fixed N2 deterministically.

**Post-M11 verification (2026-05-18)**: ran `examples/calculator.spy`
and `examples/json_parse.spy` 5 times each after the M11 class-system
fixes. Both completed cleanly all 5/5 runs (previously calculator was
0-of-3 clean, json_parse 0-of-3). **Strong empirical evidence that
BUG-026 was a manifestation of BUG-016** — subclass field aliasing
overwrote the vtable pointer; the non-determinism was the heap layout
varying across runs; the underlying trigger was always the same
offset-aliasing. This bug is **provisionally closed**, pending a real
torture test (e.g. running each example 100 times in CI) to upgrade to
"confirmed fixed".

### Speculation
Likely caused by the GC walking objects in the M9 `in_jit`-paused heap,
or holding stale references after the JIT releases a module. The fact
that source-position changes flip the outcome points at a function-table
indexing bug (see #5). With BUG-016 (the deterministic sibling) fixed,
re-run the JSON / calculator programs in a loop to see whether anything
non-deterministic remains.

### Fix sketch
Repro reliably first (try the JSON parser plus a deterministic seed).
Audit `vm/src/gc.rs` for code paths that read object headers while the
JIT is mid-compile, and `vm/src/jit.rs` for dangling pointers into the
released module. Consider adding a `--gc-debug-poison` flag that fills
freed slots with `0xDEADBEEF` so use-after-free shows up loudly.

---

## 5. Position-sensitive crash from function ordering

### Symptom
Reordering function declarations in the source toggles whether the
program crashes (see C2's probe 63). The crash mirrors #4, so this is
probably the same root cause exposed via a different surface.

### Speculation
Some part of the function table is indexed by source position (e.g.
line/col span, AST node id derived from source order) where it should be
indexed by a stable identifier (FuncId). When the order changes, the key
collides differently.

The class_id → type_id alias bug fixed in M11 also flipped under
declaration-order changes (Pentagon as 4th vs 5th subclass), so a chunk
of this symptom may already be gone. The fact that re-running calculator
and json_parse is still non-deterministic should be verified.

### Fix sketch
Find every `Span`-keyed `HashMap` / `BTreeMap` in `compiler/src/`
(`grep -rn "HashMap<Span"`). Each is a candidate. Replace with a stable
id (`FuncId`, `ClassId`, etc.) and re-run the failing JSON program in a
loop to confirm determinism.

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
