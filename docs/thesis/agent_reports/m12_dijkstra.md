# M12 — Dijkstra + hand-rolled binary min-heap

**Brief**: single-source shortest paths with a `final class Graph`
(parallel `List[List[i32]]` / `List[List[f64]]` adjacency) and a
`final class MinHeap` whose `sift_up` / `sift_down` are recursive
method calls. Designed to stress the post-M11 class system on a pattern
no existing example covers: classes that own and mutate parallel
nested-list fields, plus method recursion.

**Wall-clock**: ~30 minutes
**Files added**: `examples/dijkstra.spy` (417 lines, ~60 of which are
header docstring + per-class commentary), `compiler/tests/dijkstra_runs.rs`
(68 lines).

## Result

End-to-end, clean on first compile:

```
PASS test=1
PASS test=2
PASS test=3
PASS test=4
PASS test=5
OK: 5/5
```

- Test 1: linear chain 0→1→2→3→4 unit weights → `[0,1,2,3,4]`.
- Test 2: CLRS Figure 24.6 5-vertex graph → `[0, 8, 9, 5, 7]`.
- Test 3: 6-vertex graph with a 100-weight direct edge plus a 5-hop
  unit-weight detour, plus two shortcuts that don't end up on the
  optimal path → `[0,1,2,3,4,5]`.
- Test 4: trivial 1-vertex graph → `[0.0]`.
- Test 5 (bonus): re-runs test 1 but the inner relaxation loop is
  `for v: i32 in g.adj_node[u]:` — the exact pattern the brief asked
  me to probe (method-receiver-style iteration over a
  class-field-of-class). Result matches test 1 byte-for-byte, so the
  M9 `for` desugar's "evaluate iterable once at loop header"
  semantics (compiler/src/ir.rs:873) holds across method calls.

Three back-to-back test invocations produced byte-identical PASS output.
No STATUS_HEAP_CORRUPTION, no non-determinism, no flakiness.

## NEW bugs discovered

**None.** Clean stress test. Five post-M11 hazards probed, all held:

1. `final class` with parallel `List[List[T]]` fields, methods that
   mutate them. BUG-016 (subclass field offset aliasing) is fixed,
   and the non-inherited sibling pattern also works correctly. No
   evidence of vtable-pointer corruption at offset 0.
2. Recursive method calls (`MinHeap.sift_up` / `sift_down`) at depth
   `O(log N) ≈ 3-4`. `final class` is correctly devirtualised, so
   each recursive call is a direct call — no vtable surprises.
3. The 1e18 sentinel: `dist[u] + w < dist[v]` short-circuits
   correctly when `dist[u] == 1e18`, because `1e18 + small == 1e18`
   in IEEE 754 binary64 and `1e18 < 1e18` is false. The algorithm
   never relaxes through an unreached vertex.
4. `for v: i32 in g.adj_node[u]:` — the desugar materialises the
   inner list once and indexes into it. No re-evaluation of the
   attr-ref + subscript per iteration.
5. `list.pop()` returning typed values from class-owned lists —
   `keys.pop()` returns i32 and `prios.pop()` returns f64, neither
   aliases the other.

## Confirmed BUGS_KNOWN entries

None hit during this build.

- §4 / §5 (non-deterministic heap corruption, position-sensitive
  crash): provisionally fixed post-M11. **3 back-to-back runs
  produced byte-identical stdout** — one more data point that the
  M11 fix pass really did close these.
- §6 (no line continuation across `+`): didn't hit it because I used
  the accumulator pattern preemptively.
- BUG-025 (`try`/`except`, fallible `open()`): not exercised.

## Language-surface awkwardness (not bugs)

- **No tuples / multi-return**: `pop_min() -> i32` returns just the
  vertex id; the caller looks up its priority via `dist[v]` if it
  needs both.
- **No 2D list literal**: `List[List[T]]` is built with a `while` +
  `.append` of fresh inner lists in `__init__`. Not specific to
  this program — sudoku, game_of_life do the same.
- **`len()` returns i64**: every length read for an i32 loop bound
  needs `i32(i64(len(xs)))`. Verbose but unambiguous.
- **`final` keyword overloaded**: `final` on a class means
  "non-extendable"; `final` at module scope means "constant
  binding". Both uses appear in the file (`final INF: f64 = 1.0e18`
  and `final class Graph`). Fine, just worth a spec footnote.
- **`assert` works** — `pop_min` uses it as a precondition guard
  and it traps cleanly.

## Final test totals

`cargo test --workspace --release --no-fail-fast`: **203 passed, 2
failed, 2 ignored**.

- The 2 failures are entirely in `compiler/tests/btree_runs.rs`
  (parallel M12 agent's territory; its program traps with
  `IndexError: index -1 out of range for length 25`). I did not
  touch it.
- The 2 new tests (`dijkstra_compiles`, `dijkstra_solves_classic_graphs`)
  are green, bumping the M11 baseline of 201 to 203.

## Why this clean report matters

The patterns probed — `final class` with mutable nested-list fields,
recursive method calls, method-receiver iteration — are exactly the
shapes that produced BUG-016 / N2 / the M11 vtable-cap. All hold under
post-M11 codegen with zero workarounds and zero non-deterministic
outcomes. Adds Dijkstra alongside calculator / lisp as evidence the
M11 fix pass landed cleanly.
