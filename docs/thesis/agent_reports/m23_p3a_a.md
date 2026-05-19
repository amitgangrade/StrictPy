# M23 P3a-A — `subprocess` + `pathlib` stdlib modules

**Brief**: Phase 3a's first parallel-agent stdlib round.  Cross the line
from "pure Rust + std::fs" into genuine OS-level process control
(`subprocess`) and round-trip pathlib's flat-function API to give the
stdlib parity with Python's `pathlib` discoverability.  ID range
**350-389** (40 ids) of which 20 used (350-355 subprocess, 370-383
pathlib).

**Wall-clock**: ~2.5 hours (read-through SHARED_BRIEF + M20a + M22 P2C/
P2D reports + module registrations + ~450 LOC of native handlers + 18
in-process tests + 4 subprocess tests + 2 example programs + spec
§9.24-§9.25).
**Files changed**: 4 source files + 1 spec section + 1 agent report +
2 new examples + 3 new test files.
**Tests**: 468 baseline (post-M22) + **22 new** (18 in-process VM + 4
subprocess) + 2 incidental example-sweep coverage units = **passing,
0 failing** with the workspace test runner (see "Final test totals"
below).

## The smooth ride continues into OS FFI

M19's stdlib-module-table absorbed everything Phase 1/2 threw at it
(sys/os/path/io/time/random/math/json/re/argparse/collections/csv/
base64/hashlib/itertools/statistics/struct/urllib_parse).  Phase 3a
is the first round where modules step outside "pure-Rust algorithm or
`std::fs` syscall" into genuine OS-level lifecycle territory —
`subprocess` is `std::process::Command` with a *handle table* — and
the M19 infrastructure absorbed it with the same shape as every prior
module.  Zero changes to the resolver's import resolution, zero
changes to `typecheck.rs`, zero changes to `ir.rs`.

The diff:

| File | Lines added | Purpose |
|---|---|---|
| `shared/src/native.rs` | +100 | 20 new `NativeFn` variants + `from_u32` arms |
| `compiler/src/resolver.rs` | +220 | Two `StdlibModule` registrations |
| `vm/src/builtins.rs` | +430 | New dispatch arms + `read_list_str` + `SUBPROCESS_TABLE` helpers |
| `STRICTPY_SPEC.md` | +160 | §9.24 (subprocess) + §9.25 (pathlib) |

## Native ID layout (M23 P3a-A uses 350–383, 20/40 slots used)

| Range | Module | Count |
|---|---|---|
| 350–355 | `subprocess` | 6 |
| 370–383 | `pathlib` | 14 |
| 356–369, 384–389 | reserved (v0.3) | 20 |

20 IDs reserved for v0.3 expansion.  Subprocess's reserved block
covers the obvious v0.3 extensions: env-var injection
(`run_with_env`), streaming-stdout reads, `check_output`-style raise-
on-nonzero, and one slot for "communicate-style" combined stdin+stdout
piping.  Pathlib's reserved block covers `glob`, `iterdir`,
`symlink_to`, and the matching `mkdir` / `unlink` / `resolve` trio
once stdlib classes land in v0.3 and `Path("p").x()` becomes
expressible.

## Design choice 1: opaque `i64` handles into a global process table

The brief flagged this as the central design risk for subprocess:
StrictPy v0.2 has no stdlib classes (M20c flagged it, M22 P2A
flagged it again, M22 P2D flagged it again), so a Pythonic
`Popen.poll()` / `.wait()` / `.kill()` surface can't ship.  The two
options:

* **(a)** Opaque `i64` handle into a VM-owned process registry — same
  shape M5 used for `io.File` (resource table + integer handle).
* **(b)** Skip lifecycle entirely; ship only `run` and
  `run_with_stdin` (the blocking convenience wrappers).

I went with **(a)** because the brief explicitly listed `spawn` /
`wait` / `try_wait` / `kill` as in-scope.  The implementation uses a
global `Mutex<Option<HashMap<i64, std::process::Child>>>` rather than
storing the table per-Interpreter, because `Interpreter::from_shared`
can clone state when M6 threads spawn — and users will want "spawn in
main thread, wait in worker thread" patterns to work.  Handles come
from an `AtomicI64` counter and are never recycled (~300 millennia at
one-spawn-per-microsecond).

A small wart: `try_wait` and `kill` need both the table-lock guard
and the entry-lookup.  The borrow checker forced me to split into an
outer `guard.as_mut()` step and an inner `table.get_mut(&handle)` step
— Rust correctly objects to closing over both the guard and a borrow
into it.  The fix is two lines of plumbing; documented inline.

## Design choice 2: pathlib as a flat-function API (not a class)

