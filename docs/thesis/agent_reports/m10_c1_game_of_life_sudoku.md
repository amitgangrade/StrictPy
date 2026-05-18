# M10-C1 — Game of Life + Sudoku solver

**Brief**: write two real-world programs to surface bugs. Game of Life
stresses nested-list reference semantics; Sudoku stresses recursion + 2D
mutation + booleans.

**Wall-clock**: ~60 minutes
**Tool uses**: 84
**Files added**: `examples/game_of_life.spy` (157 lines), `examples/sudoku.spy` (205 lines),
`compiler/tests/game_of_life_runs.rs` (75 lines), `compiler/tests/sudoku_runs.rs` (71 lines).

## Result

**Both programs run end-to-end. Zero new compiler/VM bugs surfaced.**

This is an interesting negative result. The agent specifically probed for:

- Nested list reference semantics: `grid[r][c] = v` writes through
  `grid[r]`'s returned reference. ✓ Confirmed via probe test.
- Boolean nullable narrowing (possible cousin of BUG-001 for booleans):
  `if recurse(grid): return true` followed by mutation restore. ✓ No
  silent miscompile. The M10-AB audit appears to have caught the boolean
  cousins of the float bug.
- Per-generation grid allocation in a tight loop. 20 generations × 30×15
  grid = 9,000 list allocations across the run. ✓ No GC pathology.
- 81-cell sudoku backtracker recursion. ✓ Well under the 1024-frame cap.

## Game of Life output (first 5 lines + a later frame)

```
-- generation 0 --
.#............................
..#...........................
###...........................
..............................
..............................
                              [...]
-- generation 1 --
..............................
#.#...........................
.##...........................
```

## Sudoku output

```
solution:
5 3 4 | 6 7 8 | 9 1 2
6 7 2 | 1 9 5 | 3 4 8
1 9 8 | 3 4 2 | 5 6 7
------+-------+------
8 5 9 | 7 6 1 | 4 2 3
4 2 6 | 8 5 3 | 7 9 1
[...]
row0=534678912
```

## Language-surface awkwardness

Even though no compiler bugs were found, the surface friction was real:

- **No 2-D list literal syntax confirmed safe**: agent wrote a 9-arg
  `make_row(...)` helper rather than risk hitting an untested codepath
  with `[[5,3,0,...],[...]]`. The `csv_aggregate` precedent of incremental
  `.append` won.
- **No tuple types**: `find_empty` had to return `List[i32]` of `[found, row, col]`
  instead of a natural `Tuple[bool, i32, i32]`.
- **No `for x in xs:`**: every neighbor count, every row scan is an indexed
  `while`. The Game-of-Life neighbor count is 8 hand-unrolled `cell_at`
  calls.
- **No `chr(35)` or `char → str` auto-promotion**: had to emit `"#"`/`"."`
  string literals instead of building chars dynamically.
- **No printf**: assert signatures built by concatenating `str(grid[0][c])`
  in a loop to produce `row0=534678912`.
- **No `Result[T, E]`**: backtracker uses `bool` return + side-effect
  mutation instead of returning a structured success/failure value.

## Why this report matters

The "zero new bugs" result is **evidence the M10-AB nullable audit was
thorough**. C1's probes for boolean cousins of the f64 bug all came back
clean. C2 (JSON parser) running in parallel DID find new bugs — but in a
different class (vtable/inheritance), not the same nullable-dispatch
pattern.

This signals that audit-after-discovery generalizes: when one bug is
found in a class of dispatch site, auditing all similar sites in the same
file is high-leverage. Other classes of bugs in OTHER files need their
own discovery process — usually via more stress tests.
