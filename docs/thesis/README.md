# StrictPy Thesis Archive

This directory preserves the intermediate state, decisions, and artifacts of the
StrictPy project so a future research thesis can reconstruct the work without
relying on conversation history or developer recollection.

## What's here

```
docs/thesis/
├── README.md                       This file
├── timeline.md                     Per-milestone narrative with key events
├── methodology.md                  How the project was conducted
│                                   (AI-pair-programming process, decisions,
│                                    measurement discipline)
├── stats/
│   ├── per_milestone.csv           Machine-readable metrics per milestone
│   └── per_milestone.md            Same data, human-readable table
├── milestones/                     Per-milestone deep-dive notes
│   ├── m0_spec.md
│   ├── m1_lexer_parser.md
│   ├── ...
│   └── m10_realworld.md
├── bugs/
│   └── catalog.md                  Every bug found, classified, fixed/deferred
├── design_decisions/               Key architectural choices with rationale
│   ├── unified_jit_abi.md
│   ├── nullable_unwrap_dispatch.md
│   ├── is_native_class_flag.md
│   ├── conservative_gc_with_in_jit_pause.md
│   └── per_function_jit_opt_in.md
└── agent_reports/                  Verbatim agent task summaries, ordered
                                    chronologically. Source data for the
                                    "AI-assisted development" angle.
```

## Source-of-truth pointers

These live in the project root, NOT under `docs/thesis/`, but are part of the
thesis evidence:

- **`STRICTPY_SPEC.md`** (1,813 lines) — the canonical language and VM spec.
  Frozen at v0.1 when the project started; minor amendments documented in the
  milestone notes when they happen.
- **`BLOG_POST.md`** — the public-facing performance-journey narrative covering
  M0–M9. Cross-references `bench/history/` for the data behind every claim.
- **`BUGS_KNOWN.md`** — deferred-bug catalogue maintained as a live document.
  Architectural bugs only; trivial bugs go to `docs/thesis/bugs/catalog.md`
  and get fixed in the next milestone.
- **`README.md`** — implementation status, build instructions, what runs today.
- **`bench/history/`** — five timestamped benchmark snapshots from
  M7-unfair through M10. Each is both a JSON and a rendered markdown report.
- **`bench/harness.py`** — the benchmark harness itself. Reproducible:
  `python bench/harness.py` regenerates `bench/results.json` and
  `bench/BENCH_REPORT.md`. `--report-only` re-renders from existing JSON.
- **Git history** — `git log --oneline` is sparse (4 commits) because M0–M9
  landed in the initial commit. Subsequent milestones each get one commit.
  The commit messages capture intent and quantitative deltas.

## Thesis chapters this archive supports

- **Chapter: Design** — `STRICTPY_SPEC.md` + `design_decisions/`
- **Chapter: Implementation** — `milestones/` + line-of-code totals in `stats/`
- **Chapter: Performance** — `bench/history/` + `BLOG_POST.md`'s progression table
- **Chapter: Methodology** — `methodology.md` + `agent_reports/`
- **Chapter: Findings** — `bugs/catalog.md` + `BUGS_KNOWN.md`

## Discipline going forward

For each subsequent milestone, this archive gets:

1. A new file in `milestones/` with the milestone's scope, agent briefs (link),
   results, and any unexpected findings.
2. The agent reports (verbatim or condensed) appended to `agent_reports/`.
3. New rows in `stats/per_milestone.csv`.
4. New bug entries in `bugs/catalog.md` (one per bug found, regardless of
   whether fixed immediately).
5. If a milestone introduces a load-bearing design choice, a new file in
   `design_decisions/`.

The archive is intentionally machine-readable where possible (CSV stats,
benchmark JSON) so future analysis can be quantitative.
