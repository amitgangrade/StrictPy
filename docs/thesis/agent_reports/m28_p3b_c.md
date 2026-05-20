# M28 P3b-C — `http_client` stdlib module

**Brief**: Ship a synchronous HTTP/1.1 client as a single stdlib
module, riding the M19-established stdlib-module-table infrastructure
that now has 35 modules behind it.  The brief earmarked NativeFn IDs
**620–649** (30 slots) and emphasised the **single hardest-won
methodology lesson** of the M22-M27 rounds: commit the first version
of the work *before* spending budget on the long-form report.

**Wall-clock**: ~80 minutes agent compute.  The cold build of `ureq`
+ `rustls` + `webpki-roots` + `url` + the rest of the new transitive
crate set was the dominant single contributor (~3m 36s on the build
host); handler logic was ~30 minutes.

**Files changed**:
1. `vm/Cargo.toml` — `ureq = { version = "2", default-features =
   false, features = ["tls"] }` + `url = "2"`.  Two top-level deps,
   ~40 transitive (rustls, ring, webpki, idna, icu_*, etc.).
2. `shared/src/native.rs` — 11 `NativeFn` variants in the 620-630
   range with `from_u32` arms.  631-649 reserved for v0.3
   (connection pooling, cookies, redirect policy hooks).
3. `compiler/src/resolver.rs` — one `StdlibModule` registration
   appended at the end of `seed_stdlib_modules`.  Uses three
   composite type aliases (`p3b_c_status_body_tuple_ty`,
   `p3b_c_status_hdrs_body_tuple_ty`, `p3b_c_url_parse_tuple_ty`)
   for readability — the signatures are dense enough that inlining
   them at every `StdlibItem` would have crossed the readability
   threshold.
4. `vm/src/builtins.rs` — 11 dispatch arms + 6 module-private
   helpers (`p3b_c_read_header_pairs`, `p3b_c_simple_request`,
   `p3b_c_full_request`, `p3b_c_collect_response_headers`,
   `p3b_c_read_response_body`, `p3b_c_read_response_body_lossy`)
   + a 60-entry `http_status_text` static table.  Every loop
   variable and intermediate binding uses the `p3b_c_<fn>_<purpose>`
   prefix per the M27 P3c-D methodology note (cherry-pick alignment).
5. `STRICTPY_SPEC.md` — §9.42 (orchestrator renumbers).
6. `examples/http_client_demo.spy` — hermetic offline demo (~120
   LOC) exercising `url_parse`, `urlencode`/`urldecode`,
   `status_text`, plus the `ValueError` path on a malformed URL.
7. `compiler/tests/http_client_demo_runs.rs` — 7 integration
   tests: 2 hermetic (compile-only + demo subprocess), 4 loopback
   tests that spawn a `std::net::TcpListener` server on
   `127.0.0.1:0` inside the harness and feed StrictPy code at the
   resulting port (GET 200, POST body, 404 returned-not-raised,
   `request_with_headers` collects `X-Test` response header), plus
   one direct `urlencode` round-trip test.

## API surface (11 functions, IDs 620-630)

| ID  | Name                    | Signature |
|-----|-------------------------|-----------|
| 620 | `get`                   | `(str) -> (i32, str)` |
| 621 | `post`                  | `(str, str, str) -> (i32, str)` |
| 622 | `put`                   | `(str, str, str) -> (i32, str)` |
| 623 | `delete`                | `(str) -> (i32, str)` |
| 624 | `head`                  | `(str) -> (i32, str)` |
| 625 | `request`               | `(str, str, str, [(str, str)], f64) -> (i32, str)` |
| 626 | `request_with_headers`  | `... -> (i32, [(str, str)], str)` |
| 627 | `urlencode`             | `([(str, str)]) -> str` |
| 628 | `urldecode`             | `(str) -> str` |
| 629 | `url_parse`             | `(str) -> (str, str, i32, str)` |
| 630 | `status_text`           | `(i32) -> str` |

631-649 reserved (the brief allocated 30, the surface used 11).

## Why stateless — no SharedVm slot table

Unlike `sqlite3` (M23 P3a-D), `zipfile` / `tarfile` (M27 P3c-D), or
`threading` (M23 P3a-C), every `http_client` handler is a pure
function of its arguments.  No persistent connection, no cookie
store, no session state.  Each call opens a fresh socket via `ureq`,
sends the request, drains the response, closes.  This means:

* No `Interpreter::shared` field additions — `interp.rs` is
  untouched.
