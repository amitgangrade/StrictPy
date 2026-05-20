# M28 P3b-A — `socket` stdlib module

**Brief**: ship a raw TCP/UDP networking surface backed by `std::net`.
Same opaque-handle + slot-table shape as M27 P3c-D's zip/tar agent —
three new `SharedVm` slot tables (`tcp_streams`, `tcp_listeners`,
`udp_sockets`), 19 `NativeFn` variants in the 570-599 ID range, and
the str-as-byte-buffer convention M22's `struct` established for binary
payloads.  No new crate dep: `std::net` covers TCP, UDP, and DNS
resolution portably across Linux, macOS, and Windows.

**Wall-clock**: ~80 min (most of which was the first round-trip build
chain — release rebuild after the `interp.rs` SharedVm field change is
the dominant cost, ~2 min each).

**Files changed**:

1. `shared/src/native.rs` — 19 new `NativeFn` variants (IDs 570-588;
   589-599 reserved for v0.3) and matching `from_u32` arms.
2. `compiler/src/resolver.rs` — one `StdlibModule` registration
   (`"socket"`) appended at the end of `seed_stdlib_modules`.  Two
   tuple-return types (`Tuple[i64, str]` for `accept` and
   `Tuple[str, str, i32]` for `udp_recv_from`) declared inline.
3. `vm/src/interp.rs` — three new `SharedVm` fields wrapped in
   `Arc<Mutex<Vec<Option<Arc<T>>>>>` (see "Why double-Arc" below);
   matching `vec![None]` initialisers in both `SharedVm::new` and
   `new_with_jit`.
4. `vm/src/builtins.rs` — 19 handler arms (~550 LOC) plus two helper
   functions (`arc_tcp_stream`, `arc_udp_socket`).  Every loop /
   intermediate binding uses the `p3b_a_` prefix per the brief's
   Lesson 2 to dodge cherry-pick alignment with M27's `p3c_d_`
   handlers.
5. `STRICTPY_SPEC.md` — new §9.40 (~130 lines) covering the full API,
   error model, cross-platform notes, and the deferred list.
6. `examples/socket_demo.spy` — TCP loopback echo with a worker
   thread doing connect + send + recv, main doing accept + recv +
   send.  ~110 LOC.
7. `examples/socket_udp_demo.spy` — UDP loopback echo (single
   process, two sockets).  ~55 LOC.
8. `compiler/tests/socket_demo_runs.rs` — 4 subprocess tests
   (compile-only + run-and-assert for each demo).

## API surface (19 functions, IDs 570-588)

| ID  | Name                  | Signature |
|-----|-----------------------|-----------|
| 570 | `connect_tcp`         | `(host: str, port: i32) -> i64` |
| 571 | `send`                | `(h: i64, data: str) -> i32` |
| 572 | `recv`                | `(h: i64, max_bytes: i32) -> str` |
| 573 | `recv_exact`          | `(h: i64, n: i32) -> str` |
| 574 | `close`               | `(h: i64) -> None` |
| 575 | `set_timeout_secs`    | `(h: i64, secs: f64) -> None` |
| 576 | `peer_addr`           | `(h: i64) -> str` |
| 577 | `local_addr`          | `(h: i64) -> str` |
| 578 | `listen_tcp`          | `(host: str, port: i32, backlog: i32) -> i64` |
| 579 | `accept`              | `(l: i64) -> Tuple[i64, str]` |
| 580 | `close_listener`      | `(l: i64) -> None` |
| 581 | `udp_socket`          | `() -> i64` |
| 582 | `udp_bind`            | `(host: str, port: i32) -> i64` |
| 583 | `udp_send_to`         | `(h: i64, data: str, host: str, port: i32) -> i32` |
| 584 | `udp_recv_from`       | `(h: i64, max_bytes: i32) -> Tuple[str, str, i32]` |
| 585 | `udp_close`           | `(h: i64) -> None` |
| 586 | `gethostbyname`       | `(host: str) -> str` |
| 587 | `resolve`             | `(host: str, port: i32) -> List[str]` |
| 588 | `gethostname`         | `() -> str` |

589-599 reserved for v0.3 (set_nodelay / set_keepalive / shutdown
half-close / UNIX-domain sockets / TLS wrapper).

## Why double-Arc on the slot tables (the load-bearing design call)

First sketch — copy-paste from M27 zip/tar — was
`Arc<Mutex<Vec<Option<TcpStream>>>>`.  This deadlocked the TCP echo
demo on the first run: main's `recv_exact(server_handle, 4)` and
worker's `send(client_handle, "ping")` both tried to take the table
mutex.  `recv_exact` was holding it across a blocking `read_exact()`,
so the worker's `send` waited forever for the table lock — and
`recv_exact` couldn't progress because the bytes were stuck in the
worker's send queue, which couldn't drain because the OS-level send
was waiting on the worker thread's table-mutex acquisition.  Classic
"don't hold a mutex across blocking I/O" foot-gun, which the M27
zip/tar agent never noticed because it never had two threads working
sibling slots simultaneously.

**Fix**: wrap each slot's payload in `Arc<T>` and have the handlers
grab a refcount (cheap, atomic) before releasing the table mutex.
`TcpStream` implements `Read` / `Write` for `&TcpStream`, so the
shared `Arc<TcpStream>` is enough — no inner `Mutex<TcpStream>`
needed for the read/write side.  `UdpSocket` is the same shape
(`send_to` / `recv_from` are `&self`).

