# Pending changes to BLOG_POST.md

Captured before session compaction. Goal: when the rewrite happens in a
future session, this file + the current `BLOG_POST.md` + the thesis
archive (`docs/thesis/`) are enough to produce the updated post.

The current `BLOG_POST.md` was written at the end of M9. Everything in
M10, M11, and M12 has happened since — that's the bulk of the new material.

**M12 update (2026-05-18)**: this file was originally written at end-of-M11.
M12 added a second stress-test round (regex, dijkstra, btree), the
BUG-026/027 torture test (250/250 clean — those bugs are now CONFIRMED
fixed, not just provisional), and 2 new bugs (BUG-034 / BUG-035; the
former fixed inline). Headline numbers are updated below to post-M12
values. The post-M11 narrative below is retained because the M12
chapter is mostly a confirmation of M11, plus the new "negative-form
silent miscompiles" lesson.

**M13–M17 update (2026-05-18)**: a 5-milestone language-completeness
sprint shipped after M12. Each milestone is a feature-sized chapter
in the rewrite. Sequenced because every feature touched ir.rs /
typecheck.rs and parallel agents would have conflicted.

- M13: short-circuit and/or (BUG-035 closed; first mid-expression CFG
  manipulation in the project).
- M14: tuples + destructuring (heap-allocated synthetic class layouts;
  zero new VM opcodes; eliminates the highest-frequency M10-M12 friction).
- M15: try/except/finally + raise (BUG-025 closed; lazy materialisation
  of exception objects; automatic per-function JIT carve-out).
- M16: isinstance + match case Constructor() (eliminates kind:i32
  discriminator workaround; M11 + M16 ship a coherent class system
  neither alone delivers).
- M17: generics with call-site monomorphisation (lazy worklist;
  per-instantiation operator binding; eliminates rewrite-per-type
  friction).

Post-M17 state: 255 tests, 0 failed, 31 bugs found, 30 fixed, 1
deferred (only BUG-028 lexer line-continuation remains). 28 example
programs. The language is now meaningfully "Python-shaped"; remaining
gaps (generic classes, exception subclassing, bounded generics) are
v0.2.

---

## Headline numbers that need updating

| Field | Old (M9) | New (post-M17) | Source |
|---|---|---|---|
| Examples | 7 | **28** | `examples/*.spy` count |
| Tests passing | 134 | **255** | `docs/thesis/stats/per_milestone.csv` last row |
| Benchmark wins | 16/0/0 | 16/0/0 (still) | `bench/history/m11_class_fix.json` |
| fib(30) | 13.5ms (12× CPython) | 13.1ms (~11× CPython) | M11/M12 bench (unchanged through M17) |
| Quicksort 100K | 18.6ms (13× CPython) | 18.6ms (12× CPython) | M11 bench |
| Distinct bugs found | "~12" | **31** | `docs/thesis/bugs/catalog.md` summary table |
| Deferred bugs | 6 in BUGS_KNOWN.md | **1** (only BUG-028 lexer) | post-M17 BUGS_KNOWN.md state |
| Code total | ~13K lines | **~24K Rust + ~5.5K StrictPy + ~4K tests + ~14K docs** | wc -l (approx, post-M17) |
| Milestones | M0-M9 | M0-M17 | git log |
| **NEW capabilities since M9** | — | tuples, try/except, isinstance, match case, generics | M13-M17 |

The "beats CPython by up to 17×" tagline is still right (fib(33) is 16-17×
depending on noise).

---

## New sections to add

### 1. A "stress testing scales superlinearly" chapter

This is the **single most important new finding** from M10/M11/M12 and deserves
its own section. Four sub-stories to weave together:

- **M10 csv_aggregate**: one 143-line program → BUG-001 nullable f64
  miscompile → audit pass → 4 more silent miscompiles in codegen.rs.
- **M10 round of 6 programs in parallel**: json_parse alone found 8
  bugs including the catastrophic `is not none` inverted.
