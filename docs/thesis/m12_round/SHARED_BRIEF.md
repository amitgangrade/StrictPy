# M12 stress-test round — shared briefing for all C-agents

Read this file FIRST, then your task-specific brief, then begin work.

## Project context (one paragraph)

StrictPy is a statically typed Python dialect with a Rust-implemented
compiler + bytecode VM + Cranelift JIT. It targets cache-friendly object
layouts and type-specialised opcodes. Post-M11 we have 20 example programs,
201 tests, 29 distinct bugs found, 27 fixed, 2 deferred. The repo is at
github.com/amitgangrade/StrictPy. The point of the M12 round is **finding
bugs, not shipping a demo**. Each stress program is a probe into the
language surface; the deliverable is your honest report on what broke and
how you worked around it.

## Files to read FIRST (in order)

1. `STRICTPY_SPEC.md` — language surface (v0.1, frozen)
2. `BUGS_KNOWN.md` — currently open bugs. Read every section carefully.
3. `docs/thesis/bugs/catalog.md` — the 29 fixed/deferred bugs catalog.
4. `docs/thesis/agent_reports/m11_c6_lisp_interpreter.md` — the most
   recent stress-test report; same shape as what you produce.
5. `docs/thesis/agent_reports/m10_c2_json_parser_markov.md` — the highest-
   leverage stress test ever run (8 bugs in one task). Use it as the gold
   standard for "report verbatim and find bugs."
6. `shared/src/native.rs` (lines 1-130 only) — the NativeFn enum is the
   ground-truth list of stdlib operations the language supports.
7. One existing example similar in shape to yours — list below.
8. `docs/thesis/agent_briefing_patterns.md` — meta-patterns; skim if curious.

## Post-M11 language state (THIS IS NEW — calibrate your expectations)

The M11 round just landed a major class-system overhaul. Things that USED
to be broken and ARE NOW FIXED:

- **`sealed` classes dispatch virtually correctly** (BUG-015). Use
  `sealed class Foo:` if you want a closed hierarchy. Subclass methods
  ARE reached.
- **Subclass field offsets are correct** (BUG-016). You can declare
  `Sub(Base) { extra: i32 }` and it will NOT alias Base's last field.
  No more "field-less base class" workaround.
- **Vtables support arbitrarily many slots** (BUG-017/033). Base class
  can have 6, 10, 20 virtual methods. Subclass can override any subset.
  No more "≤4 virtual methods on base" workaround.
- **`is not none` is correct** (BUG-008). Use it freely.
- **`str(char)` returns a one-char string** (BUG-019), not a decimal codepoint.
- **`char(i32)` typechecks** (BUG-018).
- **`dict.has(k)` typechecks** (BUG-020).
- **`for x in xs:` works** for List[T] receivers (BUG-024).
- **`list.pop()`, `list.sort()`, `sorted()`, `str.split(sep)` all exist** (BUG-021/022/023).
- **`i32(x: i64)` truncates correctly; `i64(f: f64)` truncates toward zero** (BUG-031).
- **BUG-026/027** (non-deterministic heap corruption, position-sensitive crash)
  are **provisionally fixed** — empirically calculator + json_parse run 5/5
  cleanly after M11, where pre-M11 they were 0/3. The M12 torture-test
  agent is upgrading them to "confirmed fixed". For your purposes, assume
  they're fixed; **but if you hit STATUS_HEAP_CORRUPTION (exit 0xC0000374
  on Windows) or non-deterministic output, REPORT IT — that means they
  came back, which is huge news.**

## Things that are STILL BROKEN (don't waste time on these)

- **No `try`/`except` codegen** (BUG-025). Parser accepts it; IR drops it.
  Don't use `try:` — programs will compile but the body runs unguarded.
- **No fallible `open()`** (BUG-025). `open(path, "r")` traps if the file
  is missing. Don't write programs that depend on graceful file-not-found.
- **No implicit line continuation across trailing `+`** (BUG-028). Use
  accumulator pattern: `s = s + "..."`.
- **No `isinstance(x, T)`** and **`match`/`case` constructor patterns
  don't lower** — sealed hierarchies can't externally discriminate
  variants without a manual `kind: i32` field. (BUT: virtual methods now
  work correctly, so prefer that.)
- **No tuples / multiple return values.** Use a tiny final class or
  thread state via 1-element mutable lists.
- **No user-code generics**. `fn id[T](x: T) -> T` parses but isn't
  monomorphised at user call sites. Rewrite per type if needed.