* No cross-thread synchronisation concerns — two threads each calling
  `http_client.get(url)` interact only at the kernel-socket layer.
* The cost: a small per-call socket setup overhead (~1-3ms on
  localhost, ~50-200ms over the public internet) instead of
  pool-amortised reuse.  Acceptable for v0.2's "stdlib coverage"
  goal; connection pooling is one of the v0.3 candidates in the
  629-649 reserved range.

## Why `ureq` + `url`

The brief allowed the agent to choose between `ureq`, `reqwest::blocking`,
and hand-rolling on `rustls` + `http`.  `ureq` won on three axes:

1. **Synchronous API.** `reqwest::blocking` is a thin shim over the
   async `reqwest` runtime, dragging `tokio` into the build.  ureq
   uses a hand-built sync I/O loop — no runtime.
2. **TLS bundled, no system OpenSSL.** The `tls` feature pulls
   `rustls` + `webpki-roots` (a Mozilla CA bundle compiled in).
   No `cargo run` failure on a fresh CI image because libssl-dev
   isn't installed.
3. **Small dep graph.** With `default-features = false` (drops the
   cookie store we don't need), the addition is ~40 transitive
   crates.  Most of those are shared with the rest of the
   workspace (regex / icu / etc. already pulled by `url`).

`url` is added as a separate top-level dep because:

* URL parsing has corner cases (IPv6 literal hosts, IDN punycode,
  ports above u16, `userinfo` syntax) that the hand-rolled
  alternative would either get wrong or grow to ~200 LOC of careful
  state-machine code.
* The crate is widely used in the Rust ecosystem (already a
  transitive dep of `ureq` via the `rustls` chain), so the
  marginal binary-size cost is essentially zero.

## Why 4xx / 5xx are returned, not raised

Mirrors Python's `requests` library: `requests.get(url).status_code`
is a regular integer; `raise_for_status()` is opt-in.  The
alternative — raising an `IOError` for every non-2xx response — is
hostile to the common pattern "check for 404 to decide whether to
create vs. update".  ureq itself signals this via
`Err(ureq::Error::Status(code, resp))`, which we decode into the
same `(code, body)` shape the success path produces, so callers
don't have to special-case the error type.

## The `request_with_headers` shape

The brief asked for both `request` (status + body) and
`request_with_headers` (status + response headers + body) variants.
The motivation: most callers don't want the response-header dict
because building it costs N string allocations + N tuple allocations
+ a list allocation, and most calls don't care.  `request` skips
all that work and returns just the two fields the caller asked for.

Implementation note: `ureq::Response` is **not** `Clone`, so the
header-collection path has to run *before* `into_reader` consumes
the response.  Both branches in `p3b_c_full_request` do this
inline rather than going through the shared `p3b_c_decode_response`
helper, which the convenience methods use.

## `url_parse` corner cases

* `http://example.com` (no path) → `("/", ...)`, not `""`.  Matches
  the curl / Postman / browser behaviour where the root path is
  implicit.
* `http://example.com:8080/x?q=1` → port `8080`, path `/x?q=1`.
  The query string travels with the path (matches the way Python's
  `urlparse` exposes `.path` + `.query` as separate fields, except
  we concatenate them for the v0.2 4-element-tuple shape; a
  5-tuple with query separate would have been the alternative).
* `https://example.com` (no port) → port `443`.
* `http://example.com` (no port) → port `80`.
* `not a url` → `ValueError`.  `url::Url::parse` rejects relative
  references; v0.2 only handles absolute URLs.

## Hardest three things (in retrospect)

1. **`ureq::Response` is not `Clone`.** First draft of
   `p3b_c_full_request` called `p3b_c_decode_response(resp_result.clone(), …)`
   to get the (status, body) pair, then peeked at the response again
   for headers.  This doesn't compile — Response wraps an open
   socket, so cloning would be a use-after-free hazard.  The fix is
   to do the header collection *before* `into_reader` consumes the
   response in the headers branch.  This also pushed the
   header-collection helper into its own function so the success and
   4xx/5xx arms can share it.

2. **Test scaffolding for loopback HTTP.**  A tiny test server is
   ~50 LOC of `TcpListener` + `BufReader::read_line` + canned
   response.  The trick is *not* trying to be a real HTTP server —
   the test server doesn't parse the body, doesn't honour
   `Connection: keep-alive`, doesn't even care about the HTTP
   method.  It just consumes the headers until the blank line,
   drains the announced `Content-Length`, and emits the canned
   response.  Three layers of timeout (`set_read_timeout`, the
   `n_requests` counter, `recv_timeout` on the port channel) prevent
   any test from hanging if the client doesn't connect.

3. **Multi-line type annotations don't parse.**  Hit twice — once
   in the demo, once in the loopback test source.  StrictPy parses
   `x: T = expr` as a single line; continuing onto the next line
   produces `expected expression, found Newline`.  Fixed by
   inlining; documented here for future agents because the parser
   error message is opaque if you're not expecting this restriction.

## Incidental bugs / oddities

Zero.  The stdlib-module seam absorbed one more module without
complaint — same shape as M22-M27 predecessors.  The interesting
data point is that this is the first stdlib module that introduces
network I/O; the existing test infrastructure absorbed it via the
loopback-server pattern without needing any new VM- or
interpreter-side hooks.

## Cross-platform notes

* `ureq` is pure Rust with rustls TLS; builds and runs cleanly on
  Windows / Linux / macOS without `cfg(target_os = ...)` gates.
* `webpki-roots` ships the same Mozilla CA bundle on every platform,
  so TLS chain validation behaves identically across hosts.
* The loopback server uses `std::net::TcpListener::bind("127.0.0.1:0")`
  which works identically on all three platforms (Windows Firewall
  does not prompt for outbound loopback).
* No platform-specific code in any of the new files.

## Test totals

`cargo test --workspace --release` was running at report-write time
to confirm the full workspace stays green.  The new file
`compiler/tests/http_client_demo_runs.rs` contributes **7 tests**:

* `http_client_demo_compiles` — compile-only check on the demo.
* `http_client_demo_runs_via_spy_exe` — drives the demo through
  the built `spy.exe` and asserts on 16 stdout markers.
* `http_client_loopback_get_returns_200_and_body` — GET against
  loopback, asserts 200 + body.
* `http_client_loopback_post_sends_body` — POST against loopback
  with form-encoded body, asserts 201 + response body.
* `http_client_loopback_404_returned_not_raised` — confirms 4xx
  is a return value, not an exception.
* `http_client_loopback_request_with_headers_captures_x_test` —
  `request_with_headers` round-trip, verifies an `X-Test` server
  header appears in the response-headers list.
* `http_client_urlencode_roundtrip_via_demo` — focused
  `urlencode` check.

## What's next (v0.3 candidates, IDs 631-649 reserved)

* **Connection pooling.**  A `Session` opaque-handle pattern (M23
  sqlite3-style) would amortise the TCP + TLS handshake.
* **Cookie store.**  Re-enable the ureq cookie feature and expose
  a session-scoped cookie jar.
* **Streaming uploads / downloads.**  Today the body must fit in
  memory (64 MiB cap).  A real bytes type would also help here.
* **Redirect policy.**  Configurable max-redirects + a "raise on
  redirect" mode.
* **Per-request auth helpers.**  `http_client.basic_auth(user, pw)`
  → `("Authorization", "Basic …")` — convenience over the manual
  `request_with_headers` path.
* **HTTP/2.**  Would require swapping out `ureq`; not a priority
  while HTTP/1.1 covers >95% of stdlib use cases.

## Methodology notes

* **First-commit discipline.**  The brief's Lesson 1 — commit
  before 60% of budget — was honoured by staging
  resolver/native/builtins changes + demo + tests + spec into a
  single first commit *before* writing this report.  Even if the
  report-writing path were to hit a budget wall, the orchestrator
  would have a clean integrable commit.
* **Cherry-pick alignment.**  All loop variables and intermediate
  bindings use the `p3b_c_<purpose>` prefix per the M27 P3c-D
  precedent.  None of the new lines collide verbatim with sqlite3 /
  zipfile / tarfile / logging handlers — git's three-way merge
  should have no false-anchor candidates.
* **Hermetic demo + integration test split.**  The demo runs
  offline (so `cargo run --bin spy -- examples/http_client_demo.spy`
  works on any laptop, including air-gapped CI runners).  The
  loopback tests live in the test harness because they need
  scaffolding (server spawn, port discovery) that doesn't belong in
  user-facing example code.

The bet from M19 ("a stable module-table makes new modules trivial
to ship") continues to pay out: **35 stdlib modules** now slot into
the same `seed_stdlib_modules` registration, and none of them have
required changes to the resolver / typecheck / IR / codegen layers.
http_client is the first network-I/O module; the next pressure test
will be modules that need new runtime infrastructure (true binary
bytes, async, multiprocess) — those are v0.3 work.
