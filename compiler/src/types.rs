//! Semantic type representation. See spec §5.
//!
//! Distinct from the *syntactic* [`crate::ast::Type`] — that one mirrors
//! source text, this one is the canonical resolved form used by the type
//! checker, IR, and codegen.

use std::collections::HashMap;

/// Newtype id for a user-defined class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClassId(pub u32);

/// Newtype id for a protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProtoId(pub u32);

/// Newtype id for a type-inference variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeVarId(pub u32);

/// Primitive types from spec §5.1.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimTy {
    Bool,
    I8, I16, I32, I64,
    U8, U16, U32, U64,
    F32, F64,
    Char,
    Str,
    Bytes,
    BigInt,
    /// Type of the `none` literal before promotion to `T?`.
    Null,
    /// `None` (the unit return type).
    Unit,
}

impl PrimTy {
    pub fn is_integer(self) -> bool {
        matches!(
            self,
            PrimTy::I8 | PrimTy::I16 | PrimTy::I32 | PrimTy::I64
                | PrimTy::U8 | PrimTy::U16 | PrimTy::U32 | PrimTy::U64
                | PrimTy::BigInt
        )
    }
    pub fn is_signed_int(self) -> bool {
        matches!(self, PrimTy::I8 | PrimTy::I16 | PrimTy::I32 | PrimTy::I64 | PrimTy::BigInt)
    }
    pub fn is_float(self) -> bool {
        matches!(self, PrimTy::F32 | PrimTy::F64)
    }
    pub fn is_numeric(self) -> bool {
        self.is_integer() || self.is_float()
    }
}

/// A type constructor for parameterized types.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeCtor {
    List,
    Dict,
    Set,
    Tuple,
    /// `Channel[T]` runtime container (spec §16.3) — stdlib: producer.spy
    Channel,
    /// `Atomic[T]` runtime container — stdlib: spec §16.4
    Atomic,
    /// `Future[T]` runtime container (spec §9.43) — M32 asyncio.
    /// Receiver shape mirrors `Channel[T]` (await/is_ready are
    /// special-cased in the typechecker + IR — see
    /// `resolve_native_method`).
    Future,
    /// `Range` iterable returned by `range(...)` — stdlib: spec §9.1
    Range,
    /// `Iterable[T]` / `Iterator[T]` protocols treated as generic constructors
    Iterable,
    Iterator,
    /// User-defined generic class.
    Class(ClassId),
    /// Generic protocol.
    Protocol(ProtoId),
}

/// The semantic type universe (spec §5.1).
#[derive(Debug, Clone)]
pub enum Ty {
    Primitive(PrimTy),
    Class(ClassId),
    Protocol(ProtoId),
    Generic { base: TypeCtor, args: Vec<Ty> },
    Function { params: Vec<Ty>, ret: Box<Ty> },
    Tuple(Vec<Ty>),
    Nullable(Box<Ty>),
    /// The empty type — values of this type never exist (spec §5.1.1).
    Never,
    /// Unification variable used during inference (spec §10.4).
    Var(TypeVarId),
}

impl Ty {
    /// Trivial subtyping — reflexive, `Never <: T`, `T <: T?`.
    ///
    /// For the full algorithm (classes, protocols, variance) call
    /// [`is_subtype`] which takes a [`TypeContext`].
    pub fn subtype(&self, other: &Ty) -> bool {
        // The full impl requires a TypeContext; we fall back to syntactic
        // equality + trivial rules here. Callers that need protocol /
        // class-hierarchy subtyping must go through [`is_subtype`].
        is_subtype_trivial(self, other)
    }

    pub fn is_never(&self) -> bool { matches!(self, Ty::Never) }

    pub fn is_null(&self) -> bool { matches!(self, Ty::Primitive(PrimTy::Null)) }

    pub fn is_nullable(&self) -> bool { matches!(self, Ty::Nullable(_) | Ty::Primitive(PrimTy::Null)) }

    pub fn unwrap_nullable(&self) -> Ty {
        match self {
            Ty::Nullable(t) => (**t).clone(),
            other => other.clone(),
        }
    }

    pub fn display(&self) -> String { display_ty(self) }
}

pub fn display_ty(t: &Ty) -> String {
    match t {
        Ty::Primitive(p) => format!("{:?}", p).to_lowercase(),
        Ty::Class(ClassId(i)) => format!("class#{}", i),
        Ty::Protocol(ProtoId(i)) => format!("proto#{}", i),
        Ty::Generic { base, args } => {
            let inner = args.iter().map(display_ty).collect::<Vec<_>>().join(", ");
            format!("{:?}[{}]", base, inner)
        }
        Ty::Function { params, ret } => {
            let ps = params.iter().map(display_ty).collect::<Vec<_>>().join(", ");
            format!("fn({}) -> {}", ps, display_ty(ret))
        }
        Ty::Tuple(elems) => {
            let inner = elems.iter().map(display_ty).collect::<Vec<_>>().join(", ");
            format!("Tuple[{}]", inner)
        }
        Ty::Nullable(t) => format!("{}?", display_ty(t)),
        Ty::Never => "Never".into(),
        Ty::Var(TypeVarId(i)) => format!("?T{}", i),
    }
}

