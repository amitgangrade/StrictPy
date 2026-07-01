//! Bidirectional type checker. See spec §10.4.
//!
//! Two modes:
//!
//! - **Synthesis** (`synth`): given an expression, compute its type bottom-up.
//! - **Checking** (`check`): given an expression and an expected type, verify
//!   compatibility and propagate the expected type into sub-expressions that
//!   need it (e.g., lambda parameter types, numeric literal widths).
//!
//! Generic inference uses local unification per call site: each type
//! parameter becomes a fresh [`crate::types::TypeVarId`], every argument
//! contributes equality constraints, and the substitution must collapse to
//! a unique solution by the end of the argument list (spec §10.4).

use std::collections::{HashMap, HashSet};

use crate::ast::{
    self, Arg, BinOp, Block, ClassDecl, ComprehensionKind, Expr, FuncDecl, Literal, Lvalue, Span,
    Stmt, TopDecl, UnaryOp,
};
use crate::error::{codes, CompileError, ErrorCode};
use crate::ir::{eval_const_expr, IRConst};
use crate::resolver::{FunctionSig, ResolvedModule, SymbolId, SymbolKind};
use crate::types::{
    is_subtype, is_subtype_trivial, ty_eq, BoundKind, ClassId, ClassLayout, MethodSig, PrimTy,
    ProtoId, ProtocolInfo, Ty, TypeContext, TypeCtor, TypeVarId,
};

#[derive(Debug, Clone)]
pub struct TypedExpr {
    pub expr: Expr,
    pub ty: Ty,
}

#[derive(Debug, Clone)]
pub struct TypedStmt {
    pub stmt: Stmt,
}

#[derive(Debug, Clone)]
pub struct TypedBlock {
    pub block: Block,
}

#[derive(Debug)]
pub struct TypedModule {
    pub resolved: ResolvedModule,
    pub expr_types: HashMap<(u32, u32), Ty>,
    /// M17: every (fn_sym, type_args) pair discovered at a call site during
    /// typecheck. The IR lowerer materialises one mangled function per entry.
    /// Stored as `Vec` (not `HashSet`) because `Ty` is not Hash/Eq.
    /// De-duplicated by `display_ty(type_args)` while building.
    pub instantiations: Vec<(SymbolId, Vec<Ty>)>,
    /// M31: every (class_id, type_args) pair discovered at a constructor site
    /// during typecheck. The IR lowerer materialises one mangled class layout
    /// + per-instantiation method bodies per entry. Same dedup semantics as
    /// `instantiations` above.
    pub class_instantiations: Vec<(ClassId, Vec<Ty>)>,
}

// ─────────────────────────────────────────────────────────────────────────
//  Checker state
// ─────────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct TypeChecker {
    expr_types: HashMap<(u32, u32), Ty>,
    instantiations: Vec<(SymbolId, Vec<Ty>)>,
    /// Set of `(sid, mangled_args_key)` already in `instantiations`, used to
    /// dedupe across call sites (and across transitive monomorphisations).
    instantiation_keys: HashSet<(SymbolId, String)>,
    /// M31: like `instantiations`, but for generic-class constructor sites.
    class_instantiations: Vec<(ClassId, Vec<Ty>)>,
    class_instantiation_keys: HashSet<(ClassId, String)>,
}

/// One frame of local-binding type info.  Used for flow-sensitive narrowing
/// (spec §6.4) — narrowed bindings are stored on top of the base type.
#[derive(Default, Clone)]
struct Env {
    types: HashMap<SymbolId, Ty>,
}

struct Ctx<'a> {
    classes: &'a HashMap<ClassId, ClassLayout>,
    protocols: &'a HashMap<ProtoId, ProtocolInfo>,
    /// Symbol-id → resolved type (base, before any narrowing).
    base_types: HashMap<SymbolId, Ty>,
}

impl<'a> Ctx<'a> {
    fn ty_ctx(&self) -> TypeContext<'_> {
        TypeContext { classes: self.classes, protocols: self.protocols }
    }
}

impl TypeChecker {
    pub fn new() -> Self { Self::default() }

    pub fn check(mut self, resolved: ResolvedModule) -> Result<TypedModule, CompileError> {
        // Build the type context (immutable view of classes/protocols).
        let classes = resolved.class_layouts.clone();
        let protocols = resolved.protocols.clone();

        // Seed base types from every symbol that carries one.
        let mut base_types: HashMap<SymbolId, Ty> = HashMap::new();
        for s in &resolved.symbols.symbols {
            if let Some(t) = &s.ty {
                base_types.insert(s.id, t.clone());
            }
        }
        let mut ctx = Ctx { classes: &classes, protocols: &protocols, base_types };

        // Dict non-`str`-key guard (Lane D / Wave-1 correctness). The runtime
        // dict is hardcoded to string keys (`vm/src/strdict.rs`), but nothing
        // restricted the *type* of K — so `Dict[i64, V]` / `Dict[Tuple, V]`
        // compiled and then SEGFAULTed at subscript. Reject any declared type
        // carrying a non-`str` Dict key up front, at the declaration's span.
        // Inferred dict-literal keys are checked separately at the literal site.
        for s in &resolved.symbols.symbols {
            if let Some(t) = &s.ty {
                if let Some(bad_key) = ty_first_bad_dict_key(t) {
                    return Err(type_err(s.def_span, codes::TYPE_DICT_NON_STR_KEY,
                        format!("Dict key type must be `str`, got `{}` (in `{}`); non-`str` \
                                 dict keys are not supported and would crash at runtime",
                                bad_key.display(), s.name)));
                }
            }
        }

        // Walk top-level decls.
        for decl in resolved.module.decls.clone().iter() {
            match decl {
                TopDecl::Func(f) => {
                    self.check_func(f, &mut ctx, None, &resolved)?;
                }
                TopDecl::Class(c) => {
                    self.check_class(c, &mut ctx, &resolved)?;
                }
                TopDecl::Const(c) => {
                    let expected = ctx.base_types.get(&resolved.symbols
                        .lookup(resolved.module_scope, &c.name).unwrap()).cloned()
                        .unwrap_or(Ty::Never);
                    let env = Env::default();
                    self.check_expr(&c.value, &expected, &env, &ctx, &resolved)?;
                }
                TopDecl::Protocol(_) | TopDecl::TypeAlias(_) => {}
            }
        }

        // Module-level `final` initialisers must be compile-time evaluable:
        // IR lowering folds each const to a literal and substitutes it at
        // every reference site, so an unfoldable initialiser used to lower
        // to `None` — numeric 0 — silently (`final S: f64 = 4.0 * PI * PI`
        // read back as 0.0). Mirror the lowerer's fixed-point fold here
        // (initialisers may reference other consts in any declaration
        // order) and reject whatever is left over as E3003.
        let mut module_consts: HashMap<String, (IRConst, Ty)> = HashMap::new();
        let mut pending: Vec<_> = resolved
            .module
            .decls
            .iter()
            .filter_map(|d| match d {
                TopDecl::Const(c) => Some(c),
                _ => None,
            })
            .collect();
        loop {
            let before = pending.len();
            let mut still_pending = Vec::new();
            for c in pending {
                let ty = ctx
                    .base_types
                    .get(&resolved.symbols.lookup(resolved.module_scope, &c.name).unwrap())
                    .cloned()
                    .unwrap_or(Ty::Never);
                match eval_const_expr(&c.value, &ty, &module_consts) {
                    Some(v) => {
                        module_consts.insert(c.name.clone(), (v, ty));
                    }
                    None => still_pending.push(c),
                }
            }
            pending = still_pending;
            if pending.len() == before {
                break;
            }
        }
        if let Some(c) = pending.first() {
            return Err(CompileError::Semantic {
                file: String::new(),
                line: c.span.line,
                col: c.span.col,
                code: codes::SEM_CONST_INIT_NOT_CONST,
                message: format!(
                    "initialiser of `final {}` cannot be evaluated at compile time; \
                     module-level `final` initialisers must be built from literals, \
                     other `final` consts, unary `+`/`-`/`not`, and binary \
                     arithmetic/bitwise operators, without reference cycles",
                    c.name
                ),
            });
        }

        Ok(TypedModule {
            resolved,
            expr_types: self.expr_types,
            instantiations: self.instantiations,
            class_instantiations: self.class_instantiations,
        })
    }

    fn check_class(&mut self, c: &ClassDecl, ctx: &mut Ctx, r: &ResolvedModule)
        -> Result<(), CompileError>
    {
        let cid = *r.symbols.scopes[r.module_scope.0 as usize].names.get(&c.name)
            .map(|s| r.class_of_symbol.get(s).unwrap())
            .unwrap();
        if let Some(init) = &c.init {
            self.check_func(init, ctx, Some(cid), r)?;
        }
        for m in &c.methods {
            self.check_func(m, ctx, Some(cid), r)?;
        }
        Ok(())
    }

