# M35 P4-B — Typed `sqlite3.Connection` + `sqlite3.Cursor` classes

**Brief**: ship typed `Connection` and `Cursor` classes wrapping the
M23 P3a-D flat handle-passing `sqlite3` surface — the M29 web
framework report and several real-world programs called this out as
the highest-friction stdlib API in v0.2.  Part of the M35 parallel
round (three agents shipping stdlib classes together using the M34
prelude-registration pattern).  Disjoint NativeFn range 800-819.

**Wall-clock**: ~1.5 hours of agent compute, well under the 2-3 hour
budget the brief estimated.  First commit landed at ~50% of budget
(the cap was 60%, comfortably under).  The lower agent count is
because most of the infrastructure already existed — M34's prelude-
class pattern, the M23 P3a-D logic itself, the class-name + method-
name IR hook — so this milestone was mostly stitching established
pieces together with one new shared state field and one new parser
contextual-keyword rule.

**Tests**: 10 new VM integration tests in `vm/tests/m35_sqlite_class.rs`
plus 2 demo tests in `compiler/tests/sqlite_class_demo_runs.rs`,
covering:

* `sqlite3.open` + CREATE / INSERT (parameter-bound) / SELECT round-trip
* `fetchone` iteration to exhaustion (with the `NONE_SENTINEL`
  return-shape gotcha the M34 report flagged)
* `fetchall` after `fetchone` returns *only the remaining rows*, and a
  second `fetchall` is the empty list — the iteration-position
  contract that lets mixed `fetchone`/`fetchall` work
* `query_params` filters rows correctly
* `column_names` matches the SELECT list
* `last_insert_rowid` / `changes` for write-side metadata
* Bad SQL raises `ValueError` (caught via `try/except`)
* `close` is idempotent; use-after-close raises `ValueError`
* Empty query → `row_count == 0`, first `fetchone` returns `none`
* Flat M23 P3a-D surface and typed M35 P4-B surface coexist in the
  same program with no interference

All 12 new tests pass.  No regressions in the M23 P3a-D suite (all
13 existing sqlite3 tests still pass).

## Infrastructure pattern — straight M34 reuse

The M34 agent's report explicitly noted "no `re.Pattern` /
`sqlite3.Connection` infrastructure reuse from M34" as a known
trade-off of the prelude-registration shortcut.  In practice the
prelude shape is the right one for v0.3:

1. Add two `is_native: true` classes to `seed_prelude` alongside
   Channel / Thread / io.File.
2. Hang the methods off `class_layouts` for type-checking purposes
   but rely on the IR's class-name + method-name dispatch hook (M34
   pattern) to route every call to the matching `NativeFn`.
3. Add one new SharedVm field (`sqlite_cursors`) for the per-cursor
   state — the connection slot table is reused unchanged.
4. The typecheck synthesises a constructor signature
   (`m35_p4b_sqlite_ctor_param_tys`) so `Connection(handle)` and
   `Cursor(handle)` type-check despite having no user-level
   `__init__` body.

No new IR ops, no new GC scanning code, no changes to the resolver's
`StdlibItemKind` enum, no module-scoped class registration
plumbing.  The infrastructure work the M34 report deferred to v0.4 is
genuinely deferred — M35 P4-B does not push it forward.  The
user-visible API is identical to what proper module-scoped class
registration would produce; only the implementation needs to change
in v0.4, not the source code that calls into it.

The one Lesson 2 win: every new local in shared files (`resolver.rs`,
`builtins.rs`, `ir.rs`, `typecheck.rs`) is prefixed `p4b_` so the M35
parallel round's P4-A (`re.Pattern`, 790-799) and P4-C (`Hasher`,
820-829) agents' changes can land alongside this one without local-
variable shadowing or stylistic merge conflicts.  Helper functions
that the IR / VM dispatch loop name are also `p4b_*`-prefixed for the
same reason; the user-visible class names (`Connection`, `Cursor`)
and NativeFn variant names (`Sqlite3ConnectionExecute` etc.) are not
— those are part of the API surface and prefix discipline doesn't
apply.

## The Cursor-state design choice

Each Cursor holds an i64 handle into `SharedVm.sqlite_cursors: HashMap<i64,
CursorState>` rather than embedding the rows directly in a class field.
Three reasons:

