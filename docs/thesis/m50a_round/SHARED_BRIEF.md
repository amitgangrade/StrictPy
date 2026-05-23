# M50a — `tabular.serve` HTTP transport + minimal browser-tab frontend

## Context

Way back in the original Pandas-plan design discussion, the user asked for "a desktop based UI to render a dataframe and interactively apply filters and pivots." M37-M49 built the full `tabular` v1 surface. **M50a starts the desktop UI track**: ship a localhost HTTP server that serves the DataFrame as JSON + a minimal bundled HTML/JS frontend that renders it interactively.

The shape per the original M37 design discussion: **Option A — Webview-served browser UI**. User code calls `tabular.serve(df, port)`; this opens a localhost HTTP server; the user (or the program) points a browser at `http://localhost:<port>/` and sees the DataFrame rendered + filter/groupby controls.

**M50a's scope is the HTTP transport + a minimal-functional frontend**. M50b will polish the JS frontend (better table widget, better filter UI, sortable columns). M50c will add interactive pivot/groupby UI.

**Key architectural decision**: implement the HTTP server **directly in Rust** in `vm/src/builtins.rs` using `std::net::TcpListener`, NOT via the M28 socket stdlib + M29 webserver-framework path. Reasons:

1. `tabular.serve` needs direct access to DataFrame internals for fast JSON serialization. Going via M28 socket + StrictPy user code would force a copy through StrictPy's str/List/Dict surface — defeats the perf story.
2. M29's framework is user-level StrictPy code (~2400 LOC); requiring it for `tabular.serve` would put a lot of dependency surface in the wrong place.
3. The DataFrame ID registry (for filter/groupby derived-dataframes) needs server-side state that's easier to manage in Rust than in StrictPy.

The Rust HTTP server stays simple: hand-rolled HTTP/1.1 request-line + headers parser (we just need GET and POST with small bodies), no Content-Encoding, no keep-alive complications. ~200-300 LOC. The M28 P3b-A socket handler has the std::net usage patterns to reuse.

You are the **32nd** of an unbroken Lesson-1-compliant agent streak (M28 → M49). **Classification: disjoint-handler** (new methods + new server-loop helpers; no new sealed-class subclass; no cross-dispatch fanout). First commit at ~20% of budget.

## Files to read FIRST (in order)