    fn check_func(&mut self, f: &FuncDecl, ctx: &mut Ctx, recv: Option<ClassId>, r: &ResolvedModule)
        -> Result<(), CompileError>
    {
        // Build env: each param symbol → its declared type.  We must find the
        // param symbols by looking up names in the function scope.  Because we
        // didn't track the function scope id by FuncDecl, we discover params
        // by walking ctx.base_types for symbols whose def_span falls inside f.
        // Simpler approach: enumerate symbols whose def_span lies within f's body
        // span — that's noisy.  Instead, we look up by name via a fresh closure:
        // walk the symbol table for symbols whose scope is one whose parent chain
        // contains the receiver class (best-effort).
        //
        // To keep this robust without restructuring the resolver, we make `env`
        // start empty and seed it on the fly inside statements.  Params are
        // pre-loaded via name lookup against ctx.base_types using their symbol ids:
        // we find them by scanning the symbol table for `Param` symbols whose
        // def_span equals each `p.span`.
        // M61b: a non-default parameter may not follow a defaulted one. Skip
        // the implicit `self` of methods/constructors.
        let skip_self = matches!(f.params.first(), Some(p) if p.name == "self") as usize;
        crate::argbind::check_default_order(
            &f.params, skip_self, &format!("`{}`", f.name),
        )?;
        let mut env = Env::default();
        for p in &f.params {
            for sym in &r.symbols.symbols {
                if matches!(sym.kind, SymbolKind::Param)
                    && sym.def_span.start == p.span.start
                    && sym.def_span.end == p.span.end
                {
                    if let Some(t) = &sym.ty {
                        env.types.insert(sym.id, t.clone());
                    }
                    break;
                }
            }
        }
        // M61b: type-check each parameter default against its declared type.
        // Defaults are evaluated at call time *before* parameters bind, so
        // they are checked in an empty local env (they may reference top-level
        // `final` constants and literals, but not other parameters).
        let default_env = Env::default();
        for p in &f.params {
            if let Some(def) = &p.default {
                let pkey = ast_type_span(&p.ty);
                let pty = r.ast_type_to_ty.get(&pkey).cloned().unwrap_or(Ty::Never);
                let got = self.check_or_synth(def, Some(&pty), &default_env, ctx, r)?;
                if !is_subtype(&got, &pty, &ctx.ty_ctx()) {
                    return Err(type_err(p.span, codes::TYPE_MISMATCH,
                        format!("default for parameter `{}`: expected {}, got {}",
                                p.name, pty.display(), got.display())));
                }
            }
        }
        // Compute expected return type.
        let ret_ty = if matches!(f.return_ty, ast::Type::Named { ref name, .. } if name == "None") {
            Ty::Primitive(PrimTy::Unit)
        } else {
            // Use ast_type_to_ty side table.
            let key = ast_type_span(&f.return_ty);
            r.ast_type_to_ty.get(&key).cloned().unwrap_or_else(|| Ty::Never)
        };
        let _ = recv;
        // M62b: a function declared `-> Iterator[T]` is a generator function
        // and must contain at least one `yield`. Otherwise its body could
        // never produce the iterator it promises (v1 has no other way to
        // construct an `Iterator[T]` value).
        if let Ty::Generic { base: TypeCtor::Iterator, .. } = &ret_ty {
            if !block_contains_yield(&f.body) {
                return Err(type_err(f.span, codes::SEM_YIELD_OUTSIDE_GENERATOR,
                    format!("function `{}` is declared `-> Iterator[T]` but contains no `yield`; a generator function must `yield` at least one value", f.name)));
            }
        }
        self.check_block(&f.body, &ret_ty, &env, ctx, r)?;
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Statements
    // ─────────────────────────────────────────────────────────────────────

    fn check_block(&mut self, b: &Block, ret: &Ty, env: &Env, ctx: &Ctx, r: &ResolvedModule)
        -> Result<(), CompileError>
    {
        let mut env = env.clone();
        for stmt in &b.stmts {
            self.check_stmt(stmt, ret, &mut env, ctx, r)?;
        }
        Ok(())
    }

    fn check_stmt(&mut self, s: &Stmt, ret: &Ty, env: &mut Env, ctx: &Ctx, r: &ResolvedModule)
        -> Result<(), CompileError>
    {
        match s {
            Stmt::Let { name, ty, init, span } => {
                let expected_key = ast_type_span(ty);
                let expected = r.ast_type_to_ty.get(&expected_key).cloned().unwrap_or(Ty::Never);
                let got = self.check_or_synth(init, Some(&expected), env, ctx, r)?;
                if !is_subtype(&got, &expected, &ctx.ty_ctx()) {
                    return Err(type_err(*span, codes::TYPE_MISMATCH,
                        format!("let `{}`: expected {}, got {}", name, expected.display(), got.display())));
                }
                // Bind the new local in env.
                if let Some(sid) = r.symbols.scopes.iter().enumerate()
                    .find_map(|(_, sc)| sc.names.get(name).copied())
                {
                    // Find the symbol whose def_span matches.
                    if let Some(sym) = r.symbols.symbols.iter()
                        .find(|s| s.name == *name && s.def_span.start == span.start
                                  && s.def_span.end == span.end)
                    {
                        env.types.insert(sym.id, expected.clone());
                    } else {
                        env.types.insert(sid, expected.clone());
                    }
                }
            }
            Stmt::LetDestructure { names, tys, init, span } => {
                // M14 tuples. Synthesize the RHS first to learn its element
                // types, then verify each per-name annotation (if any) and
                // bind each local in env at the correct elem type.
                let got = self.check_or_synth(init, None, env, ctx, r)?;
                let elem_tys: Vec<Ty> = match &got {
                    Ty::Tuple(ts) => ts.clone(),
                    _ => return Err(type_err(*span, codes::TYPE_MISMATCH,
                        format!("destructuring let requires a tuple RHS, got {}", got.display()))),
                };
                if elem_tys.len() != names.len() {
                    return Err(type_err(*span, codes::TYPE_MISMATCH,
                        format!("destructuring let: RHS has {} elements but {} names",
                            elem_tys.len(), names.len())));
                }
                for (i, n) in names.iter().enumerate() {
                    let elem_ty = &elem_tys[i];
                    if let Some(ast_t) = &tys[i] {
                        let expected = r.ast_type_to_ty.get(&ast_type_span(ast_t))
                            .cloned().unwrap_or(Ty::Never);
                        if !is_subtype(elem_ty, &expected, &ctx.ty_ctx()) {
                            return Err(type_err(*span, codes::TYPE_MISMATCH,
                                format!("destructure `{}`: expected {}, got {}",
                                    n, expected.display(), elem_ty.display())));
                        }
                    }
                    if let Some(sym) = r.symbols.symbols.iter()
                        .find(|s| s.name == *n && s.def_span.start == span.start
                                  && s.def_span.end == span.end)
                    {
                        env.types.insert(sym.id, elem_ty.clone());
                    }
                }
            }
            // Lane B: star-unpack `before, *star, after = xs`. The RHS must be
            // a `List[T]`. Each fixed name binds at `T`; the star name binds
            // at `List[T]` (a fresh list of the middle elements).
            Stmt::LetStarDestructure { before, star, after, init, span } => {
                let got = self.check_or_synth(init, None, env, ctx, r)?;
                let elem = match &got {
                    Ty::Generic { base: TypeCtor::List, args } if args.len() == 1 => args[0].clone(),
                    _ => return Err(type_err(*span, codes::TYPE_MISMATCH,
                        format!("star-unpack requires a List[T] on the right, got {}", got.display()))),
                };
                let star_ty = Ty::Generic { base: TypeCtor::List, args: vec![elem.clone()] };
                let bind = |this: &mut Self, env: &mut Env, name: &str, ty: &Ty| {
                    if let Some(sym) = r.symbols.symbols.iter()
                        .find(|s| s.name == *name && s.def_span.start == span.start
                                  && s.def_span.end == span.end)
                    {
                        env.types.insert(sym.id, ty.clone());
                    }
                    let _ = this;
                };
                for n in before.iter() { bind(self, env, n, &elem); }
                bind(self, env, star, &star_ty);
                for n in after.iter() { bind(self, env, n, &elem); }
            }
            Stmt::Assign { target, value, span } => {
                let lhs_ty = self.lvalue_type(target, env, ctx, r)?;
                let rhs = self.check_or_synth(value, Some(&lhs_ty), env, ctx, r)?;
                if !is_subtype(&rhs, &lhs_ty, &ctx.ty_ctx()) {
                    return Err(type_err(*span, codes::TYPE_MISMATCH,
                        format!("assignment: expected {}, got {}", lhs_ty.display(), rhs.display())));
                }
            }
            Stmt::AugAssign { target, value, op, span } => {
                let lhs_ty = self.lvalue_type(target, env, ctx, r)?;
                let rhs = self.check_or_synth(value, Some(&lhs_ty), env, ctx, r)?;
                // Lane A: `/=` is true division and yields f64, so the target
                // must already be a float — otherwise we would store f64 bits
                // back into an integer slot. (`//=` keeps integer semantics.)
                if matches!(op, BinOp::Div) {
                    let is_float_lhs = matches!(&lhs_ty, Ty::Primitive(p) if p.is_float());
                    if !is_float_lhs {
                        return Err(type_err(*span, codes::TYPE_BINOP_MISMATCH,
                            format!("`/=` performs true (float) division; target `{}` is not a float — use `//=` for integer division", lhs_ty.display())));
                    }
                }
                if !ty_eq(&lhs_ty, &rhs) {
                    return Err(type_err(*span, codes::TYPE_BINOP_MISMATCH,
                        format!("aug-assign type mismatch: {} vs {}", lhs_ty.display(), rhs.display())));
                }
            }
            Stmt::Return { value, span } => {
                // M62b: inside a generator function (return type `Iterator[T]`),
                // a bare `return` is the exhaustion signal — like Python's
                // generators. `return <value>` is not supported (the value
                // would be a StopIteration payload, which v1 doesn't expose).
                if let Ty::Generic { base: TypeCtor::Iterator, .. } = ret {
                    match value {
                        None => {}
                        Some(_) => {
                            return Err(type_err(*span, codes::TYPE_MISMATCH,
                                "return: a generator function (Iterator[T]) may only use a bare `return` to stop iteration — use `yield` to produce values".into()));
                        }
                    }
                    return Ok(());
                }
                match value {
                    Some(v) => {
                        let got = self.check_or_synth(v, Some(ret), env, ctx, r)?;
                        if !is_subtype(&got, ret, &ctx.ty_ctx()) {
                            return Err(type_err(*span, codes::TYPE_MISMATCH,
                                format!("return: expected {}, got {}", ret.display(), got.display())));
                        }
                    }
                    None => {
                        if !matches!(ret, Ty::Primitive(PrimTy::Unit) | Ty::Never) {
                            return Err(type_err(*span, codes::TYPE_MISMATCH,
                                format!("return: expected {}, got None", ret.display())));
                        }
                    }
                }
            }
            Stmt::Yield { value, span } => {
                // M62b: legal only inside a generator function, i.e. one whose
                // declared return type is `Iterator[T]`. The yielded expression
                // must have type `T`.
                match ret {
                    Ty::Generic { base: TypeCtor::Iterator, args } if args.len() == 1 => {
                        let elem = args[0].clone();
                        let got = self.check_or_synth(value, Some(&elem), env, ctx, r)?;
                        if !is_subtype(&got, &elem, &ctx.ty_ctx()) {
                            return Err(type_err(*span, codes::TYPE_YIELD_MISMATCH,
                                format!("yield: expected {}, got {}", elem.display(), got.display())));
                        }
                    }
                    _ => {
                        return Err(type_err(*span, codes::SEM_YIELD_OUTSIDE_GENERATOR,
                            "`yield` is only allowed inside a generator function (one declared `-> Iterator[T]`)".into()));
                    }
                }
            }
            Stmt::If { cond, then_block, elifs, else_block, span } => {
                let cty = self.check_or_synth(cond, Some(&Ty::Primitive(PrimTy::Bool)), env, ctx, r)?;
                if !matches!(cty, Ty::Primitive(PrimTy::Bool)) {
                    return Err(type_err(*span, codes::TYPE_BOOL_REQUIRED,
                        format!("if condition must be bool, got {}", cty.display())));
                }
                // Narrowing: if cond is `x is none` / `x is not none`, narrow within branches.
                let (then_narrow, else_narrow) = narrowings_from_cond(cond, r, env);
                {
                    let mut env2 = env.clone();
                    apply_narrows(&mut env2, &then_narrow);
                    self.check_block(then_block, ret, &env2, ctx, r)?;
                }
                for (ec, eb) in elifs {
                    let cty = self.check_or_synth(ec, Some(&Ty::Primitive(PrimTy::Bool)), env, ctx, r)?;
                    if !matches!(cty, Ty::Primitive(PrimTy::Bool)) {
                        return Err(type_err(*span, codes::TYPE_BOOL_REQUIRED, "elif condition must be bool".into()));
                    }
                    self.check_block(eb, ret, env, ctx, r)?;
                }
                if let Some(eb) = else_block {
                    let mut env2 = env.clone();
                    apply_narrows(&mut env2, &else_narrow);
                    self.check_block(eb, ret, &env2, ctx, r)?;
                }
                // Early-return narrowing: if then-branch always returns, apply else-narrow to env after.
                if block_always_returns(then_block) {
                    apply_narrows(env, &else_narrow);
                }
            }
            Stmt::While { cond, body, else_block, span } => {
                let cty = self.check_or_synth(cond, Some(&Ty::Primitive(PrimTy::Bool)), env, ctx, r)?;
                if !matches!(cty, Ty::Primitive(PrimTy::Bool)) {
                    return Err(type_err(*span, codes::TYPE_BOOL_REQUIRED, "while cond must be bool".into()));
                }
                self.check_block(body, ret, env, ctx, r)?;
                if let Some(eb) = else_block { self.check_block(eb, ret, env, ctx, r)?; }
            }
            Stmt::For { var, var_ty, iter, body, else_block, span } => {
                let iter_ty = self.check_or_synth(iter, None, env, ctx, r)?;
                let key = ast_type_span(var_ty);
                let vty = r.ast_type_to_ty.get(&key).cloned().unwrap_or(Ty::Never);

                // Wave-2 Lane D: `for x: T in obj:` over a user class must
                // implement the iterator protocol (`__iter__` returning an
                // iterator class whose `__next__(self) -> T?` yields the
                // elements). Validate the protocol and that the declared loop
                // var type `T` matches `__next__`'s value type (the nullable's
                // inner type — `none` is the done sentinel, never bound to the
                // loop var). Built-in handle-backed classes and the generic
                // container/iterator/range forms keep their existing handling.
                if let Ty::Class(cid) = &iter_ty {
                    let is_native = ctx.classes.get(cid)
                        .map(|l| l.is_native)
                        .unwrap_or(false);
                    if !is_native {
                        // `__iter__` must exist and return a user class.
                        let iter_ret = lookup_dunder(ctx.classes, *cid, "__iter__")
                            .map(|m| m.ret.clone());
                        let it_cid = match &iter_ret {
                            Some(Ty::Class(it)) => Some(*it),
                            _ => None,
                        };
                        match it_cid {
                            None => {
                                return Err(type_err(*span, codes::TYPE_MISMATCH,
                                    format!("`for` over `{}` requires an `__iter__(self) -> <IteratorClass>` \
                                             method whose return type is a user class implementing \
                                             `__next__`", iter_ty.display())));
                            }
                            Some(it) => {
                                let next_sig = lookup_dunder(ctx.classes, it, "__next__");
                                match next_sig {
                                    None => {
                                        return Err(type_err(*span, codes::TYPE_MISMATCH,
                                            format!("the iterator returned by `{}.__iter__` does not \
                                                     implement `__next__(self) -> T?`", iter_ty.display())));
                                    }
                                    Some(sig) => {
                                        // Element type = `__next__`'s value type
                                        // (unwrap the `T?` nullable; `none` = done).
                                        let elem_ty = match &sig.ret {
                                            Ty::Nullable(inner) => (**inner).clone(),
                                            other => other.clone(),
                                        };
                                        if !is_subtype(&elem_ty, &vty, &ctx.ty_ctx()) {
                                            return Err(type_err(*span, codes::TYPE_MISMATCH,
                                                format!("loop variable `{}` declared `{}` but `__next__` \
                                                         yields `{}`", var, vty.display(), elem_ty.display())));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                let _ = span;
                let mut env2 = env.clone();
                if let Some(sym) = r.symbols.symbols.iter()
                    .find(|s| s.name == *var
                          && matches!(s.kind, SymbolKind::Local)
                          && s.def_span.start == body.span.start.saturating_sub(0)
                          || (s.name == *var && matches!(s.kind, SymbolKind::Local)))
                {
                    env2.types.insert(sym.id, vty);
                }
                self.check_block(body, ret, &env2, ctx, r)?;
                if let Some(eb) = else_block { self.check_block(eb, ret, env, ctx, r)?; }
            }
            Stmt::Match { scrutinee, arms, span } => {
                let scrut_ty = self.check_or_synth(scrutinee, None, env, ctx, r)?;
                let mut has_wildcard = false;
                // Track which class ids are matched by Constructor patterns —
                // used for the sealed-hierarchy exhaustiveness warning.
                let mut matched_classes: HashSet<ClassId> = HashSet::new();
                for arm in arms {
                    let mut env2 = env.clone();
                    match &arm.pattern {
                        ast::Pattern::Wildcard(_) => {
                            has_wildcard = true;
                        }
                        ast::Pattern::Identifier(name, pspan) => {
                            has_wildcard = true;
                            // Bind the identifier to the scrutinee's type.
                            if let Some(sid) = r.symbols.symbols.iter()
                                .find(|s| s.name == *name && s.def_span.start == pspan.start)
                                .map(|s| s.id)
                            {
                                env2.types.insert(sid, scrut_ty.clone());
                            }
                        }
                        ast::Pattern::Constructor { ty, fields, .. } => {
                            // Resolve the pattern's class id and narrow the
                            // scrutinee (if it's a simple ident).
                            let key = ast_type_span(ty);
                            let pat_ty = r.ast_type_to_ty.get(&key).cloned();
                            if let Some(Ty::Class(cid)) = &pat_ty {
                                matched_classes.insert(*cid);
                                // Bind each Identifier sub-pattern to the
                                // corresponding field's declared type.
                                if let Some(layout) = ctx.classes.get(cid).cloned() {
                                    for (i, sub) in fields.iter().enumerate() {
                                        if let ast::Pattern::Identifier(fname, pspan) = sub {
                                            if let Some(finfo) = layout.fields.get(i) {
                                                if let Some(sid) = r.symbols.symbols.iter()
                                                    .find(|s| s.name == *fname && s.def_span.start == pspan.start)
                                                    .map(|s| s.id)
                                                {
                                                    env2.types.insert(sid, finfo.ty.clone());
                                                }
                                            }
                                        }
                                    }
                                }
                                // If the scrutinee was an ident, also narrow
                                // it to the pattern's class within the arm.
                                if let Expr::Ident { span: sspan, .. } = scrutinee {
                                    if let Some(sid) = r.ident_to_symbol.get(&(sspan.start, sspan.end)) {
                                        env2.types.insert(*sid, Ty::Class(*cid));
                                    }
                                }
                            }
                        }
                        ast::Pattern::Tuple(elems, _) => {
                            if let Ty::Tuple(elem_tys) = &scrut_ty {
                                for (i, sub) in elems.iter().enumerate() {
                                    if let ast::Pattern::Identifier(fname, pspan) = sub {
                                        if let Some(elem_ty) = elem_tys.get(i) {
                                            if let Some(sid) = r.symbols.symbols.iter()
                                                .find(|s| s.name == *fname && s.def_span.start == pspan.start)
                                                .map(|s| s.id)
                                            {
                                                env2.types.insert(sid, elem_ty.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        ast::Pattern::Literal(_, _) => {}
                    }
                    self.check_block(&arm.body, ret, &env2, ctx, r)?;
                }
                // M16 exhaustiveness (spec §6.5): if scrutinee is a sealed
                // class, every direct subclass must be matched (or a
                // wildcard provided). Emit a warning to stderr on miss —
                // not a hard error in v0.1.
                if !has_wildcard {
                    if let Ty::Class(cid) = &scrut_ty {
                        if let Some(layout) = ctx.classes.get(cid) {
                            if layout.is_sealed {
                                let missing: Vec<&str> = ctx
                                    .classes
                                    .values()
                                    .filter(|c| c.base == Some(*cid)
                                        && !matched_classes.contains(&c.id))
                                    .map(|c| c.name.as_str())
                                    .collect();
                                if !missing.is_empty() {
                                    eprintln!(
                                        "warning: match on sealed `{}` is non-exhaustive; missing: {} (at byte {}-{})",
                                        layout.name,
                                        missing.join(", "),
                                        span.start, span.end,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            Stmt::Try { body, handlers, else_block, finally_block, .. } => {
                self.check_block(body, ret, env, ctx, r)?;
                for h in handlers {
                    self.check_block(&h.body, ret, env, ctx, r)?;
                }
                if let Some(eb) = else_block { self.check_block(eb, ret, env, ctx, r)?; }
                if let Some(fb) = finally_block { self.check_block(fb, ret, env, ctx, r)?; }
            }
            Stmt::With { expr, body, span, .. } => {
                let res_ty = self.check_or_synth(expr, None, env, ctx, r)?;
                // v1 only knows how to run cleanup (`__exit__`) for `io.File`.
                // Every other context manager used to lower its cleanup to a
                // silent no-op — locks/DB handles/etc. were never released, so
                // RAII was quietly broken. Until general `__enter__`/`__exit__`
                // dispatch lands, reject any non-`io.File` `with` resource with
                // a clear compile error rather than emitting a no-op cleanup.
                let is_file = matches!(
                    &res_ty,
                    Ty::Class(cid) if ctx.classes.get(cid)
                        .map(|l| l.name == "io.File")
                        .unwrap_or(false)
                );
                if !is_file {
                    return Err(type_err(*span, codes::TYPE_UNSUPPORTED_CONTEXT_MANAGER,
                        format!("`with` is only supported for `io.File` resources in v1; \
                                 `{}` has no cleanup wired through `with` (its `__exit__` \
                                 would silently never run). Use an explicit try/finally \
                                 instead.", res_ty.display())));
                }
                self.check_block(body, ret, env, ctx, r)?;
            }
            Stmt::Raise { exc, cause, span } => {
                // `raise X from Y`: the `from` cause used to be parsed then
                // silently dropped (never type-checked, never lowered). Validate
                // it here so a stray `raise E from 42` is rejected, and so its
                // side effects are evaluated; IR lowering folds the cause into
                // the raised exception's message (see `IR::lower_cause_chain`).
                if let Some(c) = cause {
                    let cause_ty = self.check_or_synth(c, None, env, ctx, r)?;
                    if let Ty::Class(cid) = &cause_ty {
                        if !crate::types::class_is_exception(*cid, ctx.classes) {
                            return Err(type_err(*span, codes::TYPE_NOT_AN_EXCEPTION,
                                "`raise X from Y`: the cause `Y` must be an exception value \
                                 (a subclass of `Exception`)".into()));
                        }
                    } else {
                        return Err(type_err(*span, codes::TYPE_NOT_AN_EXCEPTION,
                            "`raise X from Y`: the cause `Y` must be an exception value \
                             (a subclass of `Exception`)".into()));
                    }
                }
                // M15: `raise IOError("msg")` is the supported v0.1 shape.
                // Recognise the (ExceptionName, single-str-arg) pattern and
                // verify the message argument is `str`-typed — without going
                // through the normal class-constructor path, which would
                // demand an __init__ that built-in exception classes don't
                // carry.  See `IR::Stmt::Raise` for the matching materialise
                // pattern.
                if let Expr::Call { callee, args, .. } = exc {
                    if let Expr::Ident { name, .. } = callee.as_ref() {
                        if is_builtin_exception_name(name) {
                            if args.len() != 1 {
                                return Err(type_err(*span, codes::TYPE_ARITY,
                                    format!("`raise {}(...)` takes exactly 1 string argument", name)));
                            }
                            let _ = self.check_expr(&args[0].value, &Ty::Primitive(PrimTy::Str), env, ctx, r)?;
                            // Stash the call's type as the exception class so
                            // IR lowering can find the class id via expr_types.
                            if let Some(sid) = r.symbols.lookup(r.module_scope, name) {
                                if let Some(cid) = r.symbols.get(sid).class_id {
                                    self.expr_types.insert(
                                        match callee.as_ref() {
                                            Expr::Ident { span, .. } => (span.start, span.end),
                                            _ => (0, 0),
                                        },
                                        Ty::Class(cid),
                                    );
                                }
                            }
                            // Note the exception-call span too so IR can identify it.
                            return Ok(());
                        }
                        // M63a: `raise MyError(args...)` where `MyError` is a
                        // user-defined class.  It must transitively descend
                        // from the built-in `Exception` base; otherwise only
                        // exception types may be raised.  We type-check the
                        // constructor call through the normal class path (which
                        // validates the `__init__` arity / argument types and
                        // records `expr_types[name] = Ty::Class(cid)` for IR).
                        if let Some(sid) = r.symbols.lookup(r.module_scope, name) {
                            if let Some(cid) = r.symbols.get(sid).class_id {
                                if !crate::types::class_is_exception(cid, ctx.classes) {
                                    return Err(type_err(*span, codes::TYPE_NOT_AN_EXCEPTION,
                                        format!("cannot raise `{}`: only subclasses of `Exception` \
                                                 may be raised", name)));
                                }
                                // Exception subclass — fall through to the
                                // normal constructor type-check below.
                            }
                        }
                    }
                }
                // M63a: type-check the raised expression (a user-exception
                // constructor call, or a bound exception value being
                // re-raised).  For a value being raised directly we require it
                // to be an exception-typed class so a stray `raise 42` is
                // rejected at compile time.
                let raised_ty = self.check_or_synth(exc, None, env, ctx, r)?;
                if let Ty::Class(cid) = &raised_ty {
                    if !crate::types::class_is_exception(*cid, ctx.classes) {
                        return Err(type_err(*span, codes::TYPE_NOT_AN_EXCEPTION,
                            "cannot raise a non-exception value: only subclasses of \
                             `Exception` may be raised".into()));
                    }
                }
            }
            Stmt::Assert { cond, msg, span } => {
                // The example syntax `assert(cond, msg)` parses as `assert <tuple>`.
                // Unpack a 2-tuple into (cond, msg) for ergonomic parity with the spec form.
                let (real_cond, real_msg): (&Expr, Option<&Expr>) = match cond {
                    Expr::Tuple { elems, .. } if elems.len() == 2 && msg.is_none() => {
                        (&elems[0], Some(&elems[1]))
                    }
                    Expr::Tuple { elems, .. } if elems.len() == 1 => (&elems[0], msg.as_ref()),
                    _ => (cond, msg.as_ref()),
                };
                let cty = self.check_or_synth(real_cond, Some(&Ty::Primitive(PrimTy::Bool)), env, ctx, r)?;
                if !matches!(cty, Ty::Primitive(PrimTy::Bool)) {
                    return Err(type_err(*span, codes::TYPE_BOOL_REQUIRED,
                        "assert cond must be bool".into()));
                }
                if let Some(m) = real_msg {
                    let _ = self.check_or_synth(m, Some(&Ty::Primitive(PrimTy::Str)), env, ctx, r)?;
                }
            }
            Stmt::Expr { expr, .. } => {
                let _ = self.check_or_synth(expr, None, env, ctx, r)?;
            }
            Stmt::Del { target, span } => {
                // Spec §7.5 `del_stmt`. v1 implements deletion for Dict
                // entries only (`del d[k]` lowers to NativeFn::DictRemove).
                // Any other target has no runtime deletion path, and the IR
                // used to lower the whole statement to nothing — a silent
                // no-op `del` is worse than a missing feature, so reject
                // everything we can't actually delete.
                match target {
                    Lvalue::Index { obj, indices, .. } => {
                        let obj_ty = self.synth_expr(obj, env, ctx, r)?;
                        match &obj_ty {
                            Ty::Generic { base: TypeCtor::Dict, args: a } if a.len() == 2 => {
                                if indices.len() != 1 {
                                    return Err(type_err(*span, codes::TYPE_ARITY,
                                        "del on a Dict takes exactly one key: `del d[k]`".into()));
                                }
                                let _ = self.check_expr(&indices[0], &a[0], env, ctx, r)?;
                            }
                            other => {
                                return Err(type_err(*span, codes::TYPE_MISMATCH,
                                    format!("del is only supported on Dict entries \
                                             (`del d[k]`); cannot delete from {}",
                                            other.display())));
                            }
                        }
                    }
                    _ => {
                        return Err(type_err(*span, codes::TYPE_MISMATCH,
                            "del is only supported on Dict entries (`del d[k]`); \
                             deleting names or attributes is not part of v1".into()));
                    }
                }
            }
            Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::Pass { .. } => {}
        }
        Ok(())
    }

    fn lvalue_type(&mut self, lv: &Lvalue, env: &Env, ctx: &Ctx, r: &ResolvedModule)
        -> Result<Ty, CompileError>
    {
        match lv {
            Lvalue::Ident { name, span } => {
                let key = (span.start, span.end);
                if let Some(sid) = r.ident_to_symbol.get(&key) {
                    if let Some(t) = env.types.get(sid) { return Ok(t.clone()); }
                    if let Some(t) = ctx.base_types.get(sid) { return Ok(t.clone()); }
                }
                // Fall back to scope lookup.
                if let Some(sid) = r.symbols.lookup(r.module_scope, name) {
                    if let Some(t) = ctx.base_types.get(&sid) { return Ok(t.clone()); }
                }
                Err(type_err(*span, codes::RESOLVE_UNDEFINED,
                    format!("unknown name `{}`", name)))
            }
            Lvalue::Attr { obj, name, span } => {
                let obj_ty = self.synth_expr(obj, env, ctx, r)?;
                self.attr_type(&obj_ty, name, *span, ctx)
            }
            Lvalue::Index { obj, indices, span } => {
                let obj_ty = self.synth_expr(obj, env, ctx, r)?;
                // Wave 2 / Lane C: subscript-store on a user class dispatches to
                // `__setitem__(self, key, value)`. Check the key against the
                // dunder's first declared parameter, and return its *value*
                // parameter type so the surrounding assignment checks the RHS
                // against it. Falls through to `index_type` for built-in
                // containers and for classes lacking `__setitem__`. A
                // parameterised receiver is `Ty::Generic { Class(cid), .. }`.
                if let Some(cid) = class_cid_of(&obj_ty) {
                    if let Some(res) =
                        self.class_index_set_type(cid, &obj_ty, indices, *span, env, ctx, r)
                    {
                        return res;
                    }
                }
                for i in indices { let _ = self.synth_expr(i, env, ctx, r)?; }
                self.index_type(&obj_ty, *span)
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Expressions: synth / check
    // ─────────────────────────────────────────────────────────────────────

    fn check_or_synth(&mut self, e: &Expr, expected: Option<&Ty>, env: &Env, ctx: &Ctx, r: &ResolvedModule)
        -> Result<Ty, CompileError>
    {
        match expected {
            Some(exp) => self.check_expr(e, exp, env, ctx, r),
            None => self.synth_expr(e, env, ctx, r),
        }
    }

    /// Resolve the type an integer-literal operand would take under an
    /// `expected` type, WITHOUT applying the out-of-range check. Used by the
    /// unary-minus path so a negated literal can be range-checked against its
    /// signed value (see `check_expr`'s `Unary { Neg, .. }` arm). Mirrors the
    /// suffix/expected/default-i64 resolution in `check_expr`'s int-literal arm.
    fn synth_int_literal_ty_no_range(&self, operand: &Expr, expected: &Ty) -> Ty {
        if let Expr::Literal { lit: Literal::Int { suffix, .. }, .. } = operand {
            if let Some(s) = suffix {
                return Ty::Primitive(int_suffix_to_prim(*s));
            }
            if let Ty::Primitive(p) = expected {
                if p.is_numeric() { return Ty::Primitive(*p); }
            } else if let Ty::Nullable(inner) = expected {
                if let Ty::Primitive(p) = inner.as_ref() {
                    if p.is_numeric() { return Ty::Primitive(*p); }
                }
            }
        }
        Ty::Primitive(PrimTy::I64)
    }

    fn check_expr(&mut self, e: &Expr, expected: &Ty, env: &Env, ctx: &Ctx, r: &ResolvedModule)
        -> Result<Ty, CompileError>
    {
        // Special-case literals: an int literal with no suffix takes on expected width.
        if let Expr::Literal { lit, span } = e {
            let lit_ty = match lit {
                Literal::Int { suffix, .. } => {
                    // Lane A: bare integer literals default to i64 (spec §3).
                    if let Some(s) = suffix {
                        Ty::Primitive(int_suffix_to_prim(*s))
                    } else if let Ty::Primitive(p) = expected {
                        if p.is_numeric() { Ty::Primitive(*p) } else { Ty::Primitive(PrimTy::I64) }
                    } else if let Ty::Nullable(inner) = expected {
                        if let Ty::Primitive(p) = inner.as_ref() {
                            if p.is_numeric() { Ty::Primitive(*p) } else { Ty::Primitive(PrimTy::I64) }
                        } else { Ty::Primitive(PrimTy::I64) }
                    } else {
                        Ty::Primitive(PrimTy::I64)
                    }
                }
                Literal::Float { suffix, .. } => {
                    if let Some(crate::lexer::FloatSuffix::F32) = suffix {
                        Ty::Primitive(PrimTy::F32)
                    } else if let Ty::Primitive(PrimTy::F32) = expected {
                        Ty::Primitive(PrimTy::F32)
                    } else {
                        Ty::Primitive(PrimTy::F64)
                    }
                }
                Literal::Str(_) => Ty::Primitive(PrimTy::Str),
                Literal::Bytes(_) => Ty::Primitive(PrimTy::Bytes),
                Literal::Char(_) => Ty::Primitive(PrimTy::Char),
                Literal::Bool(_) => Ty::Primitive(PrimTy::Bool),
                Literal::None => Ty::Primitive(PrimTy::Null),
            };
            // Wave-2 Lane F: a too-large integer literal used to truncate
            // silently to its resolved integer width at IR materialisation;
            // make it a clean compile error here, while the full-precision
            // value is still in hand.
            if let (Literal::Int { value, .. }, Ty::Primitive(p)) = (lit, &lit_ty) {
                check_int_literal_in_range(*value, *p, *span)?;
            }
            self.expr_types.insert((span.start, span.end), lit_ty.clone());
            if !is_subtype(&lit_ty, expected, &ctx.ty_ctx()) {
                return Err(type_err(*span, codes::TYPE_MISMATCH,
                    format!("literal of type {} doesn't match expected {}",
                            lit_ty.display(), expected.display())));
            }
            return Ok(lit_ty);
        }
        // A unary `-` / `+` applied directly to a numeric literal should coerce
        // the literal to the expected width too, so `x: i64 = -1` and
        // `range(a, b, -1)` behave like their positive counterparts. Without
        // this, `-1` is `Unary(Neg, Int)` (not a bare literal), so it misses
        // the branch above, the inner literal defaults to i32, and the negation
        // is rejected against an i64 context. Restricted to literal operands so
        // `-someVar` keeps its operand's real type.
        if let Expr::Unary { op: uop @ (UnaryOp::Neg | UnaryOp::Pos), operand, span } = e {
            if matches!(
                operand.as_ref(),
                Expr::Literal { lit: Literal::Int { .. } | Literal::Float { .. }, .. }
            ) {
                // Wave-2 Lane F: range-check a *negated* integer literal against
                // the negated value, so `-9223372036854775808` (== i64::MIN) and
                // `-128i8` are accepted even though the bare magnitude
                // (9223372036854775808 / 128) is out of range. We resolve the
                // operand's width without triggering the per-literal range check
                // (which only knows the positive magnitude), then validate the
                // signed value here.
                if let Expr::Literal { lit: Literal::Int { value, .. }, span: inner_span } =
                    operand.as_ref()
                {
                    let inner_ty = self.synth_int_literal_ty_no_range(operand, expected);
                    if let Ty::Primitive(p) = &inner_ty {
                        let signed = if matches!(uop, UnaryOp::Neg) { -*value } else { *value };
                        check_int_literal_in_range(signed, *p, *span)?;
                        self.expr_types.insert((inner_span.start, inner_span.end), inner_ty.clone());
                        self.expr_types.insert((span.start, span.end), inner_ty.clone());
                        return Ok(inner_ty);
                    }
                }
                let inner = self.check_expr(operand, expected, env, ctx, r)?;
                self.expr_types.insert((span.start, span.end), inner.clone());
                return Ok(inner);
            }
        }
        // Lane A: an arithmetic binop checked against a numeric primitive
        // expectation propagates that width into literal operands, so
        // `x: i32 = 1 + 2` keeps `1` and `2` at i32 (they would otherwise
        // default to i64 and the i64 result would fail the i32 subtype
        // check). Only kicks in when BOTH operands can adopt the expected
        // type (numeric literals, or operands already of that type); mixed
        // non-literal operands fall through to the widening synth path.
        if let Expr::Binary { op, lhs, rhs, span } = e {
            let is_arith = matches!(op,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::FloorDiv
                | BinOp::Rem | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor);
            // `/` always yields f64, so it can only satisfy an f64 expectation;
            // let the generic path produce f64 and subtype-check it.
            if is_arith {
                if let Ty::Primitive(ep) = expected {
                    if ep.is_numeric()
                        && operand_can_adopt(lhs, *ep)
                        && operand_can_adopt(rhs, *ep)
                    {
                        let lt = self.check_expr(lhs, expected, env, ctx, r)?;
                        let rt = self.check_expr(rhs, expected, env, ctx, r)?;
                        if matches!(lt, Ty::Primitive(p) if p == *ep)
                            && matches!(rt, Ty::Primitive(p) if p == *ep)
                        {
                            self.expr_types.insert((span.start, span.end), expected.clone());
                            return Ok(expected.clone());
                        }
                    }
                }
            }
        }
        // For collection literals, push expected type element-wise.
        match e {
            Expr::List { elems, span } => {
                let elem_expected = match expected {
                    Ty::Generic { base: TypeCtor::List, args } if args.len() == 1 => Some(args[0].clone()),
                    _ => None,
                };
                let mut elem_ty = elem_expected.clone();
                for el in elems {
                    let t = self.check_or_synth(el, elem_ty.as_ref(), env, ctx, r)?;
                    if elem_ty.is_none() { elem_ty = Some(t); }
                }
                let elem = elem_ty.unwrap_or(Ty::Never);
                let ty = Ty::Generic { base: TypeCtor::List, args: vec![elem] };
                self.expr_types.insert((span.start, span.end), ty.clone());
                if !is_subtype(&ty, expected, &ctx.ty_ctx()) {
                    return Err(type_err(*span, codes::TYPE_MISMATCH,
                        format!("expected {}, got {}", expected.display(), ty.display())));
                }
                return Ok(ty);
            }
            Expr::Set { elems, span } => {
                // Mirror the List branch: push the expected element type
                // into each element so `s: Set[i64] = {0}` checks the
                // literal against i64 instead of synthesising Set[i32].
                let elem_expected = match expected {
                    Ty::Generic { base: TypeCtor::Set, args } if args.len() == 1 => Some(args[0].clone()),
                    _ => None,
                };
                let mut elem_ty = elem_expected.clone();
                for el in elems {
                    let t = self.check_or_synth(el, elem_ty.as_ref(), env, ctx, r)?;
                    if elem_ty.is_none() { elem_ty = Some(t); }
                }
                let elem = elem_ty.unwrap_or(Ty::Never);
                check_set_elem_ty(&elem, *span)?;
                let ty = Ty::Generic { base: TypeCtor::Set, args: vec![elem] };
                self.expr_types.insert((span.start, span.end), ty.clone());
                if !is_subtype(&ty, expected, &ctx.ty_ctx()) {
                    return Err(type_err(*span, codes::TYPE_MISMATCH,
                        format!("expected {}, got {}", expected.display(), ty.display())));
                }
                return Ok(ty);
            }
            // Lane A: push expected element types into a tuple literal so
            // `t: Tuple[i32, i32] = (42, 42)` checks each `42` against i32
            // instead of synthesising Tuple[i64, i64] (bare ints now default
            // to i64). Mirrors the List/Set/Dict branches above. Only applies
            // when the expected type is a tuple of matching arity.
            Expr::Tuple { elems, span } => {
                if let Ty::Tuple(exp_elems) = expected {
                    if exp_elems.len() == elems.len() {
                        let mut tys = Vec::with_capacity(elems.len());
                        for (el, exp) in elems.iter().zip(exp_elems) {
                            tys.push(self.check_expr(el, exp, env, ctx, r)?);
                        }
                        let ty = Ty::Tuple(tys);
                        self.expr_types.insert((span.start, span.end), ty.clone());
                        if !is_subtype(&ty, expected, &ctx.ty_ctx()) {
                            return Err(type_err(*span, codes::TYPE_MISMATCH,
                                format!("expected {}, got {}", expected.display(), ty.display())));
                        }
                        return Ok(ty);
                    }
                }
                // Fall through to the generic synth+subtype path.
                let got = self.synth_expr(e, env, ctx, r)?;
                if !is_subtype(&got, expected, &ctx.ty_ctx()) {
                    return Err(type_err(*span, codes::TYPE_MISMATCH,
                        format!("expected {}, got {}", expected.display(), got.display())));
                }
                return Ok(got);
            }
            Expr::Dict { entries, span } => {
                let (kexp, vexp) = match expected {
                    Ty::Generic { base: TypeCtor::Dict, args } if args.len() == 2 => {
                        (Some(args[0].clone()), Some(args[1].clone()))
                    }
                    _ => (None, None),
                };
                let mut kty = kexp.clone();
                let mut vty = vexp.clone();
                for (k, v) in entries {
                    let tk = self.check_or_synth(k, kty.as_ref(), env, ctx, r)?;
                    let tv = self.check_or_synth(v, vty.as_ref(), env, ctx, r)?;
                    if kty.is_none() { kty = Some(tk); }
                    if vty.is_none() { vty = Some(tv); }
                }
                let kk = kty.unwrap_or(Ty::Never);
                let vv = vty.unwrap_or(Ty::Never);
                if !is_valid_dict_key(&kk) {
                    return Err(type_err(*span, codes::TYPE_DICT_NON_STR_KEY,
                        format!("Dict key type must be `str`, got `{}`; non-`str` dict keys \
                                 are not supported and would crash at runtime", kk.display())));
                }
                let ty = Ty::Generic { base: TypeCtor::Dict, args: vec![kk, vv] };
                self.expr_types.insert((span.start, span.end), ty.clone());
                return Ok(ty);
            }
            Expr::Comprehension { .. } => {
                let ty = self.check_comprehension(e, Some(expected), env, ctx, r)?;
                if !is_subtype(&ty, expected, &ctx.ty_ctx()) {
                    return Err(type_err(expr_span(e), codes::TYPE_COMPREHENSION_ELEM_MISMATCH,
                        format!("expected {}, got {}", expected.display(), ty.display())));
                }
                return Ok(ty);
            }
            _ => {}
        }

        let got = self.synth_expr(e, env, ctx, r)?;
        if !is_subtype(&got, expected, &ctx.ty_ctx()) {
            // Allow promotion of literal numeric types via re-synthesis if the
            // expected type is numeric and we got a default. Already handled above.
            return Err(type_err(expr_span(e), codes::TYPE_MISMATCH,
                format!("expected {}, got {}", expected.display(), got.display())));
        }
        Ok(got)
    }

    fn synth_expr(&mut self, e: &Expr, env: &Env, ctx: &Ctx, r: &ResolvedModule)
        -> Result<Ty, CompileError>
    {
        let ty = self.synth_expr_inner(e, env, ctx, r)?;
        let key = (expr_span(e).start, expr_span(e).end);
        self.expr_types.insert(key, ty.clone());
        Ok(ty)
    }

    fn synth_expr_inner(&mut self, e: &Expr, env: &Env, ctx: &Ctx, r: &ResolvedModule)
        -> Result<Ty, CompileError>
    {
        match e {
            Expr::Literal { lit, span } => {
                let lit_ty = match lit {
                    Literal::Int { suffix, .. } => {
                        // Lane A: bare (unsuffixed) integer literals default to i64
                        // (spec §3 "0 // i64 by default"). Previously i32.
                        if let Some(s) = suffix { Ty::Primitive(int_suffix_to_prim(*s)) }
                        else { Ty::Primitive(PrimTy::I64) }
                    }
                    Literal::Float { suffix, .. } => {
                        if let Some(crate::lexer::FloatSuffix::F32) = suffix {
                            Ty::Primitive(PrimTy::F32)
                        } else { Ty::Primitive(PrimTy::F64) }
                    }
                    Literal::Str(_) => Ty::Primitive(PrimTy::Str),
                    Literal::Bytes(_) => Ty::Primitive(PrimTy::Bytes),
                    Literal::Char(_) => Ty::Primitive(PrimTy::Char),
                    Literal::Bool(_) => Ty::Primitive(PrimTy::Bool),
                    Literal::None => Ty::Primitive(PrimTy::Null),
                };
                // Wave-2 Lane F: reject a bare integer literal that overflows
                // its default i64 width (was silently truncated at lowering).
                if let (Literal::Int { value, .. }, Ty::Primitive(p)) = (lit, &lit_ty) {
                    check_int_literal_in_range(*value, *p, *span)?;
                }
                Ok(lit_ty)
            }
            Expr::Ident { name, span } => {
                let key = (span.start, span.end);
                if let Some(sid) = r.ident_to_symbol.get(&key) {
                    if let Some(t) = env.types.get(sid) { return Ok(t.clone()); }
                    if let Some(t) = ctx.base_types.get(sid) { return Ok(t.clone()); }
                }
                // Fallback module scope.
                if let Some(sid) = r.symbols.lookup(r.module_scope, name) {
                    if let Some(t) = ctx.base_types.get(&sid) { return Ok(t.clone()); }
                }
                Err(type_err(*span, codes::RESOLVE_UNDEFINED,
                    format!("unknown name `{}`", name)))
            }
            Expr::Tuple { elems, .. } => {
                let mut ts = Vec::new();
                for el in elems { ts.push(self.synth_expr(el, env, ctx, r)?); }
                Ok(Ty::Tuple(ts))
            }
            Expr::List { elems, .. } => {
                let mut elem_ty: Option<Ty> = None;
                for el in elems {
                    let t = self.synth_expr(el, env, ctx, r)?;
                    elem_ty = Some(match elem_ty {
                        None => t,
                        Some(prev) => lub(&prev, &t).unwrap_or(prev),
                    });
                }
                Ok(Ty::Generic { base: TypeCtor::List, args: vec![elem_ty.unwrap_or(Ty::Never)] })
            }
            Expr::Set { elems, span } => {
                let mut elem_ty: Option<Ty> = None;
                for el in elems {
                    // Unsuffixed int literals default to i64 here (guide §2:
                    // `42` defaults to i64) — without this `{0}` synthesises
                    // Set[i32] and is rejected against every i64-flavoured
                    // context. Suffixed literals and non-literal elements
                    // keep their own type.
                    let t = if is_unsuffixed_int_literal(el) {
                        self.check_expr(el, &Ty::Primitive(PrimTy::I64), env, ctx, r)?
                    } else {
                        self.synth_expr(el, env, ctx, r)?
                    };
                    elem_ty = Some(match elem_ty {
                        None => t,
                        Some(prev) => lub(&prev, &t).unwrap_or(prev),
                    });
                }
                let elem = elem_ty.unwrap_or(Ty::Never);
                check_set_elem_ty(&elem, *span)?;
                Ok(Ty::Generic { base: TypeCtor::Set, args: vec![elem] })
            }
            Expr::Dict { entries, span } => {
                let mut kty: Option<Ty> = None;
                let mut vty: Option<Ty> = None;
                for (k, v) in entries {
                    kty = Some(self.synth_expr(k, env, ctx, r)?);
                    vty = Some(self.synth_expr(v, env, ctx, r)?);
                }
                let kk = kty.unwrap_or(Ty::Never);
                if !is_valid_dict_key(&kk) {
                    return Err(type_err(*span, codes::TYPE_DICT_NON_STR_KEY,
                        format!("Dict key type must be `str`, got `{}`; non-`str` dict keys \
                                 are not supported and would crash at runtime", kk.display())));
                }
                Ok(Ty::Generic { base: TypeCtor::Dict, args: vec![
                    kk, vty.unwrap_or(Ty::Never)] })
            }
            Expr::Unary { op, operand, span } => {
                // Wave-2 Lane F: a negated bare int literal (`-9223372036854775808`)
                // is range-checked against its *signed* value so i64::MIN and the
                // other type minima are accepted. Synthesis has no expected width,
                // so the operand defaults to i64. Handle this before the generic
                // recursion, which would otherwise reject the positive magnitude.
                if matches!(op, UnaryOp::Neg) {
                    if let Expr::Literal { lit: Literal::Int { value, suffix }, span: inner_span } =
                        operand.as_ref()
                    {
                        let p = match suffix {
                            Some(s) => int_suffix_to_prim(*s),
                            None => PrimTy::I64,
                        };
                        check_int_literal_in_range(-*value, p, *span)?;
                        self.expr_types.insert((inner_span.start, inner_span.end), Ty::Primitive(p));
                        let key = (span.start, span.end);
                        self.expr_types.insert(key, Ty::Primitive(p));
                        return Ok(Ty::Primitive(p));
                    }
                }
                let t = self.synth_expr(operand, env, ctx, r)?;
                match op {
                    UnaryOp::Not => {
                        if !matches!(t, Ty::Primitive(PrimTy::Bool)) {
                            return Err(type_err(*span, codes::TYPE_BOOL_REQUIRED,
                                format!("`not` requires bool, got {}", t.display())));
                        }
                        Ok(Ty::Primitive(PrimTy::Bool))
                    }
                    UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot => {
                        if let Ty::Primitive(p) = t {
                            if p.is_numeric() { return Ok(Ty::Primitive(p)); }
                        }
                        Err(type_err(*span, codes::TYPE_BINOP_MISMATCH,
                            "unary operator requires numeric operand".into()))
                    }
                }
            }
            Expr::Binary { op, lhs, rhs, span } => self.check_binary(*op, lhs, rhs, *span, env, ctx, r),
            Expr::NullCoalesce { lhs, rhs, span } => {
                let lt = self.synth_expr(lhs, env, ctx, r)?;
                let inner = match &lt {
                    Ty::Nullable(t) => (**t).clone(),
                    Ty::Primitive(PrimTy::Null) => Ty::Never,
                    other => other.clone(),
                };
                let rt = self.check_expr(rhs, &inner, env, ctx, r)?;
                let _ = span;
                Ok(rt)
            }
            Expr::Ternary { cond, then_expr, else_expr, span } => {
                let ct = self.check_or_synth(cond, Some(&Ty::Primitive(PrimTy::Bool)), env, ctx, r)?;
                if !matches!(ct, Ty::Primitive(PrimTy::Bool)) {
                    return Err(type_err(*span, codes::TYPE_BOOL_REQUIRED,
                        "ternary condition must be bool".into()));
                }
                let a = self.synth_expr(then_expr, env, ctx, r)?;
                let b = self.synth_expr(else_expr, env, ctx, r)?;
                lub(&a, &b).ok_or_else(|| type_err(*span, codes::TYPE_MISMATCH,
                    format!("ternary branches differ: {} vs {}", a.display(), b.display())))
            }
            Expr::Call { callee, args, span } => self.synth_call(callee, args, *span, env, ctx, r),
            Expr::MethodCall { receiver, method, args, span } => {
                // M19: `sys.exit(0)` parses as MethodCall (the postfix
                // parser folds `Attr + LParen` into MethodCall). If the
                // receiver is a builtin-module Ident, dispatch to the
                // module's item *before* synth'ing the receiver — which
                // would fail because builtin-module symbols carry no
                // type.
                if let Expr::Ident { name: mname, .. } = receiver.as_ref() {
                    if let Some(sid) = r.symbols.lookup(r.module_scope, mname) {
                        if matches!(r.symbols.get(sid).kind, SymbolKind::BuiltinModule) {
                            let mod_name = r.module_alias.get(&sid).cloned()
                                .unwrap_or_else(|| mname.clone());
                            if let Some(m) = r.stdlib_modules.get(&mod_name) {
                                if let Some(item) = m.find(method) {
                                    if let Ty::Function { params, ret } = &item.ty {
                                        if args.len() != params.len() {
                                            return Err(type_err(*span, codes::TYPE_ARITY,
                                                format!(
                                                    "{}.{}() expects {} arg(s), got {}",
                                                    mod_name, method,
                                                    params.len(), args.len(),
                                                )));
                                        }
                                        for (a, pt) in args.iter().zip(params.iter()) {
                                            let _ = self.check_expr(&a.value, pt, env, ctx, r)?;
                                        }
                                        return Ok((**ret).clone());
                                    }
                                    return Err(type_err(*span, codes::TYPE_NOT_CALLABLE,
                                        format!(
                                            "{}.{} is not callable (it's a constant)",
                                            mod_name, method
                                        )));
                                }
                                // M20a: legacy fall-through. The pre-M19
                                // `io` BuiltinModule symbol pre-exists in
                                // the prelude (so `io.File` resolves);
                                // method calls on it for items not in the
                                // stdlib table should keep working. Pass
                                // to the regular receiver path.
                                let joined = format!("{}.{}", mname, method);
                                if r.symbols.lookup(r.module_scope, &joined).is_none() {
                                    return Err(type_err(*span,
                                        codes::LINK_NO_SUCH_MODULE_ITEM,
                                        format!(
                                            "module `{}` has no item named `{}`",
                                            mod_name, method
                                        )));
                                }
                                // else: fall through to synth_method_call.
                            }
                        }
                    }
                }
                let recv_ty = self.synth_expr(receiver, env, ctx, r)?;
                self.synth_method_call(&recv_ty, method, args, *span, env, ctx, r)
            }
            Expr::Attr { obj, name, span } => {
                // Module attr access: `sys.argv`, `io.File`, etc.  The
                // obj here is an `Expr::Ident` whose symbol is either
                //   * a BuiltinModule introduced via `import sys`
                //     (M19) → look up the item in `stdlib_modules`,
                //   * or a legacy flat-named alias like `io` whose
                //     `io.File` is registered in the prelude as a
                //     symbol with the joined name.
                if let Expr::Ident { name: mname, .. } = obj.as_ref() {
                    if let Some(sid) = r.symbols.lookup(r.module_scope, mname) {
                        if matches!(r.symbols.get(sid).kind, SymbolKind::BuiltinModule) {
                            // M19: try the proper stdlib module table
                            // first. The module backing this alias is
                            // recorded in `module_alias` (set by
                            // `register_top_decls` for `import sys`).
                            let mod_name = r.module_alias.get(&sid).cloned()
                                .unwrap_or_else(|| mname.clone());
                            if let Some(m) = r.stdlib_modules.get(&mod_name) {
                                if let Some(item) = m.find(name) {
                                    return Ok(item.ty.clone());
                                }
                                // M20a: try legacy flattened-name path
                                // *before* erroring out — `io.File` was
                                // pre-registered in the prelude under the
                                // joined symbol "io.File" since M5, and
                                // the new `io` stdlib module doesn't list
                                // `File` (it's a class, not a NativeFn).
                                let joined = format!("{}.{}", mname, name);
                                if let Some(s2) = r.symbols.lookup(r.module_scope, &joined) {
                                    if let Some(t) = ctx.base_types.get(&s2) { return Ok(t.clone()); }
                                }
                                return Err(type_err(*span,
                                    codes::LINK_NO_SUCH_MODULE_ITEM,
                                    format!(
                                        "module `{}` has no attribute `{}` (available: {})",
                                        mod_name, name,
                                        m.items.iter().map(|i| i.name.as_str())
                                            .collect::<Vec<_>>().join(", "),
                                    )));
                            }
                            // Legacy fall-through: flattened name like "io.File".
                            let joined = format!("{}.{}", mname, name);
                            if let Some(s2) = r.symbols.lookup(r.module_scope, &joined) {
                                if let Some(t) = ctx.base_types.get(&s2) { return Ok(t.clone()); }
                            }
                            // Fallback: assume the module attr is well-known but unmodeled.
                            return Ok(Ty::Never);
                        }
                    }
                }
                let obj_ty = self.synth_expr(obj, env, ctx, r)?;
                self.attr_type(&obj_ty, name, *span, ctx)
            }
            Expr::Index { obj, indices, span } => {
                let obj_ty = self.synth_expr(obj, env, ctx, r)?;
                // Generic instantiation: `Channel[i32]`, `List[i32]`, etc. —
                // when the obj resolves to an unparameterized container constructor,
                // treat the indices as type names and produce a parameterized Generic.
                if let Ty::Generic { base, args } = &obj_ty {
                    if args.is_empty() {
                        let mut tyargs = Vec::new();
                        for i in indices {
                            // Each index should be an Ident naming a type, or itself synthesize to a type-symbol.
                            if let Expr::Ident { name, .. } = i {
                                if let Some(sid) = r.symbols.lookup(r.module_scope, name) {
                                    if matches!(r.symbols.get(sid).kind,
                                                 SymbolKind::PrimType | SymbolKind::Class | SymbolKind::Protocol)
                                    {
                                        if let Some(t) = &r.symbols.get(sid).ty {
                                            tyargs.push(t.clone());
                                            continue;
                                        }
                                    }
                                }
                            }
                            tyargs.push(self.synth_expr(i, env, ctx, r)?);
                        }
                        // M14: normalize `Tuple[T1, T2, ...]` written as an
                        // index-on-ident to Ty::Tuple, mirroring the resolver.
                        if matches!(base, TypeCtor::Tuple) {
                            return Ok(Ty::Tuple(tyargs));
                        }
                        return Ok(Ty::Generic { base: base.clone(), args: tyargs });
                    }
                }
                // Wave 2 / Lane C: subscript-read on a user class dispatches to
                // `__getitem__(self, key)`. Check the key against the dunder's
                // declared parameter and adopt its return type. Falls through to
                // the built-in `index_type` for List/Dict/str/tuple receivers and
                // for classes that don't define `__getitem__`. A parameterised
                // receiver (`Box[str]`) is `Ty::Generic { Class(cid), .. }`.
                if let Some(cid) = class_cid_of(&obj_ty) {
                    if let Some(res) =
                        self.class_index_get_type(cid, &obj_ty, indices, *span, env, ctx, r)
                    {
                        return res;
                    }
                }
                for i in indices { let _ = self.synth_expr(i, env, ctx, r)?; }
                self.index_type(&obj_ty, *span)
            }
            // Lane B: slice `obj[lo:hi:step]`. Supported on `str` and
            // `List[T]`; the result type is the receiver type (a `str` slice
            // is a `str`, a `List[T]` slice is a `List[T]`). Every present
            // bound must be an integer.
            Expr::Slice { obj, lo, hi, step, span } => {
                let obj_ty = self.synth_expr(obj, env, ctx, r)?;
                for bound in [lo, hi, step].into_iter().flatten() {
                    let bt = self.synth_expr(bound, env, ctx, r)?;
                    if !matches!(&bt, Ty::Primitive(p) if p.is_integer()) {
                        return Err(type_err(expr_span(bound), codes::TYPE_MISMATCH,
                            format!("slice bounds must be integers, got {}", bt.display())));
                    }
                }
                match &obj_ty {
                    Ty::Primitive(PrimTy::Str) => Ok(Ty::Primitive(PrimTy::Str)),
                    Ty::Generic { base: TypeCtor::List, args } if args.len() == 1 => {
                        Ok(Ty::Generic { base: TypeCtor::List, args: args.clone() })
                    }
                    _ => Err(type_err(*span, codes::TYPE_NO_METHOD,
                        format!("type {} is not sliceable (slicing is supported on str and List[T])",
                            obj_ty.display()))),
                }
            }
            Expr::Lambda { params, return_ty, body, span: _ } => {
                let mut env2 = env.clone();
                for p in params {
                    let key = ast_type_span(&p.ty);
                    let t = r.ast_type_to_ty.get(&key).cloned().unwrap_or(Ty::Never);
                    if let Some(sym) = r.symbols.symbols.iter()
                        .find(|s| s.name == p.name && s.def_span.start == p.span.start)
                    {
                        env2.types.insert(sym.id, t);
                    }
                }
                let ret = if matches!(return_ty, ast::Type::Named { ref name, .. } if name == "None") {
                    Ty::Primitive(PrimTy::Unit)
                } else {
                    let key = ast_type_span(return_ty);
                    r.ast_type_to_ty.get(&key).cloned().unwrap_or(Ty::Never)
                };
                let _bt = self.check_or_synth(body, Some(&ret), &env2, ctx, r)?;
                Ok(Ty::Function {
                    params: params.iter().map(|p| {
                        let key = ast_type_span(&p.ty);
                        r.ast_type_to_ty.get(&key).cloned().unwrap_or(Ty::Never)
                    }).collect(),
                    ret: Box::new(ret),
                })
            }
            Expr::Cast { expr, target, span } => {
                let _ = self.synth_expr(expr, env, ctx, r)?;
                let key = ast_type_span(target);
                let t = r.ast_type_to_ty.get(&key).cloned().unwrap_or(Ty::Never);
                let _ = span;
                Ok(t)
            }
            Expr::Comprehension { .. } => self.check_comprehension(e, None, env, ctx, r),
        }
    }

    /// M62a: type-check a list/set/dict comprehension. `expected` carries the
    /// requested result type from assignment context (e.g. `List[i64]`), which
    /// lets the body expression's literals take on the target element width.
    fn check_comprehension(&mut self, e: &Expr, expected: Option<&Ty>,
                           env: &Env, ctx: &Ctx, r: &ResolvedModule)
        -> Result<Ty, CompileError>
    {
        let (kind, var, var_span, var_ty, iter, body, value, cond, span) = match e {
            Expr::Comprehension { kind, var, var_span, var_ty, iter, body, value, cond, span } =>
                (*kind, var, *var_span, var_ty, iter, body, value, cond, *span),
            _ => unreachable!("check_comprehension on non-comprehension"),
        };

        // The iterable must be a `List[T]` — the only iteration shape the
        // comprehension lowering supports (same restriction as `for`).
        let iter_ty = self.check_or_synth(iter, None, env, ctx, r)?;
        let iter_elem = match &iter_ty {
            Ty::Generic { base: TypeCtor::List, args } if args.len() == 1 => args[0].clone(),
            _ => {
                return Err(type_err(expr_span(iter), codes::TYPE_COMPREHENSION_NOT_ITERABLE,
                    format!("comprehension iterates over a List[T]; got {}", iter_ty.display())));
            }
        };

        // Bind the loop variable to its declared type inside the body scope.
        // The declared annotation must be compatible with the iterable's
        // element type (same as `for x: T in xs:`).
        let var_decl = r.ast_type_to_ty.get(&ast_type_span(var_ty)).cloned().unwrap_or(Ty::Never);
        let var_ty_final = if matches!(var_decl, Ty::Never) { iter_elem.clone() } else { var_decl };
        let mut env2 = env.clone();
        if let Some(sym) = r.symbols.symbols.iter()
            .find(|s| s.name == *var
                  && matches!(s.kind, SymbolKind::Local)
                  && s.def_span.start == var_span.start)
        {
            env2.types.insert(sym.id, var_ty_final.clone());
        }

        // Optional `if` filter must be bool. Synthesise (rather than check
        // against bool) so a non-bool filter yields the comprehension-specific
        // E2042 instead of the generic E2001.
        if let Some(c) = cond {
            let ct = self.synth_expr(c, &env2, ctx, r)?;
            if !matches!(ct, Ty::Primitive(PrimTy::Bool)) {
                return Err(type_err(expr_span(c), codes::TYPE_COMPREHENSION_FILTER_NOT_BOOL,
                    format!("comprehension `if` filter must be bool, got {}", ct.display())));
            }
        }

        let result = match kind {
            ComprehensionKind::List | ComprehensionKind::Set => {
                let ctor = if matches!(kind, ComprehensionKind::List) {
                    TypeCtor::List
                } else {
                    TypeCtor::Set
                };
                let label = if matches!(kind, ComprehensionKind::List) { "List" } else { "Set" };
                // Expected element type (if assignment context provided one).
                let elem_expected = match expected {
                    Some(Ty::Generic { base, args }) if *base == ctor && args.len() == 1 =>
                        Some(args[0].clone()),
                    _ => None,
                };
                let elem_ty = self.check_comprehension_elem(
                    body, elem_expected.as_ref(), &env2, ctx, r,
                    &format!("{label} comprehension body"))?;
                let elem = elem_expected.unwrap_or(elem_ty);
                if matches!(kind, ComprehensionKind::Set) {
                    check_set_elem_ty(&elem, span)?;
                }
                Ty::Generic { base: ctor, args: vec![elem] }
            }
            ComprehensionKind::Dict => {
                let (kexp, vexp) = match expected {
                    Some(Ty::Generic { base: TypeCtor::Dict, args }) if args.len() == 2 =>
                        (Some(args[0].clone()), Some(args[1].clone())),
                    _ => (None, None),
                };
                let kty = self.check_comprehension_elem(
                    body, kexp.as_ref(), &env2, ctx, r, "dict comprehension key")?;
                let val = value.as_ref().expect("dict comprehension has a value expr");
                let vty = self.check_comprehension_elem(
                    val, vexp.as_ref(), &env2, ctx, r, "dict comprehension value")?;
                Ty::Generic {
                    base: TypeCtor::Dict,
                    args: vec![kexp.unwrap_or(kty), vexp.unwrap_or(vty)],
                }
            }
        };
        self.expr_types.insert((span.start, span.end), result.clone());
        Ok(result)
    }

    /// M62a: check one comprehension sub-expression (body / dict key / dict
    /// value) against an optional expected element type. On mismatch this
    /// emits a comprehension-specific `E2041` (rather than the generic
    /// `E2001`), so the diagnostic points at the right feature. When no
    /// expected type is available the expression is synthesised.
    fn check_comprehension_elem(&mut self, e: &Expr, expected: Option<&Ty>,
                                env: &Env, ctx: &Ctx, r: &ResolvedModule, what: &str)
        -> Result<Ty, CompileError>
    {
        match expected {
            None => self.synth_expr(e, env, ctx, r),
            Some(exp) => {
                // Try the normal checked path first — this gives numeric
                // literal widening (e.g. `[0 for ...]` targeting `List[i64]`).
                if let Ok(t) = self.check_expr(e, exp, env, ctx, r) {
                    return Ok(t);
                }
                // It didn't fit: re-synthesise to report the actual type.
                let got = self.synth_expr(e, env, ctx, r)?;
                Err(type_err(expr_span(e), codes::TYPE_COMPREHENSION_ELEM_MISMATCH,
                    format!("{what} has type {}, expected {}", got.display(), exp.display())))
            }
        }
    }

    /// WAVE-2 LANE-B: type-check a binary operator whose left operand is a
    /// user-defined class `cid`, routing it to the operator's dunder method.
    ///
    /// On success the result type is the dunder's declared return type (with the
    /// receiver's generic substitution applied), and the rhs has been checked
    /// against the dunder's single declared operand type. Returns a clean
    /// `E2001` if the class (and its bases) define no such dunder.
    ///
    /// `__ne__` falls back to `__eq__` when the class defines only `__eq__`; the
    /// result is still `bool` (the IR lowers it as `not __eq__`). Ordering
    /// operators require their own dunder — there is no `>`-from-`<` synthesis.
    fn check_class_binop_dunder(
        &mut self, op: BinOp, cid: ClassId, lt: &Ty, rhs: &Expr, span: Span,
        env: &Env, ctx: &Ctx, r: &ResolvedModule,
    ) -> Result<Ty, CompileError> {
        let dunder = binop_dunder(op).expect("caller guarantees an operator dunder");
        // Build the receiver's tv -> concrete substitution so generic dunder
        // param/return types specialise (mirrors check_method_call's recv_subst).
        let recv_subst: HashMap<u32, Ty> = match lt {
            Ty::Generic { base: TypeCtor::Class(c), args } => {
                if let Some(cl) = ctx.classes.get(c) {
                    cl.generic_tvars.iter().zip(args.iter())
                        .map(|(tv, a)| (tv.0, a.clone())).collect()
                } else { HashMap::new() }
            }
            _ => HashMap::new(),
        };
        // `!=` may borrow `__eq__` when `__ne__` is absent (default `not __eq__`).
        let sig = lookup_dunder(ctx.classes, cid, dunder)
            .or_else(|| if matches!(op, BinOp::Ne) {
                lookup_dunder(ctx.classes, cid, "__eq__")
            } else { None });
        let sig = match sig {
            Some(s) => s,
            None => {
                let cname = ctx.classes.get(&cid).map(|c| c.name.as_str()).unwrap_or("<class>");
                return Err(type_err(span, codes::TYPE_BINOP_MISMATCH,
                    format!("type `{}` has no `{}` for operator `{:?}`", cname, dunder, op)));
            }
        };
        let ret_ty = subst_ty(&sig.ret, &recv_subst);
        // Check the rhs against the dunder's single declared operand type.
        // A binary dunder takes exactly one operand after `self`; if the user
        // declared something else, fall back to synthesising the rhs (the
        // resolver/arity machinery reports the malformed signature elsewhere).
        if let Some(param) = sig.params.first() {
            let expected = subst_ty(param, &recv_subst);
            let _ = self.check_expr(rhs, &expected, env, ctx, r)?;
        } else {
            let _ = self.synth_expr(rhs, env, ctx, r)?;
        }
        Ok(ret_ty)
    }

    fn check_binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr, span: Span,
                    env: &Env, ctx: &Ctx, r: &ResolvedModule) -> Result<Ty, CompileError>
    {
        // For arithmetic / comparison: synth LHS first, then check RHS against that type
        // when both operands are not literals.  This gives the numeric-literal-width
        // inference per spec §10.4.
        match op {
            BinOp::And | BinOp::Or => {
                let lt = self.check_or_synth(lhs, Some(&Ty::Primitive(PrimTy::Bool)), env, ctx, r)?;
                let rt = self.check_or_synth(rhs, Some(&Ty::Primitive(PrimTy::Bool)), env, ctx, r)?;
                if !matches!(lt, Ty::Primitive(PrimTy::Bool)) || !matches!(rt, Ty::Primitive(PrimTy::Bool)) {
                    return Err(type_err(span, codes::TYPE_BOOL_REQUIRED,
                        format!("`{:?}` requires bool operands, got {} and {}",
                                op, lt.display(), rt.display())));
                }
                Ok(Ty::Primitive(PrimTy::Bool))
            }
            BinOp::Is | BinOp::IsNot => {
                let _ = self.synth_expr(lhs, env, ctx, r)?;
                let _ = self.synth_expr(rhs, env, ctx, r)?;
                Ok(Ty::Primitive(PrimTy::Bool))
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let lt = self.synth_expr(lhs, env, ctx, r)?;
                // WAVE-2 LANE-B: a user-class left operand routes the comparison
                // to its dunder (`__eq__`/`__lt__`/...). Identity (`is`/`is not`)
                // is a separate match arm and stays pointer-identity. This runs
                // BEFORE the numeric/structural comparison paths below.
                if let Ty::Class(cid) | Ty::Generic { base: TypeCtor::Class(cid), .. } = &lt {
                    return self.check_class_binop_dunder(op, *cid, &lt, rhs, span, env, ctx, r);
                }
                let rt = self.check_or_synth(rhs, Some(&lt), env, ctx, r)?;
                // M63b: comparison inside a generic body is only legal when the
                // type parameter carries the matching bound. Ordering
                // (`<`, `<=`, `>`, `>=`) requires `Comparable`; equality
                // (`==`, `!=`) requires `Equatable` (a `Comparable` parameter
                // is implicitly `Equatable` too). An *unbounded* `T` is
                // rejected here — this is the negative case from the spec.
                // (M17 used to defer all of these unconditionally.)
                for opnd in [&lt, &rt] {
                    if let Ty::Var(tv) = opnd {
                        let needs_order = matches!(
                            op,
                            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
                        );
                        let ok = match r.generic_bounds.get(tv) {
                            Some(BoundKind::Comparable) => true,
                            Some(BoundKind::Equatable) => !needs_order,
                            _ => false,
                        };
                        if !ok {
                            let needed = if needs_order { "Comparable" } else { "Equatable" };
                            return Err(type_err(span, codes::TYPE_UNSATISFIED_BOUND,
                                format!(
                                    "`{:?}` requires the type parameter to be `{}`; add a bound like `[T: {}]`",
                                    op, needed, needed)));
                        }
                    }
                }
                if matches!(lt, Ty::Var(_)) || matches!(rt, Ty::Var(_)) {
                    return Ok(Ty::Primitive(PrimTy::Bool));
                }
                if !ty_eq(&lt, &rt)
                    && !is_subtype(&lt, &rt, &ctx.ty_ctx())
                    && !is_subtype(&rt, &lt, &ctx.ty_ctx())
                {
                    return Err(type_err(span, codes::TYPE_BINOP_MISMATCH,
                        format!("cannot compare {} and {}", lt.display(), rt.display())));
                }
                Ok(Ty::Primitive(PrimTy::Bool))
            }
            BinOp::In | BinOp::NotIn => {
                let rt = self.synth_expr(rhs, env, ctx, r)?;
                // Set membership checks the probe against the element type:
                // SetHas canonicalises by the *static* element type, so a
                // mismatched probe would silently answer false at runtime.
                // This also coerces unsuffixed int literals (`5 in s`).
                let rt_inner = match &rt {
                    Ty::Nullable(inner) => (**inner).clone(),
                    other => other.clone(),
                };
                if let Ty::Generic { base: TypeCtor::Set, args } = &rt_inner {
                    if args.len() == 1 {
                        let _ = self.check_expr(lhs, &args[0], env, ctx, r)?;
                        return Ok(Ty::Primitive(PrimTy::Bool));
                    }
                }
                let _lt = self.synth_expr(lhs, env, ctx, r)?;
                Ok(Ty::Primitive(PrimTy::Bool))
            }
            // Arithmetic / bitwise / shift
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::FloorDiv
            | BinOp::Rem | BinOp::Pow | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor
            | BinOp::Shl | BinOp::Shr => {
                let lt = self.synth_expr(lhs, env, ctx, r)?;
                // WAVE-2 LANE-B: a user-class left operand routes arithmetic to
                // its dunder (`__add__`/`__sub__`/...). This runs BEFORE the
                // numeric coercion/widening and error paths below, so mixed
                // numeric semantics (`1 + 2.0`, `i32 + i64`, `/`-as-float) are
                // untouched. Bitwise/shift have no Lane-B dunder (binop_dunder
                // returns None), so they fall through to the numeric error.
                if let Ty::Class(cid) | Ty::Generic { base: TypeCtor::Class(cid), .. } = &lt {
                    if binop_dunder(op).is_some() {
                        return self.check_class_binop_dunder(op, *cid, &lt, rhs, span, env, ctx, r);
                    }
                }
                // M17: inside a generic body, operand types may be unresolved
                // `Ty::Var`. Defer the operand-shape check to the per-
                // instantiation IR lowering — at the typecheck level we
                // return the operand type as-is so the body still typechecks
                // structurally. Concrete-substitution errors (e.g. `T + T`
                // with `T := some-class-with-no-Add`) surface at IR time as
                // VM traps for now; a future bounds system can move these
                // back to compile-time per instantiation.
                if matches!(lt, Ty::Var(_)) {
                    let _ = self.synth_expr(rhs, env, ctx, r)?;
                    return Ok(lt);
                }
                // Allow `str + str` (concat per spec §7.4).
                if matches!(op, BinOp::Add) && matches!(lt, Ty::Primitive(PrimTy::Str)) {
                    let rt = self.check_or_synth(rhs, Some(&Ty::Primitive(PrimTy::Str)), env, ctx, r)?;
                    if !matches!(rt, Ty::Primitive(PrimTy::Str)) {
                        return Err(type_err(span, codes::TYPE_BINOP_MISMATCH,
                            format!("string `+` requires str, got {}", rt.display())));
                    }
                    return Ok(Ty::Primitive(PrimTy::Str));
                }
                // Lane A: synthesise the RHS. When the RHS is a bare (width-
                // negotiable) numeric literal and the LHS is numeric, push the
                // LHS type into it so `f64_var + 2` keeps `2` at f64 etc.
                // Otherwise synth the RHS freely so its real type is available
                // for the widening join below.
                let rt = if is_unsuffixed_numeric_literal(rhs)
                    && matches!(&lt, Ty::Primitive(p) if p.is_numeric())
                {
                    self.check_or_synth(rhs, Some(&lt), env, ctx, r)?
                } else {
                    self.synth_expr(rhs, env, ctx, r)?
                };
                if matches!(rt, Ty::Var(_)) {
                    return Ok(rt);
                }
                // Both operands must be numeric primitives. (`str + str` already
                // returned above; user-class operands are Lane C, out of scope.)
                let (lp, rp) = match (&lt, &rt) {
                    (Ty::Primitive(lp), Ty::Primitive(rp)) if lp.is_numeric() && rp.is_numeric() => {
                        (*lp, *rp)
                    }
                    _ => {
                        // Preserve the old diagnostics: a non-numeric primitive
                        // gives the "requires numeric operands" message; an
                        // otherwise-unsupported operand the "not defined" one.
                        if let Ty::Primitive(p) = &lt {
                            if !p.is_numeric() {
                                return Err(type_err(span, codes::TYPE_BINOP_MISMATCH,
                                    format!("`{:?}` requires numeric operands, got {}", op, lt.display())));
                            }
                        } else {
                            return Err(type_err(span, codes::TYPE_BINOP_MISMATCH,
                                format!("`{:?}` not defined for {}", op, lt.display())));
                        }
                        return Err(type_err(span, codes::TYPE_BINOP_MISMATCH,
                            format!("operand type mismatch: cannot apply `{:?}` to {} and {}",
                                    op, lt.display(), rt.display())));
                    }
                };
                // Lane A: if exactly one operand is a width-negotiable numeric
                // literal and the other is a concrete numeric type, the literal
                // adopts the concrete type. This keeps `5 + i32_var` (literal on
                // the left) at i32 rather than defaulting the literal to i64 and
                // forcing a widen. Re-record the literal's expr type so IR
                // lowering sees the adopted width.
                let lhs_lit = operand_can_adopt(lhs, rp);
                let rhs_lit = operand_can_adopt(rhs, lp);
                let (lp, rp) = if lhs_lit && !rhs_lit && rp != lp {
                    let _ = self.check_expr(lhs, &Ty::Primitive(rp), env, ctx, r)?;
                    (rp, rp)
                } else if rhs_lit && !lhs_lit && rp != lp {
                    let _ = self.check_expr(rhs, &Ty::Primitive(lp), env, ctx, r)?;
                    (lp, lp)
                } else {
                    (lp, rp)
                };
                // Compute the common (widened) numeric type. Conservative: only
                // the signed-int ladder (i8/i16/i32 -> i64) and int/f32 -> f64
                // promotions widen automatically; anything else still requires
                // an exact match (so `u32 + i32`, `u64`, mixed-signedness stay
                // errors, matching the available lossless cast set).
                let common = match numeric_common_ty(lp, rp) {
                    Some(c) => c,
                    None => {
                        return Err(type_err(span, codes::TYPE_BINOP_MISMATCH,
                            format!("operand type mismatch: cannot apply `{:?}` to {} and {}",
                                    op, lt.display(), rt.display())));
                    }
                };
                // Shifts: the result follows the LHS width; the RHS is just a
                // shift count and is not widened into the result.
                if matches!(op, BinOp::Shl | BinOp::Shr) {
                    return Ok(Ty::Primitive(lp));
                }
                // `/` is true division: always yields f64 (Python 3 semantics).
                if matches!(op, BinOp::Div) {
                    return Ok(Ty::Primitive(PrimTy::F64));
                }
                // `//`, `%`, `**`, `+ - *`, bitwise: result is the common type.
                // (`//` on ints is truncating per spec §7.2; on floats it is
                // plain float division for now — see IR lowering.)
                Ok(Ty::Primitive(common))
            }
        }
    }

    /// M61b: validate a call's positional + keyword arguments against a
    /// callee's declared parameters and type-check each supplied argument
    /// against its parameter type. `ast_params` provides names + defaults
    /// (post-`self`); `param_tys` are the matching semantic parameter types
    /// (same length, same order). Omitted parameters fall back to their
    /// declared default, which was already checked at declaration time.
    fn check_call_binding(
        &mut self,
        ast_params: &[ast::Param],
        param_tys: &[Ty],
        args: &[Arg],
        span: Span,
        desc: &str,
        env: &Env,
        ctx: &Ctx,
        r: &ResolvedModule,
    ) -> Result<(), CompileError> {
        let infos = crate::argbind::ParamInfo::from_params(ast_params);
        let slots = crate::argbind::bind(&infos, args, span, desc)?;
        for (pidx, slot) in slots.iter().enumerate() {
            if let crate::argbind::Slot::Arg(ai) = slot {
                let pt = param_tys.get(pidx).cloned().unwrap_or(Ty::Never);
                let _ = self.check_expr(&args[*ai].value, &pt, env, ctx, r)?;
            }
        }
        Ok(())
    }

    fn synth_call(&mut self, callee: &Expr, args: &[Arg], span: Span,
                  env: &Env, ctx: &Ctx, r: &ResolvedModule) -> Result<Ty, CompileError>
    {
        // Determine the symbol kind for the callee — handles builtin polymorphics.
        if let Expr::Ident { name, .. } = callee {
            // Built-ins with bespoke handling.
            match name.as_str() {
                "print" | "println" => {
                    for a in args { let _ = self.synth_expr(&a.value, env, ctx, r)?; }
                    return Ok(Ty::Primitive(PrimTy::Unit));
                }
                "len" => {
                    if args.len() != 1 {
                        return Err(type_err(span, codes::TYPE_ARITY, "len takes 1 argument".into()));
                    }
                    let _ = self.synth_expr(&args[0].value, env, ctx, r)?;
                    return Ok(Ty::Primitive(PrimTy::I64));
                }
                "abs" => {
                    if args.len() != 1 {
                        return Err(type_err(span, codes::TYPE_ARITY, "abs takes 1 argument".into()));
                    }
                    return self.synth_expr(&args[0].value, env, ctx, r);
                }
                "min" | "max" => {
                    if args.is_empty() {
                        return Err(type_err(span, codes::TYPE_ARITY,
                            "min/max takes at least 1 argument".into()));
                    }
                    let mut t0 = self.synth_expr(&args[0].value, env, ctx, r)?;
                    for a in &args[1..] {
                        let t = self.synth_expr(&a.value, env, ctx, r)?;
                        if let Some(l) = lub(&t0, &t) { t0 = l; }
                    }
                    return Ok(t0);
                }
                "range" => {
                    for a in args { let _ = self.check_or_synth(&a.value, Some(&Ty::Primitive(PrimTy::I64)), env, ctx, r)?; }
                    return Ok(Ty::Generic { base: TypeCtor::Range, args: vec![] });
                }
                // real-world: stress tests producing ranked output.
                // `sorted(xs)` over `List[T]` returns `List[T]`. v1 only
                // supports T ∈ {i64, f64, str}; richer key-fn forms wait
                // for M10 generic comparators.
                "sorted" => {
                    if args.len() != 1 {
                        return Err(type_err(span, codes::TYPE_ARITY,
                            "sorted takes 1 argument".into()));
                    }
                    let t = self.synth_expr(&args[0].value, env, ctx, r)?;
                    if let Ty::Generic { base: TypeCtor::List, args: a } = &t {
                        return Ok(Ty::Generic { base: TypeCtor::List, args: a.clone() });
                    }
                    return Err(type_err(span, codes::TYPE_MISMATCH,
                        format!("sorted expects a List, got {}", t.display())));
                }
                // M61a: higher-order builtins. User callbacks now cross the
                // NativeFn boundary, so these take a closure value plus a
                // List and re-enter the interpreter per element. All forms
                // are positional; `key=`/default forms come later.
                //
                //   map(fn: T -> U, xs: List[T]) -> List[U]
                "map" => {
                    if args.len() != 2 {
                        return Err(type_err(span, codes::TYPE_ARITY,
                            "map takes 2 arguments: (fn, xs)".into()));
                    }
                    let xs_ty = self.synth_expr(&args[1].value, env, ctx, r)?;
                    let elem = match &xs_ty {
                        Ty::Generic { base: TypeCtor::List, args: a } if a.len() == 1 => a[0].clone(),
                        _ => return Err(type_err(span, codes::TYPE_MISMATCH,
                            format!("map: second argument must be a List, got {}", xs_ty.display()))),
                    };
                    // Check the callback against `T -> U`, inferring U from its
                    // declared return type.
                    let fn_ty = self.synth_expr(&args[0].value, env, ctx, r)?;
                    let u = match &fn_ty {
                        Ty::Function { params, ret } if params.len() == 1 => {
                            if !type_assignable(&elem, &params[0]) {
                                return Err(type_err(span, codes::TYPE_MISMATCH,
                                    format!("map: callback expects {} but list elements are {}",
                                            params[0].display(), elem.display())));
                            }
                            (**ret).clone()
                        }
                        _ => return Err(type_err(span, codes::TYPE_MISMATCH,
                            "map: first argument must be a 1-parameter function".into())),
                    };
                    return Ok(Ty::Generic { base: TypeCtor::List, args: vec![u] });
                }
                //   filter(fn: T -> bool, xs: List[T]) -> List[T]
                "filter" => {
                    if args.len() != 2 {
                        return Err(type_err(span, codes::TYPE_ARITY,
                            "filter takes 2 arguments: (fn, xs)".into()));
                    }
                    let xs_ty = self.synth_expr(&args[1].value, env, ctx, r)?;
                    let elem = match &xs_ty {
                        Ty::Generic { base: TypeCtor::List, args: a } if a.len() == 1 => a[0].clone(),
                        _ => return Err(type_err(span, codes::TYPE_MISMATCH,
                            format!("filter: second argument must be a List, got {}", xs_ty.display()))),
                    };
                    let fn_ty = self.synth_expr(&args[0].value, env, ctx, r)?;
                    match &fn_ty {
                        Ty::Function { params, ret } if params.len() == 1 => {
                            if !type_assignable(&elem, &params[0]) {
                                return Err(type_err(span, codes::TYPE_MISMATCH,
                                    format!("filter: predicate expects {} but list elements are {}",
                                            params[0].display(), elem.display())));
                            }
                            if !matches!(**ret, Ty::Primitive(PrimTy::Bool)) {
                                return Err(type_err(span, codes::TYPE_MISMATCH,
                                    format!("filter: predicate must return bool, got {}", ret.display())));
                            }
                        }
                        _ => return Err(type_err(span, codes::TYPE_MISMATCH,
                            "filter: first argument must be a 1-parameter function".into())),
                    }
                    return Ok(Ty::Generic { base: TypeCtor::List, args: vec![elem] });
                }
                //   reduce(fn: (U, T) -> U, xs: List[T], init: U) -> U
                "reduce" => {
                    if args.len() != 3 {
                        return Err(type_err(span, codes::TYPE_ARITY,
                            "reduce takes 3 arguments: (fn, xs, init)".into()));
                    }
                    let xs_ty = self.synth_expr(&args[1].value, env, ctx, r)?;
                    let elem = match &xs_ty {
                        Ty::Generic { base: TypeCtor::List, args: a } if a.len() == 1 => a[0].clone(),
                        _ => return Err(type_err(span, codes::TYPE_MISMATCH,
                            format!("reduce: second argument must be a List, got {}", xs_ty.display()))),
                    };
                    let fn_ty = self.synth_expr(&args[0].value, env, ctx, r)?;
                    let acc_ty = match &fn_ty {
                        Ty::Function { params, ret } if params.len() == 2 => {
                            // The accumulator type U is the callback's first
                            // param. Check `init` against it so unsuffixed int
                            // literals (e.g. `reduce(..., 0)`) take width U.
                            let _ = self.check_expr(&args[2].value, &params[0], env, ctx, r)?;
                            if !type_assignable(&elem, &params[1]) {
                                return Err(type_err(span, codes::TYPE_MISMATCH,
                                    format!("reduce: list elements {} not assignable to callback param {}",
                                            elem.display(), params[1].display())));
                            }
                            if !type_assignable(ret, &params[0]) {
                                return Err(type_err(span, codes::TYPE_MISMATCH,
                                    format!("reduce: callback returns {} but accumulator is {}",
                                            ret.display(), params[0].display())));
                            }
                            params[0].clone()
                        }
                        _ => return Err(type_err(span, codes::TYPE_MISMATCH,
                            "reduce: first argument must be a 2-parameter function".into())),
                    };
                    return Ok(acc_ty);
                }
                //   sorted_by(xs: List[T], key_fn: T -> K) -> List[T]
                "sorted_by" => {
                    if args.len() != 2 {
                        return Err(type_err(span, codes::TYPE_ARITY,
                            "sorted_by takes 2 arguments: (xs, key_fn)".into()));
                    }
                    let xs_ty = self.synth_expr(&args[0].value, env, ctx, r)?;
                    let elem = match &xs_ty {
                        Ty::Generic { base: TypeCtor::List, args: a } if a.len() == 1 => a[0].clone(),
                        _ => return Err(type_err(span, codes::TYPE_MISMATCH,
                            format!("sorted_by: first argument must be a List, got {}", xs_ty.display()))),
                    };
                    let fn_ty = self.synth_expr(&args[1].value, env, ctx, r)?;
                    match &fn_ty {
                        Ty::Function { params, ret } if params.len() == 1 => {
                            if !type_assignable(&elem, &params[0]) {
                                return Err(type_err(span, codes::TYPE_MISMATCH,
                                    format!("sorted_by: key fn expects {} but list elements are {}",
                                            params[0].display(), elem.display())));
                            }
                            if !is_comparable_key_ty(ret) {
                                return Err(type_err(span, codes::TYPE_MISMATCH,
                                    format!("sorted_by: key must be i64/f64/str, got {}", ret.display())));
                            }
                        }
                        _ => return Err(type_err(span, codes::TYPE_MISMATCH,
                            "sorted_by: second argument must be a 1-parameter function".into())),
                    }
                    return Ok(Ty::Generic { base: TypeCtor::List, args: vec![elem] });
                }
                "assert" => {
                    if args.is_empty() {
                        return Err(type_err(span, codes::TYPE_ARITY,
                            "assert takes at least 1 argument".into()));
                    }
                    let _ = self.check_expr(&args[0].value, &Ty::Primitive(PrimTy::Bool), env, ctx, r)?;
                    if args.len() >= 2 {
                        let _ = self.check_expr(&args[1].value, &Ty::Primitive(PrimTy::Str), env, ctx, r)?;
                    }
                    return Ok(Ty::Primitive(PrimTy::Unit));
                }
                // M16: `isinstance(x, T)` — runtime class check. Returns bool.
                // The second argument must name a user class. Flow-narrowing
                // happens in `narrowings_from_cond` once the call sits in an
                // `if` condition.
                "isinstance" => {
                    if args.len() != 2 {
                        return Err(type_err(span, codes::TYPE_ARITY,
                            "isinstance takes 2 arguments: (value, ClassName)".into()));
                    }
                    let _ = self.synth_expr(&args[0].value, env, ctx, r)?;
                    // Second arg: must be a class-naming ident.
                    if let Expr::Ident { name, span: ispan } = &args[1].value {
                        if let Some(sid) = r.symbols.lookup(r.module_scope, name) {
                            let s = r.symbols.get(sid);
                            if matches!(s.kind, SymbolKind::Class) && s.class_id.is_some() {
                                // Stash the class type at the second-arg's
                                // span so the IR lowerer can recover the
                                // class id when materialising IROp::IsInstance.
                                self.expr_types.insert(
                                    (ispan.start, ispan.end),
                                    Ty::Class(s.class_id.unwrap()),
                                );
                                return Ok(Ty::Primitive(PrimTy::Bool));
                            }
                        }
                    }
                    return Err(type_err(span, codes::TYPE_MISMATCH,
                        "isinstance: second argument must name a user class".into()));
                }
                "str" => {
                    // str(x) converts any value to str.
                    if args.len() != 1 {
                        return Err(type_err(span, codes::TYPE_ARITY, "str() takes 1 argument".into()));
                    }
                    let _ = self.synth_expr(&args[0].value, env, ctx, r)?;
                    return Ok(Ty::Primitive(PrimTy::Str));
                }
                // Numeric conversion functions.
                "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "f32" | "f64" => {
                    if args.len() != 1 {
                        return Err(type_err(span, codes::TYPE_ARITY,
                            format!("{}() takes 1 argument", name)));
                    }
                    let _ = self.synth_expr(&args[0].value, env, ctx, r)?;
                    return Ok(Ty::Primitive(prim_from_name_unchecked(name)));
                }
                // real-world: fix — `char(i32)` builds a Char from a
                // codepoint. The native CharFromI32 (id 23) and IR
                // dispatch were already wired; only the typechecker's
                // numeric-ctor allow-list was missing this case, so any
                // call to `char(72)` failed E2011 "not callable".
                "char" => {
                    if args.len() != 1 {
                        return Err(type_err(span, codes::TYPE_ARITY,
                            "char() takes 1 argument".into()));
                    }
                    let _ = self.synth_expr(&args[0].value, env, ctx, r)?;
                    return Ok(Ty::Primitive(PrimTy::Char));
                }
                _ => {}
            }
        }
        // M17: generic free-function call (`fn id[T](x: T) -> T: ...`). When
        // the callee names a user-defined function whose `FunctionSig` has
        // non-empty `generics`, infer the substitution from argument types
        // and record the instantiation so the IR lowerer can emit one
        // bytecode function per (sym_id, type_args) pair.
        if let Expr::Ident { name, .. } = callee {
            if let Some(sid) = r.symbols.lookup(r.module_scope, name) {
                if matches!(r.symbols.get(sid).kind, SymbolKind::Function) {
                    if let Some(sig) = r.function_sigs.get(&sid).cloned() {
                        if !sig.generic_tvars.is_empty() {
                            return self.check_generic_call(sid, &sig, args, span, env, ctx, r);
                        }
                    }
                }
            }
        }
        // M61b: non-generic user free function — bind positional + keyword
        // arguments against the declared parameters, filling defaults. We use
        // the AST parameter list (names + defaults) plus the resolved
        // signature's parameter types.
        if let Expr::Ident { name, .. } = callee {
            if let Some(sid) = r.symbols.lookup(r.module_scope, name) {
                if matches!(r.symbols.get(sid).kind, SymbolKind::Function) {
                    if let Some(sig) = r.function_sigs.get(&sid).cloned() {
                        if sig.generic_tvars.is_empty() {
                            if let Some(ast_params) = ast_free_fn_params(r, name) {
                                let param_tys: Vec<Ty> =
                                    sig.params.iter().map(|(_, t)| t.clone()).collect();
                                self.check_call_binding(
                                    ast_params, &param_tys, args, span,
                                    &format!("function `{name}`"), env, ctx, r,
                                )?;
                                return Ok(sig.ret.clone());
                            }
                        }
                    }
                }
            }
        }
        // Generic callee.
        let cty = self.synth_expr(callee, env, ctx, r)?;
        match cty {
            Ty::Function { params, ret } => {
                if args.len() != params.len() {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        format!("expected {} args, got {}", params.len(), args.len())));
                }
                for (a, pt) in args.iter().zip(params.iter()) {
                    let _ = self.check_expr(&a.value, pt, env, ctx, r)?;
                }
                Ok(*ret)
            }
            Ty::Class(cid) => {
                // Constructor call.  Look up __init__ in the class layout.
                let cl = ctx.classes.get(&cid).cloned();
                if let Some(cl) = cl {
                    // M31: generic class — infer type args from constructor
                    // arguments. Delegates to the same call-site unification
                    // M17 uses for free functions, but binds the class's TVs
                    // and records a class-instantiation entry instead of a
                    // function one.
                    if !cl.generic_tvars.is_empty() {
                        return self.check_generic_class_construct(
                            cid, &cl, args, span, env, ctx, r,
                        );
                    }
                    if let Some(init) = cl.methods.iter().find(|m| m.name == "__init__") {
                        // M61b: default + keyword binding when we can recover
                        // the constructor's AST parameters (names + defaults).
                        if let Some(ast_params) = ast_ctor_params(r, &cl.name) {
                            let desc = format!("constructor of `{}`", cl.name);
                            self.check_call_binding(
                                ast_params, &init.params, args, span, &desc, env, ctx, r,
                            )?;
                            return Ok(Ty::Class(cid));
                        }
                        if args.len() != init.params.len() {
                            return Err(type_err(span, codes::TYPE_ARITY,
                                format!("constructor of `{}` expects {} args, got {}",
                                        cl.name, init.params.len(), args.len())));
                        }
                        for (a, pt) in args.iter().zip(init.params.iter()) {
                            let _ = self.check_expr(&a.value, pt, env, ctx, r)?;
                        }
                        return Ok(Ty::Class(cid));
                    }
                    // M34: JsonValue subclass constructors have no
                    // user-level `__init__` (their initialisation goes
                    // through a native handler that allocs + stores the
                    // single payload field).  Type-check the args
                    // against the synthesised constructor signatures
                    // here so users don't see a "class has no __init__"
                    // false positive.
                    if let Some(param_tys) = m34_json_ctor_param_tys(&cl.name, ctx) {
                        if args.len() != param_tys.len() {
                            return Err(type_err(span, codes::TYPE_ARITY,
                                format!("constructor of `{}` expects {} args, got {}",
                                        cl.name, param_tys.len(), args.len())));
                        }
                        for (a, pt) in args.iter().zip(param_tys.iter()) {
                            let _ = self.check_expr(&a.value, pt, env, ctx, r)?;
                        }
                        return Ok(Ty::Class(cid));
                    }
                    // M35 P4-B: typed sqlite3.Connection / Cursor
                    // constructors — same shape as M34 above.  Each
                    // takes a single `handle: i64`.
                    if let Some(param_tys) = m35_p4b_sqlite_ctor_param_tys(&cl.name) {
                        if args.len() != param_tys.len() {
                            return Err(type_err(span, codes::TYPE_ARITY,
                                format!("constructor of `{}` expects {} args, got {}",
                                        cl.name, param_tys.len(), args.len())));
                        }
                        for (a, pt) in args.iter().zip(param_tys.iter()) {
                            let _ = self.check_expr(&a.value, pt, env, ctx, r)?;
                        }
                        return Ok(Ty::Class(cid));
                    }
                    // No explicit __init__ — only valid with no args (default ctor).
                    if !args.is_empty() {
                        return Err(type_err(span, codes::TYPE_ARITY,
                            format!("class `{}` has no __init__", cl.name)));
                    }
                    return Ok(Ty::Class(cid));
                }
                Ok(Ty::Class(cid))
            }
            Ty::Generic { base, args: _gargs } => {
                // e.g. `Channel[i32](16)` — synth_call on the indexed callee
                // produces this Generic; treat as constructor of the base.
                if let TypeCtor::Channel = base {
                    // stdlib: Channel takes (capacity: i32)
                    if args.len() != 1 {
                        return Err(type_err(span, codes::TYPE_ARITY,
                            "Channel(capacity) takes 1 argument".into()));
                    }
                    let _ = self.check_expr(&args[0].value, &Ty::Primitive(PrimTy::I32), env, ctx, r)?;
                    return Ok(Ty::Generic { base: TypeCtor::Channel, args: _gargs });
                }
                if let TypeCtor::Class(cid) = base {
                    let cl = ctx.classes.get(&cid).cloned();
                    if let Some(cl) = cl {
                        if let Some(init) = cl.methods.iter().find(|m| m.name == "__init__") {
                            if args.len() == init.params.len() {
                                for (a, pt) in args.iter().zip(init.params.iter()) {
                                    let _ = self.check_expr(&a.value, pt, env, ctx, r)?;
                                }
                                return Ok(Ty::Class(cid));
                            }
                        }
                    }
                }
                Ok(Ty::Never)
            }
            _ => Err(type_err(span, codes::TYPE_NOT_CALLABLE,
                format!("value of type {} is not callable", cty.display()))),
        }
    }

    /// M17 call-site inference for `fn f[T1, T2, ...](...)`.
    ///
    /// 1. Synthesize a type for each argument expression.
    /// 2. Unify each parameter type against its argument type, accumulating a
    ///    substitution `{Var(tv) -> concrete_ty}`. Conflicts (same tv assigned
    ///    incompatible types) and unsolved type vars produce a `TYPE_MISMATCH`
    ///    error pointed at the call.
    /// 3. Record the (sid, ordered_type_args) instantiation so IR lowering
    ///    can emit a mangled bytecode function per instantiation.
    /// 4. Return the substituted result type.
    fn check_generic_call(
        &mut self,
        sid: SymbolId,
        sig: &FunctionSig,
        args: &[Arg],
        span: Span,
        env: &Env,
        ctx: &Ctx,
        r: &ResolvedModule,
    ) -> Result<Ty, CompileError> {
        if args.len() != sig.params.len() {
            return Err(type_err(
                span,
                codes::TYPE_ARITY,
                format!("expected {} args, got {}", sig.params.len(), args.len()),
            ));
        }
        // Step 1+2 interleaved: for each (param, arg) in declaration order,
        // substitute already-solved type vars into the param type. If the
        // resulting expected type is fully concrete (no unbound TypeVars), use
        // `check_expr` so int-literal width inference (i64 from `0`) etc.
        // works. Otherwise `synth_expr` then unify to bind new vars.
        let mut subst: HashMap<u32, Ty> = HashMap::new();
        let mut arg_tys: Vec<Ty> = Vec::with_capacity(args.len());
        for (i, (_, ptype)) in sig.params.iter().enumerate() {
            let expected = subst_ty(ptype, &subst);
            let got = if contains_unbound_var(&expected) {
                self.synth_expr(&args[i].value, env, ctx, r)?
            } else {
                self.check_expr(&args[i].value, &expected, env, ctx, r)?
            };
            arg_tys.push(got.clone());
            unify_one(&expected, &got, &mut subst).map_err(|m| {
                type_err(
                    span,
                    codes::TYPE_MISMATCH,
                    format!(
                        "generic call to `{}`: argument {} type {} doesn't match parameter type {} ({})",
                        sig.name,
                        i + 1,
                        got.display(),
                        expected.display(),
                        m
                    ),
                )
            })?;
        }
        // Every declared type-parameter must have been solved.
        let mut type_args: Vec<Ty> = Vec::with_capacity(sig.generic_tvars.len());
        for tv in &sig.generic_tvars {
            match subst.get(&tv.0).cloned() {
                Some(t) => type_args.push(t),
                None => {
                    return Err(type_err(
                        span,
                        codes::TYPE_MISMATCH,
                        format!(
                            "cannot infer type parameter for generic call to `{}`; supply argument types that pin every type variable",
                            sig.name
                        ),
                    ));
                }
            }
        }
        // M63b: every solved type argument must satisfy the bound (if any)
        // declared on its type parameter.
        check_bounds_satisfied(&sig.generic_tvars, &type_args, &sig.name, span, r)?;
        // Step 3: record the instantiation (de-duped by mangled key).
        let key = mangle_args_key(&type_args);
        if self.instantiation_keys.insert((sid, key)) {
            self.instantiations.push((sid, type_args.clone()));
        }
        // Step 4: substitute the return type.
        let ret = subst_ty(&sig.ret, &subst);
        let _ = ctx;
        Ok(ret)
    }

    /// M31: constructor-site inference for `class Box[T1, T2, ...]`.
    /// Mirrors `check_generic_call` but operates on the class's `__init__`
    /// method signature (whose param types carry `Ty::Var(...)` from the
    /// class's generic scope) and records a `class_instantiations` entry
    /// instead of a `instantiations` one. The return type is the parameterised
    /// `Ty::Generic { base: TypeCtor::Class(cid), args: <inferred> }` so that
    /// downstream field accesses and method calls can substitute correctly.
    ///
    /// A class with type parameters but no explicit `__init__` is supported
    /// only when no constructor args are passed — there's no inference
    /// driver, so every `T` must be solved via `Box[i64]()` syntax. v0.3 does
    /// not yet implement the explicit-type-argument syntax; flag as
    /// E2_GENERIC_CLASS_NEEDS_INIT to give a clear error.
    fn check_generic_class_construct(
        &mut self,
        cid: ClassId,
        cl: &ClassLayout,
        args: &[Arg],
        span: Span,
        env: &Env,
        ctx: &Ctx,
        r: &ResolvedModule,
    ) -> Result<Ty, CompileError> {
        let init = cl.methods.iter().find(|m| m.name == "__init__");
        let Some(init) = init else {
            if !args.is_empty() {
                return Err(type_err(
                    span,
                    codes::TYPE_ARITY,
                    format!(
                        "generic class `{}` has no __init__; cannot infer type parameters from {} arg(s)",
                        cl.name, args.len()
                    ),
                ));
            }
            // No __init__, no args. Every TV remains unbound — currently an
            // error (the user can't pin T any other way in v0.3). Surface a
            // helpful diagnostic.
            return Err(type_err(
                span,
                codes::TYPE_MISMATCH,
                format!(
                    "cannot infer type parameter(s) for `{}`; add a constructor or supply an annotation site that pins T",
                    cl.name
                ),
            ));
        };
        if args.len() != init.params.len() {
            return Err(type_err(
                span,
                codes::TYPE_ARITY,
                format!(
                    "constructor of `{}` expects {} args, got {}",
                    cl.name,
                    init.params.len(),
                    args.len()
                ),
            ));
        }
        let mut subst: HashMap<u32, Ty> = HashMap::new();
        for (i, ptype) in init.params.iter().enumerate() {
            let expected = subst_ty(ptype, &subst);
            let got = if contains_unbound_var(&expected) {
                self.synth_expr(&args[i].value, env, ctx, r)?
            } else {
                self.check_expr(&args[i].value, &expected, env, ctx, r)?
            };
            unify_one(&expected, &got, &mut subst).map_err(|m| {
                type_err(
                    span,
                    codes::TYPE_MISMATCH,
                    format!(
                        "constructor of `{}`: argument {} type {} doesn't match parameter type {} ({})",
                        cl.name,
                        i + 1,
                        got.display(),
                        expected.display(),
                        m
                    ),
                )
            })?;
        }
        let mut type_args: Vec<Ty> = Vec::with_capacity(cl.generic_tvars.len());
        for tv in &cl.generic_tvars {
            match subst.get(&tv.0).cloned() {
                Some(t) => type_args.push(t),
                None => {
                    return Err(type_err(
                        span,
                        codes::TYPE_MISMATCH,
                        format!(
                            "cannot infer type parameter for generic class `{}`; supply constructor arguments that pin every type variable",
                            cl.name
                        ),
                    ));
                }
            }
        }
        // M63b: enforce declared bounds on the class type parameters.
        check_bounds_satisfied(&cl.generic_tvars, &type_args, &cl.name, span, r)?;
        let key = mangle_args_key(&type_args);
        if self.class_instantiation_keys.insert((cid, key)) {
            self.class_instantiations.push((cid, type_args.clone()));
        }
        let _ = ctx;
        Ok(Ty::Generic {
            base: TypeCtor::Class(cid),
            args: type_args,
        })
    }

    fn synth_method_call(&mut self, recv: &Ty, method: &str, args: &[Arg], span: Span,
                          env: &Env, ctx: &Ctx, r: &ResolvedModule) -> Result<Ty, CompileError>
    {
        // Built-in container methods (best-effort coverage for v0.1 examples).
        match (recv, method) {
            // List methods
            (Ty::Generic { base: TypeCtor::List, args: a }, "append") => {
                if a.len() == 1 && args.len() == 1 {
                    let _ = self.check_expr(&args[0].value, &a[0], env, ctx, r)?;
                }
                return Ok(Ty::Primitive(PrimTy::Unit));
            }
            (Ty::Generic { base: TypeCtor::List, args: _ }, "len" | "length") => {
                return Ok(Ty::Primitive(PrimTy::I64));
            }
            (Ty::Generic { base: TypeCtor::List, args: a }, "get") if a.len() == 1 => {
                for arg in args { let _ = self.synth_expr(&arg.value, env, ctx, r)?; }
                return Ok(Ty::Nullable(Box::new(a[0].clone())));
            }
            // real-world: in-place sort. Element type must be i64/f64/str
            // for v1 — VM raises TypeError for anything else.
            (Ty::Generic { base: TypeCtor::List, args: _ }, "sort") => {
                if !args.is_empty() {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        "List.sort takes no arguments in v1".into()));
                }
                return Ok(Ty::Primitive(PrimTy::Unit));
            }
            // M61a: in-place sort by a user key function. `key_fn: T -> K`
            // where K is a comparable primitive (i64/f64/str). Returns None.
            (Ty::Generic { base: TypeCtor::List, args: a }, "sort_by") if a.len() == 1 => {
                if args.len() != 1 {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        "List.sort_by takes 1 argument: (key_fn)".into()));
                }
                let elem = a[0].clone();
                let fn_ty = self.synth_expr(&args[0].value, env, ctx, r)?;
                match &fn_ty {
                    Ty::Function { params, ret } if params.len() == 1 => {
                        if !type_assignable(&elem, &params[0]) {
                            return Err(type_err(span, codes::TYPE_MISMATCH,
                                format!("sort_by: key fn expects {} but list elements are {}",
                                        params[0].display(), elem.display())));
                        }
                        if !is_comparable_key_ty(ret) {
                            return Err(type_err(span, codes::TYPE_MISMATCH,
                                format!("sort_by: key must be i64/f64/str, got {}", ret.display())));
                        }
                    }
                    _ => return Err(type_err(span, codes::TYPE_MISMATCH,
                        "sort_by: argument must be a 1-parameter function".into())),
                }
                return Ok(Ty::Primitive(PrimTy::Unit));
            }
            // real-world: fix — `xs.pop()` removes and returns the last
            // element of a List[T]. Empty list traps with IndexError at
            // runtime. Mirrors Python's list.pop() (no-arg form).
            (Ty::Generic { base: TypeCtor::List, args: a }, "pop") if a.len() == 1 => {
                if !args.is_empty() {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        "List.pop takes no arguments in v1".into()));
                }
                return Ok(a[0].clone());
            }
            // Dict methods
            (Ty::Generic { base: TypeCtor::Dict, args: a }, "get") if a.len() == 2 => {
                if !args.is_empty() {
                    let _ = self.check_expr(&args[0].value, &a[0], env, ctx, r)?;
                }
                return Ok(Ty::Nullable(Box::new(a[1].clone())));
            }
            (Ty::Generic { base: TypeCtor::Dict, args: a }, "keys") if a.len() == 2 => {
                return Ok(Ty::Generic { base: TypeCtor::List, args: vec![a[0].clone()] });
            }
            (Ty::Generic { base: TypeCtor::Dict, args: a }, "values") if a.len() == 2 => {
                return Ok(Ty::Generic { base: TypeCtor::List, args: vec![a[1].clone()] });
            }
            (Ty::Generic { base: TypeCtor::Dict, args: a }, "items") if a.len() == 2 => {
                // stdlib: wordcount.spy — returns iterator of (K, V) tuples
                return Ok(Ty::Generic {
                    base: TypeCtor::Iterator,
                    args: vec![Ty::Tuple(vec![a[0].clone(), a[1].clone()])],
                });
            }
            (Ty::Generic { base: TypeCtor::Dict, args: a }, "contains") if a.len() == 2 => {
                if !args.is_empty() {
                    let _ = self.check_expr(&args[0].value, &a[0], env, ctx, r)?;
                }
                return Ok(Ty::Primitive(PrimTy::Bool));
            }
            // real-world: fix — `dict.has(k) -> bool` was wired through
            // the IR (`resolve_native_method` dispatches to `DictHas`,
            // VM implements it) but the typechecker rejected it with
            // E2004 "no method `has`". Add the synth entry to mirror
            // `contains` (we keep both names — `has` is what JSON / KV
            // store / BF examples reach for).
            (Ty::Generic { base: TypeCtor::Dict, args: a }, "has") if a.len() == 2 => {
                if !args.is_empty() {
                    let _ = self.check_expr(&args[0].value, &a[0], env, ctx, r)?;
                }
                return Ok(Ty::Primitive(PrimTy::Bool));
            }
            (Ty::Generic { base: TypeCtor::Dict, args: _ }, "length" | "len") => {
                if !args.is_empty() {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        "Dict.length takes no arguments".into()));
                }
                return Ok(Ty::Primitive(PrimTy::I64));
            }
            // Set methods — guide §6.3. Set[T] is dict-backed; `add`/`has`
            // canonicalise the element by value, so T must be a hashable
            // primitive (enforced at set construction, re-checked here for
            // sets that only ever arrive through annotations).
            (Ty::Generic { base: TypeCtor::Set, args: a }, "add") if a.len() == 1 => {
                if args.len() != 1 {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        "Set.add takes 1 argument: (value)".into()));
                }
                check_set_elem_ty(&a[0], span)?;
                let _ = self.check_expr(&args[0].value, &a[0], env, ctx, r)?;
                return Ok(Ty::Primitive(PrimTy::Unit));
            }
            (Ty::Generic { base: TypeCtor::Set, args: a }, "has" | "contains") if a.len() == 1 => {
                if args.len() != 1 {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        "Set.has takes 1 argument: (value)".into()));
                }
                check_set_elem_ty(&a[0], span)?;
                let _ = self.check_expr(&args[0].value, &a[0], env, ctx, r)?;
                return Ok(Ty::Primitive(PrimTy::Bool));
            }
            (Ty::Generic { base: TypeCtor::Set, args: _ }, "length" | "len") => {
                if !args.is_empty() {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        "Set.length takes no arguments".into()));
                }
                return Ok(Ty::Primitive(PrimTy::I64));
            }
            // `d.remove(k) -> bool` — true iff the key was present. The
            // statement form `del d[k]` lowers to the same native and
            // discards the bool; the method form exists for callers that
            // need to observe whether anything was evicted (LRU caches,
            // session stores).
            (Ty::Generic { base: TypeCtor::Dict, args: a }, "remove") if a.len() == 2 => {
                if args.len() != 1 {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        "Dict.remove takes 1 argument: (key)".into()));
                }
                let _ = self.check_expr(&args[0].value, &a[0], env, ctx, r)?;
                return Ok(Ty::Primitive(PrimTy::Bool));
            }
            // str methods — stdlib: wordcount.spy
            (Ty::Primitive(PrimTy::Str), "slice") => {
                for a in args { let _ = self.check_or_synth(&a.value, Some(&Ty::Primitive(PrimTy::I64)), env, ctx, r)?; }
                return Ok(Ty::Primitive(PrimTy::Str));
            }
            (Ty::Primitive(PrimTy::Str), "char_at") => {
                for a in args { let _ = self.check_or_synth(&a.value, Some(&Ty::Primitive(PrimTy::I64)), env, ctx, r)?; }
                return Ok(Ty::Primitive(PrimTy::Char));
            }
            (Ty::Primitive(PrimTy::Str), "len" | "length") => {
                return Ok(Ty::Primitive(PrimTy::I64));
            }
            // real-world: csv_aggregate / wordcount / markov all need a
            // string splitter. `s.split(sep) -> List[str]`.
            (Ty::Primitive(PrimTy::Str), "split") => {
                for a in args { let _ = self.check_or_synth(&a.value, Some(&Ty::Primitive(PrimTy::Str)), env, ctx, r)?; }
                return Ok(Ty::Generic {
                    base: TypeCtor::List,
                    args: vec![Ty::Primitive(PrimTy::Str)],
                });
            }
            // P1: native string methods (text-processing perf).
            (Ty::Primitive(PrimTy::Str), "strip" | "lstrip" | "rstrip") => {
                return Ok(Ty::Primitive(PrimTy::Str));
            }
            (Ty::Primitive(PrimTy::Str), "find") => {
                for a in args { let _ = self.check_or_synth(&a.value, Some(&Ty::Primitive(PrimTy::Str)), env, ctx, r)?; }
                return Ok(Ty::Primitive(PrimTy::I64));
            }
            (Ty::Primitive(PrimTy::Str), "replace") => {
                for a in args { let _ = self.check_or_synth(&a.value, Some(&Ty::Primitive(PrimTy::Str)), env, ctx, r)?; }
                return Ok(Ty::Primitive(PrimTy::Str));
            }
            (Ty::Primitive(PrimTy::Str), "startswith" | "endswith" | "contains") => {
                for a in args { let _ = self.check_or_synth(&a.value, Some(&Ty::Primitive(PrimTy::Str)), env, ctx, r)?; }
                return Ok(Ty::Primitive(PrimTy::Bool));
            }
            // Strings round 2: native join/lower/upper/repeat.
            // `sep.join(xs: List[str]) -> str` — the receiver is the
            // separator. NOTE: must be intercepted here (and in the IR's
            // str-receiver dispatch) because the name `join` otherwise
            // resolves to Thread.join via NativeFn::from_name.
            (Ty::Primitive(PrimTy::Str), "join") => {
                if args.len() != 1 {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        "str.join takes 1 argument: (xs: List[str])".into()));
                }
                let want = Ty::Generic {
                    base: TypeCtor::List,
                    args: vec![Ty::Primitive(PrimTy::Str)],
                };
                let _ = self.check_or_synth(&args[0].value, Some(&want), env, ctx, r)?;
                return Ok(Ty::Primitive(PrimTy::Str));
            }
            (Ty::Primitive(PrimTy::Str), "lower" | "upper") => {
                if !args.is_empty() {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        format!("str.{method}() takes no arguments")));
                }
                return Ok(Ty::Primitive(PrimTy::Str));
            }
            (Ty::Primitive(PrimTy::Str), "repeat") => {
                if args.len() != 1 {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        "str.repeat takes 1 argument: (n: i64)".into()));
                }
                let _ = self.check_or_synth(&args[0].value, Some(&Ty::Primitive(PrimTy::I64)), env, ctx, r)?;
                return Ok(Ty::Primitive(PrimTy::Str));
            }
            // ── LANE E: expanded str methods (item 1). These mirror the
            // CPython str API.  All take str/i64 args and dispatch via
            // `NativeFn::from_name` (collision-free names), so no ir.rs
            // change is needed.  Impls live in vm/src/builtins.rs.
            //
            // search family: count / index / rindex return i64;
            // index/rindex raise ValueError at runtime when absent.
            (Ty::Primitive(PrimTy::Str), "count" | "rfind" | "index" | "rindex") => {
                if args.len() != 1 {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        format!("str.{method} takes 1 argument: (sub: str)")));
                }
                let _ = self.check_or_synth(&args[0].value, Some(&Ty::Primitive(PrimTy::Str)), env, ctx, r)?;
                return Ok(Ty::Primitive(PrimTy::I64));
            }
            // splitlines() -> List[str]
            (Ty::Primitive(PrimTy::Str), "splitlines") => {
                if !args.is_empty() {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        "str.splitlines() takes no arguments".into()));
                }
                return Ok(Ty::Generic {
                    base: TypeCtor::List,
                    args: vec![Ty::Primitive(PrimTy::Str)],
                });
            }
            // partition / rpartition(sep) -> (str, str, str)
            (Ty::Primitive(PrimTy::Str), "partition" | "rpartition") => {
                if args.len() != 1 {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        format!("str.{method} takes 1 argument: (sep: str)")));
                }
                let _ = self.check_or_synth(&args[0].value, Some(&Ty::Primitive(PrimTy::Str)), env, ctx, r)?;
                return Ok(Ty::Tuple(vec![
                    Ty::Primitive(PrimTy::Str),
                    Ty::Primitive(PrimTy::Str),
                    Ty::Primitive(PrimTy::Str),
                ]));
            }
            // padding family: width is i64, fill defaults to ' ' (1 arg only)
            (Ty::Primitive(PrimTy::Str), "zfill" | "ljust" | "rjust" | "center") => {
                if args.len() != 1 {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        format!("str.{method} takes 1 argument: (width: i64)")));
                }
                let _ = self.check_or_synth(&args[0].value, Some(&Ty::Primitive(PrimTy::I64)), env, ctx, r)?;
                return Ok(Ty::Primitive(PrimTy::Str));
            }
            // case/format family (no args) -> str
            (Ty::Primitive(PrimTy::Str),
                "title" | "swapcase" | "casefold" | "capitalize") => {
                if !args.is_empty() {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        format!("str.{method}() takes no arguments")));
                }
                return Ok(Ty::Primitive(PrimTy::Str));
            }
            // predicate family (no args) -> bool
            (Ty::Primitive(PrimTy::Str),
                "isdigit" | "isalpha" | "isalnum" | "isspace" | "isupper" | "islower") => {
                if !args.is_empty() {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        format!("str.{method}() takes no arguments")));
                }
                return Ok(Ty::Primitive(PrimTy::Bool));
            }
            // removeprefix / removesuffix(fix: str) -> str
            (Ty::Primitive(PrimTy::Str), "removeprefix" | "removesuffix") => {
                if args.len() != 1 {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        format!("str.{method} takes 1 argument: (fix: str)")));
                }
                let _ = self.check_or_synth(&args[0].value, Some(&Ty::Primitive(PrimTy::Str)), env, ctx, r)?;
                return Ok(Ty::Primitive(PrimTy::Str));
            }
            // expandtabs(tabsize: i64 = 8) -> str — accepts 0 or 1 arg
            (Ty::Primitive(PrimTy::Str), "expandtabs") => {
                if args.len() > 1 {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        "str.expandtabs takes at most 1 argument: (tabsize: i64)".into()));
                }
                if let Some(a) = args.first() {
                    let _ = self.check_or_synth(&a.value, Some(&Ty::Primitive(PrimTy::I64)), env, ctx, r)?;
                }
                return Ok(Ty::Primitive(PrimTy::Str));
            }
            // Channel methods — stdlib: producer.spy
            (Ty::Generic { base: TypeCtor::Channel, args: a }, "send") if a.len() == 1 => {
                if args.len() == 1 {
                    let _ = self.check_expr(&args[0].value, &a[0], env, ctx, r)?;
                }
                return Ok(Ty::Primitive(PrimTy::Unit));
            }
            (Ty::Generic { base: TypeCtor::Channel, args: a }, "recv") if a.len() == 1 => {
                return Ok(a[0].clone());
            }
            (Ty::Generic { base: TypeCtor::Channel, args: a }, "try_recv") if a.len() == 1 => {
                return Ok(Ty::Nullable(Box::new(a[0].clone())));
            }
            (Ty::Generic { base: TypeCtor::Channel, args: _ }, "close") => {
                return Ok(Ty::Primitive(PrimTy::Unit));
            }
            // M32: Future[T] methods — `await()` returns T, `is_ready()`
            // returns bool.  Shape mirrors Channel[T] above.  Spec §9.43.2.
            (Ty::Generic { base: TypeCtor::Future, args: a }, "await") if a.len() == 1 => {
                if !args.is_empty() {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        "Future.await() takes no arguments".into()));
                }
                return Ok(a[0].clone());
            }
            (Ty::Generic { base: TypeCtor::Future, args: _ }, "is_ready") => {
                if !args.is_empty() {
                    return Err(type_err(span, codes::TYPE_ARITY,
                        "Future.is_ready() takes no arguments".into()));
                }
                return Ok(Ty::Primitive(PrimTy::Bool));
            }
            _ => {}
        }
        // Class / protocol method lookup.
        let cid = match recv {
            Ty::Class(c) => Some(*c),
            Ty::Generic { base: TypeCtor::Class(c), .. } => Some(*c),
            _ => None,
        };
        // M31: if the receiver is a parameterised class, build the
        // tv → concrete substitution so the method's parameter & return
        // types specialise correctly (e.g. `Box[i64].unwrap()` returns i64,
        // not the raw Ty::Var(0)).
        let recv_subst: HashMap<u32, Ty> = match recv {
            Ty::Generic { base: TypeCtor::Class(c), args } => {
                if let Some(cl) = ctx.classes.get(c) {
                    let mut s = HashMap::new();
                    for (tv, arg) in cl.generic_tvars.iter().zip(args.iter()) {
                        s.insert(tv.0, arg.clone());
                    }
                    s
                } else { HashMap::new() }
            }
            _ => HashMap::new(),
        };
        if let Some(cid) = cid {
            // Walk class chain.
            let mut cur = Some(cid);
            while let Some(c) = cur {
                if let Some(cl) = ctx.classes.get(&c) {
                    if let Some(m) = cl.methods.iter().find(|m| m.name == method) {
                        let ret_ty = subst_ty(&m.ret, &recv_subst);
                        // M61b: default + keyword binding when we can recover
                        // the method's AST parameters. `recv_subst` specialises
                        // generic-class method param types before checking.
                        if let Some(ast_params) = ast_method_params(r, &cl.name, method) {
                            let param_tys: Vec<Ty> =
                                m.params.iter().map(|pt| subst_ty(pt, &recv_subst)).collect();
                            let desc = format!("method `{method}`");
                            self.check_call_binding(
                                ast_params, &param_tys, args, span, &desc, env, ctx, r,
                            )?;
                            return Ok(ret_ty);
                        }
                        if args.len() != m.params.len() {
                            return Err(type_err(span, codes::TYPE_ARITY,
                                format!("method `{}` expects {} args, got {}", method, m.params.len(), args.len())));
                        }
                        for (a, pt) in args.iter().zip(m.params.iter()) {
                            let expected = subst_ty(pt, &recv_subst);
                            let _ = self.check_expr(&a.value, &expected, env, ctx, r)?;
                        }
                        return Ok(ret_ty);
                    }
                    cur = cl.base;
                } else { break; }
            }
        }
        if let Ty::Protocol(pid) = recv {
            if let Some(p) = ctx.protocols.get(pid) {
                if let Some(m) = p.methods.iter().find(|m| m.name == method) {
                    if args.len() != m.params.len() {
                        return Err(type_err(span, codes::TYPE_ARITY,
                            format!("method `{}` expects {} args, got {}", method, m.params.len(), args.len())));
                    }
                    for (a, pt) in args.iter().zip(m.params.iter()) {
                        let _ = self.check_expr(&a.value, pt, env, ctx, r)?;
                    }
                    return Ok(m.ret.clone());
                }
            }
        }
        Err(type_err(span, codes::TYPE_NO_METHOD,
            format!("no method `{}` on type {}", method, recv.display())))
    }

    fn attr_type(&self, obj_ty: &Ty, name: &str, span: Span, ctx: &Ctx) -> Result<Ty, CompileError> {
        if name == "__dict__" {
            return Err(type_err(span, codes::TYPE_DUNDER_DICT,
                "__dict__ access is not allowed".into()));
        }
        // M14 tuples: `t.0`, `t.1`, ... — numeric attr on a tuple type.
        if let Ty::Tuple(elems) = obj_ty {
            if let Ok(idx) = name.parse::<usize>() {
                if idx < elems.len() {
                    return Ok(elems[idx].clone());
                }
                return Err(type_err(span, codes::TYPE_NO_FIELD,
                    format!("tuple index {} out of bounds (arity {})", idx, elems.len())));
            }
        }
        let cid = match obj_ty {
            Ty::Class(c) => Some(*c),
            Ty::Generic { base: TypeCtor::Class(c), .. } => Some(*c),
            _ => None,
        };
        // M31: build the receiver's tv → concrete subst (empty for
        // non-parameterised classes / non-generic field types).
        let recv_subst: HashMap<u32, Ty> = match obj_ty {
            Ty::Generic { base: TypeCtor::Class(c), args } => {
                if let Some(cl) = ctx.classes.get(c) {
                    let mut s = HashMap::new();
                    for (tv, arg) in cl.generic_tvars.iter().zip(args.iter()) {
                        s.insert(tv.0, arg.clone());
                    }
                    s
                } else { HashMap::new() }
            }
            _ => HashMap::new(),
        };
        if let Some(cid) = cid {
            let mut cur = Some(cid);
            while let Some(c) = cur {
                if let Some(cl) = ctx.classes.get(&c) {
                    if let Some(f) = cl.fields.iter().find(|f| f.name == name) {
                        return Ok(subst_ty(&f.ty, &recv_subst));
                    }
                    cur = cl.base;
                } else { break; }
            }
        }
        Err(type_err(span, codes::TYPE_NO_FIELD,
            format!("no field `{}` on type {}", name, obj_ty.display())))
    }

    fn index_type(&self, obj_ty: &Ty, span: Span) -> Result<Ty, CompileError> {
        match obj_ty {
            Ty::Generic { base: TypeCtor::List, args } if args.len() == 1 => Ok(args[0].clone()),
            Ty::Generic { base: TypeCtor::Dict, args } if args.len() == 2 => Ok(args[1].clone()),
            Ty::Generic { base: TypeCtor::Set, args } if args.len() == 1 => Ok(args[0].clone()),
            Ty::Primitive(PrimTy::Str) => Ok(Ty::Primitive(PrimTy::Char)),
            Ty::Primitive(PrimTy::Bytes) => Ok(Ty::Primitive(PrimTy::U8)),
            Ty::Tuple(elems) if !elems.is_empty() => Ok(elems[0].clone()),
            // `Channel[T]` etc. when written `Channel[i32]` is parsed as Expr::Index
            // on an identifier — treat as a constructor expression returning Generic.
            Ty::Generic { base, args } => Ok(Ty::Generic { base: base.clone(), args: args.clone() }),
            // `List` (no args) indexed with a type yields a constructor — used as `List[i32]`
            // when the parser saw bare `List` followed by `[i32]`.
            _ => Err(type_err(span, codes::TYPE_NO_METHOD,
                format!("type {} is not indexable", obj_ty.display()))),
        }
    }

    /// Wave 2 / Lane C: type-check `obj[k]` (read) where `obj` is a user class.
    ///
    /// Returns `None` when the class does not define `__getitem__`, so the
    /// caller falls back to the built-in `index_type` error path (a class with
    /// no `__getitem__` is genuinely not subscriptable). Returns `Some(Ok(ret))`
    /// when the key checks out, where `ret` is the dunder's declared return
    /// type; `Some(Err(..))` for a bad arity or key-type mismatch.
    ///
    /// `obj_ty` is the (possibly parameterised) receiver type — its type
    /// arguments are substituted into the dunder's signature so a generic
    /// container's key/return types specialise correctly.
    fn class_index_get_type(
        &mut self,
        cid: ClassId,
        obj_ty: &Ty,
        indices: &[Expr],
        span: Span,
        env: &Env,
        ctx: &Ctx,
        r: &ResolvedModule,
    ) -> Option<Result<Ty, CompileError>> {
        let sig = lookup_dunder(ctx.classes, cid, "__getitem__")?;
        Some(self.check_index_dunder(
            obj_ty, "__getitem__", sig, indices, span, env, ctx, r,
        ))
    }

    /// Wave 2 / Lane C: type-check `obj[k] = v` (store target) where `obj` is a
    /// user class. Checks the key against `__setitem__`'s first parameter and
    /// returns the *value* parameter type, so the enclosing assignment checks
    /// the RHS against it. `None` when the class lacks `__setitem__`.
    fn class_index_set_type(
        &mut self,
        cid: ClassId,
        obj_ty: &Ty,
        indices: &[Expr],
        span: Span,
        env: &Env,
        ctx: &Ctx,
        r: &ResolvedModule,
    ) -> Option<Result<Ty, CompileError>> {
        let sig = lookup_dunder(ctx.classes, cid, "__setitem__")?;
        Some(self.check_index_dunder(
            obj_ty, "__setitem__", sig, indices, span, env, ctx, r,
        ))
    }

    /// Shared key-checking core for `__getitem__` / `__setitem__`.
    ///
    /// `__getitem__(self, key)` has one parameter (the key) and the result of a
    /// subscript-read is its return type. `__setitem__(self, key, value)` has
    /// two parameters; the subscript-store target's "type" (used to check the
    /// assignment RHS) is the *value* parameter. Both forms check the single
    /// index expression against the key parameter.
    #[allow(clippy::too_many_arguments)]
    fn check_index_dunder(
        &mut self,
        obj_ty: &Ty,
        dunder: &str,
        sig: &MethodSig,
        indices: &[Expr],
        span: Span,
        env: &Env,
        ctx: &Ctx,
        r: &ResolvedModule,
    ) -> Result<Ty, CompileError> {
        // Exactly one subscript index is supported (`obj[k]`); multi-index
        // subscripts (`obj[a, b]`) aren't part of the dunder protocol here.
        if indices.len() != 1 {
            return Err(type_err(span, codes::TYPE_ARITY,
                format!("`{}` takes exactly one key: `obj[k]`, got {} indices",
                        dunder, indices.len())));
        }
        let expected_params = if dunder == "__setitem__" { 2 } else { 1 };
        if sig.params.len() != expected_params {
            return Err(type_err(span, codes::TYPE_ARITY,
                format!("`{}` on {} must take {} parameter(s) after self, found {}",
                        dunder, obj_ty.display(), expected_params, sig.params.len())));
        }
        // Specialise the dunder's signature against the receiver's type args
        // (mirrors `recv_subst` in `synth_method_call`).
        let recv_subst: HashMap<u32, Ty> = match obj_ty {
            Ty::Generic { base: TypeCtor::Class(c), args } => {
                ctx.classes.get(c).map(|cl| {
                    cl.generic_tvars.iter().zip(args.iter())
                        .map(|(tv, arg)| (tv.0, arg.clone()))
                        .collect()
                }).unwrap_or_default()
            }
            _ => HashMap::new(),
        };
        let key_ty = subst_ty(&sig.params[0], &recv_subst);
        let _ = self.check_expr(&indices[0], &key_ty, env, ctx, r)?;
        // Read → return type; store → value (second) parameter type.
        let result = if dunder == "__setitem__" {
            subst_ty(&sig.params[1], &recv_subst)
        } else {
            subst_ty(&sig.ret, &recv_subst)
        };
        Ok(result)
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Helpers
// ─────────────────────────────────────────────────────────────────────────

/// The `ClassId` of a (possibly parameterised) user class receiver, or `None`
/// for any other type. Mirrors the `cid` extraction in `synth_method_call` so
/// the index dunders fire for both `Foo` and `Foo[T]` receivers.
fn class_cid_of(ty: &Ty) -> Option<ClassId> {
    match ty {
        Ty::Class(c) => Some(*c),
        Ty::Generic { base: TypeCtor::Class(c), .. } => Some(*c),
        _ => None,
    }
}

/// Set elements must canonicalise by value — the dict-backed set runtime
/// keys on the integer value / float bit pattern / string content (see
/// vm/src/builtins.rs::set_elem_key). Reject element types that would
/// otherwise silently fall back to pointer-identity keying. `Never` is
/// unreachable from source (set literals are non-empty) but tolerated.
fn check_set_elem_ty(elem: &Ty, span: Span) -> Result<(), CompileError> {
    let ok = match elem {
        Ty::Primitive(p) => {
            (p.is_numeric() && !matches!(p, PrimTy::BigInt))
                || matches!(p, PrimTy::Bool | PrimTy::Char | PrimTy::Str)
        }
        Ty::Never => true,
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(type_err(span, codes::TYPE_MISMATCH,
            format!("Set elements must be int/float/bool/char/str in v0.3, got {}",
                    elem.display())))
    }
}

/// `42` / `-42` with no width suffix (the forms whose type is still
/// negotiable — see the literal-coercion branch in `check_expr`).
fn is_unsuffixed_int_literal(e: &Expr) -> bool {
    match e {
        Expr::Literal { lit: Literal::Int { suffix, .. }, .. } => suffix.is_none(),
        Expr::Unary { op: UnaryOp::Neg | UnaryOp::Pos, operand, .. } => {
            is_unsuffixed_int_literal(operand)
        }
        _ => false,
    }
}

/// Lane A: can this operand take on `target` when an arithmetic binop is
/// checked against an expected numeric width? True for width-negotiable
/// numeric literals (and unary +/- of them), and recursively for arithmetic
/// sub-expressions built from such operands — so `x: i32 = 1 + 2 * 3` keeps
/// every literal at i32. A *suffixed* literal only adopts its own type.
/// Non-literal operands return false and are routed through the widening
/// synth path instead (which already pushes the LHS type into literal RHSs).
fn operand_can_adopt(e: &Expr, target: PrimTy) -> bool {
    match e {
        Expr::Literal { lit: Literal::Int { suffix, .. }, .. } => {
            match suffix {
                None => target.is_integer(),
                Some(s) => int_suffix_to_prim(*s) == target,
            }
        }
        Expr::Literal { lit: Literal::Float { suffix, .. }, .. } => {
            match suffix {
                None => target.is_float(),
                Some(crate::lexer::FloatSuffix::F32) => target == PrimTy::F32,
                Some(crate::lexer::FloatSuffix::F64) => target == PrimTy::F64,
            }
        }
        Expr::Unary { op: UnaryOp::Neg | UnaryOp::Pos, operand, .. } => {
            operand_can_adopt(operand, target)
        }
        Expr::Binary { op, lhs, rhs, .. } if matches!(op,
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::FloorDiv
            | BinOp::Rem | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor) =>
        {
            operand_can_adopt(lhs, target) && operand_can_adopt(rhs, target)
        }
        _ => false,
    }
}

/// Lane A: an int or float literal with no width suffix (so its type is still
/// negotiable). Used by the binop rule to keep `f64_var + 2` typed at f64.
fn is_unsuffixed_numeric_literal(e: &Expr) -> bool {
    match e {
        Expr::Literal { lit: Literal::Int { suffix, .. }, .. } => suffix.is_none(),
        Expr::Literal { lit: Literal::Float { suffix, .. }, .. } => suffix.is_none(),
        Expr::Unary { op: UnaryOp::Neg | UnaryOp::Pos, operand, .. } => {
            is_unsuffixed_numeric_literal(operand)
        }
        _ => false,
    }
}

/// Lane A: the common (widened) type for a numeric binary operation, or
/// `None` when no *lossless* implicit widening applies.
///
/// Conservative on purpose — only the conversions backed by an existing
/// lossless cast (`I64FromI32`, `F64FromI32`, `F64FromI64`, `f32 -> f64`)
/// widen automatically:
///   * `f64` with any numeric -> `f64`
///   * `f32` with another `f32` -> `f32`; `f32` with any int -> `f64`
///   * signed-int ladder: `i8/i16/i32` with `i64` -> `i64`; otherwise the
///     equal type. `i32`-class widens to `i64`.
/// Mixed signedness, `u32`/`u64`, and `i8`/`i16`/`u8`/`u16` against a
/// *different* width still require an exact match (returns `None` unless equal).
fn numeric_common_ty(a: PrimTy, b: PrimTy) -> Option<PrimTy> {
    use PrimTy::*;
    if a == b {
        return Some(a);
    }
    // Float promotions.
    if a == F64 || b == F64 {
        // f64 absorbs any numeric.
        if a.is_numeric() && b.is_numeric() { return Some(F64); }
        return None;
    }
    if a == F32 || b == F32 {
        // f32 mixed with an integer -> f64 (lossless via the int->f64 casts).
        let other = if a == F32 { b } else { a };
        if other.is_integer() { return Some(F64); }
        return None;
    }
    // Both integers, different widths. Only widen within the signed ladder
    // where a lossless cast exists (i8/i16/i32 -> i64).
    let signed_small = |p: PrimTy| matches!(p, I8 | I16 | I32);
    if a == I64 && signed_small(b) { return Some(I64); }
    if b == I64 && signed_small(a) { return Some(I64); }
    // i8/i16 with i32 -> i32 (both fit; widen to the larger signed width).
    if matches!(a, I8 | I16) && b == I32 { return Some(I32); }
    if matches!(b, I8 | I16) && a == I32 { return Some(I32); }
    // Everything else (mixed signedness, u32/u64, disjoint small widths)
    // stays an error to avoid lossy/unsound implicit conversions.
    None
}

fn type_err(span: Span, code: ErrorCode, message: String) -> CompileError {
    CompileError::Type {
        file: String::new(), line: span.line, col: span.col, code, message,
    }
}

/// True iff `k` is an acceptable Dict key type. The runtime dict keys on
/// strings only (`vm/src/strdict.rs`), so a non-`str` key would crash at
/// subscript. `Never` is allowed (it is the key of an empty `{}` literal —
/// it never reaches a subscript with a real key). See `ty_first_bad_dict_key`.
fn is_valid_dict_key(k: &Ty) -> bool {
    matches!(k, Ty::Primitive(PrimTy::Str) | Ty::Never)
}

/// Recursively search a type for any `Dict[K, V]` whose key type `K` is not
/// `str`. Returns the first offending key type found (so the error message can
/// name it), or `None` if every nested Dict keys on `str`.
///
/// Lane D / Wave-1: the typechecker previously placed no restriction on K, so
/// `Dict[i64, V]` compiled and then SEGFAULTed at the hardcoded-string-key
/// runtime dict. This closes that footgun by rejecting the type.
fn ty_first_bad_dict_key(t: &Ty) -> Option<&Ty> {
    match t {
        Ty::Generic { base: TypeCtor::Dict, args } if args.len() == 2 => {
            if !is_valid_dict_key(&args[0]) {
                return Some(&args[0]);
            }
            // Still recurse into K and V (e.g. a Dict value that is itself a
            // bad Dict).
            ty_first_bad_dict_key(&args[0]).or_else(|| ty_first_bad_dict_key(&args[1]))
        }
        Ty::Generic { args, .. } => args.iter().find_map(ty_first_bad_dict_key),
        Ty::Tuple(ts) => ts.iter().find_map(ty_first_bad_dict_key),
        Ty::Nullable(inner) => ty_first_bad_dict_key(inner),
        Ty::Function { params, ret } => params
            .iter()
            .find_map(ty_first_bad_dict_key)
            .or_else(|| ty_first_bad_dict_key(ret)),
        _ => None,
    }
}

/// M63b: verify each concrete type argument satisfies the bound (if any)
/// declared on the matching generic type parameter. `tvars` and `type_args`
/// are parallel (declaration order); bounds are looked up in
/// `r.generic_bounds` by tvar id. A type that does not satisfy its bound
/// yields `E2015`. Type parameters without a declared bound are unconstrained.
fn check_bounds_satisfied(
    tvars: &[TypeVarId],
    type_args: &[Ty],
    owner: &str,
    span: Span,
    r: &ResolvedModule,
) -> Result<(), CompileError> {
    for (tv, arg) in tvars.iter().zip(type_args.iter()) {
        if let Some(bound) = r.generic_bounds.get(tv) {
            // A concrete primitive must satisfy the bound directly. A type
            // argument that is *itself* a bounded type variable (transitive
            // instantiation, e.g. `quicksort[T: Comparable]` forwarding `T`
            // into `partition[T: Comparable]`) satisfies the requirement when
            // the caller's bound is at least as strong — modelled here by
            // requiring the same bound. Unbounded vars fail.
            let ok = match arg {
                Ty::Var(arg_tv) => bound_implies(r.generic_bounds.get(arg_tv).copied(), *bound),
                _ => bound.satisfied_by(arg),
            };
            if !ok {
                return Err(type_err(
                    span,
                    codes::TYPE_UNSATISFIED_BOUND,
                    format!(
                        "type `{}` does not satisfy bound `{}` required by `{}`",
                        arg.display(),
                        bound.name(),
                        owner
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// M63b: does a type parameter carrying `have` satisfy a requirement for
/// `need`? `Comparable` implies `Equatable` and `Printable`; `Equatable`
/// implies only `Equatable`; an unbounded parameter (`have == None`) implies
/// nothing.
fn bound_implies(have: Option<BoundKind>, need: BoundKind) -> bool {
    match have {
        None => false,
        Some(h) if h == need => true,
        Some(BoundKind::Comparable) => {
            matches!(need, BoundKind::Equatable | BoundKind::Printable)
        }
        Some(_) => false,
    }
}

// ── M61b: AST parameter lookup for default + keyword binding ──────────────
//
// The semantic `FunctionSig` / `MethodSig` carry parameter *types* but not
// names or default expressions, so the call binder ([`argbind`]) recovers the
// declared parameters from the original AST. These helpers find the relevant
// `&[ast::Param]`, with `self` already stripped for methods/constructors so
// the slice lines up 1:1 with the call's argument list.

/// AST parameters of a free function by name (module-level decls only).
fn ast_free_fn_params<'a>(r: &'a ResolvedModule, name: &str) -> Option<&'a [ast::Param]> {
    for d in &r.module.decls {
        if let TopDecl::Func(f) = d {
            if f.name == name {
                return Some(&f.params);
            }
        }
    }
    None
}

/// WAVE-2 LANE-0 (scaffold): resolve a dunder method's *signature* on a user
/// class, walking the inheritance chain.
///
/// Reusable foundation for the operator-overloading / protocol lanes: given a
/// class id and a dunder name (`"__add__"`, `"__str__"`, `"__getitem__"`,
/// `"__iter__"`, `"__next__"`, `"__eq__"`, `"__lt__"`, …), it returns the
/// declared [`MethodSig`] so a feature lane can type-check operands against
/// the dunder's declared `params` (the parameters *after* `self`) and adopt
/// its declared `ret` type.
///
/// Contract:
/// - Returns `Some(&MethodSig)` when the class defines **or inherits** the
///   dunder; `None` otherwise.
/// - Resolution mirrors normal method lookup (typecheck.rs ~2732-2763): the
///   resolver already flattens inherited methods into `layout.methods`
///   (resolver.rs ~7532-7568), so a name lookup on the class resolves
///   inherited dunders; as belt-and-braces we still walk the `base` chain so
///   the helper is correct even for layouts that haven't been flattened.
/// - The returned signature's `params` exclude the implicit `self` (that is
///   how [`MethodSig`] is built — see resolver.rs::build_method_sig).
///
/// Note: for a *generic* receiver class, the returned types may contain the
/// class's `Ty::Var` type parameters; a feature lane should apply the
/// receiver's type-argument substitution (as the method-call checker does via
/// `recv_subst`) before comparing against concrete operand types.
///
/// `classes` is the same map carried by the type-check `Ctx`; pass
/// `ctx.classes`.
#[allow(dead_code)]
fn lookup_dunder<'a>(
    classes: &'a HashMap<ClassId, ClassLayout>,
    cid: ClassId,
    dunder: &str,
) -> Option<&'a MethodSig> {
    let mut cur = Some(cid);
    while let Some(c) = cur {
        let layout = classes.get(&c)?;
        if let Some(m) = layout.methods.iter().find(|m| m.name == dunder) {
            return Some(m);
        }
        cur = layout.base;
    }
    None
}

/// WAVE-2 LANE-B: the dunder method name a binary operator dispatches to when
/// its left operand is a user-defined class, or `None` for operators that
/// never route through a dunder (`and`/`or`/`is`/`is not`/`in`/`not in`).
///
/// Identity (`is`/`is not`) deliberately stays pointer-identity and is *not*
/// listed here. `!=` maps to `__ne__`; when a class defines `__eq__` but not
/// `__ne__`, the caller synthesises `not __eq__` (see `check_binary` /
/// `emit_binop`). Ordering operators each require their *own* dunder — there
/// is no synthesis of `>`/`>=` from `<`/`==` (documented in STRICTPY_SPEC).
fn binop_dunder(op: BinOp) -> Option<&'static str> {
    Some(match op {
        BinOp::Add => "__add__",
        BinOp::Sub => "__sub__",
        BinOp::Mul => "__mul__",
        BinOp::Div => "__truediv__",
        BinOp::FloorDiv => "__floordiv__",
        BinOp::Rem => "__mod__",
        BinOp::Pow => "__pow__",
        BinOp::Eq => "__eq__",
        BinOp::Ne => "__ne__",
        BinOp::Lt => "__lt__",
        BinOp::Le => "__le__",
        BinOp::Gt => "__gt__",
        BinOp::Ge => "__ge__",
        // Bitwise/shift dunders are out of scope for Lane B; identity,
        // membership and boolean operators never route to a dunder.
        _ => return None,
    })
}

/// AST parameters (minus `self`) of a class's `__init__`, by class name.
fn ast_ctor_params<'a>(r: &'a ResolvedModule, class_name: &str) -> Option<&'a [ast::Param]> {
    for d in &r.module.decls {
        if let TopDecl::Class(c) = d {
            if c.name == class_name {
                if let Some(init) = &c.init {
                    return Some(strip_self(&init.params));
                }
                return None;
            }
        }
    }
    None
}

/// AST parameters (minus `self`) of a named method on a class, walking the
/// inheritance chain by class name.
fn ast_method_params<'a>(
    r: &'a ResolvedModule,
    class_name: &str,
    method: &str,
) -> Option<&'a [ast::Param]> {
    for d in &r.module.decls {
        if let TopDecl::Class(c) = d {
            if c.name == class_name {
                if let Some(m) = c.methods.iter().find(|m| m.name == method) {
                    return Some(strip_self(&m.params));
                }
            }
        }
    }
    None
}

