# M29.5 — Tier 1 webserver round-out (keep-alive / chunked / multipart / shutdown / HTML errors)

**Brief**: take the M29 webserver framework + TODO demo and ship the
five Tier 1 features the M29 agent scoped out — HTTP keep-alive,
chunked transfer-encoding (both directions), `multipart/form-data`
parsing, graceful shutdown, and HTML error pages.  Application-code
work only — no edits to the stdlib (`vm/`, `compiler/`, `shared/`).

**Wall-clock**: ~2h of agent compute; first commit (all five features
in scaffold form) landed at ~55% of budget; final all-tests-green
commit at ~90%.

**Files changed (no stdlib edits)**:

| Path | LOC delta | Purpose |
|---|---|---|
| `examples/webserver/todo_app.spy` | 1446 → 2443 (+997) | All five features + 3 new handlers + multipart parser + chunked decoder/encoder + drain helper. |
| `compiler/tests/webserver_demo_runs.rs` | 290 → 654 (+364) | 5 new tests; existing 3 untouched. |
| `examples/webserver/uploads/.gitkeep` | +1 | New dir for upload-handler artefacts. |
| `.gitignore` | +5 | Ignore `examples/webserver/uploads/*` except `.gitkeep`. |

## What shipped

All five Tier 1 features landed cleanly, plus three new demo handlers
that exercise them:

### 1. HTTP keep-alive

The accept loop's per-connection handler (`handle_connection`) now
wraps the parse-dispatch-write cycle in a `while keep_going` loop
bounded by `keepalive_max_requests()` (= 100).  Persistence is decided
per-request:

* HTTP/1.1 defaults to keep-alive; HTTP/1.0 defaults to close.
* `Connection: close` from the client forces close.
* `Connection: keep-alive` overrides the version default.
* The server forces close after the per-connection cap is reached, or
  whenever the handler set `resp.force_close = true` (used for 500s
  and the 400-on-malformed path).

On a kept-alive connection, the second-and-subsequent requests run
with a 5s idle timeout via `wire_set_timeout(handle, 5.0, is_tls)`.
A timed-out recv surfaces as `IOError` and tears the loop down
cleanly.  Response headers correctly advertise `Connection: keep-alive`
and `Keep-Alive: timeout=5, max=N`, where N counts down across the
connection.

`parse_request` was changed to return a 4-tuple
`(ok, request, error_status, leftover_prefix)`; the leftover prefix
threads through to the next iteration so a `recv()` that overshot a
Content-Length-framed body doesn't drop the bytes for the next
request.

### 2. Chunked transfer-encoding (read + write)

* **Read**: `parse_chunked_body(handle, is_tls, body_prefix)` reads
  the chunked body off the wire.  Each chunk is `<hex>\r\n<bytes>\r\n`;
  the chunk-size line may carry semicolon-separated extensions that we
  silently discard.  A `0\r\n` chunk terminates the stream; trailing
  headers (RFC 7230 §4.4 trailer-part) are read-and-discarded up to a
  16-line cap.  A separate helper `read_until_crlf` does the line-
  oriented reads needed by the size line, sharing the same prefix-then-
  wire dance as `read_n_bytes`.
* **Write**: `Response` now carries a `chunked: bool` + `body_chunks:
  List[str]` field pair.  `resp_set_chunked(resp)` flips the response
  into chunked mode; `resp_add_chunk(resp, data)` appends one chunk.
  `write_response` then emits `Transfer-Encoding: chunked` (omitting
  `Content-Length`) and writes each chunk as `<hex>\r\n<bytes>\r\n`,
  ending with `0\r\n\r\n`.

The `/api/stream?n=N` handler uses this to stream N "chunk K\n" lines
with `time.sleep_ms(50)` between chunks — the test reads with `n=5`
to keep wall-clock low.

### 3. multipart/form-data

