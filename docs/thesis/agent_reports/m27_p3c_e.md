# M27 P3c-E — `logging` stdlib module

**Brief**: Ride on the M19 stdlib-module-table infrastructure (now
twenty-something modules in) to ship a `logging` surface for v0.2. The
Pythonic shape is class-heavy (named `Logger`s, `Handler` stacks,
`Formatter` instances), all of which depend on stdlib-class registration
that's still v0.3 work. v0.2 ships a **single global logger** with a
runtime level filter, an optional file sink, and the canonical CPython
default record format. Eleven NativeFn handlers, two per-instance
`SharedVm` fields, one spec section.

**Wall-clock**: ~1.5h (read-through + 11 native handlers + 2 SharedVm
fields + ~120 LOC demo + integration test + spec).
**Files changed**: 4 source files (`shared/src/native.rs`,
`compiler/src/resolver.rs`, `vm/src/interp.rs`, `vm/src/builtins.rs`) +
1 spec section + 1 demo + 1 test + 1 report.
**Tests (this module)**: 1 compile + 1 subprocess = **2 new tests**.
The demo carries 24 internal asserts.

## What `logging` adds vs. `io.write_stderr`

The v0.1 / pre-M27 way to emit diagnostics was `io.write_stderr(s)`
(M20a — string in, bytes out, no metadata, no filtering). That works
for one-off prints; it doesn't compose into the logger-with-thresholds
pattern that every long-lived program eventually grows. `logging` adds
three things on top:

| Capability | `io.write_stderr` | `logging` |
|---|---|---|
| Per-message severity tag | manual prefix | enum (DEBUG / INFO / WARNING / ERROR / CRITICAL) |
| Runtime level filter | none | `set_level` + `is_enabled_for` |
| File sink option | hand-rolled | `basic_config_to_file` |
| Per-record timestamp | manual | automatic ISO 8601 UTC |
| Format consistency | per-call discipline | fixed record format |

The format pin matches CPython's default
`%(asctime)s %(levelname)s %(message)s`:

```
2026-05-20T13:42:55Z INFO Some message here
```

Every record ends with one `\n`. This is the format you get out of a
CPython script that calls `logging.basicConfig(level=logging.INFO)` and
nothing else — the most-common 30-line Python script shape.

## Module decisions

### Why a flat module surface (no Logger / Handler classes)

The natural Python shape is `logger = logging.getLogger("app.db")` —
each named logger has its own threshold, handler stack, and propagation
parent. v0.2 doesn't have stdlib-class registration yet (same blocker
M20c hit for `JsonValue`, M22 P2A for `ArgParser`, M23 P3a-B for
`DateTime`). The escape hatch the existing stdlib uses is *flat
functions* over an opaque handle or per-process state. Following that
pattern, `logging` ships a **single global logger** whose threshold and
sink live as per-instance state on `SharedVm`:

```rust
pub log_level: std::sync::atomic::AtomicI32,
pub log_file:  std::sync::Mutex<Option<std::fs::File>>,
```

Cost of this choice: no per-module filtering (every emit hits the same
threshold); no multiple handlers (you get either stderr OR one file,
not both); no `logging.exception(...)` (would need exception/traceback
introspection, also v0.3). Benefits: zero infrastructure changes; the
implementation is 11 short native handlers; v0.3 can ship a typed
`Logger` class without breaking call sites (`logging.info(msg)` →
`getLogger("root").info(msg)` is a mechanical pass).

### Why two `basic_config` entry points instead of `basic_config(level, filename=None)`

CPython's `logging.basicConfig` accepts an optional `filename=` kwarg.
v0.2 stdlib functions don't ship default arguments — every other
stdlib module either splits into variants (M20b `random.choice_i64` /
`choice_f64` / `choice_str`) or always-passes-all-args. Following the
variant pattern:

* `basic_config(level: str) -> None` — stderr sink.
* `basic_config_to_file(level: str, filename: str) -> None` — file sink.