/// Drop a leading implicit `self` parameter, if present.
fn strip_self(params: &[ast::Param]) -> &[ast::Param] {
    match params.first() {
        Some(p) if p.name == "self" => &params[1..],
        _ => params,
    }
}

fn expr_span(e: &Expr) -> Span {
    match e {
        Expr::Literal { span, .. } | Expr::Ident { span, .. } | Expr::Tuple { span, .. }
        | Expr::List { span, .. } | Expr::Dict { span, .. } | Expr::Set { span, .. }
        | Expr::Unary { span, .. } | Expr::Binary { span, .. } | Expr::Call { span, .. }
        | Expr::MethodCall { span, .. } | Expr::Attr { span, .. } | Expr::Index { span, .. }
        | Expr::NullCoalesce { span, .. } | Expr::Ternary { span, .. }
        | Expr::Lambda { span, .. } | Expr::Cast { span, .. }
        | Expr::Slice { span, .. }
        | Expr::Comprehension { span, .. } => *span,
    }
}

fn ast_type_span(t: &ast::Type) -> (u32, u32) {
    let s = match t {
        ast::Type::Named { span, .. } | ast::Type::Nullable { span, .. }
        | ast::Type::Function { span, .. } | ast::Type::Tuple { span, .. }
        | ast::Type::Infer { span } | ast::Type::Never { span } => span,
    };
    (s.start, s.end)
}

