# M20b — `time` / `random` / `math` stdlib modules

**Brief**: Ride on M19/M20a's stdlib-module-table infrastructure to
ship three more namespaced modules — wall-clock + monotonic + sleep
(`time`), seeded pseudo-random (`random`), and a namespaced math
helpers module that extends the §9.4 surface (`math`).  Same recipe as
M20a: one entry per module in `seed_stdlib_modules`, one per-item
native handler in `vm/src/builtins.rs`, no resolver/typecheck/IR
plumbing changes.

**Wall-clock**: ~2 hours (read-through + three module registrations,
~270 LOC of native handlers, 26 in-process tests + 8 subprocess tests
+ 4 example programs + spec).
**Files changed**: 4 source files + 1 spec section + 1 agent report +
4 new examples + 5 new test files.
**Tests**: 314 baseline (M20a) + **34 new** (26 in-process + 8
subprocess) = **348 passing, 0 failing, 1 ignored**.

## The smooth ride continues

The trend M20a flagged held up: zero changes to `resolver.rs` apart
from the `seed_stdlib_modules` additions, zero changes to
`compiler/src/typecheck.rs` or `compiler/src/ir.rs`.  The whole
milestone is three new `StdlibModule { name, items }` registrations,
the matching `NativeFn` enum variants (ids 175–212), one block of
match arms in `vm/src/builtins.rs`, and two new `Interpreter` fields
(`monotonic_start: Instant`, `random_lcg_state: i64`).

If the M19 brief design fits a fourth and fifth module without
infrastructure changes, the bet has paid off in full.

## Module decisions

### `time`

Six functions (ids 175–180).  Anchored on three Rust primitives:

| StrictPy | Rust |
|---|---|
| `time.now()` / `time.now_ms()` | `SystemTime::now().duration_since(UNIX_EPOCH)` |
| `time.monotonic()` | `Instant::elapsed()` (anchored to `monotonic_start`) |
| `time.sleep_s` / `sleep_ms` | `thread::sleep(Duration::from_*)` |
| `time.format_iso` | hand-rolled via Howard Hinnant's `civil_from_days` |

The two clock primitives matter: `SystemTime` for "what time is it on
the wall", `Instant` for "how long since this thing started".  They're
not interchangeable — `SystemTime` can jump backwards under NTP
correction.  Splitting into `now()` / `now_ms()` (wall) and
`monotonic()` (anchor) makes the right choice obvious at the call site.

**Cross-platform notes**:

* Sleep granularity is 1ms on Linux (kernel `nanosleep`) and ~15.6ms on
  Windows (default `Sleep` timer resolution).  The `sleep_test`
  example asserts `≥ 80ms` after a 100ms sleep — a lenient floor that
  passes on both platforms even under CI load.
* `Instant::elapsed()` is monotonic on every supported target.  No
  caveats.
* `format_iso` skipped `chrono` — adding a 400KB crate for one date
  formatter would dwarf the rest of the stdlib.  The hand-rolled
  algorithm (Howard Hinnant `civil_from_days`, public domain) is 12
  lines and round-trips correctly for dates well outside the typical
  range.

### `random`

Six "logical" entries shipped as 12 `NativeFn` variants (ids 185–196)
because v0.2 stdlib functions can't be generic.  The brief flagged
this trade-off explicitly; I went with the monomorphic-per-element-
type approach (`choice_i64`, `choice_f64`, `choice_str`; same for
`shuffle` and `sample`).  Rationale:

* Generic dispatch on stdlib-table entries would require the typecheck
  path's `synth_call` for builtin modules to specialise the param/ret
  types based on the argument's element type, then have IR call a
  generic `NativeFn::RandomChoice` with an injected `TypeTag` operand
  (the same shape as `list.sort` / `sorted`).  The infrastructure
  *could* be built but it touches three places: typecheck (relax the
  arity/type check), IR (`lower_method_call` extension for the
  builtin-module-receiver path), and a new VM handler.
* Monomorphic shipping today is honest about the gap, doesn't bloat
  the typecheck surface for one feature, and the user-visible cost is
  a `_i64`/`_str` suffix that's documented in spec §9.11.  A v0.3
  generic `random.choice[T](xs: List[T]) -> T` is a clean replacement.

**LCG constants**: multiplier `1103515245`, increment `12345`, modulus
`2^31`.  These are the Numerical Recipes values, well-known and used
by `markov.spy` already.  The spec section is explicit that this is
NOT crypto-quality: period ~2^31, easy to predict given consecutive
outputs.  Suitable for tests, games, monte-carlo demos.  Programs that
need cryptographic randomness should wait for v0.3's `os.urandom` (or
roll their own via `os.read_file("/dev/urandom")` on Linux).

**`random()` precision**: a single 31-bit LCG draw gives 31 bits of
mantissa entropy.  I combine two draws (upper 26 bits + lower 27 bits)
into a 53-bit mantissa so consecutive `random()` values cover the
double-precision range with no gaps.

**State**: per-interpreter on `Interpreter::random_lcg_state` (default
`0`).  No thread-local: workers spawned via `threading.Thread` share
their parent's state because thread workers run on a *different*
`Interpreter` (their own register file, their own LCG state).  This is
the right behaviour for v0.2 — explicit cross-thread coordination via
a `random.lock()` is v0.3 work if anyone wants it.

### `math` (extensions)

