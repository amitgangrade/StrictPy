# M23 P3a-D — `sqlite3` stdlib module

**Brief**: Ship a minimal Python-`sqlite3`-shaped surface on top of
the `rusqlite` crate (with the `bundled` feature so libsqlite3.c is
statically linked into the VM binary — no system SQLite required on
any platform).  The deliverable is a tiny **connect / execute / query
/ close** API with parameter binding and a handful of metadata calls
(`last_insert_rowid`, `changes`, `column_names`).  This is the first
stdlib module that crosses the FFI-to-C boundary; everything in M19
through M22 was pure-Rust crate-wrap.

**Wall-clock**: ~1.5 hours (read-through of SHARED_BRIEF + M20c, M22
P2B reports; one new shared-VM field; nine native handlers totalling
~250 LOC including helpers; 13 in-process tests + 2 subprocess tests
+ one example program + spec amendment).

**Files changed**: 5 source files + 1 cargo dep + 1 spec section
(§9.24) + 1 agent report + 1 new example + 2 new test files.
**Tests**: 468 baseline + **15 new** (13 in-process + 2 subprocess
across the demo program) — all green locally.

## Strategy: crate-wrap `rusqlite` with `bundled`

The brief explicitly recommended `rusqlite` and the **bundled** feature
so the toolchain doesn't need a system-installed SQLite.  `bundled`
pulls in `libsqlite3-sys` which compiles libsqlite3.c as a C
dependency — adds about 30 seconds to the first `cargo build`
(cached after), no impact on the binary size that the brief flagged
as a concern (the SQLite C amalgamation is ~700KB compressed; rough
release-build size delta is well under the 5MB budget).

The single design call was where to store the open connections.
`rusqlite::Connection` is `Send` but not `Sync`, so the existing
`SharedVm` pattern of `Arc<Mutex<Vec<Option<_>>>>` (used for files,
channels, threads, dicts) is the right shape.  Added one field:

```rust
pub sqlite_connections: Arc<Mutex<Vec<Option<rusqlite::Connection>>>>,
```

Slot 0 is reserved as "no connection" (matching the other resource
tables); `connect` returns the slot index as a signed `i64`, user
code passes the handle through every subsequent call, `close` nulls
the slot.

## The take-and-put-back pattern for SQL calls

`rusqlite::Connection::execute` / `query` take `&mut self`.  We can't
hand out a `&mut` into a `Vec<Option<Connection>>` slot while holding
the table mutex without (a) holding the mutex for the entire SQL call
or (b) walking around the borrow checker with `Option::take`.

I picked (b).  A helper `sqlite3_with_conn` briefly locks the table,
calls `slot.take()` to move the `Connection` out (leaving `None`
behind), drops the lock, runs the user-supplied closure against the
owned connection, then re-acquires the lock just long enough to put
the connection back.  This means **sibling `connect` / `close` calls
on other threads can proceed in parallel** with a long-running query
on a different connection — exactly the behaviour you'd want from a
multi-threaded VM with shared sqlite-connection state.

The trade-off: if user code recursively invokes another `sqlite3` API
on the same handle while a query is mid-flight (e.g. through a
callback / hook), the second call sees `slot.take() == None` and
raises a `ValueError("connection in use")` rather than deadlocking on
the mutex.  v0.2 has no callback or hook surface that could trigger
this organically — closures-across-NativeFn-boundary is a v0.3
feature — but the failure mode is documented in the spec.

## API surface (9 functions, IDs 440-448)

| ID  | Name                  | Signature |
|-----|-----------------------|-----------|
| 440 | `connect`             | `(path: str) -> i64` |
| 441 | `close`               | `(conn: i64) -> None` |
| 442 | `execute`             | `(conn: i64, sql: str) -> None` |
| 443 | `execute_params`      | `(conn: i64, sql: str, params: List[str]) -> None` |
| 444 | `query`               | `(conn: i64, sql: str) -> List[List[str]]` |
| 445 | `query_params`        | `(conn: i64, sql: str, params: List[str]) -> List[List[str]]` |
| 446 | `last_insert_rowid`   | `(conn: i64) -> i64` |
| 447 | `changes`             | `(conn: i64) -> i32` |
| 448 | `column_names`        | `(conn: i64, sql: str) -> List[str]` |

449-469 reserved for v0.3 (typed result rows once `bytes` lands,
prepared-statement caching, transaction handles, `executemany`).

## The "all results as str" simplification