The brief explicitly invited a scope-down to flat functions; I took
it.  The Pythonic `Path("a") / "b"` syntax is unbuildable without
stdlib-class registration (deferred v0.3), and the two-arg-function
shape `pathlib.join(a, b)` is a natural fit for the M19
`StdlibModule` registration.

The duplicated aliases (`pathlib.join`, `pathlib.parent`,
`pathlib.name`) overlap M20a's `path` module by design — a program
that `import pathlib` for ergonomic chaining shouldn't also have to
`import path` for the basics.  The runtime handlers route to the
same Rust `std::path::Path` calls; the duplication is purely an
ergonomic / discoverability tool.  v0.3's stdlib-class registration
will collapse all this into a single `Path` class.

## Design choice 3: `stem` / `suffix` follow Python's "last extension only" rule

`splitext_python` (M20a helper, ~12 LOC) was already correctly
implementing Python's "leading dot isn't an extension" rule.
Extending it for the multi-extension case
(`"archive.tar.gz"` → `("archive.tar", ".gz")`) was a one-line
verification — it already worked because `rfind('.')` returns the
LAST dot.  Both `stem(p)` and `suffix(p)` route through
`splitext_python(basename(p))`.

## The cross-platform shell-invocation dance

Subprocess's example program needs to run *something* portably.  The
options I considered:

1. **Call `echo` directly.** ❌ — echo is a shell builtin on Windows
   (not a real executable), so `subprocess.run("echo", ["hi"])`
   fails with "program not found".  The brief explicitly warned
   about this.
2. **Pick a stdlib executable** (e.g. `python --version`) ❌ —
   assumes Python is on PATH, breaking CI hermetism.
3. **Wrap shell commands via `cmd.exe /c <cmd>` (Windows) or
   `sh -c <cmd>` (Unix).** ✅ — the canonical Python pattern when
   you specifically want a shell.  StrictPy's `sys.platform`
   distinguishes the two.

I went with **(3)** in both the example program and the in-process
tests.  The `shell_argv` helper composes the right list-shape per
platform; the rest of the demo is straight subprocess API exercise.

For the stdin-piping test I used `cat` (Unix) / `findstr x*` (Windows)
as platform-equivalent "echo stdin to stdout" filters.  `findstr x*`
matches any line because `x` repeated zero-or-more times trivially
matches everything; chose it over `more` because `more` adds prompt
lines.

## Incidentally-discovered bugs / oddities

**None requiring code changes.**  This is the third consecutive
Phase-N round with no incidental bug discovery (M20c, M22 P2C/D were
the first two).  The M19 stdlib infrastructure has now absorbed 19
modules across 4 milestones without forcing a single resolver or
typecheck change — the design tax M19 paid keeps paying out.

The one thing I almost-but-not-quite tripped on: `str.find` is NOT
in the prelude (despite being mentally obvious from Python).  My
first cut of the subprocess example used `r.1.find("hello")`; the
typechecker rejected it.  I switched to `re.search("hello", r.1)`,
which works.  Worth noting for any future agent — `find`/`contains`/
`startswith`/`endswith` are all v0.3 gaps in the prelude string
surface.

## Hardest three things (in retrospect)

1. **The Rust borrow checker on the subprocess table.**  Wanting to
   say "give me a mutable reference into the HashMap inside the
   Option inside the MutexGuard" requires three sequential reborrows.
   The first cut hoisted the unwrap to the closure level, which
   wouldn't typecheck.  Splitting `guard.as_mut().ok_or(...)?` from
   `table.get_mut(&handle).ok_or(...)?` is the canonical pattern,
   and now it reads cleanly.
2. **`std::process::ExitStatus::code()` returns `None` on Unix
   signal-termination.**  Python's `subprocess` reports negative
   signal numbers for this case; I implemented the same convention
   with a `cfg(unix)` block calling `ExitStatusExt::signal()`.
   Windows always has an integer exit code, so the `cfg(not(unix))`
   path is the unreachable-fallback `-1`.
3. **CRLF normalisation in `read_lines`.**  My first cut split on
   `\n` only, and a CRLF-ended file (`"a\r\nb\r\n"`) read as
   `["a\r", "b\r"]` — every line with a stray trailing `\r`.
   Added a per-element `\r` strip in the loop, and now Windows-
   line-ending files work the same as Unix ones.

## What v0.2 does NOT ship (and why)

* **Subprocess streaming stdin/stdout** — `Popen.stdout.read(...)`
  needs readable byte-stream handles, which v0.3 will land
  alongside the real `bytes` runtime type.  v0.2 programs can
  spawn + capture-all-at-end via `run_with_stdin`.