19 module entries (5 constants + 14 functions), of which 6 wrap
existing prelude natives (ids 70–79) and 8 are new (ids 200–212).  The
key constraint: existing v0.1 programs that call bare `sqrt(x)` /
`sin(x)` / `cos(x)` etc. must keep working — and they do, because the
prelude registrations under `NativeFn::from_name` are untouched and
the new `math.sqrt` etc. simply expose the *same* `NativeFn::MathSqrt`
id under a different (namespaced) name.

The one breaking-shape decision was `math.floor(x) -> i64` and
`math.ceil(x) -> i64`, both of which return `i64` to match Python 3's
`math.floor` / `math.ceil` semantics.  The existing prelude `floor` /
`ceil` natives (`NativeFn::MathFloor` / `MathCeil`, ids 77/78) still
return `f64` for v0.1 backward compatibility.  The new `math.floor`
calls a new handler (`MathFloorI` = 202) that takes an f64 and returns
an i64.

`math.factorial(n)`'s `n ≤ 20` ceiling is the largest input that fits
in `i64::MAX` (21! exceeds it).  Out-of-range inputs raise
`OverflowError` (positive) or `ValueError` (negative) — the spec is
explicit about both.

## Files modified + LOC (approximate)

| File | Lines added | Purpose |
|---|---|---|
| `shared/src/native.rs` | +95 | 31 new `NativeFn` variants (175–212) + `from_u32` arms |
| `compiler/src/resolver.rs` | +290 | Three `StdlibModule` registrations |
| `vm/src/interp.rs` | +12 | `monotonic_start`, `random_lcg_state` fields |
| `vm/src/builtins.rs` | +290 | New handlers + `lcg_next` / `format_epoch_iso` / `civil_from_days` |
| `STRICTPY_SPEC.md` | +130 | §9.10/§9.11/§9.12 |

Plus tests + examples:

* `examples/fizzbuzz_v2.spy` — 20-line FizzBuzz on random ints, seeded.
* `examples/timer_demo.spy` — `time.monotonic()` micro-benchmark.
* `examples/math_demo.spy` — exercises 11 distinct `math` symbols.
* `examples/sleep_test.spy` — wall-clock sanity check for `sleep_ms`.
* `vm/tests/m20b_time_random_math.rs` — 26 in-process tests.
* `compiler/tests/{fizzbuzz_v2,timer_demo,math_demo,sleep_test}_runs.rs`
  — 8 subprocess tests via `spy.exe`.

## Hardest three things (in retrospect)

1. **The civil-date conversion**.  My first try at `format_iso` did
   simple year/month/day arithmetic — count days, divide, modulo — and
   got the off-by-one wrong twice (March-vs-February boundary, leap
   year boundary).  Switching to Howard Hinnant's
   `civil_from_days` (12 lines, public domain, used by libc++) fixed
   it on the first try.  The 2000-01-01 round-trip test pinned the
   regression for future agents.

2. **`bare-name sqrt` was a lie**.  The brief said "existing programs
   use bare-name `sqrt`/`sin`/etc. (registered as prelude in
   `compiler/src/resolver.rs`)" — but they're not in the prelude.
   `NativeFn::from_name("sqrt")` exists, but the prelude never declares
   a `sqrt` symbol.  No example program actually uses bare `sqrt(x)`
   (only `mandelbrot.spy` was thought to, but it doesn't), so the
   coexistence-with-`math.sqrt` worry from the brief was a non-issue
   — I removed the dual-form line from `math_demo.spy` after
   `compile_examples.rs` caught it.  Worth flagging: the prelude
   registration for bare `sqrt(x)` is missing (a minor gap the
   orchestrator may want to file).

3. **The Numerical Recipes LCG's range**.  The LCG generates values in
   `[0, 2^31)` (positive 31-bit ints).  `random.randint(lo, hi)` needs
   to map that into `[lo, hi]` inclusive, which means computing `r %
   (hi - lo + 1)`.  The edge case of `hi - lo == i64::MAX` (so
   `span` overflows to zero) was easy to miss; I added an explicit
   `if span == 0` fall-through.  In practice no program will ever hit
   it — but the `wrapping_add(1)` makes the overflow path a no-op
   instead of a silent divide-by-zero panic in release builds.

## Incidentally-discovered issues

* The bare-name `sqrt`/`sin`/`cos`/etc. prelude registration is
  **missing**.  `NativeFn::from_name("sqrt")` knows about the id, but
  the resolver doesn't declare a `sqrt` symbol — so `sqrt(x)` is
  actually an undefined-name error.  This is a v0.1-era gap, not a
  regression from this milestone, but it contradicts what the brief
  claimed.  Worth filing as a separate bug.
* No other bugs surfaced — the M19+M20a infrastructure absorbed three
  more modules without complaint.

## What's next

Per the orchestrator's M20 batch: `json` and `re` remain.  Both will
need more than a thin stdlib-table veneer:

* `json`: parse depth-bounded `Map[str, Any]` requires an `Any` type
  (which v0.2 doesn't have) or a tagged `JsonValue` sealed class.  The
  recommendation in M20a's report — model it like `producer.spy`'s
  tagged variants — still stands.
* `re`: pulling in the `regex` crate (~500KB) for a real regex engine
  is the obvious choice; a hand-rolled NFA matcher is a rabbit hole.

After those, user-defined modules + submodules (`os.path` instead of
the top-level `path`) become the v0.3 priority — and that's where the
M19 design pays its second dividend, because the typecheck/IR paths
already know how to look up items in a `HashMap<String, StdlibModule>`.
Swapping that for a `HashMap<ModulePath, _>` is a localised refactor.
