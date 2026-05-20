# M28 — Phase 3b stdlib (networking) — shared brief

Read this file FIRST, then your task-specific brief.

## Context (post-M27 state)

- M0-M27 complete. **621 tests passing, 0 failed, 1 ignored.**
- **33 stdlib modules** across Phase 1+2+3a+3c:
  - Phase 1 (M19-M21): sys, os, path, io, time, random, math, json, re
  - Phase 2 (M22): argparse, collections, csv, base64, hashlib, itertools, statistics, struct, urllib_parse
  - Phase 3a (M23): subprocess, pathlib, datetime, threading, queue, sqlite3
  - Phase 3c (M27): shutil, tempfile, glob, fnmatch, gzip, zlib, bz2, zipfile, tarfile, logging
- M26 added an extended benchmark suite (30 cells, 28W/2T/0L vs CPython 3.12.10).
- Only BUG-028 (lexer line continuation across infix `+`) remains deferred.
- **This is the FOURTH parallel-worktree stdlib round** (after M22, M23, M27).

## Phase 3b target: 3 new stdlib modules

This round closes the **networking** gap — the single biggest remaining
v0.2 domain. After M28 the language can do raw TCP/UDP, TLS, and HTTP
client requests; together with the rest of the stdlib that's "everything
a CLI tool / log scraper / API client needs" (with the explicit
exception of async I/O).

- **P3b-A**: `socket` — raw TCP/UDP, listen/accept, DNS lookup
- **P3b-B**: `ssl` — TLS-over-TCP (handshake + read/write)
- **P3b-C**: `http_client` — HTTP/1.1 client with get/post/request

All three are **independent at the StrictPy API level**. Each module
opens its own underlying sockets internally; the StrictPy bindings
don't compose with each other. That keeps each agent's worktree
isolated.

## CRITICAL: NativeFn ID range discipline

Phase 3c used 450-569. Phase 3b reserves disjoint ranges per agent:

- **P3b-A** (socket): IDs **570-599** (30 ids)
- **P3b-B** (ssl): IDs **600-619** (20 ids)
- **P3b-C** (http_client): IDs **620-649** (30 ids)

Do NOT use IDs outside your range.

## Read FIRST, in order

1. `docs/thesis/agent_reports/m27_p3c_d.md` — most recent agent
   working with opaque i64 handles + SharedVm slot tables (zipfile).
   Same shape as your socket/ssl/http handles.
2. `docs/thesis/agent_reports/m23_p3a_c.md` — threading.Lock /
   Semaphore — earlier example of SharedVm slot-table state.
3. `docs/thesis/milestones/m27_phase3c_stdlib.md` — most recent
   worktree-round milestone; the methodology section is the load-bearing
   read.
4. `STRICTPY_SPEC.md` §6.7 (imports) and §9.6-§9.39 (existing 33
   stdlib modules). Your modules will be §9.40+ (pick any free
   numbers; the orchestrator renumbers on integration).
5. `compiler/src/resolver.rs::seed_stdlib_modules` — append after
   logging.
6. `shared/src/native.rs` — append in your reserved ID range.
7. `vm/src/builtins.rs::dispatch` — append handlers.
8. `vm/src/interp.rs` (SharedVm) — add your slot tables (M23 / M27 P3c-D
   pattern).
9. Your task-specific brief.

## CRITICAL methodology notes (lessons from M22 / M23 / M24 / M27)

### Lesson 1 (M27 escalation): "FIRST COMMIT BEFORE 60% OF YOUR BUDGET"

The "commit early" warning has now failed in 7+ agents across M23 /
M24 / M27. Plain "commit early" wording isn't strong enough. The new
rule:

**Your FIRST `git commit` must land before you have used 60% of your
estimated time budget.** If you're approaching that mark and tests
aren't passing yet, COMMIT THE WORK-IN-PROGRESS ANYWAY. You can
amend the commit later. The orchestrator strongly prefers a
half-finished committed state over a complete uncommitted state.