A v0.3 `basic_config(level, filename=None)` is a single-name merge once
default args are in.

### Level integer constants exactly match CPython

The brief was explicit: `log_level: AtomicI32` holds CPython's
integer levels (10 / 20 / 30 / 40 / 50). That decision is load-bearing:
when v0.3 ships a Python-shaped `logging.INFO` const, the value will be
`20` — same as what we store now. Programs that read or write the
threshold via either name (the integer or the string) see consistent
behaviour across the v0.2 → v0.3 boundary.

A minor compromise: the *string* names are case-insensitive in v0.2
(`"info"` == `"INFO"`). CPython's `getLevelName` is case-sensitive but
treats `"WARN"` and `"FATAL"` as aliases for `"WARNING"` and
`"CRITICAL"`. v0.2 accepts both the case-insensitive form AND the
aliases — the cost of laxity is zero (one `to_ascii_uppercase` call
per `set_level` / `log` / `is_enabled_for`) and the savings are real
("did I write `info` or `INFO`?" is a common typo).

### Default level is `WARNING`, not `NOTSET`

CPython's pre-`basicConfig` level is `WARNING` (30). Same here — calling
`get_level()` before any `basic_config` returns `"WARNING"`. This is
the threshold that makes `logging.warning(...)` visible "for free" with
no setup, which is what most one-off scripts want.

`NOTSET` (level 0 in CPython) is *not* a valid v0.2 input — it's the
"inherit from parent" sentinel in the class hierarchy, and v0.2 has no
hierarchy. `set_level("NOTSET")` raises `ValueError`.

### Thread-safety: per-record atomicity on the file path

The file sink is `Mutex<Option<File>>`. Each emit:

1. Loads `log_level` (atomic, no lock).
2. Early-returns if `level < threshold`.
3. Formats the whole record into one `String`.
4. Takes the file mutex briefly, does one `write_all`, drops the lock.

The `write_all` is a single syscall on the OS side, so two concurrent
emits on the file path produce two complete records — never half of A
interleaved with half of B. On the stderr path we skip the mutex
entirely (the OS-level stderr is line-atomic for the short writes the
record format produces). The total locking per emit is one short
critical section over the file `Option`; no blocking ever happens
inside the format string.

### Timestamp: hand-rolled, not a dependency on `time` / `datetime`

