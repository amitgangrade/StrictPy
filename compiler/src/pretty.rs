//! AST pretty-printer: walks a [`Module`] and emits canonical StrictPy
//! source. Implements the canonical style described in spec §3, §4, and
//! illustrated by the §21 examples and the seven programs under
//! `examples/*.spy`.
//!
//! ## Canonical conventions
//!
//! - 4-space indentation per block level.
//! - One blank line between top-level declarations; no blank line between
//!   class members.
//! - `fn` (not `def`); booleans are `true` / `false`; null is `none`.
//! - Single space after commas and colons; spaces around binary operators;
//!   no space around unary operators.
//! - Nullable types use postfix `T?`.
//! - Class modifiers (`final` / `open` / `sealed`) are always emitted.
//! - Decorators appear on their own line before the declaration.
//!
//! ## Limitations
//!
//! - The AST does not preserve f-string structure: by the time the parser
//!   builds the AST it has lowered f-strings to ordinary expressions (string
//!   concatenation or analogous). The pretty-printer therefore cannot
//!   round-trip an `f"..."` literal back to `f"..."` source. Triple-quoted
//!   docstrings are re-emitted with `"""..."""` when the content contains
//!   literal newlines.

use crate::ast::*;
use crate::lexer::{FloatSuffix, IntSuffix};

// ─────────────────────────────────────────────────────────────────────────
//  Public API
// ─────────────────────────────────────────────────────────────────────────

/// Print the module as canonical StrictPy source. Output ends with a
/// trailing newline.
pub fn print_module(module: &Module) -> String {
    let mut p = PrettyPrinter::new();
    p.print_module(module);
    p.finish()
}

// ─────────────────────────────────────────────────────────────────────────
//  PrettyPrinter
// ─────────────────────────────────────────────────────────────────────────

pub struct PrettyPrinter {
    buf: String,
    indent: u32,
    /// `true` if the current line is "fresh" (i.e. nothing written since the
    /// last newline, so we still need to emit the leading indentation).
    line_start: bool,
}

impl PrettyPrinter {
    pub fn new() -> Self {
        Self { buf: String::new(), indent: 0, line_start: true }
    }

    pub fn finish(mut self) -> String {
        // Ensure exactly one trailing newline.
        while self.buf.ends_with("\n\n") {
            self.buf.pop();
        }
        if !self.buf.ends_with('\n') {
            self.buf.push('\n');
        }
        self.buf
    }

    fn indent_inc(&mut self) { self.indent += 1; }
    fn indent_dec(&mut self) {
        debug_assert!(self.indent > 0, "indent underflow");
        self.indent -= 1;
    }

    fn write(&mut self, s: &str) {
        if self.line_start && !s.is_empty() {
            for _ in 0..self.indent {
                self.buf.push_str("    ");
            }
            self.line_start = false;
        }
        self.buf.push_str(s);
    }

    fn writeln(&mut self, s: &str) {
        self.write(s);
        self.buf.push('\n');
        self.line_start = true;
    }

    fn newline(&mut self) {
        self.buf.push('\n');
        self.line_start = true;
    }

    fn blank_line(&mut self) {
        // Emit a blank line, collapsing runs.
        if !self.buf.ends_with("\n\n") {
            if !self.buf.ends_with('\n') {
                self.buf.push('\n');
            }
            self.buf.push('\n');
            self.line_start = true;
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Module
    // ─────────────────────────────────────────────────────────────────────

    pub fn print_module(&mut self, module: &Module) {
        for imp in &module.imports {
            self.print_import(imp);
        }
        if !module.imports.is_empty() && !module.decls.is_empty() {
            self.blank_line();
        }
        for (i, decl) in module.decls.iter().enumerate() {
            if i > 0 {
                self.blank_line();
            }
            self.print_top_decl(decl);
        }
    }

    fn print_import(&mut self, imp: &Import) {
        let dotted = imp.path.join(".");
        if imp.items.is_empty() {
            // `import dotted [as alias]`
            self.write("import ");
            self.write(&dotted);
            if let Some(alias) = &imp.alias {
                self.write(" as ");
                self.write(alias);
            }
            self.newline();
        } else {
            self.write("from ");
            self.write(&dotted);
            self.write(" import ");
            for (i, item) in imp.items.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(&item.name);
                if let Some(alias) = &item.alias {
                    self.write(" as ");
                    self.write(alias);
                }
            }
            self.newline();
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Top-level declarations
    // ─────────────────────────────────────────────────────────────────────

    fn print_top_decl(&mut self, decl: &TopDecl) {
        match decl {
            TopDecl::Func(f) => self.print_func(f, false),
            TopDecl::Class(c) => self.print_class(c),
            TopDecl::Protocol(p) => self.print_protocol(p),
            TopDecl::Const(c) => self.print_const(c),
            TopDecl::TypeAlias(a) => self.print_type_alias(a),
        }
    }

    fn print_const(&mut self, c: &ConstDecl) {
        self.write("final ");
        self.write(&c.name);
        self.write(": ");
        self.print_type(&c.ty);
        self.write(" = ");
        self.print_expr(&c.value);
        self.newline();
    }

    fn print_type_alias(&mut self, a: &TypeAliasDecl) {
        self.write("type ");
        self.write(&a.name);
        if !a.generics.is_empty() {
            self.print_generic_params(&a.generics);
        }
        self.write(" = ");
        self.print_type(&a.ty);
        self.newline();
    }

    fn print_generic_params(&mut self, params: &[GenericParam]) {
        self.write("[");
        for (i, gp) in params.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(&gp.name);
            if !gp.bounds.is_empty() {
                self.write(": ");
                for (j, b) in gp.bounds.iter().enumerate() {
                    if j > 0 {
                        self.write(" + ");
                    }
                    self.print_type(b);
                }
            }
        }
        self.write("]");
    }

    fn print_func(&mut self, f: &FuncDecl, is_method: bool) {
        for dec in &f.decorators {
            self.print_decorator(dec);
        }
        let _ = is_method;
        self.write("fn ");
        self.write(&f.name);
        if !f.generics.is_empty() {
            self.print_generic_params(&f.generics);
        }
        self.write("(");
        for (i, p) in f.params.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.print_param(p);
        }
        self.write(") -> ");
        self.print_type(&f.return_ty);
        self.write(":");
        self.print_block(&f.body);
    }

    fn print_param(&mut self, p: &Param) {
        // Special-case `self`: convention is `self` with no explicit type.
        if p.name == "self" {
            // If the type is the default/inferred (Named "Self" or similar),
            // still emit the name only. We detect "Self" or a Named type
            // matching the enclosing class — but we don't have that context.
            // Convention from examples: write `self` with no annotation.
            //
            // However, params come in directly; we can detect a Named "Self"
            // or "_inferred" pattern. Simplest: if name is "self" and the
            // type is Named with name "Self" or Infer, omit the type.
            match &p.ty {
                Type::Named { name, args, .. } if (name == "Self" || name.is_empty()) && args.is_empty() => {
                    self.write("self");
                    return;
                }
                Type::Infer { .. } => {
                    self.write("self");
                    return;
                }
                _ => {}
            }
        }
        self.write(&p.name);
        self.write(": ");
        self.print_type(&p.ty);
        if let Some(default) = &p.default {
            self.write(" = ");
            self.print_expr(default);
        }
    }

    fn print_decorator(&mut self, dec: &Decorator) {
        self.write("@");
        self.write(&dec.path.join("."));
        if !dec.args.is_empty() {
            self.write("(");
            for (i, a) in dec.args.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.print_arg(a);
            }
            self.write(")");
        }
        self.newline();
    }

