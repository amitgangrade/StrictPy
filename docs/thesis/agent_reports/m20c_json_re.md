# M20c — `json` / `re` stdlib modules

**Brief**: Close out the M20 batch by shipping the last two stdlib
modules — JSON validate/reserialize (`json`) and regex matching (`re`)
— on top of the M19 stdlib-module-table infrastructure.  The point
isn't to add capabilities StrictPy didn't already have (M18's
`json_parse_v2.spy` and M12's `regex.spy` are full-featured pure-StrictPy
implementations); it's to package both as ergonomic
`import json; json.parse_to_string(s)` / `import re; re.search(p, s)`
surfaces so user programs don't have to rewrite the parsers.

**Wall-clock**: ~2 hours (read-through + two module registrations,
~150 LOC of native handlers, 16 in-process tests + 4 subprocess tests +
2 example programs + spec update).
**Files changed**: 5 source files + 2 Cargo.toml deps + 1 spec section
+ 1 agent report + 2 new examples + 3 new test files.
**Tests**: 348 baseline (M20b) + **20 new** (16 in-process + 4
subprocess) + 2 incidental sweeps over the new examples (compile /
parse / typecheck sweeps auto-pick up `examples/*.spy`) = **370
passing, 0 failing, 1 ignored**.

## Strategy: A (native re-implementation in Rust)

The brief offered Strategy A (native re-impl) vs Strategy B (embed
the .spy source as bundled bytecode and dispatch through a new loader).
I took **Strategy A**, matching every prior M20 sub-milestone.
Strategy B would have required new bytecode-loading machinery (where
do the bundled `.spyc` chunks live, how do they namespace their
symbols, how do their fns appear as NativeFn-dispatchable entities)
that's genuinely v0.3 work — interesting research thesis material but
overkill for two ten-LOC-of-glue modules.

Crate choices:
* **`serde_json` 1.0** for `json`.  Not previously a workspace dep —
  added to `vm/Cargo.toml` only (the VM is the only crate that runs
  user code, so a runtime dep there doesn't leak into the compiler or
  the shared metadata crate).  ~150KB of binary footprint for full
  RFC 8259 parsing + serialization is well worth not hand-rolling.
* **`regex` 1.0** for `re`.  Same story.  Linear-time NFA matcher with
  Python-`re`-compatible syntax for ~90% of patterns user code writes.

The diff for each module is **one `StdlibModule` registration in
`resolver.rs`** + **one block of `NativeFn` match arms in
`builtins.rs`**.  Zero changes to typecheck or IR.  The M19 design
keeps paying out.

## Module 1: `json` — the typed-vs-validation design choice

The brief flagged this explicitly: a typed `JsonValue` surface (the
M18 sealed-class approach) requires either (a) exposing
`JsonNull`/`JsonBool`/`JsonNumber`/`JsonString`/`JsonArray`/`JsonObject`
as built-in classes registered in the resolver — sibling to `File`,
`Channel`, etc. — or (b) bundling the M18 `json_parse_v2.spy` source
as compiled-stdlib bytecode (Strategy B from above).

**I shipped the validation-only path.**  Public API:

| Symbol | Behaviour |
|---|---|
| `json.parse_to_string(s)` | parse + canonical compact reserialize |
| `json.minify(s)` | alias of the above |
| `json.is_valid(s)` | bool predicate, never raises |
| `json.pretty(s, indent)` | parse + indented pretty-print |
| `json.escape(s)` | render `s` as a JSON string literal |

Rationale:

1. The typed-tree path is meaningful but its main consumer (`match v:
   case JsonObject(pairs): ...`) is already first-class in the
   language via `examples/json_parse_v2.spy`.  Users who want
   structured access keep using that.
2. The validate/reserialize subset covers every config-file use case
   (`is this JSON valid?`, `let me normalize this`, `print it back
   pretty`) and serializes/escapes for hand-built output.
3. The class-registration path is open-ended v0.3 work.  Built-in
   classes today live in a separate machine (the prelude registers
   `File`, `Channel`, `Atomic` as class symbols); registering classes
   *inside* stdlib modules and exposing their constructors / fields
   through the module-attr lookup would need typecheck + IR work that
   the brief's stop-criteria explicitly says to defer.

The brief said "pick whichever ships faster" — this was an easy call.

