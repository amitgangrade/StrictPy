# M10 prep — CSV aggregator: first real-world stress test

**Brief**: write a working CSV aggregator (group by string column, sum
numeric column). The goal of this task is **finding gaps**, not just
shipping a program.

**Wall-clock**: ~14 minutes
**Files added**: `examples/csv_aggregate.spy` (143 lines), `bench/data/sample.csv`,
`compiler/tests/csv_aggregate_runs.rs` (93 lines).

## The headline finding

**Discovered BUG-001: nullable-narrowing dispatch bug.**

The pattern:
```python
prev: f64? = agg.get(key)
if prev is none:
    agg[key] = amount
else:
    agg[key] = prev + amount   # ← silently emitted integer add over f64 bits
```

The type checker correctly narrows `prev` to `f64` in the `else` branch.
But `ir.rs::lower_binop` checked `Ty::Primitive(p) if p.is_float()` —
which returns `false` for `Ty::Nullable(F64)`. The narrowed type lives in
the type-check side table; the IR slot still carries the declared `f64?`.
So `+` fell through to the integer branch and emitted `IAddI64` over the
raw f64 bit pattern. Every float aggregation was garbage that
`str(f64)` happened to print as `0.0`.

**Why no benchmark caught this**: `wordcount.spy` is the only other
program using `Dict.get()` → nullable → arithmetic. It uses
`Dict[str, i32]` — both branches take the integer path. Float nullable
arithmetic had never been exercised end-to-end. Six-line fix; first time
running an unfamiliar 143-line program found a critical bug in 15 minutes.

This finding triggered M10's nullable-narrowing audit (M10-AB), which
found 4 more silent miscompiles with the same pattern in
`compiler/src/codegen.rs`.

## Stdlib additions

- `NativeFn::F64FromStr = 26` + `I64FromStr = 27`. Before this, StrictPy
  could convert between numeric types (`f64(x)`) but couldn't parse `"3.14"`
  into a number. Any program reading numeric data from text would hit
  this wall.

## Documented language gaps

| Gap | Workaround | Cost |
|---|---|---|
| No `for x in xs:` | `while i < len(xs): … i += 1` | 4 occurrences in 143 lines |
| No `str.split(sep)` | Hand-write a state-machine splitter | 3rd time writing this |
| No `sorted()` / `list.sort()` | Reimplement Lomuto quicksort | Generics absent in user code |
| No printf formatting | `str(f64)` gives shortest-round-trip — `83.5` not `83.50` | Aesthetic |
| `with X as f: type:` syntax | Explicit type annotation feels parser-hack-y | Aesthetic |

## The program

143 lines for what would be ~30 lines of Python. The bloat is concentrated
in two places: the manual `while`-with-index loops (4 of them) and the
rewritten Lomuto quicksort for `List[str]`. The aggregation core looks
clean and Python-like. "It reads like a reasonable language if you
mentally squint past the loop boilerplate."

## Output

```
category,total
gas,83.5
groceries,36.25
restaurants,50.75
```

## Why this report matters

The CSV aggregator validated the hypothesis that motivated the entire
M10 round: **real-world programs surface bugs that micro-benchmarks
don't.** One program. One critical silent miscompile. 15 minutes. The
return on investment of "write a real program in the language" was
demonstrably higher than the prior 4-program benchmark suite.
