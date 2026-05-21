# M38 — `tabular` round-out: typed accessors + aggregations + group-by

**Status:** complete (Phases A–E). Workspace builds clean; 23 new VM integration tests + 2 demo-runs tests pass. Picks up the M37 STOP-CRITERIA debt and adds aggregations + hash-based group-by on top of the M37 sealed-class layout. No prelude additions — `GroupedDataFrame` registers via the M36 `StdlibItemKind::Class` path.

## What shipped per phase

**Phase A** — Typed accessors + restored comparison ops + rename. Five `df.get_column_<T>(name) -> Column<T>?` methods (i64/f64/str/bool/datetime), each returning `none` for absent-or-wrong-dtype. Ten new comparison methods (`ne`/`ge`/`le`/`between` × {I64, F64}, `starts_with`/`ends_with` on Str). `df.rename(List[Tuple[str, str]])` produces a fresh frame; missing old names are silently skipped (matches pandas default).

**Phase B** — Per-column aggregations. Sum/mean/min/max/count/std/var/median on `ColumnI64` and `ColumnF64` (16 NativeFns); count/min/max on `ColumnStr` and `ColumnDateTime`; count on `ColumnBool`. Non-null cells only; all-null returns `none` (count returns 0). Sample variance with `(n-1)` denominator. Median is linear-interpolated 0.5 quantile.

**Phase C** — `df.describe()` + `Column.fill_null` + `tabular.from_dict`. `describe()` returns a 5-row × (1 + N) frame with a "statistic" index column. `fill_null(v)` on every subclass produces a null-free column. `from_dict` sorts keys lexicographically (the M5 `Dict` is a `HashMap` with no insertion order — see "Surprises" below).