    fn print_class(&mut self, c: &ClassDecl) {
        match c.modifier {
            ClassModifier::Final => self.write("final "),
            ClassModifier::Open => self.write("open "),
            ClassModifier::Sealed => self.write("sealed "),
        }
        self.write("class ");
        self.write(&c.name);
        if !c.generics.is_empty() {
            self.print_generic_params(&c.generics);
        }
        if !c.bases.is_empty() {
            self.write("(");
            for (i, b) in c.bases.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.print_type(b);
            }
            self.write(")");
        }
        self.write(":");
        self.newline();
        self.indent_inc();
        let mut any = false;
        for field in &c.fields {
            self.print_field(field);
            any = true;
        }
        if let Some(init) = &c.init {
            self.print_func(init, true);
            any = true;
        }
        for m in &c.methods {
            self.print_func(m, true);
            any = true;
        }
        if !any {
            // Empty body: emit `pass`.
            self.writeln("pass");
        }
        self.indent_dec();
    }

    fn print_field(&mut self, f: &FieldDecl) {
        self.write(&f.name);
        self.write(": ");
        self.print_type(&f.ty);
        if let Some(default) = &f.default {
            self.write(" = ");
            self.print_expr(default);
        }
        self.newline();
    }

    fn print_protocol(&mut self, p: &ProtocolDecl) {
        self.write("protocol ");
        self.write(&p.name);
        if !p.generics.is_empty() {
            self.print_generic_params(&p.generics);
        }
        self.write(":");
        self.newline();
        self.indent_inc();
        if p.methods.is_empty() {
            self.writeln("pass");
        } else {
            for m in &p.methods {
                self.write("fn ");
                self.write(&m.name);
                self.write("(");
                for (i, par) in m.params.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_param(par);
                }
                self.write(") -> ");
                self.print_type(&m.return_ty);
                self.newline();
            }
        }
        self.indent_dec();
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Statements
    // ─────────────────────────────────────────────────────────────────────

    fn print_block(&mut self, block: &Block) {
        self.newline();
        self.indent_inc();
        if block.stmts.is_empty() {
            self.writeln("pass");
        } else {
            for stmt in &block.stmts {
                self.print_stmt(stmt);
            }
        }
        self.indent_dec();
    }

    fn print_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, ty, init, .. } => {
                self.write(name);
                self.write(": ");
                self.print_type(ty);
                self.write(" = ");
                self.print_expr(init);
                self.newline();
            }
            Stmt::Assign { target, value, .. } => {
                self.print_lvalue(target);
                self.write(" = ");
                self.print_expr(value);
                self.newline();
            }
            Stmt::AugAssign { target, op, value, .. } => {
                self.print_lvalue(target);
                self.write(" ");
                self.write(aug_op_str(*op));
                self.write(" ");
                self.print_expr(value);
                self.newline();
            }
            Stmt::Return { value, .. } => {
                if let Some(v) = value {
                    self.write("return ");
                    self.print_expr(v);
                } else {
                    self.write("return");
                }
                self.newline();
            }
            Stmt::If { cond, then_block, elifs, else_block, .. } => {
                self.write("if ");
                self.print_expr(cond);
                self.write(":");
                self.print_block(then_block);
                for (c, b) in elifs {
                    self.write("elif ");
                    self.print_expr(c);
                    self.write(":");
                    self.print_block(b);
                }
                if let Some(eb) = else_block {
                    self.write("else:");
                    self.print_block(eb);
                }
            }
            Stmt::While { cond, body, else_block, .. } => {
                self.write("while ");
                self.print_expr(cond);
                self.write(":");
                self.print_block(body);
                if let Some(eb) = else_block {
                    self.write("else:");
                    self.print_block(eb);
                }
            }
            Stmt::For { var, var_ty, iter, body, else_block, .. } => {
                self.write("for ");
                self.write(var);
                self.write(": ");
                self.print_type(var_ty);
                self.write(" in ");
                self.print_expr(iter);
                self.write(":");
                self.print_block(body);
                if let Some(eb) = else_block {
                    self.write("else:");
                    self.print_block(eb);
                }
            }
            Stmt::Match { scrutinee, arms, .. } => {
                self.write("match ");
                self.print_expr(scrutinee);
                self.write(":");
                self.newline();
                self.indent_inc();
                if arms.is_empty() {
                    self.writeln("pass");
                } else {
                    for arm in arms {
                        self.write("case ");
                        self.print_pattern(&arm.pattern);
                        if let Some(g) = &arm.guard {
                            self.write(" if ");
                            self.print_expr(g);
                        }
                        self.write(":");
                        self.print_block(&arm.body);
                    }
                }
                self.indent_dec();
            }
            Stmt::Try { body, handlers, else_block, finally_block, .. } => {
                self.write("try:");
                self.print_block(body);
                for h in handlers {
                    self.write("except ");
                    self.print_type(&h.exc_ty);
                    if let Some(b) = &h.binding {
                        self.write(" as ");
                        self.write(b);
                    }
                    self.write(":");
                    self.print_block(&h.body);
                }
                if let Some(eb) = else_block {
                    self.write("else:");
                    self.print_block(eb);
                }
                if let Some(fb) = finally_block {
                    self.write("finally:");
                    self.print_block(fb);
                }
            }
            Stmt::With { expr, binding, body, .. } => {
                self.write("with ");
                self.print_expr(expr);
                if let Some((name, ty)) = binding {
                    self.write(" as ");
                    self.write(name);
                    self.write(": ");
                    self.print_type(ty);
                }
                self.write(":");
                self.print_block(body);
            }
            Stmt::Raise { exc, cause, .. } => {
                self.write("raise ");
                self.print_expr(exc);
                if let Some(c) = cause {
                    self.write(" from ");
                    self.print_expr(c);
                }
                self.newline();
            }
            Stmt::Assert { cond, msg, .. } => {
                self.write("assert ");
                self.print_expr(cond);
                if let Some(m) = msg {
                    self.write(", ");
                    self.print_expr(m);
                }
                self.newline();
            }
            Stmt::Del { target, .. } => {
                self.write("del ");
                self.print_lvalue(target);
                self.newline();
            }
            Stmt::Expr { expr, .. } => {
                self.print_expr(expr);
                self.newline();
            }
            Stmt::Break { .. } => self.writeln("break"),
            Stmt::Continue { .. } => self.writeln("continue"),
            Stmt::Pass { .. } => self.writeln("pass"),
        }
    }

    fn print_lvalue(&mut self, lv: &Lvalue) {
        match lv {
            Lvalue::Ident { name, .. } => self.write(name),
            Lvalue::Attr { obj, name, .. } => {
                self.print_expr_atom(obj);
                self.write(".");
                self.write(name);
            }
            Lvalue::Index { obj, indices, .. } => {
                self.print_expr_atom(obj);
                self.write("[");
                for (i, idx) in indices.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_expr(idx);
                }
                self.write("]");
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Patterns
    // ─────────────────────────────────────────────────────────────────────

    fn print_pattern(&mut self, pat: &Pattern) {
        match pat {
            Pattern::Literal(lit, _) => self.print_literal(lit),
            Pattern::Identifier(name, _) => self.write(name),
            Pattern::Wildcard(_) => self.write("_"),
            Pattern::Constructor { ty, fields, .. } => {
                self.print_type(ty);
                self.write("(");
                for (i, f) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_pattern(f);
                }
                self.write(")");
            }
            Pattern::Tuple(items, _) => {
                self.write("(");
                for (i, p) in items.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_pattern(p);
                }
                self.write(")");
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Expressions
    // ─────────────────────────────────────────────────────────────────────

    fn print_expr(&mut self, e: &Expr) {
        self.print_expr_prec(e, 0);
    }

    /// Print an expression that will appear as the receiver/callee of a
    /// postfix operator (`.`, `[]`, `()`). Adds parens if it's not already
    /// atomic.
    fn print_expr_atom(&mut self, e: &Expr) {
        if needs_atom_parens(e) {
            self.write("(");
            self.print_expr(e);
            self.write(")");
        } else {
            self.print_expr(e);
        }
    }

    fn print_expr_prec(&mut self, e: &Expr, parent_prec: u8) {
        match e {
            Expr::Literal { lit, .. } => self.print_literal(lit),
            Expr::Ident { name, .. } => self.write(name),
            Expr::Tuple { elems, .. } => {
                self.write("(");
                if elems.len() == 1 {
                    self.print_expr(&elems[0]);
                    self.write(",");
                } else {
                    for (i, x) in elems.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.print_expr(x);
                    }
                }
                self.write(")");
            }
            Expr::List { elems, .. } => {
                self.write("[");
                for (i, x) in elems.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_expr(x);
                }
                self.write("]");
            }
            Expr::Dict { entries, .. } => {
                self.write("{");
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_expr(k);
                    self.write(": ");
                    self.print_expr(v);
                }
                self.write("}");
            }
            Expr::Set { elems, .. } => {
                self.write("{");
                for (i, x) in elems.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_expr(x);
                }
                self.write("}");
            }
            Expr::Unary { op, operand, .. } => {
                let (sym, sep) = match op {
                    UnaryOp::Neg => ("-", false),
                    UnaryOp::Pos => ("+", false),
                    UnaryOp::BitNot => ("~", false),
                    UnaryOp::Not => ("not", true),
                };
                let needs_paren = parent_prec > PREC_UNARY;
                if needs_paren {
                    self.write("(");
                }
                self.write(sym);
                if sep {
                    self.write(" ");
                }
                self.print_expr_prec(operand, PREC_UNARY);
                if needs_paren {
                    self.write(")");
                }
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                let prec = precedence(*op);
                let needs_paren = prec < parent_prec;
                if needs_paren {
                    self.write("(");
                }
                // Left-assoc for all except Pow. Pow is right-assoc.
                let (left_min, right_min) = if *op == BinOp::Pow {
                    // Right-assoc: parenthesize left if same prec, allow same on right.
                    (prec + 1, prec)
                } else {
                    (prec, prec + 1)
                };
                self.print_expr_prec(lhs, left_min);
                self.write(" ");
                self.write(bin_op_str(*op));
                self.write(" ");
                self.print_expr_prec(rhs, right_min);
                if needs_paren {
                    self.write(")");
                }
            }
            Expr::Call { callee, args, .. } => {
                self.print_expr_atom(callee);
                self.write("(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_arg(a);
                }
                self.write(")");
            }
            Expr::MethodCall { receiver, method, args, .. } => {
                self.print_expr_atom(receiver);
                self.write(".");
                self.write(method);
                self.write("(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_arg(a);
                }
                self.write(")");
            }
            Expr::Attr { obj, name, .. } => {
                self.print_expr_atom(obj);
                self.write(".");
                self.write(name);
            }
            Expr::Index { obj, indices, .. } => {
                self.print_expr_atom(obj);
                self.write("[");
                for (i, x) in indices.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_expr(x);
                }
                self.write("]");
            }
            Expr::NullCoalesce { lhs, rhs, .. } => {
                let needs_paren = parent_prec > PREC_NULL_COALESCE;
                if needs_paren {
                    self.write("(");
                }
                self.print_expr_prec(lhs, PREC_NULL_COALESCE + 1);
                self.write(" ?? ");
                self.print_expr_prec(rhs, PREC_NULL_COALESCE);
                if needs_paren {
                    self.write(")");
                }
            }
            Expr::Ternary { cond, then_expr, else_expr, .. } => {
                let needs_paren = parent_prec > PREC_TERNARY;
                if needs_paren {
                    self.write("(");
                }
                self.print_expr_prec(then_expr, PREC_TERNARY + 1);
                self.write(" if ");
                self.print_expr_prec(cond, PREC_TERNARY + 1);
                self.write(" else ");
                self.print_expr_prec(else_expr, PREC_TERNARY);
                if needs_paren {
                    self.write(")");
                }
            }
            Expr::Lambda { params, return_ty, body, .. } => {
                let needs_paren = parent_prec > PREC_LAMBDA;
                if needs_paren {
                    self.write("(");
                }
                self.write("fn(");
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_param(p);
                }
                self.write(") -> ");
                self.print_type(return_ty);
                self.write(": ");
                self.print_expr(body);
                if needs_paren {
                    self.write(")");
                }
            }
            Expr::Cast { expr, target, .. } => {
                // No dedicated syntax in §4; spec uses `T(value)` for casts
                // (e.g. `i64(x)`). Print as a constructor-style call.
                self.print_type(target);
                self.write("(");
                self.print_expr(expr);
                self.write(")");
            }
        }
    }

    fn print_arg(&mut self, a: &Arg) {
        if let Some(name) = &a.name {
            self.write(name);
            self.write("=");
        }
        self.print_expr(&a.value);
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Literals
    // ─────────────────────────────────────────────────────────────────────

    fn print_literal(&mut self, lit: &Literal) {
        match lit {
            Literal::Int { value, suffix } => {
                self.write(&value.to_string());
                if let Some(s) = suffix {
                    self.write(int_suffix_str(*s));
                }
            }
            Literal::Float { value, suffix } => {
                let s = format_float(*value);
                self.write(&s);
                if let Some(sf) = suffix {
                    self.write(float_suffix_str(*sf));
                }
            }
            Literal::Str(s) => self.write(&format_str_literal(s)),
            Literal::Bytes(b) => self.write(&format_bytes_literal(b)),
            Literal::Char(c) => self.write(&format_char_literal(*c)),
            Literal::Bool(true) => self.write("true"),
            Literal::Bool(false) => self.write("false"),
            Literal::None => self.write("none"),
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Types
    // ─────────────────────────────────────────────────────────────────────

    fn print_type(&mut self, t: &Type) {
        match t {
            Type::Named { name, args, .. } => {
                self.write(name);
                if !args.is_empty() {
                    self.write("[");
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.print_type(a);
                    }
                    self.write("]");
                }
            }
            Type::Nullable { inner, .. } => {
                self.print_type(inner);
                self.write("?");
            }
            Type::Function { params, ret, .. } => {
                self.write("fn(");
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_type(p);
                }
                self.write(") -> ");
                self.print_type(ret);
            }
            Type::Tuple { elems, .. } => {
                self.write("(");
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_type(e);
                }
                self.write(")");
            }
            Type::Infer { .. } => self.write("_"),
            Type::Never { .. } => self.write("Never"),
        }
    }
}