The timestamp goes through `format_epoch_iso` (the same helper M20b's
`time.format_iso` and M23 P3a-B's `datetime.to_iso` already use). I
deliberately did *not* call into the `time` or `datetime` native
handlers — `logging` is a sibling module, not a dependent. Calling
`SystemTime::now()` directly and then `format_epoch_iso(secs)` keeps
the import graph flat: a program that `import logging`s shouldn't
pay for `import time` and `import datetime` transitively.

## Files modified + LOC (approximate)

| File | Lines added | Purpose |
|---|---|---|
| `shared/src/native.rs` | +56 | 11 NativeFn variants (550-560) + `from_u32` arms |
| `compiler/src/resolver.rs` | +98 | One `StdlibModule` registration |
| `vm/src/interp.rs` | +14 | `log_level` + `log_file` SharedVm fields + ctor wiring (×2) |
| `vm/src/builtins.rs` | +205 | 11 handlers + `level_str_to_int` + `level_int_to_str` + `log_emit` + `current_utc_iso` |
| `STRICTPY_SPEC.md` | +95 | §9.39 |

Plus:

* `examples/logging_demo.spy` — ~190-line demo with 24 internal asserts.
* `compiler/tests/logging_demo_runs.rs` — compile + subprocess assertions
  (stdout: OK banner + pretty-print lines; stderr: WARNING/ERROR
  visible at the stderr-sink path).

NativeFn IDs consumed: **11 out of 20** in my assigned 550-569 range
(561-569 reserved for v0.3 named-logger / formatter / rotating-file
work).

## Hardest three things (in retrospect)

1. **Lock-ordering for the file sink.** First cut held the
   `log_file` mutex across the stderr write path too — `let mut sink
   = ...lock()`; `match sink { Some(f) => write to f, None => write to
   stderr }`. That's wrong: a future refactor that adds a "swap the
   sink at runtime" operation would deadlock if the swap path needed
   to read stderr (e.g. to print "switching to file" itself). The fix
   was an explicit `drop(sink)` in the `None` arm before grabbing the
   stderr lock — cleanly separates the two locks. With one sink it
   doesn't matter; with two it would.

2. **Case-insensitivity vs. CPython exactness.** CPython's
   `getLevelName("info")` returns the literal string `"info"` (the
   level-name table is keyed by exact case). I chose to make
   `set_level("info")` work and have `get_level()` always return the
   canonical upper-case name — the v0.2 ergonomic win felt worth the
   semantic divergence. Documented in spec §9.39 so the v0.3 typed-
   Logger pass can decide whether to keep the lenient behaviour.

3. **Where to put `log_level` initialisation.** Two `SharedVm`
   constructors (`new` and `new_with_jit`), each materialised at
   process start. Forgot to wire `log_level` / `log_file` into the JIT
   variant on the first pass; the `cargo build --features jit` arm
   would have flagged it but the workspace test run hit the non-JIT
   path. Added the wiring to both and verified both constructors
   compile (one #[cfg(feature = "jit")] gate covers the JIT-only
   ctor).

## Incidentally-discovered issues

Zero. The M19 stdlib-module-table absorbed the eleventh-ish module
without any change to resolver / typecheck / IR / codegen layers. The
only file outside the expected four (native.rs, resolver.rs,
builtins.rs, plus interp.rs for per-instance state) that I considered
touching was — none. The same pattern M19 set up with `sys_argv_cache`
keeps holding: per-instance state on `SharedVm` lets a stdlib module
ship without disturbing anything else.

## Cross-platform notes

* **Windows + Unix**: `std::fs::OpenOptions::new().create(true).append(true)`
  works identically on both. The `write_all` syscall is short enough
  that line-atomicity is preserved within a single record on every
  filesystem the workspace targets.
* **Stderr**: `std::io::stderr().lock()` is portable; the lock is per-
  process (the OS-level stderr handle), so concurrent threads emit one
  record per `write_all` but the order across threads is non-
  deterministic. This matches CPython's `logging` — concurrent
  `logger.warning(...)` calls from two threads may print in either
  order, and that's fine for the diagnostics use case.
* **WASM / other**: no platform-specific code, so nothing to gate.

## What's next (v0.3)

* **Named loggers** — `logging.getLogger("app.db")` returns a typed
  `Logger` handle with its own threshold and a `propagate` parent
  pointer. Needs stdlib-class registration.
* **`logging.exception(msg)`** — emit at ERROR with the active
  exception's traceback appended. Needs traceback introspection
  (M15 has the exception value; the call-stack frame data is there
  but not yet exposed as a stdlib-readable shape).
* **Custom format strings** — `basic_config(format="%(name)s ...")`.
  Needs a small format-string mini-parser; CPython's format codes
  (`%(asctime)s`, `%(levelname)s`, etc.) are the canonical set.
* **Rotating file handlers** — `RotatingFileHandler(filename,
  maxBytes, backupCount)`. Pure Rust implementation, but the surface
  is class-shaped so it waits for stdlib classes.
* **Multiple handlers** — `addHandler` / `removeHandler`. Needs the
  Handler class.

The pattern from M19 ("a stable module-table makes new modules trivial
to ship") continues. Eleven NativeFn handlers, two SharedVm fields,
one spec section, ~190 LOC of demo, two new tests. Workspace baseline
preserved; the new module slots in alongside the existing two-dozen
stdlib modules with no infrastructure churn.
