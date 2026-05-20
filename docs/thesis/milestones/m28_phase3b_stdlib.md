# M28 — Phase 3b stdlib (networking)

**Date**: 2026-05-20
**Wall-clock**: ~1.5-2h parallel agent compute (3 worktree agents) +
~45min orchestrator integration (cherry-pick, recovery from one
catastrophic mis-applied patch, 2 manual brace fixes, ssl block
re-anchoring).
**Headline**: 3 stdlib modules shipped concurrently by 3 worktree-
isolated agents. The biggest single domain remaining at end of M27.
After M28 StrictPy can do TCP/UDP raw sockets, TLS-over-TCP, and
HTTP/1.1 client requests — "everything a CLI tool / log scraper /
API client needs" (with the known exception of async I/O).

## What shipped

| Agent | Modules | NativeFn IDs | Spec sections | Worktree commit |
|---|---|---|---|---|
| P3b-A | `socket` (TCP + UDP) | 570-588 (19 ids; 589-599 reserved) | §9.40 | `e104163` + `8b80d80` → main `d702dfa` |
| P3b-B | `ssl` (TLS-over-TCP) | 600-609 (10 ids; 610-619 reserved) | §9.41 | `9bad95a` + `a5d99c2` → main `f25e4b3` |
| P3b-C | `http_client` (HTTP/1.1) | 620-649 range | §9.42 | `a02e4ab` → main `461ee3e` |

**~40 NativeFns total**, well under the 80 reserved.

**3 new crate deps in vm/Cargo.toml**: `rustls` + `rustls-pki-types`
+ `webpki-roots` (P3b-B) and `ureq` (P3b-C). P3b-A added none — pure
`std::net`. Plus 3 dev-deps in compiler/Cargo.toml for the ssl
loopback test (`rcgen`, `rustls`, `rustls-pki-types`).

## Lesson 1 escalation actually worked

The methodology fix that previously failed in 7+ agents (M23, M24, M27
P3c-A/E) finally took. From the M28 brief:

> **Your FIRST `git commit` must land before you have used 60% of
> your estimated time budget.** If you're approaching that mark and
> tests aren't passing yet, COMMIT THE WORK-IN-PROGRESS ANYWAY.

Per-agent compliance:

- **P3b-A**: 2 commits (`e104163` initial + `8b80d80` deadlock fix).
  Agent self-reported "first commit landed at ~40% of budget, well
  inside the 60% threshold." Even better: agent caught a deadlock in
  their OWN code mid-task and shipped a focused fix-up commit.
- **P3b-B**: 2 commits (`9bad95a` green-build checkpoint + `a5d99c2`
  final). Agent explicitly reported "First commit landed at ~30% of
  compute budget on green build, well under the 60% threshold." Best
  Lesson 1 discipline of any M28 agent.
- **P3b-C**: 1 commit (`a02e4ab`). Agent's final report shows the
  same "tests still building, monitor will fire" tail as M27's
  failed agents — but the commit was already in place, so the work
  was preserved regardless. Suggests the agent committed mid-task
  then continued; the trailing test-rebuild was just a verification
  step.

**3 of 3 agents shipped committed work this round** (vs 3 of 5 in
M27, 0 of 4 in M24). The brief language change was the only
meaningful difference. The lesson generalises: explicit numerical
thresholds in agent briefs ("commit before 60% of budget") move the
needle where qualitative urgency ("commit early") doesn't.

## Two integration disasters worth recording

### Disaster 1: P3b-B diff against current-main destroyed P3b-A's work

After cherry-picking P3b-A onto main, I generated P3b-B's diff via
`git diff main..worktree-agent-aba4f8f0e47a762cd`. That diff was
computed against current-main — which already had P3b-A's content.
The diff therefore contained REVERSE-DELETIONS of P3b-A's
contributions (since P3b-B's worktree didn't have them). When I
applied + committed, the commit deleted 1806 lines of P3b-A's work
(`examples/socket_demo.spy`, `compiler/tests/socket_demo_runs.rs`,
the whole SHARED_BRIEF, agent report, etc.).

Caught by inspecting the commit's `--stat` output (deletion counts
> insertion counts is the smoke alarm). Recovery: `git reset --hard
HEAD~1`, regenerated the patch as `git diff c4fe0ce..worktree`
(against the PRE-M28 base), and re-applied. Clean this time.

**Pattern lesson**: when sequentially cherry-picking N parallel
worktrees onto main, ALWAYS diff against the common ancestor (the
pre-round base), not against current-main. Otherwise each
subsequent cherry-pick reverts the prior ones.

### Disaster 2: ssl block landed outside seed_stdlib_modules

The `git apply --3way` + keep-both auto-resolution placed P3b-B's
`ssl` StdlibModule block AFTER the closing `}` of
`seed_stdlib_modules`. Compile errors: "no method `seed_prelude`
found for `Resolver`" etc — because adding the ssl block inside the
impl block but outside the function broke the lexical scope of every
method after it.

Caught by the build error pointing at `seed_prelude` / `register_top_decls`
in the impl block. Recovery: extracted the ssl block from P3b-B's
worktree resolver.rs (lines 3189-3270), reset main's resolver.rs to
its pre-apply state, then Python-scripted the insertion before the
seed_stdlib_modules closing brace.