1. `LANGUAGE_GUIDE.md` (project root) — §5 tabular subsection (the existing surface you'll be serving over HTTP)
2. `docs/thesis/agent_reports/m28_p3b_a.md` — M28's `socket` stdlib (the std::net + TcpListener usage patterns; you reuse the patterns, not the M28 stdlib surface)
3. `docs/thesis/agent_reports/m29_webserver.md` — the M29 framework design (informational; M50a builds its OWN HTTP server in Rust, not on top of M29)
4. `examples/webserver/todo_app.spy` — has HTTP request parsing examples you can mirror in Rust
5. `vm/src/builtins.rs` — find:
   - The M28 P3b-A socket handlers (`m28_p3b_a_socket_*`) for std::net usage patterns
   - `m37_alloc_*` family for DataFrame access — the JSON serializer reads via these
   - `m41_build_df_with_index` / `m44_build_df_with_multiindex` — the constructors. M50a's filter/groupby endpoints construct derived DataFrames using these.
   - `m38_groupby_*` — for the groupby endpoint
6. `compiler/src/resolver.rs` — `register_tabular_module`; you'll add new function items
7. `compiler/src/ir.rs` — `m37_tabular_class_method_native_id_by_name`; possibly extend for new DataFrame methods if any
8. `shared/src/native.rs` — current NativeFn block (M49 used through 1066)

## Constraints

- **Lesson 1**: first commit at ~20% of budget. 31-streak — don't break it.
- **Variable prefix `m50a_`** (the `a` suffix because M50 is a 3-milestone sequence).
- **NativeFn IDs `1067-1090`** reserved (24 slots; M50a expected to use ~8-12).
- **No payload changes** to DataFrame or Column subclasses.
- **No new sealed-class subclasses** — disjoint-handler stays clean.
- **No new crate deps** in `vm/Cargo.toml`. std::net is in libstd; that's enough. NO `hyper` / NO `axum` / NO `tokio` (StrictPy is sync-by-design + the M32 async path is thread-backed already).
- All 1016 existing tests must keep passing.

### Edit-tool worktree leak — defensive measure

Per M44/M46/M48/M49: precautionary `cp` at session start.

```bash
for f in vm/src/builtins.rs compiler/src/resolver.rs compiler/src/ir.rs \
         shared/src/native.rs LANGUAGE_GUIDE.md; do
    cp /c/Users/AG/CascadeProjects/PythonCompiler/$f $f 2>/dev/null
done
```

Per-file `cp` recovery if symptoms appear mid-session.

## Phase A — HTTP server core + DataFrame-to-JSON serialization (~400-500 LOC)

### `tabular.serve_with_timeout` API

```python
tabular.serve_with_timeout(df: DataFrame, port: i32,
                            timeout_ms: i64) -> i32
# Boots a localhost HTTP/1.1 server on 127.0.0.1:<port>.
# Returns when the timeout expires (or earlier if the OS kills the listener).
# Returns the exit code (0 = clean timeout, nonzero = error).
# For tests + scripts that need deterministic shutdown.

tabular.serve(df: DataFrame, port: i32) -> i32
# Same as serve_with_timeout but with a sentinel timeout meaning "run
# until Ctrl-C or the parent process dies". For interactive demo use.
# Test infrastructure should use serve_with_timeout exclusively.
```

Both block the calling thread; users wanting concurrency wrap in `Thread.new` (M5 / M6).

### HTTP/1.1 server loop

Hand-rolled in Rust inside `vm/src/builtins.rs::m50a_serve_loop`. Shape:

1. Bind `TcpListener` on `127.0.0.1:<port>`.
2. Loop accepting connections (blocking accept with a periodic check against the timeout deadline).
3. For each connection: parse request line (METHOD URI HTTP/1.1\r\n), parse headers until empty line, read body if Content-Length present.
4. Dispatch on URI prefix:
   - `/` → return bundled HTML page (Phase D)
   - `/api/schema` → DataFrame schema JSON (Phase B)
   - `/api/rows?start=N&stop=M&df=ID` → paginated rows JSON (Phase B)
   - `/api/cell?row=R&col=C&df=ID` → single cell JSON (Phase B)
   - `/api/filter` POST → derived DataFrame ID (Phase C)
   - `/api/groupby` POST → derived DataFrame ID (Phase C)
   - else → 404
5. Build response: status line + Content-Type + Content-Length + body. Write to the socket. Close.

No keep-alive in v1 (each request opens + closes a fresh connection — simpler + fine for low-frequency UI traffic). No HTTPS (M29 path was rcgen-backed; v1 desktop UI is localhost-only so plain HTTP).

### DataFrame ID registry

The server keeps a `HashMap<i64, *mut DataFrame_payload>` (or equivalent). The "primary" df (passed to `serve`) is ID 0. Filter/groupby endpoints register derived DataFrames at fresh IDs and return the new ID. The JS frontend remembers the current ID and includes `?df=ID` in subsequent rows/cell requests.

For v1 simplicity: the derived-DF registry is unbounded (no LRU eviction; GC keeps them alive because the registry holds strong refs). Document the memory implication.

### DataFrame → JSON serialization helpers

Internal helpers in `m50a_*` namespace:

- `m50a_serialize_schema(df) -> String` — emits `{"names":[...], "dtypes":[...], "nrows": N, "has_index": bool, "index_name": "...", "index_nlevels": N}`.
- `m50a_serialize_rows(df, start, stop) -> String` — emits `{"rows": [[val, val, ...], ...]}` where each cell is JSON-encoded per dtype (i64 → number, f64 → number, str → string, bool → boolean, datetime → ISO-8601 string, null → `null`).
- `m50a_serialize_cell(df, row, col) -> String` — single-cell variant.

Use the M34 JsonValue tree machinery if it speeds writing — or hand-roll the JSON for performance. Either works for v1; pick whichever is shorter.

### Commit checkpoint after Phase A

`M50a A: HTTP server + DataFrame-to-JSON serialization + serve_with_timeout`. Build clean + 2 smoke tests that boot the server with a 200ms timeout and curl /api/schema using `std::net::TcpStream` from the test side.

## Phase B — Read-only endpoints (~300-400 LOC)

Implement the 3 GET endpoints:

- **GET /** → bundled HTML page (Phase D ships the real one; for Phase B use a placeholder: a single `<h1>tabular.serve OK</h1>`).
- **GET /api/schema** → 200 OK + JSON body from `m50a_serialize_schema`.
- **GET /api/rows?start=N&stop=M&df=ID** → 200 OK + JSON from `m50a_serialize_rows`. Query string parsing: hand-rolled (just split on `&` and `=`; URL-decode `%20` style if needed but the v1 demo can avoid spaces in keys).
- **GET /api/cell?row=R&col=C&df=ID** → 200 OK + JSON from `m50a_serialize_cell`.

Error handling: missing `df` param defaults to ID 0 (the primary). Out-of-range `df` → 404. Out-of-range row/col → 400 with a JSON error body.

### Commit checkpoint after Phase B

`M50a B: read-only endpoints (schema, rows, cell)`. Build clean + 5 tests covering each endpoint + error responses.

## Phase C — Filter + groupby endpoints (~300-400 LOC)

### POST /api/filter

Request body (JSON): `{"df": ID, "column": "colname", "op": "gt|lt|eq|ne|ge|le", "value": <typed>}`. Server-side:

1. Look up the source df by ID.
2. Find the column by name; dispatch on dtype.
3. Build the ColumnBool mask using the existing M38/M42 comparison handlers (you'll need a small `m50a_apply_filter_op` helper that maps op strings to the existing native handlers).
4. Apply `df.filter(mask)` → derived DataFrame.
5. Register the derived DF at a fresh ID; return `{"df": NEW_ID, "nrows": M}`.

For v1, only support single-column filters. Composite filters (AND/OR) can be M50b.

### POST /api/groupby

Request body: `{"df": ID, "by": ["col1", "col2"], "agg": {"col3": "sum", "col4": "mean"}}`. Server-side:

1. Look up the source df.
2. `df.group_by(by)` → GroupedDataFrame.
3. `gdf.agg(specs)` where specs is built from the JSON.
4. Register the derived DF at a fresh ID; return `{"df": NEW_ID, "nrows": N}`.

### Commit checkpoint after Phase C

`M50a C: filter + groupby endpoints`. Build clean + 4 tests (filter happy path + groupby happy path + invalid op + invalid column).

## Phase D — Minimal HTML+JS frontend (~300-400 LOC)

Bundled as a Rust string constant in `m50a_BUNDLED_HTML`. The JS uses **vanilla DOM** (no React/Vue/jQuery). Functional, not pretty.

### Structure

```
<!DOCTYPE html>
<html>
<head><title>tabular.serve</title>
<style>...minimal styling — borders + monospace + sticky header...</style>
</head>
<body>
  <div id="header"></div>
  <div id="filter-bar"></div>
  <div id="groupby-bar"></div>
  <table id="data"><thead></thead><tbody></tbody></table>
  <div id="status"></div>
<script>
  // Module-pattern JS: ~200 lines
  // - fetchSchema(df=0)
  // - fetchRows(df=0, start, stop)
  // - renderHeader(schema)
  // - renderRows(rows, schema)
  // - renderFilterBar(schema) — input + op-dropdown per column
  // - submitFilter() — POST /api/filter, update current df ID, reload rows
  // - renderGroupbyBar(schema) — checkbox per col (group by) + agg dropdown per col
  // - submitGroupby() — POST /api/groupby
  // - infinite scroll: fetchRows on scroll-near-bottom
  // - reset button → df ID back to 0
  // - status bar shows current df ID + row count + applied filter/groupby
</script>
</body></html>
```

### v1 deliberate simplifications

- No virtual scrolling — load 100 rows at a time, lazy-load on scroll.
- No sortable column headers (M50b polish).
- Filter is one-column-at-a-time (M50b composite).
- No multi-column groupby UI clarity — just a list of checkboxes.
- No CSV download (M50b).
- No styling beyond "looks like a spreadsheet."

### Commit checkpoint after Phase D

`M50a D: bundled HTML/JS frontend`. Build clean + 1 demo test that boots the server with 1000ms timeout + curls `/` and verifies the HTML contains the expected `<title>` and a `<script>` tag.

## Phase E — Tests + demo + LANGUAGE_GUIDE + agent report (~250-300 LOC)

### Tests (`vm/tests/m50a_tabular_serve.rs`)

Aim for 12-18 tests. **Test strategy**: launch the server in a child thread (via `std::thread::spawn`) with a 500-2000ms timeout, make HTTP requests against it via `std::net::TcpStream`, parse responses, verify. After the timeout elapses the server shuts down naturally.

Cover:
- Phase A: server boots + timeout shuts it down cleanly; serialize_schema produces expected JSON shape; serialize_rows handles nulls + each dtype.
- Phase B: GET /api/schema; GET /api/rows happy + out-of-range; GET /api/cell happy + 400 on bad inputs; missing df defaults to 0.
- Phase C: POST /api/filter eq/gt on i64 + str + with nulls; POST /api/groupby single-col + multi-col; invalid op returns 400.
- Phase D: GET / returns the bundled HTML.

### Demo

Add `examples/tabular_serve_demo.spy` (~80-120 LOC). Loads a sample CSV, opens the server, prints the URL, runs for 30 seconds, then exits. Document that the user is expected to open a browser at the printed URL.

Testable via `compiler/tests/tabular_serve_demo_runs.rs` — the demo-runs test runs the demo with a 1-2 second timeout, then asserts the program exited cleanly (no panic, port was bound, basic responses worked).

### LANGUAGE_GUIDE.md update

§5 tabular gets an "M50a additions" subsection. New §11.39 documents the v1 desktop-UI scope-down (no HTTPS; localhost-only; vanilla JS; no virtualscroll; one-column filter at a time).

Bump banner to post-M50a.

### Commit checkpoint after Phase E

`M50a E: tabular serve — tests + demo + LANGUAGE_GUIDE update + agent report`.

## Acceptance criteria (machine-checkable)

1. `cargo build --workspace --release` — clean, no new warnings.
2. `cargo test --release -p strictpy-vm --test m50a_tabular_serve` — all pass.
3. `cargo test --release -p strictpy-compiler --test tabular_serve_demo_runs` — passes.
4. **No M37-M49 regressions**: targeted sweeps pass byte-identically.
5. **Full sweep**: 1016 + N passing (N new M50a tests, target 12-18). Net should be at least 1016 + 10.
6. **Manual smoke test (optional but expected)**: `spy examples/tabular_serve_demo.spy` runs; user opens browser to localhost:<port> and sees the rendered table.

## Constraints — files NOT to modify

- `seed_prelude` in `compiler/src/resolver.rs`.
- M37-M49 tests — must keep passing untouched.
- The 12 existing tabular demos — add a separate `tabular_serve_demo.spy`.
- DO NOT touch `vm/Cargo.toml` for new deps; std::net is sufficient.

## STOP CRITERIA — priority drops if budget runs out

Six priority drops, in order. **Phase A + Phase B (serve_with_timeout + schema/rows endpoints) are the must-ship core — never drop them**.

1. **Drop Phase D HTML/JS frontend** — ship the HTTP API only. M50b becomes "build the frontend"; users can curl the API in v1.
2. **Drop Phase C POST /api/groupby** — keep POST /api/filter only. groupby UI is M50c anyway.
3. **Drop Phase C POST /api/filter** — keep read-only endpoints only. M50b adds interactive endpoints.
4. **Drop Phase B GET /api/cell** — keep schema + rows only.
5. **Drop the demo test** — keep the unit tests only (orchestrator extends an existing demo to mention serve).
6. **Drop the LANGUAGE_GUIDE rewrite** — orchestrator finishes it.

After applying any drop, document what was cut with a "what M50b should pick up" list.

## Methodology discipline

1. **First commit at ~20% of budget** — Phase A's server loop + serve_with_timeout + serialize_schema + 1 smoke test.
2. **Per-phase commits** — 5 commits (A, B, C, D, E). Disjoint-handler.
3. **Variable prefix `m50a_`** in shared files.
4. **No new IR opcodes** — pure handler bodies + new NativeFn registrations.
5. **Edit-tool worktree leak**: precautionary `cp` at session start.
6. **Test mode for HTTP**: tests use `serve_with_timeout` exclusively; do NOT have tests call the blocking `serve()` (would hang the test runner).

## Final report

Write `docs/thesis/agent_reports/m50a_tabular_serve.md` (under 600 words) covering:
- What shipped per phase (A-E)
- What was cut from STOP CRITERIA (if anything)
- LOC delta per touched file
- Final test count + verification
- Surprises / design calls (e.g., did the std::net + custom HTTP/1.1 parser need any unexpected boilerplate? did the DataFrame ID registry need GC integration?)
- "What M50b should pick up" — concrete list (sortable column headers, composite filters, virtual scrolling, CSV download, better styling, pivot UI for M50c).
- LANGUAGE_GUIDE.md update status
- Whether the Edit-tool worktree leak recurred + count + workaround effectiveness

Commit this report in Phase E's commit.

## Commit message shape (final)

```
M50a: tabular.serve HTTP transport + minimal browser-tab frontend

First milestone in the M50 desktop-UI sequence. Ships:
- tabular.serve_with_timeout(df, port, timeout_ms) and
  tabular.serve(df, port) — hand-rolled HTTP/1.1 server in Rust
  (vm/src/builtins.rs::m50a_serve_loop) using std::net directly.
  No crate deps; no M28 socket stdlib or M29 framework dependency.
- DataFrame-to-JSON serialization helpers (m50a_serialize_*).
- 5 endpoints: GET / (bundled HTML), GET /api/schema, GET /api/rows,
  GET /api/cell, POST /api/filter, POST /api/groupby.
- Server-side DataFrame ID registry for derived DFs from
  filter/groupby operations (no LRU; relies on GC).
- Minimal vanilla-JS frontend bundled as a Rust string constant:
  table with lazy-load-on-scroll + one-column filter UI +
  groupby checkbox UI.

Phase A: HTTP server core + serialize_schema/rows/cell helpers.
Phase B: read-only endpoints (schema, rows, cell).
Phase C: filter + groupby endpoints + DataFrame ID registry.
Phase D: bundled HTML+JS frontend (vanilla DOM, ~200 LOC JS).
Phase E: ~15 tests + tabular_serve_demo.spy + LANGUAGE_GUIDE
update.

NativeFn IDs 1067-... Variable prefix m50a_.
Tests: 1016 → 1016+N (N = new M50a tests).

M50b will polish the frontend (sortable headers, composite filters,
virtual scroll, CSV download, better styling). M50c will add
the pivot/groupby UI.
```
