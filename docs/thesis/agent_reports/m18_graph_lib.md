# M18-graph_lib — Generic graph algorithms over a non-generic Graph

**Brief**: Hammer M17's monomorphisation worklist with many transitive
instantiations across BFS, Dijkstra, topo-sort, and shortest-path
algorithms operating on one `final class Graph`.

**Wall-clock**: ~75 minutes (read-through + 6 probes + program + tests +
report).

**Files added**:
- `examples/graph_lib.spy` (~480 lines)
- `compiler/tests/graph_lib_runs.rs` (3 tests)
- `examples/_probe_generic_try.spy`, `_probe_transitive3.spy`,
  `_probe_min_by.spy`, `_probe_enumerate.spy`, `_probe_class_t.spy`,
  `_probe_cycle.spy` (six exploratory probes; all green)
- This report.

## Result

The program runs nine sub-tests and exits 0:

```
PASS test=1 (bfs_tree)
PASS test=2 (bfs_with_enumerate)
PASS test=3 (dijkstra_clrs)
PASS test=4 (shortest_path)
PASS test=5 (shortest_path_unreachable)
PASS test=6 (topo_dag)
PASS test=7 (topo_cycle)
PASS test=8 (safe_get_multi_T) instantiations=5
PASS test=9 (labels_safe_get)
OK: 9/9
```

## M17 worklist drainage — empirical count

The headline ask was an empirical count of monomorphic instantiations.
`compiler/tests/graph_lib_runs.rs::graph_lib_worklist_drains_to_expected_set`
loads the compiled `.spyc`, walks the function table, and prints every
mangled name. Output (run with `-- --nocapture`):

```
graph_lib mangled function table (10 entries):
  Graph.__init__
  Node.__init__
  enumerate__i32
  first_or_default__i32
  min_by__i32
  safe_get__class17
  safe_get__f64
  safe_get__i32
  safe_get__str
  safe_get__tuple_i32_f64
```

Stripping the two class constructors (which carry `__init__` but are
not generic instantiations), the **M17 worklist drained 8 monomorphic
instantiations to fixpoint** across 4 generic source functions:

| Source fn               | Instantiations |
|-------------------------|----------------|
| `safe_get[T]`           | 5 (i32, f64, str, Tuple[i32, f64], class Node) |
| `enumerate[T]`          | 1 (i32) |
| `min_by[T]`             | 1 (i32) |
| `first_or_default[T]`   | 1 (i32) |
| **Total**               | **8** |

Three of these are transitive: `min_by__i32` and `first_or_default__i32`
both call `safe_get(xs, ...)` from inside their bodies, and the
substitution `{T -> i32}` threads correctly all the way through — both
end up dispatching to the same `safe_get__i32` FuncId, not to a fresh
copy. `min_by__i32` was discovered when `dijkstra_with_parents` calls
it on the i32 candidate list; `safe_get__i32` was minted not from a
top-level call but from inside `min_by__i32`'s body. The worklist
"mint on first sighting, dispatch by key thereafter" semantics handled
this without any direct help.

## NEW bugs discovered

**None.** Every M13–M17 surface I exercised worked as advertised:

* **Generic + try/except + multiple T values**: `safe_get[T]` wraps
  `xs[i]` in `try: ... except IndexError as e: return default`. Each of
  five distinct T values (one with `Tuple[i32, f64]`, one with a
  final-class type) was confirmed to materialise as a separate
  `safe_get__<mangle>` and to return the expected default on
  out-of-range index. The M15 JIT carve-out fired per-instantiation
  automatically as the M17 report claimed.
* **Transitive 3-level instantiation**: `_probe_transitive3.spy`
  threads T through `outer[T] -> middle[T] -> inner[T]` for T = i32,
  str, f64 — all green.
* **Generic returning Tuple[i32, T]**: `min_by[T]` returns
  `Tuple[i32, T]`. Mangling encodes the return-tuple's element types
  correctly inside the body (the `(best_idx, safe_get(...))`
  literal-tuple constructor); call-site destructure
  `picked: Tuple[i32, i32] = min_by(...)` works.
* **Generic called with `Tuple[i32, f64]` argument**: `safe_get` over a
  `List[Tuple[i32, f64]]` mangles as `safe_get__tuple_i32_f64`.
  Inference picks the tuple type from the list-element argument.
* **Recursion through generics**: `topo_dfs` is a non-generic recursive
  DFS that raises `RuntimeError("cycle detected")` from inside the
  recursion. The exception propagates through the recursive frames to
  the outer `try: topo_sort(g) except RuntimeError as e` without
  stack-frame leakage. The probe `_probe_cycle.spy` is the minimal
  isolated form.

## Confirmed gaps (already documented)

