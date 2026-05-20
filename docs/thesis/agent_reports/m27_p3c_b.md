# M27 P3c-B — `glob` + `fnmatch` stdlib modules

**Brief**: Ship `glob` and `fnmatch` as the 25th and 26th stdlib modules
in StrictPy.  The infrastructure has now absorbed twelve incremental
adds without resolver / typecheck / IR / codegen changes (M19 → M26)
and this round repeats the pattern: two modules slot into
`seed_stdlib_modules`, two NativeFn blocks into `shared::native::NativeFn`,
two handler clusters into `vm::builtins::dispatch`, two spec sections,
two demos, two integration tests.  Zero churn elsewhere.

**Wall-clock**: ~75 minutes (read-through + 7 native handlers + ~280 LOC
including the `fnmatch.translate` regex emitter and the in-process test
suite + 2 demos + 2 spec sections).

**Files changed**: 4 source files + 1 spec section pair + 1 agent
report + 2 new examples + 3 new test files.

## What `glob` + `fnmatch` add

`glob` ships three functions:

* `glob.glob(pattern)` — non-recursive directory walk, returning
  sorted `List[str]` of matching paths.
* `glob.recursive(pattern)` — same but `**` matches across
  subdirectories.
* `glob.escape(s)` — quote glob metacharacters in `s` so a literal
  path matches.

`fnmatch` ships four:

* `fnmatch.fnmatch(name, pattern)` — single-string wildcard match,
  case-INsensitive on Windows / sensitive on Unix.
* `fnmatch.fnmatchcase(name, pattern)` — always case-sensitive.
* `fnmatch.filter(names, pattern)` — order-preserving list filter.
* `fnmatch.translate(pattern)` — convert a shell-glob into a regex
  string that composes with `re.fullmatch` (M20c).

Total surface: 7 functions, NativeFn IDs 480-486 (13 reserved in the
480-499 block for v0.3 — e.g. `glob.iglob`, `glob.has_magic`,
recursive variants of fnmatch).

## Module decisions

### Why the `glob` crate

`glob = "0.3"` is the canonical Rust binding for shell-style pattern
matching: it ships both a pure `glob::Pattern` matcher and a directory
walker (`glob::glob` / `glob::glob_with`).  Both are used here —
`Pattern` for `fnmatch.fnmatch*` / `fnmatch.filter`, and `glob_with` for
`glob.glob` / `glob.recursive`.  The alternative `globset` crate is
faster for repeated matching against many patterns but has a more
ceremonious API and no built-in walker; `glob`'s simpler shape is the
right fit for v0.2's "one pattern, one call" usage.

The crate is ~700 LOC and pure-Rust (no C deps), so binary size impact
is negligible — about the same as the M20b `random` LCG.

### Case sensitivity split: matching CPython

CPython's `fnmatch.fnmatch` is documented as case-insensitive on
case-insensitive filesystems (Windows) and case-sensitive elsewhere.
The implementation picks this up via `os.path.normcase`.  Our split is
mechanical: `!cfg!(windows)` gates `case_sensitive` in the
`glob::MatchOptions` for `fnmatch.fnmatch`, and `fnmatchcase` hard-codes
`true`.  The companion `glob.glob` follows the same `!cfg!(windows)`
gate, which is consistent with filesystem semantics — a Windows lookup
for `*.TXT` would match `hello.txt` since NTFS is itself
case-insensitive.

The downside is that `fnmatch.fnmatch` is non-deterministic across
platforms.  Test code that wants a portable answer must use
`fnmatchcase` — same advice CPython gives.  The demo asserts the
contract on `fnmatchcase` (where the answer is fixed) and only asserts
determinism on `fnmatch` (`a == b` on repeated calls).

### `fnmatch.translate` — hand-rolled, not via the `glob` crate

The `glob` crate doesn't expose its internal pattern-to-FSM compiler,
so `translate` is implemented by hand (~50 LOC).  The conversion is
mechanical:

