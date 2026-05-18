//! Parser. See spec §4 (grammar) and §10.3.
//!
//! Recursive descent with Pratt-style expression parsing. Consumes the
//! token stream produced by [`crate::lexer::Lexer`] and produces an
//! untyped [`Module`].

use crate::ast::*;
use crate::error::CompileError;
use crate::lexer::{Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    file: String,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0, file: String::from("<input>") }
    }

    pub fn with_file(tokens: Vec<Token>, file: String) -> Self {
        Self { tokens, pos: 0, file }
    }

    /// Parse a full module (the top-level `module` production from spec §4).
    pub fn parse_module(&mut self) -> Result<Module, CompileError> {
        let start = self.cur_span();
        // Skip any leading newlines.
        self.skip_newlines();

        let mut imports = Vec::new();
        let mut decls = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek_kind() {
                TokenKind::Eof => break,
                TokenKind::KwFrom | TokenKind::KwImport => {
                    imports.push(self.parse_import()?);
                }
                // Module-level docstring: a bare string literal on its own
                // line. The spec (§6.1) forbids top-level statements but
                // docstrings are a universal convention.
                TokenKind::StrLit(_) | TokenKind::RawStrLit(_)
                    if matches!(
                        self.peek_at(1),
                        TokenKind::Newline | TokenKind::Eof
                    ) =>
                {
                    self.bump();
                    self.expect_newline()?;
                }
                _ => decls.push(self.parse_top_decl()?),
            }
        }

        let end = self.cur_span();
        Ok(Module {
            name: String::new(),
            imports,
            decls,
            span: merge_spans(start, end),
        })
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Helpers — token stream
    // ─────────────────────────────────────────────────────────────────────

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    fn peek_at(&self, offset: usize) -> &TokenKind {
        let idx = (self.pos + offset).min(self.tokens.len() - 1);
        &self.tokens[idx].kind
    }

    fn cur_span(&self) -> Span {
        self.peek().span
    }

    fn prev_span(&self) -> Span {
        if self.pos == 0 {
            self.cur_span()
        } else {
            self.tokens[self.pos - 1].span
        }
    }

    fn bump(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if std::mem::discriminant(self.peek_kind()) == std::mem::discriminant(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenKind, what: &str) -> Result<Token, CompileError> {
        if std::mem::discriminant(self.peek_kind()) == std::mem::discriminant(kind) {
            Ok(self.bump())
        } else {
            Err(self.err(format!(
                "expected {}, found {:?}",
                what,
                self.peek_kind()
            )))
        }
    }

    fn expect_ident(&mut self) -> Result<String, CompileError> {
        match self.peek_kind().clone() {
            TokenKind::Ident(s) => {
                self.bump();
                Ok(s)
            }
            other => Err(self.err(format!("expected identifier, found {:?}", other))),
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek_kind(), TokenKind::Newline) {
            self.bump();
        }
    }

    fn expect_newline(&mut self) -> Result<(), CompileError> {
        match self.peek_kind() {
            TokenKind::Newline | TokenKind::Eof => {
                if matches!(self.peek_kind(), TokenKind::Newline) {
                    self.bump();
                }
                Ok(())
            }
            other => Err(self.err(format!("expected newline, found {:?}", other))),
        }
    }

    fn err(&self, message: String) -> CompileError {
        let span = self.cur_span();
        CompileError::Parse {
            file: self.file.clone(),
            line: span.line,
            col: span.col,
            message,
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Imports
    // ─────────────────────────────────────────────────────────────────────

    fn parse_import(&mut self) -> Result<Import, CompileError> {
        let start = self.cur_span();
        if self.eat(&TokenKind::KwFrom) {
            let path = self.parse_dotted_name()?;
            self.expect(&TokenKind::KwImport, "'import'")?;
            let mut items = Vec::new();
            loop {
                let name = self.expect_ident()?;
                let alias = if self.eat(&TokenKind::KwAs) {
                    Some(self.expect_ident()?)
                } else {
                    None
                };
                items.push(ImportItem { name, alias });
                if !self.eat(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect_newline()?;
            Ok(Import {
                path,
                items,
                alias: None,
                span: merge_spans(start, self.prev_span()),
            })
        } else {
            self.expect(&TokenKind::KwImport, "'import'")?;
            let path = self.parse_dotted_name()?;
            let alias = if self.eat(&TokenKind::KwAs) {
                Some(self.expect_ident()?)
            } else {
                None
            };
            self.expect_newline()?;
            Ok(Import {
                path,
                items: Vec::new(),
                alias,
                span: merge_spans(start, self.prev_span()),
            })
        }
    }

    fn parse_dotted_name(&mut self) -> Result<Vec<String>, CompileError> {
        let mut parts = vec![self.expect_ident()?];
        while self.eat(&TokenKind::Dot) {
            parts.push(self.expect_ident()?);
        }
        Ok(parts)
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Top-level declarations
    // ─────────────────────────────────────────────────────────────────────

    fn parse_top_decl(&mut self) -> Result<TopDecl, CompileError> {
        // Collect any preceding decorators.
        let mut decorators = Vec::new();
        while matches!(self.peek_kind(), TokenKind::At) {
            decorators.push(self.parse_decorator()?);
            self.skip_newlines();
        }

        match self.peek_kind() {
            TokenKind::KwFn => {
                let f = self.parse_func_decl(decorators)?;
                Ok(TopDecl::Func(f))
            }
            TokenKind::KwClass => {
                if !decorators.is_empty() {
                    return Err(self.err("decorators on classes not supported in v0.1".into()));
                }
                // Bare `class Foo` — implicit Final per spec §1.3 (closed by default).
                Ok(TopDecl::Class(self.parse_class_decl(ClassModifier::Final)?))
            }
            TokenKind::KwFinal => {
                // Could be `final class` or `final NAME : type = expr`.
                if matches!(self.peek_at(1), TokenKind::KwClass) {
                    self.bump(); // consume `final`
                    Ok(TopDecl::Class(self.parse_class_decl(ClassModifier::Final)?))
                } else {
                    Ok(TopDecl::Const(self.parse_const_decl()?))
                }
            }
            TokenKind::KwOpen => {
                self.bump(); // consume `open`
                Ok(TopDecl::Class(self.parse_class_decl(ClassModifier::Open)?))
            }
            TokenKind::KwSealed => {
                self.bump(); // consume `sealed`
                Ok(TopDecl::Class(self.parse_class_decl(ClassModifier::Sealed)?))
            }
            TokenKind::KwProtocol => {
                Ok(TopDecl::Protocol(self.parse_protocol_decl()?))
            }
            TokenKind::Ident(s) if s == "type" => {
                Ok(TopDecl::TypeAlias(self.parse_type_alias()?))
            }
            other => Err(self.err(format!(
                "expected top-level declaration, found {:?}",
                other
            ))),
        }
    }

    fn parse_decorator(&mut self) -> Result<Decorator, CompileError> {
        let start = self.cur_span();
        self.expect(&TokenKind::At, "'@'")?;
        let path = self.parse_dotted_name()?;
        let mut args = Vec::new();
        if self.eat(&TokenKind::LParen) {
            if !matches!(self.peek_kind(), TokenKind::RParen) {
                args = self.parse_arg_list()?;
            }
            self.expect(&TokenKind::RParen, "')'")?;
        }
        self.expect_newline()?;
        Ok(Decorator {
            path,
            args,
            span: merge_spans(start, self.prev_span()),
        })
    }

    fn parse_const_decl(&mut self) -> Result<ConstDecl, CompileError> {
        let start = self.cur_span();
        self.expect(&TokenKind::KwFinal, "'final'")?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Colon, "':'")?;
        let ty = self.parse_type()?;
        self.expect(&TokenKind::Assign, "'='")?;
        let value = self.parse_expr()?;
        self.expect_newline()?;
        Ok(ConstDecl {
            name,
            ty,
            value,
            span: merge_spans(start, self.prev_span()),
        })
    }

    fn parse_type_alias(&mut self) -> Result<TypeAliasDecl, CompileError> {
        let start = self.cur_span();
        // consume the `type` identifier (contextual keyword)
        self.bump();
        let name = self.expect_ident()?;
        let generics = if matches!(self.peek_kind(), TokenKind::LBracket) {
            self.parse_generic_params()?
        } else {
            Vec::new()
        };
        self.expect(&TokenKind::Assign, "'='")?;
        let ty = self.parse_type()?;
        self.expect_newline()?;
        Ok(TypeAliasDecl {
            name,
            generics,
            ty,
            span: merge_spans(start, self.prev_span()),
        })
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Functions
    // ─────────────────────────────────────────────────────────────────────

    fn parse_func_decl(
        &mut self,
        decorators: Vec<Decorator>,
    ) -> Result<FuncDecl, CompileError> {
        let start = self.cur_span();
        self.expect(&TokenKind::KwFn, "'fn'")?;
        let name = self.expect_ident()?;
        let generics = if matches!(self.peek_kind(), TokenKind::LBracket) {
            self.parse_generic_params()?
        } else {
            Vec::new()
        };
        self.expect(&TokenKind::LParen, "'('")?;
        let params = self.parse_params(false)?;
        self.expect(&TokenKind::RParen, "')'")?;
        self.expect(&TokenKind::Arrow, "'->'")?;
        let return_ty = self.parse_type()?;
        self.expect(&TokenKind::Colon, "':'")?;
        let body = self.parse_block()?;
        Ok(FuncDecl {
            name,
            generics,
            params,
            return_ty,
            body,
            decorators,
            span: merge_spans(start, self.prev_span()),
        })
    }

    /// Parse a parameter list. A bare `self` (without `: type`) is always
    /// permitted as the first parameter (per grammar §4: method_decl /
    /// init_decl / proto_member). Free functions simply won't use it.
    fn parse_params(&mut self, _allow_self: bool) -> Result<Vec<Param>, CompileError> {
        let mut params = Vec::new();
        if matches!(self.peek_kind(), TokenKind::RParen) {
            return Ok(params);
        }
        loop {
            let start = self.cur_span();
            if params.is_empty() {
                if let TokenKind::Ident(s) = self.peek_kind() {
                    if s == "self" && !matches!(self.peek_at(1), TokenKind::Colon) {
                        self.bump();
                        params.push(Param {
                            name: "self".into(),
                            ty: Type::Infer { span: start },
                            default: None,
                            span: start,
                        });
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                        continue;
                    }
                }
            }
            let name = self.expect_ident()?;
            self.expect(&TokenKind::Colon, "':'")?;
            let ty = self.parse_type()?;
            let default = if self.eat(&TokenKind::Assign) {
                Some(self.parse_expr()?)
            } else {
                None
            };
            params.push(Param {
                name,
                ty,
                default,
                span: merge_spans(start, self.prev_span()),
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        Ok(params)
    }

    fn parse_generic_params(&mut self) -> Result<Vec<GenericParam>, CompileError> {
        self.expect(&TokenKind::LBracket, "'['")?;
        let mut params = Vec::new();
        loop {
            let start = self.cur_span();
            let name = self.expect_ident()?;
            let mut bounds = Vec::new();
            if self.eat(&TokenKind::Colon) {
                bounds.push(self.parse_type()?);
                while self.eat(&TokenKind::Plus) {
                    bounds.push(self.parse_type()?);
                }
            }
            params.push(GenericParam {
                name,
                bounds,
                span: merge_spans(start, self.prev_span()),
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBracket, "']'")?;
        Ok(params)
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Classes & protocols
    // ─────────────────────────────────────────────────────────────────────

    fn parse_class_decl(
        &mut self,
        modifier: ClassModifier,
    ) -> Result<ClassDecl, CompileError> {
        let start = self.prev_span();
        self.expect(&TokenKind::KwClass, "'class'")?;
        let name = self.expect_ident()?;
        let generics = if matches!(self.peek_kind(), TokenKind::LBracket) {
            self.parse_generic_params()?
        } else {
            Vec::new()
        };
        let mut bases = Vec::new();
        if self.eat(&TokenKind::LParen) {
            if !matches!(self.peek_kind(), TokenKind::RParen) {
                bases.push(self.parse_type()?);
                while self.eat(&TokenKind::Comma) {
                    bases.push(self.parse_type()?);
                }
            }
            self.expect(&TokenKind::RParen, "')'")?;
            if bases.len() > 1 {
                return Err(self.err(
                    "E0005: multiple inheritance forbidden — use protocols instead".into(),
                ));
            }
        }
        self.expect(&TokenKind::Colon, "':'")?;

        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut init: Option<FuncDecl> = None;

        // Body: NEWLINE INDENT ... DEDENT, OR a single `pass` on the same line.
        self.skip_newlines();
        if matches!(self.peek_kind(), TokenKind::KwPass) {
            self.bump();
            self.expect_newline()?;
        } else {
            self.expect(&TokenKind::Indent, "indent")?;
            loop {
                self.skip_newlines();
                if matches!(self.peek_kind(), TokenKind::Dedent | TokenKind::Eof) {
                    break;
                }
                // Member: decorators? (fn ... | open fn ... | name : type [= expr])
                let mut member_decos = Vec::new();
                while matches!(self.peek_kind(), TokenKind::At) {
                    member_decos.push(self.parse_decorator()?);
                    self.skip_newlines();
                }
                // `open fn ...` (overridable method)
                let _open_method = self.eat(&TokenKind::KwOpen);
                if matches!(self.peek_kind(), TokenKind::KwFn) {
                    let f = self.parse_func_decl(member_decos)?;
                    if f.name == "__init__" {
                        if init.is_some() {
                            return Err(self.err(
                                "E0006: duplicate __init__ in class".into(),
                            ));
                        }
                        init = Some(f);
                    } else {
                        methods.push(f);
                    }
                } else {
                    if !member_decos.is_empty() {
                        return Err(self.err("decorator on field declaration".into()));
                    }
                    fields.push(self.parse_field_decl()?);
                }
            }
            self.expect(&TokenKind::Dedent, "dedent")?;
        }

        Ok(ClassDecl {
            name,
            modifier,
            generics,
            bases,
            fields,
            methods,
            init,
            span: merge_spans(start, self.prev_span()),
        })
    }

    fn parse_field_decl(&mut self) -> Result<FieldDecl, CompileError> {
        let start = self.cur_span();
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Colon, "':'")?;
        let ty = self.parse_type()?;
        let default = if self.eat(&TokenKind::Assign) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect_newline()?;
        Ok(FieldDecl {
            name,
            ty,
            default,
            span: merge_spans(start, self.prev_span()),
        })
    }

    fn parse_protocol_decl(&mut self) -> Result<ProtocolDecl, CompileError> {
        let start = self.cur_span();
        self.expect(&TokenKind::KwProtocol, "'protocol'")?;
        let name = self.expect_ident()?;
        let generics = if matches!(self.peek_kind(), TokenKind::LBracket) {
            self.parse_generic_params()?
        } else {
            Vec::new()
        };
        self.expect(&TokenKind::Colon, "':'")?;
        self.skip_newlines();
        let mut methods = Vec::new();
        if matches!(self.peek_kind(), TokenKind::KwPass) {
            self.bump();
            self.expect_newline()?;
        } else {
            self.expect(&TokenKind::Indent, "indent")?;
            loop {
                self.skip_newlines();
                if matches!(self.peek_kind(), TokenKind::Dedent | TokenKind::Eof) {
                    break;
                }
                methods.push(self.parse_proto_method()?);
            }
            self.expect(&TokenKind::Dedent, "dedent")?;
        }
        Ok(ProtocolDecl {
            name,
            generics,
            methods,
            span: merge_spans(start, self.prev_span()),
        })
    }

    fn parse_proto_method(&mut self) -> Result<ProtoMethod, CompileError> {
        let start = self.cur_span();
        self.expect(&TokenKind::KwFn, "'fn'")?;
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LParen, "'('")?;
        let params = self.parse_params(true)?;
        self.expect(&TokenKind::RParen, "')'")?;
        self.expect(&TokenKind::Arrow, "'->'")?;
        let return_ty = self.parse_type()?;
        self.expect_newline()?;
        Ok(ProtoMethod {
            name,
            params,
            return_ty,
            span: merge_spans(start, self.prev_span()),
        })
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Blocks & statements
    // ─────────────────────────────────────────────────────────────────────

    fn parse_block(&mut self) -> Result<Block, CompileError> {
        let start = self.cur_span();
        // block := NEWLINE INDENT { stmt } DEDENT
        // Tolerate the (illegal-per-spec but convenient) inline `pass` too.
        self.skip_newlines();
        let mut stmts = Vec::new();
        if matches!(self.peek_kind(), TokenKind::Indent) {
            self.bump();
            loop {
                self.skip_newlines();
                if matches!(self.peek_kind(), TokenKind::Dedent | TokenKind::Eof) {
                    break;
                }
                stmts.push(self.parse_stmt()?);
            }
            self.expect(&TokenKind::Dedent, "dedent")?;
        } else {
            // single-line block — parse one stmt
            stmts.push(self.parse_stmt()?);
        }
        Ok(Block {
            stmts,
            span: merge_spans(start, self.prev_span()),
        })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, CompileError> {
        match self.peek_kind() {
            TokenKind::KwIf => self.parse_if_stmt(),
            TokenKind::KwWhile => self.parse_while_stmt(),
            TokenKind::KwFor => self.parse_for_stmt(),
            TokenKind::KwMatch => self.parse_match_stmt(),
            TokenKind::KwTry => self.parse_try_stmt(),
            TokenKind::KwWith => self.parse_with_stmt(),
            _ => self.parse_simple_stmt(),
        }
    }

    fn parse_simple_stmt(&mut self) -> Result<Stmt, CompileError> {
        let start = self.cur_span();
        let stmt = match self.peek_kind().clone() {
            TokenKind::KwReturn => {
                self.bump();
                let value = if matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Eof | TokenKind::Dedent) {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                Stmt::Return {
                    value,
                    span: merge_spans(start, self.prev_span()),
                }
            }
            TokenKind::KwBreak => {
                self.bump();
                Stmt::Break {
                    span: merge_spans(start, self.prev_span()),
                }
            }
            TokenKind::KwContinue => {
                self.bump();
                Stmt::Continue {
                    span: merge_spans(start, self.prev_span()),
                }
            }
            TokenKind::KwPass => {
                self.bump();
                Stmt::Pass {
                    span: merge_spans(start, self.prev_span()),
                }
            }
            TokenKind::KwRaise => {
                self.bump();
                let exc = self.parse_expr()?;
                let cause = if self.eat(&TokenKind::KwFrom) {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                Stmt::Raise {
                    exc,
                    cause,
                    span: merge_spans(start, self.prev_span()),
                }
            }
            TokenKind::KwAssert => {
                self.bump();
                let cond = self.parse_expr()?;
                let msg = if self.eat(&TokenKind::Comma) {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                Stmt::Assert {
                    cond,
                    msg,
                    span: merge_spans(start, self.prev_span()),
                }
            }
            TokenKind::KwDel => {
                self.bump();
                let target = self.parse_lvalue_from_expr()?;
                Stmt::Del {
                    target,
                    span: merge_spans(start, self.prev_span()),
                }
            }
            // Let-stmt vs assign vs expr-stmt: decide with lookahead.
            // `IDENT : TYPE = EXPR` → Let.
            // `IDENT : TYPE , IDENT (: TYPE)? (...) = EXPR` → LetDestructure
            // (annotated form, possibly mixed).  Disambiguation happens after
            // parsing the first type: if a Comma follows where `=` was
            // expected, switch tracks.
            TokenKind::Ident(_)
                if matches!(self.peek_at(1), TokenKind::Colon) =>
            {
                let name = self.expect_ident()?;
                self.bump(); // colon
                let ty = self.parse_type()?;
                if matches!(self.peek_kind(), TokenKind::Comma) {
                    // Annotated tuple destructuring: `x: T1, y[: T2], ... = EXPR`.
                    // M14 tuples. See spec §5.X. The first (name, ty) is
                    // pushed; the rest are parsed in a loop. Each subsequent
                    // element may or may not carry an annotation.
                    let mut names = vec![name];
                    let mut tys: Vec<Option<Type>> = vec![Some(ty)];
                    while self.eat(&TokenKind::Comma) {
                        let n = self.expect_ident()?;
                        let t = if self.eat(&TokenKind::Colon) {
                            Some(self.parse_type()?)
                        } else {
                            None
                        };
                        names.push(n);
                        tys.push(t);
                    }
                    self.expect(&TokenKind::Assign, "'='")?;
                    let init = self.parse_expr()?;
                    Stmt::LetDestructure {
                        names,
                        tys,
                        init,
                        span: merge_spans(start, self.prev_span()),
                    }
                } else {
                    self.expect(&TokenKind::Assign, "'='")?;
                    let init = self.parse_expr()?;
                    Stmt::Let {
                        name,
                        ty,
                        init,
                        span: merge_spans(start, self.prev_span()),
                    }
                }
            }
            // `IDENT , IDENT (, IDENT)* = EXPR` → unannotated tuple destructure.
            // Per spec §5.X, types are inferred from the RHS tuple.
            TokenKind::Ident(_)
                if matches!(self.peek_at(1), TokenKind::Comma)
                    && matches!(self.peek_at(2), TokenKind::Ident(_)) =>
            {
                // Walk forward to confirm this is `IDENT (, IDENT)+ =`. If we
                // ever fail to see `IDENT` after a comma we fall through to
                // the generic expr parser (which would treat `a, b` as an
                // expression — currently a tuple literal). We re-check the
                // assignment by snapshotting the position.
                let snap = self.pos;
                let first = self.expect_ident()?;
                let mut names = vec![first];
                let mut ok = true;
                while self.eat(&TokenKind::Comma) {
                    if let TokenKind::Ident(_) = self.peek_kind() {
                        names.push(self.expect_ident()?);
                    } else {
                        ok = false;
                        break;
                    }
                }
                if ok && matches!(self.peek_kind(), TokenKind::Assign) {
                    self.bump(); // '='
                    let init = self.parse_expr()?;
                    let tys = vec![None; names.len()];
                    Stmt::LetDestructure {
                        names,
                        tys,
                        init,
                        span: merge_spans(start, self.prev_span()),
                    }
                } else {
                    // Roll back and fall through.
                    self.pos = snap;
                    let expr = self.parse_expr()?;
                    self.parse_assign_or_expr_tail(expr, start)?
                }
            }
            _ => {
                // expr, then maybe `=`/aug-op.
                let expr = self.parse_expr()?;
                self.parse_assign_or_expr_tail(expr, start)?
            }
        };
        self.expect_newline()?;
        Ok(stmt)
    }

    fn parse_assign_or_expr_tail(
        &mut self,
        expr: Expr,
        start: Span,
    ) -> Result<Stmt, CompileError> {
        let aug = match self.peek_kind() {
            TokenKind::Assign => None,
            TokenKind::PlusEq => Some(BinOp::Add),
            TokenKind::MinusEq => Some(BinOp::Sub),
            TokenKind::StarEq => Some(BinOp::Mul),
            TokenKind::SlashEq => Some(BinOp::Div),
            TokenKind::DoubleSlashEq => Some(BinOp::FloorDiv),
            TokenKind::PercentEq => Some(BinOp::Rem),
            TokenKind::DoubleStarEq => Some(BinOp::Pow),
            TokenKind::ShlEq => Some(BinOp::Shl),
            TokenKind::ShrEq => Some(BinOp::Shr),
            TokenKind::AmpEq => Some(BinOp::BitAnd),
            TokenKind::PipeEq => Some(BinOp::BitOr),
            TokenKind::CaretEq => Some(BinOp::BitXor),
            _ => {
                return Ok(Stmt::Expr {
                    span: merge_spans(start, self.prev_span()),
                    expr,
                });
            }
        };

        let target = expr_to_lvalue(&expr, self)?;
        self.bump(); // consume = or aug-op
        let value = self.parse_expr()?;
        let span = merge_spans(start, self.prev_span());
        if let Some(op) = aug {
            Ok(Stmt::AugAssign { target, op, value, span })
        } else {
            Ok(Stmt::Assign { target, value, span })
        }
    }

    fn parse_lvalue_from_expr(&mut self) -> Result<Lvalue, CompileError> {
        let expr = self.parse_expr()?;
        expr_to_lvalue(&expr, self)
    }

    fn parse_if_stmt(&mut self) -> Result<Stmt, CompileError> {
        let start = self.cur_span();
        self.expect(&TokenKind::KwIf, "'if'")?;
        let cond = self.parse_expr()?;
        self.expect(&TokenKind::Colon, "':'")?;
        let then_block = self.parse_block()?;
        let mut elifs = Vec::new();
        let mut else_block = None;
        loop {
            self.skip_newlines();
            if self.eat(&TokenKind::KwElif) {
                let c = self.parse_expr()?;
                self.expect(&TokenKind::Colon, "':'")?;
                let b = self.parse_block()?;
                elifs.push((c, b));
            } else if self.eat(&TokenKind::KwElse) {
                self.expect(&TokenKind::Colon, "':'")?;
                else_block = Some(self.parse_block()?);
                break;
            } else {
                break;
            }
        }
        Ok(Stmt::If {
            cond,
            then_block,
            elifs,
            else_block,
            span: merge_spans(start, self.prev_span()),
        })
    }

    fn parse_while_stmt(&mut self) -> Result<Stmt, CompileError> {
        let start = self.cur_span();
        self.expect(&TokenKind::KwWhile, "'while'")?;
        let cond = self.parse_expr()?;
        self.expect(&TokenKind::Colon, "':'")?;
        let body = self.parse_block()?;
        let mut else_block = None;
        self.skip_newlines();
        if self.eat(&TokenKind::KwElse) {
            self.expect(&TokenKind::Colon, "':'")?;
            else_block = Some(self.parse_block()?);
        }
        Ok(Stmt::While {
            cond,
            body,
            else_block,
            span: merge_spans(start, self.prev_span()),
        })
    }

    fn parse_for_stmt(&mut self) -> Result<Stmt, CompileError> {
        let start = self.cur_span();
        self.expect(&TokenKind::KwFor, "'for'")?;
        let var = self.expect_ident()?;
        self.expect(&TokenKind::Colon, "':'")?;
        let var_ty = self.parse_type()?;
        self.expect(&TokenKind::KwIn, "'in'")?;
        let iter = self.parse_expr()?;
        self.expect(&TokenKind::Colon, "':'")?;
        let body = self.parse_block()?;
        let mut else_block = None;
        self.skip_newlines();
        if self.eat(&TokenKind::KwElse) {
            self.expect(&TokenKind::Colon, "':'")?;
            else_block = Some(self.parse_block()?);
        }
        Ok(Stmt::For {
            var,
            var_ty,
            iter,
            body,
            else_block,
            span: merge_spans(start, self.prev_span()),
        })
    }

    fn parse_match_stmt(&mut self) -> Result<Stmt, CompileError> {
        let start = self.cur_span();
        self.expect(&TokenKind::KwMatch, "'match'")?;
        let scrutinee = self.parse_expr()?;
        self.expect(&TokenKind::Colon, "':'")?;
        self.skip_newlines();
        self.expect(&TokenKind::Indent, "indent")?;
        let mut arms = Vec::new();
        loop {
            self.skip_newlines();
            if matches!(self.peek_kind(), TokenKind::Dedent | TokenKind::Eof) {
                break;
            }
            arms.push(self.parse_match_arm()?);
        }
        self.expect(&TokenKind::Dedent, "dedent")?;
        Ok(Stmt::Match {
            scrutinee,
            arms,
            span: merge_spans(start, self.prev_span()),
        })
    }

    fn parse_match_arm(&mut self) -> Result<MatchArm, CompileError> {
        let start = self.cur_span();
        self.expect(&TokenKind::KwCase, "'case'")?;
        let pattern = self.parse_pattern()?;
        let guard = if self.eat(&TokenKind::KwIf) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect(&TokenKind::Colon, "':'")?;
        let body = self.parse_block()?;
        Ok(MatchArm {
            pattern,
            guard,
            body,
            span: merge_spans(start, self.prev_span()),
        })
    }

    fn parse_pattern(&mut self) -> Result<Pattern, CompileError> {
        let start = self.cur_span();
        match self.peek_kind().clone() {
            TokenKind::Ident(s) if s == "_" => {
                self.bump();
                Ok(Pattern::Wildcard(merge_spans(start, self.prev_span())))
            }
            TokenKind::IntLit { .. }
            | TokenKind::FloatLit { .. }
            | TokenKind::StrLit(_)
            | TokenKind::CharLit(_)
            | TokenKind::BytesLit(_)
            | TokenKind::KwTrue
            | TokenKind::KwFalse
            | TokenKind::KwNone => {
                let lit = self.parse_literal()?;
                Ok(Pattern::Literal(lit, merge_spans(start, self.prev_span())))
            }
            TokenKind::LParen => {
                self.bump();
                let first = self.parse_pattern()?;
                if !self.eat(&TokenKind::Comma) {
                    return Err(self.err(
                        "tuple pattern requires at least 2 elements; use bare pattern instead"
                            .into(),
                    ));
                }
                let mut elems = vec![first];
                if !matches!(self.peek_kind(), TokenKind::RParen) {
                    elems.push(self.parse_pattern()?);
                    while self.eat(&TokenKind::Comma) {
                        if matches!(self.peek_kind(), TokenKind::RParen) {
                            break;
                        }
                        elems.push(self.parse_pattern()?);
                    }
                }
                self.expect(&TokenKind::RParen, "')'")?;
                Ok(Pattern::Tuple(elems, merge_spans(start, self.prev_span())))
            }
            TokenKind::Ident(_) => {
                // Could be identifier pattern OR constructor pattern.
                // Look ahead: if `(` follows, it's a constructor; if `[` it's
                // generic-typed constructor; if `.` it's a dotted type.
                // Disambiguate by reading dotted_name first.
                let saved = self.pos;
                let _ = self.expect_ident();
                let mut is_ctor = false;
                while matches!(self.peek_kind(), TokenKind::Dot) {
                    self.bump();
                    if !matches!(self.peek_kind(), TokenKind::Ident(_)) {
                        break;
                    }
                    self.bump();
                    is_ctor = true; // dotted names are types
                }
                if matches!(self.peek_kind(), TokenKind::LBracket | TokenKind::LParen) {
                    is_ctor = true;
                }
                self.pos = saved;
                if is_ctor {
                    let ty = self.parse_type()?;
                    self.expect(&TokenKind::LParen, "'('")?;
                    let mut fields = Vec::new();
                    if !matches!(self.peek_kind(), TokenKind::RParen) {
                        fields.push(self.parse_pattern()?);
                        while self.eat(&TokenKind::Comma) {
                            if matches!(self.peek_kind(), TokenKind::RParen) {
                                break;
                            }
                            fields.push(self.parse_pattern()?);
                        }
                    }
                    self.expect(&TokenKind::RParen, "')'")?;
                    Ok(Pattern::Constructor {
                        ty,
                        fields,
                        span: merge_spans(start, self.prev_span()),
                    })
                } else {
                    let name = self.expect_ident()?;
                    Ok(Pattern::Identifier(name, merge_spans(start, self.prev_span())))
                }
            }
            other => Err(self.err(format!("expected pattern, found {:?}", other))),
        }
    }

    fn parse_try_stmt(&mut self) -> Result<Stmt, CompileError> {
        let start = self.cur_span();
        self.expect(&TokenKind::KwTry, "'try'")?;
        self.expect(&TokenKind::Colon, "':'")?;
        let body = self.parse_block()?;
        let mut handlers = Vec::new();
        loop {
            self.skip_newlines();
            if !matches!(self.peek_kind(), TokenKind::KwExcept) {
                break;
            }
            handlers.push(self.parse_except_handler()?);
        }
        let mut else_block = None;
        self.skip_newlines();
        if self.eat(&TokenKind::KwElse) {
            self.expect(&TokenKind::Colon, "':'")?;
            else_block = Some(self.parse_block()?);
        }
        let mut finally_block = None;
        self.skip_newlines();
        if self.eat(&TokenKind::KwFinally) {
            self.expect(&TokenKind::Colon, "':'")?;
            finally_block = Some(self.parse_block()?);
        }
        Ok(Stmt::Try {
            body,
            handlers,
            else_block,
            finally_block,
            span: merge_spans(start, self.prev_span()),
        })
    }

    fn parse_except_handler(&mut self) -> Result<ExceptHandler, CompileError> {
        let start = self.cur_span();
        self.expect(&TokenKind::KwExcept, "'except'")?;
        let exc_ty = self.parse_type()?;
        let binding = if self.eat(&TokenKind::KwAs) {
            Some(self.expect_ident()?)
        } else {
            None
        };
        self.expect(&TokenKind::Colon, "':'")?;
        let body = self.parse_block()?;
        Ok(ExceptHandler {
            exc_ty,
            binding,
            body,
            span: merge_spans(start, self.prev_span()),
        })
    }

    fn parse_with_stmt(&mut self) -> Result<Stmt, CompileError> {
        let start = self.cur_span();
        self.expect(&TokenKind::KwWith, "'with'")?;
        let expr = self.parse_expr()?;
        let binding = if self.eat(&TokenKind::KwAs) {
            let name = self.expect_ident()?;
            self.expect(&TokenKind::Colon, "':'")?;
            let ty = self.parse_type()?;
            Some((name, ty))
        } else {
            None
        };
        self.expect(&TokenKind::Colon, "':'")?;
        let body = self.parse_block()?;
        Ok(Stmt::With {
            expr,
            binding,
            body,
            span: merge_spans(start, self.prev_span()),
        })
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Expressions (Pratt-ish recursive descent)
    // ─────────────────────────────────────────────────────────────────────

    fn parse_expr(&mut self) -> Result<Expr, CompileError> {
        let start = self.cur_span();
        let lhs = self.parse_or_expr()?;
        // ternary tail: `if cond else expr`
        if self.eat(&TokenKind::KwIf) {
            let cond = self.parse_or_expr()?;
            self.expect(&TokenKind::KwElse, "'else'")?;
            let else_expr = self.parse_expr()?; // right-assoc
            Ok(Expr::Ternary {
                cond: Box::new(cond),
                then_expr: Box::new(lhs),
                else_expr: Box::new(else_expr),
                span: merge_spans(start, self.prev_span()),
            })
        } else {
            Ok(lhs)
        }
    }

    fn parse_or_expr(&mut self) -> Result<Expr, CompileError> {
        let start = self.cur_span();
        let mut lhs = self.parse_and_expr()?;
        while self.eat(&TokenKind::KwOr) {
            let rhs = self.parse_and_expr()?;
            lhs = Expr::Binary {
                op: BinOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: merge_spans(start, self.prev_span()),
            };
        }
        Ok(lhs)
    }

    fn parse_and_expr(&mut self) -> Result<Expr, CompileError> {
        let start = self.cur_span();
        let mut lhs = self.parse_not_expr()?;
        while self.eat(&TokenKind::KwAnd) {
            let rhs = self.parse_not_expr()?;
            lhs = Expr::Binary {
                op: BinOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: merge_spans(start, self.prev_span()),
            };
        }
        Ok(lhs)
    }

    fn parse_not_expr(&mut self) -> Result<Expr, CompileError> {
        let start = self.cur_span();
        if self.eat(&TokenKind::KwNot) {
            let operand = self.parse_not_expr()?;
            Ok(Expr::Unary {
                op: UnaryOp::Not,
                operand: Box::new(operand),
                span: merge_spans(start, self.prev_span()),
            })
        } else {
            self.parse_comparison()
        }
    }

    fn parse_comparison(&mut self) -> Result<Expr, CompileError> {
        let start = self.cur_span();
        let lhs = self.parse_bit_or()?;
        // Look for a single comp_op (no chaining in v0.1).
        if let Some(op) = self.peek_comp_op() {
            self.consume_comp_op();
            let rhs = self.parse_bit_or()?;
            // Reject chaining: if another comp_op follows, error.
            if self.peek_comp_op().is_some() {
                return Err(self.err(
                    "E0007: chained comparison is not allowed in v0.1 (e.g. `a < b < c`)"
                        .into(),
                ));
            }
            Ok(Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: merge_spans(start, self.prev_span()),
            })
        } else {
            Ok(lhs)
        }
    }

    fn peek_comp_op(&self) -> Option<BinOp> {
        match self.peek_kind() {
            TokenKind::EqEq => Some(BinOp::Eq),
            TokenKind::NotEq => Some(BinOp::Ne),
            TokenKind::Lt => Some(BinOp::Lt),
            TokenKind::Gt => Some(BinOp::Gt),
            TokenKind::LtEq => Some(BinOp::Le),
            TokenKind::GtEq => Some(BinOp::Ge),
            TokenKind::KwIs => {
                if matches!(self.peek_at(1), TokenKind::KwNot) {
                    Some(BinOp::IsNot)
                } else {
                    Some(BinOp::Is)
                }
            }
            TokenKind::KwIn => Some(BinOp::In),
            TokenKind::KwNot => {
                if matches!(self.peek_at(1), TokenKind::KwIn) {
                    Some(BinOp::NotIn)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn consume_comp_op(&mut self) {
        match self.peek_kind() {
            TokenKind::KwIs => {
                self.bump();
                if matches!(self.peek_kind(), TokenKind::KwNot) {
                    self.bump();
                }
            }
            TokenKind::KwNot => {
                self.bump();
                if matches!(self.peek_kind(), TokenKind::KwIn) {
                    self.bump();
                }
            }
            _ => {
                self.bump();
            }
        }
    }

    fn parse_bit_or(&mut self) -> Result<Expr, CompileError> {
        let start = self.cur_span();
        let mut lhs = self.parse_bit_xor()?;
        while self.eat(&TokenKind::Pipe) {
            let rhs = self.parse_bit_xor()?;
            lhs = Expr::Binary {
                op: BinOp::BitOr,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: merge_spans(start, self.prev_span()),
            };
        }
        Ok(lhs)
    }

    fn parse_bit_xor(&mut self) -> Result<Expr, CompileError> {
        let start = self.cur_span();
        let mut lhs = self.parse_bit_and()?;
        while self.eat(&TokenKind::Caret) {
            let rhs = self.parse_bit_and()?;
            lhs = Expr::Binary {
                op: BinOp::BitXor,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: merge_spans(start, self.prev_span()),
            };
        }
        Ok(lhs)
    }

    fn parse_bit_and(&mut self) -> Result<Expr, CompileError> {
        let start = self.cur_span();
        let mut lhs = self.parse_shift()?;
        while self.eat(&TokenKind::Amp) {
            let rhs = self.parse_shift()?;
            lhs = Expr::Binary {
                op: BinOp::BitAnd,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: merge_spans(start, self.prev_span()),
            };
        }
        Ok(lhs)
    }

    fn parse_shift(&mut self) -> Result<Expr, CompileError> {
        let start = self.cur_span();
        let mut lhs = self.parse_addition()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Shl => BinOp::Shl,
                TokenKind::Shr => BinOp::Shr,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_addition()?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: merge_spans(start, self.prev_span()),
            };
        }
        Ok(lhs)
    }

    fn parse_addition(&mut self) -> Result<Expr, CompileError> {
        let start = self.cur_span();
        let mut lhs = self.parse_multiplication()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_multiplication()?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: merge_spans(start, self.prev_span()),
            };
        }
        Ok(lhs)
    }

    fn parse_multiplication(&mut self) -> Result<Expr, CompileError> {
        let start = self.cur_span();
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::DoubleSlash => BinOp::FloorDiv,
                TokenKind::Percent => BinOp::Rem,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_unary()?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: merge_spans(start, self.prev_span()),
            };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, CompileError> {
        let start = self.cur_span();
        let op = match self.peek_kind() {
            TokenKind::Plus => Some(UnaryOp::Pos),
            TokenKind::Minus => Some(UnaryOp::Neg),
            TokenKind::Tilde => Some(UnaryOp::BitNot),
            _ => None,
        };
        if let Some(op) = op {
            self.bump();
            let operand = self.parse_unary()?;
            Ok(Expr::Unary {
                op,
                operand: Box::new(operand),
                span: merge_spans(start, self.prev_span()),
            })
        } else {
            self.parse_power()
        }
    }

    fn parse_power(&mut self) -> Result<Expr, CompileError> {
        let start = self.cur_span();
        let lhs = self.parse_postfix()?;
        if self.eat(&TokenKind::DoubleStar) {
            // right-associative: rhs uses unary (which recurses to power)
            let rhs = self.parse_unary()?;
            Ok(Expr::Binary {
                op: BinOp::Pow,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: merge_spans(start, self.prev_span()),
            })
        } else {
            Ok(lhs)
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, CompileError> {
        let start = self.cur_span();
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek_kind() {
                TokenKind::LParen => {
                    self.bump();
                    let args = if matches!(self.peek_kind(), TokenKind::RParen) {
                        Vec::new()
                    } else {
                        self.parse_arg_list()?
                    };
                    self.expect(&TokenKind::RParen, "')'")?;
                    let span = merge_spans(start, self.prev_span());
                    // Method-call shortcut: if the callee is an Attr, fold it.
                    expr = match expr {
                        Expr::Attr { obj, name, .. } => Expr::MethodCall {
                            receiver: obj,
                            method: name,
                            args,
                            span,
                        },
                        other => Expr::Call {
                            callee: Box::new(other),
                            args,
                            span,
                        },
                    };
                }
                TokenKind::Dot => {
                    self.bump();
                    // M14 tuples: allow `t.0`, `t.1`, ... — a bare integer
                    // literal after `.` is a tuple field index. We materialize
                    // it as Expr::Attr with `name = "<digit>"` so downstream
                    // typecheck/IR can dispatch on Ty::Tuple. Negative or
                    // non-decimal indices aren't legal — `IntLit` already
                    // excludes the sign (handled by unary minus) and the
                    // lexer doesn't emit hex here because `.0xff` wouldn't
                    // parse as a number after `.`.
                    let name = match self.peek_kind().clone() {
                        TokenKind::IntLit { value, .. } => {
                            self.bump();
                            value.to_string()
                        }
                        _ => self.expect_ident()?,
                    };
                    let span = merge_spans(start, self.prev_span());
                    expr = Expr::Attr {
                        obj: Box::new(expr),
                        name,
                        span,
                    };
                }
                TokenKind::LBracket => {
                    self.bump();
                    let mut indices = vec![self.parse_expr()?];
                    while self.eat(&TokenKind::Comma) {
                        if matches!(self.peek_kind(), TokenKind::RBracket) {
                            break;
                        }
                        indices.push(self.parse_expr()?);
                    }
                    self.expect(&TokenKind::RBracket, "']'")?;
                    let span = merge_spans(start, self.prev_span());
                    expr = Expr::Index {
                        obj: Box::new(expr),
                        indices,
                        span,
                    };
                }
                TokenKind::QuestionQuestion => {
                    self.bump();
                    let rhs = self.parse_unary()?;
                    let span = merge_spans(start, self.prev_span());
                    expr = Expr::NullCoalesce {
                        lhs: Box::new(expr),
                        rhs: Box::new(rhs),
                        span,
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_arg_list(&mut self) -> Result<Vec<Arg>, CompileError> {
        let mut args = Vec::new();
        loop {
            let start = self.cur_span();
            // named arg: IDENT = expr
            let name = if let TokenKind::Ident(_) = self.peek_kind() {
                if matches!(self.peek_at(1), TokenKind::Assign) {
                    let n = self.expect_ident()?;
                    self.bump(); // =
                    Some(n)
                } else {
                    None
                }
            } else {
                None
            };
            let value = self.parse_expr()?;
            args.push(Arg {
                name,
                value,
                span: merge_spans(start, self.prev_span()),
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
            if matches!(self.peek_kind(), TokenKind::RParen) {
                break;
            }
        }
        Ok(args)
    }

    fn parse_primary(&mut self) -> Result<Expr, CompileError> {
        let start = self.cur_span();
        match self.peek_kind().clone() {
            TokenKind::IntLit { .. }
            | TokenKind::FloatLit { .. }
            | TokenKind::StrLit(_)
            | TokenKind::RawStrLit(_)
            | TokenKind::CharLit(_)
            | TokenKind::BytesLit(_)
            | TokenKind::KwTrue
            | TokenKind::KwFalse
            | TokenKind::KwNone => {
                let lit = self.parse_literal()?;
                Ok(Expr::Literal {
                    lit,
                    span: merge_spans(start, self.prev_span()),
                })
            }
            TokenKind::Ident(name) => {
                self.bump();
                Ok(Expr::Ident {
                    name,
                    span: merge_spans(start, self.prev_span()),
                })
            }
            // `open` is reserved as a class modifier but is also the name of
            // the builtin `io.open` function; accept it as an identifier in
            // expression context. (Likewise for `final`/`sealed` would only
            // appear at decl position, so we don't need to special-case them.)
            TokenKind::KwOpen => {
                self.bump();
                Ok(Expr::Ident {
                    name: "open".into(),
                    span: merge_spans(start, self.prev_span()),
                })
            }
            TokenKind::LParen => self.parse_paren_or_tuple(start),
            TokenKind::LBracket => self.parse_list_literal(start),
            TokenKind::LBrace => self.parse_dict_or_set_literal(start),
            TokenKind::KwFn => self.parse_lambda(start),
            other => Err(self.err(format!("expected expression, found {:?}", other))),
        }
    }

    fn parse_literal(&mut self) -> Result<Literal, CompileError> {
        match self.peek_kind().clone() {
            TokenKind::IntLit { value, suffix } => {
                self.bump();
                Ok(Literal::Int { value, suffix })
            }
            TokenKind::FloatLit { value, suffix } => {
                self.bump();
                Ok(Literal::Float { value, suffix })
            }
            TokenKind::StrLit(s) | TokenKind::RawStrLit(s) => {
                self.bump();
                Ok(Literal::Str(s))
            }
            TokenKind::CharLit(c) => {
                self.bump();
                Ok(Literal::Char(c))
            }
            TokenKind::BytesLit(b) => {
                self.bump();
                Ok(Literal::Bytes(b))
            }
            TokenKind::KwTrue => {
                self.bump();
                Ok(Literal::Bool(true))
            }
            TokenKind::KwFalse => {
                self.bump();
                Ok(Literal::Bool(false))
            }
            TokenKind::KwNone => {
                self.bump();
                Ok(Literal::None)
            }
            other => Err(self.err(format!("expected literal, found {:?}", other))),
        }
    }

    fn parse_paren_or_tuple(&mut self, start: Span) -> Result<Expr, CompileError> {
        self.expect(&TokenKind::LParen, "'('")?;
        // Empty tuple: `()`
        if self.eat(&TokenKind::RParen) {
            return Ok(Expr::Tuple {
                elems: Vec::new(),
                span: merge_spans(start, self.prev_span()),
            });
        }
        let first = self.parse_expr()?;
        if self.eat(&TokenKind::Comma) {
            // 1-tuple `(x,)` or n-tuple `(x, y, ...)`
            let mut elems = vec![first];
            if !matches!(self.peek_kind(), TokenKind::RParen) {
                elems.push(self.parse_expr()?);
                while self.eat(&TokenKind::Comma) {
                    if matches!(self.peek_kind(), TokenKind::RParen) {
                        break;
                    }
                    elems.push(self.parse_expr()?);
                }
            }
            self.expect(&TokenKind::RParen, "')'")?;
            Ok(Expr::Tuple {
                elems,
                span: merge_spans(start, self.prev_span()),
            })
        } else {
            self.expect(&TokenKind::RParen, "')'")?;
            Ok(first)
        }
    }

    fn parse_list_literal(&mut self, start: Span) -> Result<Expr, CompileError> {
        self.expect(&TokenKind::LBracket, "'['")?;
        let mut elems = Vec::new();
        if !matches!(self.peek_kind(), TokenKind::RBracket) {
            elems.push(self.parse_expr()?);
            while self.eat(&TokenKind::Comma) {
                if matches!(self.peek_kind(), TokenKind::RBracket) {
                    break;
                }
                elems.push(self.parse_expr()?);
            }
        }
        self.expect(&TokenKind::RBracket, "']'")?;
        Ok(Expr::List {
            elems,
            span: merge_spans(start, self.prev_span()),
        })
    }

    fn parse_dict_or_set_literal(&mut self, start: Span) -> Result<Expr, CompileError> {
        self.expect(&TokenKind::LBrace, "'{'")?;
        // Empty `{}` is a dict by convention.
        if self.eat(&TokenKind::RBrace) {
            return Ok(Expr::Dict {
                entries: Vec::new(),
                span: merge_spans(start, self.prev_span()),
            });
        }
        let first = self.parse_expr()?;
        if self.eat(&TokenKind::Colon) {
            // Dict
            let v = self.parse_expr()?;
            let mut entries = vec![(first, v)];
            while self.eat(&TokenKind::Comma) {
                if matches!(self.peek_kind(), TokenKind::RBrace) {
                    break;
                }
                let k = self.parse_expr()?;
                self.expect(&TokenKind::Colon, "':'")?;
                let v = self.parse_expr()?;
                entries.push((k, v));
            }
            self.expect(&TokenKind::RBrace, "'}'")?;
            Ok(Expr::Dict {
                entries,
                span: merge_spans(start, self.prev_span()),
            })
        } else {
            let mut elems = vec![first];
            while self.eat(&TokenKind::Comma) {
                if matches!(self.peek_kind(), TokenKind::RBrace) {
                    break;
                }
                elems.push(self.parse_expr()?);
            }
            self.expect(&TokenKind::RBrace, "'}'")?;
            Ok(Expr::Set {
                elems,
                span: merge_spans(start, self.prev_span()),
            })
        }
    }

    fn parse_lambda(&mut self, start: Span) -> Result<Expr, CompileError> {
        self.expect(&TokenKind::KwFn, "'fn'")?;
        self.expect(&TokenKind::LParen, "'('")?;
        let params = self.parse_params(false)?;
        self.expect(&TokenKind::RParen, "')'")?;
        self.expect(&TokenKind::Arrow, "'->'")?;
        let return_ty = self.parse_type()?;
        self.expect(&TokenKind::Colon, "':'")?;
        let body = self.parse_expr()?;
        Ok(Expr::Lambda {
            params,
            return_ty,
            body: Box::new(body),
            span: merge_spans(start, self.prev_span()),
        })
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Types
    // ─────────────────────────────────────────────────────────────────────

    fn parse_type(&mut self) -> Result<Type, CompileError> {
        let start = self.cur_span();
        let mut ty = self.parse_type_atom(start)?;
        // Nullable suffix `?`
        if self.eat(&TokenKind::Question) {
            ty = Type::Nullable {
                inner: Box::new(ty),
                span: merge_spans(start, self.prev_span()),
            };
        }
        Ok(ty)
    }

    fn parse_type_atom(&mut self, start: Span) -> Result<Type, CompileError> {
        match self.peek_kind().clone() {
            TokenKind::KwFn => {
                self.bump();
                self.expect(&TokenKind::LParen, "'('")?;
                let mut params = Vec::new();
                if !matches!(self.peek_kind(), TokenKind::RParen) {
                    params.push(self.parse_type()?);
                    while self.eat(&TokenKind::Comma) {
                        if matches!(self.peek_kind(), TokenKind::RParen) {
                            break;
                        }
                        params.push(self.parse_type()?);
                    }
                }
                self.expect(&TokenKind::RParen, "')'")?;
                self.expect(&TokenKind::Arrow, "'->'")?;
                let ret = self.parse_type()?;
                Ok(Type::Function {
                    params,
                    ret: Box::new(ret),
                    span: merge_spans(start, self.prev_span()),
                })
            }
            TokenKind::LParen => {
                // Parenthesized tuple-type: `(T1, T2, ...)`
                self.bump();
                let first = self.parse_type()?;
                if !self.eat(&TokenKind::Comma) {
                    self.expect(&TokenKind::RParen, "')'")?;
                    return Ok(first);
                }
                let mut elems = vec![first];
                if !matches!(self.peek_kind(), TokenKind::RParen) {
                    elems.push(self.parse_type()?);
                    while self.eat(&TokenKind::Comma) {
                        if matches!(self.peek_kind(), TokenKind::RParen) {
                            break;
                        }
                        elems.push(self.parse_type()?);
                    }
                }
                self.expect(&TokenKind::RParen, "')'")?;
                Ok(Type::Tuple {
                    elems,
                    span: merge_spans(start, self.prev_span()),
                })
            }
            TokenKind::Ident(_) => {
                let mut parts = vec![self.expect_ident()?];
                while matches!(self.peek_kind(), TokenKind::Dot) {
                    self.bump();
                    parts.push(self.expect_ident()?);
                }
                let name = parts.join(".");
                let mut args = Vec::new();
                if self.eat(&TokenKind::LBracket) {
                    if !matches!(self.peek_kind(), TokenKind::RBracket) {
                        args.push(self.parse_type()?);
                        while self.eat(&TokenKind::Comma) {
                            if matches!(self.peek_kind(), TokenKind::RBracket) {
                                break;
                            }
                            args.push(self.parse_type()?);
                        }
                    }
                    self.expect(&TokenKind::RBracket, "']'")?;
                }
                Ok(Type::Named {
                    name,
                    args,
                    span: merge_spans(start, self.prev_span()),
                })
            }
            other => Err(self.err(format!("expected type, found {:?}", other))),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Helpers
// ─────────────────────────────────────────────────────────────────────────

fn merge_spans(a: Span, b: Span) -> Span {
    Span {
        start: a.start.min(b.start),
        end: a.end.max(b.end),
        line: a.line,
        col: a.col,
    }
}

fn expr_to_lvalue(expr: &Expr, parser: &Parser) -> Result<Lvalue, CompileError> {
    match expr {
        Expr::Ident { name, span } => Ok(Lvalue::Ident {
            name: name.clone(),
            span: *span,
        }),
        Expr::Attr { obj, name, span } => Ok(Lvalue::Attr {
            obj: obj.clone(),
            name: name.clone(),
            span: *span,
        }),
        Expr::Index { obj, indices, span } => Ok(Lvalue::Index {
            obj: obj.clone(),
            indices: indices.clone(),
            span: *span,
        }),
        _ => Err(parser.err("invalid assignment target".into())),
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::{FloatSuffix, IntSuffix};

    fn tok(kind: TokenKind) -> Token {
        Token { kind, span: Span::DUMMY }
    }

    fn id(s: &str) -> Token {
        tok(TokenKind::Ident(s.into()))
    }

    fn int(v: i128) -> Token {
        tok(TokenKind::IntLit { value: v, suffix: None })
    }

    fn int_s(v: i128, s: IntSuffix) -> Token {
        tok(TokenKind::IntLit { value: v, suffix: Some(s) })
    }

    fn float(v: f64) -> Token {
        tok(TokenKind::FloatLit { value: v, suffix: None })
    }

    fn nl() -> Token { tok(TokenKind::Newline) }
    fn ind() -> Token { tok(TokenKind::Indent) }
    fn ded() -> Token { tok(TokenKind::Dedent) }
    fn eof() -> Token { tok(TokenKind::Eof) }

    fn parse(tokens: Vec<Token>) -> Result<Module, CompileError> {
        Parser::new(tokens).parse_module()
    }

    #[test]
    fn test_empty_module() {
        let m = parse(vec![eof()]).unwrap();
        assert!(m.decls.is_empty());
        assert!(m.imports.is_empty());
    }

    /// Build the token sequence for `fn f(x: i32) -> i32: <NL><INDENT>return x<NL><DEDENT>`
    fn tokens_simple_function() -> Vec<Token> {
        vec![
            tok(TokenKind::KwFn),
            id("f"),
            tok(TokenKind::LParen),
            id("x"),
            tok(TokenKind::Colon),
            id("i32"),
            tok(TokenKind::RParen),
            tok(TokenKind::Arrow),
            id("i32"),
            tok(TokenKind::Colon),
            nl(),
            ind(),
            tok(TokenKind::KwReturn),
            id("x"),
            nl(),
            ded(),
            eof(),
        ]
    }

    #[test]
    fn test_simple_function() {
        let m = parse(tokens_simple_function()).unwrap();
        assert_eq!(m.decls.len(), 1);
        match &m.decls[0] {
            TopDecl::Func(f) => {
                assert_eq!(f.name, "f");
                assert_eq!(f.params.len(), 1);
                assert_eq!(f.params[0].name, "x");
                assert_eq!(f.body.stmts.len(), 1);
                match &f.body.stmts[0] {
                    Stmt::Return { value: Some(_), .. } => {}
                    other => panic!("expected Return, got {:?}", other),
                }
            }
            _ => panic!("expected func"),
        }
    }

    #[test]
    fn test_class_with_field_and_method() {
        // class Point: x: i32 <NL> fn get(self) -> i32: return self.x
        let toks = vec![
            tok(TokenKind::KwClass), id("Point"), tok(TokenKind::Colon), nl(),
            ind(),
                id("x"), tok(TokenKind::Colon), id("i32"), nl(),
                tok(TokenKind::KwFn), id("get"),
                tok(TokenKind::LParen), id("self"), tok(TokenKind::RParen),
                tok(TokenKind::Arrow), id("i32"), tok(TokenKind::Colon), nl(),
                ind(),
                    tok(TokenKind::KwReturn), id("self"), tok(TokenKind::Dot), id("x"), nl(),
                ded(),
            ded(),
            eof(),
        ];
        let m = parse(toks).unwrap();
        match &m.decls[0] {
            TopDecl::Class(c) => {
                assert_eq!(c.name, "Point");
                assert_eq!(c.fields.len(), 1);
                assert_eq!(c.methods.len(), 1);
                assert_eq!(c.fields[0].name, "x");
                assert_eq!(c.methods[0].name, "get");
            }
            _ => panic!("expected class"),
        }
    }

    #[test]
    fn test_protocol() {
        // protocol Hashable: fn hash(self) -> i64
        let toks = vec![
            tok(TokenKind::KwProtocol), id("Hashable"), tok(TokenKind::Colon), nl(),
            ind(),
                tok(TokenKind::KwFn), id("hash"),
                tok(TokenKind::LParen), id("self"), tok(TokenKind::RParen),
                tok(TokenKind::Arrow), id("i64"), nl(),
            ded(),
            eof(),
        ];
        let m = parse(toks).unwrap();
        match &m.decls[0] {
            TopDecl::Protocol(p) => {
                assert_eq!(p.name, "Hashable");
                assert_eq!(p.methods.len(), 1);
                assert_eq!(p.methods[0].name, "hash");
            }
            _ => panic!("expected protocol"),
        }
    }

    #[test]
    fn test_if_elif_else() {
        // fn f() -> i32: if true: return 1
        //                elif false: return 2
        //                else: return 3
        let toks = vec![
            tok(TokenKind::KwFn), id("f"),
            tok(TokenKind::LParen), tok(TokenKind::RParen),
            tok(TokenKind::Arrow), id("i32"), tok(TokenKind::Colon), nl(),
            ind(),
                tok(TokenKind::KwIf), tok(TokenKind::KwTrue), tok(TokenKind::Colon), nl(),
                ind(), tok(TokenKind::KwReturn), int(1), nl(), ded(),
                tok(TokenKind::KwElif), tok(TokenKind::KwFalse), tok(TokenKind::Colon), nl(),
                ind(), tok(TokenKind::KwReturn), int(2), nl(), ded(),
                tok(TokenKind::KwElse), tok(TokenKind::Colon), nl(),
                ind(), tok(TokenKind::KwReturn), int(3), nl(), ded(),
            ded(),
            eof(),
        ];
        let m = parse(toks).unwrap();
        match &m.decls[0] {
            TopDecl::Func(f) => match &f.body.stmts[0] {
                Stmt::If { elifs, else_block, .. } => {
                    assert_eq!(elifs.len(), 1);
                    assert!(else_block.is_some());
                }
                other => panic!("expected If, got {:?}", other),
            },
            _ => panic!("expected func"),
        }
    }

    #[test]
    fn test_while_loop() {
        let toks = vec![
            tok(TokenKind::KwFn), id("f"),
            tok(TokenKind::LParen), tok(TokenKind::RParen),
            tok(TokenKind::Arrow), id("None"), tok(TokenKind::Colon), nl(),
            ind(),
                tok(TokenKind::KwWhile), tok(TokenKind::KwTrue), tok(TokenKind::Colon), nl(),
                ind(), tok(TokenKind::KwBreak), nl(), ded(),
            ded(),
            eof(),
        ];
        let m = parse(toks).unwrap();
        match &m.decls[0] {
            TopDecl::Func(f) => {
                assert!(matches!(f.body.stmts[0], Stmt::While { .. }));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn test_for_loop() {
        // fn f() -> None: for x: i32 in xs: pass
        let toks = vec![
            tok(TokenKind::KwFn), id("f"),
            tok(TokenKind::LParen), tok(TokenKind::RParen),
            tok(TokenKind::Arrow), id("None"), tok(TokenKind::Colon), nl(),
            ind(),
                tok(TokenKind::KwFor), id("x"), tok(TokenKind::Colon), id("i32"),
                tok(TokenKind::KwIn), id("xs"), tok(TokenKind::Colon), nl(),
                ind(), tok(TokenKind::KwPass), nl(), ded(),
            ded(),
            eof(),
        ];
        let m = parse(toks).unwrap();
        match &m.decls[0] {
            TopDecl::Func(f) => match &f.body.stmts[0] {
                Stmt::For { var, .. } => assert_eq!(var, "x"),
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn test_match_with_patterns() {
        // fn f() -> None: match x:
        //   case 0: pass
        //   case _: pass
        let toks = vec![
            tok(TokenKind::KwFn), id("f"),
            tok(TokenKind::LParen), tok(TokenKind::RParen),
            tok(TokenKind::Arrow), id("None"), tok(TokenKind::Colon), nl(),
            ind(),
                tok(TokenKind::KwMatch), id("x"), tok(TokenKind::Colon), nl(),
                ind(),
                    tok(TokenKind::KwCase), int(0), tok(TokenKind::Colon), nl(),
                    ind(), tok(TokenKind::KwPass), nl(), ded(),
                    tok(TokenKind::KwCase), id("_"), tok(TokenKind::Colon), nl(),
                    ind(), tok(TokenKind::KwPass), nl(), ded(),
                ded(),
            ded(),
            eof(),
        ];
        let m = parse(toks).unwrap();
        match &m.decls[0] {
            TopDecl::Func(f) => match &f.body.stmts[0] {
                Stmt::Match { arms, .. } => {
                    assert_eq!(arms.len(), 2);
                    assert!(matches!(arms[0].pattern, Pattern::Literal(..)));
                    assert!(matches!(arms[1].pattern, Pattern::Wildcard(..)));
                }
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn test_try_except_finally() {
        let toks = vec![
            tok(TokenKind::KwFn), id("f"),
            tok(TokenKind::LParen), tok(TokenKind::RParen),
            tok(TokenKind::Arrow), id("None"), tok(TokenKind::Colon), nl(),
            ind(),
                tok(TokenKind::KwTry), tok(TokenKind::Colon), nl(),
                ind(), tok(TokenKind::KwPass), nl(), ded(),
                tok(TokenKind::KwExcept), id("ValueError"), tok(TokenKind::KwAs), id("e"),
                tok(TokenKind::Colon), nl(),
                ind(), tok(TokenKind::KwPass), nl(), ded(),
                tok(TokenKind::KwFinally), tok(TokenKind::Colon), nl(),
                ind(), tok(TokenKind::KwPass), nl(), ded(),
            ded(),
            eof(),
        ];
        let m = parse(toks).unwrap();
        match &m.decls[0] {
            TopDecl::Func(f) => match &f.body.stmts[0] {
                Stmt::Try { handlers, finally_block, .. } => {
                    assert_eq!(handlers.len(), 1);
                    assert_eq!(handlers[0].binding.as_deref(), Some("e"));
                    assert!(finally_block.is_some());
                }
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn test_with_stmt() {
        // fn f() -> None: with open() as g: File: pass
        let toks = vec![
            tok(TokenKind::KwFn), id("f"),
            tok(TokenKind::LParen), tok(TokenKind::RParen),
            tok(TokenKind::Arrow), id("None"), tok(TokenKind::Colon), nl(),
            ind(),
                tok(TokenKind::KwWith), id("open"),
                tok(TokenKind::LParen), tok(TokenKind::RParen),
                tok(TokenKind::KwAs), id("g"), tok(TokenKind::Colon), id("File"),
                tok(TokenKind::Colon), nl(),
                ind(), tok(TokenKind::KwPass), nl(), ded(),
            ded(),
            eof(),
        ];
        let m = parse(toks).unwrap();
        match &m.decls[0] {
            TopDecl::Func(f) => match &f.body.stmts[0] {
                Stmt::With { binding: Some((n, _)), .. } => assert_eq!(n, "g"),
                other => panic!("got {:?}", other),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn test_let_declarations() {
        // fn f() -> None: x: i32 = 0
        let toks = vec![
            tok(TokenKind::KwFn), id("f"),
            tok(TokenKind::LParen), tok(TokenKind::RParen),
            tok(TokenKind::Arrow), id("None"), tok(TokenKind::Colon), nl(),
            ind(),
                id("x"), tok(TokenKind::Colon), id("i32"),
                tok(TokenKind::Assign), int(0), nl(),
            ded(),
            eof(),
        ];
        let m = parse(toks).unwrap();
        match &m.decls[0] {
            TopDecl::Func(f) => match &f.body.stmts[0] {
                Stmt::Let { name, .. } => assert_eq!(name, "x"),
                other => panic!("got {:?}", other),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn test_augmented_assignments() {
        // fn f() -> None: x += 1
        let toks = vec![
            tok(TokenKind::KwFn), id("f"),
            tok(TokenKind::LParen), tok(TokenKind::RParen),
            tok(TokenKind::Arrow), id("None"), tok(TokenKind::Colon), nl(),
            ind(),
                id("x"), tok(TokenKind::PlusEq), int(1), nl(),
            ded(),
            eof(),
        ];
        let m = parse(toks).unwrap();
        match &m.decls[0] {
            TopDecl::Func(f) => match &f.body.stmts[0] {
                Stmt::AugAssign { op: BinOp::Add, .. } => {}
                other => panic!("got {:?}", other),
            },
            _ => panic!(),
        }
    }

    /// Parse an expression in isolation by wrapping it as `fn f() -> i32: return EXPR`.
    fn parse_return_expr(mut expr_toks: Vec<Token>) -> Expr {
        let mut toks = vec![
            tok(TokenKind::KwFn), id("f"),
            tok(TokenKind::LParen), tok(TokenKind::RParen),
            tok(TokenKind::Arrow), id("i32"), tok(TokenKind::Colon), nl(),
            ind(),
                tok(TokenKind::KwReturn),
        ];
        toks.append(&mut expr_toks);
        toks.push(nl());
        toks.push(ded());
        toks.push(eof());
        let m = parse(toks).unwrap();
        match m.decls.into_iter().next().unwrap() {
            TopDecl::Func(f) => match f.body.stmts.into_iter().next().unwrap() {
                Stmt::Return { value: Some(e), .. } => e,
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn test_binary_op_precedence() {
        // 1 + 2 * 3  →  Binary(Add, 1, Binary(Mul, 2, 3))
        let e = parse_return_expr(vec![
            int(1), tok(TokenKind::Plus), int(2), tok(TokenKind::Star), int(3),
        ]);
        match e {
            Expr::Binary { op: BinOp::Add, rhs, .. } => match *rhs {
                Expr::Binary { op: BinOp::Mul, .. } => {}
                other => panic!("got {:?}", other),
            },
            other => panic!("got {:?}", other),
        }

        // 2 ** 3 ** 4  →  Binary(Pow, 2, Binary(Pow, 3, 4))  (right-assoc)
        let e = parse_return_expr(vec![
            int(2), tok(TokenKind::DoubleStar), int(3), tok(TokenKind::DoubleStar), int(4),
        ]);
        match e {
            Expr::Binary { op: BinOp::Pow, rhs, .. } => match *rhs {
                Expr::Binary { op: BinOp::Pow, .. } => {}
                other => panic!("rhs {:?}", other),
            },
            other => panic!("got {:?}", other),
        }
    }

    #[test]
    fn test_unary_ops() {
        let e = parse_return_expr(vec![tok(TokenKind::Minus), id("x")]);
        assert!(matches!(e, Expr::Unary { op: UnaryOp::Neg, .. }));
        let e = parse_return_expr(vec![tok(TokenKind::KwNot), id("x")]);
        assert!(matches!(e, Expr::Unary { op: UnaryOp::Not, .. }));
        let e = parse_return_expr(vec![tok(TokenKind::Tilde), id("x")]);
        assert!(matches!(e, Expr::Unary { op: UnaryOp::BitNot, .. }));
    }

    #[test]
    fn test_method_call_vs_attr() {
        // a.b() → MethodCall
        let e = parse_return_expr(vec![
            id("a"), tok(TokenKind::Dot), id("b"),
            tok(TokenKind::LParen), tok(TokenKind::RParen),
        ]);
        assert!(matches!(e, Expr::MethodCall { .. }));
        // a.b → Attr
        let e = parse_return_expr(vec![id("a"), tok(TokenKind::Dot), id("b")]);
        assert!(matches!(e, Expr::Attr { .. }));
    }

    #[test]
    fn test_null_coalesce() {
        let e = parse_return_expr(vec![
            id("a"), tok(TokenKind::QuestionQuestion), id("b"),
        ]);
        assert!(matches!(e, Expr::NullCoalesce { .. }));
    }

    #[test]
    fn test_ternary() {
        // a if c else b
        let e = parse_return_expr(vec![
            id("a"), tok(TokenKind::KwIf), id("c"), tok(TokenKind::KwElse), id("b"),
        ]);
        assert!(matches!(e, Expr::Ternary { .. }));
    }

    #[test]
    fn test_lambda() {
        // fn(x: i32) -> i32: x + 1
        let e = parse_return_expr(vec![
            tok(TokenKind::KwFn), tok(TokenKind::LParen),
            id("x"), tok(TokenKind::Colon), id("i32"),
            tok(TokenKind::RParen), tok(TokenKind::Arrow), id("i32"),
            tok(TokenKind::Colon),
            id("x"), tok(TokenKind::Plus), int(1),
        ]);
        match e {
            Expr::Lambda { params, .. } => assert_eq!(params.len(), 1),
            other => panic!("got {:?}", other),
        }
    }

    fn parse_type_only(mut type_toks: Vec<Token>) -> Type {
        // fn f(x: <TYPE>) -> i32: return 0
        let mut toks = vec![
            tok(TokenKind::KwFn), id("f"),
            tok(TokenKind::LParen),
            id("x"), tok(TokenKind::Colon),
        ];
        toks.append(&mut type_toks);
        toks.extend(vec![
            tok(TokenKind::RParen),
            tok(TokenKind::Arrow), id("i32"), tok(TokenKind::Colon), nl(),
            ind(),
                tok(TokenKind::KwReturn), int(0), nl(),
            ded(),
            eof(),
        ]);
        let m = parse(toks).unwrap();
        match m.decls.into_iter().next().unwrap() {
            TopDecl::Func(f) => f.params.into_iter().next().unwrap().ty,
            _ => panic!(),
        }
    }

    #[test]
    fn test_nullable_type() {
        let t = parse_type_only(vec![id("T"), tok(TokenKind::Question)]);
        assert!(matches!(t, Type::Nullable { .. }));
    }

    #[test]
    fn test_generic_type() {
        // List[i32]
        let t = parse_type_only(vec![
            id("List"), tok(TokenKind::LBracket), id("i32"), tok(TokenKind::RBracket),
        ]);
        match t {
            Type::Named { name, args, .. } => {
                assert_eq!(name, "List");
                assert_eq!(args.len(), 1);
            }
            _ => panic!(),
        }
        // Dict[str, i32]
        let t = parse_type_only(vec![
            id("Dict"), tok(TokenKind::LBracket),
            id("str"), tok(TokenKind::Comma), id("i32"),
            tok(TokenKind::RBracket),
        ]);
        match t {
            Type::Named { args, .. } => assert_eq!(args.len(), 2),
            _ => panic!(),
        }
    }

    #[test]
    fn test_function_type() {
        // fn(i32, str) -> bool
        let t = parse_type_only(vec![
            tok(TokenKind::KwFn), tok(TokenKind::LParen),
            id("i32"), tok(TokenKind::Comma), id("str"),
            tok(TokenKind::RParen), tok(TokenKind::Arrow), id("bool"),
        ]);
        match t {
            Type::Function { params, .. } => assert_eq!(params.len(), 2),
            _ => panic!(),
        }
    }

    #[test]
    fn test_imports() {
        // from a.b import c, d as e <NL>
        // import x.y as z <NL>
        let toks = vec![
            tok(TokenKind::KwFrom), id("a"), tok(TokenKind::Dot), id("b"),
            tok(TokenKind::KwImport),
            id("c"), tok(TokenKind::Comma), id("d"), tok(TokenKind::KwAs), id("e"),
            nl(),
            tok(TokenKind::KwImport), id("x"), tok(TokenKind::Dot), id("y"),
            tok(TokenKind::KwAs), id("z"),
            nl(),
            eof(),
        ];
        let m = parse(toks).unwrap();
        assert_eq!(m.imports.len(), 2);
        assert_eq!(m.imports[0].path, vec!["a", "b"]);
        assert_eq!(m.imports[0].items.len(), 2);
        assert_eq!(m.imports[0].items[1].name, "d");
        assert_eq!(m.imports[0].items[1].alias.as_deref(), Some("e"));
        assert_eq!(m.imports[1].path, vec!["x", "y"]);
        assert_eq!(m.imports[1].alias.as_deref(), Some("z"));
    }

    #[test]
    fn test_int_suffix_roundtrip() {
        // sanity: ensure IntSuffix flows through parser unchanged
        let e = parse_return_expr(vec![int_s(42, IntSuffix::I64)]);
        match e {
            Expr::Literal { lit: Literal::Int { suffix: Some(IntSuffix::I64), .. }, .. } => {}
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn test_float_literal() {
        let e = parse_return_expr(vec![float(3.14)]);
        match e {
            Expr::Literal { lit: Literal::Float { value, suffix: None }, .. } => {
                assert!((value - 3.14).abs() < 1e-9);
            }
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn test_const_decl() {
        // final PI : f64 = 3
        let toks = vec![
            tok(TokenKind::KwFinal), id("PI"),
            tok(TokenKind::Colon), id("f64"),
            tok(TokenKind::Assign), int(3),
            nl(), eof(),
        ];
        let m = parse(toks).unwrap();
        match &m.decls[0] {
            TopDecl::Const(c) => assert_eq!(c.name, "PI"),
            _ => panic!(),
        }
        // ensure FloatSuffix path compiles
        let _ = FloatSuffix::F32;
    }
}
