# M18-expr_interp — `examples/expr_interp.spy`

**Brief**: a small expression-language interpreter (arithmetic + vars +
let + if + i64/str values) that stresses the M15+M16 surface — sealed
AST + sealed runtime-value domain walked entirely through `match`, with
`try`/`except`/`raise` catching four distinct error flavours raised
from inside recursive `eval()` frames.
**Wall-clock**: ~90 minutes.
**Files added**:
- `examples/expr_interp.spy` (~430 lines)
- `compiler/tests/expr_interp_runs.rs` (2 tests)
- `examples/_probe_divzero_name.spy` (BUG repro — see below)
- `examples/_probe_match_scrut_throws.spy`
- `examples/_probe_match_nonexhaustive.spy`
- `examples/_probe_finally_uncaught.spy`
- `examples/_probe_reraise_as_e.spy`
- `examples/_probe_generic_env_match.spy`
- `examples/_probe_isinstance_and.spy` (deferred-feature workaround demo)
- `examples/_probe_nested_constructor_pat.spy` (deferred-feature workaround demo)

## Result

10 cases run end-to-end:

```
PASS test=1 (1 + 2 => 3)
PASS test=2 (let x = 10 in x * x => 100)
PASS test=3 (if 1 < 2 then "yes" else "no" => "yes")
PASS test=4 (1 / 0 raised DivisionByZeroError)
PASS test=5 (1 + "two" raised TypeError)
PASS test=6 (undefined_var + 1 raised RuntimeError)
PASS test=7 (nested let => 4)
PASS test=8 (if 1==1 then 42 else "no" => 42)
PASS test=9 (2 * (1 / 0) raised DivisionByZeroError)
PASS test=10 ("hello" + " " + "world" => "hello world")
OK: 10/10
```

Exit code 0; main returns 0 iff `passed == total`.

## NEW bugs discovered

### BUG-CANDIDATE — `ZeroDivisionError` vs `DivisionByZeroError` spec/runtime mismatch

**Symptom**: spec §7.5.1 lists `ZeroDivisionError` as the canonical
name with "Legacy name `DivisionByZeroError` is also recognised". In
practice the runtime emits the *legacy* name on native `/ 0` traps, and
arm-filter matching is by exact string compare — so the documented
canonical name does NOT catch:

```python
try:
    z: i64 = 1i64 / 0i64
except ZeroDivisionError as e:    # does NOT match — falls through
    println("never")
except Exception as e2:
    println(e2.type_name)         # prints "DivisionByZeroError"
```

Saved at `examples/_probe_divzero_name.spy`.

**Speculation about root cause**: four sites in `vm/src/interp.rs`
(lines 911 / 943 / 974 / 1004 — for i32, i64, i32 modulo, i64 modulo
respectively) hard-code `type_name: "DivisionByZeroError".into()`.
Handler-filter matching in `propagate_exception` (`vm/src/interp.rs`
line 456) is `arm.filter == "Exception" || arm.filter == type_name` —
no alias-aware compare. Spec §7.5.1's "also recognised" line is
implementation-aspirational, not real.

**Mechanical fix options**: either (a) change the four `interp.rs`
emit sites to `"ZeroDivisionError"` (canonical per spec, but breaks any
program that does `except DivisionByZeroError`); or (b) extend the
arm-filter match to alias `("ZeroDivisionError", "DivisionByZeroError")`
as equivalent. Either is a ~5 LOC change. The resolver+typechecker
already accept both names (`compiler/src/resolver.rs:410`,
`compiler/src/typecheck.rs:1536-1537`).

**Worked around**: yes. `expr_interp.spy`'s `run_raises_case` catches
`Exception` as a residual arm and inspects `e.type_name`, so the test
passes against whichever spelling the runtime chooses. Tests 4 and 9
assert against `DivisionByZeroError` (the empirical value) with a
header comment pointing to this report.

## Confirmed gaps

All match the M16/M17 brief flags:

1. **isinstance narrowing through `and`/`or`** — `if isinstance(v, IV)
   and v.n > 0:` fails with `error[E2003]: no field `n` on type V` (the
   un-narrowed type). Per `m16_match_isinstance.md` Strategy §3, this
   is intentional. `_probe_isinstance_and.spy` documents the failure
   and shows the nested-if workaround used in `expr_interp.spy`'s
   `require_int`.

