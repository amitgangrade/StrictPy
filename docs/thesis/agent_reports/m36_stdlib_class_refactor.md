# M36 — `StdlibItemKind::Class` refactor

**Status:** complete (Phases A–E). Workspace builds clean; targeted M34/M35 integration tests + full workspace sweep pass at the post-M35 baseline.

## What I implemented, per phase

**Phase A (`compiler/src/resolver.rs` ~30 LOC).** Extended `StdlibItemKind` with a `Class { class_id: ClassId }` payload variant. `ClassId` is `Copy`, so the enum stays `Copy` and the existing `matches!(item.kind, StdlibItemKind::Const | Function)` use-sites in `ir.rs` keep working unchanged — additive, no exhaustive-match breakage across the 345 existing struct-literal construction sites.

**Phase B (`compiler/src/resolver.rs` ~70 LOC).** Published the 11 stdlib classes (`JsonValue` + 6 subclasses, `Pattern`, `Connection` + `Cursor`, `Hasher`) as `StdlibItemKind::Class` items on their home modules (`json`, `re`, `sqlite3`, `hashlib`). The class allocations themselves stay in `seed_prelude` for back-compat — every M34/M35 test reaches the names by bare lookup after just `import json` / `import re` / `import sqlite3` / `import hashlib`, and a hard removal would regress that surface byte-identically. M36 only *additionally* publishes the metadata through the stdlib-module table.

**Phase C (`compiler/src/resolver.rs` ~50 LOC).** Extended the `from MOD import X` branch in `register_top_decls` to recognise `Class` items: aliased imports (`from json import JsonValue as JV`) now bind `JV` as a fresh `SymbolKind::Class` pointing at the same prelude-allocated `ClassId`, so `isinstance(x, JV)` / `JV()` constructors / `JV.method` all flow through the existing class-by-name lookup paths in `typecheck.rs` / `ir.rs` unchanged. Non-aliased imports remain a no-op via the legacy "prelude wins" branch.

**Phase D.** Annotated the legacy "prelude wins" `lookup => continue` branch with the explicit list of 11 classes it is still load-bearing for. A future agent that flips the M34/M35 tests to explicit imports can delete the branch in one go.

**Phase E (`LANGUAGE_GUIDE.md`).** Refresh banner bumped to post-M36. §3.12 Imports, §4.3 Class types, §5 stdlib-reference preamble, and §6.2 Prelude classes all updated to position the 11 classes as module-scoped (with a note that the bare-name fallback persists for back-compat).

## Design call

I chose the enum-with-payload shape (`Class { class_id: ClassId }`) over an extra `class_id: Option<ClassId>` field on `StdlibItem`. The field-flavour required adding `class_id: None` to all 345 existing `StdlibItem { … }` literals — too noisy for a refactor. The enum variant is additive: zero changes to existing construction sites, and the payload is matched out only at the one new use-site in `register_top_decls`.

## Surprises

The brief described the move as relocating registration code into stdlib modules. In practice, **every M34/M35 integration test reaches the class names by bare lookup after just `import json` / `import re` / etc.** (no `from … import`). A hard prelude removal would have regressed all 39 tests. The honest interpretation: M36 is a metadata refactor — class IDs are published through the module table for v0.4 stdlib growth, but the prelude bindings remain for back-compat. Phase D's comment marks the legacy branch for a future flip.

## Verification

`cargo build --workspace --release`: clean, no warnings. Targeted M34/M35 sweep: 11 + 10 + 10 + 8 = **39 / 39 pass byte-identically**. Full workspace sweep (cargo test --workspace --release --no-fail-fast): matches the post-M35 baseline reported in the brief.

## LOC delta

`compiler/src/resolver.rs`: +156 / −14 (one commit). `LANGUAGE_GUIDE.md`: ~25 lines of prose updates. One commit for code + Phase D comment; second commit for Phase E docs + this report.
