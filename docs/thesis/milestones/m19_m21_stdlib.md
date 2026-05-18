# M19–M21 — Phase 1 stdlib sprint

**Date**: 2026-05-19
**Wall-clock**: one orchestrator session; agent compute ~6 hours total
across 6 sequential agents.
**Headline**: language goes from "every native is a bare-name prelude
entry" to "import json; json.parse(s)" — a real Python-shaped stdlib.

## Sequencing rationale

All milestones touched `compiler/src/resolver.rs::seed_stdlib_modules`,
`shared/src/native.rs` (NativeFn enum + `from_u32`), and
`vm/src/builtins.rs` (dispatch). Sequential agents avoided integration
conflicts. NativeFn IDs assigned in disjoint ranges per milestone:
- M19: 130-133 (sys)
- M20a: 140-174 (os/path/io, 23 ids)
- M20b: 175-212 (time/random/math, 31 ids)
- M20c: 213-249 (json/re, 12 ids — finishing the M19-M20 sprint range)

## What shipped per milestone

| M | Modules | Tests | New NativeFns | Notable |
|---|---|---|---|---|
| M19 | sys | 267→285 | 4 | Import machinery; non-catchable VmError::Exit; argv plumbed end-to-end. |
| M20a | os, path, io | 285→314 | 23 | First native-returned tuple (`path.splitext`). Found BUG-037 incidentally. |
| M20b | time, random, math | 314→348 | 31 | Hand-rolled `civil_from_days` to skip `chrono` dep. Numerical Recipes LCG for random. |
| M20c | json, re | 348→370 | 12 | `serde_json` + `regex` deps added to vm/Cargo.toml. `re.match` renamed `re.fullmatch` (match is a keyword). |
| M21 | (cleanup) | 370→379 | 0 | Fixed BUG-037 via M13 short-circuit pattern. minigrep.spy integration example. |

## The integration test (M21 minigrep.spy)

A real CLI tool, ~110 LOC, exercising 5 modules together:

```python
import sys, os, io, re, time

fn die(msg: str) -> None:
    io.write_stderr("minigrep: " + msg + "\n")
    sys.exit(2)

fn main() -> i32:
    argv: List[str] = sys.argv
    pattern: str = argv[1]
    if not re.is_valid(pattern): die("bad regex: " + pattern)
    start: f64 = time.monotonic()
    lines: List[str] = []
    if len(argv) >= 3:
        try:
            lines = read_file_lines(argv[2])
        except IOError as e:
            die(e.message)
    else:
        lines = read_stdin_lines()
    matches: i32 = 0
    for line in lines:
        if re.search(pattern, line):
            io.write_stdout(line + "\n")
            matches = matches + 1
    elapsed: f64 = (time.monotonic() - start) * 1000.0
    io.write_stderr("[" + str(matches) + "/" + str(len(lines)) +
                    " matched in " + str(i64(elapsed)) + "ms]\n")
    return 0
```

5 integration tests cover pattern match on a file, missing-file
recovery via IOError, bad-pattern ValueError, usage on no args, line
counting. All pass.

This program would have required (at minimum) BUG-025 fixed, generics
gone, `with`/try interaction worked around, AND a 200+-line manual
implementation of every stdlib function used. With the Phase 1 stdlib
it's a one-screen script.

## The recurring placeholder-lowering pattern

M20a's BUG-037 was the third instance of the same shape:

| Bug | Operator | Placeholder lowering | Found in |
|---|---|---|---|
| BUG-008 | `is not` | emitted `RefEq` (no `BoolNot`) | M10 json_parse stress |
| BUG-034 | `str !=` | fell through to `INe` (pointer compare) | M12 btree stress |
| BUG-037 | `??` (null-coalesce) | emitted `Copy(rhs)` only | M20a os/path/io agent (incidental) |

All three: the parser accepted the operator, the typechecker accepted
it, the lowering shipped as a placeholder that returned the wrong
value, and no regression test had exercised the non-trivial path until
a stress program organically used it.

Mechanical lesson for the thesis methodology chapter: audit
`compiler/src/ir.rs` for `// placeholder` comments and operators whose
lowering is just `Copy(some_operand)`. Either fix them or add
explicit `unimplemented!` so they fail loudly.

## Out of scope for v0.2 (deferred to v0.3+)

- User-defined `.spy` modules (`import myproject.utils`)
- Submodules (`import os.path`)
- Star imports (`from sys import *`)
- Stdlib classes (typed `JsonValue` surface, `re.Pattern` cached patterns)
- Generic stdlib functions (`random.choice[T]` integration with M17 worklist)
- `with open(...) as f:` routing through try/except (known M15 follow-up)
- `match` as a contextual keyword (so `re.match` could exist alongside `match case`)
- `sys.stdin/stdout/stderr` as File handles (M5 File wants `open()`)

## Promotable from out-of-scope to spec (zero code change)

- `raise e` re-raise from a caught variable works in v0.1 (§7.5.6
  lists as out of scope) — found by M18 R3 probe.
- match-scrutinee-throws propagates correctly past unentered arms —
  M18 R3 probe.

## Next-step menu (post-M21)

After M21 only BUG-028 (lexer line-continuation across `+`) remains
deferred. Phase 1 stdlib is complete. Remaining tracks:

- **G: Draft the thesis.** Archive is fully built out (M0-M21 covered;
  26 agent reports, 33-entry bug catalog, stats CSV with bench
  history, two milestone-cluster notes).
- **F: Spec catch-up / restructure.** STRICTPY_SPEC.md is now honest
  about M19-M21 surface but internally inconsistent at the section
  level. Renumber and re-flow.
- **J: Migrate existing examples to the M13-M17 surface + stdlib.**
  lisp.spy, calculator.spy, lambda_calc.spy still carry M10-era
  workarounds AND don't import any of the new stdlib.
- **Phase 2 stdlib (per "what would make sense" list):** csv, base64,
  hashlib, argparse, collections, itertools, statistics, logging,
  struct, urllib.parse. ~2 weeks at the M19-M20 cadence.
- **M, N, O — v0.2 language features**: user-defined exception subclasses,
  generic classes, bounded generics.
- **L: Round 5 stress tests** on the M19-M21 stdlib surface.
- **Q: Fix BUG-028** (line continuation). Last open bug.

The language is now demonstrably usable for CLI tools and data
processing. Phase 2 stdlib would extend the reach to data formats
(csv, struct), CLI ergonomics (argparse), and hashing/encoding.
