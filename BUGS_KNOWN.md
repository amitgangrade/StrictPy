# Known Bugs (Deferred)

This file collects bugs surfaced by the parallel real-world program stress
test (Game of Life, Sudoku, JSON parser, Markov chain, KV store, Brainfuck)
that are too architectural to fix in the same pass that landed the
straightforward fixes. Each entry has a short repro, the symptom, a
location speculation, and a fix sketch.

The "trivial" cousins of these bugs (`is not none` inversion, `str(char)`,
`char(i32)`, `dict.has`, `list.pop`) are already fixed; see
`vm/tests/real_world_fixes.rs` for the regressions.

---

## 1. Sealed-class virtual dispatch drops to the base method

### Repro
```python
sealed class SealedBase:
    fn name(self) -> str:
        return "base"

final class SealedSub(SealedBase):
    fn name(self) -> str:
        return "sub"

fn main() -> i32:
    b: SealedBase = SealedSub()
    println(b.name())        # prints "base"; should print "sub"
    return 0
```
Replacing `sealed` with `open` makes the same program print `"sub"`.

### Symptom
Virtual calls on a `sealed`-typed receiver always reach the *base*
implementation, even when the runtime type is a subclass that overrides
the method. The receiver's runtime vtable is bypassed entirely.

### Speculation
The IR `lower_method_call` path (`compiler/src/ir.rs`, around the
`VirtualCall` emission near line 1987) appears to treat `sealed` as
"closed-world devirtualisable to base", so it picks the base method's
function id at compile time. This is wrong: `sealed` only restricts where
subclasses may *be defined*, not what method body actually runs.

### Fix sketch
For `sealed` receivers, still emit `VirtualCall { vtable_slot }` exactly
like for `open` — UNLESS the resolver can statically prove the receiver's
runtime type (e.g. flow analysis on a `final` ctor result). The current
`is_open` check at line 1979 should grow a `|| layout.is_sealed` clause,
or be inverted to `is_final`.

---

## 2. Subclass field offsets alias parent's last field

### Repro
```python
class Base:
    kind: i32

class Sub(Base):
    n: i32
    value: bool
    fn __init__(self, v: bool) -> None:
        self.kind = 200
        self.n = 7
        self.value = v

fn main() -> i32:
    s: Sub = Sub(true)
    println(str(s.kind))   # expected 200, prints 1 (overlapped by self.n=7? no — by self.value=true)
    return 0
```

### Symptom
When a subclass declares its own fields, the first subclass field starts
at offset 0 rather than after the parent's last field. The two slots
alias: writing the subclass field overwrites the parent's field.

### Speculation
`compiler/src/resolver.rs::layout_class` (the routine that computes
`ClassLayout.fields[*].offset`) restarts the offset counter at 0 for
subclasses instead of seeding it with `parent.size`.

### Fix sketch
When laying out a subclass, initialise the next-offset cursor to
`parent_layout.size_in_words` (or whatever the layout uses), then proceed.
Also update `ClassLayout.size` so further subclasses extend correctly.
Touches GC scanning (object size lookup) and `Opcode::New` (allocation
size), so verify those read the *whole-class* size, not the parent's.

---

## 3. Virtual dispatch table wraps modulo 4

### Repro
```python
open class Shape:
    fn name(self) -> str: return "shape"

final class A(Shape): fn name(self) -> str: return "A"
final class B(Shape): fn name(self) -> str: return "B"
final class C(Shape): fn name(self) -> str: return "C"
final class D(Shape): fn name(self) -> str: return "D"   # falls back to "shape"
final class E(Shape): fn name(self) -> str: return "E"   # prints "A"
```

### Symptom
With four or more sibling overrides of the same base method, dispatch
keys wrap: the 4th sibling resolves to the base, the 5th to the first
sibling, etc.

### Speculation
A vtable slot index somewhere is masked with `& 0x3` (or equivalently a
fixed 2-bit shift) instead of taking the full slot. Could be in
`Opcode::CallVirtual` decode in `vm/src/interp.rs`, or in
`Opcode::LoadVtable`, or in the codegen of `IROp::VirtualCall`.

### Fix sketch
Grep for `& 0x3`, `& 3`, `>> 2`, and any byte-packed encoding around
vtable slots. Replace with a full `u32` slot id. Make sure the type table
writer (`compiler/src/ir.rs::write_type_table`) and the runtime
`ObjectHeader.vtable` reader agree on the stride.

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

### Speculation
Likely caused by the GC walking objects in the M9 `in_jit`-paused heap,
or holding stale references after the JIT releases a module. The fact
that source-position changes flip the outcome points at a function-table
indexing bug (see #5).

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

## Reference: bugs fixed in the same pass

| # | Bug | Fix location | Regression test |
|---|-----|--------------|-----------------|
| 1 | `is not none` inverted | `compiler/src/ir.rs` (emit_binop, IsNot arm) | `vm/tests/real_world_fixes.rs::is_not_none_takes_correct_branch_when_value_is_some` |
| 2 | `str(char)` decimal codepoint | `compiler/src/ir.rs` (lower_call, str dispatch) | `vm/tests/real_world_fixes.rs::str_of_char_returns_single_codepoint_string` |
| 3 | `char(i32)` E2011 not callable | `compiler/src/typecheck.rs` (synth_call) | `vm/tests/real_world_fixes.rs::char_constructor_typechecks_and_produces_correct_codepoint` |
| 4 | `dict.has` E2004 no method | `compiler/src/typecheck.rs` (synth_method_call) | `vm/tests/real_world_fixes.rs::dict_has_typechecks_and_returns_bool` |
| 5 | `print` unreachable from source | already wired in `shared/src/native.rs::from_name` | (no-op — see commit notes) |
| 6 | `list.pop()` missing | `shared/src/native.rs`, `compiler/src/typecheck.rs`, `vm/src/builtins.rs` | `vm/tests/real_world_fixes.rs::list_pop_removes_and_returns_last_element` |
