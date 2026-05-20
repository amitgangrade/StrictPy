# StrictPy v0.2 — Release Notes

**Release date**: 2026-05-21
**Tag**: `v0.2.0`
**Span**: M0 → M30 (30 milestones, 5 calendar days)
**Status**: First "frozen" release. Zero known correctness bugs. Ready
for use as a research demonstration / teaching artifact / personal
project foundation. Not production-grade — see §"Known v0.2 limits"
below.

---

## What v0.2 is

A statically typed Python dialect, compiler, and bytecode VM with a
Cranelift JIT, written in Rust from scratch in 5 calendar days using
AI-orchestrated development. The implementation hosts a working
HTTP/1.1 + HTTPS web framework written in StrictPy on top of its own
stdlib.

If v0.1 is the *design* (the 1,813-line `STRICTPY_SPEC.md` frozen on
day one), v0.2 is the *implementation* — everything M0 added to that
design over the next 4½ days: a compiler frontend (lexer / parser /
resolver / typechecker / IR / optimiser), a bytecode VM, a
mark-and-sweep GC, the Cranelift AOT JIT, 36 stdlib modules, and 96+
example programs including a real web framework.

---

## Headline numbers

| Metric | v0.2 |
|---|---:|
| Tests passing | **656** (0 failed, 1 ignored) |
| Bugs found over the project | 35 |
| Bugs fixed | **35** |
| Bugs deferred | **0** |
| Stdlib modules | 36 |
| Example programs | 96+ |
| Workspace LOC (Rust) | ~36,000 |
| StrictPy example LOC | ~13,400 |
| Benchmark wins (canonical 16-cell suite) | 16/0/0 |
| Benchmark wins (extended 30-cell suite) | 28/2/0 |
| fib(30) | 13.1 ms (~12× faster than CPython 3.12) |
| Web framework throughput | ~2,200 req/s HTTP, ~800 req/s HTTPS |

---

## What v0.2 ships

### Language

Statically typed Python syntax with mandatory annotations and concrete
numeric types. Spec at [`STRICTPY_SPEC.md`](STRICTPY_SPEC.md).

Major language features delivered across M0–M30:

- **M0–M2**: Spec, lexer, parser, resolver, bidirectional type checker,
  20-case negative-conformance suite.
- **M3–M4**: SSA-ish IR, optimisation passes, `.spyc` bytecode format,
  register-based interpreter, mark-and-sweep GC.
- **M5–M7**: Stdlib runtime (channels, file IO, dicts, math, real OS
  threading), `is_native` class flag for runtime-class dispatch.
- **M8–M9**: Cranelift AOT compilation. fib(30) drops 931 ms → 13.5 ms
  (64× over the interpreter, 11× faster than CPython). Full JIT
  coverage extended to heap mutation, fields, virtual calls, allocation.
- **M11**: Class system overhaul — sealed/open hierarchies, subclass
  field layout, vtable inheritance fixes, `op_new` `class_id`↔`type_id`
  collision repair.
- **M13–M17**: Five language features in five milestones — short-circuit
  `and`/`or` (M13), tuples + destructuring (M14), try/except/finally +
  raise (M15), isinstance + match-case with flow narrowing (M16),
  generic free functions with call-site monomorphisation (M17).

### Stdlib

36 modules across five phases:

- **Phase 1** (M19–M21): `sys`, `os`, `path`, `io`, `time`, `random`,
  `math`, `json`, `re`.
- **Phase 2** (M22): `argparse`, `collections`, `csv`, `base64`,
  `hashlib`, `itertools`, `statistics`, `struct`, `urllib_parse`.
- **Phase 3a** (M23): `subprocess`, `pathlib`, `datetime`, `threading`
  (Lock + Semaphore), `queue` (PriorityQueue), `sqlite3`.
- **Phase 3c** (M27): `shutil`, `tempfile`, `glob`, `fnmatch`, `gzip`,
  `zlib`, `bz2`, `zipfile`, `tarfile`, `logging`.
- **Phase 3b** (M28 + M28.5): `socket`, `ssl` (bidirectional —
  client + server side TLS), `http_client`.

### Toolchain

- **Single unified `spy` CLI** (M25): `spy script.spy` compiles-if-stale
  + runs, with bytecode cached in `__spycache__/`. `spy script.spyc`
  runs precompiled. `spy -c "code"` inline. `spy --compile-only`
  for explicit compile workflows. Python-analogous shape.
- **Benchmark harness** (`bench/harness.py`): canonical 16-cell suite +
  extended 30-cell suite (`--extended`). 8 historical snapshots
  preserved in `bench/history/`.

