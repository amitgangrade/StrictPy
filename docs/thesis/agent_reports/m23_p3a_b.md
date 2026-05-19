# M23 P3a-B — `datetime` stdlib module

**Brief**: Ride on the M19 stdlib-module-table infrastructure that
M20a/M20b/M20c/M22 have now pushed through nine times to ship the
tenth-tier add: a `datetime` module with calendar arithmetic,
timezone-aware moments, and ISO 8601 parse/format.  The `time`
module from M20b is the prerequisite layer: it ships `time.now() ->
f64`, `time.format_iso(secs) -> str`, and the hand-rolled Howard
Hinnant `civil_from_days` epoch-day → civil-date helper.  The
`datetime` module is the calendar layer **on top of** that — same
epoch primitives, richer surface.

**Wall-clock**: ~1.5 hours (read-through + one module registration +
~22 native handlers + ~250 LOC of pure-Rust helpers + 26 in-process
tests + 2 subprocess tests + 1 example + spec).
**Files changed**: 4 source files + 1 spec section + 1 agent report +
1 new example + 2 new test files.
**Tests (this module)**: 26 in-process + 2 subprocess = **28 new
tests**, all green.  Workspace baseline 468 → preserved.

## What `datetime` adds vs. `time`

M20b's `time` is the epoch layer — wall clock (`time.now() -> f64`),
monotonic (`time.monotonic() -> f64`), sleep, and a one-way
epoch-to-ISO formatter (`time.format_iso(f64) -> str`).  That's the
*"how long ago"* surface.

`datetime` is the calendar layer — *"what date is it"*.  Concretely:

| Question | M20b answer | M23 P3a-B answer |
|---|---|---|
| What's the timestamp now? | `time.now()` (f64 secs) | `datetime.now()` (i64 secs) |
| Print a timestamp | `time.format_iso(secs)` | `datetime.to_iso(dt)` (+ `to_date_str` / `to_time_str`) |
| Parse a timestamp | — | `datetime.from_iso(s)` |
| Build a moment from civil date | — | `datetime.from_ymd(y, m, d)` / `from_ymd_hms(...)` |
| Component extraction | — | `year` / `month` / `day` / `hour` / `minute` / `second` / `weekday` / `ymd` |
| Calendar arithmetic | — | `add_seconds` / `add_days` / `diff_seconds` / `diff_days` |
| Timezone | implicit UTC | `local_offset_minutes()` for display conversion |

The two modules share the underlying Howard Hinnant
`civil_from_days` algorithm — M20b uses it for the
epoch-seconds → "YYYY-MM-DDTHH:MM:SSZ" path; M23 P3a-B reuses it
in `epoch_to_ymd` (for component extraction) and pairs it with its
inverse `days_from_civil` (newly added in this milestone) for the
civil-date → epoch direction.  Twelve lines of public-domain
arithmetic each, no `chrono` / `time` crate dependency.

## Module decisions

### Why integer seconds, not a `DateTime` class

The natural Python shape would be a `datetime.datetime` class with
attributes.  v0.2 doesn't have stdlib-class registration yet — the
same blocker M20c hit for `JsonValue` and M22 hit for `ArgParser`.
The escape hatch the existing stdlib uses is *primitive handles*:
make every value a plain primitive (`i64` for argparse handles,
`Dict[str, i64]` for `Counter`).  Following that pattern, both
`DateTime` and `Duration` are `i64` in v0.2 — unix epoch seconds.

Cost of this choice: no method-style calls (`dt.year()` becomes
`datetime.year(dt)`), no class-level type discrimination
(a `DateTime` and a `Duration` are the same Rust type), no
sub-second precision.  Benefits: zero infrastructure changes;
arithmetic uses the same i64 ops as everything else; v0.3 can
ship a typed `DateTime` sealed class without breaking call sites
(rename `datetime.year(dt)` → `dt.year()` in a single mechanical
pass).

### Why `local_offset_minutes()` instead of TZ-aware DateTimes

The full Python semantics — timezone-aware `datetime` with named
zones like `"America/New_York"` — requires tzdata.  Either ship a
copy of `zoneinfo` (~1MB) or pull in the `chrono-tz` crate (~600KB
including data).  Both blow up the vm-crate binary for one feature.