**Pattern lesson**: the keep-both resolution can mis-place blocks
across function-boundary lines that look like ordinary closing
braces. The build error always points at the next function whose
methods become unresolved — that's the diagnostic anchor.

## The familiar closing-brace fix (third round in a row)

`vm/src/builtins.rs` again had 2 missing `}` between adjacent match
arms — same M27 pattern:
1. Between P3b-A's `SocketGethostname` arm body close and P3b-B's
   `SslConnect` arm.
2. Between P3b-B's `SslGetVerifyCerts` arm body close and P3b-C's
   `HttpClientGet` arm.

Plus a NEW variant: the keep-both resolution dropped TWO closing
braces simultaneously where P3b-B's `ssl_no_verify::NoVerify` module
ended and P3b-C's helper functions began — one for the `impl
ServerCertVerifier for NoVerify {` block, one for the `mod
ssl_no_verify {` block. Fixed by inserting both at once.

This is now an established pattern: every M27+ integration adds N-1
missing `}` between adjacent agents' final and first
match-arms/blocks. The orchestrator should expect this and run
`cargo build` after each apply specifically to catch it.

## Test coverage

All three agents added loopback-only network tests:
- **P3b-A**: TCP echo + UDP echo, both on 127.0.0.1:0 (OS-assigned port)
- **P3b-B**: rcgen-generated self-signed cert, rustls-server loopback
  receiver, StrictPy `ssl.connect` client roundtrip
- **P3b-C**: hand-rolled HTTP/1.1 server in `std::net::TcpListener`
  thread, StrictPy `http_client.get/post` against it

No public-internet dependencies. Documented up-front in the SHARED_BRIEF.

## API examples

### socket (P3b-A)
```python
import socket
# TCP client
let h = socket.connect_tcp("127.0.0.1", 8080)
socket.send(h, "GET / HTTP/1.1\r\n\r\n")
let response = socket.recv(h, 4096)
socket.close(h)

# TCP server
let lis = socket.listen_tcp("0.0.0.0", 8080, 32)
let (conn, peer) = socket.accept(lis)
```

### ssl (P3b-B)
```python
import ssl
let h = ssl.connect("example.com", 443)
ssl.send(h, "GET / HTTP/1.1\r\nHost: example.com\r\n\r\n")
let response = ssl.recv(h, 65536)
ssl.close(h)
```

### http_client (P3b-C)
```python
import http_client
let (status, body) = http_client.get("https://example.com/")
let (status2, body2) = http_client.post("https://api.example.com/v1",
    "{\"key\": \"value\"}", "application/json")
```

## v0.2 limits documented in §9.40-§9.42

- No async I/O (everything blocks; users use M6 threads for concurrency)
- No connection pooling in http_client (fresh socket per call)
- No HTTP/2 — HTTP/1.1 only
- No WebSockets (v0.3)
- No server-side TLS in ssl (client only)
- ssl.set_verify_certs(false) is test-only — production paths can't
  accidentally invoke the "trust everything" verifier (lives in a
  separate module)

## What v0.2 stdlib still doesn't have

After M28 the language has **36 stdlib modules**:

| Phase | Milestones | Modules | NativeFn IDs |
|---|---|---:|---:|
| Phase 1 | M19-M21 | 9 | 130-249 |
| Phase 2 | M22 | 9 | 250-347 |
| Phase 3a | M23 | 7 | 350-449 |
| Phase 3c | M27 | 10 | 450-569 |
| **Phase 3b** | **M28** | **3** | **570-649** |

Remaining v0.2 gaps:

**Phase 3d** (utility & debugging):
- `traceback` — format_exc, format_tb (improves StrictPy's one-line
  exception output)
- `enum` — needs minor language support for value-bound class members
- `functools` — reduce, partial, lru_cache, cmp_to_key
- `uuid` — UUID v4/v7
- `secrets` — crypto-secure random

**v0.3 territory**:
- Async I/O (architectural — needs event loop)
- Generic classes (`class Box[T]:`)
- Stdlib classes (typed `JsonValue` tree, `re.Pattern`, etc.)
- Server-side TLS, HTTP/2, WebSockets
- Connection pooling, async DNS

## Tests + size

- **Tests**: 621 → ~640 (M28 added ~20 new tests across 3 agents).
- **Examples**: 79 → 85 (+6: socket_demo, socket_udp_demo,
  ssl_demo, http_client_demo, + 2 probes).
- **Stdlib modules**: 33 → 36.
- **vm/Cargo.toml deps**: 17 → 20 (+ rustls, rustls-pki-types,
  webpki-roots, ureq; minus none).
- **compiler/Cargo.toml dev-deps**: +3 (rcgen, rustls,
  rustls-pki-types for the ssl loopback test).

## Next-step menu (post-M28)

- **G**: Draft the thesis. Archive is fully built through M28.
- **Phase 3d**: utility/debugging stdlib (traceback, enum, functools,
  uuid, secrets). Small, parallel-friendly.
- **N**: Generic classes (`class Box[T]:`). Unblocks typed stdlib
  classes (Logger/Formatter/JsonValue tree).
- **Placeholder-lowering audit** on `compiler/src/ir.rs::emit_binop`.
- **Q**: BUG-028 lexer line continuation. The last open bug.
- **An orchestrator integration harness**: codify the lessons
  (always diff against pre-round base; auto-insert closing braces at
  agent-boundary detected via grep; smoke-test for net-LOC < 0).