`MultipartPart` is a final class with the spec-mandated four fields
(`name`, `filename`, `content_type`, `body`).  `parse_multipart_body`
walks the body boundary-by-boundary, peeling off the
`Content-Disposition: form-data; name="X"; filename="Y"` header (only
the quoted form, which is what every browser sends) plus the part's
own `Content-Type`.  Trailing boundary `--<X>--` terminates.

**Scope**: single nesting level — we do not recurse into a part whose
own Content-Type is `multipart/mixed` / `multipart/related`.
Sufficient for the file-upload use case and the test; deeper nesting
is v0.3.  Capped at 64 parts per body to avoid an attacker streaming
a million empty parts.

`req_multipart_parts(req) -> Tuple[bool, List[MultipartPart]]` is the
public surface; it returns `(false, [])` on `Content-Type` that isn't
`multipart/form-data` or on a malformed body.

### 4. Graceful shutdown — via `--shutdown-after-secs N` flag

StrictPy v0.2 has no `signal` stdlib.  Per the brief's fallback
strategy, the demo accepts `--shutdown-after-secs N` and spawns a
timer thread that, after N seconds, signals shutdown:

1. **Set a `Lock`-backed shutdown flag.**  An unheld lock means
   "running"; a held lock means "shutdown requested".  The accept
   loop calls `shutdown_requested(flag)` (a non-blocking probe via
   `lock_try_acquire`) before each accept.
2. **Wake the blocked accept** by self-connecting to the listener
   with `socket.connect_tcp(host, port)`.  This is necessary because
   `socket.close_listener` does NOT unblock an in-flight accept (see
   surface finding §S1 below).
3. **Close the listener** so subsequent accepts also fail.

After the accept loop returns, `main` calls `drain_in_flight(sem,
permits, 10.0)` which acquires every permit from the per-connection
semaphore — when it can get them all, every connection-handling thread
has finished.  Logs a warning if drain didn't complete within 10s.

This satisfies the test
`webserver_demo_graceful_shutdown_completes_in_flight`: with
`--shutdown-after-secs 3`, an in-flight 500ms streaming request
completes cleanly before the server prints `SHUTDOWN` and exits.

### 5. HTML error pages

`error_html(status: i32) -> Response` returns a small (~10 LOC body)
inline HTML page with status code, RFC reason phrase, and a
description for 400 / 404 / 405 / 411 / 413 / 415 / 500 / fallback.
No CSS, no images.  `error_html_with_detail(status, detail)` adds a
`<pre>`-escaped exception detail block — used by `safe_dispatch` so a
500 from a handler exception surfaces the type + message safely
escaped via `html_escape`.