impl Default for PrettyPrinter {
    fn default() -> Self { Self::new() }
}

// ─────────────────────────────────────────────────────────────────────────
//  Helpers: precedence, op names, literal formatting
// ─────────────────────────────────────────────────────────────────────────

// Precedence levels: higher = binds tighter. Mirrors the grammar in §4.
const PREC_TERNARY: u8       = 1;
const PREC_LAMBDA: u8        = 1;
const PREC_OR: u8            = 2;
const PREC_AND: u8           = 3;
// `not` is unary; for nested comparisons we still treat it via PREC_UNARY.
const PREC_NULL_COALESCE: u8 = 4;
const PREC_CMP: u8           = 5;  // ==, !=, <, >, <=, >=, is, in, ...
const PREC_BIT_OR: u8        = 6;
const PREC_BIT_XOR: u8       = 7;
const PREC_BIT_AND: u8       = 8;
const PREC_SHIFT: u8         = 9;
const PREC_ADD: u8           = 10;
const PREC_MUL: u8           = 11;
const PREC_POW: u8           = 12;
const PREC_UNARY: u8         = 13;

fn precedence(op: BinOp) -> u8 {
    match op {
        BinOp::Or => PREC_OR,
        BinOp::And => PREC_AND,
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
        | BinOp::Is | BinOp::IsNot | BinOp::In | BinOp::NotIn => PREC_CMP,
        BinOp::BitOr => PREC_BIT_OR,
        BinOp::BitXor => PREC_BIT_XOR,
        BinOp::BitAnd => PREC_BIT_AND,
        BinOp::Shl | BinOp::Shr => PREC_SHIFT,
        BinOp::Add | BinOp::Sub => PREC_ADD,
        BinOp::Mul | BinOp::Div | BinOp::FloorDiv | BinOp::Rem => PREC_MUL,
        BinOp::Pow => PREC_POW,
    }
}