Suggested checkpoint discipline:
- ~20% of budget: scaffolding done — first NativeFn dispatching → COMMIT
- ~40% of budget: all NativeFns implemented → COMMIT (amend)
- ~60% of budget: tests passing → COMMIT (amend)
- ~80% of budget: report drafted → COMMIT (amend)
- Final: report polished → COMMIT (amend)

If at any checkpoint you're at risk of running out, **commit then**
and stop. A committed half-implementation is recoverable; an
uncommitted full implementation often isn't.

### Lesson 2 (M27): closing-brace pattern at match-arm boundaries

When the orchestrator merges your worktree onto main alongside the
other agents, the standard append-at-end conflict-resolution path
produces a missing `}` between your final match arm and the next
agent's first match arm. This is fixed mechanically by the
orchestrator with one Edit per integration.

Helpful action on your side: make sure your LAST match arm has the
canonical closing `}` shape:
```rust
NativeFn::YourLastFn => {
    // ... handler body ...
    Ok(result)
}
```

Don't add trailing comments or whitespace inside the final arm's
closing brace — they confuse git's three-way merge alignment heuristic.

### Lesson 3: spec section collisions are expected

M22 had all 4 agents pick §9.15+. M27 had P3c-A and P3c-D both pick
§9.30/§9.31. M28 will have similar overlap. **Pick any free §9.X**;
the orchestrator renumbers all M28 sections (§9.40-§9.42 expected)
on integration.

## Network-bound testing discipline

This is the first stdlib round where modules do real I/O over the
network. Tests MUST NOT depend on the public internet:

1. **No external HTTP requests in tests.** Don't hit example.com,
   httpbin.org, or any public URL. Tests fail when CI is offline.

2. **Use loopback servers for self-tests.** Spawn a `std::net::TcpListener`
   in your integration test setup, bind it to `127.0.0.1:0` (get an
   OS-assigned port), then point your StrictPy code at the resulting
   port. Tear down at end of test.

3. **DNS lookups in tests**: use `localhost` only. `socket.gethostbyname("localhost")`
   should return `127.0.0.1`.

4. **TLS tests** need a self-signed cert. Either:
   - Ship a tiny test cert in `compiler/tests/fixtures/`
   - Generate one in test setup using `rcgen` (a small Rust crate),
     OR
   - Skip the cert-validation tests and only test the handshake against
     a manually-provided cert; document the gap.

5. **Mock HTTP server pattern**: for http_client tests, spawn a tiny
   Rust HTTP server in the test (`std::net::TcpListener` + hand-parse
   one request, send canned response). Look at how M23 P3a-A tested
   subprocess for the "spawn helper + test against it" pattern.

## Patterns established by Phase 1/2/3a/3c (don't reinvent)

- **Opaque handles (i64)**: see M23 sqlite3, M27 zipfile/tarfile.
  Each module has a SharedVm slot table mapping i64 → resource.
- **Tuple returns**: `Interpreter::alloc_tuple_obj(elements)`. Used
  for things like `socket.accept() -> Tuple[i64, str]` (handle + peer addr).
- **str-as-byte-buffer**: M22 P2D struct, M27 P3c-C gzip. Each str
  codepoint 0..=255 is one byte.
- **Raising exceptions**: `Err(VmError::UncaughtException { type_name: "IOError".into(), message: "...".into() })`.
- **Timeouts**: use `Duration` with sensible defaults (30s for TCP
  connect, 30s for read).

## v0.2 limits (don't waste time)

- **No async I/O.** All networking is synchronous and blocking. Users
  who want concurrency use OS threads (M6 + M23 threading).
