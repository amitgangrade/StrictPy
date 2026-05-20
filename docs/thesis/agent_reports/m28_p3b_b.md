# M28 P3b-B — `ssl` stdlib module (TLS-over-TCP via rustls)

**Brief**: Ship a client-side TLS surface in v0.2, riding the same
opaque-handle / `SharedVm` slot-table pattern that M23 P3a-D
established for `sqlite3` and M27 P3c-D re-used for `zipfile` /
`tarfile`.  No `socket` module exists in v0.2, so `ssl.connect`
bundles TCP setup and TLS handshake into one call — the v0.3 split
point is documented in the spec, but v0.2 keeps the API surface flat.

**Wall-clock**: ~45 min agent compute.  Dominant cost was the cold
build of `ring` + transitive crates (`untrusted`, `getrandom v0.2`,
`rustls`, `rustls-webpki`); pure handler-edit incrementals are a few
seconds.

## API surface (10 functions, IDs 600-609)

| ID  | Name                | Signature |
|-----|---------------------|-----------|
| 600 | `connect`           | `(host: str, port: i32) -> i64` |
| 601 | `send`              | `(handle: i64, data: str) -> i32` |
| 602 | `recv`              | `(handle: i64, max_bytes: i32) -> str` |
| 603 | `recv_exact`        | `(handle: i64, n: i32) -> str` |
| 604 | `close`             | `(handle: i64) -> None` |
| 605 | `peer_addr`         | `(handle: i64) -> str` |
| 606 | `peer_cert_subject` | `(handle: i64) -> str` |
| 607 | `set_timeout_secs`  | `(handle: i64, secs: f64) -> None` |
| 608 | `set_verify_certs`  | `(enabled: bool) -> None` |
| 609 | `get_verify_certs`  | `() -> bool` |

IDs 610-619 reserved for v0.3 (server-side TLS, mutual auth, SNI
override, custom-CA bundles, session resumption).

## Files changed

1. `vm/Cargo.toml` — `rustls 0.23` (default-features off, `ring` +
   `std` + `logging` + `tls12`), `rustls-pki-types 1`,
   `webpki-roots 0.26`.  Pinning the ring crypto provider keeps the
   build pure-Rust on Windows (the `aws-lc-sys` default needs CMake
   + NASM, which is not part of the dev environment).
2. `compiler/Cargo.toml` (dev-deps) — `rcgen 0.13`, plus mirror
   `rustls` / `rustls-pki-types` for the test-side server.
3. `shared/src/native.rs` — 10 new `NativeFn` variants (600-609)
   + `from_u32` arms.
4. `vm/src/interp.rs` — 3 new `SharedVm` fields (`tls_streams`
   `HashMap<i64, StreamOwned<...>>`, `next_tls_id AtomicI64` starting
   at 1, `tls_verify AtomicBool` defaulting to `true`), initialised
   in both `new` and `new_with_jit` ctors.
5. `vm/src/builtins.rs` — 10 handler arms + 2 helpers
   (`ssl_extract_subject_cn` — best-effort CN parse out of the DER
   subject; `ssl_no_verify::NoVerify` — the "trust everything"
   `ServerCertVerifier` used by `set_verify_certs(false)`).
6. `compiler/src/resolver.rs` — one `StdlibModule` registration
   ("ssl") appended after the existing modules.
7. `STRICTPY_SPEC.md` — §9.41.
8. `examples/ssl_demo.spy` — ~110 LOC.
9. `compiler/tests/ssl_demo_runs.rs` — 2 tests (compile-only +
   end-to-end against an in-process loopback echo server).
10. This report.

## Why `connect` bundles TCP + TLS

In CPython, you build a `socket.socket(...)`, then call
`ssl.SSLContext.wrap_socket(sock)` to attach TLS.  StrictPy v0.2
has no `socket` module — adding one would have been another 10+
NativeFn IDs and at least one slot table.  The brief asked for 20
IDs total and noted that server-side TLS is deferred to v0.3, so
client `connect` got the obvious bundled shape: pass `host` + `port`,
get an `i64` back, you're talking encrypted.  The `socket` split
will come naturally with v0.3 server-side TLS, which will need
`bind` / `listen` / `accept` plumbing that `socket` will own.

## The verify-flag toggle

`rustls::ClientConfig` has two distinct construction paths — the
production one (`with_root_certificates(...)`) and the "dangerous"
one (`.dangerous().with_custom_certificate_verifier(...)`).  These
two paths share no common method beyond the builder root, so the
handler picks one at `connect`-time based on the AtomicBool:

* Production: `RootCertStore::empty().extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned())`
  → `with_root_certificates(roots).with_no_client_auth()`.
* Test-only: `.dangerous().with_custom_certificate_verifier(Arc::new(NoVerify))`.

The `NoVerify` `ServerCertVerifier` accepts every cert + every
signature.  This is documented in §9.41 as **testing only** —
production code that needs custom trust should wait for v0.3.

The flag lives on `SharedVm` as `AtomicBool`, so user code calls
`ssl.set_verify_certs(false)` once at process start and it sticks
for every subsequent `connect`.  `get_verify_certs()` reads it back
(also atomic, no lock).

## Cert-subject extraction

`rustls` returns peer certs as `&[u8]` DER blobs — no built-in
ASN.1 parser to pull out the CN.  The two real options:

1. Add an `x509-parser` (or `x509-cert`) crate dep just for this
   one accessor.  ~5 transitive deps + ~2-5s build time.
