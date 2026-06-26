//! # StrictPy conformance suite — negative cases
//!
//! Each entry in [`CASES`] is a tiny `.spy` snippet that the StrictPy spec
//! says **must** be rejected. The single [`negative_conformance`] test
//! iterates over them and asserts that [`strictpy_compiler::compile_source`]
//! returns the expected [`ErrorCategory`].
//!
//! ## Why category, not error code
//!
//! Per `STRICTPY_SPEC.md` §18.2, error codes live in fixed ranges:
//!
//! | Range   | Category   | `CompileError` variant |
//! |---------|------------|------------------------|
//! | `E0xxx` | parse      | `CompileError::Parse`     |
//! | `E1xxx` | resolve    | `CompileError::Resolve`   |
//! | `E2xxx` | type       | `CompileError::Type`      |
//! | `E3xxx` | semantic   | `CompileError::Semantic`  |
//! | `E4xxx` | linker     | (n/a in M2)               |
//!
//! Milestone 2 introduces the resolver and type checker. The specific
//! `E1xxx`/`E2xxx`/`E3xxx` numbers that get assigned to each diagnostic are
//! the resolver/typechecker agent's choice. This suite intentionally only
//! asserts the **variant** (category), so it stays valid no matter which
//! exact code is picked, as long as the diagnostic ends up in the correct
//! pipeline stage.
//!
//! ## Adding a new case
//!
//! Append a [`NegativeCase`] to [`CASES`]:
//!
//! - `name`: short snake_case, used in the simulated file name.
//! - `source`: the `.spy` body, embedded as a raw string. Keep it minimal
//!   (2–6 lines), just enough to trigger the violation.
//! - `expected_category`: which `CompileError` variant the diagnostic
//!   should land in.
//! - `spec_section`: the §-reference in `STRICTPY_SPEC.md` that the
//!   snippet violates. Used in failure messages so you can jump to the
//!   spec when a case regresses.
//! - `description`: one-line human gloss.
//!
//! ## Enabling the suite
//!
//! Currently the test is `#[ignore]`. Once the M2 resolver+typechecker
//! agent lands their work, drop the `#[ignore]` attribute (or invoke
//! `cargo test -- --ignored`) and the orchestrator will pick it up.

use strictpy_compiler::compile_source;
use strictpy_compiler::error::CompileError;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ErrorCategory {
    Parse,
    Resolve,
    Type,
    Semantic,
}

struct NegativeCase {
    name: &'static str,
    source: &'static str,
    expected_category: ErrorCategory,
    spec_section: &'static str,
    description: &'static str,
}

fn categorize(err: &CompileError) -> ErrorCategory {
    match err {
        CompileError::Parse { .. } => ErrorCategory::Parse,
        CompileError::Resolve { .. } => ErrorCategory::Resolve,
        CompileError::Type { .. } => ErrorCategory::Type,
        CompileError::Semantic { .. } => ErrorCategory::Semantic,
        CompileError::Io(e) => panic!("unexpected IO error in conformance test: {e}"),
    }
}

fn check_rejected(case: &NegativeCase) -> Result<(), String> {
    let name = case.name.to_string();
    let src = case.source.to_string();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        compile_source(format!("conformance/{name}.spy"), &src)
    }));
    match result {
        // The pipeline reached an unimplemented later stage (M3+) without the
        // typechecker rejecting the program — so this case slipped past M2.
        Err(panic_info) => {
            let msg = panic_info
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic_info.downcast_ref::<&str>().copied())
                .unwrap_or("<non-string panic>");
            Err(format!(
                "{}: expected rejection per spec {} ({}), but typecheck passed and later stage panicked: {}",
                case.name, case.spec_section, case.description, msg
            ))
        }
        Ok(Ok(_)) => Err(format!(
            "{}: expected rejection per spec {} ({}), but compilation succeeded",
            case.name, case.spec_section, case.description
        )),
        Ok(Err(err)) => {
            let actual = categorize(&err);
            if actual != case.expected_category {
                Err(format!(
                    "{}: expected {:?} per spec {} ({}), got {:?}: {:?}",
                    case.name, case.expected_category, case.spec_section,
                    case.description, actual, err
                ))
            } else {
                Ok(())
            }
        }
    }
}