### Real-software demonstration

`examples/webserver/todo_app.spy` (2,443 LOC): a complete HTTP/1.1 +
HTTPS web framework + TODO API demo written **in StrictPy user code on
top of the language's own stdlib**. Features:

- Sinatra/Flask-shaped Request/Response/Handler/Router/Server design
- HTTP keep-alive with 5s idle timeout
- Chunked transfer encoding (both directions)
- `multipart/form-data` parsing for file uploads
- HTTPS via M28.5 server-side TLS (`--tls cert key` flag)
- Graceful shutdown via `--shutdown-after-secs N`
- HTML 4xx/5xx error pages
- SQLite-backed TODO persistence
- Access logging via `logging` stdlib

Performance: ~2,200 req/s HTTP, ~800 req/s HTTPS, ~1,500 req/s with
SQLite-backed queries — within 2× of Flask+gunicorn for an
equivalent workload. The gap is the async event loop (deferred to v0.3).

---

## Performance summary

### Canonical 4-program benchmark (16 cells, vs CPython 3.12.10)

**16 wins, 0 ties, 0 losses.** Headline numbers (best-of-3 wall-clock):

| Benchmark | StrictPy | CPython 3.12 | Speedup |
|---|---:|---:|---:|
| fib(30) | 13.1 ms | 159.5 ms | **12×** |
| fib(33) | 34.8 ms | 537.7 ms | **15×** |
| quicksort(100K) | 18.6 ms | 238.6 ms | **13×** |
| dot product (1M f64) | 54.0 ms | 239.1 ms | **4×** |
| Mandelbrot 60×30 | 13.6 ms | 56.6 ms | **4×** |

### Extended 30-cell suite (M26)

**28 wins, 2 ties, 0 losses.** Pure-compute (5 programs at 3 sizes):
n-queens, Sieve of Eratosthenes, matrix multiply, binary tree
insertion, heap sort. Stdlib (5 programs at 3 sizes): JSON round-trip,
regex throughput, SHA-256, CSV parse, SQLite. Full report in
[`bench/EXTENDED_REPORT.md`](bench/EXTENDED_REPORT.md).

### Web framework (M29)

| Endpoint | HTTP req/s | HTTPS req/s |
|---|---:|---:|
| `/health` (no I/O) | ~2,200 | ~800 |
| `GET /api/todos` (1 SQLite query) | ~1,500 | ~700 |
| `POST /api/todos` (1 SQLite insert) | ~1,100 | ~600 |

---

## Known v0.2 limits (all deferred to v0.3)

These are unimplemented features with documented rationale, not bugs:

- **No async I/O / event loop.** All networking blocks; concurrency
  via OS threads. The ~2× gap to Flask+gunicorn measured in M29 is
  exactly this.
- **GC paused during JIT'd execution** (`in_jit: AtomicUsize`).
  Long-running programs with JIT'd hot loops and >16 MB live data
  will stall. The M26 `btree(10k)` row is the empirical witness.
  Precise Cranelift stack maps will fix this.
- **No generic classes** (`class Box[T]:`). Generic *free functions*
  work since M17. Generic classes are the single highest-leverage
  v0.3 ergonomic win — would shrink the M29 framework ~30%.
- **No stdlib classes.** Typed `JsonValue` / `Request` / `Response` /
  `Logger` / `re.Pattern` / `sqlite3.Connection` — all blocked on
  generic classes.
- **No user-defined exception subclasses.** v0.2 ships 10 built-in
  names (`Exception`, `ValueError`, `IOError`, `ZeroDivisionError`,
  etc.).
- **No bounded generics** (`T: Comparable`).
- **No HTTP/2, no WebSockets.** v0.2 is HTTP/1.1 only.
- **No production-grade password hashing** (bcrypt / argon2).
- **No `traceback`, `enum`, `functools`, `uuid`, `secrets` stdlib
  modules** (Phase 3d — quality-of-life modules deferred to v0.3).
- **`with open(...)` doesn't route IOError through enclosing
  try/except** — workaround is explicit `try: with ... except:`.
- **No NumPy / pandas integration.** Architectural; see
  [`docs/thesis/design_decisions/why_no_numpy_pandas.md`](docs/thesis/design_decisions/why_no_numpy_pandas.md).

---

## Reproducing v0.2

