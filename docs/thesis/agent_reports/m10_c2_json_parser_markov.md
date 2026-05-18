# M10 — C2 agent: JSON parser + Markov chain

**Brief**: write a recursive-descent JSON parser with a tagged-union AST,
plus a Markov chain text generator. Goal is to surface bugs by exercising
patterns the existing examples don't.

**Wall-clock**: ~93 minutes
**Tool uses**: 348
**Files added**: `examples/json_parse.spy` (374 lines), `examples/markov.spy` (157 lines),
`compiler/tests/json_parse_runs.rs` (66 lines), `compiler/tests/markov_runs.rs` (68 lines).

## The headline finding

**`is not none` is INVERTED at the IR level.** Every program in the codebase
that uses `if x is not none:` had been silently running the wrong branch
since the type system landed in M2. The bug went undetected because every
existing example was written using `if x is none: ... else: ...` workarounds.

This is the single most consequential bug found in the project's stress
testing. Severity: critical. Status: fixed in M10's follow-up fix pass.

## All 8 bugs surfaced by this one agent

1. **Sealed-class virtual dispatch drops to base method.**
   `b: SealedBase = SealedSub(); b.name()` returns `SEALED_BASE`, not
   `SEALED_SUB`. Same code with `open class SealedBase` works.
   → Deferred. `BUGS_KNOWN.md §1`.

2. **Subclass field offsets alias parent's last field.** Reproduced for
   i32/i64/f64/bool/str — every combination overlaps. `Sub(Base) { n: i32; value: bool }`
   ends up with `self.kind == 1` because `value`'s bool is packed into the
   wrong slot.
   → Deferred. `BUGS_KNOWN.md §2`. High-severity but architectural.

3. **Virtual dispatch table wraps modulo 4.** With ≥4 subclass overrides
   of the same base method: 4th → base impl, 5th → 1st sub, 6th → 2nd sub.
   Confirmed with 6 trivially-identical subclasses C1..C6.
   → Deferred. `BUGS_KNOWN.md §3`.

4. **`is not none` is INVERTED.** (See above.)
   → Fixed in M10 follow-up pass.

5. **`str(c: char)` returns codepoint as decimal text.** `str('h')` prints
   `"104"`. Root cause: IR lowerer routes every `str(x)` to
   `NativeFn::StrFromAny` which has no way to distinguish a char-typed u64
   from i64. `StrFromChar = 10` exists but is unreachable.
   → Fixed in M10 follow-up pass.

6. **Mid-parse VM heap corruption on the json_parse program.** Cumulative
   small allocations trigger STATUS_HEAP_CORRUPTION on Windows. Crash is
   non-deterministic. Depends on subclass declaration order (probe 53)
   AND on whether a method with a `str` parameter is defined on a class
   that also defines a JsonValue-style hierarchy.
   → Deferred. `BUGS_KNOWN.md §4`. Likely GC/JIT teardown interaction.

7. **Adding a free function before another reorders crash behavior.**
   Probe 63: defining `fn parse_num(x: i32) -> i32: return 0` between two
   unrelated functions toggles whether the program crashes. Position-
   sensitive — function table indexed by source position somewhere it
   shouldn't be. Probably same root cause as #6.
   → Deferred. `BUGS_KNOWN.md §5`.

8. **No implicit line continuation across trailing `+`.**
   `return "a " +\n    "b"` errors with E0001. Forces accumulator-pattern
   string building.
   → Deferred. `BUGS_KNOWN.md §6`. Lexer enhancement.

## The Dict[str, List[T]] surprise

The agent expected `Dict[str, List[str]]` to hit the "primitive-valued
dicts only" limitation documented in M5's report. **It actually works fine
in v0.1.** The agent confirmed with a probe: round-trips through `d.get(k)`
/ `d[k] = xs`, mutations persist on subsequent reads. Markov uses the
natural Dict shape.

For JSON the agent used parallel arrays (`List[str]` keys + `List[JsonValue]`
values) anyway because `Dict[str, JsonValue]` would have involved the
buggy class hierarchy and they didn't want to muddy the bisect.

## The sealed-class workaround

Bugs 1, 2, and 3 ganged up. The final JsonValue hierarchy in
`examples/json_parse.spy` is:

- `open class JsonValue` with no fields (works around bugs 1 and 2)
- 3 subclasses max: `JsonObject`, `JsonArray`, `JsonAtom` (works around bug 3)
- `JsonAtom` carries its own internal `kind: i32` discriminator that
  subsumes null/bool/number/string (collapses 6 logical variants → 3 actual
  subclasses)

External discrimination relies on a virtual `render(self) -> str` method.
The spec's `match` / `case` syntax parses but the IR lowerer treats
`Stmt::Match` as an M4 placeholder (`compiler/src/ir.rs:894`).

## Output verification

**Markov**: clean. 41 generated words via LCG with seed 1234567. Sample:
```
Alice without pictures or conversations in her sister was beginning to hear the Rabbit say to do once or conversations So she was considering in her that nor did Alice was considering in her feel very much out of getting up
```

**JSON**: partial. Atom-only round-trips (`null`, `true`, `false`) print
correctly. The headline 5-key/nested-array input triggers the heap
corruption (bug #6) mid-parse. Running through `run_file_capture` (in-process)
crashes during VM teardown even when the program prints correct output
first. Exit code `0xC0000374` = STATUS_HEAP_CORRUPTION.

## Why this report is preserved verbatim

This single agent task surfaced more bugs than the prior 8 milestones
combined. The honest "what's broken" detail — the probe-by-probe bisect
through 53+ minimal repros — is the source of all six new entries in
`BUGS_KNOWN.md`. Without verbatim preservation, the thesis would lose the
empirical evidence that stress-testing produces superlinear bug discovery
compared to test-writing.