// ── Cases ───────────────────────────────────────────────────────────────
//
// Grouped by spec section. Each `.spy` snippet should be the smallest
// program that demonstrates the violation. Programs without a `main`
// function are fine for tests targeting resolve/type errors in other
// declarations — the resolver/typechecker should reject before the
// missing-`main` check fires. For cases where the only violation is in
// `main` itself, the snippet includes a `main` so the error surfaces there.

const CASES: &[NegativeCase] = &[
    // ── §5.3 numeric coercion ───────────────────────────────────────────
    // (Wave-1 Lane A) `i32 + i64` is now ACCEPTED — implicit lossless widening
    // (i32→i64) was added; see numeric_coercion_runs.rs for the positive cases.
    // The former `i32_plus_i64_rejected` negative case was removed accordingly.
    NegativeCase {
        name: "float_assigned_to_int_rejected",
        source: "\
fn f() -> i32:
    x: i32 = 3.14
    return x
",
        expected_category: ErrorCategory::Type,
        spec_section: "§5.3",
        description: "float literal not implicitly convertible to i32",
    },
    NegativeCase {
        name: "lossy_assignment_without_cast_rejected",
        source: "\
fn f() -> i32:
    big: i64 = 1
    small: i32 = big
    return small
",
        expected_category: ErrorCategory::Type,
        spec_section: "§5.3",
        description: "i64 -> i32 requires explicit conversion",
    },

    // ── §5.5 forbidden constructs ───────────────────────────────────────
    NegativeCase {
        name: "any_type_rejected",
        source: "\
fn f(x: Any) -> i32:
    return 0
",
        expected_category: ErrorCategory::Type,
        spec_section: "§5.5",
        description: "Any type is forbidden",
    },
    NegativeCase {
        name: "setattr_dynamic_name_rejected",
        source: "\
final class Point:
    x: i32 = 0

fn f(obj: Point, name: str) -> None:
    setattr(obj, name, 1)
",
        // Implementation choice: forbidden-construct rejections are E2xxx (Type),
        // not E3xxx (Semantic). Spec §18.2 is permissive about the boundary.
        expected_category: ErrorCategory::Type,
        spec_section: "§5.5",
        description: "setattr with non-literal name is forbidden",
    },
    NegativeCase {
        name: "eval_call_rejected",
        source: "\
fn f() -> i32:
    return eval(\"1 + 1\")
",
        expected_category: ErrorCategory::Type,
        spec_section: "§5.5",
        description: "eval is forbidden at runtime",
    },
    NegativeCase {
        name: "multiple_inheritance_rejected",
        source: "\
open class A:
    a: i32 = 0

open class B:
    b: i32 = 0

class C(A, B):
    c: i32 = 0
",
        // Caught at parse time (parser E0005) — the rule is in spec §5.5 but
        // the grammar enforces it directly via the single-base bases list.
        expected_category: ErrorCategory::Parse,
        spec_section: "§5.5",
        description: "multiple inheritance is forbidden (single inheritance only)",
    },
    NegativeCase {
        name: "final_class_subclassed_rejected",
        source: "\
final class A:
    a: i32 = 0

class B(A):
    b: i32 = 0
",
        expected_category: ErrorCategory::Type,
        spec_section: "§5.5",
        description: "cannot subclass a final class",
    },
    NegativeCase {
        name: "dict_attr_assignment_rejected",
        source: "\
final class Point:
    x: i32 = 0

fn f(obj: Point) -> None:
    obj.__dict__ = {}
",
        expected_category: ErrorCategory::Type,
        spec_section: "§5.5",
        description: "modifying instance __dict__ is forbidden",
    },

    // ── §6.2 / §6.3 name resolution + definite assignment ───────────────
    NegativeCase {
        name: "undeclared_name_rejected",
        source: "\
fn f() -> i32:
    return undefined_var
",
        expected_category: ErrorCategory::Resolve,
        spec_section: "§6.2",
        description: "reference to undeclared name",
    },
    NegativeCase {
        name: "assign_before_declare_rejected",
        source: "\
fn f() -> i32:
    x = 5
    return x
",
        expected_category: ErrorCategory::Resolve,
        spec_section: "§6.3",
        description: "assignment to never-declared name",
    },
    NegativeCase {
        name: "duplicate_let_same_scope_rejected",
        source: "\
fn f() -> i32:
    x: i32 = 1
    x: i32 = 2
    return x
",
        expected_category: ErrorCategory::Resolve,
        spec_section: "§6.3",
        description: "redeclaration of name in same scope",
    },
    // NOTE: §6.2 "no nonlocal" rule isn't currently triggerable via the surface
    // grammar — StrictPy has no nested fn decls (the grammar only lists fn as a
    // top-level decl), and lambdas have expression bodies so they can't contain
    // an assignment statement. Re-add a §6.2 negative case here when v0.2 adds
    // either nested fn decls or block-bodied lambdas.

    // ── §6.4 nullable narrowing ─────────────────────────────────────────
    NegativeCase {
        name: "nullable_used_without_narrowing_rejected",
        source: "\
fn f(x: i32?) -> i32:
    return x + 1
",
        expected_category: ErrorCategory::Type,
        spec_section: "§6.4",
        description: "T? cannot be used as T without narrowing",
    },
    NegativeCase {
        name: "nullable_arithmetic_rejected",
        source: "\
fn f(x: i32?) -> i32?:
    return x + 1
",
        expected_category: ErrorCategory::Type,
        spec_section: "§6.4",
        description: "arithmetic operators not defined on nullable types",
    },

    // NOTE: §6.5 exhaustiveness is currently a TODO in the type checker
    // (any non-trivial match passes silently). Re-add a non_exhaustive_match
    // case here once that check lands.

    // ── §5 generics / protocols / function calls ────────────────────────
    NegativeCase {
        name: "list_invariance_rejected",
        source: "\
open class A:
    a: i32 = 0

final class B(A):
    b: i32 = 0

fn takes_list_of_a(xs: List[A]) -> None:
    pass

fn main() -> i32:
    ys: List[B] = [B()]
    takes_list_of_a(ys)
    return 0
",
        expected_category: ErrorCategory::Type,
        spec_section: "§5.2",
        description: "List is invariant; List[B] is not a subtype of List[A]",
    },
    NegativeCase {
        name: "protocol_method_missing_rejected",
        source: "\
protocol Drawable:
    fn draw(self) -> None

final class NotDrawable:
    x: i32 = 0

fn render(d: Drawable) -> None:
    d.draw()

fn main() -> i32:
    render(NotDrawable())
    return 0
",
        expected_category: ErrorCategory::Type,
        spec_section: "§5.2",
        description: "type passed to protocol param must satisfy protocol",
    },
    NegativeCase {
        name: "wrong_arity_call_rejected",
        source: "\
fn f(a: i32, b: i32) -> i32:
    return a + b

fn main() -> i32:
    return f(1)
",
        expected_category: ErrorCategory::Type,
        spec_section: "§5.5",
        description: "call with wrong arity",
    },
    NegativeCase {
        name: "wrong_arg_type_rejected",
        source: "\
fn f(a: i32) -> i32:
    return a

fn main() -> i32:
    return f(\"hello\")
",
        expected_category: ErrorCategory::Type,
        spec_section: "§5.3",
        description: "argument type does not match parameter type",
    },

    // ── Additional cases beyond the required 20 ─────────────────────────
    NegativeCase {
        name: "return_type_mismatch_rejected",
        source: "\
fn f() -> i32:
    return \"hello\"
",
        expected_category: ErrorCategory::Type,
        spec_section: "§5.3",
        description: "return expression must match declared return type",
    },
    NegativeCase {
        name: "use_before_declare_rejected",
        source: "\
fn f() -> i32:
    y: i32 = x + 1
    x: i32 = 5
    return y
",
        expected_category: ErrorCategory::Resolve,
        spec_section: "§6.3",
        description: "use of name before its declaration",
    },
    // ── §4.1 const_decl: module-level `final` initialisers ──────────────
    NegativeCase {
        name: "const_init_not_compile_time_rejected",
        source: "\
fn f() -> i64:
    return 1

final X: i64 = f()

fn main() -> i32:
    return 0
",
        expected_category: ErrorCategory::Semantic,
        spec_section: "§4.1",
        description: "module-level `final` initialiser must be compile-time \
                      evaluable (a call is not); silently lowering to 0 was \
                      the historical bug",
    },

    // ── Wave-1 correctness (Lane D): silent-footgun fixes ───────────────
    // Dict non-`str` key (E2072): `Dict[i64, _]` compiled then SEGFAULTed at
    // subscript because the runtime dict is hardcoded to string keys.
    NegativeCase {
        name: "dict_i64_key_rejected",
        source: "\
fn main() -> i32:
    d: Dict[i64, i64] = {}
    d[1i64] = 10i64
    return 0
",
        expected_category: ErrorCategory::Type,
        spec_section: "§7.x",
        description: "Dict key type must be str; non-str keys would segfault",
    },
    // Dict non-`str` key inferred from a literal. The literal `{1i64: "a"}`
    // synthesises `Dict[i64, str]`; the dict-literal key check rejects it.
    NegativeCase {
        name: "dict_int_literal_key_rejected",
        source: "\
fn sink(d: Dict[str, str]) -> None:
    pass

fn main() -> i32:
    sink({1i64: \"a\"})
    return 0
",
        expected_category: ErrorCategory::Type,
        spec_section: "§7.x",
        description: "dict literal with int key inferred as non-str is rejected",
    },
    // Tuple-keyed dict (E2072) — `Dict[Tuple[i64,i64], _]`.
    NegativeCase {
        name: "dict_tuple_key_rejected",
        source: "\
fn main() -> i32:
    d: Dict[Tuple[i64, i64], str] = {}
    return 0
",
        expected_category: ErrorCategory::Type,
        spec_section: "§7.x",
        description: "Dict with a tuple key type is rejected",
    },
    // Unknown decorator (E2071): decorators parse but do nothing, so an
    // unrecognized `@deco` is now a hard error instead of a silent no-op.
    NegativeCase {
        name: "unknown_decorator_rejected",
        source: "\
@lru_cache
fn fib(n: i32) -> i32:
    return n

fn main() -> i32:
    return 0
",
        expected_category: ErrorCategory::Type,
        spec_section: "§5.x",
        description: "unknown decorator is a hard error, not a silent no-op",
    },
    // Unsupported context manager (E2070): `with` only supports io.File; any
    // other resource's cleanup used to silently never run.
    NegativeCase {
        name: "with_non_file_rejected",
        source: "\
final class Lock:
    held: bool = false

fn main() -> i32:
    with Lock() as l: Lock:
        return 0
    return 0
",
        expected_category: ErrorCategory::Type,
        spec_section: "§7.6",
        description: "with on a non-io.File context manager is rejected (cleanup would be a no-op)",
    },
    // `raise X from Y` where Y is not an exception (E2050): the cause used to
    // be parsed then silently dropped; now it is type-checked.
    NegativeCase {
        name: "raise_from_non_exception_rejected",
        source: "\
fn main() -> i32:
    raise ValueError(\"bad\") from 42
    return 0
",
        expected_category: ErrorCategory::Type,
        spec_section: "§7.5",
        description: "raise X from Y requires Y to be an exception value",
    },
];

#[test]
fn negative_conformance() {
    let failures: Vec<_> = CASES
        .iter()
        .filter_map(|case| check_rejected(case).err())
        .collect();
    if !failures.is_empty() {
        panic!(
            "\n{} of {} negative conformance cases failed:\n  - {}\n",
            failures.len(),
            CASES.len(),
            failures.join("\n  - ")
        );
    }
}
