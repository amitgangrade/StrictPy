# M14 — tuples and tuple destructuring (multi-return)

**Brief**: every M10–M12 stress program flagged "no tuples / no
multi-return" as the single biggest language-surface awkwardness.
This milestone closes that gap with tuple literals, `Tuple[...]` type
annotations, `t.0`/`t.1` field access, destructuring `let` /
`assign`, return-position tuples, `==`/`!=` element-wise equality,
and `str(tuple)` formatting. Arity 2..=8.

**Wall-clock**: ~90 minutes
**Tests**: 212 baseline + 8 new M14 regression tests + 2 new
dijkstra-with-tuples integration tests = **222 passing, 0 failing**.

## Representation chosen: (a) heap-allocated tuple objects

`Ty::Tuple(Vec<Ty>)` already existed in the type system from day
one — it just had no lowering, so every tuple expression evaluated to
`Const(IRConst::None)` (an unceremonious null). The cheapest path to
real semantics was to **treat each distinct tuple shape as a synthetic
nominal class**: one type-table entry per `display_ty(Ty::Tuple(...))`
key, one 8-byte slot per element, no methods, `kind=3` in the type
table per the spec's preexisting reservation, GC traced as a normal
class (every 8-byte slot conservatively scanned, see
`vm/src/gc.rs::trace_object` `GcKind::Class` arm).

This reuses `IROp::Alloc { class_id }`, `IROp::Load { offset }`, and
`IROp::Store { offset }` verbatim. Zero new opcodes, zero new natives,
zero `vm/src/interp.rs` changes. The lift was entirely in the
compiler.

The alternative — flat slot-pack in the caller, no heap — would have
required ABI changes everywhere (function returns become N adjacent
registers, calling conventions differ for tuple vs scalar return,
codegen splits per tuple shape). For v0.1 this trade-off is clearly
wrong: the heap-object path lets a tuple be passed around as a
single 8-byte pointer, just like a class instance.

## Files modified

| file | one-line summary |
|------|------------------|
| `compiler/src/ast.rs` | added `Stmt::LetDestructure { names, tys, init, span }` (the Pattern enum was insufficient — `let` always binds a single name, so we extend Stmt instead of routing through Pattern). |
| `compiler/src/lexer.rs` | tracked `prev_was_postfix_terminator` flag so `.<digit>` lexes as `Dot IntLit` (tuple field) when preceded by an ident/RParen/RBracket/IntLit, but still as a `.5` float at the start of an expression. |
| `compiler/src/parser.rs` | (i) `t.0` parse: numeric literal after `.` becomes the attr name. (ii) annotated destructure `x: T1, y: T2 = expr`. (iii) unannotated destructure `x, y = expr` with snapshot-rollback fallback when the comma turns out to belong to an expression-tuple. |
| `compiler/src/resolver.rs` | normalised `Tuple[T1, T2, ...]` → `Ty::Tuple` (was `Ty::Generic{Tuple, args}` — split-brain). Added `LetDestructure` resolution: each name becomes its own Local symbol. |
| `compiler/src/typecheck.rs` | (i) `LetDestructure` synthesises RHS, verifies arity and per-name annotations element-wise. (ii) `attr_type` now matches numeric `name` on `Ty::Tuple` and returns `elems[idx]`. (iii) `Tuple[...]` written via `Expr::Index` also normalises to `Ty::Tuple`. |
| `compiler/src/ir.rs` | (i) new `register_tuple_types` pass scans `expr_types` + AST type table + class field/method types, emits one synthetic class-style `TypeTableEntry` per shape with `kind=3` and uniform 8-byte-per-elem layout. (ii) `Expr::Tuple` lowering: `Alloc(tid)` + N `Store(offset)`. (iii) `Expr::Attr` on tuple type: `Load(offset=8*idx)`. (iv) `Stmt::LetDestructure` lowers RHS once, emits N `Load`+`WriteLocal` pairs. (v) `lower_tuple_eq` helper for `==`/`!=` element-wise. (vi) `lower_str_of_tuple` + `str_of_value` helpers stringify by element-wise dispatch. (vii) one quirk: `Stmt::Assert`'s tuple-wrapped cond is unwrapped at IR time, mirroring the typechecker. |
| `compiler/src/pretty.rs` | print the new `Stmt::LetDestructure`. |
| `vm/tests/m14_tuples.rs` | NEW — 8 regression tests covering all six task acceptance cases plus the class-ref-element case. |
| `examples/dijkstra_with_tuples.spy` | NEW — Dijkstra rewrite where `MinHeap.pop_min` returns `Tuple[i32, f64]` and the main loop destructures it, eliminating the `visited[]` array workaround the original needed because `pop_min` could only return one value. |
| `compiler/tests/dijkstra_with_tuples_runs.rs` | NEW — integration test modelled on `dijkstra_runs.rs`. 4 PASS lines + summary check. |
| `STRICTPY_SPEC.md` | new §5.5 "Tuples (v0.1 — M14)" with the surface, the §5.2 covariance line already covered subtyping. §5.6 is the old "Forbidden constructs" renumbered. |

Unchanged: `vm/src/interp.rs`, `vm/src/builtins.rs`, `vm/src/gc.rs`,
`shared/src/opcode.rs`, `shared/src/native.rs`,
`compiler/src/codegen.rs`, `compiler/src/bytecode.rs`. The "no new
opcodes / no VM changes" property of representation (a) paid off.