1. **GC compatibility**: a class field of type `List[List[str]]` would
   need the GC's class scanner to trace it — fine in principle (the
   `GcKind::Class` 8-byte-slot scanner already handles List pointers,
   as M34's JList does), but it would couple the row buffer's
   lifetime to the Cursor instance.  With the side-table the row
   buffer lives in the `CursorState` struct as a plain `Vec<Vec<String>>`
   (no GC involvement) and only the i64 handle is on the class.
2. **`column_names` after exhaustion**: Python's iteration semantics
   say a closed cursor's `description` (column names) is still queryable.
   The side-table holds the column names alongside the rows, so
   `column_names()` works regardless of `next_row`'s position.
3. **`row_count` is total, not remaining**: another Python semantic
   that's easier to express when the row buffer is stable.
   `fetchall` advances `next_row` to `rows.len()`; `row_count` reports
   `rows.len()` unconditionally.

The slot table uses a monotonic `AtomicI64` allocator (start at 1; 0
reserved for "no cursor") instead of the slot-array shape M23 P3a-D
used for connections.  Monotonic ids mean a stale Cursor whose
underlying state was somehow removed (which v0.3 never actually
does — Cursors are not explicitly closed) traps cleanly rather than
aliasing a recycled slot.  Match what M28 P3b-B does for TLS
streams.  The brief explicitly called out a HashMap shape; this
implementation follows that.

## Parser change — `KwOpen` after `.`

The brief's `sqlite3.open(path) -> Connection` API hit a parser
issue: `open` is a reserved keyword (it's the class-modifier prefix
in `open class Foo`).  The parser already special-cases it as a bare
identifier in expression position (so `f = open("file.txt", "r")`
works) but not after `.` in member-access — so `sqlite3.open(...)`
hit `expected identifier, found KwOpen`.

Fix: add `KwOpen` to the same list of contextual keywords the parser
accepts after `.` (alongside `KwAwait` / `KwAsync`, which were
already handled the same way for `future.await()`).  After `.` the
token is syntactically unambiguous — it can only be a method or
attribute name — so the keyword form is safe to recognise without
backtracking.

The alternative the brief suggested was renaming to `connect_typed`,
but `open` is the natural Python-aligned name (matches the broader
"open a database / file / archive" convention).  The parser fix is
~5 lines and clears the path for future `.open` methods on other
stdlib classes (zipfile / tarfile / etc.) if v0.4 wants to surface
them.

## NONE_SENTINEL — heeded the M34 report