The compromise is one helper: `local_offset_minutes()` returns the
*process-local* TZ offset.  Programs that need to render a UTC
DateTime in local time call `add_seconds(dt, off * 60)` themselves;
programs that need cross-zone arithmetic punt to v0.3.  The brief
called this out as the right scope-down, and it lands cleanly: 22
native handlers cover everything in the assigned API.

### Platform code for `local_offset_minutes()`

The brief allowed shipping `0` with a TODO if the platform code
didn't fit the time budget.  It did fit — both Windows
(`GetTimeZoneInformation`) and Unix (`localtime_r`) expose the
offset via `extern` FFI that doesn't need a crate.  Inline FFI
declarations (`extern "system"` for Windows, `extern "C"` for Unix)
declare just the struct fields we read.  On unsupported targets
(wasm32, etc.) we fall through to a `0` return — same surface, less
useful answer, but the program still compiles.

Caveat: the value reflects *current* TZ.  DST transitions during
the program's lifetime are not retroactively tracked.  For v0.2
this matches Python's `time.timezone` (a constant captured at
import time) — good enough for the "render the wall clock" use case
that's 95% of why anyone reaches for this function.

### ISO 8601 parser scope

The brief listed the common forms — `"YYYY-MM-DDTHH:MM:SSZ"`,
`"...+00:00"`, `"YYYY-MM-DD"` — and said "reject the rest with
ValueError".  The implementation accepts a slightly broader set
because the alternatives are essentially free:

* `"YYYY-MM-DDTHH:MM:SS"` (naive — treated as UTC).
* `"YYYY-MM-DD HH:MM:SS"` (space separator — common variant).
* `"...+HHMM"` / `"...+HH:MM"` for the offset suffix (both forms
  accepted).
* Negative-year dates `"-YYYY-MM-DD"` for BCE — same proleptic
  Gregorian calendar.

Not accepted: fractional seconds, week numbers (`"2026-W21-2"`),
ordinal dates (`"2026-139"`).  Those raise `ValueError`.

### Tuple return for `ymd`

`datetime.ymd(dt) -> Tuple[i32, i32, i32]` returns all three civil
components in one call.  The implementation packs three `i32` slots
through `interp.alloc_tuple_obj`, the same path M20a established
for `path.splitext` and M20c reused for `re.find`.  No new
infrastructure — just three slots packed into one allocation, the
typecheck layer's existing `Ty::Tuple` machinery handles the field
access (`t.0` / `t.1` / `t.2`) at the call site.

## Files modified + LOC (approximate)

| File | Lines added | Purpose |
|---|---|---|
| `shared/src/native.rs` | +70 | 22 new `NativeFn` variants (390–411) + `from_u32` arms |
| `compiler/src/resolver.rs` | +175 | One `StdlibModule` registration |
| `vm/src/builtins.rs` | +400 | 22 handlers + `days_from_civil` + `ymd_to_epoch_seconds` + ISO parser + platform offset FFI |
| `STRICTPY_SPEC.md` | +80 | §9.24 |

Plus:

* `examples/datetime_demo.spy` — 100-line demo with 13 internal
  asserts.
* `vm/tests/m23_p3a_b_datetime.rs` — 26 in-process tests.
* `compiler/tests/datetime_demo_runs.rs` — 2 subprocess tests via
  `spy.exe`.

NativeFn IDs consumed: **22 out of 30** in my assigned 390–419
range (412–419 reserved for v0.3 — named TZs, fractional seconds,
strftime/strptime).

## Hardest three things (in retrospect)

1. **The signed-arithmetic edge cases.**  Two places had to be
   right for pre-1970 dates to work.  First, `epoch_to_ymd` uses
   `secs.div_euclid(86400)` (not `/`) so a negative second count
   yields the *previous* day's epoch-day rather than truncating
   toward zero.  Second, `weekday` uses `(days + 3).rem_euclid(7)`
   for the same reason.  The test `weekday_handles_pre_epoch_correctly`
   pins the regression: 1969-12-31 was a Wednesday, and a naive
   `%` operator would have returned a negative weekday on the i32
   wrap.

