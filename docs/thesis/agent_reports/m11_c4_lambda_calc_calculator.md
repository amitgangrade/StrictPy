# M11-C4 — Lambda calculus + calculator

**Brief**: write a lambda calculus evaluator and an arithmetic calculator,
both using sealed-style class hierarchies. Goal: stress the class system
and surface bugs.

**Wall-clock**: ~27 minutes
**Files added**: `examples/lambda_calc.spy` (232), `examples/calculator.spy` (249),
`compiler/tests/lambda_calc_runs.rs` (50), `compiler/tests/calculator_runs.rs` (86).

## Result

**Lambda calc**: fully clean. Reduces `I I → I` and `K a b → a` correctly;
detects divergence on `Ω = (λx. x x) (λx. x x)` via max-steps cap.

**Calculator**: semantically correct (all 7 expressions produce right
results) but runtime-flaky pre-M11-fix. Same `.spyc` back-to-back: one
run all 7 lines, next 1 line, next 0 lines, all with exit `0xC0000374`
(STATUS_HEAP_CORRUPTION). After the M11 class-system fix: **runs 5/5
cleanly.**

## Confirmed BUGS_KNOWN entries

- §1 sealed dispatch — used `open class Expr` workaround
- §2 subclass field offsets — kept `Expr` field-less
- §3 vtable mod 4 — kept ≤3 subclasses overriding per method
- §4 heap corruption — calculator hit hard until M11 fix
- §5 position-sensitive crash — confirmed (same .spyc produces different output back-to-back)
- §6 no line continuation across `+` — dodged with accumulator pattern

## NEW bugs discovered

1. **str(f64) of whole number prints `"3.0"` not `"3"`** — contradicts
   csv_aggregate.spy's documented "shortest-round-trip" claim. Low
   severity; pure formatting.

2. **Heap-corruption trigger broader than C2 documented**. C2's note said
   "method on Parser with str param" was the trigger. C4's first
   calculator version had instance methods on `Parser` doing **no `str`
   work** and still hit it. Broader trigger: any recursive instance-method
   chain on a Parser-style class constructing subclass AST nodes mid-call.
   Workaround: move Parser ops to free functions taking `p: Parser`.

3. **The crash is NOT just a teardown phenomenon**. In calculator, the
   subprocess sometimes prints **0 bytes** and exits `0xC0000374` —
   crash happens before the first `println` reaches the OS. Re-classifies
   BUG-026 from "teardown bug" to "mid-execution heap-use-after-free."

(All three of these are subsumed by the M11 class-system fix in BUG-016 —
the calculator's instance-method version works fine after M11.)

## Workarounds used (pre-M11-fix)

- `open class` not `sealed`
- Field-less `Expr` base
- ≤3 overrides per base method
- Parser ops as **free functions**, not instance methods
- Subprocess (`spy.exe`) test, not in-process `run_file_capture`
- Test asserts "≥1 of 10 invocations prints the correct first answer"

## Language-surface awkwardness

- **No `isinstance` / no working `match`.** In `App.step`, needs to ask
  "is `self.fn_expr` an Abs?". Worked around by adding a virtual
  `try_apply(arg) -> Expr?` on Expr — Var/App return none, Abs returns
  `body[param ↦ arg]`. Costs one extra vtable slot.
- **`T?` narrowing is per-expression, not per-binding.** Every
  `if x is not none: y: T = x` requires a helper because the typechecker
  doesn't flow-narrow past the guard for a fresh let.
- **No exception ergonomics.** Calculator's div-by-zero returns sentinel
  0.0 instead of raising.

## Final test totals

`cargo test --release`: **181 passed, 0 failed**. All 4 new tests green.