- **Nullable narrowing is per-expression, not per-binding.** After
  `if x is not none:`, a fresh `let y: T = x` may still fail. Common
  workaround: write a free function `unwrap_xxx(x: T?) -> T:` that asserts
  internally.

## File-ownership boundaries (PARALLEL AGENTS — DO NOT TOUCH OTHERS' FILES)

You may only create/edit the files listed in **your** task brief.
- Do NOT modify any file under `compiler/src/` or `vm/src/`.
- Do NOT touch any other `examples/*.spy` than your own.
- Do NOT modify any test file other than the one your brief assigns you.
- Do NOT touch `BUGS_KNOWN.md`, the bug catalog, or any other thesis
  archive file. (The orchestrator integrates your report into those.)

If you discover a fix is mechanically tiny and obviously correct (e.g.
add a missing NativeFn entry, fix a typecheck synth table), write it up
in your report — don't apply it. The integration pass decides whether to
merge it.

## What "success" means

Success is **a complete and honest report**, even if the program is
half-built and crashes. Specifically:

- A program that runs end-to-end is great, but a program that exposes a
  new bug is more valuable than one that ships clean by working around
  the surface.
- Treat every workaround you reach for as evidence of a language gap.
  Note it explicitly in the report.
- If you find a deterministic minimal repro for any anomaly, save it as
  `examples/_probe_<thing>.spy` (the prefix `_probe_` is reserved for
  these — the integration pass deletes them after extracting the relevant
  one into a regression test).

## STOP CRITERIA

- If you spend more than ~half your time budget on a single bug bisect
  without a minimal repro, stop and report what you've found.
- If the language is missing something fundamental (e.g. you need
  recursion through a Dict[str, ClassRef] and it segfaults), document
  the gap, work around it with a different data shape, and continue.
- If `cargo test --workspace --release` was green when you started and
  is now red on a file you didn't touch, that's a regression on someone
  else's territory — stop and flag it.

## Acceptance criteria (machine-checkable)

1. `cargo build --workspace --release` succeeds.
2. `cargo test --workspace --release` runs your new test(s); they
   pass OR you have documented exactly why they can't (e.g. heap
   corruption regression, in which case raise the alarm).
3. Your program is in `examples/<name>.spy` and roughly 100-400 lines.
4. Your test file is `compiler/tests/<name>_runs.rs` (mirror
   `compiler/tests/calculator_runs.rs` shape if your program uses any
   class hierarchy; otherwise `compiler/tests/sudoku_runs.rs` shape).
5. Your verbatim report is at
   `docs/thesis/agent_reports/m12_<name>.md`.

## Report shape (~300-700 words)

Mirror the M11 reports' structure:

```
# M12-C{N} — <program name>

**Brief**: one-line description.

**Wall-clock**: ~X minutes
**Files added**: examples/<name>.spy (N lines), compiler/tests/<name>_runs.rs (N).

## Result
What runs end-to-end. Paste 5-15 lines of expected stdout.

## NEW bugs discovered
For each bug, give:
- minimal repro (≤20 lines of .spy code)
- symptom (what it does vs what you expected)
- speculation about root cause / file in `compiler/src/` or `vm/src/`
- whether you worked around it and how

## Confirmed BUGS_KNOWN entries
List which currently-open or post-M11-deferred bugs you hit, with section
numbers from BUGS_KNOWN.md.

## Language-surface awkwardness (not necessarily bugs)
Things that required ugly workarounds but are arguably "spec is what it is".

## Final test totals
Output of `cargo test --release` summary line (e.g. "228 passed, 0 failed").
```

## Reporting honesty

Critical: write the report **as you go**, not at the end. The "what
went wrong" / "what I had to work around" sections are the load-bearing
material for the thesis. A laconic "I built X, here's the code" report
is significantly less valuable than a detailed "I tried A, hit B, dug
into C, found root cause D" report.

Verbatim style: paste actual error messages. List every NativeFn entry
you considered using and which ones were missing. If the bug is a
miscompile, include the wrong-output you observed.

## Final discipline check before submitting

- Run `cargo test --workspace --release`. Confirm green.
- Re-read your example program. Are there workarounds you reached for
  silently that didn't make it into the report? Add them.
- Re-read BUGS_KNOWN.md. For each open bug, did you hit it? Mention it
  if so.
- Word-count your report. 300-700 words is the target. Trim filler.
