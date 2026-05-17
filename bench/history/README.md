# Benchmark history

Snapshots taken at major milestones to support the "performance journey" narrative in the blog post.

| File | Milestone | What changed | Key result (fib(30)) |
|---|---|---|---|
| `m7_pre_jit_unfair.json` | M7 (initial) | First benchmark. Python timing included parse+compile (unfair). | 967ms (3.83× slower than Python) |
| `m7_pre_jit_fair.json` | M7 (corrected) | Python pre-compiled to `.pyc`; fair comparison. | 931ms (3.58× slower) |
| `m8_jit.json` + `.md` | M8 (JIT) | Cranelift AOT compilation for arith/cmp/branch/call/list-read. | 14.6ms (**11× faster than Python**) |
| `m9_full_jit.json` + `.md` | M9 (full coverage) | JIT coverage extended to heap mutation, fields, virtual calls. | (pending) |

Each `.json` is consumed directly by `bench/harness.py --report-only` to regenerate the matching report. The `.md` files are the rendered reports at that point in time.

## Method

- Best-of-3 wall-clock runs per cell.
- Total wall-clock, including process startup, excluding bytecode compilation on both sides.
- Host: Windows 11, single-thread workload.
- CPython 3.12.10 throughout (the comparison baseline is stable).