2. **Nested Constructor patterns** — `case Pair(IV(a), IV(b)):` parses
   without complaint but lowers as if `IV(a)` and `IV(b)` were
   *expressions* in pattern position; the inner names never bind, and
   the next reference to `a` fails with `error[E1004]: unknown name
   `a``. Per spec §6.5.1: deferred to v0.2. The diagnostic is
   confusing (should be "nested constructor patterns are not yet
   supported") but the feature works correctly via the flattening
   workaround in `_probe_nested_constructor_pat.spy`.

## Language-surface awkwardness (not necessarily bugs)

- **No method-on-Parser**: per BUGS_KNOWN.md #4/#5, methods on a class
  that *also* holds non-trivial state still destabilise the heap. So
  `Parser` is bare state + `__init__`, and `p_peek`/`p_skip_ws`/etc.
  are all free functions taking `p: Parser`. Same workaround as
  `calculator.spy`. Mechanical, but worth flagging that the natural
  OOP shape is still unsafe one milestone after M16.

- **`raise e` on a bound exception works, undocumented**: spec §7.5.6
  lists "Bare `raise` (re-raise)" as out of scope but is silent on
  `raise e` where `e: Exception` is the binding from `except ... as
  e:`. My `_probe_reraise_as_e.spy` confirms this round-trips: inner
  handler catches, prints, then `raise e` re-throws past the inner and
  the outer handler catches. Worth a spec note.

- **`finally` on uncaught propagation runs (good!)**: confirmed by
  `_probe_finally_uncaught.spy`. Spec §7.5.4 promised this and the
  runtime delivers it across recursive frames — even though my
  `expr_interp.spy` doesn't itself rely on this (its top-level handler
  catches all four exception flavours specifically).

- **Match scrutinee that raises propagates correctly**:
  `_probe_match_scrut_throws.spy`. The scrutinee is evaluated in the
  caller's frame (per the "evaluated exactly once into a hidden slot"
  lowering, §6.5.1), so an exception fired during scrutinee evaluation
  unwinds normally past the un-entered match arms. The brief had
  flagged this as spec-undefined; I'd argue the observed behaviour is
  the only sensible one and should be promoted to spec.

- **Exhaustiveness warning** (§6.5): fires correctly on a sealed-class
  match missing one arm without `_:`; does NOT fire on
  `expr_interp.spy`'s 5-arm match (Lit/Var/Binop/If/Let). Confirmed in
  `_probe_match_nonexhaustive.spy`.

- **String concatenation via `+` over `StrVal` works** without
  surprises; left-associative chaining `"hello" + " " + "world"`
  produces `"hello world"` (test 10).

## Workaround DELTA vs pre-M13

Not a strict rewrite of an existing program — `expr_interp.spy` is new
shape. Closest pre-M13 cognate is `calculator_with_match.spy` (M16);
mine adds runtime errors and a string-typed value variant, which the
M16 calculator doesn't have.

## Final test totals

```
cargo build --workspace --release  # success
cargo test  --workspace --release  # 263 passed; 0 failed; 1 ignored
```

The 1 ignored is the unchanged pre-M12 `probe_str_ne_bug_repro`.
Baseline before this task was 255 passed; M18 round-4 agents add +6
across siblings plus my +2 (`expr_interp_runs.rs`).

## What I tried but didn't push

- **Generic `env_lookup[K, V]`**: I have it as a probe
  (`_probe_generic_env_match.spy`) but kept the actual `expr_interp.spy`
  env on parallel `List[str]` + `List[Value]` because (a) the brief
  said M17 generics were optional for this task and (b) a generic
  env'd add 0 stress to the M15+M16 surface, which is the load-bearing
  part.

- **More-than-5-frame deep recursive division**: tested
  `2 * (1 / 0)` (6 frames) and confirmed unwind. Going deeper would
  not have stressed anything new — the handler-frame walk is O(frames)
  per `propagate_exception` and already crosses recursive eval/match
  layers.

- **Generic + `match` body**: my `_probe_generic_env_match.spy`
  exercises a generic + nested match in the *caller* but not a generic
  *body* containing a match. The brief flagged "generic + match case"
  (item 3) as a likely bug source; deferring to a sibling M18 agent's
  scope since the stress profile here is M15+M16 first.

- **Match scrutinee that is itself a generic call** — would exercise
  M17's monomorphisation through the M16 hidden-slot lowering;
  out-of-scope for this task.
