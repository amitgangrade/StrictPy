# StrictPy conformance suite

The two `conformance_*.rs` test files in this directory pin the StrictPy
compiler against `STRICTPY_SPEC.md`. They are integration tests of
`strictpy_compiler::compile_source` and intentionally exercise behavior,
not internal data structures.

| File                          | Purpose                                                |
|-------------------------------|--------------------------------------------------------|
| `conformance_negative.rs`     | Programs the spec says **must** be rejected.           |
| `conformance_positive.rs`     | Every `examples/*.spy` program **must** type-check.    |
| `conformance_README.md`       | This file.                                             |

Both suites are gated on the M2 milestone (resolver + type checker) and
are currently `#[ignore]`d. The orchestrator will enable them once the
M2 agent has landed.

## What "conformance" means here

Conformance tests are the contract between the spec and the
implementation. They are deliberately *narrow*: each one points at a
single section of `STRICTPY_SPEC.md` and asserts a single behavior. If a
case starts to fail, the failure message names the spec section so the
investigator knows where to read.

These tests are **not** unit tests for any one stage of the pipeline.
They go through the full public entry point, `compile_source`, so they
keep working as the internals get refactored.

## Adding a new negative case

Append a `NegativeCase` to the `CASES` array in
`conformance_negative.rs`:

```rust
NegativeCase {
    name: "my_new_violation_rejected",
    source: "\
fn f() -> i32:
    // ...minimal snippet that triggers the violation...
    return 0
",
    expected_category: ErrorCategory::Type,
    spec_section: "§5.3",
    description: "human-readable one-liner",
},
```

Guidelines:

- **Keep snippets small.** 2–6 lines. A snippet that fails for two
  reasons is ambiguous — the test can't tell which check fired.
- **Pin the spec section.** `spec_section` shows up in failure
  messages. Future maintainers will jump straight to that section.
- **Assert the category, not the code number.** Specific error code
  numbers (`E2001` etc.) belong to the resolver/typechecker
  implementation. Tests assert `ErrorCategory`, which maps to the
  `CompileError` variant. See the mapping below.

## Spec section → error category mapping

| Spec area                            | Category   | `CompileError` variant   |
|--------------------------------------|------------|--------------------------|
| §4 (grammar / surface syntax)        | `Parse`    | `CompileError::Parse`    |
| §5.5 (`Any`, `eval`, multi-inherit)  | `Semantic` or `Type` (depending on the rule — `Any` shows up during type checking; `eval`/`multi-inherit` are caught as a separate semantic check) |
| §5.3 (numeric coercion)              | `Type`     | `CompileError::Type`     |
| §5.2 (subtyping / variance)          | `Type`     | `CompileError::Type`     |
| §6.2 (scoping, no `nonlocal`)        | `Resolve`  | `CompileError::Resolve`  |
| §6.3 (definite assignment)           | `Resolve`  | `CompileError::Resolve`  |
| §6.4 (nullable narrowing)            | `Type`     | `CompileError::Type`     |
| §6.5 (match exhaustiveness)          | `Semantic` | `CompileError::Semantic` |

The full error-code numbering scheme is documented in spec §18.2 and
mirrored in `compiler/src/error.rs::codes`.

## Adding a new positive example

Drop a new `.spy` file into the repo-level `examples/` directory. The
positive suite picks it up automatically — no test edits required.

## Enabling the suite after M2 lands

Both tests carry:

```rust
#[ignore = "M2 in progress; enable after resolver+typechecker land"]
```

When the resolver/typechecker agent reports M2 complete:

1. Delete the `#[ignore = ...]` attribute on `negative_conformance` and
   `positive_conformance`.
2. Run `cargo test -p strictpy-compiler --test conformance_negative
   --test conformance_positive` and triage any failures.
3. If a positive example doesn't type-check, fix the example (or the
   compiler — whichever is wrong by the spec).
4. If a negative case is *accepted*, the implementation is too
   permissive; file a bug pointing at the relevant spec section.

Until then, `cargo test` will skip both suites and `cargo check
-p strictpy-compiler --tests` will only verify that the test code
compiles.
