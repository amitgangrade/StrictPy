# Benchmark history

Snapshots taken at major milestones to support the "performance journey" narrative in the blog post.

| File | Milestone | What changed | Key result (fib(30)) |
|---|---|---|---|
| `m7_pre_jit_unfair.json` | M7 (initial) | First benchmark. Python timing included parse+compile (unfair). | 967ms (3.83× slower than Python) |
| `m7_pre_jit_fair.json` | M7 (corrected) | Python pre-compiled to `.pyc`; fair comparison. | 931ms (3.58× slower) |
| `m8_jit.json` + `.md` | M8 (JIT) | Cranelift AOT compilation for arith/cmp/branch/call/list-read. | 14.6ms (**11× faster than Python**) |
| `m9_full_jit.json` + `.md` | M9 (full coverage) | JIT coverage extended to heap mutation, fields, virtual calls. | 13.5ms (12× faster) |
| `m10_real_world.json` + `.md` | M10 (real-world stress test) | 6 new programs landed (game_of_life, sudoku, json_parse, markov, kvstore, brainfuck); 11 bugs surfaced, 6 critical-or-medium fixed, 6 architectural deferred to BUGS_KNOWN.md; new stdlib: for-in, str.split, sorted/sort, list.pop. Same wins as M9 with no regression. | 15.8ms (10× faster) |
| `m11_class_fix.json` + `.md` | M11 (class-system overhaul) | 5 more programs (lambda_calc, calculator, tictactoe, levenshtein, lisp). Class/vtable cluster fixed: BUG-015/016/017 + N1 (vtable >4 slots) + N2 (heap corruption on subclass+vcall). Root cause for vtable-mod-4 was a class_id vs type_id collision in op_new. Primitive ctors (i32/i64/f64/char) now do arg-type dispatch. BUG-026/027 provisionally closed. Same 16/16 wins, slight perf improvement. | 13.1ms (11× faster) |
| `m22_phase2_stdlib.json` | M22 (Phase 2 stdlib complete) | M12-M22 layered on top: stress round 4 (BUG-036), 5 language features (short-circuit and/or, tuples, try/except, isinstance + match, generics), 17 stdlib modules across Phase 1 (M19-M21: sys/os/path/io/time/random/math/json/re) and Phase 2 (M22: argparse/collections/csv/base64/hashlib/itertools/statistics/struct/urllib_parse). Bench codegen untouched. 16/16 wins held across all 11 milestones since M9. | 15.7ms (12× faster) |
| `m26_extended.json` | M26 (extended benchmark suite) | First snapshot of a **10-test extension** beyond the canonical 4-program suite. 5 pure-compute (n-queens / sieve / matmul / btree / heapsort) + 5 stdlib (json / regex / sha256 / csv / sqlite). 30 cells × best-of-3. Headline: **28 wins / 2 ties / 0 losses**. Pure compute matches the canonical wins (0.02–0.23× ratios); the one non-win is btree(10k) at 1.13× where allocation pressure overwhelms the JIT win. Stdlib results were surprising — all 15 cells go to StrictPy, with the narrowing-at-scale pattern visible on every test as Python's ~50–70ms startup amortises. Rendered as a separate report at `bench/EXTENDED_REPORT.md`. | (same as M25 era; bench codegen unchanged) |

Each `.json` is consumed directly by `bench/harness.py --report-only` to regenerate the matching report. The `.md` files are the rendered reports at that point in time.

## Method

- Best-of-3 wall-clock runs per cell.
- Total wall-clock, including process startup, excluding bytecode compilation on both sides.
- Host: Windows 11, single-thread workload.
- CPython 3.12.10 throughout (the comparison baseline is stable).
