# M36 — `StdlibItemKind::Class` refactor

## Context

StrictPy v0.3 added 11 stdlib classes (M34 + M35) and currently registers them all in the **prelude** (alongside truly-built-in classes like Channel, Thread, io.File, Dict, Set, List, plus exception names). The legacy "prelude wins" branch in the import resolver makes `from json import JsonValue` work transparently because the name is already bound when the import runs.

This was a deliberate scope-down — M34 + M35 agents shipped the user-visible features while leaving the registration infrastructure question for a later focused agent. That agent is you.

**The prelude is now crowded** (17 stdlib classes, well past the original ~3 prelude classes the legacy code was designed for). HANDOFF.md flags this as "urgent before M40". This refactor unblocks v0.4 stdlib classes (which a future Pandas-shaped library would add 20+ more of).

**Pure refactor — no API change.** Every existing `.spy` program must continue to work byte-identically. Every existing test must continue to pass. The user-visible surface is unchanged.

## Files to read FIRST (in order)

1. `STRICTPY_SPEC.md` §10.2 (import semantics) — the prelude vs module-import surface
2. `compiler/src/resolver.rs` lines 188-212 (the `StdlibItem` / `StdlibModule` / `StdlibItemKind` definitions)
3. `compiler/src/resolver.rs::seed_prelude` (lines 4094-4779) — focus on the 11 classes at lines 4329-4774
4. `compiler/src/resolver.rs::seed_stdlib_modules` (starts line 388) — the existing function/const registration pattern
5. `compiler/src/resolver.rs` lines 4785-4879 — the import-resolution "prelude wins" branch
6. `compiler/src/ir.rs::lower_method_call` around line 4180 — the M34/M35 class-method dispatch path (read enough to confirm it's name-based and won't need changes)

## What's currently in place

**Classes to relocate (11 total)**:
- M34 JsonValue family (7 classes): `JsonValue`, `JNull`, `JBool`, `JInt`, `JFloat`, `JString`, `JList`, `JObject` → into the `json` stdlib module
- M35 P4-A: `Pattern` → into the `re` stdlib module
- M35 P4-B: `Connection`, `Cursor` → into the `sqlite3` stdlib module
- M35 P4-C: `Hasher` → into the `hashlib` stdlib module

**Classes that STAY in the prelude (do not touch)**:
- `Channel[T]`, `Thread`, `io.File`, `Dict[K,V]`, `Set[T]`, `List[T]` — these are truly built-in primitives, not stdlib classes
- 10 exception names (BaseException, Exception, ValueError, etc.) — built-in
- `True` / `False` — primitive constants

**Dispatch (do not change)**: `compiler/src/ir.rs` has three class-method dispatchers (`m34_json_class_method_native_id_by_name`, `m35_p4b_sqlite_class_method_native_id_by_name`, `m35_re_pattern_method_native_id_by_name`). All three are pure name-based lookups — they do NOT care whether the class came from the prelude or a module. They should not need any changes. Verify but don't modify.

## The task

### Phase A — extend `StdlibItemKind` (small, ~30 LOC)

1. Add `StdlibItemKind::Class` variant.
2. `StdlibItem` needs to carry enough metadata to install a class binding when imported. Two reasonable shapes:
   - **Recommended**: add `class_id: Option<ClassId>` field to `StdlibItem`. `None` for Const/Function, `Some(cid)` for Class. The ClassId is filled in by the same registration code that previously did this in `seed_prelude` (creating the ClassLayout, fresh_class(), etc.).
   - Alternative: make `StdlibItemKind` itself an enum-with-payload (`Class { class_id: ClassId }`). Pick whichever fits the existing code style better.

### Phase B — relocate the 11 class registrations (~300-400 LOC moved)

Move each block from `seed_prelude` into the corresponding stdlib module's seed function (extend `seed_stdlib_modules`, or factor out helpers `seed_json_classes` / `seed_re_classes` / `seed_sqlite3_classes` / `seed_hashlib_classes`). Keep the ClassLayout shape byte-identical — same field names, same method signatures, same `is_native: true`, same `payload_size`.

Per-class registration shape (inferred from the survey):
```rust
// inside seed_json_module (or similar):
let pattern_cid = self.fresh_class();
// ... fresh_class / make_symbol(SymbolKind::Class) / class_name_to_id.insert ...
self.class_layouts.insert(pattern_cid, ClassLayout { ... });

// then add to module items:
items.push(StdlibItem {
    name: "Pattern".into(),
    kind: StdlibItemKind::Class,
    ty: Ty::Class(pattern_cid),
    native_id: 0,  // unused for classes; or use a sentinel
    class_id: Some(pattern_cid),
});
```

**Order matters**: `seed_stdlib_modules` currently runs BEFORE `seed_prelude` (or AFTER — verify from the resolver init order). The new class registration must happen at the point where `class_name_to_id` is still mutable. Use whatever ordering keeps the resolver init working.

### Phase C — update the import resolver (~30-80 LOC)