A `try_clone()` would have worked too (it's the standard "dup the
fd" pattern), but on Windows `WSADuplicateSocket` is a real syscall
(not as cheap as `dup` on Unix), and it creates a *separate* SOCKET
handle that doesn't share all socket options with the original.
Specifically, `set_read_timeout` on one duplicate doesn't reliably
propagate to the other on Windows — which is exactly the
"set the timeout then read" pattern the echo demo exercises.  The
Arc-refcount approach has neither problem: every handler operates on
the *same* underlying socket, so timeouts set in one call are visible
in the next.

## What I scoped down

* No `set_nonblocking` / `set_nodelay` / `set_keepalive` — the
  ~99% loopback / single-request-response use case doesn't need
  them, and they're all setsockopt one-liners that v0.3 can land
  without disturbing the existing API.
* No `shutdown(Read)` / `shutdown(Write)` — only `Both` via `close`.
  Half-close is rarely needed in straight request-reply protocols
  and the API surface stays small.
* No UNIX-domain sockets — Windows didn't grow `AF_UNIX` support
  until late Win10, and the `std::os::unix::net` interface isn't
  cross-platform.  v0.3 work, gated on either platform-conditional
  compilation or a portability shim.
* No TLS / SSL.  Callers should layer a TLS module on top — v0.3+
  candidate.
* No multicast / broadcast — the v0.2 UDP surface is unicast-only.
* `accept` blocks forever (no timeout).  Callers who want a
  bounded-wait accept should spin off a worker thread.

## Cross-platform notes (Windows vs Unix)

* **Linux / macOS**: `std::net` wraps the POSIX syscalls.  No
  surprises.
* **Windows**: `std::net` uses winsock.  Two API-visible gotchas:
  1. `close()` explicitly calls `flush()` then `shutdown(Both)`
     before dropping the socket.  Without the flush, winsock can
     drop in-flight bytes when the socket is closed with unsent
     data still in its send queue.  Documented in §9.40 ("Cross-
     platform notes" subsection).
  2. `recv_exact` on a peer-closed socket reports
     `ErrorKind::UnexpectedEof`, which we re-raise as `IOError`
     (same shape as the Unix `Read::read_exact` path).
* **IPv6**: transparently supported on all three platforms.
  `connect_tcp` and friends don't split into v4/v6 variants;
  `(host, port).to_socket_addrs()` walks both families and Rust's
  `TcpStream::connect` picks the first one that works.

## Methodology: commit-before-report

The brief flagged the compute-budget-exhaustion risk: agents finish
work, then burn the report-writing budget and the orchestrator
inherits an uncommitted worktree.  This time I committed once the
core handlers built clean (3 of 4 tests passing, before the deadlock
fix); the deadlock fix and report sit as follow-ups in the same
worktree, so the orchestrator gets a committed state even if the
follow-ups are interrupted.

## Methodology: variable-prefix discipline

Per the brief's Lesson 2, every local in the 19 handlers uses the
`p3b_a_<area>_<purpose>` prefix (e.g. `p3b_a_sock_ct_handle`,
`p3b_a_sock_us_table`).  Zero lines collide verbatim with the M27
`p3c_d_*` zip/tar handlers, the M23 `sqlite3` handlers, or the M27
`logging` handlers — git's myers/patience cherry-pick alignment has
no false-anchor candidates.

## Incidental bugs / oddities found

None requiring code changes.  The stdlib-module seam absorbed a 19-
function module without complaint.

## Final test totals

* Pre-existing baseline (M0-M27 worktree): 621 tests passing.
* New tests added: 4 (`socket_demo_compiles`,
  `socket_udp_demo_compiles`, `socket_demo_runs_via_spy_exe`,
  `socket_udp_demo_runs_via_spy_exe`).
* All on loopback (`127.0.0.1`) — no public-internet I/O.

## What's next (Phase 3b integration)

The orchestrator will cherry-pick this commit along with the sibling
Phase 3b worktrees.  Conflicts likely in:

* `compiler/src/resolver.rs` — `seed_stdlib_modules` end-of-fn;
  appended `.insert(...)`.  Mechanical merge.
* `shared/src/native.rs` — disjoint id ranges by design; should be
  clean.
* `vm/src/builtins.rs` — append-only handler arms; clean.
* `vm/src/interp.rs` — `SharedVm` body and both constructors.  M28
  P3b-A is the only agent in this round that touches `SharedVm` (as
  far as the brief enumerates), so no merge conflict expected.
* `STRICTPY_SPEC.md` — new §9.40; orchestrator may renumber.

## Hardest three things (in retrospect)

1. **The deadlock**.  ~25 min from "all 3 compile + UDP runs" to
   "TCP demo also runs".  The bug was textbook (mutex held across
   blocking I/O) but the symptoms — bytes appearing on the wire
   only after the worker closed — sent me chasing Nagle's algorithm
   for a few minutes before I noticed the Worker-blocks-Main
   pattern in the per-line-timing of the test output.  Refactoring
   the slot table to `Arc<T>` instead of `try_clone()` was a
   ~30-line change once the diagnosis was clear.
2. **The StrictPy `slice()` parameter type**.  The first demo
   draft used `peer_addr.slice(0i32, 4i32)` (matching the `i32`
   index conventions other String APIs use); `slice` actually takes
   `i64`, which the type checker correctly flagged at compile time.
   Trivial fix but a small reminder that StrictPy's String API isn't
   100% uniform with the rest of the str surface.
3. **Tuple field syntax**.  StrictPy uses `t.0` / `t.1` / `t.2`,
   not `t[0]`.  Python-shaped indexing compiled but with the wrong
   element type (the type checker correctly errored, just with a
   somewhat oblique message about expected vs actual primitive type).
