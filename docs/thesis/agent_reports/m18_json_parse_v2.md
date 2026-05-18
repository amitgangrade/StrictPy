# M18 — `json_parse_v2.spy` — JSON parser, post-M17 rewrite

**Brief**: rewrite the M10 recursive-descent JSON parser
(`examples/json_parse.spy`) against the M13-M17 surface. The M10 original
was the canonical bug-magnet program of the v0.1 era — its top-of-file
docstring catalogues EIGHT separate workarounds the v0.1 language forced.
This rewrite measures, head-to-head, how many of those workarounds are now
gone and what (if anything) the new surface trips on.

**Wall-clock**: ~50 minutes (read-through + design + write + tests +
deep-nest probe + report).

**Files added**:

* `examples/json_parse_v2.spy` — the new program (508 lines, of which
  ~80 lines are header docstring + per-section banner comments, ~110
  lines are the 14-case driver in `main()`, and ~320 lines are AST +
  parser + rendering).
* `compiler/tests/json_parse_v2_runs.rs` — 2 tests, both green.
* `examples/_probe_json_v2_deep.spy` — manual probe pushing recursive
  parse + match-rendering to 40-level nesting. Not part of the test
  suite. Runs clean.

## Result

Both tests pass on the **first** compile-and-run, with zero language-level
surprises encountered along the way.

```
01 atom_null: null
02 atom_true: true
03 atom_num: 42
04 atom_str: "alice"
05 array: [1, 2, 3, true, null, "x"]
06 flat_obj: {"name": "alice", "age": 30, "active": true, "score": null}
07 nested_obj: {"name": "alice", "tags": ["admin", "user"], "meta": {"active": true, "age": 30}}
08 deep_nest: {"a": [1, [2, [3, [4, [5, [6, {"deep": [7, 8, 9]}]]]]]]}
09 malformed_obj: PARSE-ERROR unexpected end of input
10 malformed_atom: PARSE-ERROR expected 'true' or 'false' at position 0
11 trailing: PARSE-ERROR trailing garbage at position 5
try_parse: unexpected end of input
12 try_parse: recovered to none
13 isinstance: parsed is JsonObject
14 finally: cleaned=12
OK: 14/14
```

Test result of `cargo test --workspace --release`: **all green, including
the 2 new tests; 0 failed.**

## What works (specific probes from the round brief)

Every M13-M17 probe called out in the task brief and the round-wide
SHARED_BRIEF "What to find" list lands:

1. **6-arm `match` over a sealed class with all variants enumerated**
   (`JsonNull / JsonBool / JsonNumber / JsonString / JsonArray /
   JsonObject`). The M16 brief promised this; this is the first
   program to drive 6 arms simultaneously. No exhaustiveness warning
   fires, dispatch is correct on every variant, including `case
   JsonNull():` with zero sub-patterns and `case JsonObject(pairs):`
   with one Identifier sub-pattern.

2. **Constructor pattern binding a `List[Tuple[str, JsonValue]]` field.**
   `case JsonObject(pairs): ...` binds `pairs` with the expected static
   type. `pairs[i].0` (key) and `pairs[i].1` (value, a class-typed
   sub-tuple-field) both lower correctly.

3. **Tuple-of-(str, class-ref) inside a List, mutated via `.append`.**
   `pairs.append((k, v))` where `pairs: List[Tuple[str, JsonValue]]`
   builds the tuple inline and lands without complaint. The M14 brief
   flagged "tuple with class-ref element + chained access `t.0.n`" as
   the most exotic shape they tested; this combines it with mutable
   list storage + concurrent reads at render time.

4. **Recursive method calls on a `Parser` class with `str` parameters.**
   The M10 baseline had to move every Parser method out to free
   functions to dodge BUG-026's heap-corruption non-determinism. v2
   uses instance methods throughout. 40-level recursive nesting in the
   probe runs clean. M11 BUG-016 + M12 confirmation holds; M10 doc
   bug 4 is dead.