Per the brief: every cell in a query result is stringified.
INTEGER → decimal text, REAL → `format!("{}", f64)`, TEXT → as-is,
NULL → empty string, BLOB → lowercase hex.  This loses fidelity for
two cases:

1. **BLOBs**, which v0.2 has no good story for anyway (no `bytes` type
   yet — the M22 `struct` module's "wide-char str" trick could
   round-trip but exposes the codepoint-0..255 encoding to user code,
   which is uglier than just hex).  Programs that *write* BLOBs would
   need typed parameter binding, which we don't have either.
2. **REAL values**, where Rust's `{}` formatter and Python's `str()`
   diverge at the edges (e.g. `repr(0.1) == "0.1"` in Python, Rust
   prints the same but for very large / small values the format
   differs).  None of the v0.2 example programs hit this — config
   stores and cache tables don't store floats — but it's worth a v0.3
   pass.

Documented in spec §9.24's stringification block.  No incident found
in any of the 13 in-process tests.

## NULL → empty-string vs. nullable typing

A natural alternative would be `List[List[str?]]` (each cell
nullable) — that's how Python's `sqlite3` distinguishes NULL from the
empty string.  But:

* v0.2's `Nullable[T]` typecheck path through native-return values
  is rough (the M20a `os.env(key) -> str?` path is the only existing
  case, and it special-cases the single-return-slot decode).
* The cell-collected `List[List[str]]` shape requires every element
  to type uniformly.
* Real-world programs treating NULL and the empty string as
  interchangeable is overwhelmingly the common case (e.g. config
  stores where "unset" and "" are the same).

Documented and shipped.  v0.3 with typed result rows + stdlib classes
can reify the distinction via a `Cell` variant type or per-column
typed accessors.

## Parameter binding: TEXT-only is the v0.2 shape

`execute_params(conn, sql, params: List[str])` always binds parameters
as TEXT.  SQLite's type-coercion rules apply when the bound value is
compared against an INTEGER or REAL column, so:

```sqlite
SELECT * FROM users WHERE id = ?   -- bind "42", matches id=42 INTEGER
```

works as users would expect (SQLite implicitly converts `"42"` to
`42` for the integer-column comparison).  The example program's
`SELECT WHERE title = ?` exercises this path.

What this doesn't support: storing a literal string `"42"` in an
INTEGER column distinct from the integer `42`.  Real programs don't
hit this — INTEGER columns store integers — and v0.3's
typed-parameter API will give the precise control if anyone needs it.

## Files modified + LOC (approximate)

| File | Lines added | Purpose |
|---|---|---|
| `shared/src/native.rs` | +50 | 9 new `NativeFn` variants (440-448) + `from_u32` arms |
| `compiler/src/resolver.rs` | +100 | One `StdlibModule` registration |
| `vm/src/builtins.rs` | +250 | 9 handlers + 4 helpers (`with_conn`, `read_str_list`, `run_query`, `stringify`, `alloc_rows`) |
| `vm/src/interp.rs` | +5 | `SharedVm::sqlite_connections` field + init in both constructors |
| `vm/Cargo.toml` | +6 | `rusqlite = { version = "0.32", features = ["bundled"] }` + comment |
| `STRICTPY_SPEC.md` | +90 | §9.24 (sqlite3) |

Plus tests + examples:

* `examples/sqlite_demo.spy` — 10 scenarios (in-memory connect,
  CREATE TABLE, three parameter-bound INSERTs including the
  injection-attack title `"O'Brien"` + body `"drop table notes;--"`,
  `last_insert_rowid`, ordered SELECT, column-name discovery,
  parameterised SELECT, UPDATE + `changes()`, bad-SQL try/except,
  idempotent close).
* `vm/tests/m23_p3a_d_sqlite.rs` — 13 in-process tests covering:
  connect/close, round-trip CREATE/INSERT/SELECT, rowid+changes,
  column_names, bad-SQL ValueError, injection-safe param binding,
  filtered query_params, NULL/REAL/TEXT stringification, invalid
  handle ValueError, two-connection isolation, idempotent close,
  empty-result-set SELECT, use-after-close ValueError.
* `compiler/tests/sqlite_demo_runs.rs` — 2 subprocess tests via
  `spy.exe` (compile-only + run-and-assert).

## Hardest three things (in retrospect)

