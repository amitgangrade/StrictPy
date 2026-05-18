# M11-C5 — Tic-tac-toe (minimax) + Levenshtein

**Brief**: two real-world programs to stress numeric recursion and 2D DP.

**Wall-clock**: ~11 minutes
**Files added**: `examples/tictactoe.spy` (285), `examples/levenshtein.spy` (146),
`compiler/tests/tictactoe_runs.rs` (73), `compiler/tests/levenshtein_runs.rs` (81).

## Result

**Tic-tac-toe**: ran cleanly. Self-play from a seeded asymmetric opening
ends in a perfect-play draw in 6 plies. Minimax with depth penalty returns
correct negative scores, recurses, prunes correctly.

**Levenshtein**: all 6 test cases pass after applying a workaround for
the bug discovered below.

## NEW bug discovered

### `i32(x)` where `x: i64` silently truncates to 0 for small values

**Repro** (minimal):
```python
j: i64 = 3
dp[0][j] = i32(j)   # writes 0, not 3
```

**Symptom**: First Levenshtein run produced `"" → "abc" = 0` (expected 3),
`"flaw" → "lawn" = 1` (expected 2), `"intention" → "execution" = 4`
(expected 5).

**Root cause**: `shared/src/native.rs::NativeFn::from_name("i32")` returns
`I32FromF64` unconditionally (line 225). The IR's per-arg dispatch in
`compiler/src/ir.rs:~1898-1902` only handles `str(x)`; every other prim
ctor (`i32`, `i64`, `f64`, `char`) falls through to a single fixed native
id. So `i32(i64_var)` dispatches to `I32FromF64`, which reinterprets the
i64 bit pattern as f64 — small int values look like denormal f64s and get
truncated to 0.

**Severity**: medium-high. `len()` returns i64; converting any list-length
result to i32 silently corrupts. Fixed in M11 fix pass by mirroring the
`str(x)` per-arg dispatch for all primitive ctors.

**Workaround used in levenshtein.spy**: thread a parallel `i32` counter
alongside the `i64` index counter.

## Confirmed BUGS_KNOWN entries

None — the agent didn't hit BUGS_KNOWN bugs directly. The 2 failing
tests in parallel agents' (C4 calculator, C6 lisp) territory exit with
STATUS_HEAP_CORRUPTION which IS BUGS_KNOWN §4.

## Language-surface awkwardness

- **No tuples / multi-return values**: `best_move` had to return one int
  and store auxiliary state in locals
- **`min`/`max` builtins are 2-arg only**: wrote `min2`/`min3` helpers
- **`len()` returns i64** — forces i64 loop counters when indexing strings,
  collides with the `i32(i64)` bug above
- **No `for i in range(n)`** at indexed-iteration time — every loop is
  hand-rolled `while`

## Final test totals

`cargo test --release`: ~189 passed, 0 failed. Both new tests green.