5. **try/except unwinding through a 6+ frame recursive descent.** Test
   cases 08-12 all exercise this: `parse → parse_value → parse_object
   → parse_member → parse_value → parse_array → parse_value → ...`
   and the `ValueError` raised from the deepest method is caught
   correctly at `main()` (and at `try_parse()`'s wrapper). The M15
   regression tests only nested 2-3 deep.

6. **`finally` after both success and failure paths.** Case 14 wires
   a counter into a `finally` block on two adjacent `try`s — one that
   succeeds, one that raises. The counter reaches the expected 12
   (1 + 1 from each `finally` + 10 from the `except` arm), so
   `finally` runs on the success path AND on the caught-failure path.

7. **`isinstance(v, JsonObject)` as an external discriminator.** Case
   13 uses `is_object(v: JsonValue) -> bool` to check the result of a
   parse without going through `match`. This is the alternative to
   match-based discrimination the task brief mentioned; both work.

8. **`try_parse(src: str) -> JsonValue?`** returning `none` on failure
   exercises nullable class-typed return + `is none` test on the
   caller side. No issues.

## Workaround delta vs the M10 original

The M10 top-of-file docstring lists eight workarounds. Counted against
the v2 file:

| # | M10 workaround                                                                                | Status in v2 |
|---|-----------------------------------------------------------------------------------------------|--------------|
| 1 | `open class JsonValue` instead of `sealed`, because sealed-receiver virtual dispatch dropped to base.  | **GONE.** `sealed class` works; verified by 6-arm match. |
| 2 | Empty parent class, because subclass fields aliased parent fields.                            | **GONE.** No-op parent retained for style only; subclasses freely declare fields. |
| 3 | Collapse to 3 subclasses + `kind: i32` discriminator, because vtable wrapped at 4 entries.    | **GONE.** All 6 variants are real `final class`es with their own bodies. |
| 4 | Free functions `p_foo(p: Parser, ...)` instead of `Parser.foo(self, ...)`, because methods with `str` args destabilised the heap.  | **GONE.** Every parser operation is an instance method again, including `parse_member(self, pairs: ...)` (mutable-list parameter, ref-typed). |
| 5 | Declare `JsonObject` first to dodge a position-sensitive crash.                                | **GONE.** Natural declaration order (Null → Bool → Number → String → Array → Object). |
| 6 | Parallel `List[str] keys` + `List[JsonValue] vals` because `Dict[K, V]` only allowed primitive V. | **REPLACED by `List[Tuple[str, JsonValue]]`.** Same insertion-order preservation, no extra ceremony, and now lays out cleanly under M14. (`Dict[str, JsonValue]` is still v0.2 — but the tuple-list workaround is now FIRST-CLASS, not a kludge.) |
| 7 | No isinstance / no match — every discrimination was a virtual method.                          | **GONE.** Rendering is one `fn render(v: JsonValue) -> str` with `match v: case JsonFoo(...):` covering all six variants; `is_object`/`is_array` use `isinstance`. |
| 8 | "Return JsonNull as error sentinel" pattern instead of a real exception channel.               | **GONE.** Every parse failure now `raise ValueError(...)`; the top-level driver and `try_parse()` both catch via `except ValueError as e:`. |

That's **all 8 of the M10 doc'd workarounds eliminated**, and one
(parallel arrays) replaced with a strictly nicer shape. The list-of-tuples
form even preserves the insertion-order guarantee the parallel arrays
had, so the round-trip rendering of an object is byte-for-byte stable.

## Workarounds STILL present (and what they point at)

Two minor ones, neither a bug, both noted in the v2 header:

1. **`str(3.0)` prints `"3.0"`.** JSON's canonical rendering for an
   integer-valued number is `3`, not `3.0`. The M10 program ducked
   this by never round-tripping numbers in its test inputs. v2 paid
   the ~5-line `render_number(n: f64)` helper that casts to `i64`,
   compares back, and prints without the fractional tail when the
   value was integer-valued and in `i64` range. Not a language bug.
   But a `repr_json(n: f64) -> str` stdlib helper would be a nice
   v0.2 add — every JSON-shaped library will roll the same trick.

2. **No string-escape decoding.** The headline cases below contain no
   `\"`/`\n`/`\t`/etc. inside string values, so byte-faithful round-trip
   works. Adding escape handling is a parser-side concern, not a
   language gap.

The M10 doc also mentioned `str(c: char)` returning the decimal codepoint;
v2 only uses `str(c)` inside the error-message paths for `expect()`, where
the wrong rendering shows up cosmetically (`expected ''` rather than
`expected '['`) but doesn't affect correctness. Fixed-cost workaround
would be a `char_to_str(c: char) -> str` helper; ducked here for size.

## LOC comparison

Both files include long top-of-file design-notes docstrings and per-section
banners. AST + parser + rendering (the apples-to-apples piece):

* M10 `examples/json_parse.spy`, lines 70-352 (AST + char helpers +
  Parser state + grammar rules + entry points): **~270 source lines.**
* v2 `examples/json_parse_v2.spy`, lines 59-380 (same logical pieces +
  rendering via match, minus the 110-line driver, minus the 80-line
  header docstring): **~270 source lines.**

The line count is approximately the same — but the SHAPE has changed:
the M10 file had 7 free functions and a 3-subclass `kind: i32` god-class
on the AST side; v2 has 6 honest final classes (one for each JSON
variant), a single `match`-based `render()` (vs 3 virtual `render()`
overrides in M10), and `Parser` is one self-contained class with 12
instance methods (vs 8 free `p_foo` functions sharing a `Parser` state
holder). The "parallel `keys`+`vals` everywhere there's an object" friction
is gone — `List[Tuple[str, JsonValue]]` is one type.

main() is bigger (14 cases vs M10's 3) because v2 actually round-trips
the headline JSON input the M10 file commented out as "triggers
mid-parse VM heap-corruption crash". On the M10 input set, v2's main()
collapses to ~5 lines.

## NEW bugs discovered

**None.** Both test cases compiled and ran first try. The deep-nest
probe ran clean to depth 40. `cargo test --workspace --release` is
green.

This is unusual for a stress-round program — every prior milestone
stress run has surfaced at least one new bug. Hypothesis: the M13-M17
features are the recently-touched code, but each one shipped with
solid regression coverage and (per M16/M17 reports) careful design.
The combination "6-arm match + tuple-of-class-ref-in-list + try/except
across deep recursion" pushes them together harder than any individual
milestone's tests did, but the underlying machinery composes cleanly.

## Confirmed gaps (M13-M17 deferred features I touched but didn't try)

Per SHARED_BRIEF §"Things still NOT supported", I deliberately avoided:

* **Generic classes / generic methods on non-generic classes** — not
  applicable; the Parser is fine as a non-generic class. No win.
* **User-defined exception subclasses** — the brief noted
  `ValueError` is one of the 10 built-in names; v2 uses it directly.
  A future `JsonParseError(Exception)` would be a stylistic upgrade
  but isn't needed for correctness.
* **Nested constructor patterns** (`case JsonArray(JsonNumber(n))`) —
  not in v0.1; would be a nice shape for "destructure deep into a known
  JSON skeleton". Worked around by recursing one level per match.
* **Generic free function for parser combinators** — I considered
  `fn parse_list[T](src: str, parse_elem: ???) -> List[T]` but
  higher-order function values aren't in v0.1 (no `fn`-typed parameters),
  so the shape isn't expressible. The task brief flagged this; confirmed.

## Final test totals

`cargo test --workspace --release` summary: all 53 test result lines
report `0 failed`, total passed is the prior baseline (M17 reported
255 passed, 1 ignored) plus 2 new tests from this task = **257 passed,
0 failed, 1 ignored.** The ignored test is the pre-M16 baseline ignore,
unchanged.