- **M11 round of 5 programs**: lisp interpreter found N1 (vtable cap)
  and N2 (deterministic Pair-crash); calculator confirmed BUG-026's
  pre-first-println variant; collectively triggered the class-system
  overhaul.
- **M12 round of 3 programs + torture test**: regex (0 bugs), dijkstra
  (0 bugs), btree (2 bugs — BUG-034 silent `str !=` miscompile, BUG-035
  no short-circuit). The torture test (250 sequential runs) confirmed
  BUG-026/027 fixed. Two of three stress programs finding zero bugs is
  itself a confirmation result.

Through-line: **bugs cluster. When you find one, audit hard for
siblings.** And: **deterministic repros unlock non-deterministic
mysteries** (BUG-030 → BUG-016 → BUG-026 collateral fix). And, new from
M12: **confirmation is a deliverable** — programs that run first-try
in the natural shape are themselves evidence the prior milestone landed.

This deserves its own headed section after the JIT story and before
"what I learned" — call it something like "**M10 / M11 / M12: real
programs find real bugs (and then confirm fixes)**" or "**The post-JIT
chapter: making it correct**".

### 1b. The M12 confirmation chapter ("the absence of bugs is a result")

A short sub-section worth ~150-200 words. The M12 round shipped 3
parallel stress programs (regex, dijkstra, btree) plus a torture test
for the M11 provisionally-closed bugs. Two of three stress programs —
regex (sealed hierarchy with 8 subclasses + 6 vmethods + class-ref
fields) and dijkstra (class with parallel List[List[T]] fields +
recursive methods) — found ZERO bugs. The regex agent's report
phrasing is the load-bearing rhetoric: "8 sealed subclasses, 6 virtual
methods, class-ref subclass fields, ran first-try without a single
workaround." Pre-M11, every similar program was a bug catalogue.

Then BUG-034 (`str != str` always true — same shape as BUG-008 but on a
different operator and a different type) shows that even at M12 the
trickle hasn't stopped. Add to the "lessons" section: **negative-form
silent miscompiles hide behind positive-form code conventions** — any
new comparison operator needs both forms tested.

Then the torture test: 250 sequential invocations across calculator,
json_parse, and lisp = 250 clean = BUG-026/027 confirmed. In 3.12s of CI
wall-clock. The marginal cost of "provisional → confirmed" is tiny;
the credibility upgrade is large.

### 2. The BUG-029 story (op_new class_id ↔ type_id collision)

This is the most thesis-quality bug of the whole project. Worth ~300
words as a standalone vignette in the new chapter or as one of the
lessons. The shape:

- "Vtable wraps mod 4" symptom in M10
- Looked like a `& 0x3` mask somewhere — wasn't
- Was actually a long-standing M3-era hack: `op_new` falls back to
  indexing the type table by class_id when the operand doesn't match a
  known type_id
- Hack worked silently for 10 milestones because class_id and type_id
  ranges never overlapped
- 4th user class arrives with class_id 16 → numerically equals Shape's
  type_id 16 → silent wrong-type allocation → 4th sibling looks like
  Shape → "vtable mod 4" appearance
- Lesson: latent bugs accumulate dose-dependently. M3-era convenience
  hack only triggers in M10 when accumulated state crosses a threshold.

Source: `docs/thesis/bugs/catalog.md` BUG-029 entry.

### 3. The CPython→CPython→CPython→StrictPy realization

A clean rhetorical pivot worth adding to "What I Learned": **the
hardest part of building a competitive Python alternative wasn't beating
CPython on numbers — it was finding bugs by running real programs.**

The benchmark story is dramatic (4-17× faster) and dominates the current
post, but the real engineering work was M10/M11 — and that work was
mostly bug-hunting, not optimization.

Source: methodology.md.

### 4. The thesis archive / repository sections

Brief mention that:
- The project is now public at github.com/amitgangrade/StrictPy
- A complete thesis archive at `docs/thesis/` preserves every milestone's
  metrics, agent reports, design decisions, and bug catalog
