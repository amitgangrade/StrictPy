# M10-C3 — KV store with WAL + Brainfuck interpreter

**Brief**: write a concurrent KV store with append-only WAL persistence,
and a classic Brainfuck interpreter. Stress concurrency + character
handling.

**Wall-clock**: ~49 minutes
**Tool uses**: 92
**Files added**: `examples/kvstore.spy` (194 lines), `examples/brainfuck.spy` (176 lines),
`compiler/tests/kvstore_runs.rs` (84 lines), `compiler/tests/brainfuck_runs.rs` (38 lines).

## Result

**Both programs run end-to-end.** Brainfuck prints "Hello World!" from the
classic 106-byte BF program. KV store processes 213 commands (200 SETs + 13
GETs/DELs/etc.) through a worker thread, persists to WAL, and recovers on
second run.

### Brainfuck stdout
```
Hello World!
```

### KV store sample
```
SET	name	alice → OK
SET	age	30 → OK
GET	name → alice
GET	age → 30
GET	missing → <none>
SET	name	bob → OK
GET	name → bob
DEL	age → OK
...
GET	counter → iter-199
SHUTDOWN → BYE
```

## 3 new bugs surfaced

1. **BUG-018: `char(i32)` rejected with E2011** despite `NativeFn::CharFromI32 = 23`
   existing and being fully wired in the VM. Typechecker's numeric-ctor
   allow-list omits `char`. Trigger: `char(some_i32_expr)` in source.

2. **BUG-019: `str(c: char)` returns codepoint as decimal text.**
   `str('h')` prints `"104"`, not `"h"`. Root cause: IR lowerer routes
   every `str(x)` to `NativeFn::StrFromAny` which can't distinguish
   char-typed u64 from i64-typed. `StrFromChar = 10` exists but is
   unreachable.

3. **BUG-020: `dict.has(k)` rejected with E2004.** `NativeFn::DictHas`
   exists and is implemented; typechecker's `synth_method_call` Dict arm
   doesn't list `has`. Workaround: `d.get(k) is none`.

## Threading + GC observations (the load-bearing positive results)

**Long-running JIT'd worker thread worked.** The `worker_loop` on its own
OS thread ran the entire 213-command workload (~200 SET iterations
allocating fresh strings + a fresh response per command + dict-entry
replacement each iteration). Completed in well under a second. **M9's
`in_jit` GC pause did NOT cause any visible failure here.**

**Closure capture across threads:** `Thread(fn() -> None: worker_loop(cmds, resp, wal_path))`
capturing 3 values worked exactly as `producer.spy` predicted.

**File append + concurrent access**: main thread sends strings via
channel; worker writes to WAL file. No corruption observed in the WAL
file. Inter-thread serialization via the channel is sufficient.

Caveat: agent could not directly observe heap growth (test only asserts
exit code + final state). At this scale (sub-second execution), the
`in_jit` pause is benign. The compromise might bite at much larger
scales — see `design_decisions/conservative_gc_with_in_jit_pause.md`.

## Channel race observations

Agent **avoided `try_recv` entirely** because of the documented "empty
vs closed" indistinguishability (M5 limitation). Used **`recv()` + a
`SHUTDOWN` sentinel command** instead. Worked perfectly — no race, no
dropped commands, deterministic shutdown.

**Recommendation surfaced by this work**: the spec's §16.3 examples
should adopt the SHUTDOWN-sentinel pattern until `try_recv` is fixed to
return distinguishable sentinels for empty vs closed.

## `char` handling

`c == '+'` worked correctly throughout the brainfuck dispatch loop.
Char-literal equality, including `'\t'` and `'\n'`, all behaved as
expected. **The surprises were all in conversion (bugs 1 and 2), never
in comparison.**

## Other language-surface gaps

- `[wanted char(i32) but had to build a 256-char ASCII lookup string]`
  — bug 1.
- `[wanted str(char) but had to use table.slice(i, i+1)]` — bug 2.
- `[wanted dict.has(k) but had to use dict.get(k) is none]` — bug 3.
- `[wanted list.pop() but had to rebuild a list without its last
  element]` — no pop/remove in native list API. Worked around in
  `build_jumps`. **Fixed in M10 follow-up as BUG-021.**
- `[wanted str.split('\t') but had to write split_tabs by hand]` — same
  gap csv_aggregate.spy hit.
- `[wanted print without newline but Print native is unreachable from
  source]` — turned out to be a false alarm; later verified the
  `"print"` entry was already in `from_name`.
- `[wanted try/except for missing-file IO but open() has no fallible
  variant]` — deferred to exception handling work.