`router_dispatch` (404 + 405), `handle_static` (404 + 403), the
chunked-encoded parse-failure path in `handle_connection`, and
`dispatch_handler`'s 501 fallback all now return HTML.  Per-handler
JSON error responses (e.g. `handle_upload`'s 415 / 400 / 500) stay as
JSON — those are API-shape responses, not framework-level error
pages.

### Demo additions

* **POST `/api/upload`** — accepts `multipart/form-data` with a
  `file` field, sanitises the filename (basename only, no dots, no
  `..`, ASCII alnum/`.`/`-`/`_` only), writes to
  `examples/webserver/uploads/<safe_name>`, returns 201 + JSON `{"url":
  "/api/uploads/<name>", "size": N}`.
* **GET `/api/uploads/<*rest>`** — serves files from the uploads dir
  with a ext-based content-type table (txt / html / json / css / js
  / png / jpg / pdf / octet-stream).  Rejects `..` / `\` / `/` in the
  path with 403; missing file → HTML 404.
* **GET `/api/stream`** — streams N lines, one chunk per line, with
  `time.sleep_ms(50)` between chunks.  N defaults to 100 and can be
  lowered to ≤1000 via `?n=N`.  The test uses `n=5`.
* **Home page** now includes a `<form method="POST"
  enctype="multipart/form-data">` upload form so a browser can
  exercise the upload route.

## Tests

| Test | Behaviour exercised | Status |
|---|---|---|
| `webserver_demo_compiles` | M29 — compile-only sanity. | green (unchanged) |
| `webserver_demo_runs_http` | M29 — 12 HTTP round trips, happy + error paths. | green (unchanged) |
| `webserver_demo_runs_https` | M29 — TLS handshake + HTTPS round trip. | green (unchanged) |
| `webserver_demo_keepalive_serves_multiple_requests_on_one_conn` | Raw TcpStream sends 3 requests on one socket; verifies `Connection: keep-alive`, `Connection: close`, `Keep-Alive` headers. | green |
| `webserver_demo_chunked_response_streams_correctly` | `GET /api/stream?n=5`; verifies `Transfer-Encoding: chunked`, no `Content-Length`, all chunks present, `0\r\n\r\n` terminator. | green |
| `webserver_demo_multipart_upload_roundtrip` | Hand-built multipart POST → 201 with `{"url":...,"size":...}`; follow-up GET returns the exact uploaded bytes. | green |
| `webserver_demo_returns_html_404_not_textplain` | `GET /no/such/route` → 404 with `Content-Type: text/html` + `<h1>404` in body. | green |
| `webserver_demo_graceful_shutdown_completes_in_flight` | `--shutdown-after-secs 3` + in-flight streaming request (n=10, ~500ms) completes cleanly; server exits within 20s wall-clock. | green |

All 8 tests pass.  Single run takes ~3.5s on Windows 11.

## Surface findings (M28/M28.5)

### §S1. `socket.close_listener` does NOT unblock an in-flight `socket.accept`

This was the single most load-bearing discovery for the graceful
shutdown path.  The VM impl in `vm/src/builtins.rs:5402-5446` (the
`NativeFn::SocketAccept` arm) takes a `clone()` of the listener Arc
and drops the table mutex before calling `accept()` — so even after
`close_listener` takes the slot, the in-flight accept thread still
holds a live Arc and stays blocked on the OS-level accept syscall.

**Workaround we used** (in `shutdown_timer_thread`): self-connect to
the listener address with `socket.connect_tcp(host, port)` to wake
the blocked accept with a dummy connection, then drop it immediately.
This is the classic POSIX wake-up trick and works fine.

**Why this isn't a blocker for v0.2**: the surface as-is supports
graceful shutdown if you know to use the wake-up trick.  But it would
be more ergonomic if `close_listener` did a `shutdown()` on the
listener fd before dropping its Arc.  That'd let user code rely on
the simpler "set flag, close listener, accept returns IOError" loop.
Worth a §9.40 spec follow-up.

### §S2. Old M29 v0.2 ergonomic gaps still bit, exactly as M29 documented

* **No typed `JsonValue`**: I didn't touch the JSON parsing path
  (M29 already used `extract_json_text_field` to walk the canonical
  compact form), but it would have made the multipart parser cleaner
  if I'd had access to a real value tree for the form fields.  As-is
  the multipart parser hand-walks bytes, which is fine but ~150 LOC
  vs. ~50 LOC with a real parser.
* **BUG-039** (`name in dict` unreliable on `Dict[str, *]`): worked
  around with `dict.get()` / `is not none` everywhere, same as M29.
  Specifically painful in `write_response` where I want to *suppress*
  the `Content-Length` header when chunked — I had to use a
  "set-to-empty-string-then-skip-empties" sentinel pattern because
  there's no `dict.remove(k)` or `del d[k]` in v0.2.  Worth a §9
  follow-up to expose `Dict.pop(k)`.
* **No expression-unwrap (`x!`) for nullables**: I hit this twice in
  the multipart handler (finding the `file` part — I had to use a
  3-line "store into local, then index" dance rather than a single
  `parts.find(p => p.name == "file")!`).  Not a blocker, just verbose.
* **No closures across NativeFn boundary**: same as M29, didn't bite
  me because the handler-id indirection from M29 still works.  But
  the shutdown-timer's lambda captures four loop-local i64/str/i32
  values — the v0.2 immutable-capture rule (§8.6) handled them
  correctly.
* **No `set/clear` on `socket.set_timeout_secs(handle, 0.0)`?**
  Actually spec §9.40 says `0.0` clears the timeout, which I exploit
  to switch between the 30s first-request budget and the 5s
  keepalive idle budget.  Works as documented.

### §S3. `logging.warn` is not a thing — it's `logging.warning`

Trivial — I had it wrong on first attempt; the spec at §9.31 line
2379 says `fn warning(msg: str) -> None`.  Compiler emits a "missing
attribute" error at IR-lower time, which was the right level of
help.

### §S4. `Response` field-add inside a final class is painless

Adding `chunked: bool`, `body_chunks: List[str]`, and `force_close:
bool` to the existing `final class Response` was a one-line-per-field
diff in `__init__`.  No BUGS_KNOWN.md M11 sealed-class footguns
surfaced — because there's no inheritance involved, the v-table
caveats don't apply.

## LOC summary

* `todo_app.spy`: 1446 → 2443 (**+997 LOC**, +69%).  Roughly:
  * keep-alive refactor: ~150 LOC (mostly the rewrite of
    `handle_connection` from one-shot to per-conn loop + the
    `read_n_bytes` / `read_header_block_with_prefix` extraction).
  * chunked read+write: ~180 LOC (`parse_chunked_body`,
    `read_until_crlf`, `hex_string`, `resp_set_chunked` /
    `resp_add_chunk`, the `write_response` rewrite).
  * multipart parser + MultipartPart class: ~180 LOC.
  * graceful shutdown (timer + flag + drain): ~110 LOC.
  * HTML error pages: ~50 LOC.
  * new handlers (`/api/upload`, `/api/uploads/*`, `/api/stream`) +
    filename sanitisation + content-type tables: ~250 LOC.
  * misc — keepalive constants, response field additions,
    field-by-field route registration: ~80 LOC.
* `compiler/tests/webserver_demo_runs.rs`: 290 → 654 (**+364 LOC**).
  Most of the new code is the raw-`TcpStream` `read_http_response`
  helper (necessary for the keep-alive test, which needs to send
  multiple requests on one connection without ureq's connection
  pooling getting in the way) and the test bodies themselves (~30
  LOC each).

## Performance

I didn't re-run the M29 `ab` benchmarks because the changes are
shape-preserving on the throughput-critical path — `/health` requests
that don't use chunked encoding go through almost exactly the M29
hot path (the only new work is the keep-alive header decisions, ~5
extra branches per request).  Rough expectation: `/health` should
still hit ~2200 req/s on Windows 11; the additional new-connection
overhead for non-kept-alive clients is nil, and kept-alive clients
should be measurably faster because we skip the
accept-spawn-thread-handshake cost on requests 2..N.

Each /api/stream request takes ~50ms × N chunks ≈ 250ms for n=5 (the
test value) and ~5s for n=100 (the default — exercises the streaming
correctly on a manual test).

## Verdict

All five Tier 1 features shipped; all 3 existing tests and all 5
new tests pass.  The single new fragility I hit was the
`close_listener`-doesn't-unblock-accept gotcha, which was worked
around in user code with a self-wakeup connect.  No stdlib changes;
no probe-program needed (the gotcha is a documented quirk worth a
v0.3 §9.40 cleanup but not a hard bug).

The M29 framework absorbed roughly 1000 LOC of additional user code
without surfacing any new language footguns — the same v0.2
ergonomic gaps (no typed JSON, BUG-039, no expression-unwrap) bit in
the same ways M29 documented.  The patch is mostly mechanical:
header parsing, byte buffering, state-machine plumbing.  This is
exactly the workload the framework brief was designed to stress, and
it held up.