- **No connection pooling.** Each request opens a fresh socket.
- **No HTTP/2** — HTTP/1.1 only. `ureq` makes this trivial.
- **No WebSockets** — protocol-specific, ship as separate v0.3 module.
- **No async DNS** — `getaddrinfo` is blocking. OK for v0.2.
- **No `match` as attr name** (it's a hard keyword since M16).
- **No stdlib classes** — opaque i64 handles + slot tables.
- **No closures across NativeFn boundary** — your handlers can't take
  user callbacks.

## File-ownership boundaries

You may modify (orchestrator merges):
- `compiler/src/resolver.rs` (append your module)
- `shared/src/native.rs` (your reserved ID range)
- `vm/src/builtins.rs` (append handlers)
- `vm/src/interp.rs` (new SharedVm slot tables)
- `vm/Cargo.toml` (add your crates only)
- `STRICTPY_SPEC.md` (append §9.X — orchestrator renumbers)

NEW files (no conflict):
- `examples/<your-module>_demo.spy` (one per module)
- `compiler/tests/<your-module>_demo_runs.rs` OR `vm/tests/m28_<your-module>.rs`
- `compiler/tests/fixtures/<file>.pem` if you need a cert
- `docs/thesis/agent_reports/m28_p3b_<letter>.md`

Do NOT touch:
- Existing examples or test files from M0-M27.
- Other agents' module registrations.
- BUGS_KNOWN.md / bugs/catalog.md / timeline.md / stats/* (orchestrator
  integrates).
- Any Phase 1/2/3a/3c stdlib module code.

## Suggested Rust crates (your choice — these are starting points)

- **P3b-A** (socket): just `std::net` (TcpStream, TcpListener, UdpSocket).
  No new crate needed. For `getaddrinfo`, use `std::net::ToSocketAddrs`.
- **P3b-B** (ssl): `rustls = "0.23"` + `rustls-pki-types = "1"` +
  `webpki-roots = "0.26"` (system trust store). Or `rustls-platform-verifier`
  for OS-native cert verification.
- **P3b-C** (http_client): `ureq = "2"` — synchronous HTTP/1.1 client,
  bundles rustls + webpki-roots, no async. ~2k LOC dependency.
  Alternatively hand-roll on `rustls` directly (~500 LOC HTTP/1.1
  parser); ureq is the obvious choice unless you have a specific
  reason.

## STOP CRITERIA

- If you need more NativeFn IDs than your reserved range, scope down.
  Do NOT reach into another agent's range.
- If you find a bug in the M0-M27 surface, save a minimal repro at
  `examples/_probe_<thing>.spy` and report. Don't try to fix it
  yourself unless it's in your module's territory.
- **CRITICAL**: if your first commit hasn't landed by 60% of your time
  budget, COMMIT WHATEVER IS WORKING NOW and stop. The orchestrator
  prefers committed half-progress over uncommitted full-progress.

## Acceptance criteria

1. `cargo build --workspace --release` succeeds in your worktree.
2. `cargo test --workspace --release` passes — 621 baseline preserved,
   plus your new tests.
3. At least one example program per module + integration tests.
4. Spec amendments for each module (any §9.X numbers — orchestrator
   renumbers).
5. **Your FIRST commit lands before 60% of your time budget.**
   See Lesson 1 above.
6. Report at `docs/thesis/agent_reports/m28_p3b_<letter>.md`
   (~600-1000 words).
7. Network tests use loopback only (127.0.0.1 / localhost). No external
   URLs.

## Reporting

Mirror the M23 P3a-x / M27 P3c-x report style. The thesis cares about:
- Which Phase 1/2/3a/3c modules your new modules built on.
- Which design choices you made (and what you scoped down).
- Any incidental bugs found (M27 found one bzip2 hang in P3c-C; M22
  was zero across 4 agents; M24 found BUG-039).
- Cross-platform notes (Windows vs Linux differ for socket options,
  IPv6 details, etc.).
- Final test totals.

Begin by reading the M27 P3c-D report (most recent opaque-handle agent)
+ M27 milestone note + this brief. Report when done.
