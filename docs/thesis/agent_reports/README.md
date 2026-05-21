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

(M26 was a single-session extended-benchmark addition — no sub-agents.
See `bench/EXTENDED_REPORT.md`.)

| `m27_p3c_a.md` | M27 P3c-A | shutil + tempfile (NativeFns 450-472, §9.30/9.31). Closes the v0.2 gap M24-D documented (no recursive rmdir). `shutil.which` does Windows .exe lookup. |
| `m27_p3c_b.md` | M27 P3c-B | glob + fnmatch (NativeFns 480-486, §9.32/9.33). Uses Rust `glob` crate; `fnmatch.translate` hand-rolled (~50 LOC). |
| `m27_p3c_c.md` | M27 P3c-C | gzip + zlib + bz2 (NativeFns 500-510, §9.34/9.35/9.36). Round-trip + level + checksum (crc32/adler32). **Found and worked around a bzip2 write-side decoder hang** on malformed input; switched to read-side. |
| `m27_p3c_d.md` | M27 P3c-D | zipfile + tarfile (NativeFns 520-535, §9.37/9.38). Uses `zip` + `tar` Rust crates. Two slot tables per format (read/write). Tar writer is enum-typed for plain/gz/bz2 modes. |
| `m27_p3c_e.md` | M27 P3c-E | logging (NativeFns 550-560, §9.39). Flat global-logger surface (v0.3 will add class-shaped Logger/Handler/Formatter on top of stdlib classes). Hand-rolled timestamp formatting via M20b's civil_from_days. |
| `m28_p3b_a.md` | M28 P3b-A | socket (NativeFns 570-588, §9.40). TCP+UDP raw + listen/accept + DNS. No new crate deps. **Self-caught a deadlock** in initial impl (Arc<T> needed instead of holding outer Mutex across blocking I/O); shipped a focused fix-up commit. |
| `m28_p3b_b.md` | M28 P3b-B | ssl (NativeFns 600-609, §9.41). TLS-over-TCP via rustls + ring + webpki-roots. Hand-rolled ~30-LOC CN extractor vs pulling in x509-parser. **Best Lesson 1 discipline of any M28 agent**: green-build checkpoint at ~30% of budget. |
| `m28_p3b_c.md` | M28 P3b-C | http_client (NativeFns 620-649, §9.42). HTTP/1.1 via ureq (bundles rustls + webpki-roots). Stateless handlers, fresh socket per call. Loopback test uses hand-rolled TcpListener-based mock HTTP server. |
| `m28_5_p3b_d.md` | M28.5 P3b-D | Server-side TLS extension to ssl (NativeFns 610-612, §9.41 amended in place). Option A — parallel tls_server_streams table; server handles ≥ 1M, client < 1M. Single agent, clean Lesson 1 + 2 discipline, zero conflicts at integration. |
| `m29_webserver.md` | M29 | **Largest single-program stress test of the project.** 1,446 LOC Sinatra-shaped HTTP/1.1 + HTTPS framework + TODO demo. **Zero new bugs in M28/M28.5 — first stress round with zero finds.** 4 v0.2 ergonomics gaps documented (no JsonValue tree, `from` keyword, `T?` unwrap, BUG-039 for non-str Dict). ~2200 req/s HTTP performance, within 2× of Flask+gunicorn. 4 commits, all before 80% budget. |
| `m29_5_webserver_roundout.md` | M29.5 | Tier 1 round-out of the M29 framework: HTTP keep-alive + chunked TE + multipart + graceful shutdown + HTML error pages. ~1,000 LOC added. **Found BUG-040** (`socket.close_listener` doesn't unblock blocked `accept` — M28 P3b-A handler Arc-clones the listener and drops the mutex before the blocking syscall). Workaround: self-connect from shutdown timer. |
| `m30_bug028.md` | M30 BUG-028 | Lexer line continuation across infix operators. 95-LOC additive fix in `compiler/src/lexer.rs` tracking the last significant token; suppress NEWLINE if it's a binary operator needing a right operand. 11 new regression tests. Last open frontend bug. |
| `m30_bug040.md` | M30 BUG-040 | `socket.close_listener` now wakes blocked `accept()` via belt-and-braces `shutdown(fd, SHUT_RDWR)` + self-connect (with wildcard-bind rewrite to loopback). **Cross-platform finding**: Windows winsock does NOT wake `accept` from `shutdown` alone (Microsoft KB-179942) — self-connect is essential on Windows. |
| `m31_generic_classes.md` | M31 | **First v0.3 feature**: generic classes (`class Box[T]:`, `Pair[K,V]:`, `Stack[T]:`). Extends M17 worklist to classes. Per-instantiation type_id + method bodies. New IR Pass 2.7 + 3.6 running to joint fixpoint with M17's Pass 2.6 + 3.5. Constructor-site inference (no explicit `Box[i64]()` — v0.4). +8 tests. Lesson 1 streak: 11 consecutive clean agents. Unblocks v0.3 stdlib classes (typed JsonValue, Request/Response, re.Pattern). |