* **`subprocess.run(env=...)`** — process-local env-var mutation is
  available via `os.set_env` in the parent, which inherits to
  children.  Genuinely scoped env-injection is a v0.3 ergonomics
  feature.
* **Pathlib stdlib classes** — same v0.3 blocker that punted typed
  `JsonValue` (M20c), `ArgParser` (M22 P2A), and `Counter[K, V]`
  (also M22 P2A).  Three modules now want this; the design debt is
  concrete enough to motivate the v0.3 work.
* **`pathlib.glob` / `iterdir` / `match`** — overlap with M20a's
  `os.listdir` + M20c's `re` module; explicit composition is
  workable for v0.2.

## Cross-platform notes

* **Subprocess argument quoting** differs between Windows (which
  re-parses CommandLineToArgvW) and Unix (verbatim).  Rust's
  `std::process::Command::args` handles each host correctly — pass
  arguments as separate `List[str]` elements, not a pre-joined
  string.
* **Path separators**: pathlib's `join`, `with_name`, and `parts`
  use `std::path::Path` methods that emit the OS-native separator.
  The example tests check `len(joined) == 3` (one separator char
  whichever platform) rather than substring-matching on the
  separator itself.
* **`std::process::Child::kill`** returns `Err(InvalidInput)` if the
  child has already exited.  I swallow that error and report
  success — matches Python's `Popen.kill` on an already-dead
  process.  Any other kill-error surfaces as IOError.

## Files modified + LOC (approximate)

| File | Lines added | Purpose |
|---|---|---|
| `shared/src/native.rs` | +100 | 20 `NativeFn` variants (350-355 + 370-383) + `from_u32` arms |
| `compiler/src/resolver.rs` | +220 | `subprocess` + `pathlib` `StdlibModule` registrations |
| `vm/src/builtins.rs` | +430 | 20 dispatch arms + `read_list_str` helper + `SUBPROCESS_TABLE` / `subprocess_table_insert` / `subprocess_table_take` |
| `STRICTPY_SPEC.md` | +160 | §9.24 (subprocess) + §9.25 (pathlib) |

Plus new examples + tests (no conflict with sibling agents):

* `examples/subprocess_demo.spy` — platform-aware shell invocation,
  exercises `run` / `run_with_stdin` / `spawn` / `wait` / bogus-spawn
  IOError.
* `examples/pathlib_demo.spy` — pure-function exercises of join /
  with_suffix / with_name / parent / name / stem / suffix / parts /
  is_absolute / absolute / read_text / write_text / read_lines /
  relative_to.
* `vm/tests/m23_p3a_a_subprocess_pathlib.rs` — 18 in-process tests
  covering happy paths, error paths (IOError on missing program /
  bad handle, ValueError on not-a-subpath), and edge cases (CRLF
  stripping in read_lines, leading-dot stem, multi-extension suffix).
* `compiler/tests/subprocess_demo_runs.rs` — 2 subprocess tests
  (compiles, end-to-end via spy.exe).
* `compiler/tests/pathlib_demo_runs.rs` — 2 subprocess tests
  (compiles, end-to-end via spy.exe).

## Final test totals

* **468 baseline (post-M22) preserved.**
* **+18 in-process** tests (`vm/tests/m23_p3a_a_subprocess_pathlib.rs`).
* **+4 subprocess** tests (`compiler/tests/{subprocess,pathlib}_demo_runs.rs`).
* Plus the auto-sweep `parse_examples` / `typecheck_examples` /
  `compile_examples` over the two new `.spy` files — 2 implicit
  coverage units, no new test files.
* **Total: ~492 passing, 0 failing, 1 ignored.**

## What's next

Two pieces of v0.3 work that the M23 P3a-A arc highlights as
natural:

1. **Stdlib-class registration.**  Promotes pathlib's flat-function
   API to a `Path` class with `__truediv__` for the `Path("a") /
   "b"` chaining, and subprocess's `i64`-handle table to a
   `Process` class with `.wait()` / `.kill()` / `.poll()` methods.
   Same one-shape-fits-all upgrade lands typed JsonValue, ArgParser,
   Counter[K,V] simultaneously.
2. **A real `bytes` runtime type.**  Subprocess's
   `run_with_stdin(..., stdin_data: str)` is mildly awkward when the
   data isn't UTF-8 (e.g. piping a binary protocol).  Real `bytes`
   would let the signature become `stdin_data: bytes`, with `str`
   inputs auto-encoded.

The Phase-3a-A round closes here.  The orchestrator integrates this
worktree (along with P3a-B/C/D's parallel work) onto main with
renumbered spec sections.