fn bin_op_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::FloorDiv => "//",
        BinOp::Rem => "%",
        BinOp::Pow => "**",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::Is => "is",
        BinOp::IsNot => "is not",
        BinOp::In => "in",
        BinOp::NotIn => "not in",
        BinOp::And => "and",
        BinOp::Or => "or",
    }
}

fn aug_op_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+=",
        BinOp::Sub => "-=",
        BinOp::Mul => "*=",
        BinOp::Div => "/=",
        BinOp::FloorDiv => "//=",
        BinOp::Rem => "%=",
        BinOp::Pow => "**=",
        BinOp::BitAnd => "&=",
        BinOp::BitOr => "|=",
        BinOp::BitXor => "^=",
        BinOp::Shl => "<<=",
        BinOp::Shr => ">>=",
        // The grammar only defines aug-assign for the operators above; for
        // any other BinOp we fall back to a best-effort string. This branch
        // should be unreachable in well-formed ASTs.
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
        | BinOp::Is | BinOp::IsNot | BinOp::In | BinOp::NotIn
        | BinOp::And | BinOp::Or => "=",
    }
}

fn int_suffix_str(s: IntSuffix) -> &'static str {
    match s {
        IntSuffix::I8 => "i8",
        IntSuffix::I16 => "i16",
        IntSuffix::I32 => "i32",
        IntSuffix::I64 => "i64",
        IntSuffix::U8 => "u8",
        IntSuffix::U16 => "u16",
        IntSuffix::U32 => "u32",
        IntSuffix::U64 => "u64",
    }
}