fn int_suffix_to_prim(s: crate::lexer::IntSuffix) -> PrimTy {
    use crate::lexer::IntSuffix::*;
    match s {
        I8 => PrimTy::I8, I16 => PrimTy::I16, I32 => PrimTy::I32, I64 => PrimTy::I64,
        U8 => PrimTy::U8, U16 => PrimTy::U16, U32 => PrimTy::U32, U64 => PrimTy::U64,
    }
}

/// Wave-2 Lane F: reject an integer literal whose value (kept at full `i128`
/// precision by the lexer) does not fit the integer type it resolves to.
///
/// The headline case is a bare literal outside the i64 range: it used to be
/// truncated silently to `i64` at IR materialisation (`9223372036854775808`
/// wrapped to `i64::MIN`). BigInt is not yet implemented, so this is a clean
/// `E2073` compile error rather than a silent wrap. Suffixed / typed-context
/// literals are also bounds-checked against their own width (so `256u8`
/// reports out-of-range here instead of wrapping at lowering). `BigInt` (which
/// has no fixed width) and non-integer resolved types are passed through.
fn check_int_literal_in_range(
    value: i128,
    prim: PrimTy,
    span: Span,
) -> Result<(), CompileError> {
    let (lo, hi): (i128, i128) = match prim {
        PrimTy::I8 => (i8::MIN as i128, i8::MAX as i128),
        PrimTy::I16 => (i16::MIN as i128, i16::MAX as i128),
        PrimTy::I32 => (i32::MIN as i128, i32::MAX as i128),
        PrimTy::I64 => (i64::MIN as i128, i64::MAX as i128),
        PrimTy::U8 => (u8::MIN as i128, u8::MAX as i128),
        PrimTy::U16 => (u16::MIN as i128, u16::MAX as i128),
        PrimTy::U32 => (u32::MIN as i128, u32::MAX as i128),
        PrimTy::U64 => (u64::MIN as i128, u64::MAX as i128),
        // BigInt is arbitrary-precision (no fixed bounds); anything else isn't
        // an integer type and is handled elsewhere.
        _ => return Ok(()),
    };
    if value < lo || value > hi {
        return Err(type_err(
            span,
            codes::TYPE_INT_LITERAL_OUT_OF_RANGE,
            format!(
                "integer literal {value} out of range for {prim:?}; \
                 BigInt not yet supported"
            ),
        ));
    }
    Ok(())
}

