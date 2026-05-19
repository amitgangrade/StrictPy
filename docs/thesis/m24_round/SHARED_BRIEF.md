# M24 — Phase 3a stress test (shared brief)

The **first stress round on the Phase 3a stdlib surface** (subprocess,
pathlib, datetime, threading.Lock + Semaphore, queue.PriorityQueue,
sqlite3). Read this file FIRST, then your task-specific brief.

## Context (post-M23 state)

- M0-M23 complete. **553 tests passing**, 0 failed, 1 ignored.
- **24 stdlib modules** total. Phase 3a (M23) added: subprocess,
  pathlib, datetime, threading, queue, sqlite3.
- Only BUG-028 (lexer line continuation across `+`) deferred. One
  incidental fix in M23 P3a-C (resolver-shadow when stdlib module name
  matches existing prelude import).
- Each Phase 3a module has its own per-module test file but **none
  have been combined in a single program**. That's the gap this round
  fills.

## The stress-test discipline (carry-over from M10/M12/M18)

You are NOT shipping a polished demo. **You are stress-testing 7
modules in combination, looking for incidental bugs.** Past rounds:

| Round | Programs | Bugs found |
|---|---|---:|
| M10 (round 1) | 6 real programs | 17 |
| M11 (round 2) | 5 | 6 |
| M12 (round 3) | 3 | 2 |
| M18 (round 4) | 4 (M13-M17 surface) | 1 |
| **M24 (round 5)** | 4 (M23 P3a surface) | ? |

The trend is downward — but every round has found something. Phase 3a
is the riskiest stdlib round so far (OS FFI in subprocess + threading
+ sqlite). My pre-M23 prediction was "1-3 incidental bugs"; the M23
agents found 1 in isolation. Combination-stress finds different bugs
than per-module unit tests.

## CRITICAL: file-ownership boundaries

You may ONLY create/edit:
- `examples/<your-program-name>.spy` (and `examples/_probe_<name>.spy`
  for minimal repros)
- `compiler/tests/<your-program-name>_runs.rs`
- `docs/thesis/agent_reports/m24_<your-letter>.md`

Do NOT modify:
- ANY file in `compiler/src/` or `vm/src/` or `shared/src/`. If you
  find a bug, write a minimal repro at `examples/_probe_<thing>.spy`
  and document. The orchestrator decides whether to fix inline.
- Existing examples or test files.
- `STRICTPY_SPEC.md`, `BUGS_KNOWN.md`, `docs/thesis/bugs/catalog.md`,
  `docs/thesis/timeline.md`, `docs/thesis/stats/*`.

Worktree isolation means you won't see your siblings' commits. The
orchestrator integrates everyone at the end.

## Read FIRST, in order

1. This brief.
2. `docs/thesis/agent_reports/m18_*.md` — last stress round; same
   discipline. **Read m18_torture.md** for the rigour bar (250
   sequential runs gave the M11 BUG-026/027 closure).
3. `docs/thesis/agent_reports/m23_p3a_*.md` — each Phase 3a module's
   builder describes its API + design choices + known limits. Read
   the relevant ones for your task.
4. `STRICTPY_SPEC.md` §9.24-§9.29 — the formal surface.
5. `BUGS_KNOWN.md` — currently open bugs.
6. `docs/thesis/milestones/m23_phase3a_stdlib.md` — the Phase 3a
   milestone-cluster note.
7. Your task-specific brief.

## The known v0.2 limits (don't waste time)

- **`match` is a hard keyword** (M16). Pick a different attr name.
- **No stdlib classes** in v0.2 — sealed types like `ArgParser`,
  `Connection`, `DateTime` are flat i64 handles or untyped strings.
- **No bytes type** — `struct.pack_*` uses the "each codepoint 0..255"
  trick. `sqlite3` stringifies all result cells.
- **`with open(...) as f:`** does NOT route through try/except.
  Use `try: with ... except IOError:` explicitly.
- **Nullable narrowing is per-expression**, not per-binding.
- **No closures across NativeFn boundary** — handlers can't take
  user functions as args.
- **Generic stdlib functions** don't compose with M17 worklist
  (random.choice ships as `_i64`/`_f64`/`_str` monomorphic variants).
- **subprocess.kill** uses TerminateProcess on Windows / SIGKILL on
  Unix. No SIGTERM in v0.2.
- **threading.Lock** is non-reentrant (Python `Lock`, not `RLock`).
  Re-acquiring deadlocks.
- **datetime is integer-second precision.** No fractional seconds.

## STOP criteria

- If you find a deterministic minimal repro of a bug, **save it as
  `examples/_probe_<descriptive_name>.spy`** and continue building
  your program (work around the bug or note it as blocking).
- If you find a non-deterministic bug (test passes sometimes, fails
  others), capture the smallest snippet that exhibits it AND a
  repro-rate estimate (e.g. "fails ~3/10 runs").
- If your program needs a runtime primitive that doesn't exist
  (e.g. signals, async I/O), document and move on.
- If you spend more than 90 min on a single bisect, stop and report.
  Half-found bugs are still valuable.

## Acceptance criteria

1. `cargo build --workspace --release` succeeds in your worktree.
2. `cargo test --workspace --release` passes — 553 baseline preserved,
   plus your new tests.
3. Your program runs end-to-end (with workarounds for any bugs you
   found).
4. At least 1 integration test (`compiler/tests/<name>_runs.rs`)
   verifying canonical output.
5. Report at `docs/thesis/agent_reports/m24_<your-letter>.md`
   (~500-1000 words):
   - What program you built (one paragraph summary).
   - **Bugs found, with file:line speculation if you can pinpoint.**
   - Workarounds reached for, and what they imply about the language.
   - Specific Phase 3a probes you ran (per the "probe list" in your
     task brief).
   - Final test totals.

## Reporting honesty

You're not graded on "did you ship a clean demo". You're graded on
"did you produce a credible, detailed bug report (or honest 'nothing
broke') after exercising the new surface in combination."

A laconic "I built X, here's the code" is significantly LESS useful
than "I tried A, hit B, dug into C, found root cause D, worked
around with E." The M10 C2 JSON-parser report (~8 bugs in one task)
is the gold standard. The M18 round's verdict was "the M11 class fix
held under stress" — that's also a legitimate finding, just less
exciting.

Begin by reading this brief, the m18_* reports, and the m23_p3a_*
reports relevant to your task. Report when done.