- The work is fully reproducible (`cargo build --release && python
  bench/harness.py`)

Update "How to reproduce this" section to use the github URL.

---

## Sections to update (existing content needs revising)

### TL;DR table

Replace with M11 numbers. Current table only shows 5 benchmark cells —
keep it that way but with updated times. Add a row about "20 example
programs all running end-to-end" if there's space.

### "The milestones" section

Currently ends at M9. Add M10 and M11 as two new sub-sections matching
the existing prose style. Each ~200-300 words. Cover:

- M10: real-world stress test round; 6 programs in parallel; 17 bugs
  surfaced; 11 fixed including is-not-none inversion (the highlight
  bug); nullable audit pattern.
- M11: class-system overhaul; another 5 programs; 3 architectural
  bugs fixed (BUG-015/016/017); the BUG-029 latent-hack story;
  BUG-026/027 collateral-fixed.

The blog already has a precedent for "M3.5 detour" — same shape works
here. M10 and M11 each had their own surprises and lessons.

### "What I learned" section

The current 5 lessons are good but can be sharpened, and 2-3 new
lessons should be added from M10/M11:

**Existing lessons to keep (lightly polished)**:
1. Static types make AOT compilation trivial
2. The interpreter was the bottleneck, not the runtime
3. Tests that don't assert on values lie
4. AI-assisted development needs hard acceptance criteria
5. Methodology matters more than micro-optimization

**New lessons from M10/M11/M12**:
6. **Stress testing has superlinear ROI**. 23 real programs surfaced
   31 bugs; the original 7 examples + 4 benchmarks had surfaced ~12.
   Each new program found bugs the previous round didn't.
7. **Bugs cluster around a pattern**. Audit on first discovery. M10's
   CSV bug → 4 siblings. M11's "vtable mod 4" → 3 root causes.
8. **Deterministic repros are gold**. The non-deterministic
   heap-corruption bug looked unsolvable for a full milestone. M11's
   deterministic Pair-crash (N2/BUG-030) revealed BUG-016 as the root
   cause; fixing BUG-016 collateral-fixed the non-deterministic
   variant too. M12's torture test then converted "provisionally fixed"
   to "250/250 confirmed."
9. **Latent bugs accumulate dose-dependently**. Hacks that work
   silently for years can trigger after enough state accumulates.
   BUG-029 needed both 10+ milestones of accumulated class registrations
   AND a specific numeric collision.
10. **Negative-form silent miscompiles hide behind positive-form
    conventions**. BUG-008 (`is not` inverted) sat latent from M2 until
    M10 because every example used `if x is none: ... else: ...`.
    BUG-034 (`str !=` always true) sat latent from when strings became
    first-class until M12 because every example used `==` for string
    compares. Mechanical lesson: any new comparison operator needs both
    forms tested explicitly.