fn float_suffix_str(s: FloatSuffix) -> &'static str {
    match s {
        FloatSuffix::F32 => "f32",
        FloatSuffix::F64 => "f64",
    }
}

/// Format an f64 so that it round-trips through the lexer as a float (not
/// an int). Always includes a decimal point or exponent.
fn format_float(v: f64) -> String {
    if v.is_nan() {
        return "(0.0 / 0.0)".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 { "(1.0 / 0.0)".to_string() } else { "(-1.0 / 0.0)".to_string() };
    }
    let s = format!("{}", v);
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{}.0", s)
    }
}

/// Format a `str` literal, re-escaping control characters. Uses a triple-
/// quoted form when the contents contain a literal newline.
fn format_str_literal(s: &str) -> String {
    if s.contains('\n') {
        // Triple-quoted form. We still escape backslashes and triple-quote
        // runs to keep the literal unambiguous.
        let mut out = String::from("\"\"\"");
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' => out.push_str("\\\\"),
                '"' => {
                    // Escape a `"` only if it would form `"""`.
                    let mut ahead = chars.clone();
                    let next1 = ahead.next();
                    let next2 = ahead.next();
                    if next1 == Some('"') && next2 == Some('"') {
                        out.push_str("\\\"");
                    } else {
                        out.push('"');
                    }
                }
                _ => out.push(c),
            }
        }
        out.push_str("\"\"\"");
        out
    } else {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '\0' => out.push_str("\\0"),
                c if (c as u32) < 0x20 => {
                    out.push_str(&format!("\\x{:02x}", c as u32));
                }
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }
}

fn format_bytes_literal(bytes: &[u8]) -> String {
    let mut out = String::from("b\"");
    for &b in bytes {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0 => out.push_str("\\0"),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\x{:02x}", b)),
        }
    }
    out.push('"');
    out
}

