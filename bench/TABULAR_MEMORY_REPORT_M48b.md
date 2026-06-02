# M48b — `tabular` memory deep-dive: where StrictPy's 4–5× peak-RSS gap comes from

**Status:** investigation only. No code shipped — this milestone is a
measurement + root-cause + recommendation, as scoped in HANDOFF's priority
queue. A "packed column" representation is the suggested v0.5 follow-up but
is explicitly out of scope here.

---

## UPDATE (M59): null-mask packing implemented — and what it actually moves

M59 implemented this report's #1 recommendation: the per-column null mask
is now stored as a `List[i64]` **bitset** (1 bit/cell) instead of a
`List[bool]` (8 bytes/cell) — **64× denser**. Phase A routed all ~100 null
reads through one accessor (`m37_read_nulls`); Phase B flipped that accessor
+ the two column allocators (`m37_alloc_column`, `m47_alloc_col_categorical`)
to pack/unpack the bitset. Verified correct: 785/0 on the VM suite (every
tabular op with nulls).

**Honest empirical result** (probe: hold a 1M-row × 8-i64-col `read_csv`
frame, `bench/m59_mem_probe.spy`): the **live-heap** footprint of the null
masks drops exactly as predicted (8 cols × 8 MB = 64 MB → ~1 MB). **But
peak/settled process RSS barely moved** (peak 649→642 MB; settled-after-GC
206→205 MB). Two reasons, both important:
1. **Peak RSS is dominated by construction transients**, not held null
   masks — `read_csv` materialises millions of `String` cells + per-column
   accumulators, which dwarf the mask savings at the high-water mark.
2. **The conservative-GC allocator retains freed pages** — once the smaller
   bitset replaces the `List[bool]`, the freed 63 MB is not returned to the
   OS, so RSS plateaus near the peak even though the *live* heap shrank.

**Takeaway:** null-mask packing is a real, exact reduction in **live working
set** (it pays off for long-held frames and reduces re-allocation pressure),
but it does **not** by itself shrink the 4–5× **peak**-RSS gap. Closing that
additionally requires cutting construction transients — pack directly from
the `Vec<bool>` accumulator (skip the intermediate `List[bool]`), exact-size
filter results, and ideally return freed pages to the OS. Those are the
genuine peak-RSS levers; M59 is the necessary foundation under them.

---

## 1. The gap, measured (M48)

The M48 benchmark suite measured peak process RSS (psutil, polled every
50 ms) for StrictPy `tabular` vs pandas 3.0 / NumPy across operations and
sizes. The headline memory finding:

| Cell | StrictPy peak RSS | pandas peak RSS | ratio |
|---|---:|---:|---:|
| `filter` / large | **1.07 GB** | 0.20 GB | **5.35×** |
| (typical large cells) | — | — | **~4–5×** |

The M51 `merge_cat_codes` run reproduced the same shape under cross-product
load: a low-cardinality string-key merge hit **9.3 GB** RSS on StrictPy vs
~1.9 GB on pandas — the per-cell overhead below, amplified by a many-to-many
join blow-up.

This report explains the 4–5× from first principles (byte-level, with
source evidence) and shows the static model predicts ~4.3× for a mixed
frame — matching the measurement.

## 2. The StrictPy heap memory model (evidence)

All sizes verified against `vm/src/object.rs` and `vm/src/interp.rs`.

| Object | Layout | Size |
|---|---|---:|
| `ObjectHeader` (`object.rs`) | `vtable: *const RuntimeType` (8) + `gc_meta: u64` (8) | **16 B** fixed/object |
| `ListRepr` (`object.rs`) | header (16) + `length` (8) + `capacity` (8) + `data: *mut u8` (8) | **40 B** + data buffer |
| List element (`interp.rs::alloc_list`/`list_push`) | every slot is `u64`-wide, **regardless of element type** | **8 B / element** |
| `StringRepr` (`object.rs`) | header (16) + `length` (8) + `byte_len` (8) + `data` (8) + `flags` (8) | **48 B** + byte buffer, **no interning** |
| Column (`ColumnI64` etc.) | header (16) + payload (24: values-ptr, nulls-ptr, length) | **40 B** + 2 ListReprs |
| DataFrame | header (16) + payload (56) | 72 B (negligible) |

Two policies matter a lot:

- **`alloc_list` reserves `capacity.max(4)`** and **`list_push` grows by
  2× doubling** (`interp.rs`). A result list built element-by-element (e.g.
  the output of `filter`) can therefore carry **up to ~2× capacity slack**.