`json.pretty(s, indent)` is hand-rolled (30 lines of recursive
formatting over `serde_json::Value`) rather than `serde_json`'s
`PrettyFormatter::with_indent`.  The latter requires the
`serde::Serialize` trait in scope, which would pull `serde` in as a
direct workspace dep (it's only transitive today).  The hand-rolled
walker is byte-compatible with serde_json's pretty output for the
cases user programs hit.

## Module 2: `re` — the `match` keyword collision

Public API:

| Symbol | Behaviour |
|---|---|
| `re.fullmatch(pat, s)` | true iff pat matches the entire string |
| `re.search(pat, s)` | true iff pat matches anywhere |
| `re.find(pat, s)` | `(i32, i32)` start/end of first match, or `(-1, -1)` |
| `re.find_all(pat, s)` | `List[str]` of all non-overlapping matches |
| `re.replace(pat, repl, s)` | substitute every match |
| `re.split(pat, s)` | split on matches |
| `re.is_valid(pat)` | bool predicate over pattern compilation |

The interesting wrinkle: **`re.match` doesn't parse.**  StrictPy's
lexer treats `match` as a hard keyword (M16 introduced
`match/case`).  When the parser sees `re.match`, it lexes as `Ident("re")
DOT KwMatch` — and the parser's attribute path wants an `Ident` after
the dot, not a keyword.  Three options:

1. **Patch the lexer/parser** to allow `match` as an attribute name
   (contextual-keyword treatment, the way Python does it).  This is
   the principled fix and would also benefit any future user who
   wants `class Foo: fn match(self, x): ...`.  Out of scope for M20c.
2. **Rename to `fullmatch`.**  Already the more accurate name for the
   anchored-at-both-ends semantic the brief asks for (Python's
   `re.match` only anchors at the start — `re.fullmatch` anchors both
   ends).
3. **Use `match_`** or another suffix.  Ugly.

I picked option 2.  Documented in spec §9.14 with a note pointing at
the lexer collision so the v0.3 contextual-keyword work knows what
needs unblocking.

`re.replace(pattern, replacement, s)` argument order matches Python's
`re.sub(pattern, repl, string)`.  The brief's example
`re.replace("[0-9]", "X", "a1b2c3") -> "aXbXcX"` only makes sense
under this order (the haystack is the third argument); my first cut
had `(pattern, s, replacement)` and the test promptly caught it.

`re.find` returns a `(i32, i32)` tuple, packing two i32 starts/ends
into u64 slots via `Interpreter::alloc_tuple_obj` — the helper added
in M20a for `path.splitext`.  The slots are opaque u64 to the GC and
to the `Load(offset)` IR primitive, so packing primitives there
"just works" without runtime-type-table cooperation.

## Files modified + LOC (approximate)

| File | Lines added | Purpose |
|---|---|---|
| `shared/src/native.rs` | +50 | 12 new `NativeFn` variants (213–217 json, 220–226 re) + `from_u32` arms |
| `compiler/src/resolver.rs` | +115 | Two `StdlibModule` registrations |
| `vm/src/builtins.rs` | +180 | New handlers + `compile_regex` helper + `write_pretty` JSON formatter |
| `vm/Cargo.toml` | +3 | `serde_json` + `regex` runtime deps |
| `STRICTPY_SPEC.md` | +110 | §9.13 (json) + §9.14 (re) |

Plus tests + examples:

* `examples/json_demo.spy` — 8 round-trip scenarios + try/except + nested round-trip.
* `examples/regex_demo.spy` — 10 scenarios covering fullmatch, search, find, find_all, replace, split, is_valid, bad-pattern handling.
* `vm/tests/m20c_json_re.rs` — 16 in-process tests.
* `compiler/tests/{json_demo,regex_demo}_runs.rs` — 4 subprocess tests via `spy.exe`.

## Hardest three things (in retrospect)

1. **The `re.match` keyword collision.**  Took 5 minutes of confused
   debugging on `error[E0001]: expected identifier, found KwMatch` —
   the lexer is doing exactly the right thing for `match/case`
   support but it forbids any other usage.  Renaming to `fullmatch`
   was the right call; the contextual-keyword fix in the
   lexer/parser is v0.3 work.

2. **The `re.replace` argument order.**  The brief's signature
   `re.replace(pattern, s, replacement)` and its example
   `re.replace("[0-9]", "X", "a1b2c3") -> "aXbXcX"` are internally
   inconsistent (the example call would set `s="X"` and
   `replacement="a1b2c3"`, producing "X").  Reading the
   example backwards revealed the intent — Python's `re.sub` order.
   Worth flagging because future users reading the brief will hit
   the same paradox.

3. **`json.pretty`'s indent parameter.**  serde_json's
   `Serializer::with_formatter(PrettyFormatter::with_indent(b"  "))`
   path needs `serde::Serialize` in scope to call `Value::serialize`,
   which would pull `serde` in as a direct workspace dep (it's
   transitive today, through `serde_json`).  Hand-rolling a 30-line
   recursive printer was simpler than fighting the trait-in-scope
   problem.  The output is byte-identical to serde_json's for the
   cases user programs hit.

## Incidentally-discovered bugs / oddities

* **None requiring code changes.**  M19+M20a+M20b infrastructure
  absorbed two more modules without complaint.  This is the first
  M20-batch sub-milestone with no incidental-bug discovery (M19 had
  the legacy `io.File` shadowing, M20a had `??` null-coalesce
  always-fallback, M20b found the missing bare-name `sqrt` prelude
  registration).  The trend of one bug per round is broken — which I
  read as a maturity signal more than a coverage gap, because the
  test surface here is large (20 new tests across two crates +
  example-sweep auto-coverage).
* The brief's own example for `re.replace` is internally inconsistent
  (see hardest-three above).  Not a bug, but worth flagging for the
  orchestrator's catalogue.

## What's next

M20 closes here.  v0.3 work the M19→M20c arc highlights:

* **User-defined modules and submodules** (`os.path` instead of the
  top-level `path`, user `.spy` files importable by name).  The M19
  `StdlibModuleTable` is one swap away from a generic
  `HashMap<ModulePath, _>` — the loader and on-disk packaging are the
  real work.
* **Strategy B for stdlib in StrictPy itself.**  Bundle the existing
  `json_parse_v2.spy` / `regex.spy` examples *as* the stdlib, dispatch
  imports against an embedded bytecode index, expose their classes
  through normal class resolution.  This is the self-hosting story —
  the StrictPy stdlib written in StrictPy.  Genuinely thesis-relevant
  research material.
* **Stdlib-class registration**.  The minimum new piece: let a
  `StdlibModule` carry a `Vec<StdlibClass>` alongside its
  `Vec<StdlibItem>`, so `json.JsonNumber` etc. resolve through the
  same module-attr lookup that `json.parse_to_string` uses today.
  This unblocks the typed-`JsonValue` surface and the `re.Pattern`
  cached-compilation handle.
* **Contextual keywords for `match`.**  Lets `re.match(...)` parse
  alongside `match v: case ...`.  Small lexer/parser change; would
  also rehabilitate any future `instance.match(x)` user-code use.
