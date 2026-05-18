# Per-milestone metrics

Source data: [per_milestone.csv](per_milestone.csv).
Benchmark data: `bench/history/`.

## Quantitative progression

| M | Tests | Compiler LOC | VM LOC | Examples LOC | Bugs found | Bugs fixed | Deferred | Bench W/T/L | fib(30) ms |
|---|---:|---:|---:|---:|---:|---:|---:|:---:|---:|
| M0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | — | — |
| M1 | 55 | 5,618 | 0 | 0 | 0 | 0 | 0 | — | — |
| M2 | 74 | 8,662 | 0 | 0 | 0 | 0 | 0 | — | — |
| M3 | 89 | 11,900 | 0 | 202 | 0 | 0 | 0 | — | — |
| M4 | 95 | 11,900 | 3,125 | 202 | **3** | 0 | 3 | — | — |
| M3.5 | 107 | 12,500 | 3,125 | 202 | 3 | **3** | 0 | — | — |
| M5 | 121 | 12,500 | 5,200 | 202 | 0 | 0 | 0 | — | — |
| M6 | 132 | 13,000 | 5,500 | 202 | 6 | 6 | 0 | — | — |
| M7 | 134 | 13,200 | 5,650 | 202 | 3 | 3 | 0 | — | — |
| **M8** | 134 | 13,200 | 6,900 | 202 | 0 | 0 | 0 | **10/2/4** | **14.6** |
| **M9** | 134 | 13,200 | 7,300 | 202 | 0 | 0 | 0 | **16/0/0** | **13.5** |
| **M10** | **173** | 13,700 | 8,600 | **1,660** | **17** | 11 | **6** | 16/0/0 | 15.8 |
| **M11** | **201** | 13,900 | 8,600 | **3,470** | 6 | **10** | 2 | 16/0/0 | 13.1 |
| **M12** | **206** | 13,553 | 7,437 | **4,947** | 2 | 1 | 1 | 16/0/0 | 13.1 |
| **M13** | 212 | 13,700 | 7,437 | 4,947 | 0 | 1 | 0 | 16/0/0 | 13.1 |
| **M14** | 222 | 14,100 | 7,437 | 5,200 | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M15** | 234 | 14,600 | 7,900 | 5,300 | 0 | 1 | 0 | 16/0/0 | 13.1 |
| **M16** | 245 | 15,000 | 8,000 | 5,400 | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M17** | **255** | **15,631** | **7,780** | **5,517** | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M18** | **267** | 15,700 | 7,820 | **7,400** | 1 | 1 | 0 | 16/0/0 | 13.1 |
| **M19** | 285 | 16,100 | 7,950 | 7,600 | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M20a** | 314 | 16,300 | 8,050 | 8,000 | 1 | 0 | 1 | 16/0/0 | 13.1 |
| **M20b** | 348 | 16,500 | 8,200 | 8,200 | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M20c** | 370 | 16,650 | 8,400 | 8,350 | 0 | 0 | 0 | 16/0/0 | 13.1 |
| **M21** | **379** | **16,823** | **8,789** | **8,467** | 0 | **1** | 0 | 16/0/0 | 13.1 |

Notes:
- LOC is at end-of-milestone; M1's LOC are mostly lexer+parser+pretty (already in their final shape).
- "Bugs found" = bugs first observed in this milestone (whether fixed here or later).
- "Bugs fixed" = bugs closed during this milestone.
- "Bench W/T/L" = StrictPy wins / ties / losses on the 16-cell benchmark suite (only meaningful M7 onwards).
- The "M3.5" row is a sub-milestone for the IR-bug-fix detour between M4 and M5.

## Benchmark progression

Same suite (16 cells across fib, quicksort, dot, mandelbrot) across snapshots.
See `bench/history/` for the underlying JSON.

| Snapshot | W | T | L | fib(30) | best vs CPython | worst vs CPython |
|---|---:|---:|---:|---:|:---:|:---:|
| M7-unfair* | 5 | 3 | 8 | 967 ms | 4× faster (fib(20)) | 5.4× slower (fib(33)) |
| M7-fair | 5 | 3 | 8 | 931 ms | 4.7× faster (fib(20)) | 5.1× slower (fib(32)) |
| M8 (JIT) | 10 | 2 | 4 | 14.6 ms | **11× faster** (fib(30)) | 3× slower (quicksort 100K) |
| M9 (full JIT) | **16** | 0 | 0 | 13.5 ms | **17× faster** (fib(32)) | **5× faster** (dot 1M) |
| M10 (stress test) | 16 | 0 | 0 | 15.8 ms | **17× faster** (fib(33)) | **4× faster** (dot 100K, mandelbrot) |
| M11 (class fix) | 16 | 0 | 0 | 13.1 ms | **16× faster** (fib(33)) | **5× faster** (dot 1M, mandelbrot) |
| M12 (stress test 2 + torture) | 16 | 0 | 0 | 13.1 ms | (same as M11; no codegen change affecting perf) | — |
| M13–M17 (language completeness) | 16 | 0 | 0 | 13.1 ms | (no benchmark cells touch the new surface yet) | — |
| M18 (round 4 stress test) | 16 | 0 | 0 | 13.1 ms | (correctness round; no perf delta) | — |
| M19–M21 (stdlib sprint) | 16 | 0 | 0 | 13.1 ms | (8 new stdlib modules; no codegen affecting perf) | — |

\* M7-unfair: Python timing included parse+compile time. Methodology bug
caught and fixed at M10-prep; the M7-fair snapshot is the honest baseline.

## Bug discovery vs. fix rate

| Source of bug | Count | Notes |
|---|---:|---|
| Caught by tests during milestone | 11 | The expected case |
| Surfaced by NEXT milestone running broken code | 6 | M4 caught 3 M3 bugs; M6 caught 3 M3.5 bugs |
| Caught by stress test (CSV agg / 6 real-world programs) | 11 | The "real world surfaces bugs" hypothesis confirming itself |
| Caught by post-bug audit (M10 AB nullable audit) | 4 | One bug found → four siblings |
| Caught by formal spec review | 0 | Spec didn't drive bug discovery; running programs did |

Total: ~32 bug observations, ~24 distinct underlying defects (some bugs had
multiple symptoms reported separately).

## Test growth pattern

Linear-ish from M1 to M7, then nearly flat M7–M9, then a +39 jump at M10:

```
M1   55  ████████████
M2   74  ████████████████
M3   89  ███████████████████
M4   95  ████████████████████
M3.5 107 ███████████████████████
M5   121 ██████████████████████████
M6   132 ████████████████████████████
M7   134 ████████████████████████████▌
M8   134 ████████████████████████████▌
M9   134 ████████████████████████████▌
M10  173 █████████████████████████████████████
```

The M7–M9 plateau reflects that those milestones were heavy *runtime* work
(dispatch, JIT, GC) where new functionality came with regression tests that
exercised existing code paths rather than adding fresh test categories.

The M10 jump is from real-world programs (each example gets a `*_runs.rs`
integration test) plus the nullable-audit regression suite (8 tests for the
silent miscompiles AB found) plus the real_world_fixes.rs suite (8 tests
for the bugs C2/C3 surfaced).