/// M15: built-in exception-class names recognised in `raise` and `except`.
/// The catch-all `"Exception"` matches any thrown type at runtime; the others
/// match by exact `type_name` string. Must mirror the resolver's prelude
/// list (`resolver.rs::install_prelude`).
pub fn is_builtin_exception_name(name: &str) -> bool {
    matches!(
        name,
        "Exception"
            | "ValueError"
            | "IndexError"
            | "KeyError"
            | "TypeError"
            | "OverflowError"
            | "DivisionByZeroError"
            | "ZeroDivisionError"
            | "IOError"
            | "NullPointerError"
            | "AssertionError"
            | "RuntimeError"
            | "StopIteration"
            | "ChannelClosedError"
    )
}

fn prim_from_name_unchecked(name: &str) -> PrimTy {
    match name {
        "i8" => PrimTy::I8, "i16" => PrimTy::I16, "i32" => PrimTy::I32, "i64" => PrimTy::I64,
        "u8" => PrimTy::U8, "u16" => PrimTy::U16, "u32" => PrimTy::U32, "u64" => PrimTy::U64,
        "f32" => PrimTy::F32, "f64" => PrimTy::F64,
        _ => PrimTy::I32,
    }
}

/// Least-upper-bound (just enough to make tests pass).
/// M61a: is a value of type `from` acceptable where `to` is expected?
/// Used by the higher-order builtins (`map`/`filter`/`reduce`/`sorted_by`)
/// to check the user callback's parameter/return types against the list
/// element / accumulator types. Permissive on numeric widths and nullable
/// widening (same policy `lub` encodes); the runtime is the final guard.
fn type_assignable(from: &Ty, to: &Ty) -> bool {
    if ty_eq(from, to) {
        return true;
    }
    // `lub` returning `to` (or a supertype) means `from` widens into `to`.
    match lub(from, to) {
        Some(l) => ty_eq(&l, to) || is_subtype_trivial(from, to),
        None => false,
    }
}