2. **`run_file_capture` panics on uncaught exceptions.**  The VM
   test helper returns `Result<(i32, String), VmError>`, and the
   `Err` arm fires for `UncaughtException` (not just for
   compile / I/O errors).  My first cut at the error-case tests
   used `.expect("run")` and saw the helper panic *inside* the
   ValueError raise — not as a runtime fail, but as a test panic.
   The fix is the `assert_raises_value_error` helper that matches
   on `VmError::UncaughtException { type_name, message }` and
   asserts on the type name + message content.  Same pattern the
   M20b factorial-overflow test uses.

3. **TZ offset sign confusion.**  `GetTimeZoneInformation` and
   ISO 8601 use opposite sign conventions.  Windows reports
   `Bias` such that `UTC = local + Bias` (PST: `Bias=480`), but
   ISO 8601 and Python's `tm_gmtoff` use `local - UTC` (PST:
   `-480` minutes / `-28800` seconds).  The first cut of
   `parse_iso_tz_suffix` had `+5:00` and `-5:00` producing the
   *same* UTC instant (because I was adding the offset both
   ways).  The fix was to read the ISO standard carefully:
   "local time = UTC time + offset" — so `utc = local - offset`,
   not `local + offset`.  The test
   `from_iso_with_negative_offset` pins it: a `-05:00` suffix on
   a local timestamp gives a UTC instant *5 hours later*.

## Incidentally-discovered issues

Zero.  The M19 → M22 trend held — the stdlib-module-table
infrastructure absorbed the tenth module without any change to
resolver / typecheck / IR / codegen.  The only file outside the
expected four (native.rs, resolver.rs, builtins.rs, Cargo.toml-or-
spec) that I considered touching was `vm/src/interp.rs` for
per-interpreter state (the way M19 added `sys_argv_cache` and
M20b added `monotonic_start` / `random_lcg_state`).  `datetime`
needs no per-process state — every call is a pure function of its
arguments — so `interp.rs` stays untouched.

## Cross-platform notes

* **Windows** (`target_os = "windows"`): `GetTimeZoneInformation`
  via inline `extern "system"` FFI.  Returns 0 minutes if the API
  call fails (rare — only on broken installations).
* **Unix family** (`target_family = "unix"`, covers Linux + macOS +
  BSD): `localtime_r` via inline `extern "C"` FFI.  Reads
  `tm_gmtoff` (a `long` — 64-bit on LP64, which is every
  vm-supported target).
* **Other** (wasm32, embedded): falls through to constant `0`.
  Programs are documented as treating `0` as "UTC fallback".

The two platform-conditional bodies live in `vm/src/builtins.rs`
behind `#[cfg(target_os = "windows")]` and
`#[cfg(target_family = "unix")]`.  No platform-specific code in any
other file — `cargo build --workspace --release` on either
platform takes the same code path through everything except those
two function bodies.

## What's next

* **v0.3 typed `DateTime` class** — when stdlib classes ship,
  rename `datetime.year(dt)` → `dt.year()` etc.  Call sites change
  mechanically; the underlying NativeFn ids stay the same.
* **`time.timedelta` shape** — currently `Duration` is an `i64`
  (seconds).  A typed `Duration` class could expose
  `.total_seconds()` / `.days` / `.microseconds`.  Until then,
  arithmetic on plain seconds covers the use cases.
* **Named timezones** — would need the `chrono-tz` crate or a
  bundled `zoneinfo` snapshot.  Defer to v0.3 with the rest of
  the "stdlib gets big" push.
* **`strftime` / `strptime`** — format-string parsing.  v0.2 ships
  fixed ISO 8601 only; v0.3 should ship `%Y-%m-%d`-style format
  strings for log-line interop.

The bet from M19 ("a stable module-table makes new modules trivial
to ship") continues to pay out.  Ten modules now slot into the
same `seed_stdlib_modules` registration with the same vm-side
match-arm-per-id dispatch, and none of them have required changes
to the resolver / typecheck / IR / codegen layers.  The next time
this assumption gets pressure-tested is M23 P3a-D's `sqlite3`
module, which has to bring in libsqlite3 via FFI — a different
class of integration than anything Phase 1 or 2 attempted.