/// Equality up to syntactic structure.  Type vars are equal iff their ids match.
pub fn ty_eq(a: &Ty, b: &Ty) -> bool {
    match (a, b) {
        (Ty::Primitive(p), Ty::Primitive(q)) => p == q,
        (Ty::Class(a), Ty::Class(b)) => a == b,
        (Ty::Protocol(a), Ty::Protocol(b)) => a == b,
        (Ty::Generic { base: b1, args: a1 }, Ty::Generic { base: b2, args: a2 }) => {
            b1 == b2 && a1.len() == a2.len() && a1.iter().zip(a2).all(|(x, y)| ty_eq(x, y))
        }
        (Ty::Function { params: p1, ret: r1 }, Ty::Function { params: p2, ret: r2 }) => {
            p1.len() == p2.len()
                && p1.iter().zip(p2).all(|(x, y)| ty_eq(x, y))
                && ty_eq(r1, r2)
        }
        (Ty::Tuple(a), Ty::Tuple(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| ty_eq(x, y))
        }
        (Ty::Nullable(a), Ty::Nullable(b)) => ty_eq(a, b),
        (Ty::Never, Ty::Never) => true,
        (Ty::Var(a), Ty::Var(b)) => a == b,
        _ => false,
    }
}

/// M63b: a built-in protocol bound declarable on a generic type parameter,
/// e.g. `[T: Comparable]`. v1 ships a fixed set of structural bounds satisfied
/// only by built-in primitive / `str` / `char` types — there is no user-level
/// trait/impl mechanism yet (user classes cannot satisfy a bound).
///
/// Which built-in types satisfy each bound:
///   * `Comparable` — `i8..i64`, `u8..u64`, `f32`, `f64`, `char`, `str`
///     (enables `<`, `<=`, `>`, `>=` inside a bounded generic body).
///   * `Equatable`  — every `Comparable` type plus `bool`
///     (enables `==`, `!=`).
///   * `Printable`  — every primitive plus `str` / `char`
///     (enables `str(x)` / display).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoundKind {
    Comparable,
    Equatable,
    Printable,
}

impl BoundKind {
    /// Map a bound type name (`Comparable`, `Equatable`, `Printable`,
    /// `Display`) to a [`BoundKind`]. Returns `None` for any other name so the
    /// resolver can reject unknown bounds.
    pub fn from_name(name: &str) -> Option<BoundKind> {
        match name {
            "Comparable" => Some(BoundKind::Comparable),
            "Equatable" => Some(BoundKind::Equatable),
            "Printable" | "Display" => Some(BoundKind::Printable),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            BoundKind::Comparable => "Comparable",
            BoundKind::Equatable => "Equatable",
            BoundKind::Printable => "Printable",
        }
    }

    /// Does the concrete type `t` satisfy this bound? Only built-in
    /// primitive / `str` / `char` types can — user classes, protocols,
    /// generics, tuples, etc. never satisfy a v1 bound.
    pub fn satisfied_by(self, t: &Ty) -> bool {
        let p = match t {
            Ty::Primitive(p) => *p,
            _ => return false,
        };
        match self {
            BoundKind::Comparable => {
                p.is_numeric() || matches!(p, PrimTy::Str | PrimTy::Char)
            }
            BoundKind::Equatable => {
                p.is_numeric()
                    || matches!(p, PrimTy::Str | PrimTy::Char | PrimTy::Bool)
            }
            BoundKind::Printable => {
                p.is_numeric()
                    || matches!(p, PrimTy::Str | PrimTy::Char | PrimTy::Bool)
            }
        }
    }
}

/// Field record inside a class layout.
#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub name: String,
    pub ty: Ty,
    pub offset: u32,
}

/// Method record (used for vtables and protocol satisfaction).
#[derive(Debug, Clone)]
pub struct MethodSig {
    pub name: String,
    pub params: Vec<Ty>,
    pub ret: Ty,
}

#[derive(Debug, Clone)]
pub struct ClassLayout {
    pub id: ClassId,
    pub name: String,
    pub base: Option<ClassId>,
    pub is_open: bool,
    pub is_sealed: bool,
    pub fields: Vec<FieldInfo>,
    pub methods: Vec<MethodSig>,
    /// Generic type-parameter names declared on the class (e.g. `T` in `List[T]`).
    pub generics: Vec<String>,
    /// M31: one [`TypeVarId`] per entry of `generics`, in declaration order.
    /// Empty for non-generic and built-in classes. Resolver-allocated so the
    /// type-checker can build substitutions at constructor sites by index,
    /// exactly as M17 does for free functions.
    pub generic_tvars: Vec<TypeVarId>,
    /// True for built-in stdlib classes (Channel, Thread, io.File, Dict, str)
    /// whose methods are dispatched via NativeFn rather than a vtable.
    /// User-defined classes are always false. See ir.rs::lower_method_call.
    pub is_native: bool,
    /// Payload size in bytes (excluding the 16-byte object header). For a
    /// subclass this includes the parent's payload — so the next subclass's
    /// fields lay out *after* every inherited field. See M11 BUG-016.
    pub payload_size: u32,
}

