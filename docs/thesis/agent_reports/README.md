# Agent reports archive

Verbatim or condensed reports from sub-agent task invocations, ordered
chronologically. Primary source material for the "AI-assisted development"
chapter of the thesis.

## Format

Each file is one agent task. Filename pattern:
`<milestone>_<short-description>.md`. Each report includes:

- Task brief summary (what was asked)
- Agent's report (verbatim where preserved; condensed otherwise)
- Wall-clock duration and token usage if known
- Files touched
- Notable findings

## Coverage

Earlier milestones (M0–M9) have reports preserved in conversation
transcript but only partially reconstructed here. The discipline starting
at M10 is: every agent's report gets a verbatim copy in this directory at
the time it lands.

## Files

| File | Milestone | Description |
|---|---|---|
| `m8_cranelift_jit.md` | M8 | Cranelift AOT integration. fib(30): 931ms → 14.6ms. |
| `m9_full_jit_coverage.md` | M9 | Heap mutation + fields + virtual calls. All 16/16 cells win. |
| `m10_csv_aggregator_stress.md` | M10 prep | First real-world program. Found BUG-001 (nullable f64). |
| `m10_ab_nullable_audit.md` | M10 | Audit found 4 more nullable miscompiles in codegen.rs. |
| `m10_c1_game_of_life_sudoku.md` | M10 | Game of Life + Sudoku. Zero new bugs found. |
| `m10_c2_json_parser_markov.md` | M10 | JSON + Markov. **Found 8 bugs incl. is-not-none INVERTED.** |
| `m10_c3_kvstore_brainfuck.md` | M10 | KV store + Brainfuck. Found 3 typecheck/native bugs. |
| `m10_fix_pass.md` | M10 | Fixed critical bugs from C2/C3 reports. |
| `m11_c4_lambda_calc_calculator.md` | M11 | λ-calc + calculator. Calculator hits BUG-026 hard. |
| `m11_c5_tictactoe_levenshtein.md` | M11 | Tic-tac-toe + Levenshtein. Found `i32(i64)` silent truncation. |
| `m11_c6_lisp_interpreter.md` | M11 | Toy Lisp. **Found N1 (vtable >4 slots) + N2 (deterministic heap corruption).** |
| `m11_class_system_fix.md` | M11 | Class-system overhaul. **3 converging root causes for vtable-mod-4; class_id ↔ type_id collision in op_new.** Provisionally closes BUG-026/027. |
| `m12_regex.md` | M12 | Thompson-NFA regex engine. **Zero new bugs.** Confirmation that M11 holds for sealed/8-subclass/6-vmethod hierarchies with class-ref fields. |
| `m12_dijkstra.md` | M12 | Dijkstra + min-heap PQ. **Zero new bugs.** Confirmation for `final class` with parallel nested-list fields + recursive method calls. |
| `m12_btree.md` | M12 | In-memory B-tree (order 4). Found **BUG-034** (str != str silent miscompile — same shape as BUG-008) and **BUG-035** (and/or no short-circuit). BUG-034 fixed inline. |
| `m12_torture.md` | M12 | Torture test (250 sequential runs across calculator/json_parse/lisp). **BUG-026 + BUG-027 CONFIRMED FIXED.** |
| `m13_short_circuit.md` | M13 | Short-circuit `and`/`or` (BUG-035 fixed). First mid-expression CFG manipulation; documents the slot-phi pattern for M15 try/except. |
| `m14_tuples.md` | M14 | Tuples + destructuring. Heap-allocated synthetic class layouts; zero new VM opcodes. Eliminates highest-frequency workaround. Incidentally fixed an assert(cond, msg) IR-tuple-allocation crash. |
| `m15_try_except.md` | M15 | try/except/finally + raise. **BUG-025 closed.** Lazy materialisation of exception objects; per-function JIT carve-out. |
| `m16_match_isinstance.md` | M16 | `isinstance(x, T)` + `match case Constructor()`. Eliminates the `kind: i32` discriminator workaround. Flow-narrowing for isinstance mirrors is-not-none. |
| `m17_generics.md` | M17 | Generic free functions with call-site monomorphisation. Lazy worklist (Pass 2.6 seeds; Pass 3.5 drains to fixpoint). Generic classes deferred to v0.2. |
| `m18_algorithms_lib.md` | M18 | R1 — Generic algorithms library. 8 generic fns × 5 primitive types + Tuple + class. **Zero new bugs.** |
| `m18_json_parse_v2.md` | M18 | R2 — JSON parser rewritten with M13-M17 surface. All 8 M10 workarounds eliminated. **Zero new bugs.** |
| `m18_expr_interp.md` | M18 | R3 — Expression interpreter. Found **BUG-036** (divzero name mismatch). Plus 7 archived edge-case probes. |
| `m18_graph_lib.md` | M18 | R4 — Generic graph algorithms. M17 worklist verified: 8 monomorphic instantiations drained to fixpoint, 2 discovered transitively. **Zero new bugs.** |
| `m19_import_sys.md` | M19 | Import machinery + sys module. First real module system. Non-catchable VmError::Exit. argv plumbed end-to-end. |
| `m20a_os_path_io.md` | M20a | os + path + io stdlib (23 NativeFns). Found **BUG-037** incidentally (?? null-coalesce always-fallback). |
| `m20b_time_random_math.md` | M20b | time + random + math stdlib (31 NativeFns). Numerical Recipes LCG; hand-rolled civil_from_days for ISO formatting. |
| `m20c_json_re.md` | M20c | json + re stdlib (12 NativeFns). serde_json + regex crates added only to vm/Cargo.toml. **Phase 1 stdlib complete.** |
| `m22_p2a.md` | M22 P2A | argparse + collections + csv (26 NativeFns, 250-280). First parallel worktree-isolated stdlib agent. ArgParser as Dict[str, str] shim pending v0.3 stdlib classes. |
| `m22_p2b.md` | M22 P2B | base64 + hashlib (9 NativeFns, 290-304). 5 new crate deps in vm/Cargo.toml: base64, sha1, sha2, md-5, hmac. Zero incidental bugs. |
| `m22_p2c.md` | M22 P2C | itertools + statistics (20 NativeFns, 310-329). Monomorphic per-type variants matching M20b random.*. Zero incidental bugs. |
| `m22_p2d.md` | M22 P2D | struct + urllib_parse (18 NativeFns, 330-347). `str`-as-byte-buffer encoding for binary IO. Self-fixed an `OBJECT_HEADER_SIZE` mismatch. |
| `m23_p3a_a.md` | M23 P3a-A | subprocess + pathlib (20 NativeFns, 350-389). i64-handle process registry; flat-fn pathlib pending v0.3 stdlib classes. |
| `m23_p3a_b.md` | M23 P3a-B | datetime (22 NativeFns, 390-411). Hand-rolled `civil_from_days` + platform-specific local_offset via FFI; no chrono dep. |
| `m23_p3a_c.md` | M23 P3a-C | threading.Lock + Semaphore + queue.PriorityQueue (18 NativeFns, 420-437). Three new SharedVm slot tables. Incidental resolver fix for stdlib-module shadowing of legacy `from X import Y` prelude bindings. |
| `m23_p3a_d.md` | M23 P3a-D | sqlite3 (9 NativeFns, 440-448) via rusqlite-bundled. Stringified result cells; v0.3 typed rows. |
| `m24_a.md` | M24-A | Stress: background job_scheduler (subprocess + threading.Lock + queue.PriorityQueue + datetime). 9/9 probes PASS, 0 bugs. |
| `m24_b.md` | M24-B | Stress: event_log CLI (sqlite3 + datetime + argparse + io + pathlib + re). 14/14 probes PASS. **Found BUG-039** (`k in Dict[str,*]` always false) + segfault sibling on Dict[i64,_]. |
| `m24_c.md` | M24-C | Stress: parallel test_runner (subprocess + threading + queue + sqlite3 + time). 10/10 PASS. Real parallelism verified: 3.62×-5.75× speedup at N=4. |
| `m24_d.md` | M24-D | Stress: fs_migrator (pathlib + os + datetime + subprocess + io). 10/10 PASS. Documented missing stdlib primitives (os.mtime, os.size, re capture groups). |

(M25 was a single-conversation orchestrator refactor — no sub-agents,
no agent report. See `docs/thesis/milestones/m25_unified_cli.md`.)
