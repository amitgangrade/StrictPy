# M13–M17 — Language-completeness sprint

**Date**: 2026-05-18
**Wall-clock**: one orchestrator session; agent compute ≈ 4 hours total.
**Headline**: closed every language-feature gap from the M10-M12
agent-report "awkwardness" sections in a single 5-milestone chain.

## Sequencing rationale

All 5 features touched `compiler/src/ir.rs` and `compiler/src/typecheck.rs`,
so parallel agents would have conflicted on integration. Sequential
agents, one per milestone, each commit pushed to main before the next
starts. The order matters: M13's mid-expression CFG pattern is the
prerequisite for M15's handler/finally lowering, and M14's
synthetic-class-layout pattern is the prerequisite for M15's lazy
exception-object materialisation.

## What shipped

| M | Feature | Workaround eliminated | Notable detail |
|---|---|---|---|
| M13 | Short-circuit `and`/`or` (BUG-035) | Nested `if` around indexed-array guards | First mid-expression CFG manipulation. |
| M14 | Tuples + destructuring | 1-element mutable lists; wrapper Pair classes | Zero new VM opcodes — heap-allocated synthetic class layouts. |
| M15 | try/except/finally + raise (BUG-025) | Sentinel returns from fallible ops | Lazy exception-object materialisation on `as e:` bind. JIT carve-out automatic. |
| M16 | isinstance + match case | `kind: i32` discriminators in every sealed AST | Flow-narrowing mirrors is-not-none. Sealed exhaustiveness warns to stderr. |
| M17 | Generic free functions | Rewrite-per-type for containers and algorithms | Lazy monomorphisation via worklist; per-instantiation operator binding. |

## Cross-milestone story

The compound effect is bigger than any one feature. Two examples:

1. **M11 + M16 ship a coherent class system.** M11 fixed subclass field
   offsets (BUG-016) so `layout.fields[i].offset` is correct for
   subclass fields. M16's Constructor pattern field-binding uses
   exactly that offset and worked first-try. Neither milestone alone
   delivers the natural Python form `case Pair(car, cdr):`.

2. **M13 → M15 dependency chain.** M13's `lower_short_circuit` is a
   prototype of M15's `lower_try`: both pre-seed a slot in the entry
   block, branch conditionally, materialise the "alternate" value in
   a fresh block, phi-merge in a join block. The M13 agent's report
   "Gotchas" section was explicitly written as a reference for M15;
   M15's report cites it.

## New bugs / honest gaps

Each agent recorded incidental findings (the M14 agent's brief said
"M14 found the assert-tuple crash; that's the bar"). Across M13-M17:

- M13: none.
- M14: **assert(cond, msg) IR-tuple-allocation crash**. Every
  example using asserts with messages would have regressed. Fixed
  in the same patch.
- M15: **`with open(...) as f:` does not route through try/except.**
  Documented in spec §7.5.4. Long-term fix: desugar `with` to
  try/finally.
- M16: none.
- M17: **`TypedModule::instantiations` was unhashable.** The field had
  been `HashSet<(SymbolId, Vec<Ty>)>` since M1 but `Ty` doesn't derive
  `Hash`/`Eq` — nothing had ever inserted into it. Switched to
  `Vec` + string-keyed dedup. No user-visible symptom; archaeology
  finding.

Pattern: incidental-bug discovery rate is roughly one substantive find
per milestone touching the compiler. Lower than M10's stress-test rate
(one program → 8 bugs) but still nonzero — language features add
their own attack surface.

## Test growth

```
M12 → 206
M13 → 212 (+6 from m13_short_circuit.rs)
M14 → 222 (+8 m14_tuples.rs, +2 dijkstra_with_tuples_runs.rs)
M15 → 234 (+10 m15_try_except.rs, +2 safe_open_runs.rs)
M16 → 245 (+9 m16_match_isinstance.rs, +2 calculator_with_match_runs.rs)
M17 → 255 (+8 m17_generics.rs, +2 quicksort_generic_runs.rs)
```

## Spec changes

- §5.1.5 — generic free functions
- §5.5 — tuples (M14)
- §6.5 / §6.5.1 — isinstance / match (M16)
- §7.5 — try/except/finally (M15)
- §9.1 — isinstance in the builtin list

## Next-step menu (post-M17)

The "language completeness" set is essentially done for v0.1. Remaining
options, ordered by my assessment of ROI:

- **F: Spec catch-up v0.1 → v0.2 (formal).** STRICTPY_SPEC.md is now
  honest about M13-M17 surface but the document is internally
  inconsistent at the section level — the original v0.1 organisation
  was M0-era. A focused agent could renumber and re-flow it.
- **J (new): Migrate existing examples to use the new surface.**
  json_parse, lisp, calculator, lambda_calc all carry obsolete
  workarounds (open class instead of sealed; kind:i32 discriminators).
  A cleanup pass would shrink examples LOC ~20-30% and make the
  thesis examples more presentable.
- **K (new): Round 4 of stress tests.** ROI curve flattening but
  the new surface (try/except, match, generics) hasn't been stressed
  by anything yet. Likely to surface bugs.
- **L (new): `with` → try/finally desugaring** (closes the M15 known gap).
- **G: Draft the thesis.** Archive is ready; M0-M17 covered.
- **H: Performance deep-dive.** Headline numbers unchanged since M11;
  fib(30) at 13.1ms might be improvable.
- **M (new): User-defined exception subclasses.** Pre-built exception
  names work but `class MyError(Exception):` doesn't. Small lift.
- **N (new): Generic classes** (`class Box[T]:`). Was v0.2 in M17 brief.
  Larger lift; the worklist machinery is reusable.
- **O (new): Bounded generics** (`T: Comparable`). Would let
  `quicksort[T]` compile-time-check operator availability instead of
  failing at instantiation. Smaller lift if a protocol/trait system
  is already in place.