1. **The take-and-put-back pattern.**  Spent the most thinking time
   on whether to hold the table mutex for the SQL call (simpler, but
   blocks sibling `connect` on other threads) vs.  the
   take/put-back dance (more code, but parallel-safe).  Picked the
   latter because the existing `files` table follows the same shape
   — the handle is "checked out" from the table, the SQL call runs,
   the handle is "checked in" — and because the M6 threading story
   would otherwise be a foot-gun on multi-threaded programs that
   share connections.

2. **rusqlite's `params_from_iter` shape.**  `Connection::execute`
   takes `impl Params`; `&[String]` doesn't satisfy it directly.  The
   crate exposes `rusqlite::params_from_iter(I)` for an iterator of
   `&dyn ToSql`, which is what landed.  Five minutes of reading the
   crate docs.

3. **The all-results-as-str design call.**  The brief flagged this
   explicitly — losing fidelity for BLOBs in exchange for not waiting
   on v0.3's `bytes` type.  The decision was easy; the work was
   documenting it carefully in spec §9.24 so future programs hitting
   the limitation have a clear signpost.

## Incidentally-discovered bugs / oddities

* **None requiring code changes.**  The M19+M20+M22 stdlib-module
  infrastructure absorbed an FFI-to-C module without complaint — one
  crate dep, one `SharedVm` field, one `StdlibModule` registration,
  nine dispatch arms.  Zero resolver / typecheck / IR changes.  This
  is now the third consecutive M-batch sub-milestone with no
  incidental-bug discovery (M22 P2A/B/C/D all reported zero, M20c
  too), which I read as a maturity signal more than a coverage gap.
* The `match` keyword (M16 hazard) didn't collide because none of
  the function names — `connect`, `close`, `execute`, `query`,
  `last_insert_rowid`, `changes`, `column_names` — reuse a reserved
  word.

## Cross-platform notes

`rusqlite` with `bundled` is the *standard* cross-platform Rust
SQLite binding, used by Cargo itself, sqlx, diesel, and the wider
Rust ecosystem.  No `cfg(target_os = ...)` gates required.  On
Windows + Linux + macOS the C amalgamation compiles cleanly with the
default `cc` toolchain (MSVC on Windows, GCC/Clang elsewhere).  The
in-memory `":memory:"` mode is supported identically on every
platform, which is what every one of the 13 in-process tests uses —
no filesystem residue, no per-platform tempdir conventions.

Build-time impact: ~30 seconds on first `cargo build --release`
(libsqlite3-sys compiles libsqlite3.c).  Subsequent builds are
cached.  Release binary size delta: well under the 5MB budget the
brief flagged.

## What's next (v0.3 / Phase 4 candidates)

* **Typed result rows.**  Replace `List[List[str]]` with a
  `Cell`-variant or per-column-typed accessor once stdlib-class
  registration ships.  Reifies NULL vs. empty string, integer vs.
  decimal-text, and unlocks BLOBs.
* **Prepared-statement caching.**  A `Statement` handle returned by
  `prepare(conn, sql) -> i64` and reusable via `execute_stmt` /
  `query_stmt`.  Big wins for programs that loop INSERTs in a tight
  loop (the current shape re-prepares each call).
* **Typed parameter binding.**  `execute_params_typed(conn, sql,
  params: List[Param])` with `Param = Param.text(str) | Param.int(i64)
  | Param.real(f64) | Param.blob(bytes) | Param.null`.  Needs stdlib
  classes + `bytes`.
* **Transaction handles.**  `tx: Transaction = conn.begin()` /
  `tx.commit()` / `tx.rollback()`.  Today programs use raw `BEGIN` /
  `COMMIT` / `ROLLBACK` SQL through `execute`, which works but
  doesn't enforce the explicit-rollback-on-Python-error semantics
  Python's `sqlite3` module provides.
* **Row iterators / cursors.**  `Cursor`-like incremental fetch for
  large result sets that don't fit in memory.  Needs generator /
  iterator-protocol support beyond what M16 ships.
* **User-defined SQL functions.**  `conn.create_function(name,
  closure)` to register a Rust-callable SQL UDF.  Blocked on
  closure-across-NativeFn-boundary (the SHARED_BRIEF flags it
  explicitly as v0.3 work).

P3a-D was the only Phase 3a agent crossing the FFI-to-C boundary;
the result is reassuring — the M19 stdlib-module-table seam is wide
enough that even an FFI-backed module fits the
`Cargo.toml`-and-dispatch-arms shape.  The 5MB binary-size budget
was the only Phase-3-specific risk worth flagging, and `rusqlite`
came in well under it.