/// M61a: a key for `sorted_by`/`sort_by` must be a comparable primitive —
/// an integer/float width or `str`. (This sidesteps full comparator
/// generics; key-based ordering is enough for v1.)
fn is_comparable_key_ty(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Primitive(
            PrimTy::I8 | PrimTy::I16 | PrimTy::I32 | PrimTy::I64
                | PrimTy::U8 | PrimTy::U16 | PrimTy::U32 | PrimTy::U64
                | PrimTy::F32 | PrimTy::F64
                | PrimTy::Str
        )
    )
}

fn lub(a: &Ty, b: &Ty) -> Option<Ty> {
    if ty_eq(a, b) { return Some(a.clone()); }
    if is_subtype_trivial(a, b) { return Some(b.clone()); }
    if is_subtype_trivial(b, a) { return Some(a.clone()); }
    // T and T? -> T?
    if let Ty::Nullable(inner) = a { if ty_eq(inner, b) { return Some(a.clone()); } }
    if let Ty::Nullable(inner) = b { if ty_eq(inner, a) { return Some(b.clone()); } }
    if matches!(a, Ty::Primitive(PrimTy::Null)) { return Some(Ty::Nullable(Box::new(b.clone()))); }
    if matches!(b, Ty::Primitive(PrimTy::Null)) { return Some(Ty::Nullable(Box::new(a.clone()))); }
    None
}