The existing `from json import JsonValue` path (resolver.rs lines 4785-4879) currently:
1. Looks up the item in the module → not found (because JsonValue isn't in the json module today)
2. Falls through to "prelude wins" → succeeds because JsonValue is in the prelude

After this refactor:
1. Look up the item in the module → found, kind=Class
2. Bind the local name to the class symbol (use `make_symbol(scope, local_name, SymbolKind::Class, Span::DUMMY, Some(Ty::Class(class_id)))` — same shape as today's prelude binding, just scoped to the import site).

Also handle the `import json` whole-module case: when the user writes `import json` then `json.JsonValue`, the class needs to be accessible as a module attribute. Check how `sys.argv` and `sys.exit` work today and mirror that for classes.

### Phase D — keep the "prelude wins" legacy branch

The legacy branch is there for cases where neither the module nor the new code finds an item. Don't delete it; just make sure the new code path is reached first for the 11 relocated classes. Add a comment marking which classes the legacy branch is no longer needed for, in case a future agent decides to delete the dead code.

### Phase E — LANGUAGE_GUIDE.md update

`§6.2 Prelude classes` currently lists Pattern / Connection / Cursor / Hasher / JsonValue + 6 subclasses as prelude classes. After the refactor, they are NOT in the prelude — they're module-imported. Update the table to:
- Remove the 11 stdlib-class rows
- Add a note above the §5 stdlib reference that "Stdlib classes (JsonValue / Pattern / Connection / Cursor / Hasher / …) are module-scoped — import them via `from json import JsonValue` etc."

Also update §3.12 Imports to reflect that stdlib classes are now normal module items (not legacy "prelude wins"). And §4.3 narrative about "11 stdlib classes registered in the prelude" → "11 stdlib classes available via stdlib imports".

Bump the version banner at the top: "Last refresh: post-M36 (2026-05-21)".

## Acceptance criteria

1. **`cargo build --workspace --release`** clean (no warnings on touched code).
2. **All M34 + M35 tests still pass byte-identically**: 11 + 10 + 10 + 8 = 39 integration tests in `vm/tests/m34_json_value.rs`, `m35_re_pattern.rs`, `m35_sqlite_class.rs`, `m35_hashlib_streaming.rs`.
3. **Full workspace test sweep**: `cargo test --workspace --release --no-fail-fast` passes 723 / 0 / 1 (same as M35). No regressions.
4. **Existing `.spy` examples still work**: `examples/json_typed_demo.spy`, `examples/re_pattern_demo.spy`, `examples/sqlite_class_demo.spy`, `examples/hashlib_streaming_demo.spy` all run to completion (compiler/tests/*_demo_runs.rs already cover this).
5. **The `class_name_to_id` HashMap** continues to hold all 11 class IDs after `seed_stdlib_modules` + `seed_prelude` both run, so dispatch in ir.rs continues to find them by name.
6. **No new examples break**: `cargo test --release -p strictpy-compiler` (all the `*_demo_runs.rs` tests) green.

## Constraints — files NOT to modify

- `vm/src/builtins.rs` (handler bodies are correct; they take a receiver pointer and don't care about registration source)
- `shared/src/native.rs` (NativeFn IDs are stable)
- `vm/src/interp.rs` for class dispatch (unchanged)
- The 39 M34/M35 test files (if a test stops passing, the refactor is wrong — don't change the test)
- Any `examples/*.spy` file (zero user-visible API change)

## STOP CRITERIA — when to ship a smaller working version

1. **Phase A + B done but Phase C blocks**: you've added `StdlibItemKind::Class`, moved the registrations, but the import resolver doesn't find them via the new path. **STOP. Leave the legacy "prelude wins" branch active**, commit Phase A + B alone, and report that Phase C needs design discussion. The benefit of A + B alone is structural — class metadata lives where it belongs — even if resolution still uses the legacy path.

2. **Phase C done but some test breaks**: do NOT modify the test to make it pass. If a test that previously passed now fails, the refactor introduced a behavior change. **STOP. Revert the offending sub-change**, commit what's clean, and report the breakage with the test name + failure mode.

3. **You hit 80% of budget before Phase E (docs)**: ship the code changes, commit, and report "LANGUAGE_GUIDE.md update pending" — the orchestrator can finish the docs in 5 minutes.

## Methodology discipline (the project's Lesson 1 + Lesson 2)

**Lesson 1: FIRST commit before 60% of budget.** Get to a green-build, tests-passing checkpoint and commit early. Even if it's just Phase A. **17 consecutive clean agents M28→M35** have followed this; don't break the streak.

**Lesson 2: Distinctive variable prefix.** This is a refactor, not a parallel-agent round, so collision risk is low. Still, use prefix `m36_` for any new helper functions / local variables you introduce in shared files (resolver.rs, ir.rs).

**Lesson 3: Commit per phase.** Phase A → commit. Phase B → commit. Phase C → commit. Phase D + E → commit. Four commits is fine; the orchestrator squashes if desired.

## Reporting requirements

After the work, write a brief report at `docs/thesis/agent_reports/m36_stdlib_class_refactor.md` (verbatim or condensed) with:
- What you implemented per phase (~150 words)
- Any surprises / design calls (e.g., did you go with `class_id: Option<ClassId>` field or enum-with-payload?)
- Final test count + verification that the 39 M34/M35 tests pass unchanged
- LOC delta (rough: +inserted, -deleted in resolver.rs)
- Whether you completed Phase E (LANGUAGE_GUIDE.md update) or punted it

Total report length: under 400 words.

## Commit message shape

```
M36: StdlibItemKind::Class refactor — move 11 stdlib classes from prelude to module scope

Pure infrastructure refactor. The 7 JsonValue classes (M34), Pattern (M35 P4-A),
Connection + Cursor (M35 P4-B), and Hasher (M35 P4-C) move from `seed_prelude`
into their respective stdlib modules' item lists, behind a new
`StdlibItemKind::Class` variant.

Public API unchanged. All 39 M34/M35 integration tests pass unchanged; full
workspace test sweep at 723 / 0 / 1, identical to M35.

Closes the M34/M35 scope-down debt. Unblocks v0.4 stdlib classes (next prelude
slot pressure check happens at +10 classes rather than the current +0 margin).

The 6 base prelude classes (Channel, Thread, io.File, Dict, Set, List) and 10
exception names stay in the prelude — they are truly built-in primitives.
```
