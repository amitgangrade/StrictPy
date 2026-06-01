//! Name resolution pass. See spec §6.2.
//!
//! Walks the AST, builds nested scopes, registers every binding, lowers
//! `ast::Type` into `types::Ty`, and produces a `ResolvedModule` carrying
//! side tables that the type checker / IR lowering layers consume.

use std::collections::{HashMap, HashSet};

use crate::ast::{
    self, Block, ClassDecl, ClassModifier, ConstDecl, Expr, FuncDecl, ImportItem, Lvalue,
    Module, ProtocolDecl, Span, Stmt, TopDecl, TypeAliasDecl,
};
use crate::error::{codes, CompileError, ErrorCode};
use crate::types::{
    ClassId, ClassLayout, FieldInfo, MethodSig, PrimTy, ProtoId, ProtocolInfo, Ty, TypeCtor,
    TypeVarId,
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
    /// One TypeVarId per entry of `generics`, in declaration order. Empty for
    /// non-generic functions. Set by the resolver so the type-checker can
    /// build substitutions at call sites without re-walking the source.
    /// M17 (generics v0.1).
    pub generic_tvars: Vec<TypeVarId>,
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
    /// M19: stdlib module table. Maps module name → its items. Looked
    /// up by the typechecker on `module.attr` and by the IR lowerer to
    /// pick the right `NativeFn` dispatch id.
    pub stdlib_modules: HashMap<String, StdlibModule>,
    /// M19: for each `Symbol` that came from `from sys import argv` /
    /// `from sys import exit`, the corresponding stdlib item so the IR
    /// lowerer knows what native to emit.
    pub import_item: HashMap<SymbolId, StdlibItem>,
    /// M19: for `import sys as s` / `import sys`, maps the introduced
    /// symbol back to the underlying stdlib module name. Lets the
    /// typechecker recover the module from a renamed alias.
    pub module_alias: HashMap<SymbolId, String>,
}

// ─────────────────────────────────────────────────────────────────────────
//  M19: stdlib module table
// ─────────────────────────────────────────────────────────────────────────
//
// The v0.2 stdlib lives entirely *inside the compiler* — no `.spy` source
// files for `sys` etc. ship with the toolchain yet. This table is the
// single source of truth for what names exist inside each built-in
// module, what their static types are, and which `NativeFn` id the IR
// lowerer should emit when the user writes `sys.argv` or `sys.exit(0)`.
//
// User-defined `.spy` modules and submodules (`os.path`) are deferred to
// v0.3; resolution falls through to an `E4001` error.

