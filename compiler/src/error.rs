//! Compiler diagnostics. See spec §18.
//!
//! Error code categories (spec §18.2):
//!
//! | Range  | Category               |
//! |--------|------------------------|
//! | E0xxx  | parse errors           |
//! | E1xxx  | name resolution        |
//! | E2xxx  | type errors            |
//! | E3xxx  | semantic errors        |
//! | E4xxx  | linker / module errors |
//! | W0xxx  | warnings               |

use thiserror::Error;

/// Stable error code, e.g. `E0123`. Strings instead of an enum so that adding
/// a new code does not require a recompile of every crate that references it.
pub type ErrorCode = &'static str;

/// Top-level compiler error type. Every fallible stage of the pipeline returns
/// this. Sub-stages may produce richer internal error types and convert here.
#[derive(Debug, Error)]
pub enum CompileError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("error[{code}]: {message} at {file}:{line}:{col}",
        code = "E0001")]
    Parse {
        file: String,
        line: u32,
        col: u32,
        message: String,
    },

    #[error("error[{code}]: {message} at {file}:{line}:{col}")]
    Resolve {
        file: String,
        line: u32,
        col: u32,
        code: ErrorCode,
        message: String,
    },

    #[error("error[{code}]: {message} at {file}:{line}:{col}")]
    Type {
        file: String,
        line: u32,
        col: u32,
        code: ErrorCode,
        message: String,
    },

    #[error("error[{code}]: {message} at {file}:{line}:{col}")]
    Semantic {
        file: String,
        line: u32,
        col: u32,
        code: ErrorCode,
        message: String,
    },
}

/// Error-code constants. Numbering scheme matches spec §18.2.
///
/// New codes should be appended; never re-use a retired number, since these
/// appear in user-visible diagnostics and tooling may key on them.
pub mod codes {
    use super::ErrorCode;

    // ── E0xxx: parse ─────────────────────────────────────────────────────
    pub const PARSE_UNEXPECTED_TOKEN: ErrorCode = "E0001";
    pub const PARSE_UNEXPECTED_EOF: ErrorCode = "E0002";
    pub const PARSE_BAD_INDENT: ErrorCode = "E0003";
    pub const PARSE_TAB_INDENT: ErrorCode = "E0004";

    // ── E1xxx: name resolution ───────────────────────────────────────────
    /// Duplicate top-level declaration (spec §6.2).
    pub const RESOLVE_DUPLICATE_DECL: ErrorCode = "E1001";
    /// Duplicate `let` in the same scope (spec §6.3).
    pub const RESOLVE_DUPLICATE_LET: ErrorCode = "E1002";
    /// Assignment to an undeclared variable (spec §6.3).
    pub const RESOLVE_ASSIGN_UNDECLARED: ErrorCode = "E1003";
    /// Name not in scope (spec §6.2).
    pub const RESOLVE_UNDEFINED: ErrorCode = "E1004";
    /// Cannot assign to captured variable from enclosing function (spec §6.2).
    pub const RESOLVE_CAPTURE_ASSIGN: ErrorCode = "E1005";
    /// Unknown type name in a type annotation.
    pub const RESOLVE_UNKNOWN_TYPE: ErrorCode = "E1006";

    // Back-compat aliases for older code paths.
    pub const RESOLVE_REDEFINITION: ErrorCode = "E1002";
    pub const RESOLVE_UNINIT: ErrorCode = "E1003";

    // ── E2xxx: type errors ───────────────────────────────────────────────
    pub const TYPE_BINOP_MISMATCH: ErrorCode = "E2001";
    pub const TYPE_BOOL_REQUIRED: ErrorCode = "E2002";
    pub const TYPE_NO_FIELD: ErrorCode = "E2003";
    pub const TYPE_NO_METHOD: ErrorCode = "E2004";
    pub const TYPE_INFER_FAIL: ErrorCode = "E2005";
    pub const TYPE_ANY_FORBIDDEN: ErrorCode = "E2006";
    pub const TYPE_DYNAMIC_ATTR: ErrorCode = "E2007";
    pub const TYPE_EVAL_FORBIDDEN: ErrorCode = "E2008";
    pub const TYPE_DUNDER_DICT: ErrorCode = "E2009";
    pub const TYPE_MULTI_INHERIT: ErrorCode = "E2010";
    pub const TYPE_MISMATCH: ErrorCode = "E2001";
    pub const TYPE_NOT_CALLABLE: ErrorCode = "E2011";
    pub const TYPE_ARITY: ErrorCode = "E2012";
    pub const TYPE_NULLABLE_USE: ErrorCode = "E2013";
    /// Cannot subclass a `final` class (spec §5.5 / §1.3).
    pub const TYPE_SUBCLASS_FINAL: ErrorCode = "E2014";
    /// M63b: a generic type argument does not satisfy the declared bound on its
    /// type parameter, e.g. `max2[Foo](...)` where `Foo` is not `Comparable`;
    /// or use of a bound-gated operation (`<`, `==`, ...) on an *unbounded*
    /// type parameter.
    pub const TYPE_UNSATISFIED_BOUND: ErrorCode = "E2015";

    // ── M61b: default + keyword argument binding ──────────────────────────
    /// A keyword argument names a parameter the callee does not declare.
    /// (E2015 is M63b's TYPE_UNSATISFIED_BOUND; this family continues at E2020.)
    pub const TYPE_UNKNOWN_KEYWORD: ErrorCode = "E2020";
    /// A parameter is bound twice — positionally and again by keyword (or by
    /// two keyword arguments of the same name).
    pub const TYPE_DUPLICATE_ARG: ErrorCode = "E2016";
    /// A required (non-defaulted) parameter was left unbound after positional
    /// and keyword arguments were applied.
    pub const TYPE_MISSING_ARG: ErrorCode = "E2017";
    /// A positional argument follows a keyword argument at a call site.
    pub const TYPE_POSITIONAL_AFTER_KEYWORD: ErrorCode = "E2018";
    /// A required parameter (no default) follows a defaulted parameter in a
    /// declaration.
    pub const TYPE_DEFAULT_ORDER: ErrorCode = "E2019";

    // ── M62b: generators (yield) ─────────────────────────────────────────
    /// A `yield`ed expression's type does not match the generator's element
    /// type `T` (the `T` in the function's declared `Iterator[T]` return
    /// type). Allocated from the M62b type band (E2060+).
    pub const TYPE_YIELD_MISMATCH: ErrorCode = "E2060";

    // ── E3xxx: semantic ──────────────────────────────────────────────────
    pub const SEM_NONEXHAUSTIVE_MATCH: ErrorCode = "E3001";
    pub const SEM_UNREACHABLE: ErrorCode = "E3002";

    // ── M62b: generators (yield) — semantic ──────────────────────────────
    /// `yield` used outside any function, or inside a function whose declared
    /// return type is not `Iterator[T]`. Allocated from the M62b semantic
    /// band (E3030+).
    pub const SEM_YIELD_OUTSIDE_GENERATOR: ErrorCode = "E3030";

    // ── E4xxx: linker / module ───────────────────────────────────────────
    pub const LINK_MISSING_MODULE: ErrorCode = "E4001";
    /// `from x import y` where module `x` has no item named `y`,
    /// or `x.y` where `x` is a module but has no such attribute (M19).
    pub const LINK_NO_SUCH_MODULE_ITEM: ErrorCode = "E4002";
    /// Circular user-module import detected (M60). ImportError-style.
    pub const LINK_CIRCULAR_IMPORT: ErrorCode = "E4003";
}
