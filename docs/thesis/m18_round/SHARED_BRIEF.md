# M18 — Round 4 stress test (shared briefing)

The first stress round to specifically target the M13-M17 surface.
Read this before your task-specific brief.

## Project context

StrictPy is at end-of-M17 (post-M0–M17). 255 tests passing, 0 failed,
1 ignored. 28 example programs. 31 bugs found, 30 fixed, 1 deferred
(BUG-028 lexer line continuation only). The language is now
"meaningfully Python-shaped." This round exists to validate that —
historically every stress round has found bugs the regression tests
missed (M10: 17 bugs; M11: 6; M12: 2). M13-M17 added five language
features whose only test coverage is each milestone's own regression
file. The point of this round is **finding what's actually broken,
not shipping a clean demo**.

## Files to read FIRST (in order)

1. `STRICTPY_SPEC.md` — focus on §5.1.5 (generics), §5.5 (tuples),
   §6.5 / §6.5.1 (isinstance + match), §7.5 (try/except).
2. `BUGS_KNOWN.md` — only BUG-028 is open. Don't waste time on it.
3. `docs/thesis/agent_reports/m13_short_circuit.md` through `m17_generics.md`
   — five reports that document what each feature actually ships,
   what's deferred, and what gotchas the implementing agent flagged.
4. `docs/thesis/bugs/catalog.md` — historical patterns (BUG-008 / BUG-034
   negative-form silent miscompiles; BUG-001 audit; BUG-029 latent hack).
5. `shared/src/native.rs` (`NativeFn` enum at top) — stdlib.
6. **Crucially**: `examples/quicksort_generic.spy` (M17), `examples/safe_open.spy`
   (M15), `examples/calculator_with_match.spy` (M16), `examples/dijkstra_with_tuples.spy`
   (M14). These are the only programs that touch the new surface;
   read all four to see the conventions in use.

## What's new since M12 (calibrate your expectations)

**Things that USED to be missing and ARE NOW AVAILABLE:**

- **Tuples + destructuring** (M14):
  ```python
  fn pair() -> Tuple[i32, str]: return (42, "hi")
  x, y = pair()
  t: Tuple[i32, str] = pair()
  println(str(t.0))
  ```
  Tuple equality is element-wise. `str(tuple)` formats `"(a, b)"`.
  Arity 2..=8.

- **Short-circuit `and`/`or`** (M13): `b > 0 and xs[b-1] > 0` no longer
  trips IndexError(-1). Standard guard idioms work.

- **try/except/finally + raise** (M15):
  ```python
  try:
      f = open("missing.txt", "r")
  except IOError as e:
      println("missing: " + e.message)
  finally:
      cleanup()
  ```
  Built-in exception names: `Exception`, `IOError`, `IndexError`,
  `KeyError`, `ValueError`, `TypeError`, `ZeroDivisionError`,
  `AssertionError`, `NullPointerError`, `RuntimeError`,
  `ChannelClosedError`. `e.message: str` and `e.type_name: str`.
  `finally` runs on normal completion, after caught handlers, and
  before uncaught propagation.

- **isinstance + flow narrowing** (M16):
  ```python
  if isinstance(a, Cat):
      a.meow()   # `a` is narrowed to Cat in this branch
  ```
  Subclass-chain walking via vtable. Works on nullable receivers
  (narrows to non-nullable T).

- **match case patterns** (M16):
  ```python
  match v:
      case Number(n): println(str(n))
      case Pair(c, d): println(str(c.tag()))
      case (a, b): println(str(a) + " " + str(b))
      case _: println("other")
  ```
  Constructor patterns (positional field binding), Tuple patterns,
  Wildcard, Identifier, literal patterns. Scrutinee evaluated once.