The M34 agent's report called out the `0x8000_0000_0000_0000` sentinel
for nullable returns from native handlers ("NOT zero — tests will
catch it").  `Cursor.fetchone` returns `List[str]?`, so on exhaustion
the handler returns `NONE_SENTINEL` rather than `Ok(0)`.  The test
`fetchone_iterates_and_exhausts_to_none` would have caught a wrong
return shape (it asserts both the iteration count is right and a
subsequent `fetchone` after exhaustion still returns `none`).

## File-ownership compliance

Per the brief's "M35 parallel round" section, this milestone owns
NativeFn IDs **800-819** exclusively.  No P4-A (790-799) or P4-C
(820-829) NativeFn ids are touched.  Class registration in the
resolver prelude is additive — placed at the bottom of `seed_prelude`
between the M34 JsonValue block and the existing `True`/`False`
consts.  The `sqlite3.open` registration is one new `StdlibItem`
appended to the existing M23 P3a-D `sqlite3_mod.items` vec; the
flat-function shape above it is unchanged.

The new `SharedVm` field `sqlite_cursors` + `next_cursor_id` is
appended right after the existing `sqlite_connections` field — kept
adjacent for readability since they're a logical pair.  Both the
JIT and non-JIT `SharedVm` constructors initialise it (matches the
existing pattern for every other slot table).

## What's still in scope for v0.4

Documented in `STRICTPY_SPEC.md` §9.29.1's deferred list:

* **Cursor iteration via `for row in cur:`** — needs the iterator-
  protocol hook on stdlib classes; v0.3 users compose `fetchone`
  in a `while true:` loop with `if row is not none:` narrowing (see
  the demo's pattern 6).  NativeFn IDs 816 (`__iter__`) and 817
  (`__next__`) are reserved for this.
* **`Connection.commit` / `rollback`** — the existing flat surface
  routes transactions through raw SQL (`execute("BEGIN")` /
  `execute("COMMIT")` / `execute("ROLLBACK")`) and the typed surface
  does the same for v0.3.  IDs 809 + 810 are reserved.
* **Typed parameter binding** — same constraint as the flat surface;
  parameters are always bound as TEXT.  Programs that need INTEGER /
  REAL columns format the value into a string first and rely on
  SQLite's column-side coercion.

## Bug findings

None found in the M0–M34 surface during M35 P4-B.  The closest thing
was the parser's KwOpen handling, which is a documented design
choice (reserved keyword needs an explicit contextual-acceptance
hook) rather than a bug — the M14 / M32 agents already added the
same hook for `KwAwait` and `KwAsync`, so the fix slotted in
naturally.

## Test counts

| Suite | Pre-M35 P4-B | Post-M35 P4-B |
|---|---:|---:|
| Compiler integration (`compiler/tests/`) | per the M34 baseline | + 2 (sqlite_class_demo_runs) |
| VM integration (`vm/tests/`) | per the M34 baseline | + 10 (m35_sqlite_class) |
| **Added by M35 P4-B** | — | **+12** |
| **Pre-existing failures** | 1 (m33 stack overflow on Windows) | 1 (unchanged) |

## Lesson 1 compliance

First commit landed at ~50% of budget, well inside the 60% cap.
The streak holds at agent #15 clean.

## Files shipped

| Path | Lines added | Purpose |
|---|---:|---|
| `compiler/src/resolver.rs` | +135 | Register Connection + Cursor classes in `seed_prelude`; add `sqlite3.open` StdlibItem to the existing sqlite3 module. |
| `compiler/src/typecheck.rs` | +25 | `m35_p4b_sqlite_ctor_param_tys` for the synthesised constructor signatures. |
| `compiler/src/ir.rs` | +60 | Two new helpers (`m35_p4b_sqlite_class_init_native_id` + `m35_p4b_sqlite_class_method_native_id_by_name`) plus call-sites in `lower_call` / `lower_method_call` mirroring the M34 hooks. |
| `compiler/src/parser.rs` | +10 | Accept `KwOpen` as a method name after `.` (same shape as KwAwait / KwAsync). |
| `shared/src/native.rs` | +75 | 14 new NativeFn entries (800-815) + `from_u32` arms. |
| `vm/src/builtins.rs` | +325 | Dispatch arms + `p4b_*` handler functions for the typed Connection / Cursor methods. |
| `vm/src/interp.rs` | +30 | New `CursorState` struct + `sqlite_cursors` HashMap + `next_cursor_id` AtomicI64 on `SharedVm` (initialised in both ctors). |
| `vm/tests/m35_sqlite_class.rs` | +330 | 10 integration tests. |
| `compiler/tests/sqlite_class_demo_runs.rs` | +85 | 2 demo round-trip tests. |
| `examples/sqlite_class_demo.spy` | +120 | The how-to demo for the typed sqlite surface. |
| `STRICTPY_SPEC.md` | +85 | §9.29.1 "`Connection` and `Cursor` classes (v0.3 — M35 P4-B)". |
| `docs/thesis/agent_reports/m35_p4b_sqlite_class.md` | — | This report. |

Total compiler/runtime LOC: ~660 added across 7 files.  Tests + demo
+ docs: ~620.  Net ~1280 LOC for the milestone, well within the
brief's "smaller than M34 because the pattern is established"
envelope.  Most of the new lines are handler boilerplate that
delegates straight to the M23 P3a-D logic — the typed surface is
genuinely a thin ergonomics wrapper, not a rewrite.

## Verdict

Connection + Cursor ship.  The canonical use case
(`sqlite3.open(":memory:")` → `conn.execute("CREATE TABLE ...")` →
`conn.execute_params("INSERT ... VALUES (?)", params)` →
`conn.query("SELECT ...")` → `cur.fetchone()` → `conn.close()`)
works end-to-end.  All 12 new tests pass, no regressions on the
existing surface, the M29 framework's flat-function sqlite usage
continues to compile and run.  The infrastructure deferral (prelude
classes instead of module-scoped class registration) is consistent
with the M34 precedent and reversible in v0.4.  Ready for the M35
P4-A / P4-C agents to ship alongside, and for v0.4 to lift the
infrastructure into proper module-scoped class registration without
breaking any source-level API.