```powershell
git clone --branch v0.2.0 https://github.com/amitgangrade/StrictPy
cd StrictPy
cargo build --release

# Run a program
./target/release/spy.exe examples/fib.spy

# Full canonical benchmark suite
python bench/harness.py

# Extended 30-cell benchmark suite
python bench/harness.py --extended

# Test suite (656 tests, 0 failures, 1 ignored)
cargo test --workspace --release

# The M29/M29.5 web framework, HTTP mode
./target/release/spy.exe examples/webserver/todo_app.spy --port 8080

# HTTPS mode (you supply cert.pem and key.pem; see
# compiler/tests/webserver_demo_runs.rs for the rcgen-based test pattern)
./target/release/spy.exe examples/webserver/todo_app.spy --port 8443 --tls cert.pem key.pem
```

---

## Companion documentation

| Document | What it is |
|---|---|
| [`STRICTPY_SPEC.md`](STRICTPY_SPEC.md) | The language and VM spec (1,813+ lines; in-place amendments per milestone) |
| [`THESIS.md`](THESIS.md) | Mid-form technical thesis (~10,900 words) synthesising the project archive |
| [`BLOG_POST.md`](BLOG_POST.md) | Narrative-style blog post (~10,100 words) for a broader audience |
| [`README.md`](README.md) | Build instructions and what runs today |
| [`bench/BENCH_REPORT.md`](bench/BENCH_REPORT.md) | Canonical 16-cell benchmark report |
| [`bench/EXTENDED_REPORT.md`](bench/EXTENDED_REPORT.md) | Extended 30-cell benchmark report (M26) |
| [`docs/thesis/`](docs/thesis/) | The full project archive — per-milestone notes, 47 agent reports, 35-bug catalogue, design decisions, benchmark history |

---

## Methodology contributions worth recording

Beyond the language artifact, v0.2 contributed three reproducible
methodology findings to the AI-orchestrated-systems-engineering record
(documented in `THESIS.md` §5 and `BLOG_POST.md`):

1. **Numerical thresholds in agent briefs beat qualitative urgency.**
   The brief language "FIRST commit before 60% of your time budget"
   (with explicit 20%/40%/60%/80% checkpoint suggestions) replaced
   "commit early" and produced **10 consecutive clean agents** across
   M28–M30 — vs. 7+ failures across M23–M27 under the qualitative
   version of the same instruction. The intervention is reproducible.

2. **Stress tests find integration bugs unit tests can't.** Demonstrated
   twice in late milestones (BUG-039 from M24's `event_log.spy`
   exercising `key in dict`; BUG-040 from M29.5's graceful-shutdown
   path exercising `close_listener` during a blocked `accept`). The
   "real programs use APIs in combinations unit tests don't"
   mechanism is structural; the stress-test ROI curve flattens but
   never permanently reaches zero.

3. **Placeholder IR lowerings are a recurring failure mode.** Four
   instances in the catalogue (BUG-008 `is not`, BUG-034 `str !=`,
   BUG-037 `??`, BUG-039 `in`/`not in`). All four share the same
   shape: a binary-op match arm in `compiler/src/ir.rs::emit_binop`
   that punts on type-dependent lowering with a hardcoded `IROp`. A
   mechanical audit of the file (30–60 min) would have caught all
   four at once; this is a v0.3 hygiene task.

---

## What comes next (v0.3 menu)

Priority order by leverage:

1. **Generic classes** (`class Box[T]:`). Unblocks typed stdlib
   classes — typed `JsonValue` (~30% framework LOC reduction),
   `Request`/`Response` (~20% more), `re.Pattern`, etc.
2. **Async I/O / event loop.** Closes the ~2× gap to production
   Python web stacks measured in M29.
3. **Precise GC stack maps.** Eliminates the `in_jit` pause limit;
   removes the M26 `btree`-at-large-n result.
4. **Phase 3d stdlib** — `traceback`, `enum`, `functools`, `uuid`,
   `secrets`.
5. **User-defined exception subclasses.**
6. **Bounded generics.**
7. **HTTP/2 + WebSockets.**

---

## Acknowledgements

Project conceived and orchestrated by Amit Gangrade. All compiler,
VM, JIT, and stdlib implementation work done by Claude Code (Claude
Opus 4.7), running under the Claude Agent SDK. The 47 agent reports
preserved in [`docs/thesis/agent_reports/`](docs/thesis/agent_reports/)
document every substantive task, brief, and finding individually.

The spec frozen on day one ([`STRICTPY_SPEC.md`](STRICTPY_SPEC.md))
was the single highest-leverage decision — every subsequent agent
task could be briefed by pointing at a section. The project
methodology section in `THESIS.md` §5 documents this and the other
patterns that worked (and didn't) at length.

v0.2 is the freeze point. v0.3 begins with M31.