Cap the total at 7-8 lessons. Cut the weakest of the original 5 if
needed (probably #5 "methodology matters more than micro-optimization"
becomes redundant with the new lesson #8).

### "What StrictPy still can't do" section

Massively shrinks. The M9-era list had ~9 limitations; most are now
fixed. Updated list (post-M11):

**Post-M17 — most items from this list are now fixed.** Updated:

- ~~No `try/except` codegen~~ — **fixed in M15.** Full `try / except /
  finally / raise` ships; BUG-025 (fallible `open()`) closed.
- **No precise stack-map GC** — M9's `in_jit` pause still in place.
  (Unchanged.)
- ~~No `isinstance` / `match case` lowering~~ — **fixed in M16.**
  Subclass-chain isinstance + Constructor/Tuple/Wildcard patterns +
  flow narrowing + sealed-class exhaustiveness warning.
- ~~No user-code generics~~ — **fixed in M17** (free functions only).
  Call-site monomorphisation. Generic classes deferred to v0.2.
- ~~`and` / `or` bitwise approximation~~ — **fixed in M13** (BUG-035).
- **No tuples / multi-return** — ~~fixed in M14.~~
- **No implicit line continuation across infix `+`** (BUG-028) — still
  open. Only deferred bug post-M17.
- **`with open(...) as f:` doesn't route through try/except** — known
  gap from M15. Workaround: `try: with open(...) as f: ... except IOError:`
  explicitly. Long-term fix: desugar `with` to try/finally.
- **No generic classes** (`class Box[T]:`) — v0.2.
- **No user-defined exception subclasses** — v0.1 ships 10 built-in
  exception names; v0.2 adds subclassing.
- **No bounded generics** (`T: Comparable`) — v0.1 falls back to
  per-instantiation re-typecheck. v0.2.
- **No NumPy/pandas** — see `docs/thesis/design_decisions/why_no_numpy_pandas.md`.
  Three theoretical paths exist, none planned.

Remove these (now fixed): bounds checks, lambda lifting, `for x in xs:`
desugaring, `str.split`, `sorted`/`sort`, inheritance-stable vtables,
sealed dispatch, subclass field aliasing.

### "What's next" section

Update with the post-M11 menu (from end of last conversation):
- Try/except codegen (A)
- Generics in user code (B)
- Real sum types / `match case Constructor()` (C)
- Another stress-test round (D)
- Torture-test BUG-026/027 to "confirmed fixed" (E)
- Spec catch-up v0.1 → v0.2 (F)

Cut the "Does it scale to a real codebase?" question — partially
answered by the 20-example suite now.

### "Reproducing this" section

- Add github.com/amitgangrade/StrictPy as the canonical link
- Update directory tree to include `docs/thesis/`, `bench/history/`,
  `BUGS_KNOWN.md`
- Update the "Total wall-clock" line: now ~3-4 weeks elapsed (was "two
  weeks"); ~40-60 hours of agent compute (was "25 hours"). Drop the
  dollar estimate or leave it for the user to fill in honestly.

---

## Optional cuts (if length is a concern)

The new material adds ~1500 words. If the post is getting too long,
candidates to cut from the existing version:

- **"The architecture" section's directory tree** — duplicates README.
  Cut to a sentence.
- **"M3–M4: bytecode + interpreter"** narrative paragraph — can collapse
  to one sentence; the surprises in M3.5 are the interesting part.
- **The "M5–M7: stdlib and runtime classes"** narrative — can collapse
  with M6 since M5 had no narrative beats.

Keep all the bug stories and the "what I learned" — those are the
load-bearing material.

---

## Style guidance for the rewrite

The current post's voice works well. Maintain:
- First-person ("I built", not "we built")
- Honest about mistakes (the M3.5 detour story is one of the post's
  strongest sections — keep that energy)
- Concrete numbers everywhere
- Specific file:line and bug-id references make claims auditable

Things to tone down or remove:
- The "$200 in API costs" line was a guess; remove or leave for the
  user to fill in
- The "two weeks of focused work" claim — closer to 3-4 weeks now
- Avoid hyping the lessons too hard — let the numbers and stories speak

---

## Suggested workflow for the rewrite

1. **Open** `BLOG_POST.md`, `docs/thesis/timeline.md`,
   `docs/thesis/bugs/catalog.md`, `bench/history/README.md`,
   `BUGS_KNOWN.md` in one session.
2. **Insert** the new M10 / M11 sub-sections in "The milestones" using
   timeline.md as the source.
3. **Insert** the new "stress testing" headed section after the M9
   coverage section.
4. **Update** the TL;DR table with current numbers.
5. **Update** the "What I learned" section with the new lessons.
6. **Shrink** the "What StrictPy still can't do" section.
7. **Update** the "What's next" menu.
8. **Update** the "Reproducing this" section with the github URL and
   thesis archive pointer.
9. **Final pass** for tone, length, and the `$200 / two weeks` honesty
   issues.

Estimated effort: one focused agent task or ~45 minutes of orchestrator
time. Use `python bench/harness.py --report-only` to confirm current
benchmark numbers before pasting them in.