- **Bool lists store 1 byte of information in 8 bytes.** `m37_alloc_list_bool`
  / `m37_read_list_bool` write/read each element as a full `u64`
  (`builtins.rs`). Every column's null mask is a `List[bool]`.

## 3. Per-cell arithmetic — worked example

Frame: **1,000,000 rows × 5 `i64` columns + 2 `str` columns** (≈50 unique
strings/column avg, 50 B each).

**Per `i64` column (StrictPy):**
- Column object: 40 B
- values `List[i64]`: 40 B header + 8 MB data
- nulls `List[bool]`: 40 B header + **8 MB data** ← every cell's null flag costs 8 B
- **≈ 16 MB / column** vs pandas ~8.6 MB (8 MB values + ~0.6 MB bit-packed nulls)

5 i64 columns → **80 MB** (StrictPy) vs ~43 MB (pandas).

**Per `str` column (StrictPy):**
- values `List[str]`: 8 MB of pointers + N×(48 B `StringRepr` + bytes)
- nulls `List[bool]`: 8 MB
- ≈ 8 + 8 + (string objects) MB; low-cardinality strings are **not deduped**

**Total:** StrictPy ≈ **410 MB** vs pandas ≈ **95 MB** → **4.3×**, matching
the measured 4–5×. `filter`/large measures slightly higher (5.35×) because
filter **allocates a whole second frame** (transient 2× live set) **plus**
the 2× capacity slack on the push-built result lists.

## 4. Top contributors, ranked

1. **Null mask at 8 B/bool, on every column (dominant).** A 1M-row column
   spends 8 MB on its null mask alone — 64× the 0.125 MB a bit-packed mask
   would use, 8× a byte-packed one. With 5+ columns this is the single
   biggest line item. Evidence: every Column subclass carries `nulls:
   List[bool]`; `m37_*_list_bool` treat each bool as `u64`.
2. **Bool *value* columns at 8 B/element.** Same u64-slot cost; a
   `ColumnBool` of 1M cells is 8 MB vs 1 MB byte-packed / 0.125 MB bit-packed.
3. **No string interning + per-object overhead + capacity slack.** Each
   string is a separate 48 B `StringRepr` + bytes with no dedup (the
   source comments note interning is deferred v0.3 work); every List adds a
   40 B header and rounds capacity up (min 4, ×2 growth), so `filter`-style
   ops carry up to 2× slack on the result.

## 5. Recommendations (v0.5 candidates), highest leverage first

1. **Pack the null mask** — bitset (1 bit/cell) or byte-mask (1 byte/cell).
   Recovers ~7.9 MB (bitset) or ~7 MB (byte) **per 1M-row column**. This
   alone should move the steady-state gap from ~4.3× toward **~2.5–3×**.
   Moderate effort: change the null representation behind `m37_*_list_bool`
   accessors; columns already go through helpers, so the blast radius is
   contained. **Do this first.**
2. **Pack bool value columns** the same way (1 bit or 1 byte). Smaller
   absolute win (fewer bool columns than null masks) but the same mechanism.
3. **Exact-size result buffers for row-producing ops** (`filter`, `dropna`,
   `take`): pre-size the output lists to the known kept-row count instead of
   push-doubling. Cheap, targeted, kills the up-to-2× slack that makes
   `filter` the worst-measured cell.
4. **String interning / dedup** (already flagged as planned v0.3 work).
   Large win for low-cardinality string columns; turns N duplicate 48 B+bytes
   objects into N pointers to one. Bigger architectural change.
5. **Holistic "packed column" representation** (the v0.5 path): store each
   column's values in a single contiguous typed buffer + a bit-packed
   validity mask, NumPy/Arrow-style, instead of `List<T>` + `List[bool]`.
   Subsumes (1)–(3) and closes most of the remaining gap, at the cost of
   touching every column-dispatching handler. Not scoped here.

## 6. Bottom line

The 4–5× is **not** a mystery of allocator fragmentation or GC bloat — it
is explained almost entirely by two deliberate v1 simplifications: the
**8-byte-per-bool null mask carried on every column**, and the **`List<T>`
uniform-8-byte-slot** representation vs NumPy's contiguous typed buffers,
with a secondary contribution from **un-interned strings** and **2× list
capacity slack on filter-style ops**. The single highest-leverage fix is
packing the null mask; a full packed-column representation is the v0.5
endgame.