/// M34: parameter types for JsonValue subclass constructors.  These
/// classes have no user-level `__init__`; the IR special-cases them
/// (see `m34_json_class_init_native_id` in ir.rs) but the type-checker
/// needs to know what argument shape to accept.  Returns `None` for
/// any class outside the JsonValue family — the caller then falls back
/// to the normal "no __init__" handling.
///
/// The list/tuple-typed signatures (JList / JObject) reference the
/// JsonValue class id; we look it up in the type context's classes
/// map to construct the right `Ty::Class(cid)`.
fn m34_json_ctor_param_tys(class_name: &str, ctx: &Ctx<'_>) -> Option<Vec<Ty>> {
    let jsonvalue_cid = ctx.classes.values()
        .find(|cl| cl.name == "JsonValue")
        .map(|cl| cl.id)?;
    let jv_ty = Ty::Class(jsonvalue_cid);
    Some(match class_name {
        "JNull"   => vec![],
        "JBool"   => vec![Ty::Primitive(PrimTy::Bool)],
        "JInt"    => vec![Ty::Primitive(PrimTy::I64)],
        "JFloat"  => vec![Ty::Primitive(PrimTy::F64)],
        "JString" => vec![Ty::Primitive(PrimTy::Str)],
        "JList"   => vec![Ty::Generic {
            base: TypeCtor::List,
            args: vec![jv_ty.clone()],
        }],
        "JObject" => vec![Ty::Generic {
            base: TypeCtor::List,
            args: vec![Ty::Tuple(vec![
                Ty::Primitive(PrimTy::Str),
                jv_ty,
            ])],
        }],
        _ => return None,
    })
}

/// M35 P4-B: parameter types for the typed sqlite3 class
/// constructors (`Connection(handle)` / `Cursor(handle)`).  Both take
/// a single `handle: i64` — the slot index into the matching
/// `SharedVm` table.  Like `m34_json_ctor_param_tys`, returns `None`
/// for any class outside the family.
fn m35_p4b_sqlite_ctor_param_tys(class_name: &str) -> Option<Vec<Ty>> {
    Some(match class_name {
        "Connection" => vec![Ty::Primitive(PrimTy::I64)],
        "Cursor"     => vec![Ty::Primitive(PrimTy::I64)],
        _ => return None,
    })
}

/// True iff every control path through `b` exits via `return`/`raise`/`break`/`continue`.
/// Conservative: only detects trivial `return`/`raise` at the end.
fn block_always_returns(b: &Block) -> bool {
    for s in b.stmts.iter().rev() {
        match s {
            Stmt::Return { .. } | Stmt::Raise { .. } => return true,
            _ => return false,
        }
    }
    false
}

/// M62b: does this block (recursively, through nested control flow but NOT
/// through nested function/lambda bodies — there are none at statement level)
/// contain a `yield`? Used to flag a `-> Iterator[T]` function that forgot to
/// yield.
fn block_contains_yield(b: &Block) -> bool {
    b.stmts.iter().any(stmt_contains_yield)
}

fn stmt_contains_yield(s: &Stmt) -> bool {
    match s {
        Stmt::Yield { .. } => true,
        Stmt::If { then_block, elifs, else_block, .. } => {
            block_contains_yield(then_block)
                || elifs.iter().any(|(_, b)| block_contains_yield(b))
                || else_block.as_ref().map_or(false, block_contains_yield)
        }
        Stmt::While { body, else_block, .. } => {
            block_contains_yield(body)
                || else_block.as_ref().map_or(false, block_contains_yield)
        }
        Stmt::For { body, else_block, .. } => {
            block_contains_yield(body)
                || else_block.as_ref().map_or(false, block_contains_yield)
        }
        Stmt::Match { arms, .. } => arms.iter().any(|a| block_contains_yield(&a.body)),
        Stmt::Try { body, handlers, else_block, finally_block, .. } => {
            block_contains_yield(body)
                || handlers.iter().any(|h| block_contains_yield(&h.body))
                || else_block.as_ref().map_or(false, block_contains_yield)
                || finally_block.as_ref().map_or(false, block_contains_yield)
        }
        Stmt::With { body, .. } => block_contains_yield(body),
        _ => false,
    }
}

/// Narrowings to apply in `then` and `else` branches of an `if`.
#[derive(Default, Clone)]
struct Narrowing {
    entries: Vec<(SymbolId, Ty)>,
}

fn apply_narrows(env: &mut Env, n: &Narrowing) {
    for (sid, t) in &n.entries {
        env.types.insert(*sid, t.clone());
    }
}

