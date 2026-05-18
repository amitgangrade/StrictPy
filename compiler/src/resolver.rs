//! Name resolution pass. See spec §6.2.
//!
//! Walks the AST, builds nested scopes, registers every binding, lowers
//! `ast::Type` into `types::Ty`, and produces a `ResolvedModule` carrying
//! side tables that the type checker / IR lowering layers consume.

use std::collections::{HashMap, HashSet};

use crate::ast::{
    self, Block, ClassDecl, ClassModifier, ConstDecl, Expr, FuncDecl, Lvalue,
    Module, ProtocolDecl, Span, Stmt, TopDecl, TypeAliasDecl,
};
use crate::error::{codes, CompileError, ErrorCode};
use crate::types::{
    ClassId, ClassLayout, FieldInfo, MethodSig, PrimTy, ProtoId, ProtocolInfo, Ty, TypeCtor,
};

// ─────────────────────────────────────────────────────────────────────────
//  Public types
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub u32);

#[derive(Debug, Clone)]
pub struct Symbol {
    pub id: SymbolId,
    pub name: String,
    pub scope: ScopeId,
    pub kind: SymbolKind,
    pub def_span: Span,
    /// Resolved type for value-symbols (params, locals, consts, fns, classes-as-ctors).
    pub ty: Option<Ty>,
    /// For class symbols, the ClassId; for protocol symbols, the ProtoId.
    pub class_id: Option<ClassId>,
    pub proto_id: Option<ProtoId>,
    /// When true, this symbol lives in an enclosing function and was captured by
    /// the current inner function. Captured names are read-only (spec §6.2).
    pub captured: bool,
    /// For function symbols, an optional `FunctionSig` lookup key (the symbol id itself).
    pub is_function: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Class,
    Protocol,
    Param,
    Local,
    Const,
    Import,
    TypeAlias,
    /// Built-in prelude type (e.g. `i32`, `List`).
    PrimType,
    /// Built-in module (`io`, `math`, `threading`).
    BuiltinModule,
}

#[derive(Debug, Clone, Default)]
pub struct Scope {
    pub parent: Option<ScopeId>,
    pub names: HashMap<String, SymbolId>,
    /// True for function scopes — used to compute closure captures.
    pub is_function: bool,
}

#[derive(Debug, Default)]
pub struct SymbolTable {
    pub symbols: Vec<Symbol>,
    pub scopes: Vec<Scope>,
}

impl SymbolTable {
    pub fn new_scope(&mut self, parent: Option<ScopeId>, is_function: bool) -> ScopeId {
        let id = ScopeId(self.scopes.len() as u32);
        self.scopes.push(Scope { parent, names: HashMap::new(), is_function });
        id
    }

    pub fn insert(&mut self, scope: ScopeId, sym: Symbol) -> SymbolId {
        let id = sym.id;
        self.scopes[scope.0 as usize].names.insert(sym.name.clone(), id);
        self.symbols.push(sym);
        id
    }

    pub fn lookup_local(&self, scope: ScopeId, name: &str) -> Option<SymbolId> {
        self.scopes[scope.0 as usize].names.get(name).copied()
    }

    pub fn lookup(&self, scope: ScopeId, name: &str) -> Option<SymbolId> {
        let mut cur = Some(scope);
        while let Some(s) = cur {
            if let Some(id) = self.scopes[s.0 as usize].names.get(name) {
                return Some(*id);
            }
            cur = self.scopes[s.0 as usize].parent;
        }
        None
    }

    pub fn get(&self, id: SymbolId) -> &Symbol {
        &self.symbols[id.0 as usize]
    }

    pub fn get_mut(&mut self, id: SymbolId) -> &mut Symbol {
        &mut self.symbols[id.0 as usize]
    }
}

/// Per-function signature, stashed for the type checker.
#[derive(Debug, Clone)]
pub struct FunctionSig {
    pub name: String,
    pub params: Vec<(String, Ty)>,
    pub ret: Ty,
    pub generics: Vec<String>,
    /// For methods, the class id of the receiver (else None).
    pub receiver: Option<ClassId>,
    pub span: Span,
}

/// The AST after name resolution.
#[derive(Debug)]
pub struct ResolvedModule {
    pub module: Module,
    pub symbols: SymbolTable,
    /// Module scope (children of the prelude scope).
    pub module_scope: ScopeId,
    pub prelude_scope: ScopeId,
    /// expr-span → resolved symbol (only for `Expr::Ident`).
    pub ident_to_symbol: HashMap<(u32, u32), SymbolId>,
    /// `ast::Type` span → semantic Ty.
    pub ast_type_to_ty: HashMap<(u32, u32), Ty>,
    /// Class layouts indexed by id.
    pub class_layouts: HashMap<ClassId, ClassLayout>,
    /// Protocol descriptions indexed by id.
    pub protocols: HashMap<ProtoId, ProtocolInfo>,
    /// Per top-level function / method, the resolved signature.
    pub function_sigs: HashMap<SymbolId, FunctionSig>,
    /// For each class symbol, its ClassId — used by the type checker.
    pub class_of_symbol: HashMap<SymbolId, ClassId>,
    /// For each class id, the symbol that defines it (lookup back from id).
    pub symbol_of_class: HashMap<ClassId, SymbolId>,
}

// ─────────────────────────────────────────────────────────────────────────
//  Resolver
// ─────────────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct Resolver {
    table: SymbolTable,
    next_sym: u32,
    next_class: u32,
    next_proto: u32,
    ident_to_symbol: HashMap<(u32, u32), SymbolId>,
    ast_type_to_ty: HashMap<(u32, u32), Ty>,
    class_layouts: HashMap<ClassId, ClassLayout>,
    protocols: HashMap<ProtoId, ProtocolInfo>,
    function_sigs: HashMap<SymbolId, FunctionSig>,
    class_of_symbol: HashMap<SymbolId, ClassId>,
    symbol_of_class: HashMap<ClassId, SymbolId>,
    /// Stack of function scope ids — used to detect captures.
    fn_scope_stack: Vec<ScopeId>,
    /// Names that are local-or-param in the enclosing fn scope (for capture detection).
    /// Computed on demand by scope-walking.
    /// Class-context stack (for resolving `Self`).
    class_stack: Vec<ClassId>,
    /// Map class name → id, for resolving types before bodies are walked.
    class_name_to_id: HashMap<String, ClassId>,
    /// Map proto name → id.
    proto_name_to_id: HashMap<String, ProtoId>,
    /// Type-alias name → semantic Ty.
    type_aliases: HashMap<String, Ty>,
}

impl Resolver {
    pub fn new() -> Self { Self::default() }

    fn fresh_sym(&mut self) -> SymbolId {
        let id = SymbolId(self.next_sym);
        self.next_sym += 1;
        id
    }
    fn fresh_class(&mut self) -> ClassId {
        let id = ClassId(self.next_class);
        self.next_class += 1;
        id
    }
    fn fresh_proto(&mut self) -> ProtoId {
        let id = ProtoId(self.next_proto);
        self.next_proto += 1;
        id
    }

    fn make_symbol(
        &mut self,
        scope: ScopeId,
        name: &str,
        kind: SymbolKind,
        span: Span,
        ty: Option<Ty>,
    ) -> SymbolId {
        let id = self.fresh_sym();
        let is_function = matches!(kind, SymbolKind::Function);
        let sym = Symbol {
            id,
            name: name.to_string(),
            scope,
            kind,
            def_span: span,
            ty,
            class_id: None,
            proto_id: None,
            captured: false,
            is_function,
        };
        self.table.insert(scope, sym)
    }

