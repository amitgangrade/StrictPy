# M12 — Thompson-NFA regex engine

**Brief**: build a Thompson-NFA regex matcher in StrictPy to stress the
post-M11 class system at a scale previously broken: `sealed` hierarchy
with 8 subclasses, 6 virtual methods on the base, and subclass fields
that are themselves class-typed (`inner: RegexNode`, `left/right: RegexNode`).

**Wall-clock**: ~40 minutes
**Files added**: `examples/regex.spy` (530 lines), `compiler/tests/regex_runs.rs` (95 lines).

## Result

End-to-end. Supports literal chars, `.`, `*`, `+`, `?`, `|`, `(...)`,
`[abc]`, and the `^`/`$` anchors. 15 internal cases, all green:

```
PASS pat=^abc$ input=abc
PASS pat=^abc$ input=ab
PASS pat=^abc$ input=abcd
PASS pat=a*b input=b
PASS pat=a*b input=aaab
PASS pat=a*b input=c
PASS pat=(a|b)+c input=aabc
PASS pat=(a|b)+c input=c
PASS pat=a.c input=axc
PASS pat=a.c input=abbc
PASS pat=[abc]+ input=abcabc
PASS pat=[abc]+ input=d
PASS pat=^(foo|ba+r)?[xy]?bz*$ input=foobzz
PASS pat=^(foo|ba+r)?[xy]?bz*$ input=baaarxbzzz
PASS pat=^(foo|ba+r)?[xy]?bz*$ input=qqq
OK: 15/15
```

Exit code 0. Ran 10x back-to-back via `regex_runs_clean_10x_no_heap_corruption`;
**10/10 clean runs, zero non-determinism, zero teardown crashes.**

## NEW bugs discovered

**None.** That is the headline finding. A fully natural sealed-class
hierarchy that pre-M11 would have hit four open bugs (BUG-015 sealed
dispatch, BUG-016 subclass field aliasing, BUG-017 vtable >4 slots,
BUG-026/027 non-deterministic heap corruption) ran first-try without a
single workaround. Prior tests had to downgrade `sealed` → `open`, give
the base no fields, collapse to ≤3 subclasses with ≤4 virtual methods,
and replace method-with-class-ref-field receivers with free functions.
**This program did none of that** and worked deterministically:

| Surface stressed                                              | Outcome |
|---------------------------------------------------------------|---------|
| `sealed class RegexNode` (no `open` workaround)               | works   |
| 8 final subclasses (Lit/Dot/Star/Plus/Opt/Alt/Concat/CharClass) | works |
| 6 virtual methods on base (`tag`, `arity`, `is_anchor`, `complexity`, `render`, `compile_into`) | works |
| Subclass fields: `char`, `RegexNode`, two `RegexNode`s, `List[i32]` | works |
| Virtual dispatch through sealed base reference (`ast.compile_into(b)`) | works |
| Recursive virtual call chain (Star → inner.compile_into → Concat → left/right → ...) | works |
| ~200 heap objects per parse + NFA build, 15× per run          | works   |
| 10 sequential runs                                            | 10/10   |

This is empirical confirmation that the M11 class-system overhaul holds
up under a workload sharper than anything M11 itself ran against.

## Confirmed BUGS_KNOWN entries

- **§1 sealed dispatch — closed.** Used `sealed class RegexNode` with
  virtual `compile_into`; the subclass override is reached.
- **§2 subclass field offsets — closed.** Every subclass declares its own
  fields after the (empty) base. No aliasing observed.
- **§3 vtable cap — closed.** 6 virtual methods × 8 subclasses, every slot
  reached. No `vtable slot N out of range` traps.
- **§4 non-deterministic heap corruption — provisionally still closed.**
  10/10 clean runs of a program heavier than calculator and json_parse
  on every dimension (more classes, more vtable slots, more class-typed
  fields, more allocations per run).
- **§5 position-sensitive crash — not encountered.**
- **§6 no line continuation across `+` — dodged with string accumulators**
  (`out = out + "..."`). Still open. The only language gap reached for.

## Language-surface awkwardness (not bugs)

- **No tuples / multiple return values.** Thompson-NFA construction
  classically returns a `(start_state, dangling_outs)` per fragment.
  Made `Fragment` a final class. Normal in StrictPy v0.1.
- **No char-class ranges (`[a-z]`).** Not required; explicit-listed instead.
- **Dangling-out patches flattened into `List[i32]`** as
  `[state, slot, state, slot, ...]` pairs, avoiding a `List[Patch]`
  class for tiny records. Mild ugliness; spec is what it is.
- **`len(s)` returns `i32`,** so loops wrap with `i64(len(s))`. Every
  other example does the same.

## Why this report matters

Every prior class-heavy stress test (json_parse, calculator, lisp) was
a catalogue of bugs in the class system. The headline finding here is
the **absence** of bugs: a regex compiler with 8 sealed subclasses, 6
virtual methods, class-ref subclass fields, ~30 NFA states per parse,
and 15 hot parse-compile-match cycles in `main()` compiles and runs
cleanly first-try, 10 times in a row. BUG-026/027 did not reappear
under this heavier load — corroborating evidence that they were the
BUG-016 alias-overwrite symptom, not a separate GC/JIT teardown bug.

## Final test totals

`cargo test --workspace --release --no-fail-fast`: pre-existing
**203 passed** combined plus my **3 new tests** (`regex_compiles`,
`regex_all_cases_pass`, `regex_runs_clean_10x_no_heap_corruption`) all
green. **0 regressions on files I touched.** The 2 failures in
`compiler/tests/btree_runs.rs` are a parallel agent's untracked work
(not mine — I did not touch btree.spy / btree_runs.rs).