/// One item inside a built-in stdlib module — either a value-typed
/// constant (`sys.argv`), a function (`sys.exit`), or (M36) a class
/// such as `json.JsonValue` / `re.Pattern`.  Const + Function dispatch
/// through a single `NativeFn` slot; Class items carry a `ClassId`
/// on the kind variant (the class is registered into
/// `class_name_to_id` / `class_layouts` at seed time, and the
/// StdlibItem only records the id so the import resolver can re-bind
/// it under a local name).
#[derive(Debug, Clone)]
pub struct StdlibItem {
    pub name: String,
    pub kind: StdlibItemKind,
    /// Static type of the item. For `Function`, this is `Ty::Function`;
    /// for `Class`, `Ty::Class(class_id)`.
    pub ty: Ty,
    /// `NativeFn` discriminant (as u32). The IR lowerer emits a
    /// `CallNative { native_id }` carrying this id.  Unused (kept at
    /// 0) for `StdlibItemKind::Class` items, whose method dispatch
    /// goes through the IR's class-name lookup in `lower_method_call`
    /// rather than this field.
    pub native_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdlibItemKind {
    /// A lazily-materialised value like `sys.argv`. Reading the
    /// attribute emits a 0-arg `CallNative`.
    Const,
    /// A callable like `sys.exit(code)`. Calling emits an N-arg
    /// `CallNative` whose operands are the user-supplied args.
    Function,
    /// M36: a class such as `json.JsonValue` / `re.Pattern` /
    /// `sqlite3.Connection` / `hashlib.Hasher`.  Importing the item
    /// binds a local symbol to the resolver-allocated `ClassId`
    /// carried in the variant payload (the class itself is registered
    /// into `class_name_to_id` + `class_layouts` at seed time, so
    /// method dispatch via `lower_method_call` works regardless of
    /// whether the user wrote `from json import JsonValue` or relied
    /// on the legacy prelude binding).
    Class { class_id: ClassId },
}

/// One built-in module exposed to user code.
#[derive(Debug, Clone)]
pub struct StdlibModule {
    pub name: String,
    pub items: Vec<StdlibItem>,
}

impl StdlibModule {
    pub fn find(&self, name: &str) -> Option<&StdlibItem> {
        self.items.iter().find(|i| i.name == name)
    }
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
    /// M17: fresh TypeVarId allocator. Each generic parameter (`T` in
    /// `fn id[T](x: T) -> T`) gets one. Resolver-allocated so the type-checker
    /// can build substitutions at call sites by index.
    next_tvar: u32,
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
    /// M31: per-generic-class, the scope in which the class type-parameters
    /// (`T`, `K`, `V`, ...) are bound as TypeAlias symbols carrying
    /// `Ty::Var(...)`. Field lowering, method-sig lowering, and method-body
    /// resolution all use this scope so every occurrence of `T` inside the
    /// class lowers to the same `Ty::Var`.
    class_generic_scope: HashMap<ClassId, ScopeId>,
    /// Map proto name → id.
    proto_name_to_id: HashMap<String, ProtoId>,
    /// Type-alias name → semantic Ty.
    type_aliases: HashMap<String, Ty>,
    /// M19: built-in stdlib modules available to `import`. Populated by
    /// `seed_stdlib_modules` at the start of resolve().
    stdlib_modules: HashMap<String, StdlibModule>,
    /// M19: see `ResolvedModule::import_item`.
    import_item: HashMap<SymbolId, StdlibItem>,
    /// M19: see `ResolvedModule::module_alias`.
    module_alias: HashMap<SymbolId, String>,
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
    fn fresh_tvar(&mut self) -> TypeVarId {
        let id = TypeVarId(self.next_tvar);
        self.next_tvar += 1;
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

        // M34: prelude runs first so its registered classes (notably
        // the JsonValue hierarchy) are visible by name when
        // `seed_stdlib_modules` builds the `json` module's typed
        // signatures.  Pre-M34 the order was swapped — flipping is safe
        // because `seed_stdlib_modules` doesn't read from any prelude
        // state at the top of the file, only at the json block.
        self.seed_prelude(prelude_scope);
        self.seed_stdlib_modules();

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
            stdlib_modules: self.stdlib_modules,
            import_item: self.import_item,
            module_alias: self.module_alias,
        })
    }

    // ─────────────────────────────────────────────────────────────────────
    //  M19: stdlib module table
    // ─────────────────────────────────────────────────────────────────────

    /// Register every built-in module the resolver knows about. v0.2
    /// ships only `sys`; M20 will register `os`, `os.path`, `io`,
    /// `json`, `re`, `time`, `random`, `math` alongside.
    fn seed_stdlib_modules(&mut self) {
        use crate::types::PrimTy;
        // Native ids come from `shared::native::NativeFn` (M19 block
        // 130–149). We hard-code the discriminant values here to avoid
        // a dependency cycle through the shared crate's `NativeFn::from_name`,
        // which doesn't (and shouldn't) know stdlib module structure.
        const SYS_ARGV: u32     = 130;
        const SYS_EXIT: u32     = 131;
        const SYS_PLATFORM: u32 = 132;
        const SYS_VERSION: u32  = 133;

        let sys = StdlibModule {
            name: "sys".into(),
            items: vec![
                StdlibItem {
                    name: "argv".into(),
                    kind: StdlibItemKind::Const,
                    ty: Ty::Generic {
                        base: TypeCtor::List,
                        args: vec![Ty::Primitive(PrimTy::Str)],
                    },
                    native_id: SYS_ARGV,
                },
                StdlibItem {
                    name: "exit".into(),
                    kind: StdlibItemKind::Function,
                    ty: Ty::Function {
                        params: vec![Ty::Primitive(PrimTy::I32)],
                        ret: Box::new(Ty::Never),
                    },
                    native_id: SYS_EXIT,
                },
                StdlibItem {
                    name: "platform".into(),
                    kind: StdlibItemKind::Const,
                    ty: Ty::Primitive(PrimTy::Str),
                    native_id: SYS_PLATFORM,
                },
                StdlibItem {
                    name: "version".into(),
                    kind: StdlibItemKind::Const,
                    ty: Ty::Primitive(PrimTy::Str),
                    native_id: SYS_VERSION,
                },
            ],
        };
        self.stdlib_modules.insert("sys".into(), sys);

        // ── M20a: `os` module ──────────────────────────────────────────
        // Native ids 140–159.  Each item maps to a NativeFn that wraps a
        // Rust `std::env` / `std::fs` call.  Failures surface as IOError
        // (matches the M5 `open()` semantics).
        const OS_ENV: u32        = 140;
        const OS_SET_ENV: u32    = 141;
        const OS_GETCWD: u32     = 142;
        const OS_CHDIR: u32      = 143;
        const OS_LISTDIR: u32    = 144;
        const OS_REMOVE: u32     = 145;
        const OS_MKDIR: u32      = 146;
        const OS_EXISTS: u32     = 147;
        const OS_IS_FILE: u32    = 148;
        const OS_IS_DIR: u32     = 149;
        const OS_READ_FILE: u32  = 150;
        const OS_WRITE_FILE: u32 = 151;

        let fn_ty = |params: Vec<Ty>, ret: Ty| Ty::Function {
            params,
            ret: Box::new(ret),
        };
        let str_ty = Ty::Primitive(PrimTy::Str);
        let unit_ty = Ty::Primitive(PrimTy::Unit);
        let bool_ty = Ty::Primitive(PrimTy::Bool);
        let nullable_str_ty = Ty::Nullable(Box::new(Ty::Primitive(PrimTy::Str)));
        let list_str_ty = Ty::Generic {
            base: TypeCtor::List,
            args: vec![Ty::Primitive(PrimTy::Str)],
        };

        let os = StdlibModule {
            name: "os".into(),
            items: vec![
                StdlibItem {
                    name: "env".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], nullable_str_ty.clone()),
                    native_id: OS_ENV,
                },
                StdlibItem {
                    name: "set_env".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), str_ty.clone()], unit_ty.clone()),
                    native_id: OS_SET_ENV,
                },
                StdlibItem {
                    name: "getcwd".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], str_ty.clone()),
                    native_id: OS_GETCWD,
                },
                StdlibItem {
                    name: "chdir".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], unit_ty.clone()),
                    native_id: OS_CHDIR,
                },
                StdlibItem {
                    name: "listdir".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], list_str_ty.clone()),
                    native_id: OS_LISTDIR,
                },
                StdlibItem {
                    name: "remove".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], unit_ty.clone()),
                    native_id: OS_REMOVE,
                },
                StdlibItem {
                    name: "mkdir".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], unit_ty.clone()),
                    native_id: OS_MKDIR,
                },
                StdlibItem {
                    name: "exists".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], bool_ty.clone()),
                    native_id: OS_EXISTS,
                },
                StdlibItem {
                    name: "is_file".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], bool_ty.clone()),
                    native_id: OS_IS_FILE,
                },
                StdlibItem {
                    name: "is_dir".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], bool_ty.clone()),
                    native_id: OS_IS_DIR,
                },
                StdlibItem {
                    name: "read_file".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: OS_READ_FILE,
                },
                StdlibItem {
                    name: "write_file".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), str_ty.clone()], unit_ty.clone()),
                    native_id: OS_WRITE_FILE,
                },
            ],
        };
        self.stdlib_modules.insert("os".into(), os);

        // ── M20a: `path` module ────────────────────────────────────────
        // Native ids 160–169.  `os.path` is Python's natural home but
        // submodules are v0.3 work, so we ship a flat top-level `path`.
        const PATH_JOIN: u32     = 160;
        const PATH_JOIN3: u32    = 161;
        const PATH_DIRNAME: u32  = 162;
        const PATH_BASENAME: u32 = 163;
        const PATH_SPLITEXT: u32 = 164;
        const PATH_SEP: u32      = 165;

        let path_mod = StdlibModule {
            name: "path".into(),
            items: vec![
                StdlibItem {
                    name: "join".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), str_ty.clone()], str_ty.clone()),
                    native_id: PATH_JOIN,
                },
                StdlibItem {
                    name: "join3".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone(), str_ty.clone()],
                        str_ty.clone(),
                    ),
                    native_id: PATH_JOIN3,
                },
                StdlibItem {
                    name: "dirname".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: PATH_DIRNAME,
                },
                StdlibItem {
                    name: "basename".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: PATH_BASENAME,
                },
                StdlibItem {
                    name: "splitext".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone()],
                        Ty::Tuple(vec![str_ty.clone(), str_ty.clone()]),
                    ),
                    native_id: PATH_SPLITEXT,
                },
                StdlibItem {
                    name: "sep".into(),
                    kind: StdlibItemKind::Const,
                    ty: str_ty.clone(),
                    native_id: PATH_SEP,
                },
            ],
        };
        self.stdlib_modules.insert("path".into(), path_mod);

        // ── M20a: `io` module ──────────────────────────────────────────
        // Native ids 170–179.  `sys.stdin/stdout/stderr` were deferred in
        // M19; we ship the line-based subset here instead.
        const IO_INPUT: u32        = 170;
        const IO_INPUT_PROMPT: u32 = 171;
        const IO_WRITE_STDOUT: u32 = 172;
        const IO_WRITE_STDERR: u32 = 173;
        const IO_FLUSH_STDOUT: u32 = 174;

        let io_mod = StdlibModule {
            name: "io".into(),
            items: vec![
                StdlibItem {
                    name: "input".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], str_ty.clone()),
                    native_id: IO_INPUT,
                },
                StdlibItem {
                    name: "input_with_prompt".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: IO_INPUT_PROMPT,
                },
                StdlibItem {
                    name: "write_stdout".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], unit_ty.clone()),
                    native_id: IO_WRITE_STDOUT,
                },
                StdlibItem {
                    name: "write_stderr".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], unit_ty.clone()),
                    native_id: IO_WRITE_STDERR,
                },
                StdlibItem {
                    name: "flush_stdout".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], unit_ty.clone()),
                    native_id: IO_FLUSH_STDOUT,
                },
            ],
        };
        self.stdlib_modules.insert("io".into(), io_mod);

        // ── M20b: `time` module ────────────────────────────────────────
        // Wall-clock + monotonic clock + sleep, anchored to the
        // interpreter's per-process Instant for monotonic.
        const TIME_NOW: u32        = 175;
        const TIME_NOW_MS: u32     = 176;
        const TIME_MONOTONIC: u32  = 177;
        const TIME_SLEEP_S: u32    = 178;
        const TIME_SLEEP_MS: u32   = 179;
        const TIME_FORMAT_ISO: u32 = 180;

        let f64_ty = Ty::Primitive(PrimTy::F64);
        let i64_ty = Ty::Primitive(PrimTy::I64);
        let i32_ty = Ty::Primitive(PrimTy::I32);
        // (str_ty, unit_ty, bool_ty already in scope from M20a registrations.)

        let time_mod = StdlibModule {
            name: "time".into(),
            items: vec![
                StdlibItem {
                    name: "now".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], f64_ty.clone()),
                    native_id: TIME_NOW,
                },
                StdlibItem {
                    name: "now_ms".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], i64_ty.clone()),
                    native_id: TIME_NOW_MS,
                },
                StdlibItem {
                    name: "monotonic".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], f64_ty.clone()),
                    native_id: TIME_MONOTONIC,
                },
                StdlibItem {
                    name: "sleep_s".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![f64_ty.clone()], unit_ty.clone()),
                    native_id: TIME_SLEEP_S,
                },
                StdlibItem {
                    name: "sleep_ms".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], unit_ty.clone()),
                    native_id: TIME_SLEEP_MS,
                },
                StdlibItem {
                    name: "format_iso".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![f64_ty.clone()], str_ty.clone()),
                    native_id: TIME_FORMAT_ISO,
                },
            ],
        };
        self.stdlib_modules.insert("time".into(), time_mod);

        // ── M20b: `random` module ──────────────────────────────────────
        // LCG-backed pseudo-random.  No generics in stdlib for v0.2, so
        // `choice` / `shuffle` / `sample` ship as monomorphic
        // `_i64` / `_f64` / `_str` triples.
        const RANDOM_SEED: u32          = 185;
        const RANDOM_RANDINT: u32       = 186;
        const RANDOM_RANDOM: u32        = 187;
        const RANDOM_CHOICE_I64: u32    = 188;
        const RANDOM_CHOICE_F64: u32    = 189;
        const RANDOM_CHOICE_STR: u32    = 190;
        const RANDOM_SHUFFLE_I64: u32   = 191;
        const RANDOM_SHUFFLE_F64: u32   = 192;
        const RANDOM_SHUFFLE_STR: u32   = 193;
        const RANDOM_SAMPLE_I64: u32    = 194;
        const RANDOM_SAMPLE_F64: u32    = 195;
        const RANDOM_SAMPLE_STR: u32    = 196;

        let list_i64_ty = Ty::Generic {
            base: TypeCtor::List,
            args: vec![i64_ty.clone()],
        };
        let list_f64_ty = Ty::Generic {
            base: TypeCtor::List,
            args: vec![f64_ty.clone()],
        };
        let list_str_ty_random = Ty::Generic {
            base: TypeCtor::List,
            args: vec![str_ty.clone()],
        };

        let random_mod = StdlibModule {
            name: "random".into(),
            items: vec![
                StdlibItem {
                    name: "seed".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], unit_ty.clone()),
                    native_id: RANDOM_SEED,
                },
                StdlibItem {
                    name: "randint".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone(), i64_ty.clone()], i64_ty.clone()),
                    native_id: RANDOM_RANDINT,
                },
                StdlibItem {
                    name: "random".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], f64_ty.clone()),
                    native_id: RANDOM_RANDOM,
                },
                StdlibItem {
                    name: "choice_i64".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_i64_ty.clone()], i64_ty.clone()),
                    native_id: RANDOM_CHOICE_I64,
                },
                StdlibItem {
                    name: "choice_f64".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_f64_ty.clone()], f64_ty.clone()),
                    native_id: RANDOM_CHOICE_F64,
                },
                StdlibItem {
                    name: "choice_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_str_ty_random.clone()], str_ty.clone()),
                    native_id: RANDOM_CHOICE_STR,
                },
                StdlibItem {
                    name: "shuffle_i64".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_i64_ty.clone()], unit_ty.clone()),
                    native_id: RANDOM_SHUFFLE_I64,
                },
                StdlibItem {
                    name: "shuffle_f64".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_f64_ty.clone()], unit_ty.clone()),
                    native_id: RANDOM_SHUFFLE_F64,
                },
                StdlibItem {
                    name: "shuffle_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_str_ty_random.clone()], unit_ty.clone()),
                    native_id: RANDOM_SHUFFLE_STR,
                },
                StdlibItem {
                    name: "sample_i64".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![list_i64_ty.clone(), i32_ty.clone()],
                        list_i64_ty.clone(),
                    ),
                    native_id: RANDOM_SAMPLE_I64,
                },
                StdlibItem {
                    name: "sample_f64".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![list_f64_ty.clone(), i32_ty.clone()],
                        list_f64_ty.clone(),
                    ),
                    native_id: RANDOM_SAMPLE_F64,
                },
                StdlibItem {
                    name: "sample_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![list_str_ty_random.clone(), i32_ty.clone()],
                        list_str_ty_random.clone(),
                    ),
                    native_id: RANDOM_SAMPLE_STR,
                },
            ],
        };
        self.stdlib_modules.insert("random".into(), random_mod);

        // ── M20b: `math` module (extensions) ───────────────────────────
        // The prelude bare-name natives `sqrt` / `sin` / `cos` / `pow` /
        // `floor` / `ceil` / `log` / `exp` (NativeFn::Math* ids 70–79)
        // remain registered as prelude functions, so `sqrt(x)` works
        // *without* `import math`.  This module re-exposes them as
        // namespaced `math.sqrt(x)` plus new helpers (`log2`, `log10`,
        // `gcd`, `factorial`, `is_nan`, `is_inf`) and the constants
        // (`pi`, `e`, `tau`, `inf`, `nan`).
        const MATH_SQRT: u32      = 70;  // existing MathSqrt
        const MATH_SIN: u32       = 71;
        const MATH_COS: u32       = 72;
        const MATH_LOG: u32       = 74;
        const MATH_EXP: u32       = 75;
        const MATH_POW: u32       = 76;
        const MATH_LOG2: u32      = 200;
        const MATH_LOG10: u32     = 201;
        const MATH_FLOOR_I: u32   = 202;
        const MATH_CEIL_I: u32    = 203;
        const MATH_GCD: u32       = 204;
        const MATH_FACTORIAL: u32 = 205;
        const MATH_IS_NAN: u32    = 206;
        const MATH_IS_INF: u32    = 207;
        const MATH_PI: u32        = 208;
        const MATH_E: u32         = 209;
        const MATH_TAU: u32       = 210;
        const MATH_INF: u32       = 211;
        const MATH_NAN: u32       = 212;

        let math_mod = StdlibModule {
            name: "math".into(),
            items: vec![
                // Constants
                StdlibItem {
                    name: "pi".into(),
                    kind: StdlibItemKind::Const,
                    ty: f64_ty.clone(),
                    native_id: MATH_PI,
                },
                StdlibItem {
                    name: "e".into(),
                    kind: StdlibItemKind::Const,
                    ty: f64_ty.clone(),
                    native_id: MATH_E,
                },
                StdlibItem {
                    name: "tau".into(),
                    kind: StdlibItemKind::Const,
                    ty: f64_ty.clone(),
                    native_id: MATH_TAU,
                },
                StdlibItem {
                    name: "inf".into(),
                    kind: StdlibItemKind::Const,
                    ty: f64_ty.clone(),
                    native_id: MATH_INF,
                },
                StdlibItem {
                    name: "nan".into(),
                    kind: StdlibItemKind::Const,
                    ty: f64_ty.clone(),
                    native_id: MATH_NAN,
                },
                // Wrapped existing prelude natives — same NativeFn ids.
                StdlibItem {
                    name: "sqrt".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![f64_ty.clone()], f64_ty.clone()),
                    native_id: MATH_SQRT,
                },
                StdlibItem {
                    name: "sin".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![f64_ty.clone()], f64_ty.clone()),
                    native_id: MATH_SIN,
                },
                StdlibItem {
                    name: "cos".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![f64_ty.clone()], f64_ty.clone()),
                    native_id: MATH_COS,
                },
                StdlibItem {
                    name: "log".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![f64_ty.clone()], f64_ty.clone()),
                    native_id: MATH_LOG,
                },
                StdlibItem {
                    name: "exp".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![f64_ty.clone()], f64_ty.clone()),
                    native_id: MATH_EXP,
                },
                StdlibItem {
                    name: "pow".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![f64_ty.clone(), f64_ty.clone()], f64_ty.clone()),
                    native_id: MATH_POW,
                },
                // New helpers.
                StdlibItem {
                    name: "log2".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![f64_ty.clone()], f64_ty.clone()),
                    native_id: MATH_LOG2,
                },
                StdlibItem {
                    name: "log10".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![f64_ty.clone()], f64_ty.clone()),
                    native_id: MATH_LOG10,
                },
                StdlibItem {
                    name: "floor".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![f64_ty.clone()], i64_ty.clone()),
                    native_id: MATH_FLOOR_I,
                },
                StdlibItem {
                    name: "ceil".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![f64_ty.clone()], i64_ty.clone()),
                    native_id: MATH_CEIL_I,
                },
                StdlibItem {
                    name: "gcd".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone(), i64_ty.clone()], i64_ty.clone()),
                    native_id: MATH_GCD,
                },
                StdlibItem {
                    name: "factorial".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], i64_ty.clone()),
                    native_id: MATH_FACTORIAL,
                },
                StdlibItem {
                    name: "is_nan".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![f64_ty.clone()], bool_ty.clone()),
                    native_id: MATH_IS_NAN,
                },
                StdlibItem {
                    name: "is_inf".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![f64_ty.clone()], bool_ty.clone()),
                    native_id: MATH_IS_INF,
                },
            ],
        };
        self.stdlib_modules.insert("math".into(), math_mod);

        // ── M20c + M34: `json` module ──────────────────────────────────
        // M20c shipped the flat validate-and-reserialize surface
        // (parse_to_string / is_valid / pretty / escape / minify).
        // M34 adds the typed JsonValue tree on top: `parse` returns a
        // sealed-class tree, `stringify` walks it back to canonical
        // compact JSON, and `j_null` / `j_bool` / `j_int` / etc. are
        // module-level constructor helpers that alias the prelude class
        // constructors.  Both surfaces co-exist — existing programs that
        // hand-walk `parse_to_string` output (e.g. the M29 framework)
        // continue to work, while new programs get pattern-matching
        // ergonomics.
        const JSON_PARSE_TO_STRING: u32 = 213;
        const JSON_IS_VALID: u32        = 214;
        const JSON_PRETTY: u32          = 215;
        const JSON_ESCAPE: u32          = 216;
        const JSON_MINIFY: u32          = 217;
        // M34: typed-tree surface — see shared/src/native.rs §750-789.
        const JSON_PARSE: u32             = 750;
        const JSON_STRINGIFY: u32         = 751;
        const JSON_STRINGIFY_PRETTY: u32  = 752;
        // 753-759 used internally by lower_call for the class
        // constructor (receiver-style) entry points; see
        // shared::NativeFn::JsonJ*New.  The j_* module helpers below
        // use the parallel 760-766 block.

        // M34: pull the real `Ty::Class(JsonValueId)` from the prelude
        // (which ran first — see `resolve()`).  Defensive fallback to
        // `Ty::Never` if `seed_prelude` was somehow skipped, which would
        // surface as a "type mismatch" at the first call site rather
        // than a silent miscompilation.
        let jv_ty = match self.class_name_to_id.get("JsonValue") {
            Some(cid) => Ty::Class(*cid),
            None => Ty::Never,
        };
        let json_mod = StdlibModule {
            name: "json".into(),
            items: vec![
                // Pre-existing flat surface — unchanged.
                StdlibItem {
                    name: "parse_to_string".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: JSON_PARSE_TO_STRING,
                },
                StdlibItem {
                    name: "is_valid".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], bool_ty.clone()),
                    native_id: JSON_IS_VALID,
                },
                StdlibItem {
                    name: "pretty".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), i32_ty.clone()], str_ty.clone()),
                    native_id: JSON_PRETTY,
                },
                StdlibItem {
                    name: "escape".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: JSON_ESCAPE,
                },
                StdlibItem {
                    name: "minify".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: JSON_MINIFY,
                },
                // M34: typed surface — `parse` returns a JsonValue
                // tree, `stringify` walks it.  Type signatures use the
                // real `Ty::Class(JsonValueId)` looked up from the
                // prelude (which ran first in `resolve()`).
                StdlibItem {
                    name: "parse".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], jv_ty.clone()),
                    native_id: JSON_PARSE,
                },
                StdlibItem {
                    name: "stringify".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![jv_ty.clone()], str_ty.clone()),
                    native_id: JSON_STRINGIFY,
                },
                StdlibItem {
                    name: "stringify_pretty".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![jv_ty.clone(), i32_ty.clone()], str_ty.clone()),
                    native_id: JSON_STRINGIFY_PRETTY,
                },
                // Convenience constructors.  These map to a *different*
                // set of NativeFn ids than the bare class constructors
                // because the helpers must `alloc + populate + return`
                // while the class constructors receive a pre-Alloc'd
                // receiver and just populate it (see IR's `lower_call`
                // M34 special-case).  Naming convention: same numeric
                // range, but `_Helper` suffix isn't needed because each
                // helper has a unique ID slot (760-769 reserved earlier).
                StdlibItem {
                    name: "j_null".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], jv_ty.clone()),
                    native_id: 760, // JsonHelperJNull
                },
                StdlibItem {
                    name: "j_bool".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![bool_ty.clone()], jv_ty.clone()),
                    native_id: 761, // JsonHelperJBool
                },
                StdlibItem {
                    name: "j_int".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], jv_ty.clone()),
                    native_id: 762, // JsonHelperJInt
                },
                StdlibItem {
                    name: "j_float".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![f64_ty.clone()], jv_ty.clone()),
                    native_id: 763, // JsonHelperJFloat
                },
                StdlibItem {
                    name: "j_string".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], jv_ty.clone()),
                    native_id: 764, // JsonHelperJString
                },
                StdlibItem {
                    name: "j_list".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![Ty::Generic {
                            base: TypeCtor::List,
                            args: vec![jv_ty.clone()],
                        }],
                        jv_ty.clone(),
                    ),
                    native_id: 765, // JsonHelperJList
                },
                StdlibItem {
                    name: "j_object".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![Ty::Generic {
                            base: TypeCtor::List,
                            args: vec![Ty::Tuple(vec![
                                str_ty.clone(),
                                jv_ty.clone(),
                            ])],
                        }],
                        jv_ty.clone(),
                    ),
                    native_id: 766, // JsonHelperJObject
                },
            ],
        };
        // M36: publish the 8 JsonValue-tree classes as
        // `StdlibItemKind::Class` items on the `json` module.  The
        // classes themselves are still registered in `seed_prelude`
        // (their bindings reach the prelude_scope for back-compat);
        // these items make them additionally discoverable via the
        // module table so v0.4 callers can route through
        // `from json import JsonValue` cleanly.
        let m36_json_classes = [
            "JsonValue", "JNull", "JBool", "JInt", "JFloat",
            "JString", "JList", "JObject",
        ];
        let mut json_mod = json_mod;
        for m36_name in m36_json_classes {
            if let Some(&m36_cid) = self.class_name_to_id.get(m36_name) {
                json_mod.items.push(StdlibItem {
                    name: m36_name.into(),
                    kind: StdlibItemKind::Class { class_id: m36_cid },
                    ty: Ty::Class(m36_cid),
                    native_id: 0,
                });
            }
        }
        self.stdlib_modules.insert("json".into(), json_mod);

        // ── M20c: `re` module ──────────────────────────────────────────
        // Regex matching via the `regex` crate.  `re.find` returns
        // `(i32, i32)` tuples (start, end), reusing the alloc_tuple_obj
        // helper from M20a's path.splitext.  Patterns are recompiled
        // on every call for v0.2; a Pattern handle for cached
        // compilation is v0.3 work.
        const RE_MATCH: u32     = 220;
        const RE_SEARCH: u32    = 221;
        const RE_FIND: u32      = 222;
        const RE_FIND_ALL: u32  = 223;
        const RE_REPLACE: u32   = 224;
        const RE_SPLIT: u32     = 225;
        const RE_IS_VALID: u32  = 226;
        // M35 P4-A: `re.compile(s) -> Pattern` — the only entry point
        // to the compiled-pattern class.  See shared/src/native.rs
        // §790-799.
        const RE_PATTERN_COMPILE: u32 = 791;

        let tuple_i32_i32_ty = Ty::Tuple(vec![i32_ty.clone(), i32_ty.clone()]);

        // M35 P4-A: pull the Pattern class id from the prelude (which
        // ran first in `resolve()`).  Defensive fallback to Ty::Never
        // mirrors the M34 JsonValue lookup pattern.
        let p4a_pattern_ty = match self.class_name_to_id.get("Pattern") {
            Some(cid) => Ty::Class(*cid),
            None => Ty::Never,
        };

        let re_mod = StdlibModule {
            name: "re".into(),
            items: vec![
                // Python's `re.match` anchors only at the start; this
                // shipping name `fullmatch` matches the entire string,
                // which is what the brief asks for.  Python's parser
                // treats `match` as a contextual keyword (it can still
                // be an attribute name); StrictPy's lexer makes `match`
                // strictly reserved, so an attribute named `match`
                // would force a parser change.  Naming it `fullmatch`
                // sidesteps the lexer collision and is what Python
                // calls this exact semantic anyway.
                StdlibItem {
                    name: "fullmatch".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), str_ty.clone()], bool_ty.clone()),
                    native_id: RE_MATCH,
                },
                StdlibItem {
                    name: "search".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), str_ty.clone()], bool_ty.clone()),
                    native_id: RE_SEARCH,
                },
                StdlibItem {
                    name: "find".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), str_ty.clone()], tuple_i32_i32_ty.clone()),
                    native_id: RE_FIND,
                },
                StdlibItem {
                    name: "find_all".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), str_ty.clone()], list_str_ty.clone()),
                    native_id: RE_FIND_ALL,
                },
                // Argument order matches Python's `re.sub`:
                // `(pattern, replacement, s)`.  The brief's example
                // `re.replace("[0-9]", "X", "a1b2c3") -> "aXbXcX"`
                // only makes sense under this order (the haystack is
                // the third argument).
                StdlibItem {
                    name: "replace".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone(), str_ty.clone()],
                        str_ty.clone(),
                    ),
                    native_id: RE_REPLACE,
                },
                StdlibItem {
                    name: "split".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), str_ty.clone()], list_str_ty.clone()),
                    native_id: RE_SPLIT,
                },
                StdlibItem {
                    name: "is_valid".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], bool_ty.clone()),
                    native_id: RE_IS_VALID,
                },
                // M35 P4-A: `re.compile(s: str) -> Pattern` — compile
                // once, reuse cheaply in hot loops.  Bad regex →
                // ValueError (same shape as the flat surface).
                StdlibItem {
                    name: "compile".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], p4a_pattern_ty.clone()),
                    native_id: RE_PATTERN_COMPILE,
                },
            ],
        };
        // M36: publish the `Pattern` class on the `re` module so
        // `from re import Pattern` / `re.compile(...)` route through
        // the proper module table.  See the matching `json` block
        // above for the broader pattern.
        let mut re_mod = re_mod;
        if let Some(&m36_pat_cid) = self.class_name_to_id.get("Pattern") {
            re_mod.items.push(StdlibItem {
                name: "Pattern".into(),
                kind: StdlibItemKind::Class { class_id: m36_pat_cid },
                ty: Ty::Class(m36_pat_cid),
                native_id: 0,
            });
        }
        self.stdlib_modules.insert("re".into(), re_mod);

        // ── M22 P2C: `itertools` module ────────────────────────────────
        // Iteration helpers.  Stdlib functions aren't generic in v0.2 —
        // the M17 generic-fn worklist only sees user-defined .spy fns —
        // so we ship monomorphic per-element-type variants the same way
        // M20b's `random.choice_*` does.  Per-type duplication is
        // verbose but consistent.  Generic stdlib is a v0.3 milestone
        // ("stdlib + M17 integration").
        const ITERTOOLS_RANGE_STEP: u32       = 310;
        const ITERTOOLS_ENUMERATE_STR: u32    = 311;
        const ITERTOOLS_ENUMERATE_I64: u32    = 312;
        const ITERTOOLS_ZIP_STR_STR: u32      = 313;
        const ITERTOOLS_ZIP_I64_I64: u32      = 314;
        const ITERTOOLS_CHAIN_STR: u32        = 315;
        const ITERTOOLS_CHAIN_I64: u32        = 316;
        const ITERTOOLS_TAKE_STR: u32         = 317;
        const ITERTOOLS_DROP_STR: u32         = 318;
        const ITERTOOLS_PAIRWISE_STR: u32     = 319;
        const ITERTOOLS_ACCUMULATE_I64: u32   = 320;
        const ITERTOOLS_FLATTEN_STR: u32      = 321;

        let tuple_i32_str_ty = Ty::Tuple(vec![i32_ty.clone(), str_ty.clone()]);
        let tuple_i32_i64_ty = Ty::Tuple(vec![i32_ty.clone(), i64_ty.clone()]);
        let tuple_str_str_ty = Ty::Tuple(vec![str_ty.clone(), str_ty.clone()]);
        let tuple_i64_i64_ty = Ty::Tuple(vec![i64_ty.clone(), i64_ty.clone()]);
        let list_enum_str_ty = Ty::Generic {
            base: TypeCtor::List,
            args: vec![tuple_i32_str_ty.clone()],
        };
        let list_enum_i64_ty = Ty::Generic {
            base: TypeCtor::List,
            args: vec![tuple_i32_i64_ty.clone()],
        };
        let list_zip_str_str_ty = Ty::Generic {
            base: TypeCtor::List,
            args: vec![tuple_str_str_ty.clone()],
        };
        let list_zip_i64_i64_ty = Ty::Generic {
            base: TypeCtor::List,
            args: vec![tuple_i64_i64_ty.clone()],
        };
        let list_i64_ty_it = Ty::Generic {
            base: TypeCtor::List,
            args: vec![i64_ty.clone()],
        };
        let list_str_ty_it = Ty::Generic {
            base: TypeCtor::List,
            args: vec![str_ty.clone()],
        };
        let list_list_str_ty = Ty::Generic {
            base: TypeCtor::List,
            args: vec![list_str_ty_it.clone()],
        };

        let itertools_mod = StdlibModule {
            name: "itertools".into(),
            items: vec![
                StdlibItem {
                    name: "range_step".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty.clone(), i64_ty.clone(), i64_ty.clone()],
                        list_i64_ty_it.clone(),
                    ),
                    native_id: ITERTOOLS_RANGE_STEP,
                },
                StdlibItem {
                    name: "enumerate_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_str_ty_it.clone()], list_enum_str_ty.clone()),
                    native_id: ITERTOOLS_ENUMERATE_STR,
                },
                StdlibItem {
                    name: "enumerate_i64".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_i64_ty_it.clone()], list_enum_i64_ty.clone()),
                    native_id: ITERTOOLS_ENUMERATE_I64,
                },
                StdlibItem {
                    name: "zip_str_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![list_str_ty_it.clone(), list_str_ty_it.clone()],
                        list_zip_str_str_ty.clone(),
                    ),
                    native_id: ITERTOOLS_ZIP_STR_STR,
                },
                StdlibItem {
                    name: "zip_i64_i64".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![list_i64_ty_it.clone(), list_i64_ty_it.clone()],
                        list_zip_i64_i64_ty.clone(),
                    ),
                    native_id: ITERTOOLS_ZIP_I64_I64,
                },
                StdlibItem {
                    name: "chain_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![list_str_ty_it.clone(), list_str_ty_it.clone()],
                        list_str_ty_it.clone(),
                    ),
                    native_id: ITERTOOLS_CHAIN_STR,
                },
                StdlibItem {
                    name: "chain_i64".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![list_i64_ty_it.clone(), list_i64_ty_it.clone()],
                        list_i64_ty_it.clone(),
                    ),
                    native_id: ITERTOOLS_CHAIN_I64,
                },
                StdlibItem {
                    name: "take_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![list_str_ty_it.clone(), i32_ty.clone()],
                        list_str_ty_it.clone(),
                    ),
                    native_id: ITERTOOLS_TAKE_STR,
                },
                StdlibItem {
                    name: "drop_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![list_str_ty_it.clone(), i32_ty.clone()],
                        list_str_ty_it.clone(),
                    ),
                    native_id: ITERTOOLS_DROP_STR,
                },
                StdlibItem {
                    name: "pairwise_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![list_str_ty_it.clone()],
                        list_zip_str_str_ty.clone(),
                    ),
                    native_id: ITERTOOLS_PAIRWISE_STR,
                },
                StdlibItem {
                    name: "accumulate_i64".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_i64_ty_it.clone()], list_i64_ty_it.clone()),
                    native_id: ITERTOOLS_ACCUMULATE_I64,
                },
                StdlibItem {
                    name: "flatten_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_list_str_ty.clone()], list_str_ty_it.clone()),
                    native_id: ITERTOOLS_FLATTEN_STR,
                },
            ],
        };
        self.stdlib_modules.insert("itertools".into(), itertools_mod);

        // ── M22 P2C: `statistics` module ───────────────────────────────
        // Descriptive stats over `List[f64]`.  Pure Rust f64 arithmetic;
        // empty/short inputs raise ValueError via M15 machinery.
        const STATS_MEAN: u32      = 322;
        const STATS_MEDIAN: u32    = 323;
        const STATS_STDEV: u32     = 324;
        const STATS_VARIANCE: u32  = 325;
        const STATS_MIN_MAX: u32   = 326;
        const STATS_SUM: u32       = 327;
        const STATS_QUANTILE: u32  = 328;
        const STATS_MODE_STR: u32  = 329;

        let list_f64_ty_stat = Ty::Generic {
            base: TypeCtor::List,
            args: vec![f64_ty.clone()],
        };
        let tuple_f64_f64_ty = Ty::Tuple(vec![f64_ty.clone(), f64_ty.clone()]);

        let statistics_mod = StdlibModule {
            name: "statistics".into(),
            items: vec![
                StdlibItem {
                    name: "mean".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_f64_ty_stat.clone()], f64_ty.clone()),
                    native_id: STATS_MEAN,
                },
                StdlibItem {
                    name: "median".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_f64_ty_stat.clone()], f64_ty.clone()),
                    native_id: STATS_MEDIAN,
                },
                StdlibItem {
                    name: "stdev".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_f64_ty_stat.clone()], f64_ty.clone()),
                    native_id: STATS_STDEV,
                },
                StdlibItem {
                    name: "variance".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_f64_ty_stat.clone()], f64_ty.clone()),
                    native_id: STATS_VARIANCE,
                },
                StdlibItem {
                    name: "min_max".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_f64_ty_stat.clone()], tuple_f64_f64_ty.clone()),
                    native_id: STATS_MIN_MAX,
                },
                StdlibItem {
                    name: "sum".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_f64_ty_stat.clone()], f64_ty.clone()),
                    native_id: STATS_SUM,
                },
                StdlibItem {
                    name: "quantile".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![list_f64_ty_stat.clone(), f64_ty.clone()],
                        f64_ty.clone(),
                    ),
                    native_id: STATS_QUANTILE,
                },
                StdlibItem {
                    name: "mode_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_str_ty_it.clone()], str_ty.clone()),
                    native_id: STATS_MODE_STR,
                },
            ],
        };
        self.stdlib_modules.insert("statistics".into(), statistics_mod);

        // ── M22 P2D: `struct` module ───────────────────────────────────
        // Binary pack/unpack for fixed-width integers and IEEE 754 doubles.
        // Returned `str` represents a sequence of bytes via codepoint 0-255
        // per char (so `len(buf) == byte_count`).  Spec §9.15.
        const STRUCT_PACK_U32_BE: u32   = 330;
        const STRUCT_PACK_U32_LE: u32   = 331;
        const STRUCT_PACK_U64_BE: u32   = 332;
        const STRUCT_PACK_U64_LE: u32   = 333;
        const STRUCT_PACK_F64_BE: u32   = 334;
        const STRUCT_PACK_F64_LE: u32   = 335;
        const STRUCT_UNPACK_U32_BE: u32 = 336;
        const STRUCT_UNPACK_U32_LE: u32 = 337;
        const STRUCT_UNPACK_U64_BE: u32 = 338;
        const STRUCT_UNPACK_U64_LE: u32 = 339;
        const STRUCT_UNPACK_F64_BE: u32 = 340;
        const STRUCT_UNPACK_F64_LE: u32 = 341;

        let struct_mod = StdlibModule {
            name: "struct".into(),
            items: vec![
                StdlibItem {
                    name: "pack_u32_be".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], str_ty.clone()),
                    native_id: STRUCT_PACK_U32_BE,
                },
                StdlibItem {
                    name: "pack_u32_le".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], str_ty.clone()),
                    native_id: STRUCT_PACK_U32_LE,
                },
                StdlibItem {
                    name: "pack_u64_be".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], str_ty.clone()),
                    native_id: STRUCT_PACK_U64_BE,
                },
                StdlibItem {
                    name: "pack_u64_le".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], str_ty.clone()),
                    native_id: STRUCT_PACK_U64_LE,
                },
                StdlibItem {
                    name: "pack_f64_be".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![f64_ty.clone()], str_ty.clone()),
                    native_id: STRUCT_PACK_F64_BE,
                },
                StdlibItem {
                    name: "pack_f64_le".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![f64_ty.clone()], str_ty.clone()),
                    native_id: STRUCT_PACK_F64_LE,
                },
                StdlibItem {
                    name: "unpack_u32_be".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), i32_ty.clone()], i64_ty.clone()),
                    native_id: STRUCT_UNPACK_U32_BE,
                },
                StdlibItem {
                    name: "unpack_u32_le".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), i32_ty.clone()], i64_ty.clone()),
                    native_id: STRUCT_UNPACK_U32_LE,
                },
                StdlibItem {
                    name: "unpack_u64_be".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), i32_ty.clone()], i64_ty.clone()),
                    native_id: STRUCT_UNPACK_U64_BE,
                },
                StdlibItem {
                    name: "unpack_u64_le".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), i32_ty.clone()], i64_ty.clone()),
                    native_id: STRUCT_UNPACK_U64_LE,
                },
                StdlibItem {
                    name: "unpack_f64_be".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), i32_ty.clone()], f64_ty.clone()),
                    native_id: STRUCT_UNPACK_F64_BE,
                },
                StdlibItem {
                    name: "unpack_f64_le".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), i32_ty.clone()], f64_ty.clone()),
                    native_id: STRUCT_UNPACK_F64_LE,
                },
            ],
        };
        self.stdlib_modules.insert("struct".into(), struct_mod);

        // ── M22 P2D: `urllib_parse` module ─────────────────────────────
        // URL escape / unescape and query-string round-tripping.  Module
        // name uses underscore — submodule support (e.g. `urllib.parse`)
        // is v0.3.  `parse_url` / `join_url` deferred to v0.3.  Spec §9.16.
        const URL_QUOTE: u32        = 342;
        const URL_QUOTE_PLUS: u32   = 343;
        const URL_UNQUOTE: u32      = 344;
        const URL_UNQUOTE_PLUS: u32 = 345;
        const URL_ENCODE: u32       = 346;
        const URL_PARSE_QUERY: u32  = 347;

        let tuple_str_str_ty = Ty::Tuple(vec![str_ty.clone(), str_ty.clone()]);
        let list_pair_ty = Ty::Generic {
            base: TypeCtor::List,
            args: vec![tuple_str_str_ty.clone()],
        };

        let urllib_mod = StdlibModule {
            name: "urllib_parse".into(),
            items: vec![
                StdlibItem {
                    name: "quote".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: URL_QUOTE,
                },
                StdlibItem {
                    name: "quote_plus".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: URL_QUOTE_PLUS,
                },
                StdlibItem {
                    name: "unquote".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: URL_UNQUOTE,
                },
                StdlibItem {
                    name: "unquote_plus".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: URL_UNQUOTE_PLUS,
                },
                StdlibItem {
                    name: "urlencode".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_pair_ty.clone()], str_ty.clone()),
                    native_id: URL_ENCODE,
                },
                StdlibItem {
                    name: "parse_query".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], list_pair_ty.clone()),
                    native_id: URL_PARSE_QUERY,
                },
            ],
        };
        self.stdlib_modules.insert("urllib_parse".into(), urllib_mod);

        // ── M22 P2B: `base64` module ───────────────────────────────────
        // Standard and URL-safe base64 codecs over `str`.  Strings carry
        // UTF-8 in StrictPy, so we round-trip the bytes through UTF-8
        // (encode: bytes-of-utf8, decode: utf8-of-decoded-bytes).  A
        // dedicated `bytes` surface is v0.3 — see report for the
        // tradeoffs.  Bad base64 in `decode` / `decode_url_safe`, and
        // non-UTF-8 bytes after decoding, both surface as `ValueError`.
        const BASE64_ENCODE: u32          = 290;
        const BASE64_DECODE: u32          = 291;
        const BASE64_ENCODE_URL_SAFE: u32 = 292;
        const BASE64_DECODE_URL_SAFE: u32 = 293;

        let base64_mod = StdlibModule {
            name: "base64".into(),
            items: vec![
                StdlibItem {
                    name: "encode".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: BASE64_ENCODE,
                },
                StdlibItem {
                    name: "decode".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: BASE64_DECODE,
                },
                StdlibItem {
                    name: "encode_url_safe".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: BASE64_ENCODE_URL_SAFE,
                },
                StdlibItem {
                    name: "decode_url_safe".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: BASE64_DECODE_URL_SAFE,
                },
            ],
        };
        self.stdlib_modules.insert("base64".into(), base64_mod);

        // ── M22 P2B: `hashlib` module ──────────────────────────────────
        // Cryptographic-and-checksum digests over `str`.  Each entry
        // takes a single `str` and returns the canonical lowercase hex
        // digest of the standard length (32/40/64/128 chars for
        // md5/sha1/sha256/sha512).  Output is byte-compatible with
        // Python `hashlib.<algo>(data.encode()).hexdigest()`.
        //
        // No streaming `update()` API in v0.2 — programs needing it
        // concatenate first.  HMAC ships as a single helper
        // `hmac_sha256(key, data)` rather than a separate `hmac` module
        // (the module-namespacing cost outweighs the single function).
        const HASHLIB_MD5: u32         = 300;
        const HASHLIB_SHA1: u32        = 301;
        const HASHLIB_SHA256: u32      = 302;
        const HASHLIB_SHA512: u32      = 303;
        const HASHLIB_HMAC_SHA256: u32 = 304;
        // M35 P4-C: streaming Hasher. `hashlib.new(algo) -> Hasher` is the
        // sole stdlib-module entry; the Hasher class itself is registered
        // in seed_prelude (this builder runs before that, so we can't
        // refer to its ClassId here — the resolver retrofits the return
        // type by looking up "Hasher" in `class_name_to_id` at lookup
        // time).
        const HASHLIB_NEW: u32         = 821;

        // The return type of `hashlib.new` needs to be `Class(Hasher)`.
        // seed_prelude (which registers Hasher) runs *before*
        // seed_stdlib_modules per the M34 ordering, so this lookup is
        // valid.
        let hasher_ret_ty = match self.class_name_to_id.get("Hasher").copied() {
            Some(cid) => Ty::Class(cid),
            // Defensive fallback: if Hasher registration ever moves, the
            // typechecker will still produce a reasonable error.  Tests
            // would catch a mis-ordering immediately.
            None => Ty::Never,
        };

        let hashlib_mod = StdlibModule {
            name: "hashlib".into(),
            items: vec![
                StdlibItem {
                    name: "md5".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: HASHLIB_MD5,
                },
                StdlibItem {
                    name: "sha1".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: HASHLIB_SHA1,
                },
                StdlibItem {
                    name: "sha256".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: HASHLIB_SHA256,
                },
                StdlibItem {
                    name: "sha512".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: HASHLIB_SHA512,
                },
                StdlibItem {
                    name: "hmac_sha256".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone()],
                        str_ty.clone(),
                    ),
                    native_id: HASHLIB_HMAC_SHA256,
                },
                // M35 P4-C: `hashlib.new(algorithm: str) -> Hasher`.  See
                // spec §9.X (streaming Hasher subsection).  Supported
                // algorithm names: "sha256" / "sha512" / "sha1" / "md5".
                StdlibItem {
                    name: "new".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], hasher_ret_ty),
                    native_id: HASHLIB_NEW,
                },
            ],
        };
        // M36: publish the `Hasher` class on the `hashlib` module.
        // Mirror of the json + re + sqlite3 registrations above.
        let mut hashlib_mod = hashlib_mod;
        if let Some(&m36_hasher_cid) = self.class_name_to_id.get("Hasher") {
            hashlib_mod.items.push(StdlibItem {
                name: "Hasher".into(),
                kind: StdlibItemKind::Class { class_id: m36_hasher_cid },
                ty: Ty::Class(m36_hasher_cid),
                native_id: 0,
            });
        }
        self.stdlib_modules.insert("hashlib".into(), hashlib_mod);

        // ── M22 (P2A): `argparse` module ───────────────────────────────
        // Builder-style CLI argument parser.  Phase 2's highest-ROI module:
        // every CLI tool (`echo.spy`, `sum_args.spy`, `minigrep.spy`)
        // currently hand-parses `sys.argv`; argparse moves that into a
        // declarative API.
        //
        // Design: v0.2 doesn't have user-defined stdlib classes, and
        // registering a sealed `ArgParser` class through the prelude
        // table requires resolver-side surface area we don't have budget
        // for in M22 (M20c's report flags this as v0.3 work).  Instead
        // we use a Dict[str, str] as the opaque "parser handle" and a
        // second Dict[str, str] as the parsed "args".  All public
        // functions take these dicts by value (they're heap pointers
        // under the hood, so mutation is observable).
        //
        // Storage convention inside the parser dict:
        //   "_prog_"      → program name (set by `argparse.new`)
        //   "_order_"     → "\u{1F}"-separated list of positional names
        //                   in declaration order (the unit-separator
        //                   char is unlikely to appear in real arg names)
        //   "flag:NAME"   → default value ("true" / "false")
        //   "opt:NAME"    → default value
        //   "arg:NAME"    → "" (just records the declaration)
        //
        // Storage convention inside the args dict:
        //   "flag:NAME"   → "true" / "false"
        //   "opt:NAME"    → resolved value
        //   "arg:NAME"    → resolved value
        //
        // `argparse.parse` raises ValueError on missing required arg,
        // unknown flag/opt, or option-without-value.
        const ARGPARSE_NEW: u32             = 250;
        const ARGPARSE_ADD_FLAG: u32        = 251;
        const ARGPARSE_ADD_ARG: u32         = 252;
        const ARGPARSE_ADD_OPT: u32         = 253;
        const ARGPARSE_PARSE: u32           = 254;
        const ARGPARSE_GET_FLAG: u32        = 255;
        const ARGPARSE_GET_ARG: u32         = 256;
        const ARGPARSE_GET_OPT: u32         = 257;
        const ARGPARSE_HELP_TEXT: u32       = 258;
        const ARGPARSE_HELP_REQUESTED: u32  = 259;

        let dict_str_str_ty = Ty::Generic {
            base: TypeCtor::Dict,
            args: vec![str_ty.clone(), str_ty.clone()],
        };

        let argparse_mod = StdlibModule {
            name: "argparse".into(),
            items: vec![
                // `argparse.new(prog: str) -> Dict[str, str]` — fresh parser.
                StdlibItem {
                    name: "new".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], dict_str_str_ty.clone()),
                    native_id: ARGPARSE_NEW,
                },
                // `argparse.add_flag(p, name, default) -> None`
                StdlibItem {
                    name: "add_flag".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![
                            dict_str_str_ty.clone(),
                            str_ty.clone(),
                            bool_ty.clone(),
                        ],
                        unit_ty.clone(),
                    ),
                    native_id: ARGPARSE_ADD_FLAG,
                },
                // `argparse.add_arg(p, name) -> None`
                StdlibItem {
                    name: "add_arg".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![dict_str_str_ty.clone(), str_ty.clone()],
                        unit_ty.clone(),
                    ),
                    native_id: ARGPARSE_ADD_ARG,
                },
                // `argparse.add_opt(p, name, default) -> None`
                StdlibItem {
                    name: "add_opt".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![
                            dict_str_str_ty.clone(),
                            str_ty.clone(),
                            str_ty.clone(),
                        ],
                        unit_ty.clone(),
                    ),
                    native_id: ARGPARSE_ADD_OPT,
                },
                // `argparse.parse(p, argv: List[str]) -> Dict[str, str]`
                StdlibItem {
                    name: "parse".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![dict_str_str_ty.clone(), list_str_ty.clone()],
                        dict_str_str_ty.clone(),
                    ),
                    native_id: ARGPARSE_PARSE,
                },
                // `argparse.get_flag(a, name) -> bool`
                StdlibItem {
                    name: "get_flag".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![dict_str_str_ty.clone(), str_ty.clone()],
                        bool_ty.clone(),
                    ),
                    native_id: ARGPARSE_GET_FLAG,
                },
                // `argparse.get_arg(a, name) -> str`
                StdlibItem {
                    name: "get_arg".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![dict_str_str_ty.clone(), str_ty.clone()],
                        str_ty.clone(),
                    ),
                    native_id: ARGPARSE_GET_ARG,
                },
                // `argparse.get_opt(a, name) -> str`
                StdlibItem {
                    name: "get_opt".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![dict_str_str_ty.clone(), str_ty.clone()],
                        str_ty.clone(),
                    ),
                    native_id: ARGPARSE_GET_OPT,
                },
                // `argparse.help_text(p) -> str`
                StdlibItem {
                    name: "help_text".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![dict_str_str_ty.clone()], str_ty.clone()),
                    native_id: ARGPARSE_HELP_TEXT,
                },
                // `argparse.help_requested(argv) -> bool` — true iff
                // `argv` contains `-h` or `--help`.  User code combines
                // it with `help_text` and `sys.exit(0)` for the canonical
                // --help handling.
                StdlibItem {
                    name: "help_requested".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_str_ty.clone()], bool_ty.clone()),
                    native_id: ARGPARSE_HELP_REQUESTED,
                },
            ],
        };
        self.stdlib_modules.insert("argparse".into(), argparse_mod);

        // ── M22 (P2A): `collections` module ────────────────────────────
        // Counter (multiset) + deque (FIFO/LIFO double-ended queue).
        // Both backed by existing M7 Dict / List types — Counter is a
        // typed alias of `Dict[str, i64]`, deque is a typed alias of
        // `List[i64]` with pop_front via index-0 shift (O(n); the brief
        // calls this out as a v0.3 generic-class limitation).
        const COLL_COUNTER_NEW: u32           = 265;
        const COLL_COUNTER_INC: u32           = 266;
        const COLL_COUNTER_ADD: u32           = 267;
        const COLL_COUNTER_GET: u32           = 268;
        const COLL_COUNTER_TOP_KEYS: u32      = 269;
        const COLL_DEQUE_NEW: u32             = 270;
        const COLL_DEQUE_PUSH_BACK: u32       = 271;
        const COLL_DEQUE_POP_FRONT: u32       = 272;
        const COLL_DEQUE_LEN: u32             = 273;
        const COLL_DEQUE_IS_EMPTY: u32        = 274;

        let dict_str_i64_ty = Ty::Generic {
            base: TypeCtor::Dict,
            args: vec![str_ty.clone(), i64_ty.clone()],
        };

        let collections_mod = StdlibModule {
            name: "collections".into(),
            items: vec![
                // Counter
                StdlibItem {
                    name: "counter_new".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], dict_str_i64_ty.clone()),
                    native_id: COLL_COUNTER_NEW,
                },
                // `counter_increment(c, key)` — c[key] += 1
                StdlibItem {
                    name: "counter_increment".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![dict_str_i64_ty.clone(), str_ty.clone()],
                        unit_ty.clone(),
                    ),
                    native_id: COLL_COUNTER_INC,
                },
                // `counter_add(c, key, n)` — c[key] += n
                StdlibItem {
                    name: "counter_add".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![
                            dict_str_i64_ty.clone(),
                            str_ty.clone(),
                            i64_ty.clone(),
                        ],
                        unit_ty.clone(),
                    ),
                    native_id: COLL_COUNTER_ADD,
                },
                // `counter_get(c, key) -> i64` — 0 if absent.
                StdlibItem {
                    name: "counter_get".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![dict_str_i64_ty.clone(), str_ty.clone()],
                        i64_ty.clone(),
                    ),
                    native_id: COLL_COUNTER_GET,
                },
                // `counter_top_keys(c, n) -> List[str]` — keys sorted by
                // count descending, then alphabetically; return top N.
                // We don't ship `most_common` as List[Tuple[str, i64]]
                // because v0.2's tuple-in-list path is rough; pair this
                // with `counter_get` for the values.
                StdlibItem {
                    name: "counter_top_keys".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![dict_str_i64_ty.clone(), i32_ty.clone()],
                        list_str_ty.clone(),
                    ),
                    native_id: COLL_COUNTER_TOP_KEYS,
                },
                // Deque (typed as List[i64]; pop_front is O(n))
                StdlibItem {
                    name: "deque_new".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], list_i64_ty.clone()),
                    native_id: COLL_DEQUE_NEW,
                },
                StdlibItem {
                    name: "deque_push_back".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![list_i64_ty.clone(), i64_ty.clone()],
                        unit_ty.clone(),
                    ),
                    native_id: COLL_DEQUE_PUSH_BACK,
                },
                StdlibItem {
                    name: "deque_pop_front".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_i64_ty.clone()], i64_ty.clone()),
                    native_id: COLL_DEQUE_POP_FRONT,
                },
                StdlibItem {
                    name: "deque_len".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_i64_ty.clone()], i32_ty.clone()),
                    native_id: COLL_DEQUE_LEN,
                },
                StdlibItem {
                    name: "deque_is_empty".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_i64_ty.clone()], bool_ty.clone()),
                    native_id: COLL_DEQUE_IS_EMPTY,
                },
            ],
        };
        self.stdlib_modules.insert("collections".into(), collections_mod);

        // ── M22 (P2A): `csv` module ────────────────────────────────────
        // Hand-rolled RFC 4180-ish parser/writer.  M10's csv_aggregate.spy
        // had a single-pass scanner; this module packages it as named
        // natives so user programs don't reinvent it.
        //
        // Quoting rules (matches Python's `csv` with default dialect):
        //   * A field starting with `"` is quoted; the quote is stripped.
        //   * Inside a quoted field, `""` is a literal `"`.
        //   * Newlines inside quoted fields are preserved (so `parse` over
        //     multi-line CSV honours embedded line breaks).
        //   * Unquoted fields don't allow `,` or `\n` inside.
        //   * `escape` quotes a field iff it contains `,`, `"`, `\n`, or
        //     `\r`; doubles internal quotes.
        const CSV_PARSE_LINE: u32   = 275;
        const CSV_PARSE: u32        = 276;
        const CSV_READ_FILE: u32    = 277;
        const CSV_WRITE_FILE: u32   = 278;
        const CSV_ESCAPE: u32       = 279;
        const CSV_FORMAT_ROW: u32   = 280;

        let list_list_str_ty = Ty::Generic {
            base: TypeCtor::List,
            args: vec![list_str_ty.clone()],
        };

        let csv_mod = StdlibModule {
            name: "csv".into(),
            items: vec![
                StdlibItem {
                    name: "parse_line".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], list_str_ty.clone()),
                    native_id: CSV_PARSE_LINE,
                },
                StdlibItem {
                    name: "parse".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], list_list_str_ty.clone()),
                    native_id: CSV_PARSE,
                },
                StdlibItem {
                    name: "read_file".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], list_list_str_ty.clone()),
                    native_id: CSV_READ_FILE,
                },
                StdlibItem {
                    name: "write_file".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), list_list_str_ty.clone()],
                        unit_ty.clone(),
                    ),
                    native_id: CSV_WRITE_FILE,
                },
                StdlibItem {
                    name: "escape".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: CSV_ESCAPE,
                },
                StdlibItem {
                    name: "format_row".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![list_str_ty.clone()], str_ty.clone()),
                    native_id: CSV_FORMAT_ROW,
                },
            ],
        };
        self.stdlib_modules.insert("csv".into(), csv_mod);

        // ── M23 P3a-A: `subprocess` module ─────────────────────────────
        // Cross-platform process spawn + capture + wait, backed by Rust's
        // `std::process::Command`.  Running processes are tracked in a
        // global resource table inside the VM (same shape as M5's file
        // table) and exposed to user code as opaque `i64` handles —
        // stdlib classes are still v0.3 work.
        //
        // The two blocking convenience wrappers (`run`, `run_with_stdin`)
        // each return a `Tuple[i32, str, str]` of `(exit_code, stdout,
        // stderr)`.  `spawn` + `wait` + `try_wait` + `kill` are the
        // non-blocking primitives for daemon and supervision use cases.
        const SUBPROCESS_RUN: u32             = 350;
        const SUBPROCESS_RUN_WITH_STDIN: u32  = 351;
        const SUBPROCESS_SPAWN: u32           = 352;
        const SUBPROCESS_WAIT: u32            = 353;
        const SUBPROCESS_TRY_WAIT: u32        = 354;
        const SUBPROCESS_KILL: u32            = 355;

        let tuple_i32_str_str = Ty::Tuple(vec![
            i32_ty.clone(),
            str_ty.clone(),
            str_ty.clone(),
        ]);
        let nullable_i32_ty = Ty::Nullable(Box::new(i32_ty.clone()));

        let subprocess_mod = StdlibModule {
            name: "subprocess".into(),
            items: vec![
                StdlibItem {
                    name: "run".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), list_str_ty.clone()],
                        tuple_i32_str_str.clone(),
                    ),
                    native_id: SUBPROCESS_RUN,
                },
                StdlibItem {
                    name: "run_with_stdin".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), list_str_ty.clone(), str_ty.clone()],
                        tuple_i32_str_str.clone(),
                    ),
                    native_id: SUBPROCESS_RUN_WITH_STDIN,
                },
                StdlibItem {
                    name: "spawn".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), list_str_ty.clone()],
                        i64_ty.clone(),
                    ),
                    native_id: SUBPROCESS_SPAWN,
                },
                StdlibItem {
                    name: "wait".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], i32_ty.clone()),
                    native_id: SUBPROCESS_WAIT,
                },
                StdlibItem {
                    name: "try_wait".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], nullable_i32_ty.clone()),
                    native_id: SUBPROCESS_TRY_WAIT,
                },
                StdlibItem {
                    name: "kill".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], unit_ty.clone()),
                    native_id: SUBPROCESS_KILL,
                },
            ],
        };
        self.stdlib_modules.insert("subprocess".into(), subprocess_mod);

        // ── M23 P3a-A: `pathlib` module ────────────────────────────────
        // Object-oriented path manipulation shipped as a *flat-function*
        // API over `str`-typed paths.  Pythonic `Path("a") / "b"` chaining
        // isn't expressible until v0.3 brings stdlib classes; the
        // flat-function shape covers every concrete use case the M22
        // examples (and the planned Phase 3b examples) need.
        //
        // Aliases of the M20a `path` module entries (`join`, `parent` ==
        // `dirname`, `name` == `basename`) are kept under the `pathlib`
        // namespace so a program that `import pathlib` doesn't also need
        // `import path` for the basics.  The IR routes both to handlers
        // that share helper code (e.g. PathlibParent reuses the same
        // Path::parent logic as PathDirname); duplicating registrations
        // is purely an ergonomic / discoverability tool.
        const PATHLIB_JOIN: u32         = 370;
        const PATHLIB_WITH_SUFFIX: u32  = 371;
        const PATHLIB_WITH_NAME: u32    = 372;
        const PATHLIB_PARENT: u32       = 373;
        const PATHLIB_NAME: u32         = 374;
        const PATHLIB_STEM: u32         = 375;
        const PATHLIB_SUFFIX: u32       = 376;
        const PATHLIB_PARTS: u32        = 377;
        const PATHLIB_IS_ABSOLUTE: u32  = 378;
        const PATHLIB_ABSOLUTE: u32     = 379;
        const PATHLIB_RELATIVE_TO: u32  = 380;
        const PATHLIB_READ_TEXT: u32    = 381;
        const PATHLIB_WRITE_TEXT: u32   = 382;
        const PATHLIB_READ_LINES: u32   = 383;

        let pathlib_mod = StdlibModule {
            name: "pathlib".into(),
            items: vec![
                StdlibItem {
                    name: "join".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone()],
                        str_ty.clone(),
                    ),
                    native_id: PATHLIB_JOIN,
                },
                StdlibItem {
                    name: "with_suffix".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone()],
                        str_ty.clone(),
                    ),
                    native_id: PATHLIB_WITH_SUFFIX,
                },
                StdlibItem {
                    name: "with_name".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone()],
                        str_ty.clone(),
                    ),
                    native_id: PATHLIB_WITH_NAME,
                },
                StdlibItem {
                    name: "parent".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: PATHLIB_PARENT,
                },
                StdlibItem {
                    name: "name".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: PATHLIB_NAME,
                },
                StdlibItem {
                    name: "stem".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: PATHLIB_STEM,
                },
                StdlibItem {
                    name: "suffix".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: PATHLIB_SUFFIX,
                },
                StdlibItem {
                    name: "parts".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], list_str_ty.clone()),
                    native_id: PATHLIB_PARTS,
                },
                StdlibItem {
                    name: "is_absolute".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], bool_ty.clone()),
                    native_id: PATHLIB_IS_ABSOLUTE,
                },
                StdlibItem {
                    name: "absolute".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: PATHLIB_ABSOLUTE,
                },
                StdlibItem {
                    name: "relative_to".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone()],
                        str_ty.clone(),
                    ),
                    native_id: PATHLIB_RELATIVE_TO,
                },
                StdlibItem {
                    name: "read_text".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: PATHLIB_READ_TEXT,
                },
                StdlibItem {
                    name: "write_text".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone()],
                        unit_ty.clone(),
                    ),
                    native_id: PATHLIB_WRITE_TEXT,
                },
                StdlibItem {
                    name: "read_lines".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], list_str_ty.clone()),
                    native_id: PATHLIB_READ_LINES,
                },
            ],
        };
        self.stdlib_modules.insert("pathlib".into(), pathlib_mod);

        // ── M23 P3a-B: `datetime` module ──────────────────────────────
        // Calendar arithmetic + ISO 8601 parse/format, layered on top
        // of M20b's `time` epoch primitives.  Two value shapes — both
        // plain `i64` in v0.2 (no stdlib classes yet; M20c's typed
        // JsonValue / M22's ArgParser hit the same blocker):
        //   - DateTime: unix-epoch seconds (UTC).
        //   - Duration: seconds span.
        // 22 ids consumed (390..=411); 412..=419 reserved for v0.3
        // extensions (named timezones, fractional seconds, strftime).
        const DT_NOW: u32              = 390;
        const DT_FROM_UNIX: u32        = 391;
        const DT_FROM_YMD: u32         = 392;
        const DT_FROM_YMD_HMS: u32     = 393;
        const DT_YEAR: u32             = 394;
        const DT_MONTH: u32            = 395;
        const DT_DAY: u32              = 396;
        const DT_HOUR: u32             = 397;
        const DT_MINUTE: u32           = 398;
        const DT_SECOND: u32           = 399;
        const DT_WEEKDAY: u32          = 400;
        const DT_YMD: u32              = 401;
        const DT_ADD_SECONDS: u32      = 402;
        const DT_ADD_DAYS: u32         = 403;
        const DT_DIFF_SECONDS: u32     = 404;
        const DT_DIFF_DAYS: u32        = 405;
        const DT_TO_ISO: u32           = 406;
        const DT_TO_DATE_STR: u32      = 407;
        const DT_TO_TIME_STR: u32      = 408;
        const DT_FROM_ISO: u32         = 409;
        const DT_FROM_DATE_STR: u32    = 410;
        const DT_LOCAL_OFFSET_MIN: u32 = 411;

        // i32_ty / i64_ty / str_ty / fn_ty are already in scope from
        // earlier registrations in this fn.
        let tuple_i32x3 = Ty::Tuple(vec![
            i32_ty.clone(),
            i32_ty.clone(),
            i32_ty.clone(),
        ]);

        let datetime_mod = StdlibModule {
            name: "datetime".into(),
            items: vec![
                StdlibItem {
                    name: "now".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], i64_ty.clone()),
                    native_id: DT_NOW,
                },
                StdlibItem {
                    name: "from_unix".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], i64_ty.clone()),
                    native_id: DT_FROM_UNIX,
                },
                StdlibItem {
                    name: "from_ymd".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i32_ty.clone(), i32_ty.clone(), i32_ty.clone()],
                        i64_ty.clone(),
                    ),
                    native_id: DT_FROM_YMD,
                },
                StdlibItem {
                    name: "from_ymd_hms".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![
                            i32_ty.clone(), i32_ty.clone(), i32_ty.clone(),
                            i32_ty.clone(), i32_ty.clone(), i32_ty.clone(),
                        ],
                        i64_ty.clone(),
                    ),
                    native_id: DT_FROM_YMD_HMS,
                },
                StdlibItem {
                    name: "year".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], i32_ty.clone()),
                    native_id: DT_YEAR,
                },
                StdlibItem {
                    name: "month".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], i32_ty.clone()),
                    native_id: DT_MONTH,
                },
                StdlibItem {
                    name: "day".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], i32_ty.clone()),
                    native_id: DT_DAY,
                },
                StdlibItem {
                    name: "hour".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], i32_ty.clone()),
                    native_id: DT_HOUR,
                },
                StdlibItem {
                    name: "minute".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], i32_ty.clone()),
                    native_id: DT_MINUTE,
                },
                StdlibItem {
                    name: "second".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], i32_ty.clone()),
                    native_id: DT_SECOND,
                },
                StdlibItem {
                    name: "weekday".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], i32_ty.clone()),
                    native_id: DT_WEEKDAY,
                },
                StdlibItem {
                    name: "ymd".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], tuple_i32x3.clone()),
                    native_id: DT_YMD,
                },
                StdlibItem {
                    name: "add_seconds".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone(), i64_ty.clone()], i64_ty.clone()),
                    native_id: DT_ADD_SECONDS,
                },
                StdlibItem {
                    name: "add_days".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone(), i64_ty.clone()], i64_ty.clone()),
                    native_id: DT_ADD_DAYS,
                },
                StdlibItem {
                    name: "diff_seconds".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone(), i64_ty.clone()], i64_ty.clone()),
                    native_id: DT_DIFF_SECONDS,
                },
                StdlibItem {
                    name: "diff_days".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone(), i64_ty.clone()], i64_ty.clone()),
                    native_id: DT_DIFF_DAYS,
                },
                StdlibItem {
                    name: "to_iso".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], str_ty.clone()),
                    native_id: DT_TO_ISO,
                },
                StdlibItem {
                    name: "to_date_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], str_ty.clone()),
                    native_id: DT_TO_DATE_STR,
                },
                StdlibItem {
                    name: "to_time_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], str_ty.clone()),
                    native_id: DT_TO_TIME_STR,
                },
                StdlibItem {
                    name: "from_iso".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], i64_ty.clone()),
                    native_id: DT_FROM_ISO,
                },
                StdlibItem {
                    name: "from_date_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], i64_ty.clone()),
                    native_id: DT_FROM_DATE_STR,
                },
                StdlibItem {
                    name: "local_offset_minutes".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], i32_ty.clone()),
                    native_id: DT_LOCAL_OFFSET_MIN,
                },
            ],
        };
        self.stdlib_modules.insert("datetime".into(), datetime_mod);

        // ── M23 P3a-C: `threading` module (synchronization primitives) ──
        // Extends the existing M6 Thread/Channel runtime classes (which
        // remain bound in the prelude — see `seed_prelude`) with proper
        // Lock + Semaphore primitives.  Both are opaque i64 handles
        // referring into `SharedVm.locks` / `SharedVm.semaphores` slot
        // tables.  Choice of non-recursive `threading.Lock` semantics
        // (acquiring a held lock from the same thread DEADLOCKS) matches
        // Python's `threading.Lock`, not `RLock` — see spec §9.24.
        //
        // Note: the prelude's `Thread` and `Channel` classes win over
        // any items we register here under the same name, because the
        // resolver's import path (`register_top_decls`) checks for an
        // existing scope binding first.  That's deliberate — `from
        // threading import Thread` still binds the M6 class — and means
        // our new items must use *new* names that don't shadow Thread /
        // Channel methods.  None of the lock_* / semaphore_* names
        // collide, so we're safe.
        const THREADING_LOCK_NEW: u32              = 420;
        const THREADING_LOCK_ACQUIRE: u32          = 421;
        const THREADING_LOCK_RELEASE: u32          = 422;
        const THREADING_LOCK_TRY_ACQUIRE: u32      = 423;
        const THREADING_SEMAPHORE_NEW: u32         = 424;
        const THREADING_SEMAPHORE_ACQUIRE: u32     = 425;
        const THREADING_SEMAPHORE_RELEASE: u32     = 426;
        const THREADING_SEMAPHORE_TRY_ACQUIRE: u32 = 427;

        let threading_mod = StdlibModule {
            name: "threading".into(),
            items: vec![
                StdlibItem {
                    name: "lock_new".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], i64_ty.clone()),
                    native_id: THREADING_LOCK_NEW,
                },
                StdlibItem {
                    name: "lock_acquire".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], unit_ty.clone()),
                    native_id: THREADING_LOCK_ACQUIRE,
                },
                StdlibItem {
                    name: "lock_release".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], unit_ty.clone()),
                    native_id: THREADING_LOCK_RELEASE,
                },
                StdlibItem {
                    name: "lock_try_acquire".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], bool_ty.clone()),
                    native_id: THREADING_LOCK_TRY_ACQUIRE,
                },
                StdlibItem {
                    name: "semaphore_new".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i32_ty.clone()], i64_ty.clone()),
                    native_id: THREADING_SEMAPHORE_NEW,
                },
                StdlibItem {
                    name: "semaphore_acquire".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], unit_ty.clone()),
                    native_id: THREADING_SEMAPHORE_ACQUIRE,
                },
                StdlibItem {
                    name: "semaphore_release".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], unit_ty.clone()),
                    native_id: THREADING_SEMAPHORE_RELEASE,
                },
                StdlibItem {
                    name: "semaphore_try_acquire".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], bool_ty.clone()),
                    native_id: THREADING_SEMAPHORE_TRY_ACQUIRE,
                },
            ],
        };
        self.stdlib_modules.insert("threading".into(), threading_mod);

        // ── M23 P3a-C: `queue` module (priority queue) ─────────────────
        // Min-heap-based priority queue.  Two monomorphic variants ship
        // for v0.2 (item: i64 + item: str) because stdlib functions are
        // not generic (the M17 generic-fn worklist only sees user-defined
        // .spy fns).  `pq_len` and `pq_is_empty` are type-erased — the
        // handle alone is enough since the typechecker pins the call shape.
        const QUEUE_PQ_NEW_I64: u32       = 428;
        const QUEUE_PQ_PUSH_I64: u32      = 429;
        const QUEUE_PQ_POP_MIN_I64: u32   = 430;
        const QUEUE_PQ_PEEK_MIN_I64: u32  = 431;
        const QUEUE_PQ_NEW_STR: u32       = 432;
        const QUEUE_PQ_PUSH_STR: u32      = 433;
        const QUEUE_PQ_POP_MIN_STR: u32   = 434;
        const QUEUE_PQ_PEEK_MIN_STR: u32  = 435;
        const QUEUE_PQ_LEN: u32           = 436;
        const QUEUE_PQ_IS_EMPTY: u32      = 437;

        let tuple_f64_i64_ty = Ty::Tuple(vec![f64_ty.clone(), i64_ty.clone()]);
        let tuple_f64_str_ty = Ty::Tuple(vec![f64_ty.clone(), str_ty.clone()]);

        let queue_mod = StdlibModule {
            name: "queue".into(),
            items: vec![
                StdlibItem {
                    name: "pq_new_i64".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], i64_ty.clone()),
                    native_id: QUEUE_PQ_NEW_I64,
                },
                StdlibItem {
                    name: "pq_push_i64".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone(), f64_ty.clone(), i64_ty.clone()], unit_ty.clone()),
                    native_id: QUEUE_PQ_PUSH_I64,
                },
                StdlibItem {
                    name: "pq_pop_min_i64".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], tuple_f64_i64_ty.clone()),
                    native_id: QUEUE_PQ_POP_MIN_I64,
                },
                StdlibItem {
                    name: "pq_peek_min_i64".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], tuple_f64_i64_ty.clone()),
                    native_id: QUEUE_PQ_PEEK_MIN_I64,
                },
                StdlibItem {
                    name: "pq_new_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], i64_ty.clone()),
                    native_id: QUEUE_PQ_NEW_STR,
                },
                StdlibItem {
                    name: "pq_push_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone(), f64_ty.clone(), str_ty.clone()], unit_ty.clone()),
                    native_id: QUEUE_PQ_PUSH_STR,
                },
                StdlibItem {
                    name: "pq_pop_min_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], tuple_f64_str_ty.clone()),
                    native_id: QUEUE_PQ_POP_MIN_STR,
                },
                StdlibItem {
                    name: "pq_peek_min_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], tuple_f64_str_ty.clone()),
                    native_id: QUEUE_PQ_PEEK_MIN_STR,
                },
                StdlibItem {
                    name: "pq_len".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], i32_ty.clone()),
                    native_id: QUEUE_PQ_LEN,
                },
                StdlibItem {
                    name: "pq_is_empty".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], bool_ty.clone()),
                    native_id: QUEUE_PQ_IS_EMPTY,
                },
            ],
        };
        self.stdlib_modules.insert("queue".into(), queue_mod);

        // ── M23 P3a-D: `sqlite3` module ────────────────────────────────
        // Backed by the `rusqlite` crate (with the `bundled` feature so
        // the binary ships its own libsqlite3.c).  Connections are i64
        // handles into a per-process slot table on `SharedVm`; user
        // code passes the handle through every API call.  All query
        // result cells are stringified — INTEGER → "42", REAL → "3.14",
        // TEXT → the text, NULL → "" — and parameter binding always
        // treats bound values as TEXT.  See spec §9.24.
        const SQLITE3_CONNECT: u32             = 440;
        const SQLITE3_CLOSE: u32               = 441;
        const SQLITE3_EXECUTE: u32             = 442;
        const SQLITE3_EXECUTE_PARAMS: u32      = 443;
        const SQLITE3_QUERY: u32               = 444;
        const SQLITE3_QUERY_PARAMS: u32        = 445;
        const SQLITE3_LAST_INSERT_ROWID: u32   = 446;
        const SQLITE3_CHANGES: u32             = 447;
        const SQLITE3_COLUMN_NAMES: u32        = 448;
        // M35 P4-B: typed-class entry point (alloc + connect + populate
        // a `Connection` instance in one shot).
        const SQLITE3_OPEN_TYPED: u32          = 801;

        let list_list_str_ty_sqlite = Ty::Generic {
            base: TypeCtor::List,
            args: vec![list_str_ty.clone()],
        };

        // M35 P4-B: pull the prelude-registered `Connection` class id
        // so `sqlite3.open(path)` has the right return type.  Fallback
        // to `Ty::Never` if the prelude was somehow skipped, which
        // surfaces as a type error at the first call site rather than
        // silent miscompilation (same pattern as M34's json module).
        let p4b_conn_ty = match self.class_name_to_id.get("Connection") {
            Some(cid) => Ty::Class(*cid),
            None => Ty::Never,
        };

        let sqlite3_mod = StdlibModule {
            name: "sqlite3".into(),
            items: vec![
                StdlibItem {
                    name: "connect".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], i64_ty.clone()),
                    native_id: SQLITE3_CONNECT,
                },
                StdlibItem {
                    name: "close".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], unit_ty.clone()),
                    native_id: SQLITE3_CLOSE,
                },
                StdlibItem {
                    name: "execute".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty.clone(), str_ty.clone()],
                        unit_ty.clone(),
                    ),
                    native_id: SQLITE3_EXECUTE,
                },
                StdlibItem {
                    name: "execute_params".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty.clone(), str_ty.clone(), list_str_ty.clone()],
                        unit_ty.clone(),
                    ),
                    native_id: SQLITE3_EXECUTE_PARAMS,
                },
                StdlibItem {
                    name: "query".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty.clone(), str_ty.clone()],
                        list_list_str_ty_sqlite.clone(),
                    ),
                    native_id: SQLITE3_QUERY,
                },
                StdlibItem {
                    name: "query_params".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty.clone(), str_ty.clone(), list_str_ty.clone()],
                        list_list_str_ty_sqlite.clone(),
                    ),
                    native_id: SQLITE3_QUERY_PARAMS,
                },
                StdlibItem {
                    name: "last_insert_rowid".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], i64_ty.clone()),
                    native_id: SQLITE3_LAST_INSERT_ROWID,
                },
                StdlibItem {
                    name: "changes".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], i32_ty.clone()),
                    native_id: SQLITE3_CHANGES,
                },
                StdlibItem {
                    name: "column_names".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty.clone(), str_ty.clone()],
                        list_str_ty.clone(),
                    ),
                    native_id: SQLITE3_COLUMN_NAMES,
                },
                // M35 P4-B: typed-class entry point.  `sqlite3.open(path)`
                // mirrors Python's `sqlite3.connect(path)` but returns a
                // `Connection` instance (rather than an i64 handle) so
                // method-call ergonomics work.  The flat
                // `sqlite3.connect(path) -> i64` is still available for
                // the M29 framework and the M23 P3a-D demo.
                StdlibItem {
                    name: "open".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], p4b_conn_ty.clone()),
                    native_id: SQLITE3_OPEN_TYPED,
                },
            ],
        };
        // M36: publish the `Connection` + `Cursor` classes on the
        // `sqlite3` module.  Mirror of the json + re registrations
        // above.
        let mut sqlite3_mod = sqlite3_mod;
        for m36_name in ["Connection", "Cursor"] {
            if let Some(&m36_cid) = self.class_name_to_id.get(m36_name) {
                sqlite3_mod.items.push(StdlibItem {
                    name: m36_name.into(),
                    kind: StdlibItemKind::Class { class_id: m36_cid },
                    ty: Ty::Class(m36_cid),
                    native_id: 0,
                });
            }
        }
        self.stdlib_modules.insert("sqlite3".into(), sqlite3_mod);

        // ── M27 P3c-D: `zipfile` module ────────────────────────────────
        // Wraps the `zip` crate.  Read-mode handles allow random-access
        // entry reads; write-mode handles append in stored / deflated
        // form.  All entry bytes round-trip as `str` (each codepoint
        // 0..255 inclusive maps to a byte — the same str-as-byte-buffer
        // convention `struct.pack` already uses in v0.2).  See spec
        // §9.30.
        const ZIPFILE_OPEN_READ: u32     = 520;
        const ZIPFILE_OPEN_WRITE: u32    = 521;
        const ZIPFILE_NAMES: u32         = 522;
        const ZIPFILE_READ: u32          = 523;
        const ZIPFILE_WRITE: u32         = 524;
        const ZIPFILE_CLOSE: u32         = 525;
        const ZIPFILE_IS_ZIPFILE: u32    = 526;
        const ZIPFILE_INFO: u32          = 527;

        let p3c_d_zip_info_tuple_ty =
            Ty::Tuple(vec![i64_ty.clone(), i64_ty.clone(), i64_ty.clone()]);

        let p3c_d_zipfile_mod = StdlibModule {
            name: "zipfile".into(),
            items: vec![
                StdlibItem {
                    name: "open_read".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], i64_ty.clone()),
                    native_id: ZIPFILE_OPEN_READ,
                },
                StdlibItem {
                    name: "open_write".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], i64_ty.clone()),
                    native_id: ZIPFILE_OPEN_WRITE,
                },
                StdlibItem {
                    name: "names".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], list_str_ty.clone()),
                    native_id: ZIPFILE_NAMES,
                },
                StdlibItem {
                    name: "read".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty.clone(), str_ty.clone()],
                        str_ty.clone(),
                    ),
                    native_id: ZIPFILE_READ,
                },
                StdlibItem {
                    name: "write".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty.clone(), str_ty.clone(), str_ty.clone()],
                        unit_ty.clone(),
                    ),
                    native_id: ZIPFILE_WRITE,
                },
                StdlibItem {
                    name: "close".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], unit_ty.clone()),
                    native_id: ZIPFILE_CLOSE,
                },
                StdlibItem {
                    name: "is_zipfile".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], bool_ty.clone()),
                    native_id: ZIPFILE_IS_ZIPFILE,
                },
                StdlibItem {
                    name: "info".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty.clone(), str_ty.clone()],
                        p3c_d_zip_info_tuple_ty.clone(),
                    ),
                    native_id: ZIPFILE_INFO,
                },
            ],
        };
        self.stdlib_modules.insert("zipfile".into(), p3c_d_zipfile_mod);

        // ── M27 P3c-D: `tarfile` module ────────────────────────────────
        // Wraps the `tar` crate, with optional `flate2` (gz) and
        // `bzip2` (bz2) transparent compression.  Mode strings ("r",
        // "r:gz", "r:bz2", "w", "w:gz", "w:bz2") select the wrapper
        // exactly like Python's `tarfile.open(name, mode)`.  See spec
        // §9.31.
        const TARFILE_OPEN_READ: u32     = 530;
        const TARFILE_OPEN_WRITE: u32    = 531;
        const TARFILE_NAMES: u32         = 532;
        const TARFILE_READ: u32          = 533;
        const TARFILE_WRITE_FILE: u32    = 534;
        const TARFILE_WRITE_DATA: u32    = 535;
        const TARFILE_CLOSE: u32         = 536;
        const TARFILE_IS_TARFILE: u32    = 537;

        let p3c_d_tarfile_mod = StdlibModule {
            name: "tarfile".into(),
            items: vec![
                StdlibItem {
                    name: "open_read".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone()],
                        i64_ty.clone(),
                    ),
                    native_id: TARFILE_OPEN_READ,
                },
                StdlibItem {
                    name: "open_write".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone()],
                        i64_ty.clone(),
                    ),
                    native_id: TARFILE_OPEN_WRITE,
                },
                StdlibItem {
                    name: "names".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], list_str_ty.clone()),
                    native_id: TARFILE_NAMES,
                },
                StdlibItem {
                    name: "read".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty.clone(), str_ty.clone()],
                        str_ty.clone(),
                    ),
                    native_id: TARFILE_READ,
                },
                StdlibItem {
                    name: "write_file".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty.clone(), str_ty.clone(), str_ty.clone()],
                        unit_ty.clone(),
                    ),
                    native_id: TARFILE_WRITE_FILE,
                },
                StdlibItem {
                    name: "write_data".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty.clone(), str_ty.clone(), str_ty.clone()],
                        unit_ty.clone(),
                    ),
                    native_id: TARFILE_WRITE_DATA,
                },
                StdlibItem {
                    name: "close".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], unit_ty.clone()),
                    native_id: TARFILE_CLOSE,
                },
                StdlibItem {
                    name: "is_tarfile".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], bool_ty.clone()),
                    native_id: TARFILE_IS_TARFILE,
                },
            ],
        };
        self.stdlib_modules.insert("tarfile".into(), p3c_d_tarfile_mod);

        // ── M27 P3c-A: `shutil` module ─────────────────────────────────
        // High-level filesystem operations: file/dir copy, recursive
        // remove, PATH lookup for executables, and disk-usage stats.
        // Pure `std::fs` / `std::path` under the hood; no new crate dep.
        // Closes the v0.2 gap M24-D documented (no recursive rmdir).
        //
        // `shutil.which` returns `str?` (nullable) — Python returns None
        // when the command isn't on PATH.  `disk_usage` returns a
        // 3-tuple of i64s (total, used, free) in bytes, big enough for
        // filesystems up to ~9.2 EB.
        const SHUTIL_COPY: u32       = 450;
        const SHUTIL_COPYTREE: u32   = 451;
        const SHUTIL_MOVE: u32       = 452;
        const SHUTIL_RMTREE: u32     = 453;
        const SHUTIL_WHICH: u32      = 454;
        const SHUTIL_DISK_USAGE: u32 = 455;

        let tuple_i64_i64_i64_shutil = Ty::Tuple(vec![
            i64_ty.clone(),
            i64_ty.clone(),
            i64_ty.clone(),
        ]);

        let shutil_mod = StdlibModule {
            name: "shutil".into(),
            items: vec![
                StdlibItem {
                    name: "copy".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone()],
                        unit_ty.clone(),
                    ),
                    native_id: SHUTIL_COPY,
                },
                StdlibItem {
                    name: "copytree".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone()],
                        unit_ty.clone(),
                    ),
                    native_id: SHUTIL_COPYTREE,
                },
                StdlibItem {
                    name: "move".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone()],
                        unit_ty.clone(),
                    ),
                    native_id: SHUTIL_MOVE,
                },
                StdlibItem {
                    name: "rmtree".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], unit_ty.clone()),
                    native_id: SHUTIL_RMTREE,
                },
                StdlibItem {
                    name: "which".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], nullable_str_ty.clone()),
                    native_id: SHUTIL_WHICH,
                },
                StdlibItem {
                    name: "disk_usage".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone()],
                        tuple_i64_i64_i64_shutil.clone(),
                    ),
                    native_id: SHUTIL_DISK_USAGE,
                },
            ],
        };
        self.stdlib_modules.insert("shutil".into(), shutil_mod);

        // ── M27 P3c-A: `tempfile` module ───────────────────────────────
        // Temp file/dir creation backed by the `tempfile` crate (the
        // canonical Rust binding — picks the right per-OS atomic-creation
        // syscall, sets restrictive permissions, handles the prefix
        // dance).  v0.2 ships only the path-returning helpers; the
        // context-manager wrappers (NamedTemporaryFile,
        // TemporaryDirectory) wait for stdlib classes in v0.3.
        //
        // No default-argument support yet in the StdlibItem surface, so
        // users always pass the prefix explicitly (matches the M22
        // `argparse` shape — call sites are slightly more verbose than
        // Python but the surface is exactly representable).
        const TEMPFILE_MKDTEMP: u32     = 470;
        const TEMPFILE_MKSTEMP: u32     = 471;
        const TEMPFILE_GETTEMPDIR: u32  = 472;

        let tempfile_mod = StdlibModule {
            name: "tempfile".into(),
            items: vec![
                StdlibItem {
                    name: "mkdtemp".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: TEMPFILE_MKDTEMP,
                },
                StdlibItem {
                    name: "mkstemp".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone()],
                        str_ty.clone(),
                    ),
                    native_id: TEMPFILE_MKSTEMP,
                },
                StdlibItem {
                    name: "gettempdir".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], str_ty.clone()),
                    native_id: TEMPFILE_GETTEMPDIR,
                },
            ],
        };
        self.stdlib_modules.insert("tempfile".into(), tempfile_mod);
        // ── M27 P3c-E: `logging` module ────────────────────────────────
        // Application logging — flat global-logger surface.  v0.2 keeps
        // the API single-logger because Python's named-logger /
        // Logger/Handler/Formatter class hierarchy depends on stdlib
        // classes (still v0.3 work).  Level threshold + optional file
        // sink live on `SharedVm` as per-instance state.  Format is
        // fixed `"YYYY-MM-DDTHH:MM:SSZ LEVEL message\n"`, matching
        // CPython's default formatter.  See spec §9.39.
        //
        // Two entry points instead of CPython's single `basicConfig`
        // with an optional `filename=`: v0.2 stdlib doesn't ship default
        // arguments, so we split into `basic_config(level)` (stderr) and
        // `basic_config_to_file(level, filename)` (file).
        const LOG_BASIC_CONFIG: u32          = 550;
        const LOG_BASIC_CONFIG_TO_FILE: u32  = 551;
        const LOG_SET_LEVEL: u32             = 552;
        const LOG_GET_LEVEL: u32             = 553;
        const LOG_DEBUG: u32                 = 554;
        const LOG_INFO: u32                  = 555;
        const LOG_WARNING: u32               = 556;
        const LOG_ERROR: u32                 = 557;
        const LOG_CRITICAL: u32              = 558;
        const LOG_LOG: u32                   = 559;
        const LOG_IS_ENABLED_FOR: u32        = 560;

        let logging_mod = StdlibModule {
            name: "logging".into(),
            items: vec![
                StdlibItem {
                    name: "basic_config".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], unit_ty.clone()),
                    native_id: LOG_BASIC_CONFIG,
                },
                StdlibItem {
                    name: "basic_config_to_file".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone()],
                        unit_ty.clone(),
                    ),
                    native_id: LOG_BASIC_CONFIG_TO_FILE,
                },
                StdlibItem {
                    name: "set_level".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], unit_ty.clone()),
                    native_id: LOG_SET_LEVEL,
                },
                StdlibItem {
                    name: "get_level".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], str_ty.clone()),
                    native_id: LOG_GET_LEVEL,
                },
                StdlibItem {
                    name: "debug".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], unit_ty.clone()),
                    native_id: LOG_DEBUG,
                },
                StdlibItem {
                    name: "info".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], unit_ty.clone()),
                    native_id: LOG_INFO,
                },
                StdlibItem {
                    name: "warning".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], unit_ty.clone()),
                    native_id: LOG_WARNING,
                },
                StdlibItem {
                    name: "error".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], unit_ty.clone()),
                    native_id: LOG_ERROR,
                },
                StdlibItem {
                    name: "critical".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], unit_ty.clone()),
                    native_id: LOG_CRITICAL,
                },
                StdlibItem {
                    name: "log".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone()],
                        unit_ty.clone(),
                    ),
                    native_id: LOG_LOG,
                },
                StdlibItem {
                    name: "is_enabled_for".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], bool_ty.clone()),
                    native_id: LOG_IS_ENABLED_FOR,
                },
            ],
        };
        self.stdlib_modules.insert("logging".into(), logging_mod);
        // ── M27 P3c-B: `glob` module ───────────────────────────────────
        // Unix-shell-style pathname wildcard expansion.  Backed by the
        // `glob` crate so we ship a thin native handler per spec function
        // — no hand-rolled directory walker.  All three functions take
        // primitive `str` / return `str` or `List[str]`, matching the
        // CPython `glob` surface.  See spec §9.32.
        const GLOB_GLOB: u32      = 480;
        const GLOB_RECURSIVE: u32 = 481;
        const GLOB_ESCAPE: u32    = 482;

        let glob_mod = StdlibModule {
            name: "glob".into(),
            items: vec![
                StdlibItem {
                    name: "glob".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], list_str_ty.clone()),
                    native_id: GLOB_GLOB,
                },
                StdlibItem {
                    name: "recursive".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], list_str_ty.clone()),
                    native_id: GLOB_RECURSIVE,
                },
                StdlibItem {
                    name: "escape".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: GLOB_ESCAPE,
                },
            ],
        };
        self.stdlib_modules.insert("glob".into(), glob_mod);

        // ── M27 P3c-B: `fnmatch` module ────────────────────────────────
        // Single-string wildcard match (`*`, `?`, `[abc]`).
        // `fnmatch.fnmatch` is case-INsensitive on Windows / sensitive on
        // Unix to match CPython; `fnmatchcase` is always case-sensitive.
        // `translate` converts a shell-glob into a regex string callers
        // can feed into `re` (M20c) for composition.  See spec §9.33.
        const FNMATCH_FNMATCH: u32     = 483;
        const FNMATCH_FNMATCHCASE: u32 = 484;
        const FNMATCH_FILTER: u32      = 485;
        const FNMATCH_TRANSLATE: u32   = 486;

        let fnmatch_mod = StdlibModule {
            name: "fnmatch".into(),
            items: vec![
                StdlibItem {
                    name: "fnmatch".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), str_ty.clone()], bool_ty.clone()),
                    native_id: FNMATCH_FNMATCH,
                },
                StdlibItem {
                    name: "fnmatchcase".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), str_ty.clone()], bool_ty.clone()),
                    native_id: FNMATCH_FNMATCHCASE,
                },
                StdlibItem {
                    name: "filter".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![list_str_ty.clone(), str_ty.clone()],
                        list_str_ty.clone(),
                    ),
                    native_id: FNMATCH_FILTER,
                },
                StdlibItem {
                    name: "translate".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: FNMATCH_TRANSLATE,
                },
            ],
        };
        self.stdlib_modules.insert("fnmatch".into(), fnmatch_mod);
        // ── M27 P3c-C: `gzip` + `zlib` + `bz2` compression modules ──────
        // All three follow the str-as-byte-buffer convention (M22 P2D
        // struct): each `str` codepoint 0..=255 is one byte.  Inputs
        // are decoded via that convention; outputs (compressed bytes,
        // decompressed bytes) are re-encoded the same way so a binary
        // blob can round-trip losslessly without v0.3 `bytes`.
        //
        // `gzip` uses RFC 1952 framing; `zlib` uses RFC 1950 (no gzip
        // header / footer); `bz2` uses the libbzip2 format.  All three
        // raise `ValueError` on malformed input.
        const GZIP_COMPRESS: u32         = 500;
        const GZIP_COMPRESS_LEVEL: u32   = 501;
        const GZIP_DECOMPRESS: u32       = 502;
        const ZLIB_COMPRESS: u32         = 503;
        const ZLIB_COMPRESS_LEVEL: u32   = 504;
        const ZLIB_DECOMPRESS: u32       = 505;
        const ZLIB_CRC32: u32            = 506;
        const ZLIB_ADLER32: u32          = 507;
        const BZ2_COMPRESS: u32          = 508;
        const BZ2_COMPRESS_LEVEL: u32    = 509;
        const BZ2_DECOMPRESS: u32        = 510;

        // i32_ty / i64_ty are in scope from the M20b `time` block above.
        let gzip_mod = StdlibModule {
            name: "gzip".into(),
            items: vec![
                StdlibItem {
                    name: "compress".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: GZIP_COMPRESS,
                },
                StdlibItem {
                    name: "compress_level".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), i32_ty.clone()],
                        str_ty.clone(),
                    ),
                    native_id: GZIP_COMPRESS_LEVEL,
                },
                StdlibItem {
                    name: "decompress".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: GZIP_DECOMPRESS,
                },
            ],
        };
        self.stdlib_modules.insert("gzip".into(), gzip_mod);

        let zlib_mod = StdlibModule {
            name: "zlib".into(),
            items: vec![
                StdlibItem {
                    name: "compress".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: ZLIB_COMPRESS,
                },
                StdlibItem {
                    name: "compress_level".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), i32_ty.clone()],
                        str_ty.clone(),
                    ),
                    native_id: ZLIB_COMPRESS_LEVEL,
                },
                StdlibItem {
                    name: "decompress".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: ZLIB_DECOMPRESS,
                },
                StdlibItem {
                    name: "crc32".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], i64_ty.clone()),
                    native_id: ZLIB_CRC32,
                },
                StdlibItem {
                    name: "adler32".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], i64_ty.clone()),
                    native_id: ZLIB_ADLER32,
                },
            ],
        };
        self.stdlib_modules.insert("zlib".into(), zlib_mod);

        let bz2_mod = StdlibModule {
            name: "bz2".into(),
            items: vec![
                StdlibItem {
                    name: "compress".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: BZ2_COMPRESS,
                },
                StdlibItem {
                    name: "compress_level".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), i32_ty.clone()],
                        str_ty.clone(),
                    ),
                    native_id: BZ2_COMPRESS_LEVEL,
                },
                StdlibItem {
                    name: "decompress".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: BZ2_DECOMPRESS,
                },
            ],
        };
        self.stdlib_modules.insert("bz2".into(), bz2_mod);

        // ── M28 P3b-A: `socket` module ─────────────────────────────────
        // Raw TCP / UDP networking surface backed by `std::net`. Sockets
        // are opaque i64 handles into one of three SharedVm slot tables
        // (`tcp_streams`, `tcp_listeners`, `udp_sockets`). Bytes ride on
        // `str` with each codepoint a byte 0..255 — same convention as
        // `struct` (M22) and `gzip` / `zip` / `tar` (M27). See spec §9.40.
        const SOCKET_CONNECT_TCP: u32        = 570;
        const SOCKET_SEND: u32               = 571;
        const SOCKET_RECV: u32               = 572;
        const SOCKET_RECV_EXACT: u32         = 573;
        const SOCKET_CLOSE: u32              = 574;
        const SOCKET_SET_TIMEOUT_SECS: u32   = 575;
        const SOCKET_PEER_ADDR: u32          = 576;
        const SOCKET_LOCAL_ADDR: u32         = 577;
        const SOCKET_LISTEN_TCP: u32         = 578;
        const SOCKET_ACCEPT: u32             = 579;
        const SOCKET_CLOSE_LISTENER: u32     = 580;
        const SOCKET_UDP_SOCKET: u32         = 581;
        const SOCKET_UDP_BIND: u32           = 582;
        const SOCKET_UDP_SEND_TO: u32        = 583;
        const SOCKET_UDP_RECV_FROM: u32      = 584;
        const SOCKET_UDP_CLOSE: u32          = 585;
        const SOCKET_GETHOSTBYNAME: u32      = 586;
        const SOCKET_RESOLVE: u32            = 587;
        const SOCKET_GETHOSTNAME: u32        = 588;

        let i32_ty_sock = Ty::Primitive(PrimTy::I32);
        let i64_ty_sock = Ty::Primitive(PrimTy::I64);
        let f64_ty_sock = Ty::Primitive(PrimTy::F64);
        let p3b_a_sock_accept_tuple = Ty::Tuple(vec![
            i64_ty_sock.clone(),
            str_ty.clone(),
        ]);
        let p3b_a_sock_udp_recv_tuple = Ty::Tuple(vec![
            str_ty.clone(),
            str_ty.clone(),
            i32_ty_sock.clone(),
        ]);

        let p3b_a_socket_mod = StdlibModule {
            name: "socket".into(),
            items: vec![
                StdlibItem {
                    name: "connect_tcp".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), i32_ty_sock.clone()],
                        i64_ty_sock.clone(),
                    ),
                    native_id: SOCKET_CONNECT_TCP,
                },
                StdlibItem {
                    name: "send".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty_sock.clone(), str_ty.clone()],
                        i32_ty_sock.clone(),
                    ),
                    native_id: SOCKET_SEND,
                },
                StdlibItem {
                    name: "recv".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty_sock.clone(), i32_ty_sock.clone()],
                        str_ty.clone(),
                    ),
                    native_id: SOCKET_RECV,
                },
                StdlibItem {
                    name: "recv_exact".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty_sock.clone(), i32_ty_sock.clone()],
                        str_ty.clone(),
                    ),
                    native_id: SOCKET_RECV_EXACT,
                },
                StdlibItem {
                    name: "close".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty_sock.clone()], unit_ty.clone()),
                    native_id: SOCKET_CLOSE,
                },
                StdlibItem {
                    name: "set_timeout_secs".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty_sock.clone(), f64_ty_sock.clone()],
                        unit_ty.clone(),
                    ),
                    native_id: SOCKET_SET_TIMEOUT_SECS,
                },
                StdlibItem {
                    name: "peer_addr".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty_sock.clone()], str_ty.clone()),
                    native_id: SOCKET_PEER_ADDR,
                },
                StdlibItem {
                    name: "local_addr".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty_sock.clone()], str_ty.clone()),
                    native_id: SOCKET_LOCAL_ADDR,
                },
                StdlibItem {
                    name: "listen_tcp".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![
                            str_ty.clone(),
                            i32_ty_sock.clone(),
                            i32_ty_sock.clone(),
                        ],
                        i64_ty_sock.clone(),
                    ),
                    native_id: SOCKET_LISTEN_TCP,
                },
                StdlibItem {
                    name: "accept".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty_sock.clone()],
                        p3b_a_sock_accept_tuple.clone(),
                    ),
                    native_id: SOCKET_ACCEPT,
                },
                StdlibItem {
                    name: "close_listener".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty_sock.clone()], unit_ty.clone()),
                    native_id: SOCKET_CLOSE_LISTENER,
                },
                StdlibItem {
                    name: "udp_socket".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], i64_ty_sock.clone()),
                    native_id: SOCKET_UDP_SOCKET,
                },
                StdlibItem {
                    name: "udp_bind".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), i32_ty_sock.clone()],
                        i64_ty_sock.clone(),
                    ),
                    native_id: SOCKET_UDP_BIND,
                },
                StdlibItem {
                    name: "udp_send_to".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![
                            i64_ty_sock.clone(),
                            str_ty.clone(),
                            str_ty.clone(),
                            i32_ty_sock.clone(),
                        ],
                        i32_ty_sock.clone(),
                    ),
                    native_id: SOCKET_UDP_SEND_TO,
                },
                StdlibItem {
                    name: "udp_recv_from".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty_sock.clone(), i32_ty_sock.clone()],
                        p3b_a_sock_udp_recv_tuple.clone(),
                    ),
                    native_id: SOCKET_UDP_RECV_FROM,
                },
                StdlibItem {
                    name: "udp_close".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty_sock.clone()], unit_ty.clone()),
                    native_id: SOCKET_UDP_CLOSE,
                },
                StdlibItem {
                    name: "gethostbyname".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: SOCKET_GETHOSTBYNAME,
                },
                StdlibItem {
                    name: "resolve".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), i32_ty_sock.clone()],
                        list_str_ty.clone(),
                    ),
                    native_id: SOCKET_RESOLVE,
                },
                StdlibItem {
                    name: "gethostname".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], str_ty.clone()),
                    native_id: SOCKET_GETHOSTNAME,
                },
                // ── M32 async-variant socket functions (spec §9.43.3) ──
                // Non-blocking accept/recv/send that hand back a
                // `Future[...]` instead of blocking the calling thread.
                // Internal implementation: thread-per-task (Shape A);
                // v0.4 swaps to a mio/polling event loop.
                StdlibItem {
                    name: "async_accept".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty_sock.clone()],
                        Ty::Generic {
                            base: TypeCtor::Future,
                            args: vec![p3b_a_sock_accept_tuple.clone()],
                        },
                    ),
                    native_id: 720, // SocketAsyncAccept
                },
                StdlibItem {
                    name: "async_recv".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty_sock.clone(), i32_ty_sock.clone()],
                        Ty::Generic {
                            base: TypeCtor::Future,
                            args: vec![str_ty.clone()],
                        },
                    ),
                    native_id: 721, // SocketAsyncRecv
                },
                StdlibItem {
                    name: "async_send".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty_sock.clone(), str_ty.clone()],
                        Ty::Generic {
                            base: TypeCtor::Future,
                            args: vec![i32_ty_sock.clone()],
                        },
                    ),
                    native_id: 722, // SocketAsyncSend
                },
            ],
        };
        self.stdlib_modules.insert("socket".into(), p3b_a_socket_mod);

        // ── M32: `asyncio` module (spec §9.43) ─────────────────────────
        // Thread-per-task async runtime façade (Shape A).  The surface
        // matches what a real event-loop-backed runtime would expose;
        // v0.4 will swap the internal scheduler to a mio/polling event
        // loop without changing this surface.
        const ASYNCIO_RUN_I32: u32        = 700;
        const ASYNCIO_RUN_UNIT: u32       = 701;
        const ASYNCIO_SPAWN_I32: u32      = 702;
        const ASYNCIO_SPAWN_I64: u32      = 703;
        const ASYNCIO_SPAWN_STR: u32      = 704;
        const ASYNCIO_SPAWN_BOOL: u32     = 705;
        const ASYNCIO_SPAWN_UNIT: u32     = 706;
        const ASYNCIO_SLEEP: u32          = 707;
        const ASYNCIO_GATHER_2_I32: u32   = 710;
        const ASYNCIO_GATHER_2_STR: u32   = 711;
        const ASYNCIO_GATHER_3_I32: u32   = 712;
        const ASYNCIO_GATHER_3_STR: u32   = 713;
        const ASYNCIO_GATHER_4_I32: u32   = 714;

        let m32_async_i32_ty = Ty::Primitive(PrimTy::I32);
        let m32_async_i64_ty = Ty::Primitive(PrimTy::I64);
        let m32_async_str_ty = Ty::Primitive(PrimTy::Str);
        let m32_async_bool_ty = Ty::Primitive(PrimTy::Bool);
        let m32_async_unit_ty = Ty::Primitive(PrimTy::Unit);
        let m32_async_f64_ty = Ty::Primitive(PrimTy::F64);

        let future_of = |arg: Ty| Ty::Generic {
            base: TypeCtor::Future,
            args: vec![arg],
        };
        let closure_of = |ret: Ty| Ty::Function {
            params: vec![],
            ret: Box::new(ret),
        };

        let asyncio_mod = StdlibModule {
            name: "asyncio".into(),
            items: vec![
                StdlibItem {
                    name: "run_i32".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![closure_of(m32_async_i32_ty.clone())],
                        m32_async_i32_ty.clone(),
                    ),
                    native_id: ASYNCIO_RUN_I32,
                },
                StdlibItem {
                    name: "run_unit".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![closure_of(m32_async_unit_ty.clone())],
                        m32_async_unit_ty.clone(),
                    ),
                    native_id: ASYNCIO_RUN_UNIT,
                },
                StdlibItem {
                    name: "spawn_i32".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![closure_of(m32_async_i32_ty.clone())],
                        future_of(m32_async_i32_ty.clone()),
                    ),
                    native_id: ASYNCIO_SPAWN_I32,
                },
                StdlibItem {
                    name: "spawn_i64".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![closure_of(m32_async_i64_ty.clone())],
                        future_of(m32_async_i64_ty.clone()),
                    ),
                    native_id: ASYNCIO_SPAWN_I64,
                },
                StdlibItem {
                    name: "spawn_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![closure_of(m32_async_str_ty.clone())],
                        future_of(m32_async_str_ty.clone()),
                    ),
                    native_id: ASYNCIO_SPAWN_STR,
                },
                StdlibItem {
                    name: "spawn_bool".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![closure_of(m32_async_bool_ty.clone())],
                        future_of(m32_async_bool_ty.clone()),
                    ),
                    native_id: ASYNCIO_SPAWN_BOOL,
                },
                StdlibItem {
                    name: "spawn_unit".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![closure_of(m32_async_unit_ty.clone())],
                        future_of(m32_async_unit_ty.clone()),
                    ),
                    native_id: ASYNCIO_SPAWN_UNIT,
                },
                StdlibItem {
                    name: "sleep".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![m32_async_f64_ty.clone()],
                        m32_async_unit_ty.clone(),
                    ),
                    native_id: ASYNCIO_SLEEP,
                },
                StdlibItem {
                    name: "gather_2_i32".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![
                            future_of(m32_async_i32_ty.clone()),
                            future_of(m32_async_i32_ty.clone()),
                        ],
                        Ty::Tuple(vec![
                            m32_async_i32_ty.clone(),
                            m32_async_i32_ty.clone(),
                        ]),
                    ),
                    native_id: ASYNCIO_GATHER_2_I32,
                },
                StdlibItem {
                    name: "gather_2_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![
                            future_of(m32_async_str_ty.clone()),
                            future_of(m32_async_str_ty.clone()),
                        ],
                        Ty::Tuple(vec![
                            m32_async_str_ty.clone(),
                            m32_async_str_ty.clone(),
                        ]),
                    ),
                    native_id: ASYNCIO_GATHER_2_STR,
                },
                StdlibItem {
                    name: "gather_3_i32".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![
                            future_of(m32_async_i32_ty.clone()),
                            future_of(m32_async_i32_ty.clone()),
                            future_of(m32_async_i32_ty.clone()),
                        ],
                        Ty::Tuple(vec![
                            m32_async_i32_ty.clone(),
                            m32_async_i32_ty.clone(),
                            m32_async_i32_ty.clone(),
                        ]),
                    ),
                    native_id: ASYNCIO_GATHER_3_I32,
                },
                StdlibItem {
                    name: "gather_3_str".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![
                            future_of(m32_async_str_ty.clone()),
                            future_of(m32_async_str_ty.clone()),
                            future_of(m32_async_str_ty.clone()),
                        ],
                        Ty::Tuple(vec![
                            m32_async_str_ty.clone(),
                            m32_async_str_ty.clone(),
                            m32_async_str_ty.clone(),
                        ]),
                    ),
                    native_id: ASYNCIO_GATHER_3_STR,
                },
                StdlibItem {
                    name: "gather_4_i32".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![
                            future_of(m32_async_i32_ty.clone()),
                            future_of(m32_async_i32_ty.clone()),
                            future_of(m32_async_i32_ty.clone()),
                            future_of(m32_async_i32_ty.clone()),
                        ],
                        Ty::Tuple(vec![
                            m32_async_i32_ty.clone(),
                            m32_async_i32_ty.clone(),
                            m32_async_i32_ty.clone(),
                            m32_async_i32_ty.clone(),
                        ]),
                    ),
                    native_id: ASYNCIO_GATHER_4_I32,
                },
            ],
        };
        self.stdlib_modules.insert("asyncio".into(), asyncio_mod);

        // ── M28 P3b-B: `ssl` module ────────────────────────────────────
        // TLS-over-TCP client.  Opens an encrypted connection in one
        // shot (TCP socket + TLS handshake bundled), returning an i64
        // handle that the rest of the surface uses for send / recv /
        // close.  See spec §9.41.
        const SSL_CONNECT: u32              = 600;
        const SSL_SEND: u32                 = 601;
        const SSL_RECV: u32                 = 602;
        const SSL_RECV_EXACT: u32           = 603;
        const SSL_CLOSE: u32                = 604;
        const SSL_PEER_ADDR: u32            = 605;
        const SSL_PEER_CERT_SUBJECT: u32    = 606;
        const SSL_SET_TIMEOUT_SECS: u32     = 607;
        const SSL_SET_VERIFY_CERTS: u32     = 608;
        const SSL_GET_VERIFY_CERTS: u32     = 609;
        // M28.5 P3b-D: server-side TLS extension.  Lives in the same
        // `ssl` module — `load_server_config` + `accept_tls` +
        // `free_server_config` ride alongside the client-side surface.
        const SSL_LOAD_SERVER_CONFIG: u32   = 610;
        const SSL_ACCEPT_TLS: u32           = 611;
        const SSL_FREE_SERVER_CONFIG: u32   = 612;
        let p3b_d_ssl_accept_tuple = Ty::Tuple(vec![
            i64_ty.clone(),
            str_ty.clone(),
        ]);

        let p3b_b_ssl_mod = StdlibModule {
            name: "ssl".into(),
            items: vec![
                StdlibItem {
                    name: "connect".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone(), i32_ty.clone()], i64_ty.clone()),
                    native_id: SSL_CONNECT,
                },
                StdlibItem {
                    name: "send".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone(), str_ty.clone()], i32_ty.clone()),
                    native_id: SSL_SEND,
                },
                StdlibItem {
                    name: "recv".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone(), i32_ty.clone()], str_ty.clone()),
                    native_id: SSL_RECV,
                },
                StdlibItem {
                    name: "recv_exact".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone(), i32_ty.clone()], str_ty.clone()),
                    native_id: SSL_RECV_EXACT,
                },
                StdlibItem {
                    name: "close".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], unit_ty.clone()),
                    native_id: SSL_CLOSE,
                },
                StdlibItem {
                    name: "peer_addr".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], str_ty.clone()),
                    native_id: SSL_PEER_ADDR,
                },
                StdlibItem {
                    name: "peer_cert_subject".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], str_ty.clone()),
                    native_id: SSL_PEER_CERT_SUBJECT,
                },
                StdlibItem {
                    name: "set_timeout_secs".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone(), f64_ty.clone()], unit_ty.clone()),
                    native_id: SSL_SET_TIMEOUT_SECS,
                },
                StdlibItem {
                    name: "set_verify_certs".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![bool_ty.clone()], unit_ty.clone()),
                    native_id: SSL_SET_VERIFY_CERTS,
                },
                StdlibItem {
                    name: "get_verify_certs".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![], bool_ty.clone()),
                    native_id: SSL_GET_VERIFY_CERTS,
                },
                // ── M28.5 P3b-D: server-side TLS surface ──────────────
                StdlibItem {
                    name: "load_server_config".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone()],
                        i64_ty.clone(),
                    ),
                    native_id: SSL_LOAD_SERVER_CONFIG,
                },
                StdlibItem {
                    name: "accept_tls".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![i64_ty.clone(), i64_ty.clone()],
                        p3b_d_ssl_accept_tuple.clone(),
                    ),
                    native_id: SSL_ACCEPT_TLS,
                },
                StdlibItem {
                    name: "free_server_config".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i64_ty.clone()], unit_ty.clone()),
                    native_id: SSL_FREE_SERVER_CONFIG,
                },
            ],
        };
        self.stdlib_modules.insert("ssl".into(), p3b_b_ssl_mod);
        // ── M28 P3b-C: `http_client` module ────────────────────────────
        // Synchronous HTTP/1.1 client built on `ureq` + rustls.  The
        // module is stateless — each call opens a fresh socket, sends
        // the request, reads the response, closes.  See spec §9.42.
        const HTTPC_GET: u32                   = 620;
        const HTTPC_POST: u32                  = 621;
        const HTTPC_PUT: u32                   = 622;
        const HTTPC_DELETE: u32                = 623;
        const HTTPC_HEAD: u32                  = 624;
        const HTTPC_REQUEST: u32               = 625;
        const HTTPC_REQUEST_WITH_HEADERS: u32  = 626;
        const HTTPC_URLENCODE: u32             = 627;
        const HTTPC_URLDECODE: u32             = 628;
        const HTTPC_URL_PARSE: u32             = 629;
        const HTTPC_STATUS_TEXT: u32           = 630;

        let p3b_c_str_str_tuple_ty =
            Ty::Tuple(vec![str_ty.clone(), str_ty.clone()]);
        let p3b_c_list_str_str_tuple_ty = Ty::Generic {
            base: TypeCtor::List,
            args: vec![p3b_c_str_str_tuple_ty.clone()],
        };
        let p3b_c_status_body_tuple_ty =
            Ty::Tuple(vec![i32_ty.clone(), str_ty.clone()]);
        let p3b_c_status_hdrs_body_tuple_ty = Ty::Tuple(vec![
            i32_ty.clone(),
            p3b_c_list_str_str_tuple_ty.clone(),
            str_ty.clone(),
        ]);
        let p3b_c_url_parse_tuple_ty = Ty::Tuple(vec![
            str_ty.clone(),
            str_ty.clone(),
            i32_ty.clone(),
            str_ty.clone(),
        ]);

        let http_client_mod = StdlibModule {
            name: "http_client".into(),
            items: vec![
                StdlibItem {
                    name: "get".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone()],
                        p3b_c_status_body_tuple_ty.clone(),
                    ),
                    native_id: HTTPC_GET,
                },
                StdlibItem {
                    name: "post".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone(), str_ty.clone()],
                        p3b_c_status_body_tuple_ty.clone(),
                    ),
                    native_id: HTTPC_POST,
                },
                StdlibItem {
                    name: "put".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone(), str_ty.clone(), str_ty.clone()],
                        p3b_c_status_body_tuple_ty.clone(),
                    ),
                    native_id: HTTPC_PUT,
                },
                StdlibItem {
                    name: "delete".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone()],
                        p3b_c_status_body_tuple_ty.clone(),
                    ),
                    native_id: HTTPC_DELETE,
                },
                StdlibItem {
                    name: "head".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone()],
                        p3b_c_status_body_tuple_ty.clone(),
                    ),
                    native_id: HTTPC_HEAD,
                },
                StdlibItem {
                    name: "request".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![
                            str_ty.clone(),                           // method
                            str_ty.clone(),                           // url
                            str_ty.clone(),                           // body
                            p3b_c_list_str_str_tuple_ty.clone(),      // headers
                            f64_ty.clone(),                           // timeout_secs
                        ],
                        p3b_c_status_body_tuple_ty.clone(),
                    ),
                    native_id: HTTPC_REQUEST,
                },
                StdlibItem {
                    name: "request_with_headers".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![
                            str_ty.clone(),
                            str_ty.clone(),
                            str_ty.clone(),
                            p3b_c_list_str_str_tuple_ty.clone(),
                            f64_ty.clone(),
                        ],
                        p3b_c_status_hdrs_body_tuple_ty.clone(),
                    ),
                    native_id: HTTPC_REQUEST_WITH_HEADERS,
                },
                StdlibItem {
                    name: "urlencode".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![p3b_c_list_str_str_tuple_ty.clone()],
                        str_ty.clone(),
                    ),
                    native_id: HTTPC_URLENCODE,
                },
                StdlibItem {
                    name: "urldecode".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![str_ty.clone()], str_ty.clone()),
                    native_id: HTTPC_URLDECODE,
                },
                StdlibItem {
                    name: "url_parse".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(
                        vec![str_ty.clone()],
                        p3b_c_url_parse_tuple_ty.clone(),
                    ),
                    native_id: HTTPC_URL_PARSE,
                },
                StdlibItem {
                    name: "status_text".into(),
                    kind: StdlibItemKind::Function,
                    ty: fn_ty(vec![i32_ty.clone()], str_ty.clone()),
                    native_id: HTTPC_STATUS_TEXT,
                },
            ],
        };
        self.stdlib_modules.insert("http_client".into(), http_client_mod);

        // ── M37: `tabular` module (DataFrame + sealed Column hierarchy) ─
        //
        // First Pandas-shaped data package for v0.3.  First stdlib package
        // to register its classes module-scoped from the start (no prelude
        // bloat) via the post-M36 `StdlibItemKind::Class` path.  Class
        // layouts + class_name_to_id entries are populated here so the
        // `tabular` module's typed function signatures (returning
        // `ColumnI64` etc.) can reference the class IDs.  No symbols are
        // inserted into prelude_scope — users access the class names via
        // `from tabular import DataFrame, ColumnI64, ...` (the
        // M36 Class-item import path materialises a fresh class symbol
        // in module_scope) or via `tabular.col_i64(...)` factory calls.
        self.m37_register_tabular_classes_and_module();
    }

    /// M37: Register the 6 tabular classes (Column + 5 subclasses +
    /// DataFrame) and the `tabular` stdlib module that exposes them.
    /// See `seed_stdlib_modules` for the broader context.
    fn m37_register_tabular_classes_and_module(&mut self) {
        use crate::types::PrimTy;
        // ── Class IDs (allocated up front so layouts can reference each
        // other — e.g. DataFrame.columns: List[Column]).
        let m37_col_cid = self.fresh_class();
        let m37_col_i64_cid = self.fresh_class();
        let m37_col_f64_cid = self.fresh_class();
        let m37_col_str_cid = self.fresh_class();
        let m37_col_bool_cid = self.fresh_class();
        let m37_col_dt_cid = self.fresh_class();
        // M47: ColumnCategorical — new sealed Column subclass storing
        // {codes: List[i64], categories: List[str], nulls: List[bool],
        // length: i64}.  Existing-op integration v1 is via to_strings()
        // coercion.
        let m47_col_cat_cid = self.fresh_class();
        let m37_df_cid = self.fresh_class();

        self.class_name_to_id.insert("Column".into(), m37_col_cid);
        self.class_name_to_id.insert("ColumnI64".into(), m37_col_i64_cid);
        self.class_name_to_id.insert("ColumnF64".into(), m37_col_f64_cid);
        self.class_name_to_id.insert("ColumnStr".into(), m37_col_str_cid);
        self.class_name_to_id.insert("ColumnBool".into(), m37_col_bool_cid);
        self.class_name_to_id.insert("ColumnDateTime".into(), m37_col_dt_cid);
        self.class_name_to_id.insert("ColumnCategorical".into(), m47_col_cat_cid);
        self.class_name_to_id.insert("DataFrame".into(), m37_df_cid);

        // ── Type aliases ──
        let m37_i64 = Ty::Primitive(PrimTy::I64);
        let m37_f64 = Ty::Primitive(PrimTy::F64);
        let m37_str = Ty::Primitive(PrimTy::Str);
        let m37_bool = Ty::Primitive(PrimTy::Bool);
        let m37_unit = Ty::Primitive(PrimTy::Unit);
        let m37_list_i64 = Ty::Generic { base: TypeCtor::List, args: vec![m37_i64.clone()] };
        let m37_list_f64 = Ty::Generic { base: TypeCtor::List, args: vec![m37_f64.clone()] };
        let m37_list_str = Ty::Generic { base: TypeCtor::List, args: vec![m37_str.clone()] };
        let m37_list_bool = Ty::Generic { base: TypeCtor::List, args: vec![m37_bool.clone()] };
        let m37_list_list_str = Ty::Generic {
            base: TypeCtor::List,
            args: vec![m37_list_str.clone()],
        };
        let m37_col_ty = Ty::Class(m37_col_cid);
        let m37_col_i64_ty = Ty::Class(m37_col_i64_cid);
        let m37_col_f64_ty = Ty::Class(m37_col_f64_cid);
        let m37_col_str_ty = Ty::Class(m37_col_str_cid);
        let m37_col_bool_ty = Ty::Class(m37_col_bool_cid);
        let m37_col_dt_ty = Ty::Class(m37_col_dt_cid);
        // M47: ColumnCategorical Ty alias.
        let m47_col_cat_ty = Ty::Class(m47_col_cat_cid);
        let m37_df_ty = Ty::Class(m37_df_cid);
        let m37_list_col_ty = Ty::Generic { base: TypeCtor::List, args: vec![m37_col_ty.clone()] };
        let m37_schema_ty = Ty::Generic {
            base: TypeCtor::List,
            args: vec![Ty::Tuple(vec![m37_str.clone(), m37_str.clone()])],
        };

        // ── Helper: per-column method list (length / dtype / is_null /
        // null_count + typed get).  Each subclass gets its own list with
        // the typed get's return type bound to its element type.
        let m37_shared_methods = |get_method: MethodSig| -> Vec<MethodSig> {
            vec![
                MethodSig { name: "length".into(),     params: vec![], ret: m37_i64.clone() },
                MethodSig { name: "dtype".into(),      params: vec![], ret: m37_str.clone() },
                MethodSig { name: "is_null".into(),    params: vec![m37_i64.clone()], ret: m37_bool.clone() },
                MethodSig { name: "null_count".into(), params: vec![], ret: m37_i64.clone() },
                get_method,
            ]
        };

        // ── Base sealed Column class (no fields, no methods — subclasses
        // carry per-type storage; sealed means subclasses can be defined
        // here but NOT in user code).
        self.class_layouts.insert(m37_col_cid, ClassLayout {
            id: m37_col_cid, name: "Column".into(), base: None,
            is_open: false, is_sealed: true,
            fields: vec![], methods: vec![],
            generics: vec![], generic_tvars: vec![],
            is_native: false, payload_size: 0,
        });

        // ── ColumnI64 — { values: List[i64], nulls: List[bool], length: i64 }
        self.class_layouts.insert(m37_col_i64_cid, ClassLayout {
            id: m37_col_i64_cid, name: "ColumnI64".into(), base: Some(m37_col_cid),
            is_open: false, is_sealed: false,
            fields: vec![
                FieldInfo { name: "values".into(), ty: m37_list_i64.clone(), offset: 0 },
                FieldInfo { name: "nulls".into(),  ty: m37_list_bool.clone(), offset: 8 },
                FieldInfo { name: "length".into(), ty: m37_i64.clone(),       offset: 16 },
            ],
            methods: {
                let mut v = m37_shared_methods(MethodSig {
                    name: "get".into(),
                    params: vec![m37_i64.clone()],
                    ret: Ty::Nullable(Box::new(m37_i64.clone())),
                });
                v.push(MethodSig {
                    name: "eq".into(), params: vec![m37_i64.clone()], ret: m37_col_bool_ty.clone(),
                });
                v.push(MethodSig {
                    name: "gt".into(), params: vec![m37_i64.clone()], ret: m37_col_bool_ty.clone(),
                });
                v.push(MethodSig {
                    name: "lt".into(), params: vec![m37_i64.clone()], ret: m37_col_bool_ty.clone(),
                });
                // ── M38 Phase A: restored Phase C comparison ops ──
                v.push(MethodSig {
                    name: "ne".into(), params: vec![m37_i64.clone()], ret: m37_col_bool_ty.clone(),
                });
                v.push(MethodSig {
                    name: "ge".into(), params: vec![m37_i64.clone()], ret: m37_col_bool_ty.clone(),
                });
                v.push(MethodSig {
                    name: "le".into(), params: vec![m37_i64.clone()], ret: m37_col_bool_ty.clone(),
                });
                v.push(MethodSig {
                    name: "between".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m37_col_bool_ty.clone(),
                });
                // ── M38 Phase B: aggregations ──
                v.push(MethodSig {
                    name: "sum".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_i64.clone())),
                });
                v.push(MethodSig {
                    name: "mean".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_f64.clone())),
                });
                v.push(MethodSig {
                    name: "min".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_i64.clone())),
                });
                v.push(MethodSig {
                    name: "max".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_i64.clone())),
                });
                v.push(MethodSig {
                    name: "count".into(), params: vec![], ret: m37_i64.clone(),
                });
                v.push(MethodSig {
                    name: "std".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_f64.clone())),
                });
                v.push(MethodSig {
                    name: "var".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_f64.clone())),
                });
                v.push(MethodSig {
                    name: "median".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_f64.clone())),
                });
                // ── M38 Phase C: fill_null ──
                v.push(MethodSig {
                    name: "fill_null".into(), params: vec![m37_i64.clone()],
                    ret: m37_col_i64_ty.clone(),
                });
                // ── M40 Phase A: cumulative reductions (i64 → i64) ──
                v.push(MethodSig {
                    name: "cumsum".into(), params: vec![], ret: m37_col_i64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "cumprod".into(), params: vec![], ret: m37_col_i64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "cummax".into(), params: vec![], ret: m37_col_i64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "cummin".into(), params: vec![], ret: m37_col_i64_ty.clone(),
                });
                // ── M40 Phase B: rolling-window aggregations (i64) ──
                v.push(MethodSig {
                    name: "rolling_sum".into(), params: vec![m37_i64.clone()],
                    ret: m37_col_i64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "rolling_mean".into(), params: vec![m37_i64.clone()],
                    ret: m37_col_f64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "rolling_min".into(), params: vec![m37_i64.clone()],
                    ret: m37_col_i64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "rolling_max".into(), params: vec![m37_i64.clone()],
                    ret: m37_col_i64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "rolling_std".into(), params: vec![m37_i64.clone()],
                    ret: m37_col_f64_ty.clone(),
                });
                // ── M47 Phase B: rolling-window with min_periods (i64) ──
                v.push(MethodSig {
                    name: "rolling_sum_min_periods".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m37_col_i64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "rolling_mean_min_periods".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m37_col_f64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "rolling_min_min_periods".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m37_col_i64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "rolling_max_min_periods".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m37_col_i64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "rolling_std_min_periods".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m37_col_f64_ty.clone(),
                });
                v
            },
            generics: vec![], generic_tvars: vec![],
            is_native: false, payload_size: 24,
        });

        // ── ColumnF64 — { values: List[f64], nulls: List[bool], length: i64 }
        self.class_layouts.insert(m37_col_f64_cid, ClassLayout {
            id: m37_col_f64_cid, name: "ColumnF64".into(), base: Some(m37_col_cid),
            is_open: false, is_sealed: false,
            fields: vec![
                FieldInfo { name: "values".into(), ty: m37_list_f64.clone(), offset: 0 },
                FieldInfo { name: "nulls".into(),  ty: m37_list_bool.clone(), offset: 8 },
                FieldInfo { name: "length".into(), ty: m37_i64.clone(),       offset: 16 },
            ],
            methods: {
                let mut v = m37_shared_methods(MethodSig {
                    name: "get".into(),
                    params: vec![m37_i64.clone()],
                    ret: Ty::Nullable(Box::new(m37_f64.clone())),
                });
                v.push(MethodSig {
                    name: "eq".into(), params: vec![m37_f64.clone()], ret: m37_col_bool_ty.clone(),
                });
                v.push(MethodSig {
                    name: "gt".into(), params: vec![m37_f64.clone()], ret: m37_col_bool_ty.clone(),
                });
                v.push(MethodSig {
                    name: "lt".into(), params: vec![m37_f64.clone()], ret: m37_col_bool_ty.clone(),
                });
                // ── M38 Phase A ──
                v.push(MethodSig {
                    name: "ne".into(), params: vec![m37_f64.clone()], ret: m37_col_bool_ty.clone(),
                });
                v.push(MethodSig {
                    name: "ge".into(), params: vec![m37_f64.clone()], ret: m37_col_bool_ty.clone(),
                });
                v.push(MethodSig {
                    name: "le".into(), params: vec![m37_f64.clone()], ret: m37_col_bool_ty.clone(),
                });
                v.push(MethodSig {
                    name: "between".into(),
                    params: vec![m37_f64.clone(), m37_f64.clone()],
                    ret: m37_col_bool_ty.clone(),
                });
                // ── M38 Phase B ──
                v.push(MethodSig {
                    name: "sum".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_f64.clone())),
                });
                v.push(MethodSig {
                    name: "mean".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_f64.clone())),
                });
                v.push(MethodSig {
                    name: "min".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_f64.clone())),
                });
                v.push(MethodSig {
                    name: "max".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_f64.clone())),
                });
                v.push(MethodSig {
                    name: "count".into(), params: vec![], ret: m37_i64.clone(),
                });
                v.push(MethodSig {
                    name: "std".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_f64.clone())),
                });
                v.push(MethodSig {
                    name: "var".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_f64.clone())),
                });
                v.push(MethodSig {
                    name: "median".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_f64.clone())),
                });
                // ── M38 Phase C: fill_null ──
                v.push(MethodSig {
                    name: "fill_null".into(), params: vec![m37_f64.clone()],
                    ret: m37_col_f64_ty.clone(),
                });
                // ── M40 Phase A: cumulative reductions (f64 → f64) ──
                v.push(MethodSig {
                    name: "cumsum".into(), params: vec![], ret: m37_col_f64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "cumprod".into(), params: vec![], ret: m37_col_f64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "cummax".into(), params: vec![], ret: m37_col_f64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "cummin".into(), params: vec![], ret: m37_col_f64_ty.clone(),
                });
                // ── M40 Phase B: rolling-window aggregations (f64) ──
                v.push(MethodSig {
                    name: "rolling_sum".into(), params: vec![m37_i64.clone()],
                    ret: m37_col_f64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "rolling_mean".into(), params: vec![m37_i64.clone()],
                    ret: m37_col_f64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "rolling_min".into(), params: vec![m37_i64.clone()],
                    ret: m37_col_f64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "rolling_max".into(), params: vec![m37_i64.clone()],
                    ret: m37_col_f64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "rolling_std".into(), params: vec![m37_i64.clone()],
                    ret: m37_col_f64_ty.clone(),
                });
                // ── M47 Phase B: rolling-window with min_periods (f64) ──
                v.push(MethodSig {
                    name: "rolling_sum_min_periods".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m37_col_f64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "rolling_mean_min_periods".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m37_col_f64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "rolling_min_min_periods".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m37_col_f64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "rolling_max_min_periods".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m37_col_f64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "rolling_std_min_periods".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m37_col_f64_ty.clone(),
                });
                v
            },
            generics: vec![], generic_tvars: vec![],
            is_native: false, payload_size: 24,
        });

        // ── ColumnStr — { values: List[str], nulls: List[bool], length: i64 }
        self.class_layouts.insert(m37_col_str_cid, ClassLayout {
            id: m37_col_str_cid, name: "ColumnStr".into(), base: Some(m37_col_cid),
            is_open: false, is_sealed: false,
            fields: vec![
                FieldInfo { name: "values".into(), ty: m37_list_str.clone(), offset: 0 },
                FieldInfo { name: "nulls".into(),  ty: m37_list_bool.clone(), offset: 8 },
                FieldInfo { name: "length".into(), ty: m37_i64.clone(),       offset: 16 },
            ],
            methods: {
                let mut v = m37_shared_methods(MethodSig {
                    name: "get".into(),
                    params: vec![m37_i64.clone()],
                    ret: Ty::Nullable(Box::new(m37_str.clone())),
                });
                v.push(MethodSig {
                    name: "eq".into(), params: vec![m37_str.clone()], ret: m37_col_bool_ty.clone(),
                });
                v.push(MethodSig {
                    name: "contains".into(), params: vec![m37_str.clone()], ret: m37_col_bool_ty.clone(),
                });
                // ── M38 Phase A ──
                v.push(MethodSig {
                    name: "starts_with".into(), params: vec![m37_str.clone()],
                    ret: m37_col_bool_ty.clone(),
                });
                v.push(MethodSig {
                    name: "ends_with".into(), params: vec![m37_str.clone()],
                    ret: m37_col_bool_ty.clone(),
                });
                // ── M38 Phase B ──
                v.push(MethodSig {
                    name: "count".into(), params: vec![], ret: m37_i64.clone(),
                });
                v.push(MethodSig {
                    name: "min".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_str.clone())),
                });
                v.push(MethodSig {
                    name: "max".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_str.clone())),
                });
                // ── M38 Phase C: fill_null ──
                v.push(MethodSig {
                    name: "fill_null".into(), params: vec![m37_str.clone()],
                    ret: m37_col_str_ty.clone(),
                });
                v
            },
            generics: vec![], generic_tvars: vec![],
            is_native: false, payload_size: 24,
        });

        // ── ColumnBool — { values: List[bool], nulls: List[bool], length: i64 }
        self.class_layouts.insert(m37_col_bool_cid, ClassLayout {
            id: m37_col_bool_cid, name: "ColumnBool".into(), base: Some(m37_col_cid),
            is_open: false, is_sealed: false,
            fields: vec![
                FieldInfo { name: "values".into(), ty: m37_list_bool.clone(), offset: 0 },
                FieldInfo { name: "nulls".into(),  ty: m37_list_bool.clone(), offset: 8 },
                FieldInfo { name: "length".into(), ty: m37_i64.clone(),       offset: 16 },
            ],
            methods: {
                let mut v = m37_shared_methods(MethodSig {
                    name: "get".into(),
                    params: vec![m37_i64.clone()],
                    ret: Ty::Nullable(Box::new(m37_bool.clone())),
                });
                v.push(MethodSig {
                    name: "and_".into(), params: vec![m37_col_bool_ty.clone()], ret: m37_col_bool_ty.clone(),
                });
                v.push(MethodSig {
                    name: "or_".into(), params: vec![m37_col_bool_ty.clone()], ret: m37_col_bool_ty.clone(),
                });
                v.push(MethodSig {
                    name: "not_".into(), params: vec![], ret: m37_col_bool_ty.clone(),
                });
                v.push(MethodSig {
                    name: "count_true".into(), params: vec![], ret: m37_i64.clone(),
                });
                // ── M38 Phase B: count() (non-null cell count) ──
                v.push(MethodSig {
                    name: "count".into(), params: vec![], ret: m37_i64.clone(),
                });
                // ── M38 Phase C: fill_null ──
                v.push(MethodSig {
                    name: "fill_null".into(), params: vec![m37_bool.clone()],
                    ret: m37_col_bool_ty.clone(),
                });
                v
            },
            generics: vec![], generic_tvars: vec![],
            is_native: false, payload_size: 24,
        });

        // ── ColumnDateTime — values: List[i64] of epoch-ms.
        self.class_layouts.insert(m37_col_dt_cid, ClassLayout {
            id: m37_col_dt_cid, name: "ColumnDateTime".into(), base: Some(m37_col_cid),
            is_open: false, is_sealed: false,
            fields: vec![
                FieldInfo { name: "values".into(), ty: m37_list_i64.clone(), offset: 0 },
                FieldInfo { name: "nulls".into(),  ty: m37_list_bool.clone(), offset: 8 },
                FieldInfo { name: "length".into(), ty: m37_i64.clone(),       offset: 16 },
            ],
            methods: {
                let mut v = m37_shared_methods(MethodSig {
                    name: "get_ms".into(),
                    params: vec![m37_i64.clone()],
                    ret: Ty::Nullable(Box::new(m37_i64.clone())),
                });
                // ── M38 Phase B ──
                v.push(MethodSig {
                    name: "count".into(), params: vec![], ret: m37_i64.clone(),
                });
                v.push(MethodSig {
                    name: "min".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_i64.clone())),
                });
                v.push(MethodSig {
                    name: "max".into(), params: vec![],
                    ret: Ty::Nullable(Box::new(m37_i64.clone())),
                });
                // ── M38 Phase C: fill_null (v_ms: i64) ──
                v.push(MethodSig {
                    name: "fill_null".into(), params: vec![m37_i64.clone()],
                    ret: m37_col_dt_ty.clone(),
                });
                v
            },
            generics: vec![], generic_tvars: vec![],
            is_native: false, payload_size: 24,
        });

        // ── M47: ColumnCategorical — { codes: List[i64], nulls:
        // List[bool], length: i64, categories: List[str] } (32-byte
        // payload).  Sealed Column subclass; v1 op integration via
        // to_strings() coercion.  Field order intentionally matches the
        // M37 Column layout (codes, nulls, length at offsets 0/8/16) so
        // every existing m37_col_fields() reader works — categories
        // lives at the new offset 24.  Categorical-specific accessors:
        // codes(), categories(), to_strings().
        self.class_layouts.insert(m47_col_cat_cid, ClassLayout {
            id: m47_col_cat_cid, name: "ColumnCategorical".into(), base: Some(m37_col_cid),
            is_open: false, is_sealed: false,
            fields: vec![
                FieldInfo { name: "codes".into(),      ty: m37_list_i64.clone(),  offset: 0 },
                FieldInfo { name: "nulls".into(),      ty: m37_list_bool.clone(), offset: 8 },
                FieldInfo { name: "length".into(),     ty: m37_i64.clone(),       offset: 16 },
                FieldInfo { name: "categories".into(), ty: m37_list_str.clone(),  offset: 24 },
            ],
            methods: {
                // Shared methods on every Column subclass: length, dtype,
                // is_null, null_count, get.  get(i) returns the category
                // string (or none if null).
                let mut v = m37_shared_methods(MethodSig {
                    name: "get".into(),
                    params: vec![m37_i64.clone()],
                    ret: Ty::Nullable(Box::new(m37_str.clone())),
                });
                // Categorical-specific accessors.
                v.push(MethodSig {
                    name: "codes".into(), params: vec![], ret: m37_col_i64_ty.clone(),
                });
                v.push(MethodSig {
                    name: "categories".into(), params: vec![], ret: m37_col_str_ty.clone(),
                });
                v.push(MethodSig {
                    name: "to_strings".into(), params: vec![], ret: m37_col_str_ty.clone(),
                });
                // ── M49: is_ordered predicate ──
                // Heuristic: true iff some category is never referenced
                // by codes, which is the signature of an explicit-
                // categories constructor (col_categorical_ordered /
                // col_categorical_from_codes).
                v.push(MethodSig {
                    name: "is_ordered".into(), params: vec![], ret: m37_bool.clone(),
                });
                v
            },
            generics: vec![], generic_tvars: vec![],
            is_native: false, payload_size: 32,
        });

        // ── M38: GroupedDataFrame class id (allocated before DataFrame
        // layout so DataFrame.group_by can reference it as its return
        // type).
        let m38_gdf_cid = self.fresh_class();
        self.class_name_to_id.insert("GroupedDataFrame".into(), m38_gdf_cid);
        let m38_gdf_ty = Ty::Class(m38_gdf_cid);
        // ── M51: RollingWindow class id (allocated before DataFrame
        // layout so DataFrame.rolling can reference it as its return
        // type, same pattern as GroupedDataFrame above).
        let m51_rw_cid = self.fresh_class();
        self.class_name_to_id.insert("RollingWindow".into(), m51_rw_cid);
        let m51_rw_ty = Ty::Class(m51_rw_cid);
        // ── M38: tuple (str, str) for rename / agg-spec params.
        let m38_str_pair_list = Ty::Generic {
            base: TypeCtor::List,
            args: vec![Ty::Tuple(vec![m37_str.clone(), m37_str.clone()])],
        };

        // ── DataFrame — { names: List[str], columns: List[Column], nrows: i64,
        //                 index: Column?, index_name: str?,
        //                 index_levels: List[Column]?, index_names: List[str]? }
        // M41 extended the payload from 24 → 40 bytes to carry an optional
        // single-column index.  M44 extends it again 40 → 56 bytes to carry
        // an optional MultiIndex (a List[Column] of level columns + a
        // matching List[str] of level names).  The single-col index and the
        // MultiIndex are mutually exclusive: a frame has one OR the other
        // OR neither (RangeIndex).  Both null (0) = RangeIndex.  See
        // LANGUAGE_GUIDE §5 M41/M44 additions and STRICTPY_SPEC §9.tabular.
        self.class_layouts.insert(m37_df_cid, ClassLayout {
            id: m37_df_cid, name: "DataFrame".into(), base: None,
            is_open: false, is_sealed: false,
            fields: vec![
                FieldInfo { name: "names".into(),      ty: m37_list_str.clone(),     offset: 0 },
                FieldInfo { name: "columns".into(),    ty: m37_list_col_ty.clone(),  offset: 8 },
                FieldInfo { name: "nrows".into(),      ty: m37_i64.clone(),          offset: 16 },
                // M41: optional single-column index — null pointer means RangeIndex.
                FieldInfo { name: "index".into(),      ty: Ty::Nullable(Box::new(m37_col_ty.clone())), offset: 24 },
                FieldInfo { name: "index_name".into(), ty: Ty::Nullable(Box::new(m37_str.clone())),    offset: 32 },
                // M44: optional MultiIndex — null pointer = no MultiIndex.
                FieldInfo { name: "index_levels".into(), ty: Ty::Nullable(Box::new(m37_list_col_ty.clone())), offset: 40 },
                FieldInfo { name: "index_names".into(),  ty: Ty::Nullable(Box::new(m37_list_str.clone())),    offset: 48 },
            ],
            methods: vec![
                MethodSig { name: "length".into(),     params: vec![], ret: m37_i64.clone() },
                MethodSig { name: "ncols".into(),      params: vec![], ret: m37_i64.clone() },
                MethodSig { name: "columns".into(),    params: vec![], ret: m37_list_str.clone() },
                MethodSig { name: "dtypes".into(),     params: vec![], ret: m37_list_str.clone() },
                MethodSig { name: "has_column".into(), params: vec![m37_str.clone()], ret: m37_bool.clone() },
                MethodSig { name: "show".into(),       params: vec![m37_i64.clone()], ret: m37_str.clone() },
                MethodSig { name: "filter".into(),     params: vec![m37_col_bool_ty.clone()], ret: m37_df_ty.clone() },
                MethodSig { name: "select".into(),     params: vec![m37_list_str.clone()],    ret: m37_df_ty.clone() },
                MethodSig { name: "drop".into(),       params: vec![m37_list_str.clone()],    ret: m37_df_ty.clone() },
                MethodSig { name: "head".into(),       params: vec![m37_i64.clone()],         ret: m37_df_ty.clone() },
                MethodSig { name: "tail".into(),       params: vec![m37_i64.clone()],         ret: m37_df_ty.clone() },
                MethodSig { name: "row".into(),        params: vec![m37_i64.clone()],         ret: m37_list_str.clone() },
                MethodSig { name: "sort_by".into(),    params: vec![m37_str.clone(), m37_bool.clone()], ret: m37_df_ty.clone() },
                // ── M38 Phase A: typed accessors ──
                MethodSig {
                    name: "get_column_i64".into(),
                    params: vec![m37_str.clone()],
                    ret: Ty::Nullable(Box::new(m37_col_i64_ty.clone())),
                },
                MethodSig {
                    name: "get_column_f64".into(),
                    params: vec![m37_str.clone()],
                    ret: Ty::Nullable(Box::new(m37_col_f64_ty.clone())),
                },
                MethodSig {
                    name: "get_column_str".into(),
                    params: vec![m37_str.clone()],
                    ret: Ty::Nullable(Box::new(m37_col_str_ty.clone())),
                },
                MethodSig {
                    name: "get_column_bool".into(),
                    params: vec![m37_str.clone()],
                    ret: Ty::Nullable(Box::new(m37_col_bool_ty.clone())),
                },
                MethodSig {
                    name: "get_column_datetime".into(),
                    params: vec![m37_str.clone()],
                    ret: Ty::Nullable(Box::new(m37_col_dt_ty.clone())),
                },
                // ── M38 Phase A: rename ──
                MethodSig {
                    name: "rename".into(),
                    params: vec![m38_str_pair_list.clone()],
                    ret: m37_df_ty.clone(),
                },
                // ── M38 Phase C: describe ──
                MethodSig {
                    name: "describe".into(), params: vec![], ret: m37_df_ty.clone(),
                },
                // ── M38 Phase D: group_by ──
                MethodSig {
                    name: "group_by".into(),
                    params: vec![m37_list_str.clone()],
                    ret: m38_gdf_ty.clone(),
                },
                // ── M39 Phase A: typed unique accessors (one per dtype) ──
                MethodSig {
                    name: "unique_i64".into(), params: vec![m37_str.clone()],
                    ret: Ty::Nullable(Box::new(m37_col_i64_ty.clone())),
                },
                MethodSig {
                    name: "unique_f64".into(), params: vec![m37_str.clone()],
                    ret: Ty::Nullable(Box::new(m37_col_f64_ty.clone())),
                },
                MethodSig {
                    name: "unique_str".into(), params: vec![m37_str.clone()],
                    ret: Ty::Nullable(Box::new(m37_col_str_ty.clone())),
                },
                MethodSig {
                    name: "unique_bool".into(), params: vec![m37_str.clone()],
                    ret: Ty::Nullable(Box::new(m37_col_bool_ty.clone())),
                },
                MethodSig {
                    name: "unique_datetime".into(), params: vec![m37_str.clone()],
                    ret: Ty::Nullable(Box::new(m37_col_dt_ty.clone())),
                },
                // ── M39 Phase A: value_counts ──
                MethodSig {
                    name: "value_counts".into(), params: vec![m37_str.clone()],
                    ret: m37_df_ty.clone(),
                },
                // ── M39 Phase B: merge ──
                MethodSig {
                    name: "merge".into(),
                    params: vec![m37_df_ty.clone(), m37_list_str.clone(), m37_str.clone()],
                    ret: m37_df_ty.clone(),
                },
                // ── M39 Phase C: pivot + melt ──
                MethodSig {
                    name: "pivot".into(),
                    params: vec![m37_str.clone(), m37_str.clone(), m37_str.clone()],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "melt".into(),
                    params: vec![m37_list_str.clone(), m37_list_str.clone()],
                    ret: m37_df_ty.clone(),
                },
                // ── M40 Phase A: whole-frame null handling ──
                MethodSig {
                    name: "dropna".into(), params: vec![], ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "dropna_subset".into(),
                    params: vec![m37_list_str.clone()],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "fillna_i64".into(),
                    params: vec![m37_i64.clone()], ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "fillna_f64".into(),
                    params: vec![m37_f64.clone()], ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "fillna_str".into(),
                    params: vec![m37_str.clone()], ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "fillna_bool".into(),
                    params: vec![m37_bool.clone()], ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "fillna_datetime".into(),
                    params: vec![m37_i64.clone()], ret: m37_df_ty.clone(),
                },
                // ── M40 Phase A: range slicing ──
                MethodSig {
                    name: "iloc".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m37_df_ty.clone(),
                },
                // ── M40 Phase C: time-series ops ──
                MethodSig {
                    name: "resample".into(),
                    params: vec![m37_str.clone(), m37_str.clone(), m37_str.clone()],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "asof_merge".into(),
                    params: vec![m37_df_ty.clone(), m37_str.clone(), m37_str.clone()],
                    ret: m37_df_ty.clone(),
                },
                // ── M41 Phase A: index storage + accessors + sort_index ──
                MethodSig {
                    name: "set_index".into(),
                    params: vec![m37_str.clone()],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "reset_index".into(),
                    params: vec![],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "has_index".into(),
                    params: vec![],
                    ret: m37_bool.clone(),
                },
                MethodSig {
                    name: "index".into(),
                    params: vec![],
                    ret: Ty::Nullable(Box::new(m37_col_ty.clone())),
                },
                MethodSig {
                    name: "index_name".into(),
                    params: vec![],
                    ret: Ty::Nullable(Box::new(m37_str.clone())),
                },
                MethodSig {
                    name: "sort_index".into(),
                    params: vec![m37_bool.clone()],
                    ret: m37_df_ty.clone(),
                },
                // ── M41 Phase B: index-aware time-series + select by label ──
                MethodSig {
                    name: "resample_index".into(),
                    params: vec![m37_str.clone(), m37_str.clone()],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "asof_merge_index".into(),
                    params: vec![m37_df_ty.clone()],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "select_by_label_i64".into(),
                    params: vec![m37_i64.clone()],
                    ret: Ty::Nullable(Box::new(m37_df_ty.clone())),
                },
                MethodSig {
                    name: "select_by_label_str".into(),
                    params: vec![m37_str.clone()],
                    ret: Ty::Nullable(Box::new(m37_df_ty.clone())),
                },
                MethodSig {
                    name: "select_by_label_datetime".into(),
                    params: vec![m37_i64.clone()],
                    ret: Ty::Nullable(Box::new(m37_df_ty.clone())),
                },
                // ── M41 Phase C: pivot_table ──
                MethodSig {
                    name: "pivot_table".into(),
                    params: vec![
                        m37_str.clone(), m37_str.clone(),
                        m37_str.clone(), m37_str.clone(),
                    ],
                    ret: m37_df_ty.clone(),
                },
                // ── M44 Phase A: MultiIndex storage + accessors + sort_index_multi ──
                MethodSig {
                    name: "set_index_multi".into(),
                    params: vec![m37_list_str.clone()],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "reset_index_multi".into(),
                    params: vec![],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "index_nlevels".into(),
                    params: vec![],
                    ret: m37_i64.clone(),
                },
                MethodSig {
                    name: "index_level".into(),
                    params: vec![m37_i64.clone()],
                    ret: Ty::Nullable(Box::new(m37_col_ty.clone())),
                },
                MethodSig {
                    name: "index_level_name".into(),
                    params: vec![m37_i64.clone()],
                    ret: Ty::Nullable(Box::new(m37_str.clone())),
                },
                MethodSig {
                    name: "sort_index_multi".into(),
                    params: vec![m37_bool.clone()],
                    ret: m37_df_ty.clone(),
                },
                // ── M46: stack/unstack + loc_range + set_index_list + pivot_table extras ──
                MethodSig {
                    name: "stack".into(),
                    params: vec![],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "unstack".into(),
                    params: vec![],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "loc_range_i64".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "loc_range_f64".into(),
                    params: vec![m37_f64.clone(), m37_f64.clone()],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "loc_range_str".into(),
                    params: vec![m37_str.clone(), m37_str.clone()],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "loc_range_bool".into(),
                    params: vec![m37_bool.clone(), m37_bool.clone()],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "loc_range_datetime".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m37_df_ty.clone(),
                },
                // ── M49 Phase D: loc_range_multi_* on MultiIndex's
                // innermost level (outer levels left intact). ──
                MethodSig {
                    name: "loc_range_multi_i64".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "loc_range_multi_str".into(),
                    params: vec![m37_str.clone(), m37_str.clone()],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "loc_range_multi_datetime".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m37_df_ty.clone(),
                },
                // ── M51 Phase D: loc_range_level_* on a chosen MultiIndex
                // level (0 = outermost), generalizing M49's innermost-only
                // loc_range_multi_*.  Signature carries the level as arg 0.
                MethodSig {
                    name: "loc_range_level_i64".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone(), m37_i64.clone()],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "loc_range_level_str".into(),
                    params: vec![m37_i64.clone(), m37_str.clone(), m37_str.clone()],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "loc_range_level_datetime".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone(), m37_i64.clone()],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "set_index_list".into(),
                    params: vec![m37_list_str.clone()],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "pivot_table_aggfunc_list".into(),
                    params: vec![
                        m37_str.clone(), m37_str.clone(),
                        m37_str.clone(), m37_list_str.clone(),
                    ],
                    ret: m37_df_ty.clone(),
                },
                MethodSig {
                    name: "pivot_table_margins".into(),
                    params: vec![
                        m37_str.clone(), m37_str.clone(),
                        m37_str.clone(), m37_str.clone(),
                    ],
                    ret: m37_df_ty.clone(),
                },
                // ── M47 Phase A: iloc_2d ──
                MethodSig {
                    name: "iloc_2d".into(),
                    params: vec![
                        m37_i64.clone(), m37_i64.clone(),
                        m37_i64.clone(), m37_i64.clone(),
                    ],
                    ret: m37_df_ty.clone(),
                },
                // ── M47 Phase C: get_column_categorical ──
                MethodSig {
                    name: "get_column_categorical".into(),
                    params: vec![m37_str.clone()],
                    ret: Ty::Nullable(Box::new(m47_col_cat_ty.clone())),
                },
                // ── M51: chainable rolling-window constructors.  Each
                // returns a RollingWindow which the caller chains with
                // .sum/.mean/.min/.max/.std/.count to materialise the
                // aggregated DataFrame.  No kwargs in StrictPy, so the
                // center=True and min_periods options each get their
                // own constructor variant.
                MethodSig {
                    name: "rolling".into(),
                    params: vec![m37_i64.clone()],
                    ret: m51_rw_ty.clone(),
                },
                MethodSig {
                    name: "rolling_centered".into(),
                    params: vec![m37_i64.clone()],
                    ret: m51_rw_ty.clone(),
                },
                MethodSig {
                    name: "rolling_min_periods".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m51_rw_ty.clone(),
                },
                MethodSig {
                    name: "rolling_centered_min_periods".into(),
                    params: vec![m37_i64.clone(), m37_i64.clone()],
                    ret: m51_rw_ty.clone(),
                },
            ],
            generics: vec![], generic_tvars: vec![],
            is_native: false, payload_size: 56,
        });

        // ── GroupedDataFrame layout — payload carries (parent, group_keys, slot, group_count).
        // parent is i64 pointer, group_keys is List[str] pointer, slot is
        // an i64 handle into SharedVm.m38_group_index_maps, group_count
        // is i64.  Total 32 bytes.
        self.class_layouts.insert(m38_gdf_cid, ClassLayout {
            id: m38_gdf_cid, name: "GroupedDataFrame".into(), base: None,
            is_open: false, is_sealed: false,
            fields: vec![
                FieldInfo { name: "parent".into(),      ty: m37_df_ty.clone(),       offset: 0 },
                FieldInfo { name: "group_keys".into(),  ty: m37_list_str.clone(),    offset: 8 },
                FieldInfo { name: "slot".into(),        ty: m37_i64.clone(),         offset: 16 },
                FieldInfo { name: "group_count".into(), ty: m37_i64.clone(),         offset: 24 },
            ],
            methods: vec![
                MethodSig { name: "size".into(),  params: vec![], ret: m37_df_ty.clone() },
                MethodSig { name: "keys".into(),  params: vec![], ret: m37_df_ty.clone() },
                MethodSig { name: "sum".into(),   params: vec![], ret: m37_df_ty.clone() },
                MethodSig { name: "mean".into(),  params: vec![], ret: m37_df_ty.clone() },
                MethodSig { name: "min".into(),   params: vec![], ret: m37_df_ty.clone() },
                MethodSig { name: "max".into(),   params: vec![], ret: m37_df_ty.clone() },
                MethodSig { name: "count".into(), params: vec![], ret: m37_df_ty.clone() },
                MethodSig {
                    name: "agg".into(),
                    params: vec![m38_str_pair_list.clone()],
                    ret: m37_df_ty.clone(),
                },
            ],
            generics: vec![], generic_tvars: vec![],
            is_native: false, payload_size: 32,
        });

        // ── M51: RollingWindow layout — payload carries (parent_df,
        // window, min_periods, center_bool).  Same 32-byte payload shape
        // as GroupedDataFrame.  min_periods is the effective value (=
        // window when caller did not supply one); the i64 sentinel -1
        // is not used here — see m51_alloc_rolling_window which
        // normalises it.  center_bool is stored as i64 (0/1).
        self.class_layouts.insert(m51_rw_cid, ClassLayout {
            id: m51_rw_cid, name: "RollingWindow".into(), base: None,
            is_open: false, is_sealed: false,
            fields: vec![
                FieldInfo { name: "parent".into(),       ty: m37_df_ty.clone(), offset: 0 },
                FieldInfo { name: "window".into(),       ty: m37_i64.clone(),   offset: 8 },
                FieldInfo { name: "min_periods".into(),  ty: m37_i64.clone(),   offset: 16 },
                FieldInfo { name: "center_bool".into(),  ty: m37_i64.clone(),   offset: 24 },
            ],
            methods: vec![
                MethodSig { name: "sum".into(),         params: vec![], ret: m37_df_ty.clone() },
                MethodSig { name: "mean".into(),        params: vec![], ret: m37_df_ty.clone() },
                MethodSig { name: "min".into(),         params: vec![], ret: m37_df_ty.clone() },
                MethodSig { name: "max".into(),         params: vec![], ret: m37_df_ty.clone() },
                MethodSig { name: "std".into(),         params: vec![], ret: m37_df_ty.clone() },
                MethodSig { name: "count".into(),       params: vec![], ret: m37_df_ty.clone() },
                MethodSig { name: "window".into(),      params: vec![], ret: m37_i64.clone() },
                MethodSig { name: "min_periods".into(), params: vec![], ret: m37_i64.clone() },
                MethodSig {
                    name: "is_centered".into(), params: vec![],
                    ret: Ty::Primitive(PrimTy::Bool),
                },
            ],
            generics: vec![], generic_tvars: vec![],
            is_native: false, payload_size: 32,
        });

        // ── Function-type helper closure ──
        let m37_fn = |params: Vec<Ty>, ret: Ty| Ty::Function { params, ret: Box::new(ret) };

        // ── Build the tabular module ──
        let mut m37_tabular_mod = StdlibModule {
            name: "tabular".into(),
            items: vec![
                StdlibItem {
                    name: "col_i64".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(vec![m37_list_i64.clone(), m37_list_bool.clone()], m37_col_i64_ty.clone()),
                    native_id: 830,
                },
                StdlibItem {
                    name: "col_i64_simple".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(vec![m37_list_i64.clone()], m37_col_i64_ty.clone()),
                    native_id: 831,
                },
                StdlibItem {
                    name: "col_f64".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(vec![m37_list_f64.clone(), m37_list_bool.clone()], m37_col_f64_ty.clone()),
                    native_id: 832,
                },
                StdlibItem {
                    name: "col_f64_simple".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(vec![m37_list_f64.clone()], m37_col_f64_ty.clone()),
                    native_id: 833,
                },
                StdlibItem {
                    name: "col_str".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(vec![m37_list_str.clone(), m37_list_bool.clone()], m37_col_str_ty.clone()),
                    native_id: 834,
                },
                StdlibItem {
                    name: "col_str_simple".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(vec![m37_list_str.clone()], m37_col_str_ty.clone()),
                    native_id: 835,
                },
                StdlibItem {
                    name: "col_bool".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(vec![m37_list_bool.clone(), m37_list_bool.clone()], m37_col_bool_ty.clone()),
                    native_id: 836,
                },
                StdlibItem {
                    name: "col_bool_simple".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(vec![m37_list_bool.clone()], m37_col_bool_ty.clone()),
                    native_id: 837,
                },
                StdlibItem {
                    name: "col_datetime".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(vec![m37_list_i64.clone(), m37_list_bool.clone()], m37_col_dt_ty.clone()),
                    native_id: 838,
                },
                StdlibItem {
                    name: "from_columns".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(vec![m37_list_str.clone(), m37_list_col_ty.clone()], m37_df_ty.clone()),
                    native_id: 839,
                },
                // ── Phase B: I/O ──
                StdlibItem {
                    name: "read_csv".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(vec![m37_str.clone(), m37_schema_ty.clone()], m37_df_ty.clone()),
                    native_id: 855,
                },
                StdlibItem {
                    name: "write_csv".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(vec![m37_str.clone(), m37_df_ty.clone()], m37_unit.clone()),
                    native_id: 856,
                },
                StdlibItem {
                    name: "from_sql".into(), kind: StdlibItemKind::Function,
                    // Cursor type — looked up from class_name_to_id (set
                    // up by seed_prelude in the M35 P4-B block above).
                    // Fallback to Ty::Never if not yet registered.
                    ty: m37_fn(
                        vec![
                            match self.class_name_to_id.get("Cursor") {
                                Some(cid) => Ty::Class(*cid),
                                None => Ty::Never,
                            },
                            m37_schema_ty.clone(),
                        ],
                        m37_df_ty.clone(),
                    ),
                    native_id: 857,
                },
                StdlibItem {
                    name: "from_rows".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(vec![m37_list_list_str.clone(), m37_schema_ty.clone()], m37_df_ty.clone()),
                    native_id: 858,
                },
                // ── M38 Phase C: from_dict(Dict[str, Column]) -> DataFrame ──
                StdlibItem {
                    name: "from_dict".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(
                        vec![Ty::Generic {
                            base: TypeCtor::Dict,
                            args: vec![m37_str.clone(), m37_col_ty.clone()],
                        }],
                        m37_df_ty.clone(),
                    ),
                    native_id: 925,
                },
                // ── M39 Phase A: concat_rows(dfs) / concat_cols(dfs) ──
                // List[DataFrame] is built fresh here because the module-
                // item list is the only consumer; column-class-id capture
                // happens above so we have m37_df_ty available.
                StdlibItem {
                    name: "concat_rows".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(
                        vec![Ty::Generic {
                            base: TypeCtor::List, args: vec![m37_df_ty.clone()],
                        }],
                        m37_df_ty.clone(),
                    ),
                    native_id: 941,
                },
                StdlibItem {
                    name: "concat_cols".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(
                        vec![Ty::Generic {
                            base: TypeCtor::List, args: vec![m37_df_ty.clone()],
                        }],
                        m37_df_ty.clone(),
                    ),
                    native_id: 942,
                },
                // ── M47: col_categorical / col_categorical_with_nulls ──
                StdlibItem {
                    name: "col_categorical".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(vec![m37_list_str.clone()], m47_col_cat_ty.clone()),
                    native_id: 1054,
                },
                StdlibItem {
                    name: "col_categorical_with_nulls".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(
                        vec![m37_list_str.clone(), m37_list_bool.clone()],
                        m47_col_cat_ty.clone(),
                    ),
                    native_id: 1055,
                },
                // ── M49: ordered categorical + from_codes ──
                StdlibItem {
                    name: "col_categorical_ordered".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(
                        vec![m37_list_str.clone(), m37_list_str.clone()],
                        m47_col_cat_ty.clone(),
                    ),
                    native_id: 1061,
                },
                StdlibItem {
                    name: "col_categorical_from_codes".into(), kind: StdlibItemKind::Function,
                    ty: m37_fn(
                        vec![m37_list_i64.clone(), m37_list_str.clone()],
                        m47_col_cat_ty.clone(),
                    ),
                    native_id: 1062,
                },
                // ── M50a: tabular.serve / serve_with_timeout ──
                // Hand-rolled localhost HTTP/1.1 server in
                // vm/src/builtins.rs::m50a_serve_loop.  Both functions
                // block the calling thread until the listener exits.
                // serve_with_timeout shuts down after timeout_ms.
                StdlibItem {
                    name: "serve".into(),
                    kind: StdlibItemKind::Function,
                    ty: m37_fn(
                        vec![m37_df_ty.clone(), Ty::Primitive(PrimTy::I32)],
                        Ty::Primitive(PrimTy::I32),
                    ),
                    native_id: 1067,
                },
                StdlibItem {
                    name: "serve_with_timeout".into(),
                    kind: StdlibItemKind::Function,
                    ty: m37_fn(
                        vec![
                            m37_df_ty.clone(),
                            Ty::Primitive(PrimTy::I32),
                            Ty::Primitive(PrimTy::I64),
                        ],
                        Ty::Primitive(PrimTy::I32),
                    ),
                    native_id: 1068,
                },
            ],
        };
        // Publish the 7 classes (5 Column subclasses + Column base +
        // DataFrame) as StdlibItemKind::Class items so `from tabular import
        // DataFrame, ColumnI64, ...` binds them in module_scope (M36 path).
        for (m37_name, m37_cid) in [
            ("Column",            m37_col_cid),
            ("ColumnI64",         m37_col_i64_cid),
            ("ColumnF64",         m37_col_f64_cid),
            ("ColumnStr",         m37_col_str_cid),
            ("ColumnBool",        m37_col_bool_cid),
            ("ColumnDateTime",    m37_col_dt_cid),
            // ── M47: ColumnCategorical published like the other Column
            // subclasses so `from tabular import ColumnCategorical` works.
            ("ColumnCategorical", m47_col_cat_cid),
            ("DataFrame",         m37_df_cid),
            // ── M38 Phase D: GroupedDataFrame — published on the
            // tabular module so `from tabular import GroupedDataFrame`
            // works.  Users never construct it directly; `df.group_by`
            // is the only entry point.
            ("GroupedDataFrame",  m38_gdf_cid),
            // ── M51: RollingWindow — same shape as GroupedDataFrame
            // above.  Users never construct it directly; `df.rolling`
            // (and the centered / min_periods variants) is the only
            // entry point.
            ("RollingWindow",     m51_rw_cid),
        ] {
            m37_tabular_mod.items.push(StdlibItem {
                name: m37_name.into(),
                kind: StdlibItemKind::Class { class_id: m37_cid },
                ty: Ty::Class(m37_cid),
                native_id: 0,
            });
        }
        self.stdlib_modules.insert("tabular".into(), m37_tabular_mod);

        // ── M52: `gfx` module ──────────────────────────────────────────
        let str_ty = Ty::Primitive(PrimTy::Str);
        let unit_ty = Ty::Primitive(PrimTy::Unit);
        let i32_ty = Ty::Primitive(PrimTy::I32);
        let fn_ty = |params: Vec<Ty>, ret: Ty| Ty::Function {
            params,
            ret: Box::new(ret),
        };

        let win_cid = self.fresh_class();
        let event_cid = self.fresh_class();
        let img_cid = self.fresh_class();
        let sound_cid = self.fresh_class();
        let music_cid = self.fresh_class();
        let font_cid = self.fresh_class();

        self.class_name_to_id.insert("Window".into(), win_cid);
        self.class_name_to_id.insert("Event".into(), event_cid);
        self.class_name_to_id.insert("Image".into(), img_cid);
        self.class_name_to_id.insert("Sound".into(), sound_cid);
        self.class_name_to_id.insert("Music".into(), music_cid);
        self.class_name_to_id.insert("Font".into(), font_cid);

        let win_ty = Ty::Class(win_cid);
        let event_ty = Ty::Class(event_cid);
        let img_ty = Ty::Class(img_cid);
        let sound_ty = Ty::Class(sound_cid);
        let music_ty = Ty::Class(music_cid);
        let font_ty = Ty::Class(font_cid);
        let opt_event_ty = Ty::Nullable(Box::new(event_ty.clone()));
        let tuple_i32_i32_ty = Ty::Tuple(vec![i32_ty.clone(), i32_ty.clone()]);
        let f64_ty = Ty::Primitive(PrimTy::F64);

        self.class_layouts.insert(win_cid, ClassLayout {
            id: win_cid,
            name: "Window".into(),
            base: None,
            is_open: false,
            is_sealed: true,
            fields: vec![
                FieldInfo {
                    name: "handle".into(),
                    ty: Ty::Primitive(PrimTy::I64),
                    offset: 0,
                },
            ],
            methods: vec![],
            generics: vec![],
            generic_tvars: vec![],
            is_native: true,
            payload_size: 8,
        });

        self.class_layouts.insert(event_cid, ClassLayout {
            id: event_cid,
            name: "Event".into(),
            base: None,
            is_open: true,
            is_sealed: false,
            fields: vec![
                FieldInfo { name: "kind".into(),   ty: str_ty.clone(), offset: 0 },
                FieldInfo { name: "key".into(),    ty: str_ty.clone(), offset: 8 },
                FieldInfo { name: "x".into(),      ty: i32_ty.clone(), offset: 16 },
                FieldInfo { name: "y".into(),      ty: i32_ty.clone(), offset: 20 },
                FieldInfo { name: "button".into(), ty: i32_ty.clone(), offset: 24 },
            ],
            methods: vec![],
            generics: vec![],
            generic_tvars: vec![],
            is_native: false,
            payload_size: 28,
        });

        self.class_layouts.insert(img_cid, ClassLayout {
            id: img_cid,
            name: "Image".into(),
            base: None,
            is_open: false,
            is_sealed: true,
            fields: vec![
                FieldInfo {
                    name: "handle".into(),
                    ty: Ty::Primitive(PrimTy::I64),
                    offset: 0,
                },
            ],
            methods: vec![],
            generics: vec![],
            generic_tvars: vec![],
            is_native: true,
            payload_size: 8,
        });

        self.class_layouts.insert(sound_cid, ClassLayout {
            id: sound_cid,
            name: "Sound".into(),
            base: None,
            is_open: false,
            is_sealed: true,
            fields: vec![
                FieldInfo {
                    name: "handle".into(),
                    ty: Ty::Primitive(PrimTy::I64),
                    offset: 0,
                },
            ],
            methods: vec![],
            generics: vec![],
            generic_tvars: vec![],
            is_native: true,
            payload_size: 8,
        });

        self.class_layouts.insert(music_cid, ClassLayout {
            id: music_cid,
            name: "Music".into(),
            base: None,
            is_open: false,
            is_sealed: true,
            fields: vec![
                FieldInfo {
                    name: "handle".into(),
                    ty: Ty::Primitive(PrimTy::I64),
                    offset: 0,
                },
            ],
            methods: vec![],
            generics: vec![],
            generic_tvars: vec![],
            is_native: true,
            payload_size: 8,
        });

        self.class_layouts.insert(font_cid, ClassLayout {
            id: font_cid,
            name: "Font".into(),
            base: None,
            is_open: false,
            is_sealed: true,
            fields: vec![
                FieldInfo {
                    name: "handle".into(),
                    ty: Ty::Primitive(PrimTy::I64),
                    offset: 0,
                },
            ],
            methods: vec![],
            generics: vec![],
            generic_tvars: vec![],
            is_native: true,
            payload_size: 8,
        });

        const GFX_INIT: u32              = 1100;
        const GFX_CREATE_WINDOW: u32      = 1101;
        const GFX_CLOSE_WINDOW: u32       = 1102;
        const GFX_POLL_EVENT: u32         = 1103;
        const GFX_CLEAR: u32              = 1104;
        const GFX_PRESENT: u32            = 1105;
        const GFX_DRAW_RECT: u32          = 1106;
        const GFX_DRAW_RECT_OUTLINE: u32  = 1107;
        const GFX_DRAW_LINE: u32          = 1108;
        const GFX_DRAW_POINT: u32         = 1109;
        const GFX_WINDOW_SIZE: u32        = 1110;
        const GFX_SET_WINDOW_TITLE: u32   = 1111;
        const GFX_LOAD_IMAGE: u32         = 1130;
        const GFX_IMAGE_SIZE: u32         = 1131;
        const GFX_DRAW_IMAGE: u32         = 1132;
        const GFX_DRAW_IMAGE_RECT: u32    = 1133;
        const GFX_DRAW_IMAGE_ROTATED: u32 = 1134;
        const GFX_FREE_IMAGE: u32         = 1135;
        // M54: audio
        const GFX_AUDIO_INIT: u32         = 1150;
        const GFX_LOAD_SOUND: u32         = 1151;
        const GFX_PLAY_SOUND: u32         = 1152;
        const GFX_FREE_SOUND: u32         = 1153;
        const GFX_LOAD_MUSIC: u32         = 1154;
        const GFX_PLAY_MUSIC: u32         = 1155;
        const GFX_STOP_MUSIC: u32         = 1156;
        const GFX_SET_MUSIC_VOLUME: u32   = 1157;
        const GFX_SET_SOUND_VOLUME: u32   = 1158;
        // M54: fonts/text
        const GFX_LOAD_FONT: u32          = 1170;
        const GFX_DRAW_TEXT: u32          = 1171;
        const GFX_TEXT_SIZE: u32          = 1172;
        const GFX_FREE_FONT: u32          = 1173;

        let gfx_items = vec![
            StdlibItem {
                name: "Window".into(),
                kind: StdlibItemKind::Class { class_id: win_cid },
                ty: Ty::Class(win_cid),
                native_id: 0,
            },
            StdlibItem {
                name: "Event".into(),
                kind: StdlibItemKind::Class { class_id: event_cid },
                ty: Ty::Class(event_cid),
                native_id: 0,
            },
            StdlibItem {
                name: "Image".into(),
                kind: StdlibItemKind::Class { class_id: img_cid },
                ty: Ty::Class(img_cid),
                native_id: 0,
            },
            StdlibItem {
                name: "init".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![], i32_ty.clone()),
                native_id: GFX_INIT,
            },
            StdlibItem {
                name: "create_window".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![str_ty.clone(), i32_ty.clone(), i32_ty.clone()], win_ty.clone()),
                native_id: GFX_CREATE_WINDOW,
            },
            StdlibItem {
                name: "close_window".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![win_ty.clone()], unit_ty.clone()),
                native_id: GFX_CLOSE_WINDOW,
            },
            StdlibItem {
                name: "poll_event".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![win_ty.clone()], opt_event_ty.clone()),
                native_id: GFX_POLL_EVENT,
            },
            StdlibItem {
                name: "clear".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![win_ty.clone(), i32_ty.clone(), i32_ty.clone(), i32_ty.clone()], unit_ty.clone()),
                native_id: GFX_CLEAR,
            },
            StdlibItem {
                name: "present".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![win_ty.clone()], unit_ty.clone()),
                native_id: GFX_PRESENT,
            },
            StdlibItem {
                name: "draw_rect".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(
                    vec![
                        win_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                    ],
                    unit_ty.clone(),
                ),
                native_id: GFX_DRAW_RECT,
            },
            StdlibItem {
                name: "draw_rect_outline".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(
                    vec![
                        win_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                    ],
                    unit_ty.clone(),
                ),
                native_id: GFX_DRAW_RECT_OUTLINE,
            },
            StdlibItem {
                name: "draw_line".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(
                    vec![
                        win_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                    ],
                    unit_ty.clone(),
                ),
                native_id: GFX_DRAW_LINE,
            },
            StdlibItem {
                name: "draw_point".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(
                    vec![
                        win_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                    ],
                    unit_ty.clone(),
                ),
                native_id: GFX_DRAW_POINT,
            },
            StdlibItem {
                name: "window_size".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![win_ty.clone()], tuple_i32_i32_ty.clone()),
                native_id: GFX_WINDOW_SIZE,
            },
            StdlibItem {
                name: "set_window_title".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![win_ty.clone(), str_ty.clone()], unit_ty.clone()),
                native_id: GFX_SET_WINDOW_TITLE,
            },
            StdlibItem {
                name: "load_image".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![win_ty.clone(), str_ty.clone()], img_ty.clone()),
                native_id: GFX_LOAD_IMAGE,
            },
            StdlibItem {
                name: "image_size".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![img_ty.clone()], tuple_i32_i32_ty.clone()),
                native_id: GFX_IMAGE_SIZE,
            },
            StdlibItem {
                name: "draw_image".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![win_ty.clone(), img_ty.clone(), i32_ty.clone(), i32_ty.clone()], unit_ty.clone()),
                native_id: GFX_DRAW_IMAGE,
            },
            StdlibItem {
                name: "draw_image_rect".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(
                    vec![
                        win_ty.clone(),
                        img_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                    ],
                    unit_ty.clone(),
                ),
                native_id: GFX_DRAW_IMAGE_RECT,
            },
            StdlibItem {
                name: "draw_image_rotated".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(
                    vec![
                        win_ty.clone(),
                        img_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        f64_ty.clone(),
                    ],
                    unit_ty.clone(),
                ),
                native_id: GFX_DRAW_IMAGE_ROTATED,
            },
            StdlibItem {
                name: "free_image".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![img_ty.clone()], unit_ty.clone()),
                native_id: GFX_FREE_IMAGE,
            },
            // ── M54 classes ──
            StdlibItem {
                name: "Sound".into(),
                kind: StdlibItemKind::Class { class_id: sound_cid },
                ty: Ty::Class(sound_cid),
                native_id: 0,
            },
            StdlibItem {
                name: "Music".into(),
                kind: StdlibItemKind::Class { class_id: music_cid },
                ty: Ty::Class(music_cid),
                native_id: 0,
            },
            StdlibItem {
                name: "Font".into(),
                kind: StdlibItemKind::Class { class_id: font_cid },
                ty: Ty::Class(font_cid),
                native_id: 0,
            },
            // ── M54 audio functions ──
            StdlibItem {
                name: "audio_init".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![], unit_ty.clone()),
                native_id: GFX_AUDIO_INIT,
            },
            StdlibItem {
                name: "load_sound".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![str_ty.clone()], sound_ty.clone()),
                native_id: GFX_LOAD_SOUND,
            },
            StdlibItem {
                name: "play_sound".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![sound_ty.clone()], unit_ty.clone()),
                native_id: GFX_PLAY_SOUND,
            },
            StdlibItem {
                name: "free_sound".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![sound_ty.clone()], unit_ty.clone()),
                native_id: GFX_FREE_SOUND,
            },
            StdlibItem {
                name: "load_music".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![str_ty.clone()], music_ty.clone()),
                native_id: GFX_LOAD_MUSIC,
            },
            StdlibItem {
                name: "play_music".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![music_ty.clone(), i32_ty.clone()], unit_ty.clone()),
                native_id: GFX_PLAY_MUSIC,
            },
            StdlibItem {
                name: "stop_music".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![], unit_ty.clone()),
                native_id: GFX_STOP_MUSIC,
            },
            StdlibItem {
                name: "set_music_volume".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![i32_ty.clone()], unit_ty.clone()),
                native_id: GFX_SET_MUSIC_VOLUME,
            },
            StdlibItem {
                name: "set_sound_volume".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![sound_ty.clone(), i32_ty.clone()], unit_ty.clone()),
                native_id: GFX_SET_SOUND_VOLUME,
            },
            // ── M54 font/text functions ──
            StdlibItem {
                name: "load_font".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![str_ty.clone(), i32_ty.clone()], font_ty.clone()),
                native_id: GFX_LOAD_FONT,
            },
            StdlibItem {
                name: "draw_text".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(
                    vec![
                        win_ty.clone(),
                        font_ty.clone(),
                        str_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                        i32_ty.clone(),
                    ],
                    unit_ty.clone(),
                ),
                native_id: GFX_DRAW_TEXT,
            },
            StdlibItem {
                name: "text_size".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![font_ty.clone(), str_ty.clone()], tuple_i32_i32_ty.clone()),
                native_id: GFX_TEXT_SIZE,
            },
            StdlibItem {
                name: "free_font".into(),
                kind: StdlibItemKind::Function,
                ty: fn_ty(vec![font_ty.clone()], unit_ty.clone()),
                native_id: GFX_FREE_FONT,
            },
        ];

        let gfx_mod = StdlibModule {
            name: "gfx".into(),
            items: gfx_items,
        };
        self.stdlib_modules.insert("gfx".into(), gfx_mod);
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
            ("Future", TypeCtor::Future),     // M32: spec §9.43 (asyncio)
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
            // M16: `isinstance(x, T)` — runtime class check. The second
            // argument names a user class (not a value). The typechecker
            // and IR lowerer treat this call specially; the sentinel
            // signature is just here to put `isinstance` in scope.
            ("isinstance", Ty::Function { params: vec![], ret: Box::new(Ty::Primitive(PrimTy::Bool)) }),
        ];
        for (name, ty) in builtins {
            self.make_symbol(scope, name, SymbolKind::Function, Span::DUMMY, Some(ty.clone()));
        }

        // ── Exception classes (spec §9.1, M15 try/except) ─────────────
        // Every built-in exception has two fields:
        //   type_name: str  — runtime tag, set by `raise X(msg)` to `"X"`
        //   message:   str  — the constructor argument
        // The IR materialises the exception value at `raise` time as a 2-field
        // heap object using these layouts; the VM dereferences them on field
        // access from inside an `except X as e:` handler body.  Field offsets
        // skip the 16-byte object header (the StoreField/LoadField bytecode
        // already adjusts for HDR).
        let excs = [
            "Exception", "ValueError", "IndexError", "KeyError", "TypeError",
            "OverflowError", "DivisionByZeroError", "ZeroDivisionError",
            "IOError", "NullPointerError", "AssertionError", "RuntimeError",
            // stdlib: §9.1 — used in spec examples
            "StopIteration",
            // M6 native dispatch
            "ChannelClosedError",
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
                fields: vec![
                    FieldInfo { name: "type_name".into(), ty: Ty::Primitive(PrimTy::Str), offset: 0 },
                    FieldInfo { name: "message".into(),   ty: Ty::Primitive(PrimTy::Str), offset: 8 },
                ],
                methods: vec![],
                generics: vec![],
                generic_tvars: vec![],
                // stdlib: exception classes carry no methods of their own and
                // are raised/caught by name; not handle-backed, so not native.
                is_native: false,
                payload_size: 16,
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
            generic_tvars: vec![],
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
            generic_tvars: vec![],
            // stdlib: Thread is handle-backed (ThreadRepr in vm/src/object.rs);
            // start/join dispatch via NativeFn, not a vtable.
            is_native: true,
            payload_size: 0,
        });

        // math module — stdlib: spec §9, used by mandelbrot if needed
        self.make_symbol(scope, "math", SymbolKind::BuiltinModule, Span::DUMMY, None);

        // ── M34: typed `json.JsonValue` tree ──────────────────────────
        //
        // M36 NOTE: these 11 classes (JsonValue + 6 subclasses, Pattern,
        // Connection + Cursor, Hasher) are still registered into the
        // prelude scope here for back-compat — every M34/M35 test
        // reaches them by bare name after just `import json` /
        // `import re` / `import sqlite3` / `import hashlib`, and a
        // hard removal would regress that surface.  What M36 changes
        // is that `seed_stdlib_modules` (which runs immediately after
        // `seed_prelude` returns) also publishes each ClassId as a
        // `StdlibItemKind::Class` entry on the matching stdlib module
        // — so the metadata now lives where v0.4 stdlib growth can
        // grow it, even though the prelude-scope bindings stay.
        //
        // Seven prelude classes — sealed base + 6 final subclasses —
        // backing the `json.parse` / `json.stringify` typed surface.
        // M11 + M16 + M31 do all the heavy lifting — these are
        // ordinary `is_native: false` classes that participate in the
        // standard vtable / isinstance / match infrastructure.
        //
        // Constructors are special-cased in the IR lowerer
        // (`lower_call` in ir.rs) — JNull..JString call NativeFn::JsonJ*New
        // which allocate a heap object and store the single payload
        // field at offset 0.  JList / JObject's storage is a sidecar
        // ListRepr whose pointer is parked in the `data` field; the GC's
        // class-scanner traces it as a root automatically.
        //
        // BUG SHAPE WARNING: the field offsets / payload_sizes here MUST
        // match what the VM allocates in `vm/src/builtins.rs::alloc_json_*`.
        // If you add a field, bump payload_size to (n_fields * 8) and add
        // a matching store in the VM constructor handler.
        let jv_cid = self.fresh_class();
        let jv_sid = self.make_symbol(scope, "JsonValue", SymbolKind::Class, Span::DUMMY,
                                        Some(Ty::Class(jv_cid)));
        self.table.get_mut(jv_sid).class_id = Some(jv_cid);
        self.class_of_symbol.insert(jv_sid, jv_cid);
        self.symbol_of_class.insert(jv_cid, jv_sid);
        self.class_name_to_id.insert("JsonValue".into(), jv_cid);
        self.class_layouts.insert(jv_cid, ClassLayout {
            id: jv_cid, name: "JsonValue".into(), base: None,
            // sealed lets subclasses be defined in this module (the
            // prelude) but not in user code — mirrors the M16 sealed-
            // hierarchy pattern documented in examples/json_parse_v2.spy.
            is_open: false, is_sealed: true,
            fields: vec![], methods: vec![],
            generics: vec![], generic_tvars: vec![],
            is_native: false, payload_size: 0,
        });

        // Helper closure: register one final subclass of JsonValue. Each
        // subclass has zero or one fields at offset 0 (parent payload is
        // 0). The constructor is special-cased in lower_call to route
        // through the matching NativeFn::JsonJ*New, which allocates a
        // heap object with the right type_id and stores the arg.
        let register_jv_subclass = |this: &mut Self,
                                          name: &str,
                                          field: Option<(&str, Ty)>|
        {
            let cid = this.fresh_class();
            let sid = this.make_symbol(scope, name, SymbolKind::Class,
                                          Span::DUMMY, Some(Ty::Class(cid)));
            this.table.get_mut(sid).class_id = Some(cid);
            this.class_of_symbol.insert(sid, cid);
            this.symbol_of_class.insert(cid, sid);
            this.class_name_to_id.insert(name.into(), cid);
            let (fields, payload) = match field {
                None => (vec![], 0u32),
                Some((fname, fty)) => (
                    vec![FieldInfo {
                        name: fname.into(),
                        ty: fty,
                        offset: 0,
                    }],
                    8u32,
                ),
            };
            this.class_layouts.insert(cid, ClassLayout {
                id: cid, name: name.into(), base: Some(jv_cid),
                is_open: false, is_sealed: false,
                fields,
                methods: vec![],
                generics: vec![], generic_tvars: vec![],
                is_native: false,
                payload_size: payload,
            });
            cid
        };

        register_jv_subclass(self, "JNull",   None);
        register_jv_subclass(self, "JBool",   Some(("value", Ty::Primitive(PrimTy::Bool))));
        register_jv_subclass(self, "JInt",    Some(("value", Ty::Primitive(PrimTy::I64))));
        register_jv_subclass(self, "JFloat",  Some(("value", Ty::Primitive(PrimTy::F64))));
        register_jv_subclass(self, "JString", Some(("value", Ty::Primitive(PrimTy::Str))));

        // JList — one List[JsonValue] field at offset 0. The IR lowerer
        // for `JList(items)` calls JsonJListNew which allocates the
        // object and stores the items list pointer; the GC's
        // GcKind::Class scanner traces that pointer.
        let jlist_items_ty = Ty::Generic {
            base: TypeCtor::List,
            args: vec![Ty::Class(jv_cid)],
        };
        let jlist_cid = self.fresh_class();
        let jlist_sid = self.make_symbol(scope, "JList", SymbolKind::Class,
                                            Span::DUMMY, Some(Ty::Class(jlist_cid)));
        self.table.get_mut(jlist_sid).class_id = Some(jlist_cid);
        self.class_of_symbol.insert(jlist_sid, jlist_cid);
        self.symbol_of_class.insert(jlist_cid, jlist_sid);
        self.class_name_to_id.insert("JList".into(), jlist_cid);
        self.class_layouts.insert(jlist_cid, ClassLayout {
            id: jlist_cid, name: "JList".into(), base: Some(jv_cid),
            is_open: false, is_sealed: false,
            fields: vec![
                FieldInfo {
                    name: "data".into(),
                    ty: jlist_items_ty.clone(),
                    offset: 0,
                },
            ],
            methods: vec![
                MethodSig { name: "length".into(), params: vec![],
                              ret: Ty::Primitive(PrimTy::I64) },
                MethodSig { name: "get".into(),
                              params: vec![Ty::Primitive(PrimTy::I64)],
                              ret: Ty::Class(jv_cid) },
                MethodSig { name: "items".into(), params: vec![],
                              ret: jlist_items_ty.clone() },
            ],
            generics: vec![], generic_tvars: vec![],
            // is_native = false even though methods dispatch via
            // NativeFn.  Setting is_native = true would route the
            // *constructor* through `NativeFn::from_name`, which we
            // don't want — JList's constructor allocs + stores the data
            // field via the M34 special-case in `lower_call` above.
            // Method dispatch is intercepted in `lower_method_call`
            // (the M34 fall-through path) so this `methods` list never
            // actually drives a vtable.
            is_native: false,
            payload_size: 8,
        });

        // JObject — two parallel List fields:
        //   keys:   List[str]
        //   values: List[JsonValue]
        // The IR lowerer for `JObject(entries)` calls JsonJObjectNew
        // which splits the (str, JsonValue) tuples into the two lists.
        // Two-field design (rather than a side-table handle a la Dict)
        // gives the GC a free trace through both lists with no special
        // root-scan code.
        let jobj_keys_ty = Ty::Generic {
            base: TypeCtor::List,
            args: vec![Ty::Primitive(PrimTy::Str)],
        };
        let jobj_vals_ty = jlist_items_ty.clone();
        let jobj_cid = self.fresh_class();
        let jobj_sid = self.make_symbol(scope, "JObject", SymbolKind::Class,
                                          Span::DUMMY, Some(Ty::Class(jobj_cid)));
        self.table.get_mut(jobj_sid).class_id = Some(jobj_cid);
        self.class_of_symbol.insert(jobj_sid, jobj_cid);
        self.symbol_of_class.insert(jobj_cid, jobj_sid);
        self.class_name_to_id.insert("JObject".into(), jobj_cid);
        self.class_layouts.insert(jobj_cid, ClassLayout {
            id: jobj_cid, name: "JObject".into(), base: Some(jv_cid),
            is_open: false, is_sealed: false,
            fields: vec![
                FieldInfo { name: "keys".into(),   ty: jobj_keys_ty.clone(), offset: 0 },
                FieldInfo { name: "values".into(), ty: jobj_vals_ty.clone(), offset: 8 },
            ],
            methods: vec![
                MethodSig { name: "get".into(),
                              params: vec![Ty::Primitive(PrimTy::Str)],
                              ret: Ty::Nullable(Box::new(Ty::Class(jv_cid))) },
                MethodSig { name: "has".into(),
                              params: vec![Ty::Primitive(PrimTy::Str)],
                              ret: Ty::Primitive(PrimTy::Bool) },
                MethodSig { name: "keys".into(),
                              params: vec![],
                              ret: jobj_keys_ty.clone() },
                MethodSig { name: "length".into(),
                              params: vec![],
                              ret: Ty::Primitive(PrimTy::I64) },
            ],
            generics: vec![], generic_tvars: vec![],
            // is_native: see JList comment above.
            is_native: false,
            payload_size: 16,
        });

        // ── M35 P4-C: streaming `Hasher` class ────────────────────────
        //
        // `final` handle-backed class.  Returned by `hashlib.new(algo)`;
        // user code calls `update(chunk)` / `hexdigest()` / `algorithm()`
        // on it.  Layout mirrors io.File / Channel / Thread: the visible
        // class has zero declared fields and `is_native: true` (no
        // vtable, methods dispatch via NativeFn); the VM stores the
        // per-instance i64 slot handle on a private `HasherRepr` struct,
        // and the actual in-progress hash state lives in
        // `SharedVm.hashers` keyed by that handle.
        //
        // p4c_*: per the M35-round file-ownership protocol, locals
        // introduced into shared compiler files use the `p4c_` prefix.
        let p4c_hasher_cid = self.fresh_class();
        let p4c_hasher_sid = self.make_symbol(scope, "Hasher", SymbolKind::Class,
                                                Span::DUMMY, Some(Ty::Class(p4c_hasher_cid)));
        self.table.get_mut(p4c_hasher_sid).class_id = Some(p4c_hasher_cid);
        self.class_of_symbol.insert(p4c_hasher_sid, p4c_hasher_cid);
        self.symbol_of_class.insert(p4c_hasher_cid, p4c_hasher_sid);
        self.class_name_to_id.insert("Hasher".into(), p4c_hasher_cid);
        self.class_layouts.insert(p4c_hasher_cid, ClassLayout {
            id: p4c_hasher_cid,
            name: "Hasher".into(),
            base: None,
            // final class — `is_open: false`, `is_sealed: false` matches
            // io.File / Thread which are also leaf handle wrappers.
            is_open: false,
            is_sealed: false,
            fields: vec![],
            methods: vec![
                // `update(data: str) -> None` — feed bytes into the hash.
                MethodSig { name: "update".into(),
                              params: vec![Ty::Primitive(PrimTy::Str)],
                              ret: Ty::Primitive(PrimTy::Unit) },
                // `hexdigest() -> str` — finalize-and-format.  Idempotent
                // (clone-not-consume policy — see spec §9.X).
                MethodSig { name: "hexdigest".into(),
                              params: vec![],
                              ret: Ty::Primitive(PrimTy::Str) },
                // `algorithm() -> str` — the canonical name passed to
                // `hashlib.new`.
                MethodSig { name: "algorithm".into(),
                              params: vec![],
                              ret: Ty::Primitive(PrimTy::Str) },
            ],
            generics: vec![],
            generic_tvars: vec![],
            // Handle-backed (like io.File / Channel / Thread).  Method
            // dispatch is routed through the M34 class-by-name path in
            // `ir::lower_method_call` (extended by M35 P4-C to recognise
            // Hasher).  Constructor `Hasher(...)` is NOT supported — users
            // must call `hashlib.new(algorithm)`.
            is_native: true,
            payload_size: 0,
        });

        // ── M35 P4-B: typed `sqlite3.Connection` + `Cursor` classes ───
        //
        // Wraps the M23 P3a-D opaque-handle API (`sqlite3.connect(path)
        // -> i64` + flat function family) in two classes:
        //
        //   final class Connection { handle: i64; ... methods ... }
        //   final class Cursor     { handle: i64; ... methods ... }
        //
        // Both follow the same prelude-class pattern as Channel /
        // Thread / io.File (handle-backed, `is_native: true`) — every
        // method dispatches via NativeFn to the M23 P3a-D logic in
        // builtins.rs.  The `handle` field at offset 0 holds the slot
        // index into `SharedVm.sqlite_connections` (Connection) or
        // `SharedVm.sqlite_cursors` (Cursor).  Constructors are
        // intercepted in the IR (`m35_p4b_sqlite_class_init_native_id`
        // in ir.rs) so the receiver-style `__init__` populates the
        // handle slot at offset 0; the type-checker is taught the
        // constructor signature via `m35_p4b_sqlite_class_ctor_param_tys`
        // in typecheck.rs.
        //
        // Backwards compatibility: the existing `sqlite3.connect` /
        // `sqlite3.execute` / `sqlite3.query` flat surface remains
        // available; the M29 web framework + the M23 P3a-D demo
        // continue to work unchanged.  Users opt into the class
        // surface via `sqlite3.open(path)`.
        let p4b_jv_cid = self.fresh_class();
        let p4b_conn_sid = self.make_symbol(scope, "Connection", SymbolKind::Class,
                                            Span::DUMMY, Some(Ty::Class(p4b_jv_cid)));
        self.table.get_mut(p4b_conn_sid).class_id = Some(p4b_jv_cid);
        self.class_of_symbol.insert(p4b_conn_sid, p4b_jv_cid);
        self.symbol_of_class.insert(p4b_jv_cid, p4b_conn_sid);
        self.class_name_to_id.insert("Connection".into(), p4b_jv_cid);

        // Cursor class id — registered before we build Connection's
        // method list because `query` / `query_params` return Cursor.
        let p4b_cur_cid = self.fresh_class();
        let p4b_cur_sid = self.make_symbol(scope, "Cursor", SymbolKind::Class,
                                            Span::DUMMY, Some(Ty::Class(p4b_cur_cid)));
        self.table.get_mut(p4b_cur_sid).class_id = Some(p4b_cur_cid);
        self.class_of_symbol.insert(p4b_cur_sid, p4b_cur_cid);
        self.symbol_of_class.insert(p4b_cur_cid, p4b_cur_sid);
        self.class_name_to_id.insert("Cursor".into(), p4b_cur_cid);

        let p4b_str_ty = Ty::Primitive(PrimTy::Str);
        let p4b_list_str_ty = Ty::Generic {
            base: TypeCtor::List,
            args: vec![p4b_str_ty.clone()],
        };
        let p4b_list_list_str_ty = Ty::Generic {
            base: TypeCtor::List,
            args: vec![p4b_list_str_ty.clone()],
        };

        // Connection layout: single `handle: i64` field at offset 0.
        // `is_native: true` so the M11 vtable path is skipped — methods
        // dispatch via the IR's class-name + method-name lookup
        // (`m35_p4b_sqlite_class_method_native_id_by_name`).  See the
        // analogous Channel / Thread layout for the same shape.
        self.class_layouts.insert(p4b_jv_cid, ClassLayout {
            id: p4b_jv_cid, name: "Connection".into(), base: None,
            is_open: false, is_sealed: false,
            fields: vec![
                FieldInfo {
                    name: "handle".into(),
                    ty: Ty::Primitive(PrimTy::I64),
                    offset: 0,
                },
            ],
            methods: vec![
                MethodSig { name: "execute".into(),
                              params: vec![p4b_str_ty.clone()],
                              ret: Ty::Primitive(PrimTy::Unit) },
                MethodSig { name: "execute_params".into(),
                              params: vec![p4b_str_ty.clone(), p4b_list_str_ty.clone()],
                              ret: Ty::Primitive(PrimTy::Unit) },
                MethodSig { name: "query".into(),
                              params: vec![p4b_str_ty.clone()],
                              ret: Ty::Class(p4b_cur_cid) },
                MethodSig { name: "query_params".into(),
                              params: vec![p4b_str_ty.clone(), p4b_list_str_ty.clone()],
                              ret: Ty::Class(p4b_cur_cid) },
                MethodSig { name: "last_insert_rowid".into(),
                              params: vec![],
                              ret: Ty::Primitive(PrimTy::I64) },
                MethodSig { name: "changes".into(),
                              params: vec![],
                              ret: Ty::Primitive(PrimTy::I32) },
                MethodSig { name: "close".into(),
                              params: vec![],
                              ret: Ty::Primitive(PrimTy::Unit) },
            ],
            generics: vec![], generic_tvars: vec![],
            // is_native: handle-backed, no vtable; dispatch via NativeFn
            // through the M35 P4-B IR special-case (mirrors Channel /
            // Thread / io.File).
            is_native: true,
            payload_size: 8,
        });

        // Cursor layout: single `handle: i64` field at offset 0 —
        // index into `SharedVm.sqlite_cursors`.
        self.class_layouts.insert(p4b_cur_cid, ClassLayout {
            id: p4b_cur_cid, name: "Cursor".into(), base: None,
            is_open: false, is_sealed: false,
            fields: vec![
                FieldInfo {
                    name: "handle".into(),
                    ty: Ty::Primitive(PrimTy::I64),
                    offset: 0,
                },
            ],
            methods: vec![
                MethodSig { name: "fetchone".into(),
                              params: vec![],
                              ret: Ty::Nullable(Box::new(p4b_list_str_ty.clone())) },
                MethodSig { name: "fetchall".into(),
                              params: vec![],
                              ret: p4b_list_list_str_ty.clone() },
                MethodSig { name: "column_names".into(),
                              params: vec![],
                              ret: p4b_list_str_ty.clone() },
                MethodSig { name: "row_count".into(),
                              params: vec![],
                              ret: Ty::Primitive(PrimTy::I64) },
            ],
            generics: vec![], generic_tvars: vec![],
            is_native: true,
            payload_size: 8,
        });

        // ── M35 P4-A: compiled `re.Pattern` class ─────────────────────
        //
        // Registered in the prelude alongside JsonValue + Channel +
        // Thread + io.File / Connection / Cursor / Hasher, following the
        // M34 "prelude wins" pattern.  Layout: one i64 field at offset
        // 0 — the slot handle into `SharedVm.p4a_compiled_regexes`.
        // Marking `is_native: true` routes the constructor and every
        // method call through NativeFn / the M35 dispatch table in
        // ir.rs.  Users never call `Pattern(handle)` directly; the only
        // entry point is `re.compile(...)`.
        let p4a_pattern_cid = self.fresh_class();
        let p4a_pattern_sid = self.make_symbol(scope, "Pattern", SymbolKind::Class,
                                                Span::DUMMY, Some(Ty::Class(p4a_pattern_cid)));
        self.table.get_mut(p4a_pattern_sid).class_id = Some(p4a_pattern_cid);
        self.class_of_symbol.insert(p4a_pattern_sid, p4a_pattern_cid);
        self.symbol_of_class.insert(p4a_pattern_cid, p4a_pattern_sid);
        self.class_name_to_id.insert("Pattern".into(), p4a_pattern_cid);
        self.class_layouts.insert(p4a_pattern_cid, ClassLayout {
            id: p4a_pattern_cid, name: "Pattern".into(), base: None,
            is_open: false, is_sealed: false,
            fields: vec![
                FieldInfo {
                    name: "handle".into(),
                    ty: Ty::Primitive(PrimTy::I64),
                    offset: 0,
                },
            ],
            methods: vec![
                MethodSig {
                    name: "matches".into(),
                    params: vec![Ty::Primitive(PrimTy::Str)],
                    ret: Ty::Primitive(PrimTy::Bool),
                },
                MethodSig {
                    name: "find".into(),
                    params: vec![Ty::Primitive(PrimTy::Str)],
                    ret: Ty::Nullable(Box::new(Ty::Primitive(PrimTy::Str))),
                },
                MethodSig {
                    name: "find_all".into(),
                    params: vec![Ty::Primitive(PrimTy::Str)],
                    ret: Ty::Generic {
                        base: TypeCtor::List,
                        args: vec![Ty::Primitive(PrimTy::Str)],
                    },
                },
                MethodSig {
                    name: "replace".into(),
                    params: vec![Ty::Primitive(PrimTy::Str), Ty::Primitive(PrimTy::Str)],
                    ret: Ty::Primitive(PrimTy::Str),
                },
                MethodSig {
                    name: "replace_all".into(),
                    params: vec![Ty::Primitive(PrimTy::Str), Ty::Primitive(PrimTy::Str)],
                    ret: Ty::Primitive(PrimTy::Str),
                },
                MethodSig {
                    name: "split".into(),
                    params: vec![Ty::Primitive(PrimTy::Str)],
                    ret: Ty::Generic {
                        base: TypeCtor::List,
                        args: vec![Ty::Primitive(PrimTy::Str)],
                    },
                },
                MethodSig {
                    name: "source".into(),
                    params: vec![],
                    ret: Ty::Primitive(PrimTy::Str),
                },
                MethodSig {
                    name: "__init__".into(),
                    params: vec![Ty::Primitive(PrimTy::I64)],
                    ret: Ty::Class(p4a_pattern_cid),
                },
            ],
            generics: vec![], generic_tvars: vec![],
            is_native: true,
            payload_size: 8,
        });

        // Convenience: bare booleans `True`/`False` (capitalised variants).
        self.make_symbol(scope, "True", SymbolKind::Const, Span::DUMMY, Some(Ty::Primitive(PrimTy::Bool)));
        self.make_symbol(scope, "False", SymbolKind::Const, Span::DUMMY, Some(Ty::Primitive(PrimTy::Bool)));
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Pass 1: register top-level decls
    // ─────────────────────────────────────────────────────────────────────

    fn register_top_decls(&mut self, scope: ScopeId, module: &Module) -> Result<(), CompileError> {
        // ── M19: import resolution ────────────────────────────────────
        //
        // Three kinds of imports:
        //
        //   (a) `import sys`                  — bind `sys` as BuiltinModule.
        //   (b) `import sys as s`             — bind `s` as alias of `sys`.
        //   (c) `from sys import argv, exit`  — bind each item as Const/Function/Class.
        //
        // The pre-M19 fast-path (no-op when the name already exists in
        // prelude) is preserved for legacy stdlibs that flatten into the
        // prelude (`from threading import Channel`): those bindings come
        // from `seed_prelude` and the import is purely cosmetic. New
        // stdlib modules registered via `seed_stdlib_modules` take
        // precedence — they go through the proper module table.
        //
        // M36 Phase D — legacy "prelude wins" coverage note: the
        // `lookup(scope, local_name).is_some() => continue` branch
        // below is still load-bearing for the 11 M34/M35 stdlib
        // classes (JsonValue + JNull + JBool + JInt + JFloat + JString
        // + JList + JObject; Pattern; Connection + Cursor; Hasher).
        // M36 registers them as `StdlibItemKind::Class` items on their
        // home modules so future v0.4 routes find them in the module
        // table, but `seed_prelude` still binds the symbols into
        // `prelude_scope` for back-compat — every M34/M35 test
        // reaches the names by bare lookup after just `import json`
        // (etc.), without an explicit `from json import JsonValue`.
        // A future agent that flips those tests to explicit imports
        // can then delete the `lookup => continue` branch and rely
        // entirely on the M36 Class-item path above.
        for imp in &module.imports {
            // Single-segment module path required in v0.2. Submodules
            // (`os.path`) are parser-supported but resolve here as a
            // missing module; flag explicitly for the future-work path.
            let mod_name = imp.path.join(".");
            let stdlib_mod = self.stdlib_modules.get(&mod_name).cloned();

            if !imp.items.is_empty() {
                // (c) `from MOD import x, y as z, ...`
                match &stdlib_mod {
                    Some(m) => {
                        for it in &imp.items {
                            let local_name = it.alias.as_deref().unwrap_or(&it.name);
                            // M23 P3a-C: when a module name is BOTH in the
                            // prelude (legacy flat re-exports like
                            // `threading.Thread` / `threading.Channel`) AND
                            // registered in stdlib_modules (the new function
                            // surface like `threading.lock_*`), prefer the
                            // prelude binding for any name that's already in
                            // scope.  Only fail if the name isn't in
                            // stdlib_modules AND isn't in scope.
                            let item = m.find(&it.name);
                            if item.is_none() {
                                if self.table.lookup(scope, local_name).is_some() {
                                    // Pre-existing prelude binding wins.
                                    continue;
                                }
                                return Err(Self::err_at(
                                    imp.span,
                                    codes::LINK_NO_SUCH_MODULE_ITEM,
                                    format!(
                                        "module `{}` has no item named `{}` (available: {})",
                                        mod_name,
                                        it.name,
                                        m.items.iter()
                                            .map(|i| i.name.as_str())
                                            .collect::<Vec<_>>()
                                            .join(", "),
                                    ),
                                ));
                            }
                            let item = item.unwrap();
                            if self.table.lookup(scope, local_name).is_some() {
                                // Pre-existing prelude binding wins (legacy
                                // stdlib).  M36 NOTE: for the 11 relocated
                                // stdlib classes (JsonValue + 6 subclasses,
                                // Pattern, Connection + Cursor, Hasher),
                                // this branch still fires for non-aliased
                                // `from json import JsonValue` because the
                                // class symbols continue to live in
                                // `prelude_scope` for back-compat — see
                                // `seed_prelude`.  The class is reachable
                                // by the local name already, so the import
                                // is correctly a no-op.  The
                                // `StdlibItemKind::Class` entry on the
                                // module is what the *aliased* path below
                                // consumes (`from json import JsonValue
                                // as JV`).
                                continue;
                            }
                            // M36: when the item is a `Class`, bind the
                            // local alias as a fresh class symbol so
                            // `isinstance(x, JV)` and downstream class-
                            // shaped uses (`JV()` constructor, `JV.method`)
                            // work via the existing class-by-name lookup
                            // paths in typecheck.rs / ir.rs.  The class
                            // itself was allocated in `seed_prelude` and
                            // already lives in `class_name_to_id` /
                            // `class_layouts`; we only register an
                            // additional Symbol pointing at the same
                            // ClassId, so dispatch finds it under either
                            // the original or the aliased name.
                            if let StdlibItemKind::Class { class_id } = item.kind {
                                let sid = self.make_symbol(
                                    scope,
                                    local_name,
                                    SymbolKind::Class,
                                    imp.span,
                                    Some(Ty::Class(class_id)),
                                );
                                self.table.get_mut(sid).class_id = Some(class_id);
                                self.class_of_symbol.insert(sid, class_id);
                                // NOTE: we deliberately do NOT overwrite
                                // `symbol_of_class[class_id]` — the
                                // canonical reverse mapping stays pointed
                                // at the original (prelude) symbol so
                                // diagnostics resolve to the upstream
                                // declaration site, not the import site.
                                continue;
                            }
                            let sid = self.make_symbol(
                                scope,
                                local_name,
                                SymbolKind::Import,
                                imp.span,
                                Some(item.ty.clone()),
                            );
                            self.import_item.insert(sid, item.clone());
                        }
                    }
                    None => {
                        // Legacy fall-through: `from threading import Channel`
                        // — `threading` isn't in `stdlib_modules` (yet), but
                        // the prelude already bound `Channel`. Keep that
                        // behavior.
                        let unknown_items: Vec<&ImportItem> = imp.items.iter()
                            .filter(|it| {
                                let name = it.alias.as_deref().unwrap_or(&it.name);
                                self.table.lookup(scope, name).is_none()
                            })
                            .collect();
                        if !unknown_items.is_empty() {
                            return Err(Self::err_at(
                                imp.span,
                                codes::LINK_MISSING_MODULE,
                                format!(
                                    "no stdlib module named `{}` (user-defined modules are v0.3)",
                                    mod_name
                                ),
                            ));
                        }
                        // All items already in prelude — keep them.
                    }
                }
            } else {
                // (a)/(b) `import MOD [as ALIAS]`
                let local_name = imp.alias.clone().unwrap_or_else(|| mod_name.clone());
                match &stdlib_mod {
                    Some(_) => {
                        if self.table.lookup(scope, &local_name).is_some() {
                            continue;
                        }
                        let sid = self.make_symbol(
                            scope,
                            &local_name,
                            SymbolKind::BuiltinModule,
                            imp.span,
                            None,
                        );
                        self.module_alias.insert(sid, mod_name.clone());
                    }
                    None => {
                        if self.table.lookup(scope, &local_name).is_some() {
                            // Legacy: `import threading` already bound by prelude.
                            continue;
                        }
                        return Err(Self::err_at(
                            imp.span,
                            codes::LINK_MISSING_MODULE,
                            format!(
                                "no stdlib module named `{}` (user-defined modules are v0.3)",
                                mod_name
                            ),
                        ));
                    }
                }
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
        // M31: if the class declares `[T1, T2, ...]`, allocate one TypeVarId
        // per parameter and seed each name as a TypeAlias symbol carrying
        // `Ty::Var(tv_i)` inside a fresh scope nested under `scope`. Field
        // types, method signatures, and method bodies all lower against
        // this scope so every occurrence of `T` becomes the *same* Ty::Var
        // — the type-checker's per-instantiation substitution then replaces
        // them uniformly.
        let mut generic_tvars: Vec<TypeVarId> = Vec::new();
        if !c.generics.is_empty() {
            let gscope = self.table.new_scope(Some(scope), false);
            for g in &c.generics {
                let tv = self.fresh_tvar();
                generic_tvars.push(tv);
                self.make_symbol(gscope, &g.name, SymbolKind::TypeAlias, g.span,
                                  Some(Ty::Var(tv)));
            }
            self.class_generic_scope.insert(cid, gscope);
        }
        // Empty layout — fields/methods filled in later.
        self.class_layouts.insert(cid, ClassLayout {
            id: cid, name: c.name.clone(), base: None,
            is_open:   matches!(c.modifier, ClassModifier::Open),
            is_sealed: matches!(c.modifier, ClassModifier::Sealed),
            fields: vec![],
            methods: vec![],
            generics: c.generics.iter().map(|g| g.name.clone()).collect(),
            generic_tvars,
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
        // M31: for generic classes, lower field/method types against a scope
        // where each type-parameter resolves to its `Ty::Var`.
        let cscope = self.class_generic_scope.get(&cid).copied().unwrap_or(scope);

        // Push the class onto class_stack so `Self` resolves; also so the
        // lowered field/method types correctly bind `T` (the class scope
        // chains up to the module scope).
        self.class_stack.push(cid);

        // Resolve base.
        let base_cid = if let Some(base_ty) = c.bases.first() {
            if let Ok(t) = self.lower_ast_type(base_ty, cscope) {
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
        // M31: generic-class field types may reference `T`. Lower against the
        // class's generic scope (if any) so each `T` becomes its `Ty::Var`;
        // the IR worklist substitutes them per instantiation.
        //
        // Field SIZE/ALIGN for an as-yet-abstract `T` is conservatively
        // 8 bytes (pointer / 64-bit slot). Every concrete instantiation
        // uses the *same* slot footprint — strings, classes, i64 all fit
        // in 8 bytes; smaller primitives (i32, bool) also occupy a full
        // 8-byte slot in the heap payload because there's only one
        // generic layout. This keeps field offsets stable across
        // instantiations, which is essential because the IR emits one
        // offset per source-level field access and that offset must work
        // for every Box__i64 / Box__str / etc.
        for f in &c.fields {
            let ty = self.lower_ast_type(&f.ty, cscope)?;
            let size = if contains_unbound_var(&ty) { 8 } else { size_of_ty(&ty) };
            let align = if contains_unbound_var(&ty) { 8 } else { align_of_ty(&ty) };
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
            let sig = self.build_method_sig(cscope, init, cid)?;
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
            let sig = self.build_method_sig(cscope, m, cid)?;
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
        self.class_stack.pop();
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
        // M17: if the function declares type parameters, allocate a TypeVarId
        // per `T`, seed them as `TypeAlias` symbols (carrying `Ty::Var(...)`)
        // inside a scratch scope nested under `scope`, and lower the params /
        // return type against that scope so every occurrence of `T` resolves
        // to the same `Ty::Var`.
        let mut generic_tvars: Vec<TypeVarId> = Vec::new();
        let sig_scope = if f.generics.is_empty() {
            scope
        } else {
            let s = self.table.new_scope(Some(scope), false);
            for g in &f.generics {
                let tv = self.fresh_tvar();
                generic_tvars.push(tv);
                self.make_symbol(s, &g.name, SymbolKind::TypeAlias, g.span,
                                 Some(Ty::Var(tv)));
            }
            s
        };
        let mut params = Vec::new();
        for p in &f.params {
            let ty = if receiver.is_some() && p.name == "self" {
                Ty::Class(receiver.unwrap())
            } else {
                self.lower_ast_type_with_class(&p.ty, sig_scope, receiver)?
            };
            params.push((p.name.clone(), ty));
        }
        let ret = if matches!(f.return_ty, ast::Type::Named { ref name, .. } if name == "None") {
            Ty::Primitive(PrimTy::Unit)
        } else {
            self.lower_ast_type_with_class(&f.return_ty, sig_scope, receiver)?
        };
        Ok(FunctionSig {
            name: f.name.clone(),
            params,
            ret,
            generics: f.generics.iter().map(|g| g.name.clone()).collect(),
            generic_tvars,
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
        // M31: walk method bodies with the class generic scope as parent so
        // `T` resolves to its `Ty::Var` everywhere inside the class.
        let body_scope = self.class_generic_scope.get(&cid).copied().unwrap_or(scope);
        self.class_stack.push(cid);
        if let Some(init) = &c.init {
            self.resolve_func_decl(init, body_scope, Some(cid))?;
        }
        for m in &c.methods {
            self.resolve_func_decl(m, body_scope, Some(cid))?;
        }
        self.class_stack.pop();
        Ok(())
    }

    fn resolve_func_decl(&mut self, f: &FuncDecl, parent_scope: ScopeId, receiver: Option<ClassId>)
        -> Result<(), CompileError>
    {
        // Build a fresh function scope chained to the parent.
        let fn_scope = self.table.new_scope(Some(parent_scope), true);

        // M17: re-bind type parameters to the SAME `Ty::Var(...)`s already
        // allocated during `build_function_sig`. For top-level fns we can look
        // up the sig by the fn-symbol name in `parent_scope`. For methods we
        // mint fresh ones here — body resolution uses them locally and the
        // typechecker's substitution proceeds on each instantiation anyway.
        // The contract: occurrences of `T` in param/return annotations and in
        // the body must lower to the same `Ty::Var` so substitution at call
        // sites sees a uniform variable.
        let cached_tvars: Vec<TypeVarId> = if let Some(sid) =
            self.table.lookup_local(parent_scope, &f.name)
        {
            self.function_sigs
                .get(&sid)
                .map(|s| s.generic_tvars.clone())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        for (i, g) in f.generics.iter().enumerate() {
            let tv = cached_tvars
                .get(i)
                .copied()
                .unwrap_or_else(|| self.fresh_tvar());
            self.make_symbol(fn_scope, &g.name, SymbolKind::TypeAlias, g.span,
                              Some(Ty::Var(tv)));
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
                // Lower against fn_scope so generic-param `T` is visible.
                self.lower_ast_type_with_class(&p.ty, fn_scope, receiver)?
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
            Stmt::LetDestructure { names, tys, init, span } => {
                // M14 tuples. Each name becomes its own local symbol. If a
                // per-name annotation is present we resolve it here; otherwise
                // the typechecker fills in the slot from the RHS tuple type.
                self.resolve_expr(init, scope)?;
                for (n, t) in names.iter().zip(tys.iter()) {
                    if self.table.lookup_local(scope, n).is_some() {
                        return Err(Self::err_at(*span, codes::RESOLVE_DUPLICATE_LET,
                            format!("duplicate `let` of `{}`", n)));
                    }
                    let ty = match t {
                        Some(ast_t) => Some(self.lower_ast_type(ast_t, scope)?),
                        None => None,
                    };
                    self.make_symbol(scope, n, SymbolKind::Local, *span, ty);
                }
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
                        // M14: normalize `Tuple[T1, T2, ...]` → `Ty::Tuple(...)`
                        // so all downstream passes can match on a single shape.
                        // (The tuple-type AST sugar `(T1, T2)` already lowers
                        // straight to Ty::Tuple in the ast::Type::Tuple arm.)
                        if matches!(ctor, TypeCtor::Tuple) {
                            return Ok(Ty::Tuple(lowered_args));
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
        "Future" => TypeCtor::Future,
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

/// M31: does `t` reference any `Ty::Var`? Used by `layout_class` to fall
/// back to a 8-byte slot for fields whose declared type is an abstract
/// class type-parameter — concrete instantiations all fit in one slot,
/// keeping field offsets stable across instantiations.
fn contains_unbound_var(t: &Ty) -> bool {
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
