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
    self, Arg, BinOp, Block, ClassDecl, Expr, FuncDecl, Literal, Lvalue, Span, Stmt, TopDecl, UnaryOp,
};
use crate::error::{codes, CompileError, ErrorCode};
use crate::resolver::{FunctionSig, ResolvedModule, SymbolId, SymbolKind};
use crate::types::{
    is_subtype, is_subtype_trivial, ty_eq, BoundKind, ClassId, ClassLayout, PrimTy, ProtoId,
    ProtocolInfo, Ty, TypeContext, TypeCtor, TypeVarId,
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
        // Compute expected return type.
        let ret_ty = if matches!(f.return_ty, ast::Type::Named { ref name, .. } if name == "None") {
            Ty::Primitive(PrimTy::Unit)
        } else {
            // Use ast_type_to_ty side table.
            let key = ast_type_span(&f.return_ty);
            r.ast_type_to_ty.get(&key).cloned().unwrap_or_else(|| Ty::Never)
        };
        let _ = recv;
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
                let _ = op;
                if !ty_eq(&lhs_ty, &rhs) {
                    return Err(type_err(*span, codes::TYPE_BINOP_MISMATCH,
                        format!("aug-assign type mismatch: {} vs {}", lhs_ty.display(), rhs.display())));
                }
            }
            Stmt::Return { value, span } => {
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
                let _iter_ty = self.check_or_synth(iter, None, env, ctx, r)?;
                let key = ast_type_span(var_ty);
                let vty = r.ast_type_to_ty.get(&key).cloned().unwrap_or(Ty::Never);
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
            Stmt::With { expr, body, .. } => {
                let _ = self.check_or_synth(expr, None, env, ctx, r)?;
                self.check_block(body, ret, env, ctx, r)?;
            }
            Stmt::Raise { exc, span, .. } => {
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
                    }
                }
                let _ = self.check_or_synth(exc, None, env, ctx, r)?;
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
            Stmt::Del { .. } | Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::Pass { .. } => {}
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

    fn check_expr(&mut self, e: &Expr, expected: &Ty, env: &Env, ctx: &Ctx, r: &ResolvedModule)
        -> Result<Ty, CompileError>
    {
        // Special-case literals: an int literal with no suffix takes on expected width.
        if let Expr::Literal { lit, span } = e {
            let lit_ty = match lit {
                Literal::Int { suffix, .. } => {
                    if let Some(s) = suffix {
                        Ty::Primitive(int_suffix_to_prim(*s))
                    } else if let Ty::Primitive(p) = expected {
                        if p.is_numeric() { Ty::Primitive(*p) } else { Ty::Primitive(PrimTy::I32) }
                    } else if let Ty::Nullable(inner) = expected {
                        if let Ty::Primitive(p) = inner.as_ref() {
                            if p.is_numeric() { Ty::Primitive(*p) } else { Ty::Primitive(PrimTy::I32) }
                        } else { Ty::Primitive(PrimTy::I32) }
                    } else {
                        Ty::Primitive(PrimTy::I32)
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
            self.expr_types.insert((span.start, span.end), lit_ty.clone());
            if !is_subtype(&lit_ty, expected, &ctx.ty_ctx()) {
                return Err(type_err(*span, codes::TYPE_MISMATCH,
                    format!("literal of type {} doesn't match expected {}",
                            lit_ty.display(), expected.display())));
            }
            return Ok(lit_ty);
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
                let ty = Ty::Generic { base: TypeCtor::Dict, args: vec![kk, vv] };
                self.expr_types.insert((span.start, span.end), ty.clone());
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
            Expr::Literal { lit, .. } => Ok(match lit {
                Literal::Int { suffix, .. } => {
                    if let Some(s) = suffix { Ty::Primitive(int_suffix_to_prim(*s)) }
                    else { Ty::Primitive(PrimTy::I32) }
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
            }),
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
            Expr::Set { elems, .. } => {
                let mut elem_ty: Option<Ty> = None;
                for el in elems {
                    let t = self.synth_expr(el, env, ctx, r)?;
                    elem_ty = Some(t);
                }
                Ok(Ty::Generic { base: TypeCtor::Set, args: vec![elem_ty.unwrap_or(Ty::Never)] })
            }
            Expr::Dict { entries, .. } => {
                let mut kty: Option<Ty> = None;
                let mut vty: Option<Ty> = None;
                for (k, v) in entries {
                    kty = Some(self.synth_expr(k, env, ctx, r)?);
                    vty = Some(self.synth_expr(v, env, ctx, r)?);
                }
                Ok(Ty::Generic { base: TypeCtor::Dict, args: vec![
                    kty.unwrap_or(Ty::Never), vty.unwrap_or(Ty::Never)] })
            }
            Expr::Unary { op, operand, span } => {
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
                for i in indices { let _ = self.synth_expr(i, env, ctx, r)?; }
                self.index_type(&obj_ty, *span)
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
        }
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
                let _lt = self.synth_expr(lhs, env, ctx, r)?;
                let _rt = self.synth_expr(rhs, env, ctx, r)?;
                Ok(Ty::Primitive(PrimTy::Bool))
            }
            // Arithmetic / bitwise / shift
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::FloorDiv
            | BinOp::Rem | BinOp::Pow | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor
            | BinOp::Shl | BinOp::Shr => {
                let lt = self.synth_expr(lhs, env, ctx, r)?;
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
                let rt = self.check_or_synth(rhs, Some(&lt), env, ctx, r)?;
                if matches!(rt, Ty::Var(_)) {
                    return Ok(rt);
                }
                if !ty_eq(&lt, &rt) {
                    return Err(type_err(span, codes::TYPE_BINOP_MISMATCH,
                        format!("operand type mismatch: cannot apply `{:?}` to {} and {}",
                                op, lt.display(), rt.display())));
                }
                if let Ty::Primitive(p) = &lt {
                    if !p.is_numeric() {
                        return Err(type_err(span, codes::TYPE_BINOP_MISMATCH,
                            format!("`{:?}` requires numeric operands, got {}", op, lt.display())));
                    }
                    return Ok(Ty::Primitive(*p));
                }
                Err(type_err(span, codes::TYPE_BINOP_MISMATCH,
                    format!("`{:?}` not defined for {}", op, lt.display())))
            }
        }
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
            (Ty::Generic { base: TypeCtor::List, args: _ }, "len") => {
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
            // str methods — stdlib: wordcount.spy
            (Ty::Primitive(PrimTy::Str), "slice") => {
                for a in args { let _ = self.check_or_synth(&a.value, Some(&Ty::Primitive(PrimTy::I64)), env, ctx, r)?; }
                return Ok(Ty::Primitive(PrimTy::Str));
            }
            (Ty::Primitive(PrimTy::Str), "char_at") => {
                for a in args { let _ = self.check_or_synth(&a.value, Some(&Ty::Primitive(PrimTy::I64)), env, ctx, r)?; }
                return Ok(Ty::Primitive(PrimTy::Char));
            }
            (Ty::Primitive(PrimTy::Str), "len") => {
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
                        if args.len() != m.params.len() {
                            return Err(type_err(span, codes::TYPE_ARITY,
                                format!("method `{}` expects {} args, got {}", method, m.params.len(), args.len())));
                        }
                        for (a, pt) in args.iter().zip(m.params.iter()) {
                            let expected = subst_ty(pt, &recv_subst);
                            let _ = self.check_expr(&a.value, &expected, env, ctx, r)?;
                        }
                        return Ok(subst_ty(&m.ret, &recv_subst));
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
}

// ─────────────────────────────────────────────────────────────────────────
//  Helpers
// ─────────────────────────────────────────────────────────────────────────

fn type_err(span: Span, code: ErrorCode, message: String) -> CompileError {
    CompileError::Type {
        file: String::new(), line: span.line, col: span.col, code, message,
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

fn expr_span(e: &Expr) -> Span {
    match e {
        Expr::Literal { span, .. } | Expr::Ident { span, .. } | Expr::Tuple { span, .. }
        | Expr::List { span, .. } | Expr::Dict { span, .. } | Expr::Set { span, .. }
        | Expr::Unary { span, .. } | Expr::Binary { span, .. } | Expr::Call { span, .. }
        | Expr::MethodCall { span, .. } | Expr::Attr { span, .. } | Expr::Index { span, .. }
        | Expr::NullCoalesce { span, .. } | Expr::Ternary { span, .. }
        | Expr::Lambda { span, .. } | Expr::Cast { span, .. } => *span,
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

    #[test]
    fn test_synth_int_literal() {
        let _ = check_src("fn main() -> i32:\n    x: i32 = 0\n    return x\n").unwrap();
    }

    #[test]
    fn test_check_int_literal_against_i64() {
        let _ = check_src("fn main() -> i32:\n    x: i64 = 0\n    return 0\n").unwrap();
    }

    #[test]
    fn test_binary_op_same_type() {
        let _ = check_src("fn main() -> i32:\n    x: i32 = 1 + 2\n    return x\n").unwrap();
    }

    #[test]
    fn test_binary_op_mixed_fails() {
        let r = check_src("fn main() -> i32:\n    a: i32 = 1\n    b: i64 = 2\n    c: i32 = a + b\n    return 0\n");
        assert!(r.is_err());
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
    fn test_no_implicit_coercion() {
        let r = check_src("fn main() -> i32:\n    a: i32 = 1\n    b: i64 = 2\n    c: i64 = a + b\n    return 0\n");
        assert!(r.is_err());
    }
}