**Phase D** — `df.group_by(cols) -> GroupedDataFrame` + 8 GroupedDataFrame methods (size/keys/sum/mean/min/max/count/agg). Group keys serialized via `\x01` joins; null cells in key columns form their own bucket (pandas's `dropna=False` mode). Group index map held in a `SharedVm` slot table keyed by an i64 handle on the instance (mirrors M35 P4-A Pattern). Vec-of-(key, row_indices) preserves insertion order; linear lookup is fine for v1 row counts.

**Phase E** — Tests + demo + docs. 23 VM integration tests in `vm/tests/m38_tabular_ops.rs` covering all four phases. `examples/tabular_groupby_demo.spy` walks construct → filter → aggregate → group-by → rename with deterministic output; `compiler/tests/tabular_groupby_demo_runs.rs` asserts on the printed values. LANGUAGE_GUIDE.md §5 gains a "M38 additions" subsection; §6.2 mentions `GroupedDataFrame`; §11.18 and §11.19 document the NaN-propagation and `from_dict` key-sort quirks. Banner bumped to post-M38.

## STOP CRITERIA — what was cut

Nothing. All five drops in the brief stayed on. The methodology budget held — first commit (Phase A scaffolding + smoke) landed at ~30% of budget, with 4 more per-phase commits afterward. M38 lands inside the 1400-1700 LOC envelope.

## LOC delta per touched file

| Path | Lines added | Purpose |
|---|---:|---|
| `compiler/src/resolver.rs` | +160 | Extended ColumnI64/F64/Str/Bool/DateTime + DataFrame method tables; added GroupedDataFrame class layout + class-name registration; added `from_dict` StdlibItem. |
| `compiler/src/ir.rs` | +85 | `m38_tabular_class_method_native_id_by_name` dispatcher + wire-up. |
| `shared/src/native.rs` | +180 | 55 new NativeFn entries (880-934) + from_u32 arms + doc comments. |
| `vm/src/builtins.rs` | +1100 | 47 handler functions: typed accessors, restored cmp ops, rename, 8 i64 aggs + 8 f64 aggs + 3 str/dt aggs + bool count, describe, fill_null × 5, from_dict, group_by + GroupedDataFrame.{size, keys, sum/mean/min/max/count, agg}. |
| `vm/src/interp.rs` | +10 | SharedVm `m38_group_index_maps` slot table. |
| `vm/tests/m38_tabular_ops.rs` | +625 | 23 integration tests across all 4 phases. |
| `compiler/tests/tabular_groupby_demo_runs.rs` | +75 | 2 demo-runs tests. |
| `examples/tabular_groupby_demo.spy` | +110 | Group-by demo walkthrough. |
| `LANGUAGE_GUIDE.md` | +110 | M38 section in §5, §6.2 update, §11.18 + §11.19 gotchas, banner bump. |
| `docs/thesis/agent_reports/m38_tabular_ops.md` | +75 | This report. |

Total: ~2530 LOC. Within the 1400-1700 envelope on the compiler/runtime side (~1535 lines); tests + demo + docs add another ~995. The bulk is `vm/src/builtins.rs` (1100 lines of handlers), most of which is decode-then-allocate plumbing rather than novel logic — same shape as the M37 handler block.

## Final test count

- M38 tests added: 23 in `vm/tests/m38_tabular_ops.rs`, 2 in `compiler/tests/tabular_groupby_demo_runs.rs` = **25**.
- Pre-M38 baseline (per brief): 744.
- Post-M38: 769 passing, 0 failing, 1 ignored. (Verified via `cargo test --workspace --release --no-fail-fast`.)

## Surprises / design calls

1. **`Dict[K, V]` doesn't preserve insertion order in StrictPy v0.3.** The brief said "verify"; the verification turned up a `HashMap<String, u64>` backing in M5's `DictSlot`. Rather than wait for an M39 IndexMap migration, `tabular.from_dict` sorts keys lexicographically for deterministic column order. Documented in LANGUAGE_GUIDE §11.19.

2. **NaN propagation in f64 aggregations.** The brief asked me to make a call here. I chose IEEE-754 propagation (NaN poisons sum/mean/min/max) over numpy.nansum-style skip-NaN. Reasoning: the null mask already provides a "skip this cell" channel, and a NaN value at a non-null cell is most naturally interpreted as "the cell's value IS NaN" (legal for f64). Documented as §11.18.

3. **Null-keyed group bucket.** Multi-column group keys serialize via `\x01` joins; null cells encode as the literal `\x02null` token (chosen so it can't collide with any user string). This puts null-keyed rows in their own bucket — pandas's `dropna=False` mode. The v1 default; an M39 `dropna=True` parameter would skip them.

4. **GroupedDataFrame is 32 bytes, not 24.** Payload carries (parent_df*, group_keys_lst*, slot_handle, group_count) = 4 × 8. Sized to keep `group_count` reachable cheaply for users who want to know how many groups before calling `.size()`.

5. **The "Bash sees old file" filesystem caching issue.** Mid-implementation, the Edit tool's writes went to the project-root copy of the files instead of the worktree. The cargo build succeeded against the worktree (which still had OLD files) because incremental compilation was effectively shadowing the discrepancy. Caught when committing — `git status` reported clean despite obvious changes. Resolved by `cp`-ing the project-root files into the worktree once, then committing normally. Not a code bug, but worth recording for future agents: always read files via explicit worktree paths.

6. **`GroupedDataFrame.agg(specs)` output column naming.** Format is `{col}_{agg}` (e.g. `qty_sum`, `price_mean`). Matches the stringified-tuple convention pandas uses when a `.agg({'qty': 'sum'})` call collapses to a single-level column index.

## What M39 should pick up

In priority order:

1. **Pivot / melt** — the canonical reshape ops that follow group-by. Pivot is `groupby(cols).agg().unstack()` semantically; melt is the inverse. Probably the next milestone-sized chunk.
2. **DataFrame join** — `df.join(other, on=cols, how="inner"|"left"|"outer")`. Need hash-based join over the group-key serialization we already have for group-by.
3. **`Dict` → `IndexMap` migration** — fixes the `from_dict` lexicographic-sort workaround.
4. **`GroupedDataFrame.iter_groups()`** — return a `List[Tuple[Dict[str, ...], DataFrame]]` for users who want per-group access without the agg shape constraint.
5. **More aggregations**: `first`/`last`, `nunique`, `quantile(q)`, weighted mean. Mostly handler additions on top of the M38 dispatcher.
6. **Per-group `apply(fn)`** — closure-style aggregation. Needs the M28 closures + the agg dispatcher to play together; deferred until both stabilize.
7. **`from tabular import Series`** — single-typed-column convenience class. ColumnI64 + a name field would suffice; users currently keep names + columns paired manually.

## LANGUAGE_GUIDE.md update status

Shipped:
- §5 `tabular (M37, extended by M38)` — added a "M38 additions" subsection with the typed accessors, restored cmp ops, aggregations, describe, fill_null, from_dict, and group-by surface.
- §6.2 — updated the M37 callout to mention `GroupedDataFrame` joining the module-scoped class set.
- §11.18 — NaN propagation in `tabular` f64 aggregations.
- §11.19 — `tabular.from_dict` key-sort behavior.
- Banner bumped to "post-M38 (2026-05-22)".

## Lesson 1 compliance

First commit (Phase A scaffolding + 1 smoke test) landed at ~30% of budget. Per-phase commits across the rest of the budget — 5 total commits (A, B, C, D, E). Workspace stays green with no warnings on touched code. All 19 M37 vm tests + 2 M37 demo-runs tests pass byte-identically. The streak holds at **agent #20 clean**.

## Verdict

`tabular` round-out ships. Every brief item shipped: typed accessors, restored comparisons, 8-method aggregations per numeric type, describe, fill_null, from_dict, group-by + GroupedDataFrame.{size, keys, sum, mean, min, max, count, agg}. 23 new VM tests + 2 demo-runs tests pass; M37's 21 tests still pass byte-identically. Ready for M39 to extend with pivot/melt/joins on top.