fn format_char_literal(c: char) -> String {
    let mut out = String::from("'");
    match c {
        '\\' => out.push_str("\\\\"),
        '\'' => out.push_str("\\'"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        '\0' => out.push_str("\\0"),
        c if (c as u32) < 0x20 => out.push_str(&format!("\\x{:02x}", c as u32)),
        c => out.push(c),
    }
    out.push('\'');
    out
}

/// Whether an expression in callee/receiver/subscript-object position needs
/// to be wrapped in parens.
fn needs_atom_parens(e: &Expr) -> bool {
    !matches!(
        e,
        Expr::Literal { .. }
            | Expr::Ident { .. }
            | Expr::Tuple { .. }
            | Expr::List { .. }
            | Expr::Dict { .. }
            | Expr::Set { .. }
            | Expr::Call { .. }
            | Expr::MethodCall { .. }
            | Expr::Attr { .. }
            | Expr::Index { .. }
    )
}

// ─────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy() -> Span { Span::DUMMY }

    fn ident(name: &str) -> Expr {
        Expr::Ident { name: name.to_string(), span: dummy() }
    }

    fn named_ty(name: &str) -> Type {
        Type::Named { name: name.to_string(), args: vec![], span: dummy() }
    }

    fn generic_ty(name: &str, args: Vec<Type>) -> Type {
        Type::Named { name: name.to_string(), args, span: dummy() }
    }

    fn int_lit(v: i128) -> Expr {
        Expr::Literal {
            lit: Literal::Int { value: v, suffix: None },
            span: dummy(),
        }
    }

    fn binary(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
        Expr::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs), span: dummy() }
    }

    fn empty_block() -> Block {
        Block { stmts: vec![], span: dummy() }
    }

    fn block(stmts: Vec<Stmt>) -> Block {
        Block { stmts, span: dummy() }
    }

    fn module(decls: Vec<TopDecl>) -> Module {
        Module {
            name: "test".to_string(),
            imports: vec![],
            decls,
            span: dummy(),
        }
    }

    fn func(name: &str, params: Vec<Param>, ret: Type, body: Block) -> FuncDecl {
        FuncDecl {
            name: name.to_string(),
            generics: vec![],
            params,
            return_ty: ret,
            body,
            decorators: vec![],
            span: dummy(),
        }
    }

    #[test]
    fn test_print_minimal_function() {
        let body = block(vec![Stmt::Return {
            value: Some(int_lit(0)),
            span: dummy(),
        }]);
        let m = module(vec![TopDecl::Func(func("main", vec![], named_ty("i32"), body))]);
        let out = print_module(&m);
        assert_eq!(out, "fn main() -> i32:\n    return 0\n");
    }

    #[test]
    fn test_print_let_and_assign() {
        let stmts = vec![
            Stmt::Let {
                name: "x".to_string(),
                ty: named_ty("i32"),
                init: int_lit(1),
                span: dummy(),
            },
            Stmt::Assign {
                target: Lvalue::Ident { name: "x".to_string(), span: dummy() },
                value: int_lit(2),
                span: dummy(),
            },
            Stmt::AugAssign {
                target: Lvalue::Ident { name: "x".to_string(), span: dummy() },
                op: BinOp::Add,
                value: int_lit(3),
                span: dummy(),
            },
        ];
        let m = module(vec![TopDecl::Func(func(
            "f",
            vec![],
            named_ty("None"),
            block(stmts),
        ))]);
        let out = print_module(&m);
        assert_eq!(
            out,
            "fn f() -> None:\n    x: i32 = 1\n    x = 2\n    x += 3\n"
        );
    }

    #[test]
    fn test_print_if_elif_else() {
        let body = block(vec![Stmt::If {
            cond: ident("a"),
            then_block: block(vec![Stmt::Return { value: Some(int_lit(1)), span: dummy() }]),
            elifs: vec![(
                ident("b"),
                block(vec![Stmt::Return { value: Some(int_lit(2)), span: dummy() }]),
            )],
            else_block: Some(block(vec![Stmt::Return {
                value: Some(int_lit(3)),
                span: dummy(),
            }])),
            span: dummy(),
        }]);
        let m = module(vec![TopDecl::Func(func("f", vec![], named_ty("i32"), body))]);
        let out = print_module(&m);
        let expected = "fn f() -> i32:\n    if a:\n        return 1\n    elif b:\n        return 2\n    else:\n        return 3\n";
        assert_eq!(out, expected);
    }

    #[test]
    fn test_print_while_for() {
        let stmts = vec![
            Stmt::While {
                cond: ident("cond"),
                body: block(vec![Stmt::Break { span: dummy() }]),
                else_block: None,
                span: dummy(),
            },
            Stmt::For {
                var: "i".to_string(),
                var_ty: named_ty("i64"),
                iter: ident("xs"),
                body: block(vec![Stmt::Continue { span: dummy() }]),
                else_block: None,
                span: dummy(),
            },
        ];
        let m = module(vec![TopDecl::Func(func(
            "f",
            vec![],
            named_ty("None"),
            block(stmts),
        ))]);
        let out = print_module(&m);
        let expected = "fn f() -> None:\n    while cond:\n        break\n    for i: i64 in xs:\n        continue\n";
        assert_eq!(out, expected);
    }

    #[test]
    fn test_print_class_with_methods() {
        let init = FuncDecl {
            name: "__init__".to_string(),
            generics: vec![],
            params: vec![
                Param { name: "self".to_string(), ty: named_ty("Self"), default: None, span: dummy() },
                Param { name: "x".to_string(), ty: named_ty("i32"), default: None, span: dummy() },
            ],
            return_ty: named_ty("None"),
            body: block(vec![Stmt::Pass { span: dummy() }]),
            decorators: vec![],
            span: dummy(),
        };
        let m_get = FuncDecl {
            name: "get".to_string(),
            generics: vec![],
            params: vec![Param {
                name: "self".to_string(),
                ty: named_ty("Self"),
                default: None,
                span: dummy(),
            }],
            return_ty: named_ty("i32"),
            body: block(vec![Stmt::Return {
                value: Some(Expr::Attr {
                    obj: Box::new(ident("self")),
                    name: "x".to_string(),
                    span: dummy(),
                }),
                span: dummy(),
            }]),
            decorators: vec![],
            span: dummy(),
        };
        let cls = ClassDecl {
            name: "Box".to_string(),
            modifier: ClassModifier::Final,
            generics: vec![],
            bases: vec![],
            fields: vec![FieldDecl {
                name: "x".to_string(),
                ty: named_ty("i32"),
                default: None,
                span: dummy(),
            }],
            methods: vec![m_get],
            init: Some(init),
            span: dummy(),
        };
        let m = module(vec![TopDecl::Class(cls)]);
        let out = print_module(&m);
        let expected = "\
final class Box:
    x: i32
    fn __init__(self, x: i32) -> None:
        pass
    fn get(self) -> i32:
        return self.x
";
        assert_eq!(out, expected);
    }

    #[test]
    fn test_print_protocol() {
        let proto = ProtocolDecl {
            name: "Hashable".to_string(),
            generics: vec![],
            methods: vec![ProtoMethod {
                name: "hash".to_string(),
                params: vec![Param {
                    name: "self".to_string(),
                    ty: named_ty("Self"),
                    default: None,
                    span: dummy(),
                }],
                return_ty: named_ty("i64"),
                span: dummy(),
            }],
            span: dummy(),
        };
        let m = module(vec![TopDecl::Protocol(proto)]);
        let out = print_module(&m);
        assert_eq!(out, "protocol Hashable:\n    fn hash(self) -> i64\n");
    }

    #[test]
    fn test_print_import_and_from_import() {
        let m = Module {
            name: "t".to_string(),
            imports: vec![
                Import {
                    path: vec!["math".to_string()],
                    items: vec![],
                    alias: Some("m".to_string()),
                    span: dummy(),
                },
                Import {
                    path: vec!["threading".to_string()],
                    items: vec![
                        ImportItem { name: "Thread".to_string(), alias: None },
                        ImportItem { name: "Channel".to_string(), alias: Some("Ch".to_string()) },
                    ],
                    alias: None,
                    span: dummy(),
                },
            ],
            decls: vec![],
            span: dummy(),
        };
        let out = print_module(&m);
        assert_eq!(
            out,
            "import math as m\nfrom threading import Thread, Channel as Ch\n"
        );
    }

    #[test]
    fn test_print_match() {
        let body = block(vec![Stmt::Match {
            scrutinee: ident("x"),
            arms: vec![
                MatchArm {
                    pattern: Pattern::Literal(Literal::Int { value: 0, suffix: None }, dummy()),
                    guard: None,
                    body: block(vec![Stmt::Return { value: Some(int_lit(1)), span: dummy() }]),
                    span: dummy(),
                },
                MatchArm {
                    pattern: Pattern::Identifier("n".to_string(), dummy()),
                    guard: Some(binary(BinOp::Gt, ident("n"), int_lit(0))),
                    body: block(vec![Stmt::Return { value: Some(ident("n")), span: dummy() }]),
                    span: dummy(),
                },
                MatchArm {
                    pattern: Pattern::Wildcard(dummy()),
                    guard: None,
                    body: block(vec![Stmt::Return { value: Some(int_lit(-1)), span: dummy() }]),
                    span: dummy(),
                },
            ],
            span: dummy(),
        }]);
        let m = module(vec![TopDecl::Func(func("f", vec![], named_ty("i32"), body))]);
        let out = print_module(&m);
        let expected = "\
fn f() -> i32:
    match x:
        case 0:
            return 1
        case n if n > 0:
            return n
        case _:
            return -1
";
        assert_eq!(out, expected);
    }

    #[test]
    fn test_print_try_except_finally() {
        let body = block(vec![Stmt::Try {
            body: block(vec![Stmt::Pass { span: dummy() }]),
            handlers: vec![ExceptHandler {
                exc_ty: named_ty("ValueError"),
                binding: Some("e".to_string()),
                body: block(vec![Stmt::Pass { span: dummy() }]),
                span: dummy(),
            }],
            else_block: Some(block(vec![Stmt::Pass { span: dummy() }])),
            finally_block: Some(block(vec![Stmt::Pass { span: dummy() }])),
            span: dummy(),
        }]);
        let m = module(vec![TopDecl::Func(func("f", vec![], named_ty("None"), body))]);
        let out = print_module(&m);
        let expected = "\
fn f() -> None:
    try:
        pass
    except ValueError as e:
        pass
    else:
        pass
    finally:
        pass
";
        assert_eq!(out, expected);
    }

    #[test]
    fn test_print_with_stmt() {
        let body = block(vec![Stmt::With {
            expr: Expr::Call {
                callee: Box::new(ident("open")),
                args: vec![Arg {
                    name: None,
                    value: Expr::Literal {
                        lit: Literal::Str("a.txt".to_string()),
                        span: dummy(),
                    },
                    span: dummy(),
                }],
                span: dummy(),
            },
            binding: Some(("f".to_string(), named_ty("File"))),
            body: block(vec![Stmt::Pass { span: dummy() }]),
            span: dummy(),
        }]);
        let m = module(vec![TopDecl::Func(func("f", vec![], named_ty("None"), body))]);
        let out = print_module(&m);
        let expected = "\
fn f() -> None:
    with open(\"a.txt\") as f: File:
        pass
";
        assert_eq!(out, expected);
    }

    #[test]
    fn test_binary_precedence_parens() {
        // (1 + 2) * 3 -> "(1 + 2) * 3"
        let e1 = binary(BinOp::Mul, binary(BinOp::Add, int_lit(1), int_lit(2)), int_lit(3));
        let mut p = PrettyPrinter::new();
        p.print_expr(&e1);
        assert_eq!(p.buf, "(1 + 2) * 3");

        // 1 + 2 * 3 -> "1 + 2 * 3" (no extra parens)
        let e2 = binary(BinOp::Add, int_lit(1), binary(BinOp::Mul, int_lit(2), int_lit(3)));
        let mut p = PrettyPrinter::new();
        p.print_expr(&e2);
        assert_eq!(p.buf, "1 + 2 * 3");

        // Left-assoc: 1 - 2 - 3 -> "1 - 2 - 3"
        let e3 = binary(BinOp::Sub, binary(BinOp::Sub, int_lit(1), int_lit(2)), int_lit(3));
        let mut p = PrettyPrinter::new();
        p.print_expr(&e3);
        assert_eq!(p.buf, "1 - 2 - 3");

        // Left-assoc rhs needs parens: 1 - (2 - 3) -> "1 - (2 - 3)"
        let e4 = binary(BinOp::Sub, int_lit(1), binary(BinOp::Sub, int_lit(2), int_lit(3)));
        let mut p = PrettyPrinter::new();
        p.print_expr(&e4);
        assert_eq!(p.buf, "1 - (2 - 3)");
    }

    #[test]
    fn test_pow_right_assoc() {
        // 2 ** (3 ** 4) -> "2 ** 3 ** 4"
        let e1 = binary(BinOp::Pow, int_lit(2), binary(BinOp::Pow, int_lit(3), int_lit(4)));
        let mut p = PrettyPrinter::new();
        p.print_expr(&e1);
        assert_eq!(p.buf, "2 ** 3 ** 4");

        // (2 ** 3) ** 4 -> "(2 ** 3) ** 4"
        let e2 = binary(BinOp::Pow, binary(BinOp::Pow, int_lit(2), int_lit(3)), int_lit(4));
        let mut p = PrettyPrinter::new();
        p.print_expr(&e2);
        assert_eq!(p.buf, "(2 ** 3) ** 4");
    }

    #[test]
    fn test_lambda() {
        // fn(x: i32) -> i32: x + 1
        let lam = Expr::Lambda {
            params: vec![Param {
                name: "x".to_string(),
                ty: named_ty("i32"),
                default: None,
                span: dummy(),
            }],
            return_ty: named_ty("i32"),
            body: Box::new(binary(BinOp::Add, ident("x"), int_lit(1))),
            span: dummy(),
        };
        let mut p = PrettyPrinter::new();
        p.print_expr(&lam);
        assert_eq!(p.buf, "fn(x: i32) -> i32: x + 1");
    }

    #[test]
    fn test_nullable_type() {
        let t = Type::Nullable {
            inner: Box::new(named_ty("i32")),
            span: dummy(),
        };
        let mut p = PrettyPrinter::new();
        p.print_type(&t);
        assert_eq!(p.buf, "i32?");
    }

    #[test]
    fn test_generic_type() {
        let list = generic_ty("List", vec![named_ty("i32")]);
        let mut p = PrettyPrinter::new();
        p.print_type(&list);
        assert_eq!(p.buf, "List[i32]");

        let dict = generic_ty("Dict", vec![named_ty("str"), named_ty("i32")]);
        let mut p = PrettyPrinter::new();
        p.print_type(&dict);
        assert_eq!(p.buf, "Dict[str, i32]");
    }

    #[test]
    fn test_function_type() {
        let t = Type::Function {
            params: vec![named_ty("i32"), named_ty("str")],
            ret: Box::new(named_ty("bool")),
            span: dummy(),
        };
        let mut p = PrettyPrinter::new();
        p.print_type(&t);
        assert_eq!(p.buf, "fn(i32, str) -> bool");
    }

    #[test]
    fn test_decorator_on_function() {
        let f = FuncDecl {
            name: "f".to_string(),
            generics: vec![],
            params: vec![],
            return_ty: named_ty("i32"),
            body: block(vec![Stmt::Return { value: Some(int_lit(0)), span: dummy() }]),
            decorators: vec![Decorator {
                path: vec!["pure".to_string()],
                args: vec![],
                span: dummy(),
            }],
            span: dummy(),
        };
        let m = module(vec![TopDecl::Func(f)]);
        let out = print_module(&m);
        assert_eq!(out, "@pure\nfn f() -> i32:\n    return 0\n");
    }

    #[test]
    fn test_class_modifier() {
        for (modifier, kw) in [
            (ClassModifier::Final, "final"),
            (ClassModifier::Open, "open"),
            (ClassModifier::Sealed, "sealed"),
        ] {
            let cls = ClassDecl {
                name: "C".to_string(),
                modifier,
                generics: vec![],
                bases: vec![],
                fields: vec![],
                methods: vec![],
                init: None,
                span: dummy(),
            };
            let m = module(vec![TopDecl::Class(cls)]);
            let out = print_module(&m);
            let expected = format!("{} class C:\n    pass\n", kw);
            assert_eq!(out, expected);
        }
    }

    #[test]
    fn test_string_escape() {
        let lit = Literal::Str("a\nb\t\"c\\".to_string());
        let mut p = PrettyPrinter::new();
        p.print_literal(&lit);
        // Actually contains a literal newline, so triple-quoted form:
        assert!(p.buf.starts_with("\"\"\""));
        assert!(p.buf.ends_with("\"\"\""));

        // Without newline: regular escape.
        let lit2 = Literal::Str("tab\there\"quote".to_string());
        let mut p = PrettyPrinter::new();
        p.print_literal(&lit2);
        assert_eq!(p.buf, "\"tab\\there\\\"quote\"");

        // \n alone (no actual newline in string would be re-escaped) — verify
        // that an escaped-newline character produces \\n in the source.
        let lit3 = Literal::Str("x\\ny".to_string()); // contents literal backslash-n
        let mut p = PrettyPrinter::new();
        p.print_literal(&lit3);
        assert_eq!(p.buf, "\"x\\\\ny\"");
    }
}
