# M12 — In-memory B-tree (order 4)

**Brief**: B-tree with `insert(k, v)` and `search(k)`, stressing
`final class` + `List[BNode?]` + recursive method calls + node-splitting
that allocates and rewires class-ref slots.

**Wall-clock**: ~70 minutes
**Files added**:
- `examples/btree.spy` (506 lines)
- `examples/_probe_str_ne.spy` (54 lines — headline-bug minimal repro)
- `compiler/tests/btree_runs.rs` (128 lines, 3 tests + 1 documented-bug probe)

## Result

End-to-end, in-process via `run_file_capture`, exit 0:

```
INVARIANT_OK
OK: 5/5
```

The five sub-tests:
1. Insert (1,"one")..(5,"five"); search each — round-trip OK.
2. Insert 2 keys, search a missing key — returns `""` sentinel.
3. Insert same key twice — second value wins.
4. Insert 12 sequential keys 100..111 — forces root split + leaf
   splits; all round-trip; `node_count_keys == 12`; invariants hold.
5. Insert 25 keys in LCG-shuffled order (seed 1234567) — all
   round-trip; invariants hold (`INVARIANT_OK`).

Three back-to-back in-process runs produce bit-for-bit identical
stdout (`btree_runs_deterministically_three_times`). No
STATUS_HEAP_CORRUPTION, no non-determinism.

## NEW bugs discovered

### Bug A (HEADLINE) — `str != str` always returns true

**Repro** (`examples/_probe_str_ne.spy`):

```python
a: str = "hello"
b: str = "hello"
println(str(a == b))   # true   (correct)
println(str(a != b))   # true   (WRONG — should be false)
```

**Symptom**: `a != b` evaluates to `true` even when `a == b` also
evaluates to `true`. Confirmed across literal/literal, var/var,
`List[str]` indexing, and class field reads.

**Root cause** (high confidence): `compiler/src/ir.rs` line ~1716:
```rust
AstBinOp::Eq => {
    if is_str { IROp::StrEq } else if is_float { IROp::FEq } else { IROp::IEq }
}
AstBinOp::Ne => if is_float { IROp::FNe } else { IROp::INe },
                                               ^^^^^^^^^^^^
                                               no is_str branch
```
`StrEq` was added but `StrNe` wasn't. `Ne` on str falls through to
`INe`, which compares the two heap pointers as i64; every distinct
allocation has a distinct address, so `INe` always returns true.

**Fix sketch**: either add `IROp::StrNe` + VM dispatch mirroring
`StrEq` with negated result, or (smaller, ~4 lines) lower
`AstBinOp::Ne` on str operands as `StrEq` followed by `BoolNot`
— the same shape `IsNot` already uses (ir.rs:1722-1734).

**Workaround**: every str `a != b` in `btree.spy` is written
`not (a == b)`. `compiler/tests/btree_runs.rs::probe_str_ne_bug_repro`
is `#[ignore]`d but runs the probe and asserts the *current buggy*
output — un-ignore and flip the assertion when fixed.

### Bug B (secondary) — `and` / `or` do not short-circuit

**Symptom**: `while b > 0i32 and ranks[b - 1i32] > ranks[b]` traps
with `IndexError: index -1 out of range for length 25` after one
iteration. The right operand is evaluated unconditionally.

**Root cause** (high confidence): `compiler/src/ir.rs` line 1738:
```rust
AstBinOp::And => IROp::IAnd,    // bitwise approximation
AstBinOp::Or  => IROp::IOr,
```
Comment is honest. Both `and` and `or` lower to bitwise opcodes
that evaluate both operands.

**Fix sketch**: lower `a and b` to `if a: b else: false` at the AST
or IR level (basic-block split with conditional branch). Many
existing `and` uses pass *by accident* because the right operand
happens to be in-bounds even when the left guard is false; the bug
manifests whenever the right operand has a fault domain larger
than the guard.

**Workaround**: nested `if` (10 extra lines in the insertion sort).

## Confirmed BUGS_KNOWN entries

- **#4 / BUG-026** (non-deterministic heap corruption): 3 in-process
  runs gave identical output on the exact shape that BUG-016 was
  suspected to underlie (`final class` with `List[ClassRef?]`,
  recursive method calls, many fresh allocations). Confirms M11's
  provisional close.
- **#5 / BUG-027** (position-sensitive crash): did not reproduce.
- **#6 / BUG-028** (no line continuation across `+`): hit; used
  accumulator pattern.

## Language-surface awkwardness

- **Per-expression narrowing**: `unwrap_node` must be
  `if n is not none: return n; assert false; return BNode(true)` —
  the natural `if n is none: assert false; return n` is rejected
  (`E2001: expected class#N, got class#N?`). The unreachable trailing
  return placates the definite-return checker.
- **No tuples / multi-return**: `split_child` writes the median key,
  value, and new right node directly into the parent rather than
  returning them. Kept three `split_med_*` slots on `BNode` in case
  a method-form split is ever needed.
- **`str` with `""` sentinel** instead of `str?` for `search`,
  because per-expression narrowing makes `let v: str = t.search(k)`
  awkward when the return is `str?`.

## Class-system stress confirmation

This program exercises the exact patterns BUG-015 / 016 / 017 / N2 /
026 / 027 were about: a single `final class BNode` with 9 fields
(including `List[BNode?]` and 4 nullable slots), recursive
method-style calls (`BTree.search` → `node_search` → recurse),
splitting that allocates fresh `BNode` instances and rewires class-ref
slots on a parent, and ≥30 instances allocated during the shuffle
test. **All of it runs cleanly, deterministically, in-process.** M11
holds.

## Final test totals

`cargo test --workspace --release`: 44 test binaries, all green, 0
failures. New tests:

- `btree_compiles` — compile path
- `btree_passes_all_internal_tests` — asserts `INVARIANT_OK` + `OK: 5/5`
- `btree_runs_deterministically_three_times` — 3 in-process runs,
  identical stdout (locks in BUG-026 staying closed for this shape)
- `probe_str_ne_bug_repro` (ignored) — documents headline bug