## Non-obvious gotchas

**Lexer postfix `.<digit>` ambiguity.** The original lexer rule was
"`.` followed by digit = float literal" (for `.5`-style floats). That
clashes with `t.0`: the lexer would emit `Ident("t"), FloatLit(0.0)`
instead of `Ident("t"), Dot, IntLit(0)`. Fix: a 1-bit state flag,
`prev_was_postfix_terminator`, set by `next_token` when the just-emitted
token can be the receiver of a postfix `.`. When set, the
`.<digit>` rule is suppressed and the lexer emits `Dot` then descends
into the number lexer (which sees just `0` and produces `IntLit(0)`).
Cost: 5 lines of lexer state, zero impact on the float-literal grammar
in expression-leading positions.

**Resolver/typechecker normalisation split-brain.** `Tuple[i32, str]`
resolved to `Ty::Generic { TypeCtor::Tuple, args: [...] }` from
`resolver.rs`'s container path, but the typechecker's `Expr::Tuple`
arm produced `Ty::Tuple(...)`. Same shape, two encodings — anything
that needed to match on tuple-ness broke half the time. Fix: normalise
in BOTH places (`resolver.rs:lower_ast_type_with_class` and
`typecheck.rs:Expr::Index` instantiation) to always produce
`Ty::Tuple`, leaving `Ty::Generic { TypeCtor::Tuple, .. }` only as a
zero-args sentinel for the bare type-constructor symbol.

**Assert(cond, msg) was the only place that pre-existed `Expr::Tuple`
in healthy programs.** Every M10–M13 example uses
`assert(len(a) == len(b), "msg")` which parses as
`Stmt::Assert { cond: Expr::Tuple([Eq, Str]), msg: None }`. The
typechecker unwraps this case-by-case (`Stmt::Assert` arm at
`typecheck.rs:Stmt::Assert`), so the tuple type was NEVER stored in
`expr_types`. After M14, the IR's tuple lowering wanted to allocate
that wrapper tuple and crashed on `op_new` because the shape wasn't
registered (the scan only finds shapes in `expr_types`). Fix: mirror
the unwrap in IR's `Stmt::Assert` arm. Without this all 7
`run_examples` integration tests crashed with `STATUS_ACCESS_VIOLATION`.

**Codegen `Store` writes 8 bytes as `TypeTag::Ref` regardless of field
type.** This was pre-existing behaviour for class fields with i32/i16
elements and works because the GC scans 8-byte chunks anyway. For
tuples it means an `i32` element written at offset 0 occupies 8 bytes
(high 4 are zero-extension), and the load at the same offset reads
only the low 4 bytes via the dest type's tag. So mixed-arity tuples
just work — no codegen change needed.

## Workaround patterns now eliminated

**Before (M12 `dijkstra.spy`, `MinHeap.pop_min`)**:
```python
fn pop_min(self) -> i32:
    """Returns ONLY the vertex id; caller looks up priority by re-indexing dist[]."""
    ...
```
And the caller, which now needs a `visited[]` boolean array because
it can't compare popped priority against current dist:
```python
visited: List[bool] = []
while not pq.is_empty():
    u: i32 = pq.pop_min()
    if visited[u]:
        continue
    visited[u] = true
    ...
```

**After (M14 `dijkstra_with_tuples.spy`)**:
```python
fn pop_min(self) -> Tuple[i32, f64]:
    ...
    return (root_k, root_p)

# caller:
u: i32, p: f64 = pq.pop_min()
if p > dist[u]:
    continue            # standard textbook stale-entry guard
```
Net: `visited: List[bool]` (allocation, push-loop init, two array ops
per iteration) is gone. The program is shorter and matches the
canonical Dijkstra pseudocode 1:1.

**Other patterns previously documented in agent reports that this
unlocks (not yet migrated, future task):**
- `cursor: List[i64] = [0]` out-param idiom in csv_aggregate / markov /
  json — every "parser that needs to return (value, new_cursor)" can
  now return `Tuple[T, i64]`.
- Throwaway `final class Pair { fst, snd }` declarations sprinkled
  through tictactoe_runs, kvstore, brainfuck — at least 6 of these
  scattered across the M10–M12 corpus, each ~10 lines that can be
  inlined as a tuple type.

## Final test totals

```
$ cargo test --workspace --release
   ...
   222 passed; 0 failed; 0 ignored
```

Breakdown of the 10 new tests:

- `vm/tests/m14_tuples.rs` — 8 tests:
  1. 2-tuple literal + `t.0` / `t.1`
  2. 3-tuple with mixed types (i32, str, bool) — exercises uniform 8-byte slots over mixed primitive widths
  3. Tuple return + caller field access
  4. Annotated destructure `x: i32, y: str = pair()`
  5. Inferred destructure `x, y = pair()`
  6. `==` / `!=` element-wise (4 sub-assertions)
  7. `str(t)` → `"(1, x)"`
  8. Tuple with class-ref element + chained access `t.0.n`
- `compiler/tests/dijkstra_with_tuples_runs.rs` — 2 tests:
  1. compile-only smoke
  2. end-to-end: 4 PASS lines + summary, all distances correct

The original `dijkstra_runs` tests still pass unchanged, confirming
the M12 program is not regressed by the tuple work.