- **Generic free functions** (M17):
  ```python
  fn id[T](x: T) -> T: return x
  fn first[K, V](p: Tuple[K, V]) -> K: return p.0
  ```
  Call-site inference from argument types. Transitive instantiation
  works (outer[T] calling inner[U] where U inferred from T's value).
  Per-instantiation operator binding (T + T becomes IAdd for i32,
  StrConcat for str).

## Things still NOT supported (don't waste time on these)

- **Generic classes** (`class Box[T]:`) — parses but typecheck errors.
  v0.2. Use generic FREE FUNCTIONS over `List[T]` / etc. instead.
- **Generic methods on non-generic classes** — same. v0.2.
- **Bounded generics** (`T: Comparable`) — no bound system; per-instantiation
  re-typecheck catches operator failures at instantiation. v0.2.
- **User-defined exception subclasses** (`class MyError(Exception):`) —
  only the 10 built-in exception names work. v0.2.
- **`with open(...) as f:` does NOT route through try/except** — known
  M15 gap. If you need to catch IOError on the open call, write
  `try: with open(...) as f: ... except IOError:` explicitly. (Or
  bypass `with` and do the close manually in `finally`.)
- **Nested constructor patterns** (`case Pair(Number(n), c):`) — v0.2.
  Only Identifier and Wildcard sub-patterns.
- **Or-patterns** (`case A | B:`), guards (`case Pat if cond:`),
  range/mapping patterns — v0.2.
- **BUG-028**: no implicit line continuation across infix `+`. Use
  accumulator pattern: `s = s + "..."`.
- **NumPy / pandas** — architectural; never planned for v0.1.

## What to find (this is the point of the round)

Pre-M10 stress rounds found bugs at a rate of ~6-17 per round. M12 found
only 2 (BUG-034 + BUG-035) because most of the stress targeted the M11
class system, which had been refactored carefully. The M13-M17 surface
is new and has only been exercised by each milestone's own regression
tests. Likely bug patterns to probe:

1. **Generic + tuple combinations**: `fn pair[A, B](a: A, b: B) -> Tuple[A, B]`.
   Does mangling encode tuple types correctly? Does the worklist see
   `pair__i32_str` and `pair__str_class<5>` as distinct?

2. **Transitive instantiation depth**: `outer[T] -> middle[T] -> inner[T]`
   over ≥3 levels. Does the worklist drain fully?

3. **Generic + match case**: a generic function whose body contains
   `match scrutinee: case Foo(x): ...` — does the per-instantiation
   re-typecheck flow through match arms?

4. **try/except inside a match arm**: `case Number(n): try: 1/n except ZeroDivisionError: ...`.
   Does handler-frame management nest correctly?

5. **try/except inside a generic function**: does the JIT carve-out
   correctly fire for ALL monomorphic instantiations?

6. **finally + early-return in try**: edge case the M15 brief flagged
   as potentially under-handled.

7. **Tuple-of-class-refs in match Constructor patterns**: e.g.
   `case Pair((c1, c2)): ...` — does the nested Tuple sub-pattern work?
   (Probably not — that's a nested constructor pattern, v0.2 — but
   confirm.)

8. **isinstance flow narrowing through and/or**: M16 brief said no.
   Confirm and document.

9. **Exception thrown from inside `match` scrutinee evaluation**:
   `match expensive(): case ...` where `expensive()` raises.

10. **Re-raising an exception captured by `as e`**: not in M15 scope
    (defer doc), but worth probing — what does `raise e` do?

11. **str-keyed dict + tuple value + generic** — the M14 brief noted
    tuples in dict keys are out of scope; what about as values?

12. **Generic function called with literal tuple**: `f((1, "two"))`
    where `f[T, U](p: Tuple[T, U])` — does inference work?

## File-ownership boundaries (parallel agents — DO NOT touch others')

You may only create/edit the files listed in your task brief. The four
agents own disjoint files. Do NOT modify:
- Any other example program.
- Any file in `compiler/src/` or `vm/src/`. If you find a real bug,
  document it with a minimal repro saved as `examples/_probe_<thing>.spy`
  — the orchestrator decides whether to fix.
- `BUGS_KNOWN.md`, the bug catalog, the timeline. The orchestrator
  integrates your findings.

If you find a fix that's mechanically tiny (e.g. adding a missing
NativeFn entry, fixing a typecheck synth table), write it up in your
report — don't apply it.

## STOP CRITERIA

- If after **45 minutes** you don't have a working version of your
  program (even with workarounds), STOP and report what's broken.
- If you find that one of the M13-M17 features is fundamentally broken
  for your shape (e.g. transitive generics don't work past 2 levels),
  STOP, save the minimal repro at `examples/_probe_<feature>.spy`,
  report it as the headline.
- If `cargo test --workspace --release` was green when you started
  and is red on a file you didn't touch, that's a regression on
  someone else's territory — stop and flag.

## Acceptance criteria

1. `cargo build --workspace --release` succeeds.
2. `cargo test --workspace --release` includes your tests; they all
   pass OR you've documented exactly why they can't.
3. Your program is in `examples/<name>.spy`, ~100-400 lines.
4. Your test file is `compiler/tests/<name>_runs.rs`.
5. Your verbatim report is at `docs/thesis/agent_reports/m18_<name>.md`.

## Report shape (~400-800 words)

Mirror the M12 reports' structure:

```
# M18-<task> — <program name>

**Brief**: one-line description.
**Wall-clock**: ~X minutes
**Files added**: ...

## Result
What runs end-to-end. Paste 5-15 lines of expected stdout.

## NEW bugs discovered
For each bug:
- minimal repro (≤20 lines of .spy code)
- symptom
- speculation about root cause / file in `compiler/src/` or `vm/src/`
- whether you worked around it and how

## Confirmed gaps
List M13-M17 features that hit the documented limitations. Quote spec
section + which probe you tried.

## Language-surface awkwardness (not necessarily bugs)
Things that required ugly workarounds but are arguably "spec is what
it is."

## Workaround DELTA vs pre-M13 (if applicable)
For programs that rewrite/replace a pre-M13 example: LOC and
readability comparison.

## Final test totals
Output of `cargo test --release` summary line.
```

## Reporting honesty

Critical: write the report **as you go**, not at the end. The "what
went wrong" / "what I had to work around" sections are the load-bearing
material for the thesis. A laconic "I built X, here's the code"
report is significantly less valuable than a detailed "I tried A,
hit B, dug into C, found root cause D" report.

Paste actual error messages. List the M13-M17 features you tried and
whether each worked. If your program is a rewrite of a pre-M13 program,
include before/after LOC.

Before submitting:
- Re-read your example. Are there workarounds you reached for silently
  that didn't make it into the report? Add them.
- Re-read this brief's "What to find" section. For each probe you didn't
  try, note why (out of scope for your task, etc.).
- Word-count: 400-800 words. Trim filler.
