# M30 — Last two open bugs closed

**Date**: 2026-05-21
**Wall-clock**: ~1 hour parallel agent compute (2 focused agents) +
~15 min orchestrator integration (one catalog conflict reconciled).
**Headline**: BUG-028 (lexer line continuation across infix operators)
and BUG-040 (`socket.close_listener` doesn't unblock blocked `accept`)
both closed. **35 found / 35 fixed / 0 deferred** — the cleanest
"v0.2 frozen" state possible. The project's first time at zero open
bugs since M10.

## What shipped

| Agent | Bug | Files | NativeFns / API |
|---|---|---|---|
| M30 BUG-028 | Lexer line continuation across `+`/`and`/`or`/`==`/etc. | `compiler/src/lexer.rs` (~95 LOC) | n/a — frontend-only |
| M30 BUG-040 | `socket.close_listener` doesn't unblock `accept` | `vm/src/builtins.rs` (~120 LOC), platform-specific `shutdown(fd)` + self-connect | extended `close_listener` semantics; no new NativeFns |

Plus shared infra updates per agent:
- `STRICTPY_SPEC.md` §3.2 (lexer rule) + §9.40 (close_listener wake behaviour)
- `docs/thesis/bugs/catalog.md` (both bugs marked fixed; summary table reconciled to 35/35/0)
- 11 + 1 new regression tests
- 2 agent reports

## BUG-028: lexer line continuation

The original deferred bug, sitting since M10 (~20 milestones).
Workaround was parentheses, which kept the friction low enough that
deferral never blocked anything. Closed now for completeness — zero
behaviour change for any existing program; just removes a friction
point for any future user code that uses the natural Python idiom of
breaking a long expression across lines on a binary operator.

Implementation: track the last significant emitted token; when about
to emit a `NEWLINE` token, check whether the last significant token
was a binary operator that requires a right-hand operand. If yes,
suppress the newline and continue lexing on the next physical line
(also suppressing the indent of that continuation line). The trigger
set: `+ - * / // % ** == != < > <= >= and or & | ^ << >> = += -= *=
/= //= %= **= &= |= ^= <<= >>= in is as ??`. Deliberately EXCLUDED:
`:` (would break every block header), `,`, `.`, `->`, `@`, unary
`not` / `~`.

This is a structural cousin of the existing paren-depth-suppression
path the lexer already had — and lives right next to it in `next_token_inner`.

## BUG-040: `socket.close_listener` now wakes blocked `accept`

Found in M29.5 by the web framework's graceful-shutdown path. The
M28 P3b-A `SocketAccept` handler `Arc::clone`s the listener and
drops the slot-table mutex before the blocking syscall — so closing
the slot from another thread doesn't drop the underlying FD, and
the blocked accept stays blocked. The M29.5 framework worked around
this with a self-connect from the shutdown timer (~15 LOC user code).
M30 fixes it stdlib-side.

Implementation: extended `close_listener` to call a new
`shutdown_listener_fd` helper before removing the slot. The helper
applies **two complementary mechanisms unconditionally** (belt-and-
braces, both harmless when redundant):

1. **`shutdown(fd, SHUT_RDWR)`** on the listener's socket — the
   POSIX recipe. Uses `libc::shutdown` on Unix, inline
   `extern "system"` declaration on Windows (no new crate deps).
2. **Self-connect to `listener.local_addr()`** with 50ms timeout —
   the canonical Windows wake mechanism (per Microsoft KB-179942).
   Wildcard binds (`0.0.0.0` / `[::]`) are rewritten to loopback via
   `IpAddr::is_unspecified()`.

The cross-platform finding is worth recording in the methodology
archive: the agent empirically found that Windows winsock does NOT
wake `accept()` from `shutdown(fd)` alone (test hit the 5s watchdog).
The self-connect fallback is essential on Windows. POSIX wakes from
shutdown alone; the self-connect throwaway just gets immediately
accepted and dropped.

The M29.5 user-code self-connect workaround in
`examples/webserver/todo_app.spy::drain_in_flight` is now technically
unnecessary. Per the agent brief, it was kept in place for
archaeological value — future agents studying the M29.5 pattern will
see it, and the agent report explicitly notes the workaround is no
longer required post-M30.

## Reconciliation methodology

Both agents independently edited `docs/thesis/bugs/catalog.md`'s
summary table — both decrementing the deferred count from their
respective starting points. The orchestrator's cherry-pick of the
second commit (BUG-028 after BUG-040 had already landed) hit a
conflict on the `Stdlib semantics` row + Total. Resolution:
hand-edit to the post-both-fixes state (`Stdlib semantics 1/1/0`,
Total `35/35/0`). The conflict was syntactically trivial because
both agents followed the brief's "update the summary table"
instruction literally — but they couldn't see each other's
contemporaneous changes.

Pattern lesson for future multi-agent rounds touching the bug
catalog: include explicit "your edit will conflict with N other
agents touching this file — leave the Total row blank or with a
sentinel value; the orchestrator reconciles" guidance in the brief.

## Lesson 1 discipline now at 10 consecutive clean agents

Both M30 agents committed cleanly on their worktree branches
without orchestrator commit-on-behalf — extending the streak from
M28-M29.5's 8 to **10 consecutive clean agents**. The brief
language (the "FIRST commit before 60% of budget" numerical
threshold from M28) is now battle-tested across:

| Round | Clean commits |
|---|:---:|
| M28 (3 networking agents) | 3 / 3 |
| M28.5 (server-side TLS) | 1 / 1 |
| M29 (web framework) | 1 / 1 |
| M29.5 (framework round-out) | 1 / 1 |
| **M30 BUG-028 + BUG-040** | **2 / 2** |
| **Cumulative** | **10 / 10** |

vs. the pre-M28 history:
- M22 (4 stdlib agents) — 4 / 4 (lucky)
- M23 (4 stdlib agents) — 3 / 4 (P3a-D failed)
- M24 (4 stress agents) — 0 / 4 (all failed)
- M27 (5 stdlib agents) — 3 / 5 (2 failed)

The intervention is reproducible and the result holds. The lesson
generalises beyond StrictPy: **numerical thresholds in agent briefs
move the needle where qualitative urgency doesn't.**

## Tests + size

- **Tests**: 639 → 651 (+12: 11 from BUG-028's line-continuation
  suite + 1 from BUG-040's wake-up regression test).
- **Examples**: unchanged at 96+ (no new programs in M30).
- **Stdlib modules**: unchanged at 36.
- **Bug catalogue**: 35 found / 35 fixed / **0 deferred**.

## What this means for "v0.2 frozen"

The minimum claim for a clean v0.2 release is now achievable: every
bug found has been fixed; the remaining "what v0.2 can't do" list is
v0.3 architectural work (generic classes, async event loop, precise
GC stack maps, stdlib classes, HTTP/2, WebSockets, server-side TLS
mutual auth, NumPy integration). None of those are bugs — they're
unimplemented features documented as such.

The natural next step is **draft v0.2 release** as a fixed point:
update STRICTPY_SPEC.md's version banner, tag the commit, write a
v0.2 release-notes summary, and call the language "done for v0.2"
before starting v0.3 work.

## Next-step menu (post-M30)

- **G (release)**: Draft v0.2 release. Tag commit, refresh
  STRICTPY_SPEC.md's version string, write release notes summarising
  M0-M30 as the v0.2 freeze point.
- **N**: Generic classes (`class Box[T]:`). Unblocks typed stdlib
  classes (the single most-impactful v0.3 ergonomic win — would
  shrink the M29 framework ~30%).
- **Async event loop**: closes the ~2× perf gap to Flask+gunicorn
  measured in M29. Major architectural decision.
- **Precise GC stack maps**: closes the in_jit pause limitation;
  removes the M26 `btree`-at-large-n narrowing result.
- **Phase 3d stdlib**: traceback / enum / functools / uuid /
  secrets. Smaller modules; the M27 worktree pattern handles them
  cleanly.
- **Placeholder-lowering audit** on `compiler/src/ir.rs::emit_binop`.
  30-60 min mechanical audit to find any 5th instance.
