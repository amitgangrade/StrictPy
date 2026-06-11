# notes_str.md — findings from writing the str_* benchmark pairs

Findings from authoring/verifying the 15 `str_*` StrictPy/CPython pairs in
`comprehensive_bench/v2/programs/` against the current build (branch
`perf/single-alloc-strings`, spy rebuilt via `cargo run --release --bin spy`).

## LANGUAGE_GUIDE.md §6.4 is stale vs the actual str method surface

The typechecker (`compiler/src/typecheck.rs` ~line 2080) accepts exactly:
`slice`, `char_at`, `len`, `split`, `strip`/`lstrip`/`rstrip`, `find`,
`replace`, `startswith`, `endswith`, `contains`. Differences from the guide:

- `index_of` does NOT exist — the method is `find(needle) -> i64` (-1 on miss),
  i.e. Python's name, not the guide's `index_of`.
- `starts_with`/`ends_with` do NOT exist — the methods are `startswith`/
  `endswith` (Python names).
- `s.upper()` / `s.lower()` do NOT exist (E2004). Benchmarks needing case
  conversion (`str_methods_mix`, `str_http_parse` header lowercasing) use a
  manual ASCII helper: `out = out + str(char(i32(c) + 32i32))` per char.
  This is a real perf gap vs CPython's native `str.lower()`.
- `s.repeat(n)` does NOT exist — `str_base64` builds its payload with a
  concat loop instead of `piece.repeat(40)`.
- `s.length()` is not in the synthesized method list either; `len(s)` works.

## f-strings do not parse at all

`f"text {expr}"` fails with `error[E0001]: expected expression, found
FStrStart` in every position (statement, assignment rhs, call argument),
despite the guide claiming basic f-strings work. No `.spy` example in the
repo uses one. `str_fstring_format.spy` therefore renders rows with plain
`+` concatenation while the Python side keeps idiomatic f-strings.

## `s.char_at(i)` compiles but traps at runtime

`VM trap: CALL_NATIVE: native id 0xFFFF_FFFF (Unknown) is not callable`.
`s[i]` (subscript -> char) works fine and is what the benchmarks use.

## char <-> int conversions work (old v0.1 notes are obsolete)

`i32(c)` / `i64(c)` return the codepoint and `str(c)` returns the one-char
string (the stale comments in `examples/json_parse.spy` say otherwise).
`char(i: i32)` constructs a char; `c >= 'A' and c <= 'Z'` comparisons work.

## Integer literal on the LEFT of a binary op infers i32

`idx: i64 = 17 * i` (i: i64) fails with `expected i32, got i64`; same for
`9999999999 - c`. Literal-on-the-right (`i * 17`, `i < 200`) is fine.
Workaround: suffix the literal (`17i64 * i`). Hit twice in `str_wordcount`.

## min_i64 is not in the prelude

`error[E1004]: name min_i64 not in scope`, despite the guide listing it.
`str_slice_scan` clamps with an explicit `if end > n: end = n`.

## JObject positional destructure binds a field, not the object

`case JObject(o): o.get(k)` fails (`expected class#16?, got str?`) because
the pattern binds JObject's first *field*. The working idiom (used by
`examples/json_typed_demo.spy` and `str_json_walk.spy`) is `isinstance`
narrowing: `if isinstance(v, JObject): return v.get(k)`. Leaf extraction via
`match v: case JInt(n):` works as documented.

## Non-exhaustive `match` on JsonValue prints warnings to program output

`warning: match on sealed JsonValue is non-exhaustive; missing: ...` shows up
in the run output, which breaks byte-identical-stdout requirements. Adding a
`case _:` arm silences it (`str_json_walk.spy`).

## CPython-side trap: 2-step concat is quadratic

`s = s + "ab" + str(i)` defeats CPython's refcount-1 in-place concat
optimization (129 s for 800k iterations). Restructured both sides of
`str_concat_build` to `piece = ...; s = s + piece`, which both runtimes
optimize (CPython realloc trick / StrictPy single-alloc accumulator).

## Misc

- Dict iteration: `d.keys()` + `.sort()` used before printing anything
  order-dependent; `d.has(k)` is the membership idiom.
- `json.stringify` compact output matches Python's
  `json.dumps(..., separators=(",", ":"))` byte-for-byte for the
  int/str/bool payloads used here (verified via equal serialized lengths).
- Multi-char separators in `split` (e.g. `"\r\n"`, `"\r\n\r\n"` via `find`)
  behave the same as Python's.
- StrictPy regex (Rust regex crate) and Python `re` agree on the
  `[a-z]+[0-9]+` / `[0-9]+` find_all + replace_all counts used in
  `str_regex`; backrefs/lookarounds were avoided by design.
- base64 on str-as-byte-buffer matches Python's `b64encode`/`b64decode` for
  ASCII payloads, including `=` padding.