* **Class-body docstrings are rejected.** Putting `"""..."""` as the
  first statement inside `final class Graph:` produced
  `E0001: expected identifier, found StrLit(...)`. Worked around with
  a `#` comment above the class. (Spec §3 doesn't promise class-body
  docstrings, but every existing example in `examples/` has its
  docstrings at module or function scope only — so this is "spec is
  what it is".)
* **No generic classes / no generic methods on classes** (spec §5.1.5
  v0.2). The Graph class is non-generic by design. All polymorphism
  lives in free-function generic helpers. Did not probe this.
* **Pre-M13 baseline**: the M14 `dijkstra_with_tuples.spy` is 295
  lines; the M18 `graph_lib.spy` is ~480 lines but does four
  algorithms + nine tests + five generic-T probes in the same file.
  Apples-to-oranges, but the "min_by + safe_get" generic-helper layer
  is the LOC win: before M17 a hand-rolled equivalent would have been
  five distinct `min_by_i32` / `min_by_f64` / ... copies plus the
  matching five `safe_get_*` copies (~150 lines duplicated), and
  before M15 the `safe_get` bounds-check would have been an explicit
  `if i < 0 or i >= len(xs)` guard with no way to recover from
  arbitrary downstream IndexError. The post-M17 version writes the
  helpers once.

## Language-surface awkwardness (not bugs)

* **Mangling spec vs implementation drift.** Spec §5.1.5 table says a
  class type with id N mangles as `class<N>` (with angle brackets);
  the actual implementation at `compiler/src/typecheck.rs:1772` emits
  `class{}` with NO brackets (`class17` in our case). My test asserts
  `starts_with("safe_get__class")` to be safe. Tiny doc/impl divergence
  the M17 report also propagated. Mechanical fix is one line — either
  in the spec table or in `mangle_ty`. Not a bug.
* **`if cur != src:` for path-walking parents.** No `is`/`is not` for
  primitives in v0.1, so equality on i32 is `==`/`!=`. Fine, just
  noted for readers familiar with Python's `is not`.
* **`return (a, b)` tuple-literal inside generic body.** Works, but the
  return-type annotation must spell `Tuple[i32, T]` on the function
  signature; you can't write `(i32, T)` form there mid-generic-body in
  some contexts. Cosmetic.

## What I deliberately did NOT probe (out of scope)

* `match scrutinee:` inside a generic body (the brief flagged this as
  shape #3) — left to the sibling agent whose task touches match.
* `class MyError(Exception):` — known v0.2 gap per spec §7.5.6.
* `with open(...) as f:` inside `try` — known M15 gap.
* Re-raising via `raise e` from inside `except ... as e:` — flagged in
  brief shape #10 but out of scope for graph algorithms; my probes
  caught and recovered, never re-raised.

## Workaround DELTA vs pre-M13

Compared to `examples/dijkstra.spy` (M12, no tuples, no generics, no
try/except):

| Capability                 | Pre-M13 form                                  | M18 form                                                   |
|----------------------------|-----------------------------------------------|------------------------------------------------------------|
| Multi-return from pop_min  | Parallel `visited[]` array workaround         | `Tuple[List[f64], List[i32]]` (dist + parent)              |
| Per-T helper functions     | Hand-rolled `safe_get_i32`/`safe_get_f64`/... | One `safe_get[T]` body, monomorphised 5 ways               |
| Bounds-checked indexing    | Explicit `if i<0 or i>=len(xs)` per call      | `try: xs[i] except IndexError: default` inside `safe_get`  |
| Cycle detection in topo    | Return a magic sentinel, caller-decoded       | `raise RuntimeError("cycle detected")` + `except` at call site |
| Unreachable-dst path       | Sentinel `INF` + caller decodes               | `raise RuntimeError("no path")`                            |

The `safe_get[T]` family alone replaces what would have been ~80 lines
of duplicated bounds-check code at five element types.

## Final test totals

`cargo test --release --test graph_lib_runs`:

```
running 3 tests
test graph_lib_compiles ... ok
test graph_lib_worklist_drains_to_expected_set ... ok
test graph_lib_runs_all_nine_tests ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
```

**Workspace status**: `cargo test --workspace --release` shows ONE
failing test, `compiler/tests/compile_examples.rs::compile_all_examples_produces_valid_header`,
because that test re-compiles every `.spy` file in `examples/` and a
sibling agent's probe `_probe_isinstance_and.spy` has a typecheck error
(`E2003: no field 'n' on type class#16`). That file is outside my
file-ownership boundary; per the SHARED_BRIEF stop criteria this is a
"regression on someone else's territory — flag and stop". My own files
all compile and run cleanly in isolation, and my test binary is green.