fn narrowings_from_cond(cond: &Expr, r: &ResolvedModule, env: &Env) -> (Narrowing, Narrowing) {
    // Recognize `x is none`, `x is not none`, `x == none`, `x != none`, and
    // M16's `isinstance(x, T)`.
    let mut then_n = Narrowing::default();
    let mut else_n = Narrowing::default();
    // M16: `isinstance(x, T)` — narrow `x` to T inside the then-branch.
    // We don't narrow in the else-branch (could be a sibling subclass or
    // anything else); the spec calls this out.
    if let Expr::Call { callee, args, .. } = cond {
        if let Expr::Ident { name, .. } = callee.as_ref() {
            if name == "isinstance" && args.len() == 2 {
                if let Expr::Ident { span: xspan, .. } = &args[0].value {
                    if let Expr::Ident { name: tname, .. } = &args[1].value {
                        if let Some(sid_t) = r.symbols.lookup(r.module_scope, tname) {
                            let s = r.symbols.get(sid_t);
                            if matches!(s.kind, SymbolKind::Class) {
                                if let Some(cid) = s.class_id {
                                    if let Some(sid_x) = r.ident_to_symbol.get(&(xspan.start, xspan.end)) {
                                        then_n.entries.push((*sid_x, Ty::Class(cid)));
                                        let _ = env;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if let Expr::Binary { op, lhs, rhs, .. } = cond {
        let (is_none, is_negated) = match op {
            BinOp::Is => (matches!(rhs.as_ref(), Expr::Literal { lit: Literal::None, .. }), false),
            BinOp::IsNot => (matches!(rhs.as_ref(), Expr::Literal { lit: Literal::None, .. }), true),
            BinOp::Eq => (matches!(rhs.as_ref(), Expr::Literal { lit: Literal::None, .. }), false),
            BinOp::Ne => (matches!(rhs.as_ref(), Expr::Literal { lit: Literal::None, .. }), true),
            _ => (false, false),
        };
        if is_none {
            if let Expr::Ident { span, .. } = lhs.as_ref() {
                if let Some(sid) = r.ident_to_symbol.get(&(span.start, span.end)) {
                    let base = env.types.get(sid).cloned()
                        .or_else(|| r.symbols.get(*sid).ty.clone())
                        .unwrap_or(Ty::Never);
                    let unwrapped = base.unwrap_nullable();
                    let null_ty = Ty::Primitive(PrimTy::Null);
                    if is_negated {
                        then_n.entries.push((*sid, unwrapped));
                        else_n.entries.push((*sid, null_ty));
                    } else {
                        then_n.entries.push((*sid, null_ty));
                        else_n.entries.push((*sid, unwrapped));
                    }
                }
            }
        }
    }
    (then_n, else_n)
}

// ─────────────────────────────────────────────────────────────────────────
//  M17: generic-call unification + substitution helpers.
// ─────────────────────────────────────────────────────────────────────────

/// First-order unification: walk `pat` (which may contain `Ty::Var`s) against
/// `concrete` (which shouldn't), extending `subst`. Returns an error string
/// when the structures conflict.
///
/// Conflicts at the *generic-parameter* level (same `Ty::Var` bound to two
/// concrete types) trigger an error; downstream the type-checker re-runs
/// against each substitution so that instantiation-specific operator failures
/// (e.g. `T + T` where `T := bool`) still surface as a normal `E2*` error.
pub(crate) fn unify_one(pat: &Ty, concrete: &Ty, subst: &mut HashMap<u32, Ty>) -> Result<(), String> {
    match (pat, concrete) {
        (Ty::Var(TypeVarId(id)), c) => {
            if let Some(existing) = subst.get(id).cloned() {
                // Already bound — require structural equality with `c`.
                if !ty_eq(&existing, c) {
                    return Err(format!(
                        "type parameter ?T{} solved to both {} and {}",
                        id, existing.display(), c.display()
                    ));
                }
                Ok(())
            } else {
                subst.insert(*id, c.clone());
                Ok(())
            }
        }
        (Ty::Primitive(p), Ty::Primitive(q)) if p == q => Ok(()),
        (Ty::Class(a), Ty::Class(b)) if a == b => Ok(()),
        (Ty::Protocol(a), Ty::Protocol(b)) if a == b => Ok(()),
        (Ty::Generic { base: b1, args: a1 }, Ty::Generic { base: b2, args: a2 })
            if b1 == b2 && a1.len() == a2.len() =>
        {
            for (x, y) in a1.iter().zip(a2) { unify_one(x, y, subst)?; }
            Ok(())
        }
        (Ty::Tuple(a), Ty::Tuple(b)) if a.len() == b.len() => {
            for (x, y) in a.iter().zip(b) { unify_one(x, y, subst)?; }
            Ok(())
        }
        (Ty::Nullable(a), Ty::Nullable(b)) => unify_one(a, b, subst),
        // T may be inferred from a non-null concrete type passed to a `T?` slot
        // — but only the inner type is bound. The reverse (param: T, arg: U?)
        // would lose nullability and is rejected here.
        (Ty::Nullable(a), c) => unify_one(a, c, subst),
        (Ty::Function { params: pp, ret: pr },
         Ty::Function { params: cp, ret: cr })
            if pp.len() == cp.len() =>
        {
            for (x, y) in pp.iter().zip(cp) { unify_one(x, y, subst)?; }
            unify_one(pr, cr, subst)
        }
        (a, b) if ty_eq(a, b) => Ok(()),
        (a, b) => Err(format!("cannot unify {} with {}", a.display(), b.display())),
    }
}

/// Does `t` reference any `Ty::Var`? Used by the generic-call helper to
/// decide whether `check_expr` (no, expected type is fully ground) or
/// `synth_expr + unify` (yes) is the right strategy for an argument.
pub(crate) fn contains_unbound_var(t: &Ty) -> bool {
    match t {
        Ty::Var(_) => true,
        Ty::Generic { args, .. } | Ty::Tuple(args) => args.iter().any(contains_unbound_var),
        Ty::Function { params, ret } => {
            params.iter().any(contains_unbound_var) || contains_unbound_var(ret)
        }
        Ty::Nullable(inner) => contains_unbound_var(inner),
        _ => false,
    }
}

/// Apply a substitution to a type. Unbound vars are left in place (caller
/// decides whether that's an error).
pub(crate) fn subst_ty(t: &Ty, subst: &HashMap<u32, Ty>) -> Ty {
    match t {
        Ty::Var(TypeVarId(id)) => subst.get(id).cloned().unwrap_or_else(|| t.clone()),
        Ty::Generic { base, args } => Ty::Generic {
            base: base.clone(),
            args: args.iter().map(|a| subst_ty(a, subst)).collect(),
        },
        Ty::Function { params, ret } => Ty::Function {
            params: params.iter().map(|p| subst_ty(p, subst)).collect(),
            ret: Box::new(subst_ty(ret, subst)),
        },
        Ty::Tuple(xs) => Ty::Tuple(xs.iter().map(|x| subst_ty(x, subst)).collect()),
        Ty::Nullable(inner) => Ty::Nullable(Box::new(subst_ty(inner, subst))),
        _ => t.clone(),
    }
}

/// Mangle a list of types into a deterministic, debuggable suffix.
/// Examples: `i32`, `str_i64`, `tuple_i32_str`, `list_class3`.
pub fn mangle_args_key(args: &[Ty]) -> String {
    let mut out = String::new();
    for (i, t) in args.iter().enumerate() {
        if i > 0 { out.push('_'); }
        out.push_str(&mangle_ty(t));
    }
    out
}

fn mangle_ty(t: &Ty) -> String {
    match t {
        Ty::Primitive(p) => match p {
            PrimTy::Bool => "bool".into(),
            PrimTy::I8 => "i8".into(),  PrimTy::I16 => "i16".into(),
            PrimTy::I32 => "i32".into(), PrimTy::I64 => "i64".into(),
            PrimTy::U8 => "u8".into(),  PrimTy::U16 => "u16".into(),
            PrimTy::U32 => "u32".into(), PrimTy::U64 => "u64".into(),
            PrimTy::F32 => "f32".into(), PrimTy::F64 => "f64".into(),
            PrimTy::Char => "char".into(),
            PrimTy::Str => "str".into(),
            PrimTy::Bytes => "bytes".into(),
            PrimTy::BigInt => "bigint".into(),
            PrimTy::Null => "null".into(),
            PrimTy::Unit => "unit".into(),
        },
        Ty::Class(ClassId(id)) => format!("class{}", id),
        Ty::Protocol(ProtoId(id)) => format!("proto{}", id),
        Ty::Generic { base, args } => {
            let mut s = format!("{:?}", base).to_lowercase();
            for a in args { s.push('_'); s.push_str(&mangle_ty(a)); }
            s
        }
        Ty::Tuple(xs) => {
            let mut s = String::from("tuple");
            for x in xs { s.push('_'); s.push_str(&mangle_ty(x)); }
            s
        }
        Ty::Nullable(inner) => format!("opt_{}", mangle_ty(inner)),
        Ty::Function { .. } => "fn".into(),
        Ty::Never => "never".into(),
        Ty::Var(TypeVarId(id)) => format!("T{}", id),
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser as SpyParser;
    use crate::resolver::Resolver;

    fn parse(src: &str) -> crate::ast::Module {
        let mut lx = Lexer::new(src);
        let mut toks = Vec::new();
        loop {
            let t = lx.next_token().expect("lex");
            let eof = matches!(t.kind, crate::lexer::TokenKind::Eof);
            toks.push(t);
            if eof { break; }
        }
        SpyParser::new(toks).parse_module().expect("parse")
    }

    fn check_src(src: &str) -> Result<TypedModule, CompileError> {
        let m = parse(src);
        let resolved = Resolver::new().resolve(m)?;
        TypeChecker::new().check(resolved)
    }

    /// Resolve `src` and return the `class_layouts` map plus a
    /// `class-name -> ClassId` index, for exercising `lookup_dunder`.
    fn resolve_layouts(
        src: &str,
    ) -> (HashMap<ClassId, ClassLayout>, HashMap<String, ClassId>) {
        let m = parse(src);
        let resolved = Resolver::new().resolve(m).expect("resolve");
        let layouts = resolved.class_layouts.clone();
        let names: HashMap<String, ClassId> =
            layouts.iter().map(|(c, l)| (l.name.clone(), *c)).collect();
        (layouts, names)
    }

    // ── WAVE-2 LANE-0: lookup_dunder scaffold ────────────────────────────

    #[test]
    fn lookup_dunder_returns_signature() {
        let src = "\
final class Adder:
    n: i64
    fn __init__(self, n: i64) -> None:
        self.n = n
    fn __add__(self, other: i64) -> i64:
        return self.n + other

fn main() -> i32:
    return 0
";
        let (layouts, names) = resolve_layouts(src);
        let cid = names["Adder"];
        let sig = lookup_dunder(&layouts, cid, "__add__").expect("Adder defines __add__");
        // `self` is stripped; one declared operand of type i64, returns i64.
        assert_eq!(sig.params.len(), 1, "__add__ has one operand after self");
        assert!(ty_eq(&sig.params[0], &Ty::Primitive(PrimTy::I64)));
        assert!(ty_eq(&sig.ret, &Ty::Primitive(PrimTy::I64)));
        // Undefined dunder yields None.
        assert!(lookup_dunder(&layouts, cid, "__str__").is_none());
    }

    #[test]
    fn lookup_dunder_resolves_inherited() {
        let src = "\
open class Base:
    open fn __eq__(self, other: i64) -> bool:
        return False

final class Derived(Base):
    fn extra(self) -> i64:
        return 1

fn main() -> i32:
    return 0
";
        let (layouts, names) = resolve_layouts(src);
        let derived = names["Derived"];
        let sig = lookup_dunder(&layouts, derived, "__eq__")
            .expect("Derived inherits __eq__ from Base");
        assert_eq!(sig.params.len(), 1);
        assert!(ty_eq(&sig.ret, &Ty::Primitive(PrimTy::Bool)));
    }

    // ── WAVE-2 LANE-C: __getitem__ / __setitem__ index dunders ───────────

    /// A class with `__getitem__`/`__setitem__` type-checks subscript read and
    /// write, with the result type of a read taken from the dunder's return.
    #[test]
    fn dunder_index_read_write_typechecks() {
        let src = "\
final class Vec:
    data: List[i64]
    fn __init__(self, seed: i64) -> None:
        self.data = [seed]
    fn __getitem__(self, i: i64) -> i64:
        return self.data[i]
    fn __setitem__(self, i: i64, v: i64) -> None:
        self.data[i] = v

fn main() -> i32:
    v: Vec = Vec(10)
    x: i64 = v[0]
    v[0] = 99
    return 0
";
        check_src(src).expect("class subscript read+write must type-check");
    }

    /// The key expression is checked against the dunder's declared key
    /// parameter — a `str` key on an `i64`-keyed `__getitem__` is a Type error.
    #[test]
    fn dunder_index_wrong_key_rejected() {
        let src = "\
final class Vec:
    data: List[i64]
    fn __init__(self, seed: i64) -> None:
        self.data = [seed]
    fn __getitem__(self, i: i64) -> i64:
        return self.data[i]

fn main() -> i32:
    v: Vec = Vec(10)
    bad: i64 = v[\"x\"]
    return 0
";
        let err = check_src(src).expect_err("str key on i64 __getitem__ must fail");
        assert!(matches!(err, CompileError::Type { .. }),
            "expected Type error, got {err:?}");
    }

    /// The stored value is checked against `__setitem__`'s value parameter.
    #[test]
    fn dunder_index_wrong_value_rejected() {
        let src = "\
final class Vec:
    data: List[i64]
    fn __init__(self, seed: i64) -> None:
        self.data = [seed]
    fn __setitem__(self, i: i64, v: i64) -> None:
        self.data[i] = v

fn main() -> i32:
    v: Vec = Vec(10)
    v[0] = \"nope\"
    return 0
";
        let err = check_src(src).expect_err("str value into i64 __setitem__ must fail");
        assert!(matches!(err, CompileError::Type { .. }),
            "expected Type error, got {err:?}");
    }

    /// A class with no `__getitem__` is still genuinely not subscriptable —
    /// the fall-through to `index_type` produces a Type error.
    #[test]
    fn class_without_getitem_not_indexable() {
        let src = "\
final class Plain:
    n: i64
    fn __init__(self, n: i64) -> None:
        self.n = n

fn main() -> i32:
    p: Plain = Plain(1)
    x: i64 = p[0]
    return 0
";
        let err = check_src(src).expect_err("class without __getitem__ is not indexable");
        assert!(matches!(err, CompileError::Type { .. }),
            "expected Type error, got {err:?}");
    }

    /// A generic container's key/return types specialise to the receiver's
    /// type argument (`Box[str]` keys/returns `str`), so the right key type is
    /// enforced per instantiation.
    #[test]
    fn dunder_index_generic_class_specialises() {
        let src = "\
final class Box[T]:
    items: List[T]
    fn __init__(self, seed: T) -> None:
        self.items = [seed]
    fn __getitem__(self, i: i64) -> T:
        return self.items[i]
    fn __setitem__(self, i: i64, v: T) -> None:
        self.items[i] = v

fn main() -> i32:
    b: Box[str] = Box(\"hi\")
    s: str = b[0]
    b[0] = \"bye\"
    return 0
";
        check_src(src).expect("generic class subscript must specialise T to str");
    }

    /// The same generic container rejects a value of the wrong specialised
    /// type — storing an `i64` into a `Box[str]` is a Type error.
    #[test]
    fn dunder_index_generic_wrong_value_rejected() {
        let src = "\
final class Box[T]:
    items: List[T]
    fn __init__(self, seed: T) -> None:
        self.items = [seed]
    fn __setitem__(self, i: i64, v: T) -> None:
        self.items[i] = v

fn main() -> i32:
    b: Box[str] = Box(\"hi\")
    b[0] = 5
    return 0
";
        let err = check_src(src).expect_err("i64 into Box[str] __setitem__ must fail");
        assert!(matches!(err, CompileError::Type { .. }),
            "expected Type error, got {err:?}");
    }

    // ── WAVE-2 LANE-D: class for-loop iterator-protocol type checking ────

    /// `for x: T in obj:` over a user class with a valid `__iter__`/`__next__`
    /// protocol type-checks, and the loop var binds at `__next__`'s value
    /// type (the unwrapped nullable).
    #[test]
    fn class_for_loop_protocol_typechecks() {
        let src = "\
final class It:
    n: i64
    fn __init__(self) -> None:
        self.n = 0
    fn __iter__(self) -> It:
        return self
    fn __next__(self) -> i64?:
        if self.n >= 3:
            return none
        v: i64 = self.n
        self.n = self.n + 1
        return v

fn main() -> i32:
    total: i64 = 0
    for x: i64 in It():
        total = total + x
    return 0
";
        check_src(src).expect("valid iterator-protocol for-loop must type-check");
    }

    /// A user class without `__iter__` is not iterable — `for` over it is a
    /// compile error, not a silent run-once.
    #[test]
    fn class_for_loop_without_iter_rejected() {
        let src = "\
final class NotIterable:
    x: i64
    fn __init__(self, x: i64) -> None:
        self.x = x

fn main() -> i32:
    for v: i64 in NotIterable(3):
        return v
    return 0
";
        let r = check_src(src);
        assert!(r.is_err(), "class lacking __iter__ must be rejected as non-iterable");
    }

    /// The loop-var annotation must match `__next__`'s value type. Iterating
    /// an `i64?`-yielding iterator with `for v: str` is a compile error.
    #[test]
    fn class_for_loop_var_type_mismatch_rejected() {
        let src = "\
final class It:
    n: i64
    fn __init__(self) -> None:
        self.n = 0
    fn __iter__(self) -> It:
        return self
    fn __next__(self) -> i64?:
        if self.n >= 2:
            return none
        v: i64 = self.n
        self.n = self.n + 1
        return v

fn main() -> i32:
    for v: str in It():
        return 0
    return 0
";
        let r = check_src(src);
        assert!(r.is_err(), "loop var type must match __next__'s element type");
    }

    #[test]
    fn test_synth_int_literal() {
        let _ = check_src("fn main() -> i32:\n    x: i32 = 0\n    return x\n").unwrap();
    }

    #[test]
    fn test_check_int_literal_against_i64() {
        let _ = check_src("fn main() -> i32:\n    x: i64 = 0\n    return 0\n").unwrap();
    }

    // ── Wave-2 Lane F: integer-literal range checking ───────────────────

    #[test]
    fn int_literal_at_i64_max_is_accepted() {
        // i64::MAX exactly — must compile.
        check_src("fn main() -> i32:\n    x: i64 = 9223372036854775807\n    return 0\n")
            .expect("i64::MAX literal must be accepted");
    }

    #[test]
    fn int_literal_above_i64_max_is_rejected() {
        // i64::MAX + 1 — used to truncate silently to i64::MIN.
        let err = check_src("fn main() -> i32:\n    x: i64 = 9223372036854775808\n    return 0\n")
            .expect_err("i64::MAX + 1 literal must be a compile error");
        let msg = format!("{err}");
        assert!(msg.contains("E2073"), "want E2073, got: {msg}");
        assert!(msg.contains("out of range"), "want 'out of range', got: {msg}");
        assert!(msg.contains("BigInt"), "want BigInt mention, got: {msg}");
    }

    #[test]
    fn negated_i64_min_literal_is_accepted() {
        // -9223372036854775808 == i64::MIN. The bare magnitude is out of range
        // but the negated value is exactly representable.
        check_src("fn main() -> i32:\n    x: i64 = -9223372036854775808\n    return 0\n")
            .expect("i64::MIN literal must be accepted");
    }

    #[test]
    fn out_of_range_suffixed_literal_is_rejected() {
        // 256u8 overflows u8 — caught here instead of wrapping at lowering.
        let err = check_src("fn main() -> i32:\n    x: u8 = 256u8\n    return 0\n")
            .expect_err("256u8 must be a compile error");
        assert!(format!("{err}").contains("E2073"), "want E2073: {err}");
    }

    #[test]
    fn helper_range_check_boundaries() {
        let sp = Span::DUMMY;
        // i64 boundaries.
        assert!(check_int_literal_in_range(i64::MAX as i128, PrimTy::I64, sp).is_ok());
        assert!(check_int_literal_in_range(i64::MIN as i128, PrimTy::I64, sp).is_ok());
        assert!(check_int_literal_in_range(i64::MAX as i128 + 1, PrimTy::I64, sp).is_err());
        assert!(check_int_literal_in_range(i64::MIN as i128 - 1, PrimTy::I64, sp).is_err());
        // u64 upper bound.
        assert!(check_int_literal_in_range(u64::MAX as i128, PrimTy::U64, sp).is_ok());
        assert!(check_int_literal_in_range(u64::MAX as i128 + 1, PrimTy::U64, sp).is_err());
        // Negative into an unsigned type is rejected.
        assert!(check_int_literal_in_range(-1, PrimTy::U32, sp).is_err());
        // BigInt is unbounded — anything passes.
        assert!(check_int_literal_in_range(i128::MAX, PrimTy::BigInt, sp).is_ok());
    }

    #[test]
    fn test_binary_op_same_type() {
        let _ = check_src("fn main() -> i32:\n    x: i32 = 1 + 2\n    return x\n").unwrap();
    }

    #[test]
    fn test_binary_op_mixed_widens_to_i64() {
        // Lane A: `i32 + i64` now widens to i64 (was a hard error). Storing
        // the i64 result back into an i32 is still rejected (narrowing).
        let r = check_src("fn main() -> i32:\n    a: i32 = 1\n    b: i64 = 2\n    c: i32 = a + b\n    return 0\n");
        assert!(r.is_err(), "i64 result cannot be narrowed to i32 implicitly");
        // ...but binding it to an i64 target is fine.
        check_src("fn main() -> i32:\n    a: i32 = 1\n    b: i64 = 2\n    c: i64 = a + b\n    return 0\n")
            .expect("i32 + i64 must widen to i64");
    }

    #[test]
    fn test_subtype_reflexive() {
        let t = Ty::Primitive(PrimTy::I32);
        let classes = std::collections::HashMap::new();
        let protos = std::collections::HashMap::new();
        let cx = TypeContext { classes: &classes, protocols: &protos };
        assert!(is_subtype(&t, &t, &cx));
    }

    #[test]
    fn test_subtype_never() {
        let classes = std::collections::HashMap::new();
        let protos = std::collections::HashMap::new();
        let cx = TypeContext { classes: &classes, protocols: &protos };
        assert!(is_subtype(&Ty::Never, &Ty::Primitive(PrimTy::I32), &cx));
    }

    #[test]
    fn test_subtype_nullable() {
        let classes = std::collections::HashMap::new();
        let protos = std::collections::HashMap::new();
        let cx = TypeContext { classes: &classes, protocols: &protos };
        let i = Ty::Primitive(PrimTy::I32);
        let ni = Ty::Nullable(Box::new(Ty::Primitive(PrimTy::I32)));
        assert!(is_subtype(&i, &ni, &cx));
    }

    #[test]
    fn test_class_subtype() {
        let src = "open class B:\n    x: i32\nfinal class C(B):\n    y: i32\nfn main() -> i32:\n    return 0\n";
        check_src(src).unwrap();
    }

    #[test]
    fn test_protocol_satisfaction() {
        // Minimal — actual protocol test would need parser support for `protocol`.
        // Just verify Comparable etc. don't break.
        let _ = check_src("fn main() -> i32:\n    return 0\n").unwrap();
    }

    #[test]
    fn test_nullable_narrowing() {
        // x: i32? — after `if x is not none`, treat x as i32.
        let src = "fn main() -> i32:\n    x: i32? = none\n    if x is not none:\n        y: i32 = x + 1\n        return y\n    return 0\n";
        check_src(src).unwrap();
    }

    #[test]
    fn test_definite_assignment() {
        let r = check_src("fn main() -> i32:\n    return zzz\n");
        assert!(r.is_err());
    }

    #[test]
    fn test_forbidden_any() {
        let r = check_src("fn main() -> i32:\n    x: Any = 0\n    return 0\n");
        assert!(r.is_err());
    }

    #[test]
    fn test_numeric_widening_coercion() {
        // Lane A (supersedes the old `test_no_implicit_coercion`): the
        // signed-int ladder and int->float promotions now widen implicitly.
        check_src("fn main() -> i32:\n    a: i32 = 1\n    b: i64 = 2\n    c: i64 = a + b\n    return 0\n")
            .expect("i32 + i64 -> i64");
        check_src("fn main() -> i32:\n    a: i64 = 1\n    b: f64 = 2.0\n    c: f64 = a + b\n    return 0\n")
            .expect("i64 + f64 -> f64");
        // Mixed signedness still has no lossless cast, so it stays an error.
        let r = check_src("fn main() -> i32:\n    a: u32 = 1u32\n    b: i32 = 2i32\n    c: i64 = a + b\n    return 0\n");
        assert!(r.is_err(), "u32 + i32 has no lossless widening; must stay an error");
    }

    #[test]
    fn test_true_division_is_float() {
        // `/` yields f64 even for integer operands.
        check_src("fn main() -> i32:\n    a: i64 = 7\n    b: i64 = 2\n    q: f64 = a / b\n    return 0\n")
            .expect("integer `/` must yield f64");
        // ...so binding it to an integer is a type error.
        let r = check_src("fn main() -> i32:\n    a: i64 = 7\n    b: i64 = 2\n    q: i64 = a / b\n    return 0\n");
        assert!(r.is_err(), "true division result is f64, not i64");
        // `//` keeps the integer type.
        check_src("fn main() -> i32:\n    a: i64 = 7\n    b: i64 = 2\n    q: i64 = a // b\n    return 0\n")
            .expect("`//` must keep the integer type");
    }

    #[test]
    fn test_bare_literal_defaults_to_i64() {
        // A bare literal binds to i64 by default; assigning to i32 needs the
        // literal to fit (it adopts the annotation), but a value that only
        // fits i64 must NOT be accepted as i32 via the default.
        check_src("fn main() -> i32:\n    x: i64 = 1\n    return 0\n").expect("bare 1 ok as i64");
        check_src("fn main() -> i32:\n    x: i32 = 1\n    return 0\n").expect("bare 1 adopts i32");
    }

    #[test]
    fn test_numeric_common_ty_table() {
        use PrimTy::*;
        assert_eq!(numeric_common_ty(I32, I64), Some(I64));
        assert_eq!(numeric_common_ty(I64, I32), Some(I64));
        assert_eq!(numeric_common_ty(I32, F64), Some(F64));
        assert_eq!(numeric_common_ty(I64, F64), Some(F64));
        assert_eq!(numeric_common_ty(F32, I32), Some(F64));
        assert_eq!(numeric_common_ty(F64, F64), Some(F64));
        assert_eq!(numeric_common_ty(I8, I32), Some(I32));
        // No lossless implicit widening for these:
        assert_eq!(numeric_common_ty(U32, I32), None);
        assert_eq!(numeric_common_ty(U64, I64), None);
        assert_eq!(numeric_common_ty(F32, F64), Some(F64));
    }

    // ── Wave-1 Lane D: silent-footgun regression tests ──────────────────

    fn err_code(r: Result<TypedModule, CompileError>) -> String {
        match r {
            Ok(_) => "<no error>".into(),
            Err(CompileError::Type { code, .. }) => code.to_string(),
            Err(CompileError::Resolve { code, .. }) => code.to_string(),
            Err(CompileError::Semantic { code, .. }) => code.to_string(),
            Err(other) => format!("{other:?}"),
        }
    }

    #[test]
    fn dict_non_str_key_annotation_rejected() {
        // `Dict[i64, V]` previously compiled then SEGFAULTed at subscript.
        let r = check_src("fn main() -> i32:\n    d: Dict[i64, i64] = {}\n    return 0\n");
        assert_eq!(err_code(r), codes::TYPE_DICT_NON_STR_KEY);
    }

    #[test]
    fn dict_str_key_still_ok() {
        check_src("fn main() -> i32:\n    d: Dict[str, i64] = {}\n    return 0\n").unwrap();
    }

    #[test]
    fn dict_tuple_key_rejected() {
        let r = check_src(
            "fn main() -> i32:\n    d: Dict[Tuple[i64, i64], str] = {}\n    return 0\n",
        );
        assert_eq!(err_code(r), codes::TYPE_DICT_NON_STR_KEY);
    }

    #[test]
    fn dict_nested_bad_key_in_value_rejected() {
        // `Dict[str, Dict[i64, str]]` — the bad key is nested in the value.
        let r = check_src(
            "fn main() -> i32:\n    d: Dict[str, Dict[i64, str]] = {}\n    return 0\n",
        );
        assert_eq!(err_code(r), codes::TYPE_DICT_NON_STR_KEY);
    }

    #[test]
    fn dict_literal_int_key_rejected() {
        // Synthesised dict literal with an int key (no annotation forcing it).
        let r = check_src(
            "fn sink(d: Dict[str, str]) -> None:\n    pass\n\
             fn main() -> i32:\n    print(str({1i64: \"a\"}))\n    return 0\n",
        );
        assert_eq!(err_code(r), codes::TYPE_DICT_NON_STR_KEY);
    }

    #[test]
    fn unknown_decorator_rejected() {
        let r = check_src(
            "@lru_cache\nfn fib(n: i32) -> i32:\n    return n\n\
             fn main() -> i32:\n    return 0\n",
        );
        assert_eq!(err_code(r), codes::TYPE_UNKNOWN_DECORATOR);
    }

    #[test]
    fn unknown_method_decorator_rejected() {
        let r = check_src(
            "final class C:\n    x: i32 = 0\n    @staticmethod\n    fn f(self) -> i32:\n        return 0\n\
             fn main() -> i32:\n    return 0\n",
        );
        assert_eq!(err_code(r), codes::TYPE_UNKNOWN_DECORATOR);
    }

    #[test]
    fn with_non_file_rejected() {
        let r = check_src(
            "final class Lock:\n    held: bool = false\n\
             fn main() -> i32:\n    with Lock() as l: Lock:\n        return 0\n    return 0\n",
        );
        assert_eq!(err_code(r), codes::TYPE_UNSUPPORTED_CONTEXT_MANAGER);
    }

    #[test]
    fn raise_from_non_exception_rejected() {
        let r = check_src(
            "fn main() -> i32:\n    raise ValueError(\"x\") from 42\n    return 0\n",
        );
        assert_eq!(err_code(r), codes::TYPE_NOT_AN_EXCEPTION);
    }

    #[test]
    fn raise_from_exception_ok() {
        // `raise X from cause` where cause is a caught exception type-checks.
        check_src(
            "fn main() -> i32:\n    try:\n        raise ValueError(\"x\")\n    \
             except ValueError as cause:\n        raise KeyError(\"y\") from cause\n    return 0\n",
        )
        .unwrap();
    }

    #[test]
    fn except_tuple_binding_has_message_field() {
        // `except (A, B) as e:` — `e.message` must type-check (the bind type is
        // the first listed exception class, not the tuple itself).
        check_src(
            "fn main() -> i32:\n    try:\n        raise ValueError(\"x\")\n    \
             except (ValueError, KeyError) as e:\n        print(e.message)\n    return 0\n",
        )
        .unwrap();
    }

    #[test]
    fn try_else_type_checks() {
        check_src(
            "fn main() -> i32:\n    try:\n        x: i32 = 1\n    \
             except ValueError as e:\n        print(\"caught\")\n    \
             else:\n        print(\"ok\")\n    return 0\n",
        )
        .unwrap();
    }
}