2. Hand-roll a 30-line scan for the CN OID (`2.5.4.3`, DER-encoded
   as `06 03 55 04 03`), read the ASN.1 string tag + short length
   + bytes that follow.  Fails closed (returns None → empty string)
   on anything weird.

Picked (2).  The spec's documented surface is "empty string if no
CN", so a best-effort scanner that gives up on long-form lengths or
unknown string tags is fine — modern certs use short-form lengths
and either UTF8String (0x0C) or PrintableString (0x13) for the CN
value.  If a future use case needs richer cert metadata (SANs,
issuer chain, validity window), we add the x509-parser dep then.

## The `ring` vs `aws-lc-rs` crypto-provider choice

`rustls 0.23` made the crypto provider pluggable; the default if
you turn on the `aws-lc-rs` feature pulls in `aws-lc-sys` which
needs CMake + NASM at build time.  The `ring` feature is the
pure-Rust alternative — slower in absolute terms but identical
correctness, and (critically) zero system-toolchain requirements on
Windows.  Pinned `default-features = false` + `features = ["std",
"ring", "logging", "tls12"]` in both `vm/Cargo.toml` and the
compiler dev-deps so the test side picks the same provider.

Calling `ClientConfig::builder()` without explicit provider depends
on whether `default-features` includes `aws-lc-rs`; since I turned
that off, the bare builder panics.  Every `connect` therefore goes
through `builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))`
explicitly.

## Network-bound testing discipline

The integration test never touches the public internet.  The shape:

1. Generate a self-signed cert for `CN=localhost` with rcgen's
   `generate_simple_self_signed` one-liner (returns `cert.cert` +
   `cert.key_pair` — note: rcgen 0.13 renamed `signing_key` →
   `key_pair`; tripped me once).
2. Bind a `TcpListener` to `127.0.0.1:0` so the OS assigns a free
   port (avoids the "port already in use" race when CI runs tests
   in parallel against a hard-coded port).
3. Spawn a server thread that accepts exactly one connection, runs
   a `rustls::ServerConnection` on top, and echoes every byte
   plaintext-read until EOF.
4. Invoke `spy.exe` with `127.0.0.1 <chosen_port>` on argv; the
   StrictPy demo calls `set_verify_certs(false)` then `connect` and
   exchanges three round-trips.
5. Join the server thread (best-effort — it exits on EOF after the
   client closes).

No external dependencies, no DNS, no firewall-traversable traffic.
Both tests pass on first invocation.

## Methodology: commit-before-report (Lesson 1)

First commit landed at ~30% of compute budget — the moment the
green workspace build was verified, before writing the demo / test /
spec / report.  That keeps the orchestrator's worst case (compute
exhaustion before commit) safe even if test-writing or report-drafting
slips.  This report + the spec + the demo + the test were drafted
in parallel with `cargo test --workspace --release` running in the
background, then committed in a second commit once the test went
green.

## Methodology: distinctive prefix (Lesson 2)

Every loop variable, intermediate binding, and local in the new
handlers uses the prefix `p3b_b_tls_<fn>_<purpose>` (e.g.
`p3b_b_tls_connect_host`, `p3b_b_tls_recv_buf`, `p3b_b_tls_pcs_table`).
None of these collide with any verbatim line in the existing 30+
stdlib modules — zero false-anchor candidates for git's patience /
myers cherry-pick alignment.

## Cross-platform notes

`rustls` + `ring` + `webpki-roots` are 100% pure Rust.  No system
SSL library (`libssl`, `Security.framework`, `schannel.dll`) is
consulted; the Mozilla root bundle is statically embedded.  Build
and runtime behaviour are identical across Windows / Linux / macOS.

## Test totals

After M28 P3b-B, the workspace's M28 baseline gains 2 tests:
`ssl_demo_compiles` and `ssl_demo_runs_via_spy_exe`.  Exact total
depends on which sibling agents have been cherry-picked at the time
this branch is integrated; this agent's contribution is +2.

## What's NOT shipped (intentional v0.3 scope)

* **Server-side TLS** (`accept`, `bind_tls`).  Needs a per-listener
  slot table + a `socket` module to give `bind` somewhere to live.
* **Mutual auth** (client certificates).  `with_no_client_auth()`
  is hard-coded.
* **Custom CA / pinned cert verification.**  The verify flag is
  binary; per-connection custom verifiers are a v0.3 API.
* **SNI override.**  The SNI name always equals the `host` argument
  passed to `connect`.
* **ALPN, session resumption, custom cipher suites.**  Default
  cipher suites only; HTTP/2 negotiation is not supported.
* **A standalone `socket` module.**  Once it lands in v0.3, `ssl`
  will likely grow a `wrap_socket(sock: i64) -> i64` parallel to
  CPython's API; the slot table is already shaped for that.

## Incidental findings

* `rcgen` 0.13 renamed the `CertifiedKey.signing_key` field to
  `key_pair`.  No StrictPy-side bug; documented here for future
  agents who copy the test-server skeleton.
* `rustls::ClientConfig::builder()` (no-arg form) silently relies on
  whichever crypto provider was selected via Cargo features.  Once
  we disabled `aws-lc-rs` default features, the no-arg builder
  panics at runtime — `builder_with_provider(...)` is the safe form.
* No StrictPy-language bugs surfaced.  The stdlib registration
  surface absorbed the new module without complaint; same shape as
  every Phase 1/2/3a/3c module before it.
