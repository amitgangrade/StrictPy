//! Untyped abstract syntax tree. Mirrors spec §10.6.
//!
//! All nodes carry a `Span` so that diagnostics in later passes can point
//! back to the source. Recursive fields use `Box<...>` for sized AST nodes
//! and `Vec<...>` for lists.

use crate::lexer::{FloatSuffix, IntSuffix};

/// Byte-range plus line/column for diagnostic rendering.
#[derive(Debug, Clone, Copy, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub line: u32,
    pub col: u32,
}

impl Span {
    pub const DUMMY: Span = Span { start: 0, end: 0, line: 0, col: 0 };
}

// ─────────────────────────────────────────────────────────────────────────
//  Top-level module
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Module {
    pub name: String,
    pub imports: Vec<Import>,
    pub decls: Vec<TopDecl>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Import {
    pub path: Vec<String>,
    pub items: Vec<ImportItem>,
    pub alias: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ImportItem {
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone)]
pub enum TopDecl {
    Func(FuncDecl),
    Class(ClassDecl),
    Protocol(ProtocolDecl),
    Const(ConstDecl),
    TypeAlias(TypeAliasDecl),
}

#[derive(Debug, Clone)]
pub struct ConstDecl {
    pub name: String,
    pub ty: Type,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TypeAliasDecl {
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub ty: Type,
    pub span: Span,
}

// ─────────────────────────────────────────────────────────────────────────
//  Function / class / protocol
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FuncDecl {
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub return_ty: Type,
    pub body: Block,
    pub decorators: Vec<Decorator>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct GenericParam {
    pub name: String,
    pub bounds: Vec<Type>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Decorator {
    pub path: Vec<String>,
    pub args: Vec<Arg>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ClassDecl {
    pub name: String,
    pub modifier: ClassModifier,
    pub generics: Vec<GenericParam>,
    pub bases: Vec<Type>,
    pub fields: Vec<FieldDecl>,
    pub methods: Vec<FuncDecl>,
    pub init: Option<FuncDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassModifier {
    Final,
    Open,
    Sealed,
}

#[derive(Debug, Clone)]
pub struct FieldDecl {
    pub name: String,
    pub ty: Type,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ProtocolDecl {
    pub name: String,
    pub generics: Vec<GenericParam>,
    pub methods: Vec<ProtoMethod>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct ProtoMethod {
    pub name: String,
    pub params: Vec<Param>,
    pub return_ty: Type,
    pub span: Span,
}

// ─────────────────────────────────────────────────────────────────────────
//  Statements
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let { name: String, ty: Type, init: Expr, span: Span },
    /// Tuple destructuring let:
    ///   `x: T1, y: T2 = pair()`              — fully annotated
    ///   `x, y = pair()`                       — types inferred from RHS
    /// Per-name `Option<Type>` lets each binding optionally annotate.
    /// Added in M14 (tuples). See spec §5.X.
    LetDestructure { names: Vec<String>, tys: Vec<Option<Type>>, init: Expr, span: Span },
    Assign { target: Lvalue, value: Expr, span: Span },
    AugAssign { target: Lvalue, op: BinOp, value: Expr, span: Span },
    Return { value: Option<Expr>, span: Span },
    If {
        cond: Expr,
        then_block: Block,
        elifs: Vec<(Expr, Block)>,
        else_block: Option<Block>,
        span: Span,
    },
    While { cond: Expr, body: Block, else_block: Option<Block>, span: Span },
    For {
        var: String,
        var_ty: Type,
        iter: Expr,
        body: Block,
        else_block: Option<Block>,
        span: Span,
    },
    Match { scrutinee: Expr, arms: Vec<MatchArm>, span: Span },
    Try {
        body: Block,
        handlers: Vec<ExceptHandler>,
        else_block: Option<Block>,
        finally_block: Option<Block>,
        span: Span,
    },
    With {
        expr: Expr,
        binding: Option<(String, Type)>,
        body: Block,
        span: Span,
    },
    Raise { exc: Expr, cause: Option<Expr>, span: Span },
    Assert { cond: Expr, msg: Option<Expr>, span: Span },
    Del { target: Lvalue, span: Span },
    Expr { expr: Expr, span: Span },
    Break { span: Span },
    Continue { span: Span },
    Pass { span: Span },
}

#[derive(Debug, Clone)]
pub struct ExceptHandler {
    pub exc_ty: Type,
    pub binding: Option<String>,
    pub body: Block,
    pub span: Span,
}

// ─────────────────────────────────────────────────────────────────────────
//  Patterns (spec §4 / match_stmt)
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Literal(Literal, Span),
    Identifier(String, Span),
    Wildcard(Span),
    Constructor { ty: Type, fields: Vec<Pattern>, span: Span },
    Tuple(Vec<Pattern>, Span),
}

// ─────────────────────────────────────────────────────────────────────────
//  Expressions
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Lvalue {
    Ident { name: String, span: Span },
    Attr { obj: Box<Expr>, name: String, span: Span },
    Index { obj: Box<Expr>, indices: Vec<Expr>, span: Span },
}

#[derive(Debug, Clone)]
pub enum Literal {
    Int { value: i128, suffix: Option<IntSuffix> },
    Float { value: f64, suffix: Option<FloatSuffix> },
    Str(String),
    Bytes(Vec<u8>),
    Char(char),
    Bool(bool),
    None,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal { lit: Literal, span: Span },
    Ident { name: String, span: Span },
    Tuple { elems: Vec<Expr>, span: Span },
    List { elems: Vec<Expr>, span: Span },
    Dict { entries: Vec<(Expr, Expr)>, span: Span },
    Set { elems: Vec<Expr>, span: Span },

    Unary { op: UnaryOp, operand: Box<Expr>, span: Span },
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr>, span: Span },

    Call { callee: Box<Expr>, args: Vec<Arg>, span: Span },
    MethodCall {
        receiver: Box<Expr>,
        method: String,
        args: Vec<Arg>,
        span: Span,
    },
    Attr { obj: Box<Expr>, name: String, span: Span },
    Index { obj: Box<Expr>, indices: Vec<Expr>, span: Span },

    NullCoalesce { lhs: Box<Expr>, rhs: Box<Expr>, span: Span },
    Ternary {
        cond: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
        span: Span,
    },
    Lambda {
        params: Vec<Param>,
        return_ty: Type,
        body: Box<Expr>,
        span: Span,
    },

    Cast { expr: Box<Expr>, target: Type, span: Span },

    /// M62a: a comprehension — list `[body for x: T in iter if cond]`,
    /// set `{body for x: T in iter if cond}`, or dict
    /// `{k: v for x: T in iter if cond}`. The loop variable carries a type
    /// annotation just like the `for` statement. `cond` is the optional
    /// `if` filter. For list/set comprehensions `value` is `None`; for dict
    /// comprehensions `body` is the key expression and `value` is the value.
    Comprehension {
        kind: ComprehensionKind,
        var: String,
        /// Span of the loop-variable name token (used to key its symbol).
        var_span: Span,
        var_ty: Type,
        iter: Box<Expr>,
        body: Box<Expr>,
        value: Option<Box<Expr>>,
        cond: Option<Box<Expr>>,
        span: Span,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComprehensionKind {
    List,
    Set,
    Dict,
}

#[derive(Debug, Clone)]
pub struct Arg {
    pub name: Option<String>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Pos,
    BitNot,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    // Arithmetic
    Add, Sub, Mul, Div, FloorDiv, Rem, Pow,
    // Bitwise / shift
    BitAnd, BitOr, BitXor, Shl, Shr,
    // Comparison
    Eq, Ne, Lt, Le, Gt, Ge,
    Is, IsNot, In, NotIn,
    // Boolean
    And, Or,
}

// ─────────────────────────────────────────────────────────────────────────
//  Syntactic types (the AST's view of types — distinct from semantic `Ty`)
// ─────────────────────────────────────────────────────────────────────────

/// A type as it appears in source. The resolver/type checker translates this
/// into the semantic [`crate::types::Ty`].
#[derive(Debug, Clone)]
pub enum Type {
    /// `i32`, `bool`, etc., or a user-defined name.
    Named { name: String, args: Vec<Type>, span: Span },
    /// `T?`
    Nullable { inner: Box<Type>, span: Span },
    /// `fn(T1, T2) -> R`
    Function { params: Vec<Type>, ret: Box<Type>, span: Span },
    /// `Tuple[T1, T2, ...]` written via subscript; also the implicit form.
    Tuple { elems: Vec<Type>, span: Span },
    /// Special inferred-placeholder used by lambdas in checking mode.
    Infer { span: Span },
    /// `Never` (spec §5.1.1).
    Never { span: Span },
}