* `*` → `.*`
* `?` → `.`
* `[abc]` / `[a-z]` → passed through unchanged
* `[!abc]` → `[^abc]` (CPython's negation, regex's caret form)
* Unterminated `[` → escape the literal `[` (defensive — matches
  CPython's behaviour)
* Regex metacharacters (`.`, `^`, `$`, `+`, `(`, `)`, `|`, `{`, `}`,
  `\`) get backslash-escaped
* Output is wrapped in `(?s:...)\z`

Two subtleties that bit during implementation:

1. **`\Z` vs `\z`.**  CPython emits `\Z` (Python regex end-anchor); the
   Rust `regex` crate only supports `\z` (lowercase).  Same semantic,
   different letter.  The spec calls this out so users porting Python
   code know to expect the lowercase variant if they inspect the
   translate output directly.
2. **`(?s:...)` dot-matches-newline.**  Without the `s` flag, `*` →
   `.*` wouldn't match strings containing `\n` — but fnmatch is meant
   to be "any character" semantics.  The wrapper `(?s:...)` enables
   dot-matches-newline inside the group without globally polluting the
   regex flags (so callers chaining `translate` output with other
   patterns aren't surprised).

### Empty results, not errors

Both `glob.glob` and `glob.recursive` return an empty list when no
files match.  This matches CPython's behaviour and is the right
default — a missing-directory pattern shouldn't have to be wrapped in
try/except just to discover "no matches".  Hard errors (malformed
pattern, true I/O failure on the walker itself) still raise
`ValueError`.

### Sort order

CPython's `glob.glob` returns results in **arbitrary** order on most
platforms — the order is whatever `os.scandir` yields.  We instead
sort ascending unconditionally.  This is a deliberate divergence: the
overwhelming use case is "list these files for the user" or "iterate
deterministically in tests", both of which want a stable order.
Programs that wanted insertion-order can recover it with a single
`.sort()` no-op, but programs that wanted determinism would have to
sort manually if we didn't.  The diff is ~3 LOC (`paths.sort()` before
the alloc loop).

## Files modified + LOC (approximate)

| File | Lines added | Purpose |
|---|---|---:|
| `shared/src/native.rs` | +45 | 7 new `NativeFn` variants (480-486) + `from_u32` arms |
| `compiler/src/resolver.rs` | +80 | Two `StdlibModule` registrations |
| `vm/src/builtins.rs` | +280 | 7 handlers + `fnmatch_match` + `fnmatch_translate` |
| `vm/Cargo.toml` | +7 | `glob = "0.3"` dep |
| `STRICTPY_SPEC.md` | +115 | §9.32 + §9.33 |

Plus:

* `examples/glob_demo.spy` — 130-line demo that builds a test
  directory tree (`_m27_glob_tmp/`) with `os.mkdir` + `pathlib.write_text`,
  exercises `glob.glob` / `glob.recursive` / `glob.escape`, asserts
  the match counts, and cleans up.
* `examples/fnmatch_demo.spy` — 80-line demo of `fnmatchcase`,
  case-sensitivity contract, `filter` order preservation, and
  `translate` round-trip through `re.fullmatch`.
* `vm/tests/m27_p3c_b_glob_fnmatch.rs` — 16 in-process tests.
* `compiler/tests/glob_demo_runs.rs` + `compiler/tests/fnmatch_demo_runs.rs`
  — 2 subprocess tests each.

NativeFn IDs consumed: **7 out of 20** in the assigned 480-499 range
(487-499 reserved).

## Hardest three things (in retrospect)

1. **Rust regex's `\z` vs Python regex's `\Z`.**  The
   `fnmatch.translate` output is meant to compose with `re.fullmatch`.
   I initially emitted `\Z` (Python style) and the demo's
   `re.fullmatch(translate("*.txt"), "hello.txt")` returned false
   because the Rust `regex` crate parses `\Z` as the literal letter `Z`
   (not an end anchor).  The fix was one character — `r")\Z"` →
   `r")\z"` — but the failure mode was silent (the regex compiled
   successfully and just didn't match).  Documented the divergence
   from CPython in §9.33 since users porting Python code would
   otherwise be surprised.

2. **`require_literal_separator` flag asymmetry.**  For `glob.glob`
   (non-recursive) we want `require_literal_separator: true` so `*`
   stops at `/`.  For `glob.recursive` we want `false` so `**` walks
   subdirectories.  For `fnmatch.fnmatch` we also want `false` because
   `*` in fnmatch is "any character" (no separator semantics).  Three
   distinct uses of the same `glob::MatchOptions` struct with different
   flags — easy to copy-paste wrong.  The fix was to write each call
   site fresh rather than share an `MatchOptions` builder, which made
   the intent at each call site readable in one screen.

3. **`*` does not match `.` in CPython by default.**  CPython's
   `fnmatch` is permissive — `*` matches any character including a
   leading `.` — but `glob.glob` is restrictive (hidden files require
   an explicit `.` prefix in the pattern).  The `glob` crate's
   `require_literal_leading_dot` option toggles this.  For v0.2 we
   ship both modules with `require_literal_leading_dot: false` — the
   permissive behaviour — which matches `fnmatch` cleanly and is the
   less surprising default for `glob.glob`.  Programs that specifically
   need to exclude dotfiles can include `[!.]*` in the pattern.  This
   is a minor divergence from CPython's `glob.glob` which deserves a
   future-work note.

## Incidentally-discovered issues

Zero.  The stdlib-module-table infrastructure absorbed module 25 and
26 without any change to resolver / typecheck / IR / codegen.  All
seven handlers compile from the bog-standard `arg_str` / `arg_u64` /
`read_list_str` decoder helpers plus `interp.alloc_string` /
`interp.alloc_list` allocators that the existing stdlib uses.  The
`fnmatch_translate` helper is a pure string transform with no
interpreter dependency at all.

The one file outside the expected four I considered touching is
`vm/src/interp.rs` for a `Pattern`-cache the way M20b cached `Regex`
compilation.  Both modules currently re-parse the pattern on every
call — wasteful in a tight loop.  Deferred to v0.3 with the rest of
the "stdlib gets re-entrant performance work" push (this would
mirror the deferred `re.compile`).

## Cross-platform notes

* **Windows**: `glob.glob` and `fnmatch.fnmatch` are case-insensitive
  (matches NTFS / FAT32 lookup semantics).  Path separators in glob
  patterns may be either `/` or `\\` — the `glob` crate normalises.
* **Unix family**: `glob.glob` and `fnmatch.fnmatch` are
  case-sensitive (matches ext4/APFS lookup semantics).  Path
  separators must be `/`.
* **`fnmatchcase`**: forces case-sensitive matching regardless of
  platform — the recommended form for portable code.
* **`fnmatch.translate`**: output is platform-independent; the
  resulting regex behaves the same on every host.

The `!cfg!(windows)` toggle lives in two functions (the `glob.glob`
handler and the `fnmatch_match` helper).  No `cfg` arms elsewhere —
all other behaviour is identical.

## What's next

* **v0.3 `glob.iglob`** — lazy iterator.  Would need stdlib iterator
  support, which is also blocked on `re.findall` → iterator and a
  general iterator-of-str primitive.  Defer to the v0.3 iterator
  cluster.
* **v0.3 `glob.has_magic(s) -> bool`** — trivially "does `s` contain
  `*`, `?`, or `[`?".  Would ship as NativeFn ID 487 with no new
  infrastructure.
* **v0.3 Pattern caching** — both modules currently re-parse the
  pattern on every call.  `glob::Pattern::new` is cheap but
  measurable; under a 10k-name filter loop it dominates.  A simple
  per-process `LRU<String, Pattern>` (~30 LOC) would close the gap.
* **`fnmatch.filter_case` / `fnmatch.translate_case`** — CPython
  doesn't ship these (you reach for `re` instead), but they'd be
  consistent with the `fnmatchcase` companion-function pattern.

The infrastructure bet from M19 ("a stable module-table makes new
modules trivial to ship") continues to pay out.  The 12th-iteration
add was a 75-minute, four-file diff with zero touches to the IR / IR
lowerer / codegen / typechecker.  Each individual module is now a
focused, well-bounded ~6-hour story — but the cumulative compounding
of 26 modules adds up to a credible "batteries included" v0.2 stdlib.
