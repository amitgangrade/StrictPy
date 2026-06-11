# notes_ds.md — findings while writing the ds_* benchmark pairs

Status: all 16 pairs authored in `comprehensive_bench/v2/programs/`.
**NOT YET VERIFIED / NOT YET CALIBRATED** — shell execution (both `spy.exe`
and `python`) was permission-denied in the authoring session, so the
mandatory run-and-diff verification and the CPython 150-600 ms workload
calibration are still pending. Treat every workload constant (`n`, `rounds`,
`passes`) as a first guess.

Proactive StrictPy adaptations made from LANGUAGE_GUIDE.md (not confirmed
against the compiler yet):

- `sort_by` / `sorted_by` key functions must return a single comparable
  primitive (`i64`/`f64`/`str`) — no tuple keys. `ds_sort_by_key` therefore
  uses a composite string key (`str(10 + len(s)) + "|" + s`) to emulate
  Python's `key=lambda s: (len(s), s)`; orderings are identical because the
  length prefix is fixed-width.
- There is no documented empty-set literal (`{}` is a dict, and empty
  literals need annotations). `ds_set_ops` seeds the set with `{0}` (0 is
  also the first inserted element, so contents match Python's, where the same
  seeded literal is used).
- StrictPy `i64` arithmetic overflows where CPython promotes to bigint.
  `ds_match_dispatch` reduces every Add/Mul step mod 1000003 in BOTH
  languages so outputs stay identical.
- Dict keys are restricted to `str` (guide §11.2), so all dict-based
  benchmarks (`ds_dict_ops`, `ds_comprehensions`, `ds_string_keys_aggregation`)
  use string keys by construction.
- Comprehension loop variables require explicit type annotations and the
  iterable must be a `List[T]` (not `range(...)`), so base lists are built
  with explicit while-loops on the StrictPy side.
- `reduce` has no conditional-expression-friendly max; used the prelude
  `max_i64` inside the reducing lambda in `ds_closures_hof` (Python side uses
  the idiomatic `max(list)`).
- Top-k extraction in `ds_string_keys_aggregation` is done with a
  fixed-width "inverted count + key" composite sort string (StrictPy has no
  tuple sort keys); Python uses `sorted(items, key=lambda kv: (-kv[1], kv[0]))`.
  Tie-breaking is identical (count desc, key asc).
