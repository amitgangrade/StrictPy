//! M61b: shared call-site argument binding for default values + keyword args.
//!
//! Both the type checker and the IR lowerer must resolve a call's argument
//! list — a mix of positional and `name=value` keyword arguments — against a
//! callee's declared parameters, filling in defaults for omitted trailing
//! parameters. To keep the two passes in lock-step (so a call that type-checks
//! always lowers identically), the binding algorithm lives here and is called
//! from both.
//!
//! ## Call-site rules (enforced here)
//!   * Positional arguments bind left-to-right to the leading parameters.
//!   * A positional argument may not follow a keyword argument
//!     (`E2018`, positional-after-keyword).
//!   * Each keyword must name a real parameter (`E2015`, unknown keyword).
//!   * No parameter may be bound twice — positionally *and* by keyword, or by
//!     two keywords of the same name (`E2016`, duplicate).
//!   * Every parameter without a default must end up bound (`E2017`, missing).
//!
//! ## Declaration rule (enforced by [`check_default_order`])
//!   * A required (non-defaulted) parameter may not follow a defaulted one
//!     (`E2019`).
//!
//! The result is a `Vec<Slot>` of length `params.len()`, one entry per
//! parameter in declaration order: either the index of the argument that
//! supplies it, or [`Slot::Default`] meaning "materialise this parameter's
//! default expression".

use crate::ast::{Arg, Param, Span};
use crate::error::{codes, CompileError, ErrorCode};

/// How a single parameter slot is satisfied at a call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// Use `args[idx].value`.
    Arg(usize),
    /// Materialise the parameter's declared default expression.
    Default,
}

/// Minimal per-parameter info the binder needs. Built from either an AST
/// [`Param`] list or a resolved signature (whichever the caller has handy).
#[derive(Debug, Clone)]
pub struct ParamInfo {
    pub name: String,
    pub has_default: bool,
}

impl ParamInfo {
    pub fn from_params(params: &[Param]) -> Vec<ParamInfo> {
        params
            .iter()
            .map(|p| ParamInfo { name: p.name.clone(), has_default: p.default.is_some() })
            .collect()
    }
}

fn err(span: Span, code: ErrorCode, msg: String) -> CompileError {
    CompileError::Type {
        file: String::new(),
        line: span.line,
        col: span.col,
        code,
        message: msg,
    }
}

/// Bind a call's `args` against `params`, returning one [`Slot`] per parameter.
///
/// `callee_desc` is a short noun phrase for diagnostics, e.g. `"function `f`"`
/// or `"constructor of `Point`"` or `"method `scale`"`.
pub fn bind(
    params: &[ParamInfo],
    args: &[Arg],
    span: Span,
    callee_desc: &str,
) -> Result<Vec<Slot>, CompileError> {
    let nparams = params.len();
    let mut slots: Vec<Option<usize>> = vec![None; nparams];

    // Phase 1: positional arguments (everything up to the first keyword).
    let mut first_kw: Option<usize> = None;
    for (i, a) in args.iter().enumerate() {
        if a.name.is_some() {
            first_kw = Some(i);
            break;
        }
        if i >= nparams {
            return Err(err(
                a.span,
                codes::TYPE_ARITY,
                format!(
                    "{callee_desc} takes {nparams} positional argument{}, but {} were given",
                    if nparams == 1 { "" } else { "s" },
                    args.len()
                ),
            ));
        }
        slots[i] = Some(i);
    }

    // Phase 2: keyword arguments.
    let kw_start = first_kw.unwrap_or(args.len());
    for (i, a) in args.iter().enumerate().skip(kw_start) {
        let kw = match &a.name {
            Some(n) => n,
            // A positional argument after a keyword argument.
            None => {
                return Err(err(
                    a.span,
                    codes::TYPE_POSITIONAL_AFTER_KEYWORD,
                    format!(
                        "positional argument follows keyword argument in call to {callee_desc}"
                    ),
                ));
            }
        };
        let pidx = match params.iter().position(|p| &p.name == kw) {
            Some(p) => p,
            None => {
                return Err(err(
                    a.span,
                    codes::TYPE_UNKNOWN_KEYWORD,
                    format!("{callee_desc} has no parameter named `{kw}`"),
                ));
            }
        };
        if slots[pidx].is_some() {
            return Err(err(
                a.span,
                codes::TYPE_DUPLICATE_ARG,
                format!("{callee_desc} got multiple values for parameter `{kw}`"),
            ));
        }
        slots[pidx] = Some(i);
    }

    // Phase 3: fill defaults / report missing required parameters.
    let mut out = Vec::with_capacity(nparams);
    for (pidx, p) in params.iter().enumerate() {
        match slots[pidx] {
            Some(ai) => out.push(Slot::Arg(ai)),
            None if p.has_default => out.push(Slot::Default),
            None => {
                return Err(err(
                    span,
                    codes::TYPE_MISSING_ARG,
                    format!(
                        "{callee_desc} is missing required argument `{}`",
                        p.name
                    ),
                ));
            }
        }
    }
    Ok(out)
}

/// Reject a declaration where a required parameter follows a defaulted one.
/// `skip_leading` lets methods/constructors skip the implicit `self`.
pub fn check_default_order(
    params: &[Param],
    skip_leading: usize,
    decl_desc: &str,
) -> Result<(), CompileError> {
    let mut seen_default = false;
    for p in params.iter().skip(skip_leading) {
        if p.default.is_some() {
            seen_default = true;
        } else if seen_default {
            return Err(err(
                p.span,
                codes::TYPE_DEFAULT_ORDER,
                format!(
                    "non-default parameter `{}` follows a default parameter in {decl_desc}",
                    p.name
                ),
            ));
        }
    }
    Ok(())
}