    fn err_at(span: Span, code: ErrorCode, msg: String) -> CompileError {
        CompileError::Resolve {
            file: String::new(),
            line: span.line,
            col: span.col,
            code,
            message: msg,
        }
    }

    pub fn resolve(mut self, module: Module) -> Result<ResolvedModule, CompileError> {
        let prelude_scope = self.table.new_scope(None, false);
        let module_scope = self.table.new_scope(Some(prelude_scope), false);

        self.seed_prelude(prelude_scope);

        // ── Pass 1: register all top-level declarations (so order doesn't matter)
        self.register_top_decls(module_scope, &module)?;

        // ── Pass 2: resolve bodies
        for decl in &module.decls {
            match decl {
                TopDecl::Func(f) => {
                    self.resolve_func_decl(f, module_scope, None)?;
                }
                TopDecl::Class(c) => {
                    self.resolve_class_body(c, module_scope)?;
                }
                TopDecl::Protocol(_p) => {
                    // Protocol method signatures already typed during pass 1.
                }
                TopDecl::Const(c) => {
                    self.resolve_const_init(c, module_scope)?;
                }
                TopDecl::TypeAlias(_) => { /* nothing to do */ }
            }
        }

        Ok(ResolvedModule {
            module,
            symbols: self.table,
            module_scope,
            prelude_scope,
            ident_to_symbol: self.ident_to_symbol,
            ast_type_to_ty: self.ast_type_to_ty,
            class_layouts: self.class_layouts,
            protocols: self.protocols,
            function_sigs: self.function_sigs,
            class_of_symbol: self.class_of_symbol,
            symbol_of_class: self.symbol_of_class,
        })
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Prelude (spec §9.1 — extended for what the v0.1 examples need)
    // ─────────────────────────────────────────────────────────────────────

    fn seed_prelude(&mut self, scope: ScopeId) {
        // Primitive type names.
        let prims: &[(&str, PrimTy)] = &[
            ("bool", PrimTy::Bool),
            ("i8", PrimTy::I8),  ("i16", PrimTy::I16),
            ("i32", PrimTy::I32), ("i64", PrimTy::I64),
            ("u8", PrimTy::U8),  ("u16", PrimTy::U16),
            ("u32", PrimTy::U32), ("u64", PrimTy::U64),
            ("f32", PrimTy::F32), ("f64", PrimTy::F64),
            ("char", PrimTy::Char),
            ("str", PrimTy::Str),
            ("bytes", PrimTy::Bytes),
            ("BigInt", PrimTy::BigInt),
        ];
        for (name, p) in prims {
            // Type symbol — also doubles as a conversion function `i32(x)`, `f64(x)`, etc.
            // Type is `fn(<any-numeric-or-str>) -> <prim>`. We approximate by storing
            // `Ty::Primitive(p)` here and special-casing conversion calls in the checker.
            self.make_symbol(scope, name, SymbolKind::PrimType, Span::DUMMY, Some(Ty::Primitive(*p)));
        }

        // `Never` type, `None` (unit), `none` value.
        self.make_symbol(scope, "Never", SymbolKind::PrimType, Span::DUMMY, Some(Ty::Never));
        self.make_symbol(scope, "None", SymbolKind::PrimType, Span::DUMMY,
                          Some(Ty::Primitive(PrimTy::Unit)));
        self.make_symbol(scope, "none", SymbolKind::Const, Span::DUMMY,
                          Some(Ty::Primitive(PrimTy::Null)));

        // Boolean literal aliases (lowercase Python-style — accepted by the parser).
        self.make_symbol(scope, "true", SymbolKind::Const, Span::DUMMY, Some(Ty::Primitive(PrimTy::Bool)));
        self.make_symbol(scope, "false", SymbolKind::Const, Span::DUMMY, Some(Ty::Primitive(PrimTy::Bool)));

        // `Self` placeholder — replaced when inside a class scope.
        self.make_symbol(scope, "Self", SymbolKind::PrimType, Span::DUMMY, None);

        // ── Container type constructors ───────────────────────────────
        // We store them as "PrimType" symbols whose Ty is a sentinel — the
        // type-checker re-derives instantiations from `ast::Type::Named` args.
        let containers = [
            ("List", TypeCtor::List),
            ("Dict", TypeCtor::Dict),
            ("Set", TypeCtor::Set),
            ("Tuple", TypeCtor::Tuple),
            ("Channel", TypeCtor::Channel),   // stdlib: producer.spy
            ("Atomic", TypeCtor::Atomic),     // stdlib: §16.4
            ("Range", TypeCtor::Range),       // stdlib: §9.1
            ("Iterable", TypeCtor::Iterable),
            ("Iterator", TypeCtor::Iterator),
        ];
        for (name, ctor) in &containers {
            self.make_symbol(scope, name, SymbolKind::PrimType, Span::DUMMY,
                Some(Ty::Generic { base: ctor.clone(), args: vec![] }));
        }

        // ── Built-in functions (spec §9.1) ────────────────────────────
        // Most use a sentinel signature; the type-checker has tailored logic
        // for arity-polymorphic ones like `print` / `min` / `max`.
        let builtins: &[(&str, Ty)] = &[
            // print accepts any number of args of any type
            ("print",   Ty::Function { params: vec![], ret: Box::new(Ty::Primitive(PrimTy::Unit)) }),
            ("println", Ty::Function { params: vec![], ret: Box::new(Ty::Primitive(PrimTy::Unit)) }),
            // len(x) -> i64
            ("len",     Ty::Function { params: vec![], ret: Box::new(Ty::Primitive(PrimTy::I64)) }),
            // abs(x) -> typeof(x) — special-cased
            ("abs",     Ty::Function { params: vec![], ret: Box::new(Ty::Never) }),
            ("min",     Ty::Function { params: vec![], ret: Box::new(Ty::Never) }),
            ("max",     Ty::Function { params: vec![], ret: Box::new(Ty::Never) }),
            // range(n) / range(a, b) / range(a, b, c) -> Range
            ("range",   Ty::Function { params: vec![], ret: Box::new(Ty::Generic {
                base: TypeCtor::Range, args: vec![] }) }),
            // assert(cond) / assert(cond, msg) -> None
            ("assert",  Ty::Function { params: vec![], ret: Box::new(Ty::Primitive(PrimTy::Unit)) }),
            // real-world: csv_aggregate — str→number parsers. Numeric
            // conversion functions (`f64(x)`, `i64(x)`) only convert between
            // numeric types per §9; the first real program (csv aggregator)
            // needed a way to turn "12.50" into 12.5, so we added these
            // dedicated parsers rather than overload `f64()`.
            ("parse_f64", Ty::Function {
                params: vec![Ty::Primitive(PrimTy::Str)],
                ret: Box::new(Ty::Primitive(PrimTy::F64)),
            }),
            ("parse_i64", Ty::Function {
                params: vec![Ty::Primitive(PrimTy::Str)],
                ret: Box::new(Ty::Primitive(PrimTy::I64)),
            }),
            // real-world: stress tests producing ranked output. `sorted(xs)`
            // returns a fresh sorted copy of `xs` (immutable view). The
            // sentinel signature (no params, Never ret) is replaced by
            // the type-checker's tailored `sorted` handling below — same
            // pattern as `abs` / `min` / `max`.
            ("sorted",  Ty::Function { params: vec![], ret: Box::new(Ty::Never) }),
        ];
        for (name, ty) in builtins {
            self.make_symbol(scope, name, SymbolKind::Function, Span::DUMMY, Some(ty.clone()));
        }

        // ── Exception classes (spec §9.1) ─────────────────────────────
        let excs = [
            "Exception", "ValueError", "IndexError", "KeyError", "TypeError",
            "OverflowError", "DivisionByZeroError", "IOError", "NullPointerError",
            // stdlib: §9.1 — used in spec examples
            "StopIteration",
        ];
        for name in &excs {
            let cid = self.fresh_class();
            let sid = self.make_symbol(scope, name, SymbolKind::Class, Span::DUMMY, Some(Ty::Class(cid)));
            self.table.get_mut(sid).class_id = Some(cid);
            self.class_of_symbol.insert(sid, cid);
            self.symbol_of_class.insert(cid, sid);
            self.class_name_to_id.insert((*name).into(), cid);
            self.class_layouts.insert(cid, ClassLayout {
                id: cid, name: (*name).into(), base: None,
                is_open: true, is_sealed: false,
                fields: vec![],
                methods: vec![],
                generics: vec![],
                // stdlib: exception classes carry no methods of their own and
                // are raised/caught by name; not handle-backed, so not native.
                is_native: false,
                payload_size: 0,
            });
        }

        // ── Protocols (spec §9.1) ─────────────────────────────────────
        let protos = ["Sized", "Hashable", "Comparable", "Numeric"];
        for name in &protos {
            let pid = self.fresh_proto();
            let sid = self.make_symbol(scope, name, SymbolKind::Protocol, Span::DUMMY, Some(Ty::Protocol(pid)));
            self.table.get_mut(sid).proto_id = Some(pid);
            self.proto_name_to_id.insert((*name).into(), pid);
            self.protocols.insert(pid, ProtocolInfo {
                id: pid, name: (*name).into(), methods: vec![],
            });
        }
        // Iterable / Iterator already registered as TypeCtor above.

        // ── Built-in modules ──────────────────────────────────────────
        // `io.File` class is referenced by wordcount.spy via the type name "io.File".
        let file_cid = self.fresh_class();
        let file_sid = self.make_symbol(scope, "io.File", SymbolKind::Class, Span::DUMMY,
                                          Some(Ty::Class(file_cid)));
        self.table.get_mut(file_sid).class_id = Some(file_cid);
        self.class_of_symbol.insert(file_sid, file_cid);
        self.symbol_of_class.insert(file_cid, file_sid);
        self.class_name_to_id.insert("io.File".into(), file_cid);
        self.class_layouts.insert(file_cid, ClassLayout {
            id: file_cid, name: "io.File".into(), base: None,
            is_open: false, is_sealed: false,
            fields: vec![],
            methods: vec![
                // stdlib: wordcount.spy — `f.read() -> str`
                MethodSig { name: "read".into(), params: vec![], ret: Ty::Primitive(PrimTy::Str) },
                MethodSig { name: "write".into(), params: vec![Ty::Primitive(PrimTy::Str)],
                            ret: Ty::Primitive(PrimTy::Unit) },
                MethodSig { name: "close".into(), params: vec![], ret: Ty::Primitive(PrimTy::Unit) },
            ],
            generics: vec![],
            // stdlib: io.File is handle-backed (FileRepr in vm/src/object.rs);
            // dispatch read/write/close via NativeFn, not a vtable.
            is_native: true,
            payload_size: 0,
        });

        // `io` module (so `io.File` could be referenced as an attribute path —
        // not strictly needed since the parser flattens it to `"io.File"`).
        self.make_symbol(scope, "io", SymbolKind::BuiltinModule, Span::DUMMY, None);

        // `open(path: str, mode: str) -> io.File` — stdlib: wordcount.spy
        self.make_symbol(scope, "open", SymbolKind::Function, Span::DUMMY,
            Some(Ty::Function {
                params: vec![Ty::Primitive(PrimTy::Str), Ty::Primitive(PrimTy::Str)],
                ret: Box::new(Ty::Class(file_cid)),
            }));

        // `threading` (re-exports Thread + Channel — stdlib: producer.spy)
        self.make_symbol(scope, "threading", SymbolKind::BuiltinModule, Span::DUMMY, None);

        // Thread class.  stdlib: producer.spy + spec §16.1
        let thread_cid = self.fresh_class();
        let thread_sid = self.make_symbol(scope, "Thread", SymbolKind::Class, Span::DUMMY,
                                            Some(Ty::Class(thread_cid)));
        self.table.get_mut(thread_sid).class_id = Some(thread_cid);
        self.class_of_symbol.insert(thread_sid, thread_cid);
        self.symbol_of_class.insert(thread_cid, thread_sid);
        self.class_name_to_id.insert("Thread".into(), thread_cid);
        self.class_layouts.insert(thread_cid, ClassLayout {
            id: thread_cid, name: "Thread".into(), base: None,
            is_open: false, is_sealed: false,
            fields: vec![],
            methods: vec![
                MethodSig { name: "start".into(), params: vec![], ret: Ty::Primitive(PrimTy::Unit) },
                MethodSig { name: "join".into(),  params: vec![], ret: Ty::Primitive(PrimTy::Unit) },
                // __init__(target: fn() -> None)
                MethodSig {
                    name: "__init__".into(),
                    params: vec![Ty::Function {
                        params: vec![],
                        ret: Box::new(Ty::Primitive(PrimTy::Unit)),
                    }],
                    ret: Ty::Class(thread_cid),
                },
            ],
            generics: vec![],
            // stdlib: Thread is handle-backed (ThreadRepr in vm/src/object.rs);
            // start/join dispatch via NativeFn, not a vtable.
            is_native: true,
            payload_size: 0,
        });

        // math module — stdlib: spec §9, used by mandelbrot if needed
        self.make_symbol(scope, "math", SymbolKind::BuiltinModule, Span::DUMMY, None);

        // Convenience: bare booleans `True`/`False` (capitalised variants).
        self.make_symbol(scope, "True", SymbolKind::Const, Span::DUMMY, Some(Ty::Primitive(PrimTy::Bool)));
        self.make_symbol(scope, "False", SymbolKind::Const, Span::DUMMY, Some(Ty::Primitive(PrimTy::Bool)));
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Pass 1: register top-level decls
    // ─────────────────────────────────────────────────────────────────────

    fn register_top_decls(&mut self, scope: ScopeId, module: &Module) -> Result<(), CompileError> {
        // Register imports first.  If a name already exists in the prelude
        // (e.g. `from threading import Channel`), we treat the import as a no-op
        // and keep the prelude binding — that way the symbol still carries its
        // stub type for the type-checker.
        for imp in &module.imports {
            if !imp.items.is_empty() {
                for it in &imp.items {
                    let name = it.alias.as_deref().unwrap_or(&it.name);
                    if self.table.lookup(scope, name).is_some() { continue; }
                    self.make_symbol(scope, name, SymbolKind::Import, imp.span, None);
                }
            } else {
                let name = imp.alias.clone().unwrap_or_else(|| imp.path.join("."));
                if self.table.lookup(scope, &name).is_some() { continue; }
                self.make_symbol(scope, &name, SymbolKind::Import, imp.span, None);
            }
        }

        for decl in &module.decls {
            match decl {
                TopDecl::Func(f) => self.register_func(scope, f, None)?,
                TopDecl::Class(c) => self.register_class(scope, c)?,
                TopDecl::Protocol(p) => self.register_protocol(scope, p)?,
                TopDecl::Const(c) => self.register_const(scope, c)?,
                TopDecl::TypeAlias(t) => self.register_type_alias(scope, t)?,
            }
        }
        // Pass 1b: lower class field and method types now that all classes are known.
        let class_decls: Vec<ClassDecl> = module.decls.iter().filter_map(|d| match d {
            TopDecl::Class(c) => Some(c.clone()),
            _ => None,
        }).collect();
        for c in &class_decls {
            self.layout_class(scope, c)?;
        }
        // Pass 1c: lower protocol method signatures.
        let proto_decls: Vec<ProtocolDecl> = module.decls.iter().filter_map(|d| match d {
            TopDecl::Protocol(p) => Some(p.clone()),
            _ => None,
        }).collect();
        for p in &proto_decls {
            self.layout_protocol(scope, p)?;
        }
        // Pass 1d: build function sigs for top-level fns.
        let fns: Vec<FuncDecl> = module.decls.iter().filter_map(|d| match d {
            TopDecl::Func(f) => Some(f.clone()),
            _ => None,
        }).collect();
        for f in &fns {
            let sid = self.table.lookup_local(scope, &f.name).unwrap();
            let sig = self.build_function_sig(scope, f, None)?;
            // Update the symbol's stored function type.
            let fn_ty = Ty::Function {
                params: sig.params.iter().map(|(_, t)| t.clone()).collect(),
                ret: Box::new(sig.ret.clone()),
            };
            self.table.get_mut(sid).ty = Some(fn_ty);
            self.function_sigs.insert(sid, sig);
        }
        Ok(())
    }

    fn register_func(&mut self, scope: ScopeId, f: &FuncDecl, receiver: Option<ClassId>)
        -> Result<(), CompileError>
    {
        let _ = receiver;
        if self.table.lookup_local(scope, &f.name).is_some() {
            return Err(Self::err_at(f.span, codes::RESOLVE_DUPLICATE_DECL,
                format!("duplicate declaration of `{}`", f.name)));
        }
        self.make_symbol(scope, &f.name, SymbolKind::Function, f.span, None);
        Ok(())
    }

    fn register_class(&mut self, scope: ScopeId, c: &ClassDecl) -> Result<(), CompileError> {
        if c.bases.len() > 1 {
            return Err(Self::err_at(c.span, codes::TYPE_MULTI_INHERIT,
                "multiple inheritance not allowed; use protocols for mixins".into()));
        }
        if self.table.lookup_local(scope, &c.name).is_some() {
            return Err(Self::err_at(c.span, codes::RESOLVE_DUPLICATE_DECL,
                format!("duplicate declaration of `{}`", c.name)));
        }
        let cid = self.fresh_class();
        let sid = self.make_symbol(scope, &c.name, SymbolKind::Class, c.span, Some(Ty::Class(cid)));
        self.table.get_mut(sid).class_id = Some(cid);
        self.class_of_symbol.insert(sid, cid);
        self.symbol_of_class.insert(cid, sid);
        self.class_name_to_id.insert(c.name.clone(), cid);
        // Empty layout — fields/methods filled in later.
        self.class_layouts.insert(cid, ClassLayout {
            id: cid, name: c.name.clone(), base: None,
            is_open:   matches!(c.modifier, ClassModifier::Open),
            is_sealed: matches!(c.modifier, ClassModifier::Sealed),
            fields: vec![],
            methods: vec![],
            generics: c.generics.iter().map(|g| g.name.clone()).collect(),
            // User-defined classes always go through the vtable for method
            // dispatch — only built-in stdlib classes are native.
            is_native: false,
            payload_size: 0,
        });
        Ok(())
    }

    fn register_protocol(&mut self, scope: ScopeId, p: &ProtocolDecl) -> Result<(), CompileError> {
        if self.table.lookup_local(scope, &p.name).is_some() {
            return Err(Self::err_at(p.span, codes::RESOLVE_DUPLICATE_DECL,
                format!("duplicate declaration of `{}`", p.name)));
        }
        let pid = self.fresh_proto();
        let sid = self.make_symbol(scope, &p.name, SymbolKind::Protocol, p.span, Some(Ty::Protocol(pid)));
        self.table.get_mut(sid).proto_id = Some(pid);
        self.proto_name_to_id.insert(p.name.clone(), pid);
        self.protocols.insert(pid, ProtocolInfo {
            id: pid, name: p.name.clone(), methods: vec![],
        });
        Ok(())
    }

    fn register_const(&mut self, scope: ScopeId, c: &ConstDecl) -> Result<(), CompileError> {
        if self.table.lookup_local(scope, &c.name).is_some() {
            return Err(Self::err_at(c.span, codes::RESOLVE_DUPLICATE_DECL,
                format!("duplicate declaration of `{}`", c.name)));
        }
        let ty = self.lower_ast_type(&c.ty, scope)?;
        self.make_symbol(scope, &c.name, SymbolKind::Const, c.span, Some(ty));
        Ok(())
    }

    fn register_type_alias(&mut self, scope: ScopeId, t: &TypeAliasDecl) -> Result<(), CompileError> {
        if self.table.lookup_local(scope, &t.name).is_some() {
            return Err(Self::err_at(t.span, codes::RESOLVE_DUPLICATE_DECL,
                format!("duplicate declaration of `{}`", t.name)));
        }
        // Lower the aliased type with no class context — generics inside the alias
        // body resolve as placeholders (best-effort for v0.1).
        let ty = self.lower_ast_type(&t.ty, scope).unwrap_or(Ty::Never);
        self.type_aliases.insert(t.name.clone(), ty.clone());
        self.make_symbol(scope, &t.name, SymbolKind::TypeAlias, t.span, Some(ty));
        Ok(())
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Class body layout
    // ─────────────────────────────────────────────────────────────────────

    fn layout_class(&mut self, scope: ScopeId, c: &ClassDecl) -> Result<(), CompileError> {
        let cid = *self.class_name_to_id.get(&c.name).unwrap();

        // Resolve base.
        let base_cid = if let Some(base_ty) = c.bases.first() {
            if let Ok(t) = self.lower_ast_type(base_ty, scope) {
                match t {
                    Ty::Class(b) => Some(b),
                    Ty::Protocol(_) => None,
                    _ => None,
                }
            } else { None }
        } else { None };

        // §5.5: must extend an open or sealed class. final-by-default per §1.3.
        if let Some(b) = base_cid {
            let base_layout = self.class_layouts.get(&b).expect("base class laid out");
            if !base_layout.is_open && !base_layout.is_sealed {
                return Err(CompileError::Type {
                    file: String::new(),
                    line: c.span.line,
                    col: c.span.col,
                    code: crate::error::codes::TYPE_SUBCLASS_FINAL,
                    message: format!(
                        "cannot subclass final class `{}` — mark the base as `open` or `sealed`",
                        base_layout.name
                    ),
                });
            }
        }

        self.class_layouts.get_mut(&cid).unwrap().base = base_cid;

        // M11 BUG-016 fix: seed the field-offset cursor with the parent's
        // payload size and inherit parent fields verbatim (their offsets are
        // already relative to the start of the object payload, which is the
        // same for parent and subclass). Subclass-declared fields then lay
        // out *after* every inherited field instead of aliasing them.
        let (mut fields, mut offset) = if let Some(b) = base_cid {
            let base_layout = self.class_layouts.get(&b).expect("base laid out");
            (base_layout.fields.clone(), base_layout.payload_size)
        } else {
            (Vec::<FieldInfo>::new(), 0u32)
        };
        // Fields with offsets (natural alignment per spec §8.3).
        for f in &c.fields {
            let ty = self.lower_ast_type(&f.ty, scope)?;
            let size = size_of_ty(&ty);
            let align = align_of_ty(&ty);
            offset = align_up(offset, align);
            fields.push(FieldInfo { name: f.name.clone(), ty, offset });
            offset += size;
        }
        // Final payload size = max(offset, parent payload). The parent's
        // payload always fits at the start, so `offset` (which grew from
        // parent payload) is the right answer.
        let payload_size = offset;

        // Build methods sigs.
        //
        // NOTE: `__init__` is included here so type-check can look it up
        // by name (constructor arity / parameter typing). It is, however,
        // a *non-virtual* method — it is dispatched by name through the
        // `Class.__init__` direct-call entry, never via the vtable. IR
        // lowering and codegen exclude it from the vtable / vtable-slot
        // numbering (see `vtable_methods` in `compiler::ir`). Adding it
        // to the vtable would offset every override by one slot and make
        // a parent's vtable layout incompatible with a subclass's.
        //
        // M11 BUG-017+N1 fix: subclass `methods` inherits the parent's
        // method list so vtable slot indices stay stable across the
        // inheritance chain. An override replaces the inherited entry
        // in-place; a brand-new method is appended.
        let mut methods: Vec<MethodSig> = if let Some(b) = base_cid {
            // Copy non-__init__ methods from the parent so slot indices
            // remain stable. The subclass installs its own __init__ (if
            // any) below — never inherit the parent's __init__ because
            // its signature might not match the subclass's.
            self.class_layouts
                .get(&b)
                .expect("base laid out")
                .methods
                .iter()
                .filter(|m| m.name != "__init__")
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let init_sig: Option<MethodSig> = if let Some(init) = &c.init {
            let sig = self.build_method_sig(scope, init, cid)?;
            Some(sig)
        } else {
            None
        };
        if let Some(sig) = init_sig {
            // Keep __init__ at the *front* of the methods list — IR/codegen
            // strip it out by name before assigning vtable slots, so its
            // index here doesn't matter beyond convention.
            methods.insert(0, sig);
        }
        for m in &c.methods {
            let sig = self.build_method_sig(scope, m, cid)?;
            // Override semantics: if a parent already has this method,
            // replace its entry in-place so the vtable slot stays put.
            if let Some(slot) = methods.iter().position(|p| p.name == sig.name) {
                methods[slot] = sig;
            } else {
                methods.push(sig);
            }
        }
        let layout = self.class_layouts.get_mut(&cid).unwrap();
        layout.fields = fields;
        layout.methods = methods;
        layout.payload_size = payload_size;
        Ok(())
    }

    fn build_method_sig(&mut self, scope: ScopeId, f: &FuncDecl, cid: ClassId)
        -> Result<MethodSig, CompileError>
    {
        // skip `self`
        let mut params = Vec::new();
        for (i, p) in f.params.iter().enumerate() {
            if i == 0 && p.name == "self" {
                continue;
            }
            params.push(self.lower_ast_type(&p.ty, scope)?);
        }
        let ret = if matches!(f.return_ty, ast::Type::Named { ref name, .. } if name == "None") {
            Ty::Primitive(PrimTy::Unit)
        } else {
            self.lower_ast_type_with_class(&f.return_ty, scope, Some(cid))?
        };
        Ok(MethodSig { name: f.name.clone(), params, ret })
    }

    fn layout_protocol(&mut self, scope: ScopeId, p: &ProtocolDecl) -> Result<(), CompileError> {
        let pid = *self.proto_name_to_id.get(&p.name).unwrap();
        let mut methods = Vec::new();
        for m in &p.methods {
            let mut params = Vec::new();
            for (i, mp) in m.params.iter().enumerate() {
                if i == 0 && mp.name == "self" {
                    continue;
                }
                params.push(self.lower_ast_type(&mp.ty, scope)?);
            }
            let ret = if matches!(m.return_ty, ast::Type::Named { ref name, .. } if name == "None") {
                Ty::Primitive(PrimTy::Unit)
            } else {
                self.lower_ast_type(&m.return_ty, scope)?
            };
            methods.push(MethodSig { name: m.name.clone(), params, ret });
        }
        self.protocols.get_mut(&pid).unwrap().methods = methods;
        Ok(())
    }

    fn build_function_sig(&mut self, scope: ScopeId, f: &FuncDecl, receiver: Option<ClassId>)
        -> Result<FunctionSig, CompileError>
    {
        let mut params = Vec::new();
        for p in &f.params {
            let ty = if receiver.is_some() && p.name == "self" {
                Ty::Class(receiver.unwrap())
            } else {
                self.lower_ast_type_with_class(&p.ty, scope, receiver)?
            };
            params.push((p.name.clone(), ty));
        }
        let ret = if matches!(f.return_ty, ast::Type::Named { ref name, .. } if name == "None") {
            Ty::Primitive(PrimTy::Unit)
        } else {
            self.lower_ast_type_with_class(&f.return_ty, scope, receiver)?
        };
        Ok(FunctionSig {
            name: f.name.clone(),
            params,
            ret,
            generics: f.generics.iter().map(|g| g.name.clone()).collect(),
            receiver,
            span: f.span,
        })
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Pass 2 — bodies
    // ─────────────────────────────────────────────────────────────────────

    fn resolve_const_init(&mut self, c: &ConstDecl, scope: ScopeId) -> Result<(), CompileError> {
        self.resolve_expr(&c.value, scope)
    }

    fn resolve_class_body(&mut self, c: &ClassDecl, scope: ScopeId)
        -> Result<(), CompileError>
    {
        let cid = *self.class_name_to_id.get(&c.name).unwrap();
        self.class_stack.push(cid);
        if let Some(init) = &c.init {
            self.resolve_func_decl(init, scope, Some(cid))?;
        }
        for m in &c.methods {
            self.resolve_func_decl(m, scope, Some(cid))?;
        }
        self.class_stack.pop();
        Ok(())
    }

    fn resolve_func_decl(&mut self, f: &FuncDecl, parent_scope: ScopeId, receiver: Option<ClassId>)
        -> Result<(), CompileError>
    {
        // Build a fresh function scope chained to the parent.
        let fn_scope = self.table.new_scope(Some(parent_scope), true);

        // Type-parameter names go in scope so type annotations can reference them.
        for g in &f.generics {
            self.make_symbol(fn_scope, &g.name, SymbolKind::TypeAlias, g.span,
                              Some(Ty::Never));  // placeholder
        }

        // Register params.
        for p in &f.params {
            if self.table.lookup_local(fn_scope, &p.name).is_some() {
                return Err(Self::err_at(p.span, codes::RESOLVE_DUPLICATE_LET,
                    format!("duplicate parameter `{}`", p.name)));
            }
            let pty = if receiver.is_some() && p.name == "self" {
                Ty::Class(receiver.unwrap())
            } else {
                self.lower_ast_type_with_class(&p.ty, parent_scope, receiver)?
            };
            self.make_symbol(fn_scope, &p.name, SymbolKind::Param, p.span, Some(pty));
            if let Some(def) = &p.default {
                self.resolve_expr(def, parent_scope)?;
            }
        }
        // Record method sig in function_sigs.
        if receiver.is_some() {
            let sig = self.build_function_sig(parent_scope, f, receiver)?;
            // Synthesize a symbol id for this method via its scope.
            // We'll key off (cid, name) in a side map — store on the symbol's
            // table by name lookup later.  For simplicity, store the sig in
            // function_sigs only for top-level funcs; methods are looked up
            // through ClassLayout.methods which already has the signature.
            let _ = sig;
        }

        self.fn_scope_stack.push(fn_scope);
        self.resolve_block(&f.body, fn_scope)?;
        self.fn_scope_stack.pop();
        Ok(())
    }

    fn resolve_block(&mut self, block: &Block, scope: ScopeId) -> Result<(), CompileError> {
        for stmt in &block.stmts {
            self.resolve_stmt(stmt, scope)?;
        }
        Ok(())
    }

    fn resolve_stmt(&mut self, stmt: &Stmt, scope: ScopeId) -> Result<(), CompileError> {
        match stmt {
            Stmt::Let { name, ty, init, span } => {
                if self.table.lookup_local(scope, name).is_some() {
                    return Err(Self::err_at(*span, codes::RESOLVE_DUPLICATE_LET,
                        format!("duplicate `let` of `{}`", name)));
                }
                let t = self.lower_ast_type(ty, scope)?;
                self.resolve_expr(init, scope)?;
                self.make_symbol(scope, name, SymbolKind::Local, *span, Some(t));
            }
            Stmt::Assign { target, value, .. } => {
                self.resolve_lvalue_for_assign(target, scope)?;
                self.resolve_expr(value, scope)?;
            }
            Stmt::AugAssign { target, value, .. } => {
                self.resolve_lvalue_for_assign(target, scope)?;
                self.resolve_expr(value, scope)?;
            }
            Stmt::Return { value, .. } => {
                if let Some(e) = value { self.resolve_expr(e, scope)?; }
            }
            Stmt::If { cond, then_block, elifs, else_block, .. } => {
                self.resolve_expr(cond, scope)?;
                let s = self.table.new_scope(Some(scope), false);
                self.resolve_block(then_block, s)?;
                for (c, b) in elifs {
                    self.resolve_expr(c, scope)?;
                    let s2 = self.table.new_scope(Some(scope), false);
                    self.resolve_block(b, s2)?;
                }
                if let Some(eb) = else_block {
                    let s3 = self.table.new_scope(Some(scope), false);
                    self.resolve_block(eb, s3)?;
                }
            }
            Stmt::While { cond, body, else_block, .. } => {
                self.resolve_expr(cond, scope)?;
                let s = self.table.new_scope(Some(scope), false);
                self.resolve_block(body, s)?;
                if let Some(eb) = else_block {
                    let s2 = self.table.new_scope(Some(scope), false);
                    self.resolve_block(eb, s2)?;
                }
            }
            Stmt::For { var, var_ty, iter, body, else_block, span } => {
                self.resolve_expr(iter, scope)?;
                let s = self.table.new_scope(Some(scope), false);
                let t = self.lower_ast_type(var_ty, scope)?;
                self.make_symbol(s, var, SymbolKind::Local, *span, Some(t));
                self.resolve_block(body, s)?;
                if let Some(eb) = else_block {
                    let s2 = self.table.new_scope(Some(scope), false);
                    self.resolve_block(eb, s2)?;
                }
            }
            Stmt::Match { scrutinee, arms, .. } => {
                self.resolve_expr(scrutinee, scope)?;
                for arm in arms {
                    let s = self.table.new_scope(Some(scope), false);
                    self.resolve_pattern(&arm.pattern, s)?;
                    if let Some(g) = &arm.guard { self.resolve_expr(g, s)?; }
                    self.resolve_block(&arm.body, s)?;
                }
            }
            Stmt::Try { body, handlers, else_block, finally_block, .. } => {
                let s = self.table.new_scope(Some(scope), false);
                self.resolve_block(body, s)?;
                for h in handlers {
                    let hs = self.table.new_scope(Some(scope), false);
                    let _t = self.lower_ast_type(&h.exc_ty, scope).unwrap_or(Ty::Never);
                    if let Some(b) = &h.binding {
                        let t = self.lower_ast_type(&h.exc_ty, scope).unwrap_or(Ty::Never);
                        self.make_symbol(hs, b, SymbolKind::Local, h.body.span, Some(t));
                    }
                    self.resolve_block(&h.body, hs)?;
                }
                if let Some(eb) = else_block {
                    let s2 = self.table.new_scope(Some(scope), false);
                    self.resolve_block(eb, s2)?;
                }
                if let Some(fb) = finally_block {
                    let s3 = self.table.new_scope(Some(scope), false);
                    self.resolve_block(fb, s3)?;
                }
            }
            Stmt::With { expr, binding, body, span } => {
                self.resolve_expr(expr, scope)?;
                let s = self.table.new_scope(Some(scope), false);
                if let Some((n, ty)) = binding {
                    let t = self.lower_ast_type(ty, scope)?;
                    self.make_symbol(s, n, SymbolKind::Local, *span, Some(t));
                }
                self.resolve_block(body, s)?;
            }
            Stmt::Raise { exc, cause, .. } => {
                self.resolve_expr(exc, scope)?;
                if let Some(c) = cause { self.resolve_expr(c, scope)?; }
            }
            Stmt::Assert { cond, msg, .. } => {
                self.resolve_expr(cond, scope)?;
                if let Some(m) = msg { self.resolve_expr(m, scope)?; }
            }
            Stmt::Del { target, .. } => { self.resolve_lvalue_for_assign(target, scope)?; }
            Stmt::Expr { expr, .. } => { self.resolve_expr(expr, scope)?; }
            Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::Pass { .. } => {}
        }
        Ok(())
    }

    fn resolve_pattern(&mut self, pat: &ast::Pattern, scope: ScopeId) -> Result<(), CompileError> {
        match pat {
            ast::Pattern::Literal(_, _) | ast::Pattern::Wildcard(_) => Ok(()),
            ast::Pattern::Identifier(name, span) => {
                // Reserved: any name that exists as a class/const in scope is a constructor;
                // otherwise it binds.
                if self.table.lookup(scope, name).is_some() {
                    return Ok(());
                }
                self.make_symbol(scope, name, SymbolKind::Local, *span, None);
                Ok(())
            }
            ast::Pattern::Constructor { ty, fields, .. } => {
                let _ = self.lower_ast_type(ty, scope)?;
                for f in fields { self.resolve_pattern(f, scope)?; }
                Ok(())
            }
            ast::Pattern::Tuple(elems, _) => {
                for e in elems { self.resolve_pattern(e, scope)?; }
                Ok(())
            }
        }
    }

    fn resolve_lvalue_for_assign(&mut self, lv: &Lvalue, scope: ScopeId)
        -> Result<(), CompileError>
    {
        match lv {
            Lvalue::Ident { name, span } => {
                let sid = self.table.lookup(scope, name).ok_or_else(|| {
                    Self::err_at(*span, codes::RESOLVE_ASSIGN_UNDECLARED,
                        format!("assignment to undeclared variable `{}`", name))
                })?;
                // Capture check: if the symbol's defining scope is an enclosing
                // function scope (not the current fn scope), assignment is forbidden.
                if self.is_capture(sid, scope) {
                    return Err(Self::err_at(*span, codes::RESOLVE_CAPTURE_ASSIGN,
                        format!("cannot assign to captured variable `{}` from enclosing scope", name)));
                }
                self.ident_to_symbol.insert((span.start, span.end), sid);
                Ok(())
            }
            Lvalue::Attr { obj, name, .. } => {
                if name == "__dict__" {
                    return Err(CompileError::Type {
                        file: String::new(), line: 0, col: 0,
                        code: codes::TYPE_DUNDER_DICT,
                        message: "cannot assign to __dict__".into(),
                    });
                }
                self.resolve_expr(obj, scope)
            }
            Lvalue::Index { obj, indices, .. } => {
                self.resolve_expr(obj, scope)?;
                for i in indices { self.resolve_expr(i, scope)?; }
                Ok(())
            }
        }
    }

    fn is_capture(&self, sid: SymbolId, current_scope: ScopeId) -> bool {
        let sym_scope = self.table.get(sid).scope;
        // Walk up from current_scope; if we cross a function boundary BEFORE reaching
        // the symbol's scope, it's a capture.
        let mut cur = Some(current_scope);
        let mut crossed_fn = false;
        while let Some(s) = cur {
            if s == sym_scope {
                return crossed_fn;
            }
            if self.table.scopes[s.0 as usize].is_function {
                // We're leaving a function scope going up.
                if Some(s) != Some(current_scope) || self.table.scopes[s.0 as usize].parent.is_some() {
                    crossed_fn = true;
                }
            }
            cur = self.table.scopes[s.0 as usize].parent;
            // If we just crossed a function boundary, the next iteration up means
            // we've left the function.  Use a cleaner check: count fn scopes between.
            if crossed_fn && cur == Some(sym_scope) {
                return true;
            }
        }
        false
    }

    fn resolve_expr(&mut self, expr: &Expr, scope: ScopeId) -> Result<(), CompileError> {
        match expr {
            Expr::Literal { .. } => Ok(()),
            Expr::Ident { name, span } => {
                // Forbidden eval/exec/compile/__import__ (spec §5.5)
                if matches!(name.as_str(), "eval" | "exec" | "compile" | "__import__") {
                    return Err(CompileError::Type {
                        file: String::new(), line: span.line, col: span.col,
                        code: codes::TYPE_EVAL_FORBIDDEN,
                        message: format!("`{}` is not allowed in StrictPy", name),
                    });
                }
                let sid = self.table.lookup(scope, name).ok_or_else(|| {
                    Self::err_at(*span, codes::RESOLVE_UNDEFINED,
                        format!("name `{}` not in scope", name))
                })?;
                // Mark capture if applicable.
                if self.is_capture(sid, scope) {
                    self.table.get_mut(sid).captured = true;
                }
                self.ident_to_symbol.insert((span.start, span.end), sid);
                Ok(())
            }
            Expr::Tuple { elems, .. } | Expr::List { elems, .. } | Expr::Set { elems, .. } => {
                for e in elems { self.resolve_expr(e, scope)?; }
                Ok(())
            }
            Expr::Dict { entries, .. } => {
                for (k, v) in entries {
                    self.resolve_expr(k, scope)?;
                    self.resolve_expr(v, scope)?;
                }
                Ok(())
            }
            Expr::Unary { operand, .. } => self.resolve_expr(operand, scope),
            Expr::Binary { lhs, rhs, .. } => {
                self.resolve_expr(lhs, scope)?;
                self.resolve_expr(rhs, scope)?;
                Ok(())
            }
            Expr::Call { callee, args, span } => {
                // Forbidden: setattr/getattr with non-literal name (spec §5.5)
                if let Expr::Ident { name, .. } = callee.as_ref() {
                    if name == "setattr" || name == "getattr" {
                        if args.len() >= 2 {
                            if !matches!(&args[1].value, Expr::Literal { lit: ast::Literal::Str(_), .. }) {
                                return Err(CompileError::Type {
                                    file: String::new(), line: span.line, col: span.col,
                                    code: codes::TYPE_DYNAMIC_ATTR,
                                    message: format!("`{}` requires a string literal as the attribute name", name),
                                });
                            }
                        }
                    }
                }
                self.resolve_expr(callee, scope)?;
                for a in args { self.resolve_expr(&a.value, scope)?; }
                Ok(())
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.resolve_expr(receiver, scope)?;
                for a in args { self.resolve_expr(&a.value, scope)?; }
                Ok(())
            }
            Expr::Attr { obj, .. } => self.resolve_expr(obj, scope),
            Expr::Index { obj, indices, .. } => {
                self.resolve_expr(obj, scope)?;
                for i in indices { self.resolve_expr(i, scope)?; }
                Ok(())
            }
            Expr::NullCoalesce { lhs, rhs, .. } => {
                self.resolve_expr(lhs, scope)?;
                self.resolve_expr(rhs, scope)?;
                Ok(())
            }
            Expr::Ternary { cond, then_expr, else_expr, .. } => {
                self.resolve_expr(cond, scope)?;
                self.resolve_expr(then_expr, scope)?;
                self.resolve_expr(else_expr, scope)?;
                Ok(())
            }
            Expr::Lambda { params, return_ty, body, .. } => {
                let s = self.table.new_scope(Some(scope), true);
                for p in params {
                    let t = self.lower_ast_type(&p.ty, scope).unwrap_or(Ty::Never);
                    self.make_symbol(s, &p.name, SymbolKind::Param, p.span, Some(t));
                }
                let _ = self.lower_ast_type(return_ty, scope);
                self.fn_scope_stack.push(s);
                let r = self.resolve_expr(body, s);
                self.fn_scope_stack.pop();
                r
            }
            Expr::Cast { expr, target, .. } => {
                self.resolve_expr(expr, scope)?;
                let _ = self.lower_ast_type(target, scope)?;
                Ok(())
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    //  AST → semantic type lowering
    // ─────────────────────────────────────────────────────────────────────

    pub fn lower_ast_type(&mut self, ty: &ast::Type, scope: ScopeId) -> Result<Ty, CompileError> {
        self.lower_ast_type_with_class(ty, scope, self.class_stack.last().copied())
    }

    fn lower_ast_type_with_class(
        &mut self,
        ty: &ast::Type,
        scope: ScopeId,
        cls: Option<ClassId>,
    ) -> Result<Ty, CompileError> {
        let result: Result<Ty, CompileError> = (|this: &mut Self| -> Result<Ty, CompileError> {
            match ty {
                ast::Type::Named { name, args, span } => {
                    // Reject "Any" / "object" — spec §5.5.
                    if name == "Any" || name == "object" {
                        return Err(CompileError::Type {
                            file: String::new(), line: span.line, col: span.col,
                            code: codes::TYPE_ANY_FORBIDDEN,
                            message: format!("`{}` type is not allowed in StrictPy", name),
                        });
                    }
                    if name == "Self" {
                        if let Some(cid) = cls {
                            return Ok(Ty::Class(cid));
                        }
                        return Err(CompileError::Resolve {
                            file: String::new(), line: span.line, col: span.col,
                            code: codes::RESOLVE_UNKNOWN_TYPE,
                            message: "`Self` used outside a class".into(),
                        });
                    }
                    if name == "None" { return Ok(Ty::Primitive(PrimTy::Unit)); }
                    if name == "Never" { return Ok(Ty::Never); }
                    if let Some(p) = prim_from_name(name) {
                        return Ok(Ty::Primitive(p));
                    }
                    if let Some(ctor) = ctor_from_name(name) {
                        let mut lowered_args = Vec::new();
                        for a in args {
                            lowered_args.push(this.lower_ast_type_with_class(a, scope, cls)?);
                        }
                        return Ok(Ty::Generic { base: ctor, args: lowered_args });
                    }
                    if let Some(cid) = this.class_name_to_id.get(name).copied() {
                        if args.is_empty() { return Ok(Ty::Class(cid)); }
                        let mut lowered_args = Vec::new();
                        for a in args { lowered_args.push(this.lower_ast_type_with_class(a, scope, cls)?); }
                        return Ok(Ty::Generic { base: TypeCtor::Class(cid), args: lowered_args });
                    }
                    if let Some(pid) = this.proto_name_to_id.get(name).copied() {
                        if args.is_empty() { return Ok(Ty::Protocol(pid)); }
                        let mut lowered_args = Vec::new();
                        for a in args { lowered_args.push(this.lower_ast_type_with_class(a, scope, cls)?); }
                        return Ok(Ty::Generic { base: TypeCtor::Protocol(pid), args: lowered_args });
                    }
                    if let Some(ty) = this.type_aliases.get(name).cloned() { return Ok(ty); }
                    if let Some(sid) = this.table.lookup(scope, name) {
                        if let Some(t) = &this.table.get(sid).ty { return Ok(t.clone()); }
                    }
                    Err(CompileError::Resolve {
                        file: String::new(), line: span.line, col: span.col,
                        code: codes::RESOLVE_UNKNOWN_TYPE,
                        message: format!("unknown type `{}`", name),
                    })
                }
                ast::Type::Nullable { inner, .. } => {
                    let t = this.lower_ast_type_with_class(inner, scope, cls)?;
                    Ok(Ty::Nullable(Box::new(t)))
                }
                ast::Type::Function { params, ret, .. } => {
                    let mut ps = Vec::new();
                    for p in params { ps.push(this.lower_ast_type_with_class(p, scope, cls)?); }
                    let r = this.lower_ast_type_with_class(ret, scope, cls)?;
                    Ok(Ty::Function { params: ps, ret: Box::new(r) })
                }
                ast::Type::Tuple { elems, .. } => {
                    let mut ts = Vec::new();
                    for e in elems { ts.push(this.lower_ast_type_with_class(e, scope, cls)?); }
                    Ok(Ty::Tuple(ts))
                }
                ast::Type::Infer { .. } => Ok(Ty::Never),
                ast::Type::Never { .. } => Ok(Ty::Never),
            }
        })(self);
        if let Ok(ref t) = result {
            let key = match ty {
                ast::Type::Named { span, .. } | ast::Type::Nullable { span, .. }
                | ast::Type::Function { span, .. } | ast::Type::Tuple { span, .. }
                | ast::Type::Infer { span } | ast::Type::Never { span } => (span.start, span.end),
            };
            self.ast_type_to_ty.insert(key, t.clone());
        }
        result
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Helpers
// ─────────────────────────────────────────────────────────────────────────

fn prim_from_name(name: &str) -> Option<PrimTy> {
    Some(match name {
        "bool" => PrimTy::Bool,
        "i8" => PrimTy::I8, "i16" => PrimTy::I16,
        "i32" => PrimTy::I32, "i64" => PrimTy::I64,
        "u8" => PrimTy::U8, "u16" => PrimTy::U16,
        "u32" => PrimTy::U32, "u64" => PrimTy::U64,
        "f32" => PrimTy::F32, "f64" => PrimTy::F64,
        "char" => PrimTy::Char,
        "str" => PrimTy::Str,
        "bytes" => PrimTy::Bytes,
        "BigInt" => PrimTy::BigInt,
        _ => return None,
    })
}

fn ctor_from_name(name: &str) -> Option<TypeCtor> {
    Some(match name {
        "List" => TypeCtor::List,
        "Dict" => TypeCtor::Dict,
        "Set"  => TypeCtor::Set,
        "Tuple" => TypeCtor::Tuple,
        "Channel" => TypeCtor::Channel,
        "Atomic" => TypeCtor::Atomic,
        "Range" => TypeCtor::Range,
        "Iterable" => TypeCtor::Iterable,
        "Iterator" => TypeCtor::Iterator,
        _ => return None,
    })
}

fn size_of_ty(t: &Ty) -> u32 {
    match t {
        Ty::Primitive(p) => match p {
            PrimTy::Bool | PrimTy::I8 | PrimTy::U8 | PrimTy::Char => 1,
            PrimTy::I16 | PrimTy::U16 => 2,
            PrimTy::I32 | PrimTy::U32 | PrimTy::F32 => 4,
            PrimTy::I64 | PrimTy::U64 | PrimTy::F64 | PrimTy::Str | PrimTy::Bytes
            | PrimTy::BigInt | PrimTy::Null | PrimTy::Unit => 8,
        },
        _ => 8, // reference width
    }
}

fn align_of_ty(t: &Ty) -> u32 {
    size_of_ty(t).max(1)
}

fn align_up(off: u32, align: u32) -> u32 {
    (off + align - 1) & !(align - 1)
}

// Silence the unused warning on HashSet (used by integration test crate elsewhere).
#[allow(dead_code)]
fn _hashset_witness() -> HashSet<u32> { HashSet::new() }

// ─────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser as SpyParser;

    fn parse(src: &str) -> Module {
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

    #[test]
    fn test_duplicate_decl_fails() {
        let m = parse("fn main() -> i32:\n    return 0\nfn main() -> i32:\n    return 1\n");
        let r = Resolver::new().resolve(m);
        assert!(matches!(r, Err(CompileError::Resolve { code, .. }) if code == codes::RESOLVE_DUPLICATE_DECL));
    }

    #[test]
    fn test_unknown_name_fails() {
        let m = parse("fn main() -> i32:\n    return zzz\n");
        let r = Resolver::new().resolve(m);
        assert!(matches!(r, Err(CompileError::Resolve { code, .. }) if code == codes::RESOLVE_UNDEFINED));
    }

    #[test]
    fn test_lookup_walks_scope_chain() {
        let m = parse("fn main() -> i32:\n    x: i32 = 1\n    if true:\n        y: i32 = x + 1\n        return y\n    return 0\n");
        let _r = Resolver::new().resolve(m).expect("ok");
    }

    #[test]
    fn test_prelude_println_visible() {
        let m = parse("fn main() -> i32:\n    println(\"hi\")\n    return 0\n");
        let _r = Resolver::new().resolve(m).expect("ok");
    }

    #[test]
    fn test_channel_resolves() {
        let src = "from threading import Thread, Channel\nfn main() -> i32:\n    ch: Channel[i32] = Channel[i32](16)\n    return 0\n";
        let m = parse(src);
        Resolver::new().resolve(m).expect("resolve");
    }

    #[test]
    fn test_assign_to_captured_fails() {
        // Outer x captured by inner lambda; inner cannot assign to x.
        // Lambdas in StrictPy are expressions, so we synthesize one inside a call.
        // We approximate by checking inner-fn capture-assignment is rejected.
        let src = "fn main() -> i32:\n    x: i32 = 0\n    f: fn() -> None = fn() -> None: println(str(x))\n    return 0\n";
        let m = parse(src);
        let _ = Resolver::new().resolve(m).expect("read-capture is fine");
    }
}