#[derive(Debug, Clone)]
pub struct ProtocolInfo {
    pub id: ProtoId,
    pub name: String,
    pub methods: Vec<MethodSig>,
}

/// Context the subtyping algorithm needs to consult class/protocol metadata.
pub struct TypeContext<'a> {
    pub classes: &'a HashMap<ClassId, ClassLayout>,
    pub protocols: &'a HashMap<ProtoId, ProtocolInfo>,
}

/// Trivial subtype — used as a fallback when no context is available.
pub fn is_subtype_trivial(a: &Ty, b: &Ty) -> bool {
    if matches!(a, Ty::Never) { return true; }
    if ty_eq(a, b) { return true; }
    // Null promotes into any nullable.
    if matches!(a, Ty::Primitive(PrimTy::Null)) {
        if matches!(b, Ty::Nullable(_) | Ty::Primitive(PrimTy::Null)) { return true; }
    }
    // T <: T?
    if let Ty::Nullable(inner) = b {
        if ty_eq(a, inner) { return true; }
    }
    false
}

/// Full subtype check per spec §5.2.
pub fn is_subtype(a: &Ty, b: &Ty, ctx: &TypeContext) -> bool {
    // Strip type variables: if either side is a Var, treat as unknown — only
    // equal if same var. (Real unification happens elsewhere.)
    if matches!(a, Ty::Never) { return true; }
    if matches!(b, Ty::Never) { return ty_eq(a, b); }

    if ty_eq(a, b) { return true; }

    // Null <: T? for any T.
    if matches!(a, Ty::Primitive(PrimTy::Null)) {
        if matches!(b, Ty::Nullable(_)) { return true; }
    }
    // T <: T?
    if let Ty::Nullable(inner) = b {
        if is_subtype(a, inner, ctx) { return true; }
    }
    // T1? <: T2? iff T1 <: T2.
    if let (Ty::Nullable(x), Ty::Nullable(y)) = (a, b) {
        if is_subtype(x, y, ctx) { return true; }
    }
    // Class hierarchy (open / sealed bases).
    if let (Ty::Class(ca), Ty::Class(cb)) = (a, b) {
        return class_extends(*ca, *cb, ctx);
    }
    // Class <: Protocol.
    if let (Ty::Class(ca), Ty::Protocol(pb)) = (a, b) {
        return class_satisfies_protocol(*ca, *pb, ctx);
    }
    // Tuple covariance.
    if let (Ty::Tuple(xs), Ty::Tuple(ys)) = (a, b) {
        return xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| is_subtype(x, y, ctx));
    }
    // Function: contravariant in params, covariant in result.
    if let (Ty::Function { params: pa, ret: ra }, Ty::Function { params: pb, ret: rb }) = (a, b) {
        if pa.len() != pb.len() { return false; }
        for (x, y) in pa.iter().zip(pb) {
            if !is_subtype(y, x, ctx) { return false; }
        }
        return is_subtype(ra, rb, ctx);
    }
    // Generic containers: invariant (per spec §5.2 mutable containers are invariant).
    if let (Ty::Generic { base: b1, args: a1 }, Ty::Generic { base: b2, args: a2 }) = (a, b) {
        if b1 == b2 && a1.len() == a2.len() {
            // Tuple constructor is covariant.
            if matches!(b1, TypeCtor::Tuple) {
                return a1.iter().zip(a2).all(|(x, y)| is_subtype(x, y, ctx));
            }
            return a1.iter().zip(a2).all(|(x, y)| ty_eq(x, y));
        }
        // Class<T> <: Protocol[U]? Treat conservatively — only structural via class id.
    }
    false
}

fn class_extends(c: ClassId, target: ClassId, ctx: &TypeContext) -> bool {
    if c == target { return true; }
    let mut cur = Some(c);
    while let Some(id) = cur {
        if id == target { return true; }
        cur = ctx.classes.get(&id).and_then(|l| l.base);
    }
    false
}

fn class_satisfies_protocol(c: ClassId, p: ProtoId, ctx: &TypeContext) -> bool {
    let proto = match ctx.protocols.get(&p) { Some(p) => p, None => return false };
    // Walk class hierarchy collecting methods.
    let mut methods: Vec<MethodSig> = Vec::new();
    let mut cur = Some(c);
    while let Some(id) = cur {
        if let Some(cl) = ctx.classes.get(&id) {
            for m in &cl.methods {
                if !methods.iter().any(|existing| existing.name == m.name) {
                    methods.push(m.clone());
                }
            }
            cur = cl.base;
        } else {
            break;
        }
    }
    for required in &proto.methods {
        let provided = methods.iter().find(|m| m.name == required.name);
        let Some(p) = provided else { return false };
        if p.params.len() != required.params.len() { return false; }
        // Method params/ret must match (use sub-typing in correct variance).
        for (a, b) in p.params.iter().zip(&required.params) {
            // contravariant in params: required <: provided
            if !is_subtype(b, a, ctx) { return false; }
        }
        if !is_subtype(&p.ret, &required.ret, ctx) { return false; }
    }
    true
}
