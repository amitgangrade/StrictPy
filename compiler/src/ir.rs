//! Typed SSA intermediate representation. See spec §11.
//!
//! Three-address SSA over basic blocks. Each function is a control-flow
//! graph of [`BasicBlock`]s, each block is a list of [`Value`]s, and every
//! [`Value`] carries its [`Ty`]. Phi nodes appear at block heads.
//!
//! The M3 lowerer (`lower`) is intentionally **best-effort**: it produces a
//! structurally well-formed IR that the codegen layer can emit into a valid
//! `.spyc`, but it does NOT yet implement every desugaring rule from spec
//! §10.5 with full semantic fidelity. Unrecognised constructs lower to a
//! `ConstNone` placeholder of the right static type so SSA invariants hold.
//! M4/M6 will tighten this up.

use std::collections::HashMap;

use strictpy_shared::NativeFn;

use crate::ast::{
    self, BinOp as AstBinOp, Block, ExceptHandler, Expr, FuncDecl, Literal, Lvalue,
    Span, Stmt, TopDecl, UnaryOp,
};
use crate::resolver::{SymbolId, SymbolKind};
use crate::typecheck::{mangle_args_key, subst_ty, unify_one as unify_lower, TypedModule};
use crate::types::{display_ty, ClassId, ClassLayout, PrimTy, Ty, TypeCtor, TypeVarId};

// ─────────────────────────────────────────────────────────────────────────
//  IR node types
// ─────────────────────────────────────────────────────────────────────────

/// SSA value id, unique within a function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValueId(pub u32);

/// Basic-block id, unique within a function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

/// Function id, unique within a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FuncId(pub u32);

/// A single SSA value.
#[derive(Debug, Clone)]
pub struct Value {
    pub id: u32,
    pub ty: Ty,
    pub kind: ValueKind,
}

#[derive(Debug, Clone)]
pub enum ValueKind {
    /// A literal constant.
    Const(IRConst),
    /// The `idx`-th parameter of the enclosing function.
    Param { idx: u32 },
    /// An IR-level operation with its operand values.
    Op { op: IROp, args: Vec<ValueId> },
    /// SSA phi node at a block head.
    Phi { incoming: Vec<(BlockId, ValueId)> },
}

/// Literal constants embedded directly in the IR.
#[derive(Debug, Clone)]
pub enum IRConst {
    I32(i32),
    I64(i64),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Bool(bool),
    Str(String),
    Char(char),
    None,
}

/// IR operations. Mirrors spec §11.3 with M3 extensions for native calls
/// and closure construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IROp {
    // ── Arithmetic (spec §11.3) ──────────────────────────────────────────
    IAdd, ISub, IMul, IDiv, IRem, INeg,
    IShl, IShr, IAnd, IOr, IXor, INot,
    FAdd, FSub, FMul, FDiv, FNeg,

    // ── Conversions ──────────────────────────────────────────────────────
    IExt, ITrunc, FtoI, ItoF, FExt, FTrunc, Bitcast,

    // ── Comparisons ──────────────────────────────────────────────────────
    IEq, INe, ILt, ILe, IGt, IGe,
    FEq, FNe, FLt, FLe, FGt, FGe,
    StrEq,
    RefEq,

    // ── Boolean (`not`) ──────────────────────────────────────────────────
    BoolNot,

    // ── Memory / object ──────────────────────────────────────────────────
    /// Allocate a fresh instance of `class_id` (uninitialised — caller must
    /// follow up with field stores or an `__init__` call).
    Alloc { class_id: u32 },
    Load { offset: u32 },
    Store { offset: u32 },
    ArrayNew,
    ArrayGet,
    ArraySet,
    ArrayLen,
    /// Push an element onto a list. Args: `[list, value]`. Used to populate
    /// list literals after `ArrayNew`.
    ListPush,

    // ── Locals (slot-based) ─────────────────────────────────────────────
    /// Read the current value of local slot `slot` into the destination
    /// SSA value. Lets reads inside a loop body observe writes from the
    /// previous iteration without needing real phi insertion.
    ReadLocal { slot: u16 },
    /// Write the operand (args[0]) into local slot `slot`.
    WriteLocal { slot: u16 },

    // ── Calls ────────────────────────────────────────────────────────────
    DirectCall { fn_id: FuncId },
    VirtualCall { vtable_slot: u32 },
    IfaceCall { itable_id: u32, slot: u32 },
    IndirectCall,
    /// Native / FFI / prelude builtin. `native_id` is a [`NativeFn`] discriminant.
    NativeCall { native_id: u32 },

    // ── Generic SSA helpers ──────────────────────────────────────────────
    Copy,
    Select,

    // ── Closures ─────────────────────────────────────────────────────────
    ClosureNew { fn_id: FuncId, n_captures: u32 },
    ClosureCall,

    // ── Safety / checks ──────────────────────────────────────────────────
    BoundsCheck,
    NullCheck,
    TypeCheck,
    /// M16: `isinstance(x, T)` — read the object's runtime class id and walk
    /// the parent chain. Args: `[obj]`. Returns `bool`. `class_id` is the
    /// runtime *type-table* id (matching the operand of `IROp::Alloc`).
    IsInstance { class_id: u32 },

    // ── Exceptions (M15 try/except) ──────────────────────────────────────
    /// Push a handler frame onto the interpreter's handler stack at the
    /// start of a `try` body. The frame describes which exception type
    /// names to catch and where to dispatch each one, the optional finally
    /// block, and (per-arm) which local slot receives the bound exception
    /// value at handler entry. Lowered to `Opcode::EnterTry` in codegen
    /// (block ids in `arms`/`finally_block` are patched to byte offsets at
    /// emit time, same machinery as the `Branch` terminator).
    TryEnter {
        /// One arm per `except`. Each filter is a constant-pool string
        /// index whose value is the exception type-name; "Exception"
        /// matches anything.
        arms: Vec<TryHandlerArm>,
        /// Block to run after a normal body completion / handler completion,
        /// before the join block. `None` if the `try` had no `finally`.
        finally_block: Option<BlockId>,
    },
    /// Pop the topmost handler frame. Emitted at the bottom of the `try`
    /// body and (when a `finally` exists) at the end of the handler arm
    /// bodies just before branching into the finally block.
    TryLeave,
    /// Marker at the end of a finally block. If the interpreter has a
    /// pending exception stashed in `pending_exception`, re-raise it;
    /// otherwise no-op. Lowered to `Opcode::Rethrow`.
    EndFinally,
}

/// One arm of a `try ... except ... [except ...]` IR statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TryHandlerArm {
    /// Constant-pool index of the type-name string this arm catches.
    /// "Exception" matches any thrown type.
    pub filter_str_idx: u32,
    /// Block where this arm's handler body begins.
    pub handler_block: BlockId,
    /// Local slot the handler binds the exception value into.
    /// `u32::MAX` means "no `as e:` binding".
    pub bind_slot: u32,
}

/// Terminator at the end of every basic block.
#[derive(Debug, Clone)]
pub enum Terminator {
    Branch { target: BlockId },
    CondBranch { cond: ValueId, t: BlockId, f: BlockId },
    Ret { value: Option<ValueId> },
    Throw { exc: ValueId },
    /// Catch handler entry. See spec §11.3.
    Catch,
    Unreachable,
}

#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub id: BlockId,
    pub values: Vec<Value>,
    pub terminator: Terminator,
}

#[derive(Debug, Clone)]
pub struct IRFunction {
    pub id: FuncId,
    pub name: String,
    pub params: Vec<Ty>,
    pub ret: Ty,
    pub blocks: Vec<BasicBlock>,
}

/// One entry in the module's type table (spec §12.6).
#[derive(Debug, Clone)]
pub struct TypeTableEntry {
    pub type_id: u32,
    pub kind: u8, // 0=primitive, 1=class, 2=protocol, 3=tuple, etc.
    pub name_idx: u32,
    pub size: u32,
    pub base_type: u32,
    pub fields: Vec<TypeFieldEntry>,
    pub vtable: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct TypeFieldEntry {
    pub name_idx: u32,
    pub type_id: u32,
    pub offset: u32,
}

/// One entry in the module's function table (spec §12.7).
#[derive(Debug, Clone)]
pub struct FunctionTableEntry {
    pub fn_id: u32,
    pub name_idx: u32,
    pub type_id: u32,
    pub num_params: u16,
    pub num_locals: u16,
    pub flags: u16,
}

#[derive(Debug, Default)]
pub struct IRModule {
    pub functions: Vec<IRFunction>,
    pub const_pool: Vec<IRConst>,
    pub string_table: Vec<String>,
    pub type_table: Vec<TypeTableEntry>,
    pub function_table: Vec<FunctionTableEntry>,
}

// ─────────────────────────────────────────────────────────────────────────
//  Lowering entry point
// ─────────────────────────────────────────────────────────────────────────

/// Lower a type-checked module to typed SSA IR.
pub fn lower(module: TypedModule) -> IRModule {
    let mut lw = Lowerer::new(module);
    lw.run();
    lw.finish()
}

// ─────────────────────────────────────────────────────────────────────────
//  Lowerer state
// ─────────────────────────────────────────────────────────────────────────

struct Lowerer {
    typed: TypedModule,
    out: IRModule,
    /// Intern table: string → string-table index.
    str_intern: HashMap<String, u32>,
    /// Top-level function name → assigned fn id. For generic functions this
    /// maps to the **first instantiation**'s FuncId so legacy callers (no
    /// substitution) get a sensible default; M17 generic-aware call sites
    /// instead consult `fn_id_for_inst`.
    fn_id_by_name: HashMap<String, FuncId>,
    /// Class id → assigned type-table type_id.
    class_type_id: HashMap<u32, u32>,
    /// M14: synthetic type-table id per tuple shape, keyed by
    /// `display_ty(Ty::Tuple(elems))` (a stable, structural string).
    /// The shape's field offsets are always `8 * i` (one 8-byte slot per
    /// element) so the same tid can serve every element-type permutation
    /// that displays identically. See `register_tuple_type`.
    tuple_type_id: HashMap<String, u32>,
    /// Top-level `final` const declarations folded to their literal value.
    /// Used at every reference site (see `Expr::Ident` lowering).
    module_consts: HashMap<String, (IRConst, Ty)>,
    /// Next free fn id.
    next_fn_id: u32,
    /// Next free type id (after primitives 0..15).
    next_type_id: u32,
    /// M17: every generic top-level fn whose declaration carries `[T, ...]`.
    /// Indexed by source name. Used to look up the FuncDecl when lowering
    /// per-instantiation copies.
    generic_fn_decls: HashMap<String, FuncDecl>,
    /// M31: every generic class declaration (`class Box[T]:`). Indexed by
    /// `ClassId` so per-instantiation lowering can recover the source
    /// FuncDecls for `__init__` + methods.
    generic_class_decls: HashMap<ClassId, ast::ClassDecl>,
    /// M17: source name → SymbolId, so the typechecker-recorded instantiation
    /// list can be located.
    generic_fn_sid: HashMap<String, SymbolId>,
    /// M17: per-instantiation FuncId. Key: `(sid, mangle_args_key)`.
    fn_id_for_inst: HashMap<(SymbolId, String), FuncId>,
    /// M17: per-instantiation generic-arg vector. Same key; lets the lowerer
    /// reconstruct the substitution map when emitting the body.
    type_args_for_inst: HashMap<(SymbolId, String), Vec<Ty>>,
    /// M17: name of the mangled IRFunction for `(sid, key)` — used to set the
    /// `irfn.name` so codegen / debugging show the substituted signature.
    mangled_name_for_inst: HashMap<(SymbolId, String), String>,
    /// M17: TypeVarIds declared by each generic fn, in declaration order, so
    /// `lower_call` (which only has access to `Lowerer` state via `LowerCtx`)
    /// can rebuild the substitution from arg types.
    tvars_for_sid: HashMap<SymbolId, Vec<TypeVarId>>,
    /// M17 worklist: instantiations to lower. Populated from
    /// `typed.instantiations` at startup, extended on-the-fly when a
    /// generic body calls another generic (transitive monomorphisation).
    inst_worklist: Vec<(SymbolId, Vec<Ty>)>,
    /// M31: per-instantiation type_id for generic classes. Key is
    /// `(class_id, mangle_args_key(type_args))`.
    class_inst_type_id: HashMap<(ClassId, String), u32>,
    /// M31: per-instantiation `__init__` FuncId. Same key.
    class_inst_init_fn: HashMap<(ClassId, String), FuncId>,
    /// M31: per-instantiation method FuncId, keyed by
    /// `(class_id, mangle_args_key, method_name)`.
    class_inst_method_fn: HashMap<(ClassId, String, String), FuncId>,
    /// M31: mangled class name per instantiation (e.g. `Box__i64`) — used
    /// when emitting TypeTableEntry name index and IRFunction names.
    class_inst_name: HashMap<(ClassId, String), String>,
    /// M31: class-instantiation worklist. Each entry is a
    /// `(class_id, type_args)` pair whose `__init__` + methods still need
    /// lowering. Populated from `typed.class_instantiations` at startup,
    /// extended on-the-fly when a generic body constructs another generic
    /// class (transitive monomorphisation).
    class_inst_worklist: Vec<(ClassId, Vec<Ty>)>,
}

impl Lowerer {
    fn new(typed: TypedModule) -> Self {
        Self {
            typed,
            out: IRModule::default(),
            str_intern: HashMap::new(),
            fn_id_by_name: HashMap::new(),
            class_type_id: HashMap::new(),
            tuple_type_id: HashMap::new(),
            module_consts: HashMap::new(),
            next_fn_id: 0,
            next_type_id: 16,
            generic_fn_decls: HashMap::new(),
            generic_class_decls: HashMap::new(),
            generic_fn_sid: HashMap::new(),
            fn_id_for_inst: HashMap::new(),
            type_args_for_inst: HashMap::new(),
            mangled_name_for_inst: HashMap::new(),
            tvars_for_sid: HashMap::new(),
            inst_worklist: Vec::new(),
            class_inst_type_id: HashMap::new(),
            class_inst_init_fn: HashMap::new(),
            class_inst_method_fn: HashMap::new(),
            class_inst_name: HashMap::new(),
            class_inst_worklist: Vec::new(),
        }
    }

    fn intern_str(&mut self, s: &str) -> u32 {
        if let Some(idx) = self.str_intern.get(s) {
            return *idx;
        }
        let idx = self.out.string_table.len() as u32;
        self.out.string_table.push(s.to_string());
        self.str_intern.insert(s.to_string(), idx);
        idx
    }

    fn fresh_fn_id(&mut self) -> FuncId {
        let id = FuncId(self.next_fn_id);
        self.next_fn_id += 1;
        id
    }

    fn fresh_type_id(&mut self) -> u32 {
        let id = self.next_type_id;
        self.next_type_id += 1;
        id
    }

    fn run(&mut self) {
        // Pass 1: pre-register every top-level function (and methods) so calls
        // can resolve to a FuncId during body lowering.
        // Reborrow trick: clone the decl list (cheap; AST is `Clone`).
        let decls = self.typed.resolved.module.decls.clone();
        for d in &decls {
            match d {
                TopDecl::Func(f) => {
                    if !f.generics.is_empty() {
                        // M17: generic free fns get one FuncId per instantiation,
                        // not one shared id. Stash the AST so the worklist can
                        // re-walk the body once per (T1, T2, ...). The unmangled
                        // name is **not** registered in `fn_id_by_name` because
                        // there is no IRFunction to dispatch to — every call site
                        // must rewrite to a mangled per-instantiation id.
                        self.generic_fn_decls.insert(f.name.clone(), f.clone());
                        if let Some(sid) = self
                            .typed
                            .resolved
                            .symbols
                            .lookup(self.typed.resolved.module_scope, &f.name)
                        {
                            self.generic_fn_sid.insert(f.name.clone(), sid);
                            if let Some(sig) =
                                self.typed.resolved.function_sigs.get(&sid)
                            {
                                self.tvars_for_sid
                                    .insert(sid, sig.generic_tvars.clone());
                            }
                        }
                        continue;
                    }
                    let fid = self.fresh_fn_id();
                    self.fn_id_by_name.insert(f.name.clone(), fid);
                }
                TopDecl::Class(c) => {
                    // Pre-assign type id for the class so cross-references work.
                    // M31: generic classes get one type_id *per instantiation*
                    // rather than a single shared id — registered later in
                    // Pass 2.7. The class_type_id slot here is unused for
                    // generic classes (lookups go through class_inst_type_id).
                    if !c.generics.is_empty() {
                        if let Some(cid_sid) = self.lookup_module_symbol(&c.name) {
                            if let Some(cid) = self.typed.resolved.symbols.get(cid_sid).class_id {
                                self.generic_class_decls.insert(cid, c.clone());
                            }
                        }
                        // Don't pre-allocate any FuncIds — they're minted
                        // per-instantiation as the class_inst worklist drains.
                        continue;
                    }
                    if let Some(cid_sid) = self.lookup_module_symbol(&c.name) {
                        if let Some(cid) = self.typed.resolved.symbols.get(cid_sid).class_id {
                            let tid = self.fresh_type_id();
                            self.class_type_id.insert(cid.0, tid);
                        }
                    }
                    // Pre-register __init__ + methods with unique synthesized names.
                    if c.init.is_some() {
                        let fid = self.fresh_fn_id();
                        let name = format!("{}.__init__", c.name);
                        self.fn_id_by_name.insert(name, fid);
                    }
                    for m in &c.methods {
                        let fid = self.fresh_fn_id();
                        let name = format!("{}.{}", c.name, m.name);
                        self.fn_id_by_name.insert(name, fid);
                    }
                }
                TopDecl::Const(c) => {
                    // Fold each `final` const to its literal value so all
                    // references substitute the constant directly. v0.1
                    // requires const initialisers to be literal-typed
                    // (per the grammar) so this is sound for non-pathological
                    // inputs; non-literal initialisers fall back to None.
                    let ty = self
                        .lookup_ast_ty(&c.ty)
                        .unwrap_or(Ty::Primitive(PrimTy::Unit));
                    if let Some(irc) = literal_to_irconst(&c.value, &ty) {
                        self.module_consts.insert(c.name.clone(), (irc, ty));
                    }
                }
                _ => {}
            }
        }

        // Pass 2: emit class type-table entries from class_layouts.
        let class_layouts = self.typed.resolved.class_layouts.clone();
        // Stable order: by class id (deterministic across runs).
        let mut class_ids: Vec<u32> = class_layouts.keys().map(|c| c.0).collect();
        class_ids.sort();
        for cid_raw in class_ids {
            let cid = ClassId(cid_raw);
            let layout = match class_layouts.get(&cid) {
                Some(l) => l,
                None => continue,
            };
            // M31: skip generic classes. Their type-table entries are
            // emitted per-instantiation in Pass 2.7 below — the abstract
            // layout (with `Ty::Var` field types) has no runtime form.
            if !layout.generic_tvars.is_empty() { continue; }
            let tid = *self.class_type_id.entry(cid_raw).or_insert_with(|| {
                let id = self.next_type_id;
                self.next_type_id += 1;
                id
            });
            let name_idx = self.intern_str(&layout.name);
            let mut fields = Vec::new();
            for f in &layout.fields {
                let fname = self.intern_str(&f.name);
                fields.push(TypeFieldEntry {
                    name_idx: fname,
                    type_id: self.type_id_of_ty(&f.ty),
                    offset: f.offset,
                });
            }
            // vtable: one fn id per virtual method, looked up by
            // `Class.method` name. `__init__` is intentionally excluded —
            // constructors are direct-called by name and shouldn't take
            // a vtable slot (see resolver.rs comment).
            //
            // M11 BUG-017+N1 fix: walk up the inheritance chain when a
            // subclass doesn't override an inherited method. Without this
            // step, an inherited slot would be filled with u32::MAX and the
            // first call through a base-typed receiver would trap with
            // "unresolved vtable slot N".
            let mut vtable = Vec::new();
            for m in &layout.methods {
                if m.name == "__init__" {
                    continue;
                }
                // Walk this class and its ancestors looking for the most
                // derived definition of `m.name` that has an emitted fn id.
                let mut fid = u32::MAX;
                let mut cur = Some(cid);
                while let Some(c) = cur {
                    let cur_layout = match class_layouts.get(&c) {
                        Some(l) => l,
                        None => break,
                    };
                    let key = format!("{}.{}", cur_layout.name, m.name);
                    if let Some(FuncId(found)) = self.fn_id_by_name.get(&key) {
                        fid = *found;
                        break;
                    }
                    cur = cur_layout.base;
                }
                vtable.push(fid);
            }
            let base_type = layout
                .base
                .and_then(|b| self.class_type_id.get(&b.0).copied())
                .unwrap_or(strictpy_shared::file_format::NO_BASE_TYPE);
            // M11 BUG-016 fix: subclass allocations must include parent
            // fields too. With `layout.fields` now containing the parent's
            // fields verbatim plus the subclass's own, the conservative
            // `8 * fields.len()` formula naturally covers both — and we
            // additionally take the max with `payload_size` (which respects
            // natural alignment of mixed-width primitives) as a belt-and-
            // braces guard. The GC scans 8 bytes at a time so always pad up.
            let words = (layout.fields.len() as u32).max(
                (layout.payload_size + 7) / 8
            );
            let size = 16 + words * 8;
            self.out.type_table.push(TypeTableEntry {
                type_id: tid,
                kind: 1, // class
                name_idx,
                size,
                base_type,
                fields,
                vtable,
            });
        }

        // Pass 2.5 (M14): scan the typed module for every distinct tuple
        // shape and emit a synthetic type-table entry per shape.  The
        // resulting type ids are stashed in `tuple_type_id` and used by
        // `Expr::Tuple` lowering to pick the right Alloc operand.  Layout
        // is uniform 8-byte-per-element so equality / field access can be
        // computed purely from arity + index.
        self.register_tuple_types();

        // Pass 2.6 (M17): pre-register a FuncId for every *fully concrete*
        // generic-fn instantiation the typechecker discovered (i.e. those
        // whose type args don't contain any unbound `Ty::Var`). Calls that
        // appear inside another generic body get rediscovered with concrete
        // arguments during Pass 3.5 lowering.
        let initial: Vec<(SymbolId, Vec<Ty>)> = self.typed.instantiations.clone();
        for (sid, type_args) in initial {
            if type_args.iter().any(has_unbound_var) { continue; }
            self.register_instantiation(sid, type_args);
        }

        // Pass 2.7 (M31): same dance for generic *classes*. For each
        // typechecker-recorded `(class_id, type_args)`, pre-allocate a
        // per-instantiation type_id, a per-instantiation `__init__` FuncId
        // (if the source declares one), and a FuncId per declared method.
        // The corresponding TypeTableEntry is emitted immediately so
        // Alloc operands can reference the concrete tid. Method bodies are
        // lowered later in Pass 3.6 by draining `class_inst_worklist`.
        let class_initial: Vec<(ClassId, Vec<Ty>)> =
            self.typed.class_instantiations.clone();
        for (cid, type_args) in class_initial {
            if type_args.iter().any(has_unbound_var) { continue; }
            self.register_class_instantiation(cid, type_args);
        }

        // Pass 3: lower function bodies.
        for d in &decls {
            match d {
                TopDecl::Func(f) => {
                    // M17: generic templates aren't lowered directly — they
                    // produce one IRFunction per instantiation in Pass 3.5.
                    if !f.generics.is_empty() { continue; }
                    let fid = *self.fn_id_by_name.get(&f.name).unwrap();
                    let irfn = self.lower_func(fid, f, None);
                    self.register_fn_table(&irfn);
                    self.out.functions.push(irfn);
                }
                TopDecl::Class(c) => {
                    // M31: skip generic-class templates — their methods
                    // produce one IRFunction per instantiation in Pass 3.6.
                    if !c.generics.is_empty() { continue; }
                    let recv_cid = self
                        .lookup_module_symbol(&c.name)
                        .and_then(|sid| self.typed.resolved.symbols.get(sid).class_id);
                    if let Some(init) = &c.init {
                        let name = format!("{}.__init__", c.name);
                        let fid = *self.fn_id_by_name.get(&name).unwrap();
                        let mut irfn = self.lower_func(fid, init, recv_cid);
                        irfn.name = name;
                        self.register_fn_table(&irfn);
                        self.out.functions.push(irfn);
                    }
                    for m in &c.methods {
                        let name = format!("{}.{}", c.name, m.name);
                        let fid = *self.fn_id_by_name.get(&name).unwrap();
                        let mut irfn = self.lower_func(fid, m, recv_cid);
                        irfn.name = name;
                        self.register_fn_table(&irfn);
                        self.out.functions.push(irfn);
                    }
                }
                _ => {}
            }
        }

        // Pass 3.5 (M17) + 3.6 (M31): drive the monomorphisation worklists
        // to fixpoint. The two are interleaved because:
        //
        //   * lowering a generic-fn body can discover a *class*
        //     instantiation (e.g. `fn unbox[T](b: Box[T]) -> T:`
        //     transitively constructs `Box[i64]` when called with
        //     `Box[i64]` arg);
        //   * lowering a generic-class method body can discover a *fn*
        //     instantiation (e.g. `Stack[T].push` calling `swap[T]`).
        //
        // We loop until both worklists are drained simultaneously.
        loop {
            let fn_pending = !self.inst_worklist.is_empty();
            let cls_pending = !self.class_inst_worklist.is_empty();
            if !fn_pending && !cls_pending { break; }

            while let Some((sid, type_args)) = self.inst_worklist.pop() {
                let key = mangle_args_key(&type_args);
                let fid = match self.fn_id_for_inst.get(&(sid, key.clone())).copied() {
                    Some(f) => f,
                    None => continue,
                };
                let mangled = self.mangled_name_for_inst
                    .get(&(sid, key.clone())).cloned().unwrap_or_default();
                let src_name = self.typed.resolved.symbols.get(sid).name.clone();
                let decl = match self.generic_fn_decls.get(&src_name).cloned() {
                    Some(d) => d,
                    None => continue,
                };
                let tvars = self.tvars_for_sid.get(&sid).cloned().unwrap_or_default();
                let mut subst: HashMap<u32, Ty> = HashMap::new();
                for (tv, ty_arg) in tvars.iter().zip(&type_args) {
                    subst.insert(tv.0, ty_arg.clone());
                }
                let irfn = self.lower_func_instantiation(fid, &decl, &mangled, &subst);
                self.register_fn_table(&irfn);
                self.out.functions.push(irfn);
            }

            while let Some((cid, type_args)) = self.class_inst_worklist.pop() {
                self.lower_class_instantiation(cid, &type_args);
            }
        }
    }

    /// M31: register a fresh type_id, per-instantiation `__init__` FuncId,
    /// and one FuncId per declared method for a generic-class instantiation
    /// `(class_id, type_args)`. Emits the TypeTableEntry immediately so
    /// Alloc operands at call sites can reference the concrete tid.
    /// Idempotent — repeated calls with the same key are no-ops. Pushes
    /// the entry onto `class_inst_worklist` so the bodies get lowered in
    /// Pass 3.6.
    fn register_class_instantiation(&mut self, cid: ClassId, type_args: Vec<Ty>) {
        let key = mangle_args_key(&type_args);
        if self.class_inst_type_id.contains_key(&(cid, key.clone())) {
            return;
        }
        let layout = match self.typed.resolved.class_layouts.get(&cid).cloned() {
            Some(l) => l,
            None => return,
        };
        // Build the substitution {tv_i -> type_args[i]}.
        let mut subst: HashMap<u32, Ty> = HashMap::new();
        for (tv, ty_arg) in layout.generic_tvars.iter().zip(&type_args) {
            subst.insert(tv.0, ty_arg.clone());
        }
        // Mangled class name (e.g. `Box__i64`, `Pair__str_i32`). Stored
        // both for the type-table's name entry and for naming the
        // per-instantiation IRFunctions.
        let mangled = format!("{}__{}", layout.name, key);
        let tid = self.fresh_type_id();
        self.class_inst_type_id.insert((cid, key.clone()), tid);
        self.class_inst_name.insert((cid, key.clone()), mangled.clone());

        // Pre-allocate FuncIds for __init__ + each method. We also look up
        // the source FuncDecl from `generic_class_decls`. If a method body
        // happens to call into another generic class, the eager FuncId
        // allocation here means the body lowering can resolve the call
        // directly.
        let class_decl = self.generic_class_decls.get(&cid).cloned();
        let has_init = class_decl.as_ref().map(|c| c.init.is_some()).unwrap_or(false);
        if has_init {
            let fid = self.fresh_fn_id();
            self.class_inst_init_fn.insert((cid, key.clone()), fid);
        }
        if let Some(c_decl) = &class_decl {
            for m in &c_decl.methods {
                let fid = self.fresh_fn_id();
                self.class_inst_method_fn
                    .insert((cid, key.clone(), m.name.clone()), fid);
            }
        }

        // Emit the TypeTableEntry for this instantiation. Field offsets
        // come from the abstract layout (every Ty::Var field uses an
        // 8-byte slot, so offsets remain valid). Field types are
        // substituted so the runtime type_id of each field references the
        // concrete type.
        //
        // The vtable: one fn id per non-`__init__` method in the layout's
        // method list. We resolve each name via `class_inst_method_fn`.
        let name_idx = self.intern_str(&mangled);
        let mut fields_out = Vec::new();
        for f in &layout.fields {
            let sub_ty = subst_ty(&f.ty, &subst);
            let fname = self.intern_str(&f.name);
            fields_out.push(TypeFieldEntry {
                name_idx: fname,
                type_id: self.type_id_of_ty(&sub_ty),
                offset: f.offset,
            });
        }
        let mut vtable = Vec::new();
        for m in &layout.methods {
            if m.name == "__init__" { continue; }
            let fid = self
                .class_inst_method_fn
                .get(&(cid, key.clone(), m.name.clone()))
                .map(|f| f.0)
                .unwrap_or(u32::MAX);
            vtable.push(fid);
        }
        // M31: payload sizing follows the M11 BUG-016 conservative formula
        // — at least one 8-byte word per field, padded to layout.payload_size.
        let words = (layout.fields.len() as u32).max((layout.payload_size + 7) / 8);
        let size = 16 + words * 8;
        self.out.type_table.push(TypeTableEntry {
            type_id: tid,
            kind: 1, // class
            name_idx,
            size,
            // Generic classes have no inheritance support in M31 — the
            // base must be None per the language scope. We surface
            // NO_BASE_TYPE here. (Subclassing a parameterised class is
            // documented as v0.4 work.)
            base_type: strictpy_shared::file_format::NO_BASE_TYPE,
            fields: fields_out,
            vtable,
        });

        self.class_inst_worklist.push((cid, type_args));
    }

    /// M31: lower one body of a generic-class instantiation. Emits
    /// `__init__` (if declared) and every method as a separate IRFunction
    /// under the substitution `{tv_i -> type_args[i]}`. The class body
    /// must have been pre-registered in `class_inst_*` tables — this
    /// function panics if not. Naming: each emitted IRFunction is
    /// `Box__i64.__init__`, `Box__i64.unwrap`, etc., so the function
    /// table dump remains debuggable.
    fn lower_class_instantiation(&mut self, cid: ClassId, type_args: &[Ty]) {
        let key = mangle_args_key(type_args);
        let layout = match self.typed.resolved.class_layouts.get(&cid).cloned() {
            Some(l) => l,
            None => return,
        };
        let class_decl = match self.generic_class_decls.get(&cid).cloned() {
            Some(d) => d,
            None => return,
        };
        let mangled = self
            .class_inst_name
            .get(&(cid, key.clone()))
            .cloned()
            .unwrap_or_default();
        let mut subst: HashMap<u32, Ty> = HashMap::new();
        for (tv, ty_arg) in layout.generic_tvars.iter().zip(type_args.iter()) {
            subst.insert(tv.0, ty_arg.clone());
        }
        // Lower __init__ if present.
        if let Some(init) = &class_decl.init {
            if let Some(fid) = self
                .class_inst_init_fn
                .get(&(cid, key.clone()))
                .copied()
            {
                let irfn = self.lower_method_instantiation(
                    fid,
                    init,
                    &format!("{}.__init__", mangled),
                    Some(cid),
                    &subst,
                );
                self.register_fn_table(&irfn);
                self.out.functions.push(irfn);
            }
        }
        for m in &class_decl.methods {
            let fid = match self
                .class_inst_method_fn
                .get(&(cid, key.clone(), m.name.clone()))
                .copied()
            {
                Some(f) => f,
                None => continue,
            };
            let irfn = self.lower_method_instantiation(
                fid,
                m,
                &format!("{}.{}", mangled, m.name),
                Some(cid),
                &subst,
            );
            self.register_fn_table(&irfn);
            self.out.functions.push(irfn);
        }
    }

    /// M31: like `lower_func_instantiation`, but also handles the
    /// implicit `self` parameter. The body sees `self: Ty::Class(cid)` —
    /// field accesses and method dispatch on `self` then resolve through
    /// the (substituted) class layout, exactly as a non-generic method
    /// body does. The `subst` map is applied to every type lookup so
    /// per-instantiation field/local types come out concrete.
    fn lower_method_instantiation(
        &mut self,
        id: FuncId,
        f: &FuncDecl,
        mangled_name: &str,
        recv: Option<ClassId>,
        subst: &HashMap<u32, Ty>,
    ) -> IRFunction {
        let mut fb = FuncBuilder::new(id, mangled_name);
        let mut param_tys: Vec<Ty> = Vec::new();
        if let Some(cid) = recv {
            param_tys.push(Ty::Class(cid));
            fb.params.push(("self".to_string(), Ty::Class(cid)));
        }
        for p in &f.params {
            if recv.is_some() && p.name == "self" { continue; }
            let raw = self.lookup_ast_ty(&p.ty).unwrap_or(Ty::Primitive(PrimTy::Unit));
            let ty = subst_ty(&raw, subst);
            param_tys.push(ty.clone());
            fb.params.push((p.name.clone(), ty));
        }
        let raw_ret = self.lookup_ast_ty(&f.return_ty).unwrap_or(Ty::Primitive(PrimTy::Unit));
        let ret_ty = subst_ty(&raw_ret, subst);

        let entry = fb.new_block();
        fb.current = entry;
        for (idx, (name, ty)) in fb.params.clone().iter().enumerate() {
            let v = fb.push_value(ty.clone(), ValueKind::Param { idx: idx as u32 });
            let slot = fb.alloc_slot(name, ty.clone());
            fb.emit_write_local(slot, v);
        }

        let mut lifted: Vec<IRFunction> = Vec::new();
        {
            let mut ctx = LowerCtx {
                typed: &self.typed,
                str_intern: &mut self.str_intern,
                string_table: &mut self.out.string_table,
                fn_id_by_name: &self.fn_id_by_name,
                class_layouts: &self.typed.resolved.class_layouts,
                class_type_id: &self.class_type_id,
                tuple_type_id: &self.tuple_type_id,
                module_consts: &self.module_consts,
                next_fn_id: &mut self.next_fn_id,
                lifted_functions: &mut lifted,
                type_subst: subst.clone(),
                generic_fn_sid: &self.generic_fn_sid,
                fn_id_for_inst: &mut self.fn_id_for_inst,
                mangled_name_for_inst: &mut self.mangled_name_for_inst,
                tvars_for_sid: &self.tvars_for_sid,
                inst_worklist: &mut self.inst_worklist,
                class_inst_type_id: &mut self.class_inst_type_id,
                class_inst_init_fn: &mut self.class_inst_init_fn,
                class_inst_method_fn: &mut self.class_inst_method_fn,
                class_inst_name: &mut self.class_inst_name,
                class_inst_worklist: &mut self.class_inst_worklist,
            };
            let _ = lower_block(&mut fb, &mut ctx, &f.body);
        }

        let cur_id = fb.current;
        let cur_idx = cur_id.0 as usize;
        if let Terminator::Unreachable = fb.blocks[cur_idx].terminator {
            fb.blocks[cur_idx].terminator = Terminator::Ret { value: None };
        }

        let main_irfn = IRFunction {
            id,
            name: mangled_name.to_string(),
            params: param_tys,
            ret: ret_ty,
            blocks: fb.blocks,
        };
        for lf in lifted {
            self.register_fn_table(&lf);
            self.out.functions.push(lf);
        }
        main_irfn
    }

    /// M17: register a fresh `FuncId` and mangled name for a generic-fn
    /// instantiation `(sid, type_args)`. Idempotent — calls with the same
    /// key are no-ops. Pushes the entry onto `inst_worklist` for body
    /// lowering in Pass 3.5.
    fn register_instantiation(&mut self, sid: SymbolId, type_args: Vec<Ty>) -> FuncId {
        let key = mangle_args_key(&type_args);
        if let Some(fid) = self.fn_id_for_inst.get(&(sid, key.clone())).copied() {
            return fid;
        }
        let src_name = self.typed.resolved.symbols.get(sid).name.clone();
        let mangled = format!("{}__{}", src_name, key);
        let fid = self.fresh_fn_id();
        self.fn_id_for_inst.insert((sid, key.clone()), fid);
        self.type_args_for_inst.insert((sid, key.clone()), type_args.clone());
        self.mangled_name_for_inst.insert((sid, key.clone()), mangled);
        self.inst_worklist.push((sid, type_args));
        fid
    }

    /// M17: lower one body of a generic fn under substitution `subst`. The
    /// substitution applies to every type annotation (params, return type,
    /// locals) and to every `expr_types` lookup during expression lowering
    /// — the lowerer carries it inside `LowerCtx::type_subst`.
    fn lower_func_instantiation(
        &mut self,
        id: FuncId,
        f: &FuncDecl,
        mangled_name: &str,
        subst: &HashMap<u32, Ty>,
    ) -> IRFunction {
        let mut fb = FuncBuilder::new(id, mangled_name);
        let mut param_tys: Vec<Ty> = Vec::new();
        for p in &f.params {
            let raw = self.lookup_ast_ty(&p.ty).unwrap_or(Ty::Primitive(PrimTy::Unit));
            let ty = subst_ty(&raw, subst);
            param_tys.push(ty.clone());
            fb.params.push((p.name.clone(), ty));
        }
        let raw_ret = self.lookup_ast_ty(&f.return_ty).unwrap_or(Ty::Primitive(PrimTy::Unit));
        let ret_ty = subst_ty(&raw_ret, subst);

        let entry = fb.new_block();
        fb.current = entry;
        for (idx, (name, ty)) in fb.params.clone().iter().enumerate() {
            let v = fb.push_value(ty.clone(), ValueKind::Param { idx: idx as u32 });
            let slot = fb.alloc_slot(name, ty.clone());
            fb.emit_write_local(slot, v);
        }

        let mut lifted: Vec<IRFunction> = Vec::new();
        {
            let mut ctx = LowerCtx {
                typed: &self.typed,
                str_intern: &mut self.str_intern,
                string_table: &mut self.out.string_table,
                fn_id_by_name: &self.fn_id_by_name,
                class_layouts: &self.typed.resolved.class_layouts,
                class_type_id: &self.class_type_id,
                tuple_type_id: &self.tuple_type_id,
                module_consts: &self.module_consts,
                next_fn_id: &mut self.next_fn_id,
                lifted_functions: &mut lifted,
                type_subst: subst.clone(),
                generic_fn_sid: &self.generic_fn_sid,
                fn_id_for_inst: &mut self.fn_id_for_inst,
                mangled_name_for_inst: &mut self.mangled_name_for_inst,
                tvars_for_sid: &self.tvars_for_sid,
                inst_worklist: &mut self.inst_worklist,
                class_inst_type_id: &mut self.class_inst_type_id,
                class_inst_init_fn: &mut self.class_inst_init_fn,
                class_inst_method_fn: &mut self.class_inst_method_fn,
                class_inst_name: &mut self.class_inst_name,
                class_inst_worklist: &mut self.class_inst_worklist,
            };
            let _ = lower_block(&mut fb, &mut ctx, &f.body);
        }

        let cur_id = fb.current;
        let cur_idx = cur_id.0 as usize;
        if let Terminator::Unreachable = fb.blocks[cur_idx].terminator {
            fb.blocks[cur_idx].terminator = Terminator::Ret { value: None };
        }

        let main_irfn = IRFunction {
            id,
            name: mangled_name.to_string(),
            params: param_tys,
            ret: ret_ty,
            blocks: fb.blocks,
        };
        for lf in lifted {
            self.register_fn_table(&lf);
            self.out.functions.push(lf);
        }
        main_irfn
    }

    /// Walk every expression / param / return / field / local type observed
    /// during type-checking and register a synthetic class-style type-table
    /// entry per distinct `Ty::Tuple` shape.  Each shape gets a uniform
    /// 8-byte-per-element layout so `t.N` lowers to `Load(offset=8*N)` and
    /// `Expr::Tuple` lowers to `Alloc(tid)` followed by N `Store(offset=8*i)`s.
    /// Idempotent — re-running is a no-op once registered.
    fn register_tuple_types(&mut self) {
        // Collect tuple shapes from expr types and from class/function/etc.
        let mut shapes: Vec<Vec<Ty>> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let visit = |ty: &Ty, shapes: &mut Vec<Vec<Ty>>, seen: &mut std::collections::HashSet<String>| {
            fn walk(ty: &Ty, shapes: &mut Vec<Vec<Ty>>, seen: &mut std::collections::HashSet<String>) {
                match ty {
                    Ty::Tuple(elems) => {
                        let key = display_ty(ty);
                        if seen.insert(key) {
                            shapes.push(elems.clone());
                        }
                        for e in elems { walk(e, shapes, seen); }
                    }
                    Ty::Generic { args, .. } => for a in args { walk(a, shapes, seen); }
                    Ty::Function { params, ret } => {
                        for p in params { walk(p, shapes, seen); }
                        walk(ret, shapes, seen);
                    }
                    Ty::Nullable(inner) => walk(inner, shapes, seen),
                    _ => {}
                }
            }
            walk(ty, shapes, seen);
        };

        // expr_types from typechecker
        for ty in self.typed.expr_types.values() {
            visit(ty, &mut shapes, &mut seen);
        }
        // function param/return types via resolver-tracked AST types
        for ty in self.typed.resolved.ast_type_to_ty.values() {
            visit(ty, &mut shapes, &mut seen);
        }
        // class field types
        for layout in self.typed.resolved.class_layouts.values() {
            for f in &layout.fields { visit(&f.ty, &mut shapes, &mut seen); }
            for m in &layout.methods {
                for p in &m.params { visit(p, &mut shapes, &mut seen); }
                visit(&m.ret, &mut shapes, &mut seen);
            }
        }

        for shape in shapes {
            let key = display_ty(&Ty::Tuple(shape.clone()));
            if self.tuple_type_id.contains_key(&key) { continue; }
            let tid = self.fresh_type_id();
            let name_idx = self.intern_str(&format!("__Tuple{}__", shape.len()));
            // Each element occupies one 8-byte slot (header excluded).
            let mut fields = Vec::new();
            for (i, elem) in shape.iter().enumerate() {
                let fname = self.intern_str(&format!("{}", i));
                fields.push(TypeFieldEntry {
                    name_idx: fname,
                    type_id: self.type_id_of_ty(elem),
                    offset: (i as u32) * 8,
                });
            }
            let size = 16 + (shape.len() as u32) * 8;
            self.out.type_table.push(TypeTableEntry {
                type_id: tid,
                kind: 3, // tuple per file_format spec
                name_idx,
                size,
                base_type: strictpy_shared::file_format::NO_BASE_TYPE,
                fields,
                vtable: Vec::new(),
            });
            self.tuple_type_id.insert(key, tid);
        }
    }

    fn register_fn_table(&mut self, irfn: &IRFunction) {
        let name_idx = self.intern_str(&irfn.name);
        let type_id = self.type_id_of_ty(&Ty::Function {
            params: irfn.params.clone(),
            ret: Box::new(irfn.ret.clone()),
        });
        self.out.function_table.push(FunctionTableEntry {
            fn_id: irfn.id.0,
            name_idx,
            type_id,
            num_params: irfn.params.len() as u16,
            num_locals: 0,
            flags: 0,
        });
    }

    fn finish(self) -> IRModule {
        self.out
    }

    fn lookup_module_symbol(&self, name: &str) -> Option<SymbolId> {
        let scope = self.typed.resolved.module_scope;
        self.typed.resolved.symbols.lookup(scope, name)
    }

    /// Map a semantic `Ty` to a u32 type-table id used in bytecode operands.
    /// Primitives get well-known ids 0..15; classes/protocols look up the
    /// class_type_id map; generics/tuples get u32::MAX as a placeholder.
    fn type_id_of_ty(&self, t: &Ty) -> u32 {
        match t {
            Ty::Primitive(p) => match p {
                PrimTy::Bool => 0,
                PrimTy::I8 => 1,  PrimTy::I16 => 2,
                PrimTy::I32 => 3, PrimTy::I64 => 4,
                PrimTy::U8 => 5,  PrimTy::U16 => 6,
                PrimTy::U32 => 7, PrimTy::U64 => 8,
                PrimTy::F32 => 9, PrimTy::F64 => 10,
                PrimTy::Char => 11,
                PrimTy::Str => 12,
                PrimTy::Bytes => 13,
                PrimTy::BigInt => 14,
                PrimTy::Null => 15,
                PrimTy::Unit => 15,
            },
            Ty::Class(cid) => self.class_type_id.get(&cid.0).copied().unwrap_or(u32::MAX),
            Ty::Nullable(inner) => self.type_id_of_ty(inner),
            Ty::Tuple(_) => {
                let key = display_ty(t);
                self.tuple_type_id.get(&key).copied().unwrap_or(u32::MAX)
            }
            _ => u32::MAX,
        }
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Per-function lowering
    // ─────────────────────────────────────────────────────────────────────

    fn lower_func(
        &mut self,
        id: FuncId,
        f: &FuncDecl,
        recv: Option<ClassId>,
    ) -> IRFunction {
        let mut fb = FuncBuilder::new(id, &f.name);
        // Resolve param types — they come from f.params via the resolver, but
        // the AST `Type` is syntactic, not semantic. We look up the symbol of
        // each param name in the enclosing function scope by best-effort:
        // since we don't have direct access to the function scope id, we map
        // param ast::Type → semantic Ty via the resolver's ast_type_to_ty
        // table keyed by span.
        let mut param_tys: Vec<Ty> = Vec::new();
        // Implicit `self` for methods.
        if let Some(cid) = recv {
            param_tys.push(Ty::Class(cid));
            fb.params.push(("self".to_string(), Ty::Class(cid)));
        }
        for p in &f.params {
            // The parser keeps `self` as the first AST param of methods.
            // We've already installed the implicit `self` above with the
            // correct receiver type, so skip the AST entry to avoid a
            // duplicate slot (which would overwrite `self` with Unit and
            // turn every subsequent field-store into a null deref).
            if recv.is_some() && p.name == "self" {
                continue;
            }
            let ty = self.lookup_ast_ty(&p.ty).unwrap_or(Ty::Primitive(PrimTy::Unit));
            param_tys.push(ty.clone());
            fb.params.push((p.name.clone(), ty));
        }
        let ret_ty = self.lookup_ast_ty(&f.return_ty).unwrap_or(Ty::Primitive(PrimTy::Unit));

        // Allocate the entry block, install param values into slots so
        // assignments rebind correctly (see `slot_of`).
        let entry = fb.new_block();
        fb.current = entry;
        for (idx, (name, ty)) in fb.params.clone().iter().enumerate() {
            let v = fb.push_value(ty.clone(), ValueKind::Param { idx: idx as u32 });
            let slot = fb.alloc_slot(name, ty.clone());
            fb.emit_write_local(slot, v);
        }

        // Lower body — best-effort.
        let mut lifted: Vec<IRFunction> = Vec::new();
        {
            let mut ctx = LowerCtx {
                typed: &self.typed,
                str_intern: &mut self.str_intern,
                string_table: &mut self.out.string_table,
                fn_id_by_name: &self.fn_id_by_name,
                class_layouts: &self.typed.resolved.class_layouts,
                class_type_id: &self.class_type_id,
                tuple_type_id: &self.tuple_type_id,
                module_consts: &self.module_consts,
                next_fn_id: &mut self.next_fn_id,
                lifted_functions: &mut lifted,
                // M17: non-generic bodies have an empty substitution.
                type_subst: HashMap::new(),
                generic_fn_sid: &self.generic_fn_sid,
                fn_id_for_inst: &mut self.fn_id_for_inst,
                mangled_name_for_inst: &mut self.mangled_name_for_inst,
                tvars_for_sid: &self.tvars_for_sid,
                inst_worklist: &mut self.inst_worklist,
                class_inst_type_id: &mut self.class_inst_type_id,
                class_inst_init_fn: &mut self.class_inst_init_fn,
                class_inst_method_fn: &mut self.class_inst_method_fn,
                class_inst_name: &mut self.class_inst_name,
                class_inst_worklist: &mut self.class_inst_worklist,
            };
            let _ = lower_block(&mut fb, &mut ctx, &f.body);
        }

        // Ensure the last block has a terminator.
        let cur_id = fb.current;
        let cur_idx = cur_id.0 as usize;
        if let Terminator::Unreachable = fb.blocks[cur_idx].terminator {
            // For -> None or fall-through, emit Ret(None).
            fb.blocks[cur_idx].terminator = Terminator::Ret { value: None };
        }

        let main_irfn = IRFunction {
            id,
            name: f.name.clone(),
            params: param_tys,
            ret: ret_ty,
            blocks: fb.blocks,
        };

        // Lambdas lifted while lowering this function become first-class
        // module-level IRFunctions. Register them in the function table
        // and emit them alongside top-level fns.
        for lf in lifted {
            self.register_fn_table(&lf);
            self.out.functions.push(lf);
        }

        main_irfn
    }

    fn lookup_ast_ty(&self, t: &ast::Type) -> Option<Ty> {
        // The resolver maps `ast::Type` spans to semantic `Ty`. Recover via
        // the span of `t` itself.
        let span = ast_type_span(t);
        self.typed
            .resolved
            .ast_type_to_ty
            .get(&(span.start, span.end))
            .cloned()
    }
}

fn ast_type_span(t: &ast::Type) -> Span {
    match t {
        ast::Type::Named { span, .. }
        | ast::Type::Nullable { span, .. }
        | ast::Type::Function { span, .. }
        | ast::Type::Tuple { span, .. }
        | ast::Type::Infer { span }
        | ast::Type::Never { span } => *span,
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  FuncBuilder — per-function mutable state
// ─────────────────────────────────────────────────────────────────────────

struct FuncBuilder {
    #[allow(dead_code)]
    id: FuncId,
    #[allow(dead_code)]
    name: String,
    params: Vec<(String, Ty)>,
    blocks: Vec<BasicBlock>,
    current: BlockId,
    next_value: u32,
    /// Symbol-name → slot index for locals that live in addressable slots.
    /// Reads emit `ReadLocal`; writes emit `WriteLocal`. This lets values
    /// stored in one block be observed by reads in another (e.g. across
    /// the back-edge of a `while` loop).
    slot_of: HashMap<String, u16>,
    /// Per-slot type, indexed by slot id.
    slot_ty: Vec<Ty>,
    /// Loop stack for break/continue: (header_block, exit_block).
    loop_stack: Vec<(BlockId, BlockId)>,
}

impl FuncBuilder {
    fn new(id: FuncId, name: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            params: Vec::new(),
            blocks: Vec::new(),
            current: BlockId(0),
            next_value: 0,
            slot_of: HashMap::new(),
            slot_ty: Vec::new(),
            loop_stack: Vec::new(),
        }
    }

    /// Allocate a fresh slot for `name` with the given type. Returns the
    /// slot index. If `name` already has a slot, return the existing one
    /// (rebinding does not change the slot).
    fn alloc_slot(&mut self, name: &str, ty: Ty) -> u16 {
        if let Some(s) = self.slot_of.get(name) {
            return *s;
        }
        let s = self.slot_ty.len() as u16;
        self.slot_ty.push(ty);
        self.slot_of.insert(name.to_string(), s);
        s
    }

    fn slot_for(&self, name: &str) -> Option<u16> {
        self.slot_of.get(name).copied()
    }

    fn slot_type(&self, slot: u16) -> Ty {
        self.slot_ty
            .get(slot as usize)
            .cloned()
            .unwrap_or(Ty::Primitive(PrimTy::Unit))
    }

    /// Emit a `WriteLocal` for `slot` storing `v`.
    fn emit_write_local(&mut self, slot: u16, v: ValueId) {
        self.push_value(
            Ty::Primitive(PrimTy::Unit),
            ValueKind::Op { op: IROp::WriteLocal { slot }, args: vec![v] },
        );
    }

    /// Emit a `ReadLocal` for `slot` and return the freshly-bound SSA value.
    fn emit_read_local(&mut self, slot: u16) -> ValueId {
        let ty = self.slot_type(slot);
        self.push_value(ty, ValueKind::Op { op: IROp::ReadLocal { slot }, args: vec![] })
    }

    fn new_block(&mut self) -> BlockId {
        let id = BlockId(self.blocks.len() as u32);
        self.blocks.push(BasicBlock {
            id,
            values: Vec::new(),
            terminator: Terminator::Unreachable,
        });
        id
    }

    fn fresh_value(&mut self) -> u32 {
        let v = self.next_value;
        self.next_value += 1;
        v
    }

    fn push_value(&mut self, ty: Ty, kind: ValueKind) -> ValueId {
        let id = self.fresh_value();
        let v = Value { id, ty, kind };
        let bidx = self.current.0 as usize;
        self.blocks[bidx].values.push(v);
        ValueId(id)
    }

    fn terminate(&mut self, t: Terminator) {
        let bidx = self.current.0 as usize;
        if matches!(self.blocks[bidx].terminator, Terminator::Unreachable) {
            self.blocks[bidx].terminator = t;
        }
    }

    fn switch_to(&mut self, b: BlockId) {
        self.current = b;
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Lowering context (immutable views passed down)
// ─────────────────────────────────────────────────────────────────────────

struct LowerCtx<'a> {
    typed: &'a TypedModule,
    str_intern: &'a mut HashMap<String, u32>,
    string_table: &'a mut Vec<String>,
    fn_id_by_name: &'a HashMap<String, FuncId>,
    class_layouts: &'a HashMap<ClassId, ClassLayout>,
    #[allow(dead_code)]
    class_type_id: &'a HashMap<u32, u32>,
    /// M14: tuple-shape → synthetic type-table id, set up in Pass 2 and
    /// consulted when lowering `Expr::Tuple` to pick the right Alloc tid.
    tuple_type_id: &'a HashMap<String, u32>,
    /// Top-level `final` consts folded to their literal IR value.
    module_consts: &'a HashMap<String, (IRConst, Ty)>,
    /// Mutable handle to the lowerer's fn-id allocator so lifted lambdas
    /// can claim a fresh id.
    next_fn_id: &'a mut u32,
    /// Lambdas lowered as the body of the current function go here; the
    /// outer lowerer flushes them into the module's function list once
    /// the parent function finishes.
    lifted_functions: &'a mut Vec<IRFunction>,
    /// M17: type-substitution active for the *current* function body. Empty
    /// for non-generic fns; a `Var(tv) -> concrete` map for an
    /// instantiation. Used by `expr_ty` to apply substitution to recorded
    /// types and by `lower_call` to specialise generic-callee dispatch.
    type_subst: HashMap<u32, Ty>,
    /// M17: source name → SymbolId for every generic top-level fn.
    generic_fn_sid: &'a HashMap<String, SymbolId>,
    /// M17: per-instantiation FuncId table keyed by `(sid, mangle_key)`.
    /// Mutable so `lower_call` can mint a fresh FuncId the *first* time it
    /// sees a transitive instantiation (e.g. `quicksort[i64]` inside its
    /// own body, or `partition[i64]` from `quicksort[i64]`), and emit a
    /// well-formed `DirectCall` to it immediately.
    fn_id_for_inst: &'a mut HashMap<(SymbolId, String), FuncId>,
    /// M17: mangled name per instantiation (parallel to `fn_id_for_inst`).
    mangled_name_for_inst: &'a mut HashMap<(SymbolId, String), String>,
    /// M17: TypeVarIds declared by each generic fn, in declaration order.
    tvars_for_sid: &'a HashMap<SymbolId, Vec<TypeVarId>>,
    /// M17: worklist of `(sid, type_args)` pairs needing body lowering.
    /// `lower_call` pushes here when it mints a new FuncId; the outer
    /// `run()` loop pops from here.
    inst_worklist: &'a mut Vec<(SymbolId, Vec<Ty>)>,
    /// M31: per-instantiation tables for generic classes. Mirrors the
    /// generic-fn maps above. Mutable so `lower_call` (constructor of a
    /// generic class with concrete type args discovered transitively in
    /// a generic body) can register fresh instantiations and emit
    /// well-typed `Alloc + DirectCall` immediately.
    class_inst_type_id: &'a mut HashMap<(ClassId, String), u32>,
    class_inst_init_fn: &'a mut HashMap<(ClassId, String), FuncId>,
    class_inst_method_fn: &'a mut HashMap<(ClassId, String, String), FuncId>,
    class_inst_name: &'a mut HashMap<(ClassId, String), String>,
    class_inst_worklist: &'a mut Vec<(ClassId, Vec<Ty>)>,
}

impl<'a> LowerCtx<'a> {
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(idx) = self.str_intern.get(s) {
            return *idx;
        }
        let idx = self.string_table.len() as u32;
        self.string_table.push(s.to_string());
        self.str_intern.insert(s.to_string(), idx);
        idx
    }

    fn expr_ty(&self, span: Span) -> Ty {
        let raw = self.typed
            .expr_types
            .get(&(span.start, span.end))
            .cloned()
            .unwrap_or(Ty::Primitive(PrimTy::Unit));
        // M17: if we're inside an instantiated generic body, apply the
        // active substitution so downstream lowering sees concrete types
        // (e.g. `Ty::Var(0)` becomes `Ty::Primitive(PrimTy::I64)`). For
        // non-generic bodies the map is empty and this is a no-op.
        if self.type_subst.is_empty() { raw } else { subst_ty(&raw, &self.type_subst) }
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Statement / expression lowering
// ─────────────────────────────────────────────────────────────────────────

fn lower_block(fb: &mut FuncBuilder, ctx: &mut LowerCtx, b: &Block) -> Option<()> {
    for s in &b.stmts {
        lower_stmt(fb, ctx, s)?;
    }
    Some(())
}

fn lower_stmt(fb: &mut FuncBuilder, ctx: &mut LowerCtx, s: &Stmt) -> Option<()> {
    match s {
        Stmt::Let { name, init, ty, .. } => {
            let v = lower_expr(fb, ctx, init);
            let slot_ty = ctx
                .typed
                .resolved
                .ast_type_to_ty
                .get(&(ast_type_span(ty).start, ast_type_span(ty).end))
                .cloned()
                .or_else(|| find_value_ty(fb, v))
                .unwrap_or(Ty::Primitive(PrimTy::Unit));
            let slot = fb.alloc_slot(name, slot_ty);
            fb.emit_write_local(slot, v);
            Some(())
        }
        Stmt::LetDestructure { names, init, .. } => {
            // M14 tuples: evaluate the RHS once into a hidden tuple value,
            // then materialise each name as a Load(8*i) followed by a slot
            // write.  Element types are pulled from the RHS tuple's static
            // type (which the typechecker already verified against any
            // per-name annotations).
            let tup = lower_expr(fb, ctx, init);
            let tup_ty = find_value_ty(fb, tup).unwrap_or(Ty::Primitive(PrimTy::Unit));
            let elem_tys: Vec<Ty> = match &tup_ty {
                Ty::Tuple(ts) => ts.clone(),
                _ => vec![Ty::Primitive(PrimTy::Unit); names.len()],
            };
            for (i, n) in names.iter().enumerate() {
                let elem_ty = elem_tys.get(i).cloned()
                    .unwrap_or(Ty::Primitive(PrimTy::Unit));
                let v = fb.push_value(
                    elem_ty.clone(),
                    ValueKind::Op {
                        op: IROp::Load { offset: (i as u32) * 8 },
                        args: vec![tup],
                    },
                );
                let slot = fb.alloc_slot(n, elem_ty);
                fb.emit_write_local(slot, v);
            }
            Some(())
        }
        Stmt::Assign { target, value, .. } => {
            let v = lower_expr(fb, ctx, value);
            lower_lvalue_store(fb, ctx, target, v);
            Some(())
        }
        Stmt::AugAssign { target, op, value, .. } => {
            // x op= y  ⇒  x = x op y
            let cur = lower_lvalue_load(fb, ctx, target);
            let rhs = lower_expr(fb, ctx, value);
            let ty = find_value_ty(fb, cur).unwrap_or(Ty::Primitive(PrimTy::Unit));
            let combined = emit_binop(fb, *op, cur, rhs, ty);
            lower_lvalue_store(fb, ctx, target, combined);
            Some(())
        }
        Stmt::Return { value, .. } => {
            let v = value.as_ref().map(|e| lower_expr(fb, ctx, e));
            fb.terminate(Terminator::Ret { value: v });
            // Open a new unreachable block so subsequent stmts still have a current block.
            let nb = fb.new_block();
            fb.switch_to(nb);
            Some(())
        }
        Stmt::If { cond, then_block, elifs, else_block, .. } => {
            lower_if_chain(fb, ctx, cond, then_block, elifs, else_block.as_ref());
            Some(())
        }
        Stmt::While { cond, body, .. } => {
            lower_while(fb, ctx, cond, body);
            Some(())
        }
        Stmt::For { var, var_ty, iter, body, .. } => {
            // Desugar `for x: T in xs: body` into the index-counted while
            // loop equivalent, for now supporting only `List[T]` receivers:
            //
            //     __i: i64 = 0
            //     __n: i64 = ArrayLen(xs)
            //     while __i < __n:
            //         x: T = xs[__i]
            //         <body>
            //         __i = __i + 1
            //
            // Generic iterator protocol (`__iter__` / `__next__`) is M10
            // work; until then a non-List receiver falls back to the old
            // placeholder behaviour so we don't regress IR validity for
            // programs that loop over `range(...)` etc.
            //
            // real-world: Game of Life, Sudoku, JSON parser et al. all
            // want a one-liner `for x in xs:` instead of hand-rolled
            // index loops.
            let iter_ty = ctx.expr_ty(expr_span(iter));
            let is_list = matches!(
                &iter_ty,
                Ty::Generic { base: TypeCtor::List, .. }
            );
            if !is_list {
                // Fallback path — preserves the prior placeholder so we
                // don't crash on `for i in range(...)` or other non-List
                // iterables. TODO(M10): full __iter__/__next__ protocol.
                let _ = lower_expr(fb, ctx, iter);
                let placeholder_ty = Ty::Primitive(PrimTy::Unit);
                let v = fb.push_value(placeholder_ty.clone(), ValueKind::Const(IRConst::None));
                let slot = fb.alloc_slot(var, placeholder_ty);
                fb.emit_write_local(slot, v);
                return lower_block(fb, ctx, body);
            }

            // Element type from `List[T]` — fall back to the declared
            // var_ty if the args slot is empty for some reason.
            let elem_ty = match &iter_ty {
                Ty::Generic { args, .. } if !args.is_empty() => args[0].clone(),
                _ => ctx
                    .typed
                    .resolved
                    .ast_type_to_ty
                    .get(&(ast_type_span(var_ty).start, ast_type_span(var_ty).end))
                    .cloned()
                    .unwrap_or(Ty::Primitive(PrimTy::Unit)),
            };

            // Materialise the iterable once, before the loop header, so
            // `xs` is only evaluated a single time.
            let arr = lower_expr(fb, ctx, iter);
            let i64_ty = Ty::Primitive(PrimTy::I64);

            // __i: i64 = 0
            let zero = fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(0)));
            let i_slot = {
                // Use a unique name so a nested `for` doesn't clobber the outer
                // one's counter. Slot-of-name lookup keys the slot table.
                let n = format!("__for_i_{}", fb.slot_ty.len());
                let s = fb.alloc_slot(&n, i64_ty.clone());
                fb.emit_write_local(s, zero);
                s
            };
            // __n: i64 = ArrayLen(xs)
            let len_v = fb.push_value(
                i64_ty.clone(),
                ValueKind::Op { op: IROp::ArrayLen, args: vec![arr] },
            );
            let n_slot = {
                let n = format!("__for_n_{}", fb.slot_ty.len());
                let s = fb.alloc_slot(&n, i64_ty.clone());
                fb.emit_write_local(s, len_v);
                s
            };

            // Declare the user-visible loop variable up-front so reads
            // inside the body resolve through `slot_for(var)`.
            let var_slot = fb.alloc_slot(var, elem_ty.clone());

            // while __i < __n:
            let header = fb.new_block();
            let body_b = fb.new_block();
            let exit = fb.new_block();
            fb.terminate(Terminator::Branch { target: header });

            // header: test
            fb.switch_to(header);
            let i_cur = fb.emit_read_local(i_slot);
            let n_cur = fb.emit_read_local(n_slot);
            let cond = fb.push_value(
                Ty::Primitive(PrimTy::Bool),
                ValueKind::Op { op: IROp::ILt, args: vec![i_cur, n_cur] },
            );
            fb.terminate(Terminator::CondBranch { cond, t: body_b, f: exit });

            // body: x = xs[__i]; <body>; __i = __i + 1
            fb.switch_to(body_b);
            let i_now = fb.emit_read_local(i_slot);
            let elt = fb.push_value(
                elem_ty.clone(),
                ValueKind::Op { op: IROp::ArrayGet, args: vec![arr, i_now] },
            );
            fb.emit_write_local(var_slot, elt);

            fb.loop_stack.push((header, exit));
            lower_block(fb, ctx, body);
            fb.loop_stack.pop();

            // __i = __i + 1
            let i_again = fb.emit_read_local(i_slot);
            let one = fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(1)));
            let next_i = fb.push_value(
                i64_ty.clone(),
                ValueKind::Op { op: IROp::IAdd, args: vec![i_again, one] },
            );
            fb.emit_write_local(i_slot, next_i);
            fb.terminate(Terminator::Branch { target: header });

            fb.switch_to(exit);
            Some(())
        }
        Stmt::With { expr, binding, body, .. } => {
            // M7 simplification (spec §7.6 full version is try/finally):
            //   __res = <expr>            # for `open(...)` this is a FileRepr
            //   <body>
            //   _   = FileClose(__res)    # always-execute close on normal exit
            //
            // We don't yet route exceptions through the close — that needs
            // real try/finally edges (M4+ work). The body either runs to
            // completion or aborts the whole program, both of which the
            // FileClose handles correctly for the v0.1 examples.
            let v = lower_expr(fb, ctx, expr);
            let ty = find_value_ty(fb, v).unwrap_or(Ty::Primitive(PrimTy::Unit));
            if let Some((name, _)) = binding {
                let slot = fb.alloc_slot(name, ty.clone());
                fb.emit_write_local(slot, v);
            }
            lower_block(fb, ctx, body);
            // Pick the right close native by the static type of the
            // resource. For io.File (the only stdlib resource manager in
            // v0.1) use FileClose; for any other type, skip the close —
            // we'd otherwise dispatch FileClose against a non-file handle.
            let is_file = matches!(
                &ty,
                Ty::Class(cid)
                    if ctx
                        .class_layouts
                        .get(cid)
                        .map(|l| l.name == "io.File")
                        .unwrap_or(false)
            );
            if is_file {
                fb.push_value(
                    Ty::Primitive(PrimTy::Unit),
                    ValueKind::Op {
                        op: IROp::NativeCall { native_id: NativeFn::FileClose as u32 },
                        args: vec![v],
                    },
                );
            }
            Some(())
        }
        Stmt::Try { body, handlers, finally_block, .. } => {
            lower_try(fb, ctx, body, handlers, finally_block.as_ref());
            Some(())
        }
        Stmt::Raise { exc, .. } => {
            lower_raise(fb, ctx, exc);
            Some(())
        }
        Stmt::Assert { cond, .. } => {
            // M14 quirk: the typechecker unwraps `assert(cond, msg)` (parsed as
            // `assert <2-tuple>`) into `(cond, msg)`, so the inner Eq is what
            // got typed — the outer Tuple has no entry in expr_types and would
            // now wrongly allocate a tuple with tid=u32::MAX.  Mirror that
            // unwrap here so we lower the real condition expression.
            let real_cond: &Expr = match cond {
                Expr::Tuple { elems, .. } if elems.len() == 2 => &elems[0],
                Expr::Tuple { elems, .. } if elems.len() == 1 => &elems[0],
                other => other,
            };
            let _ = lower_expr(fb, ctx, real_cond);
            // Skip the call to assert() for now — codegen will see the discarded value.
            Some(())
        }
        Stmt::Del { .. } => Some(()),
        Stmt::Expr { expr, .. } => {
            let _ = lower_expr(fb, ctx, expr);
            Some(())
        }
        Stmt::Break { .. } => {
            if let Some((_, exit)) = fb.loop_stack.last().copied() {
                fb.terminate(Terminator::Branch { target: exit });
                let nb = fb.new_block();
                fb.switch_to(nb);
            }
            Some(())
        }
        Stmt::Continue { .. } => {
            if let Some((header, _)) = fb.loop_stack.last().copied() {
                fb.terminate(Terminator::Branch { target: header });
                let nb = fb.new_block();
                fb.switch_to(nb);
            }
            Some(())
        }
        Stmt::Match { scrutinee, arms, .. } => {
            lower_match(fb, ctx, scrutinee, arms);
            Some(())
        }
        Stmt::Pass { .. } => Some(()),
    }
}

fn lower_if_chain(
    fb: &mut FuncBuilder,
    ctx: &mut LowerCtx,
    cond: &Expr,
    then_block: &Block,
    elifs: &[(Expr, Block)],
    else_block: Option<&Block>,
) {
    // Flatten the if/elif chain into a sequence of two-way branches.
    let exit = fb.new_block();

    let cv = lower_expr(fb, ctx, cond);
    let then_b = fb.new_block();
    let else_b = fb.new_block();
    fb.terminate(Terminator::CondBranch { cond: cv, t: then_b, f: else_b });

    fb.switch_to(then_b);
    lower_block(fb, ctx, then_block);
    fb.terminate(Terminator::Branch { target: exit });

    fb.switch_to(else_b);
    // Chain elifs recursively.
    for (i, (ec, eb)) in elifs.iter().enumerate() {
        let ecv = lower_expr(fb, ctx, ec);
        let et = fb.new_block();
        let ef = fb.new_block();
        fb.terminate(Terminator::CondBranch { cond: ecv, t: et, f: ef });
        fb.switch_to(et);
        lower_block(fb, ctx, eb);
        fb.terminate(Terminator::Branch { target: exit });
        fb.switch_to(ef);
        let _ = i;
    }
    if let Some(eb) = else_block {
        lower_block(fb, ctx, eb);
    }
    fb.terminate(Terminator::Branch { target: exit });

    fb.switch_to(exit);
}

fn lower_while(fb: &mut FuncBuilder, ctx: &mut LowerCtx, cond: &Expr, body: &Block) {
    let header = fb.new_block();
    let body_b = fb.new_block();
    let exit = fb.new_block();

    fb.terminate(Terminator::Branch { target: header });
    fb.switch_to(header);
    let cv = lower_expr(fb, ctx, cond);
    fb.terminate(Terminator::CondBranch { cond: cv, t: body_b, f: exit });

    fb.switch_to(body_b);
    fb.loop_stack.push((header, exit));
    lower_block(fb, ctx, body);
    fb.loop_stack.pop();
    fb.terminate(Terminator::Branch { target: header });

    fb.switch_to(exit);
}

// ─────────────────────────────────────────────────────────────────────────
//  M16: match / case lowering
// ─────────────────────────────────────────────────────────────────────────
//
// Strategy: evaluate the scrutinee exactly once into a fresh local slot, then
// emit each arm as an if-elif test that reads from that slot.
//
// * Pattern::Wildcard / Pattern::Identifier — unconditional branch into the
//   arm body. Identifier additionally binds the scrutinee to a new local.
// * Pattern::Constructor { ty, fields } — emit `IsInstance` against the
//   class type-table id; on match, bind each `fields[i]` (which is itself
//   a Pattern, but for v0.1 we only support Identifier sub-patterns) to the
//   value at the class's `fields[i].offset`.
// * Pattern::Tuple(elems) — accept unconditionally (the static typechecker
//   already verified arity and element types). Each element pattern is bound
//   the same way as Constructor fields, at offset `8 * i`.
// * Pattern::Literal — equality test via IEq / FEq / StrEq depending on the
//   scrutinee's primitive kind.
//
// After every arm a Branch(exit) is emitted on the success path. The
// fall-through after the last arm also branches to exit, so unmatched
// scrutinees simply fall out of the construct (Python semantics: no
// `MatchError` exception in v0.1; exhaustiveness is enforced via spec
// §6.5 in the typechecker as a warning).
fn lower_match(
    fb: &mut FuncBuilder,
    ctx: &mut LowerCtx,
    scrutinee: &Expr,
    arms: &[ast::MatchArm],
) {
    // Evaluate the scrutinee exactly once and stash in a hidden local. Each
    // arm test re-reads it so the source expression isn't re-evaluated.
    let scrut_v = lower_expr(fb, ctx, scrutinee);
    let scrut_ty = find_value_ty(fb, scrut_v).unwrap_or(Ty::Primitive(PrimTy::Unit));
    // Reserve a slot name that can't clash with a user identifier.
    let slot_name = format!("__match_scrut_{}", fb.fresh_value());
    let scrut_slot = fb.alloc_slot(&slot_name, scrut_ty.clone());
    fb.emit_write_local(scrut_slot, scrut_v);

    let exit = fb.new_block();

    for arm in arms {
        let scrut_read = fb.emit_read_local(scrut_slot);
        match &arm.pattern {
            ast::Pattern::Wildcard(_) => {
                lower_block(fb, ctx, &arm.body);
                fb.terminate(Terminator::Branch { target: exit });
                // Anything after a wildcard is unreachable in source; the
                // typechecker / parser does not currently forbid it, so be
                // tolerant and continue lowering subsequent arms into a
                // fresh dead block.
                let dead = fb.new_block();
                fb.switch_to(dead);
            }
            ast::Pattern::Identifier(name, _) => {
                // Bind the scrutinee to `name` and unconditionally enter
                // the arm body.
                let slot = fb.alloc_slot(name, scrut_ty.clone());
                fb.emit_write_local(slot, scrut_read);
                lower_block(fb, ctx, &arm.body);
                fb.terminate(Terminator::Branch { target: exit });
                let dead = fb.new_block();
                fb.switch_to(dead);
            }
            ast::Pattern::Constructor { ty, fields, .. } => {
                let cid_opt = ctor_pattern_class_id(ctx, ty);
                let arm_b = fb.new_block();
                let next_b = fb.new_block();
                if let Some(cid) = cid_opt {
                    let tid = ctx.class_type_id.get(&cid.0).copied().unwrap_or(cid.0);
                    let cond = fb.push_value(
                        Ty::Primitive(PrimTy::Bool),
                        ValueKind::Op {
                            op: IROp::IsInstance { class_id: tid },
                            args: vec![scrut_read],
                        },
                    );
                    fb.terminate(Terminator::CondBranch {
                        cond,
                        t: arm_b,
                        f: next_b,
                    });
                    fb.switch_to(arm_b);
                    // Bind each Identifier sub-pattern to the matching field.
                    if let Some(layout) = ctx.class_layouts.get(&cid).cloned() {
                        let scrut_in_arm = fb.emit_read_local(scrut_slot);
                        for (i, sub) in fields.iter().enumerate() {
                            if let ast::Pattern::Identifier(fname, _) = sub {
                                if let Some(finfo) = layout.fields.get(i) {
                                    let v = fb.push_value(
                                        finfo.ty.clone(),
                                        ValueKind::Op {
                                            op: IROp::Load { offset: finfo.offset },
                                            args: vec![scrut_in_arm],
                                        },
                                    );
                                    let slot = fb.alloc_slot(fname, finfo.ty.clone());
                                    fb.emit_write_local(slot, v);
                                }
                            }
                            // Wildcards and nested constructor patterns are
                            // v0.2 work; v0.1 only supports flat Identifier
                            // sub-patterns and Wildcards (no binding).
                        }
                    }
                    lower_block(fb, ctx, &arm.body);
                    fb.terminate(Terminator::Branch { target: exit });
                    fb.switch_to(next_b);
                } else {
                    // Couldn't resolve class; arm becomes unreachable.
                    fb.terminate(Terminator::Branch { target: next_b });
                    fb.switch_to(next_b);
                }
            }
            ast::Pattern::Tuple(elems, _) => {
                // Tuples are statically-shaped, so we accept unconditionally
                // and just bind each Identifier sub-pattern.
                if let Ty::Tuple(elem_tys) = &scrut_ty {
                    for (i, sub) in elems.iter().enumerate() {
                        if let ast::Pattern::Identifier(name, _) = sub {
                            let elem_ty = elem_tys
                                .get(i)
                                .cloned()
                                .unwrap_or(Ty::Primitive(PrimTy::Unit));
                            let v = fb.push_value(
                                elem_ty.clone(),
                                ValueKind::Op {
                                    op: IROp::Load { offset: (i as u32) * 8 },
                                    args: vec![scrut_read],
                                },
                            );
                            let slot = fb.alloc_slot(name, elem_ty);
                            fb.emit_write_local(slot, v);
                        }
                    }
                }
                lower_block(fb, ctx, &arm.body);
                fb.terminate(Terminator::Branch { target: exit });
                let dead = fb.new_block();
                fb.switch_to(dead);
            }
            ast::Pattern::Literal(lit, _) => {
                // Equality test against the literal. Currently supports
                // ints / strs / bools / chars / floats. Anything weirder
                // falls through (no match).
                let lit_v = lower_literal_for_match(fb, lit, &scrut_ty);
                let cmp_op = match &scrut_ty {
                    Ty::Primitive(PrimTy::F32) | Ty::Primitive(PrimTy::F64) => IROp::FEq,
                    Ty::Primitive(PrimTy::Str) => IROp::StrEq,
                    _ => IROp::IEq,
                };
                let cond = fb.push_value(
                    Ty::Primitive(PrimTy::Bool),
                    ValueKind::Op {
                        op: cmp_op,
                        args: vec![scrut_read, lit_v],
                    },
                );
                let arm_b = fb.new_block();
                let next_b = fb.new_block();
                fb.terminate(Terminator::CondBranch {
                    cond,
                    t: arm_b,
                    f: next_b,
                });
                fb.switch_to(arm_b);
                lower_block(fb, ctx, &arm.body);
                fb.terminate(Terminator::Branch { target: exit });
                fb.switch_to(next_b);
            }
        }
    }

    // Fall-through (no match): branch to exit.
    fb.terminate(Terminator::Branch { target: exit });
    fb.switch_to(exit);
}

fn ctor_pattern_class_id(ctx: &LowerCtx, ty: &ast::Type) -> Option<ClassId> {
    // Resolve the AST type to a Ty via the resolver's ast_type_to_ty map,
    // then read off the class id.
    let key = ast_type_span(ty);
    let resolved = ctx
        .typed
        .resolved
        .ast_type_to_ty
        .get(&(key.start, key.end))
        .cloned();
    match resolved {
        Some(Ty::Class(cid)) => Some(cid),
        _ => {
            // Fallback: try to resolve by name lookup if the AST is a bare
            // ident. Constructor patterns with dotted names (modules) are
            // not supported in v0.1.
            if let ast::Type::Named { name, .. } = ty {
                if let Some(sid) = ctx
                    .typed
                    .resolved
                    .symbols
                    .lookup(ctx.typed.resolved.module_scope, name)
                {
                    return ctx.typed.resolved.symbols.get(sid).class_id;
                }
            }
            None
        }
    }
}

fn lower_literal_for_match(fb: &mut FuncBuilder, lit: &Literal, expected: &Ty) -> ValueId {
    match lit {
        Literal::Int { value, .. } => match expected {
            Ty::Primitive(PrimTy::I64) | Ty::Primitive(PrimTy::U64) => fb.push_value(
                expected.clone(),
                ValueKind::Const(IRConst::I64(*value as i64)),
            ),
            _ => fb.push_value(
                Ty::Primitive(PrimTy::I32),
                ValueKind::Const(IRConst::I32(*value as i32)),
            ),
        },
        Literal::Float { value, .. } => fb.push_value(
            Ty::Primitive(PrimTy::F64),
            ValueKind::Const(IRConst::F64(*value)),
        ),
        Literal::Str(s) => fb.push_value(
            Ty::Primitive(PrimTy::Str),
            ValueKind::Const(IRConst::Str(s.clone())),
        ),
        Literal::Char(c) => fb.push_value(
            Ty::Primitive(PrimTy::Char),
            ValueKind::Const(IRConst::Char(*c)),
        ),
        Literal::Bool(b) => fb.push_value(
            Ty::Primitive(PrimTy::Bool),
            ValueKind::Const(IRConst::Bool(*b)),
        ),
        Literal::None => fb.push_value(
            Ty::Primitive(PrimTy::Null),
            ValueKind::Const(IRConst::None),
        ),
        Literal::Bytes(_) => fb.push_value(
            Ty::Primitive(PrimTy::Unit),
            ValueKind::Const(IRConst::None),
        ),
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  M15: try / except / finally / raise
// ─────────────────────────────────────────────────────────────────────────

/// Lower `try: B except T1 as e1: H1 except T2: H2 finally: F` to:
///
/// ```text
///   try_body:
///     IROp::TryEnter { arms=[(T1, h1, e1_slot), (T2, h2, none)], finally=F }
///     <B>
///     IROp::TryLeave
///     Branch <post_body>             (post_body = finally if present, else after)
///
///   h1:                              (entered via runtime handler-frame dispatch;
///     <H1>                            no static predecessor — exception edges
///     Branch <post_body>              are managed by the VM)
///   h2:
///     <H2>
///     Branch <post_body>
///
///   F:  (only if there's a finally clause)
///     <F>
///     IROp::EndFinally               (re-raises if pending exception stashed)
///     Branch <after>
///
///   after:                           — successor of normal completion paths.
/// ```
///
/// The handler arms are NOT reachable via static CFG edges — they're entered
/// only via the VM's handler-frame propagation in `Interpreter::run_until`.
/// Codegen patches the `arms[i].handler_block` and `finally_block` to byte
/// offsets inside the EnterTry instruction operand.
fn lower_try(
    fb: &mut FuncBuilder,
    ctx: &mut LowerCtx,
    body: &Block,
    handlers: &[ExceptHandler],
    finally_block: Option<&Block>,
) {
    let after = fb.new_block();
    let finally_b: Option<BlockId> = finally_block.as_ref().map(|_| fb.new_block());

    // Allocate handler blocks AND their bind slots up front, so the TryEnter
    // operand can reference them.
    let mut handler_blocks: Vec<BlockId> = Vec::with_capacity(handlers.len());
    let mut bind_slots: Vec<u32> = Vec::with_capacity(handlers.len());
    let mut filter_idxs: Vec<u32> = Vec::with_capacity(handlers.len());
    for h in handlers {
        handler_blocks.push(fb.new_block());
        let filter_name = exception_filter_name(&h.exc_ty);
        let idx = ctx.intern(&filter_name);
        filter_idxs.push(idx);
        if let Some(name) = &h.binding {
            // Bind slot type: the caught exception class (best-effort —
            // pulled from the AST type via the resolver's type lookup).
            let slot_ty = exception_filter_ty(ctx, &h.exc_ty);
            let slot = fb.alloc_slot(name, slot_ty);
            bind_slots.push(slot as u32);
        } else {
            bind_slots.push(u32::MAX);
        }
    }

    let arms: Vec<TryHandlerArm> = handlers
        .iter()
        .enumerate()
        .map(|(i, _)| TryHandlerArm {
            filter_str_idx: filter_idxs[i],
            handler_block: handler_blocks[i],
            bind_slot: bind_slots[i],
        })
        .collect();

    // Emit the TryEnter as the first instruction of the current block.
    fb.push_value(
        Ty::Primitive(PrimTy::Unit),
        ValueKind::Op {
            op: IROp::TryEnter { arms, finally_block: finally_b },
            args: vec![],
        },
    );

    // Body.
    lower_block(fb, ctx, body);
    // Normal-completion path: pop the handler frame, then branch to finally
    // (if any) else `after`.
    fb.push_value(
        Ty::Primitive(PrimTy::Unit),
        ValueKind::Op { op: IROp::TryLeave, args: vec![] },
    );
    let post_body_target = finally_b.unwrap_or(after);
    fb.terminate(Terminator::Branch { target: post_body_target });

    // Handler arms — each starts in its own block, entered only via the VM's
    // exception dispatch. After running, branch to finally (if any) else after.
    for (i, h) in handlers.iter().enumerate() {
        fb.switch_to(handler_blocks[i]);
        lower_block(fb, ctx, &h.body);
        fb.terminate(Terminator::Branch { target: post_body_target });
    }

    // Finally block (if present).
    if let (Some(fb_id), Some(fin)) = (finally_b, finally_block) {
        fb.switch_to(fb_id);
        lower_block(fb, ctx, fin);
        fb.push_value(
            Ty::Primitive(PrimTy::Unit),
            ValueKind::Op { op: IROp::EndFinally, args: vec![] },
        );
        fb.terminate(Terminator::Branch { target: after });
    }

    fb.switch_to(after);
}

/// Lower `raise IOError("msg")` to:
///   v = Alloc(IOError_type_id)
///   Store(0)  v, "IOError"          (type_name field)
///   Store(8)  v, "msg"               (message field)
///   Throw v
///
/// For unrecognised shapes (e.g. raising a bare value), fall back to lowering
/// the expression and throwing whatever it produced. The runtime will treat
/// the resulting value as if it were an exception heap object — useful for
/// re-raise scenarios once those land.
fn lower_raise(fb: &mut FuncBuilder, ctx: &mut LowerCtx, exc: &Expr) {
    // Recognise `<ExceptionName>("message")`.
    if let Expr::Call { callee, args, .. } = exc {
        if let Expr::Ident { name, .. } = callee.as_ref() {
            if crate::typecheck::is_builtin_exception_name(name) && args.len() == 1 {
                // Look up the class id.
                let scope = ctx.typed.resolved.module_scope;
                let sid = ctx.typed.resolved.symbols.lookup(scope, name);
                let cid = sid.and_then(|sid| ctx.typed.resolved.symbols.get(sid).class_id);
                if let Some(cid) = cid {
                    let tid = ctx.class_type_id.get(&cid.0).copied().unwrap_or(cid.0);
                    let alloc = fb.push_value(
                        Ty::Class(cid),
                        ValueKind::Op { op: IROp::Alloc { class_id: tid }, args: vec![] },
                    );
                    // type_name field (offset 0).
                    let tname_v = fb.push_value(
                        Ty::Primitive(PrimTy::Str),
                        ValueKind::Const(IRConst::Str(name.clone())),
                    );
                    fb.push_value(
                        Ty::Primitive(PrimTy::Unit),
                        ValueKind::Op {
                            op: IROp::Store { offset: 0 },
                            args: vec![alloc, tname_v],
                        },
                    );
                    // message field (offset 8).
                    let msg_v = lower_expr(fb, ctx, &args[0].value);
                    fb.push_value(
                        Ty::Primitive(PrimTy::Unit),
                        ValueKind::Op {
                            op: IROp::Store { offset: 8 },
                            args: vec![alloc, msg_v],
                        },
                    );
                    fb.terminate(Terminator::Throw { exc: alloc });
                    let nb = fb.new_block();
                    fb.switch_to(nb);
                    return;
                }
            }
        }
    }
    // Fallback path — lower whatever expression was raised and throw it.
    let v = lower_expr(fb, ctx, exc);
    fb.terminate(Terminator::Throw { exc: v });
    let nb = fb.new_block();
    fb.switch_to(nb);
}

/// Extract the filter name from an `except T as e:` clause's AST type.
/// `Type::Named { name, .. }` → `name.clone()`; everything else maps to
/// "Exception" (catch-all) — defensive.
fn exception_filter_name(ty: &ast::Type) -> String {
    if let ast::Type::Named { name, .. } = ty {
        name.clone()
    } else {
        "Exception".into()
    }
}

/// Resolve the static type used for the exception-binding slot. Pulled from
/// the resolver's `ast_type_to_ty` so the slot's type matches what the
/// typechecker recorded.
fn exception_filter_ty(ctx: &LowerCtx, ty: &ast::Type) -> Ty {
    let key = (ast_type_span(ty).start, ast_type_span(ty).end);
    ctx.typed
        .resolved
        .ast_type_to_ty
        .get(&key)
        .cloned()
        .unwrap_or(Ty::Primitive(PrimTy::Unit))
}

// ─────────────────────────────────────────────────────────────────────────
//  Lvalues
// ─────────────────────────────────────────────────────────────────────────

fn lower_lvalue_load(fb: &mut FuncBuilder, ctx: &mut LowerCtx, lv: &Lvalue) -> ValueId {
    match lv {
        Lvalue::Ident { name, .. } => {
            if let Some(slot) = fb.slot_for(name) {
                return fb.emit_read_local(slot);
            }
            if let Some((irc, ty)) = ctx.module_consts.get(name).cloned() {
                return fb.push_value(ty, ValueKind::Const(irc));
            }
            fb.push_value(Ty::Primitive(PrimTy::Unit), ValueKind::Const(IRConst::None))
        }
        Lvalue::Attr { obj, name, span } => {
            let recv = lower_expr(fb, ctx, obj);
            let ty = ctx.expr_ty(*span);
            // Look up offset by inspecting receiver's class layout.
            let obj_ty = ctx.expr_ty(expr_span(obj));
            let offset = field_offset(ctx.class_layouts, &obj_ty, name).unwrap_or(0);
            fb.push_value(ty, ValueKind::Op { op: IROp::Load { offset }, args: vec![recv] })
        }
        Lvalue::Index { obj, indices, span } => {
            // M7: dispatch on receiver — Dict[K,V] / str use natives, only
            // List[T] uses ArrayGet.
            let recv_ty = ctx.expr_ty(expr_span(obj));
            let arr = lower_expr(fb, ctx, obj);
            let idx = if let Some(i) = indices.first() {
                lower_expr(fb, ctx, i)
            } else {
                fb.push_value(Ty::Primitive(PrimTy::I64), ValueKind::Const(IRConst::I64(0)))
            };
            let ty = ctx.expr_ty(*span);
            match &recv_ty {
                Ty::Generic { base: TypeCtor::Dict, .. } => fb.push_value(
                    ty,
                    ValueKind::Op {
                        op: IROp::NativeCall { native_id: NativeFn::DictGet as u32 },
                        args: vec![arr, idx],
                    },
                ),
                Ty::Primitive(PrimTy::Str) => fb.push_value(
                    ty,
                    ValueKind::Op {
                        op: IROp::NativeCall { native_id: NativeFn::StrCharAt as u32 },
                        args: vec![arr, idx],
                    },
                ),
                _ => fb.push_value(
                    ty,
                    ValueKind::Op { op: IROp::ArrayGet, args: vec![arr, idx] },
                ),
            }
        }
    }
}

fn lower_lvalue_store(fb: &mut FuncBuilder, ctx: &mut LowerCtx, lv: &Lvalue, v: ValueId) {
    match lv {
        Lvalue::Ident { name, .. } => {
            // Write to the local's stable slot. If no slot exists yet
            // (assignment without a prior `let`), create one with the
            // value's static type.
            let slot = if let Some(s) = fb.slot_for(name) {
                s
            } else {
                let ty = find_value_ty(fb, v).unwrap_or(Ty::Primitive(PrimTy::Unit));
                fb.alloc_slot(name, ty)
            };
            fb.emit_write_local(slot, v);
        }
        Lvalue::Attr { obj, name, .. } => {
            let recv = lower_expr(fb, ctx, obj);
            let obj_ty = ctx.expr_ty(expr_span(obj));
            let offset = field_offset(ctx.class_layouts, &obj_ty, name).unwrap_or(0);
            fb.push_value(
                Ty::Primitive(PrimTy::Unit),
                ValueKind::Op { op: IROp::Store { offset }, args: vec![recv, v] },
            );
        }
        Lvalue::Index { obj, indices, .. } => {
            // M7: dispatch on receiver — Dict uses NativeFn::DictSet.
            let recv_ty = ctx.expr_ty(expr_span(obj));
            let arr = lower_expr(fb, ctx, obj);
            let idx = if let Some(i) = indices.first() {
                lower_expr(fb, ctx, i)
            } else {
                fb.push_value(Ty::Primitive(PrimTy::I64), ValueKind::Const(IRConst::I64(0)))
            };
            match &recv_ty {
                Ty::Generic { base: TypeCtor::Dict, .. } => {
                    fb.push_value(
                        Ty::Primitive(PrimTy::Unit),
                        ValueKind::Op {
                            op: IROp::NativeCall { native_id: NativeFn::DictSet as u32 },
                            args: vec![arr, idx, v],
                        },
                    );
                }
                _ => {
                    fb.push_value(
                        Ty::Primitive(PrimTy::Unit),
                        ValueKind::Op { op: IROp::ArraySet, args: vec![arr, idx, v] },
                    );
                }
            }
        }
    }
}

fn field_offset(
    class_layouts: &HashMap<ClassId, ClassLayout>,
    obj_ty: &Ty,
    name: &str,
) -> Option<u32> {
    let cid = match obj_ty {
        Ty::Class(c) => Some(*c),
        // M31: parameterised generic-class receiver. Field offsets are the
        // *same* as on the abstract layout (every Ty::Var field occupies
        // one 8-byte slot; instantiations don't reshuffle).
        Ty::Generic { base: TypeCtor::Class(c), .. } => Some(*c),
        _ => None,
    };
    if let Some(cid) = cid {
        if let Some(layout) = class_layouts.get(&cid) {
            if let Some(f) = layout.fields.iter().find(|f| f.name == name) {
                return Some(f.offset);
            }
        }
    }
    None
}

fn expr_span(e: &Expr) -> Span {
    match e {
        Expr::Literal { span, .. }
        | Expr::Ident { span, .. }
        | Expr::Tuple { span, .. }
        | Expr::List { span, .. }
        | Expr::Dict { span, .. }
        | Expr::Set { span, .. }
        | Expr::Unary { span, .. }
        | Expr::Binary { span, .. }
        | Expr::Call { span, .. }
        | Expr::MethodCall { span, .. }
        | Expr::Attr { span, .. }
        | Expr::Index { span, .. }
        | Expr::NullCoalesce { span, .. }
        | Expr::Ternary { span, .. }
        | Expr::Lambda { span, .. }
        | Expr::Cast { span, .. } => *span,
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Expressions
// ─────────────────────────────────────────────────────────────────────────

fn lower_expr(fb: &mut FuncBuilder, ctx: &mut LowerCtx, e: &Expr) -> ValueId {
    match e {
        Expr::Literal { lit, span } => lower_literal(fb, ctx, lit, *span),
        Expr::Ident { name, span } => {
            if let Some(slot) = fb.slot_for(name) {
                return fb.emit_read_local(slot);
            }
            if let Some((irc, ty)) = ctx.module_consts.get(name).cloned() {
                return fb.push_value(ty, ValueKind::Const(irc));
            }
            // M19: `from sys import argv` — the symbol resolves to a stdlib
            // const. Read-as-value emits a 0-arg CallNative; the function
            // form `exit(0)` is handled in lower_call below.
            let scope = ctx.typed.resolved.module_scope;
            if let Some(sid) = ctx.typed.resolved.symbols.lookup(scope, name) {
                if let Some(item) = ctx.typed.resolved.import_item.get(&sid).cloned() {
                    if matches!(item.kind, crate::resolver::StdlibItemKind::Const) {
                        let ty = ctx.expr_ty(*span);
                        return fb.push_value(
                            ty,
                            ValueKind::Op {
                                op: IROp::NativeCall { native_id: item.native_id },
                                args: vec![],
                            },
                        );
                    }
                }
            }
            // Unknown ident (likely a prelude/builtin/class) — placeholder.
            let ty = ctx.expr_ty(*span);
            fb.push_value(ty, ValueKind::Const(IRConst::None))
        }
        Expr::Tuple { elems, span } => {
            // M14: tuples are heap objects whose layout mirrors a synthetic
            // class (one 8-byte slot per element).  Allocate via IROp::Alloc
            // using the type-table id registered in `register_tuple_types`,
            // then store each element at offset `8 * i`.
            let ty = ctx.expr_ty(*span);
            let elem_vs: Vec<ValueId> = elems.iter().map(|e| lower_expr(fb, ctx, e)).collect();
            let key = display_ty(&ty);
            let tid = ctx.tuple_type_id.get(&key).copied().unwrap_or(u32::MAX);
            let alloc = fb.push_value(
                ty.clone(),
                ValueKind::Op { op: IROp::Alloc { class_id: tid }, args: vec![] },
            );
            for (i, ev) in elem_vs.into_iter().enumerate() {
                fb.push_value(
                    Ty::Primitive(PrimTy::Unit),
                    ValueKind::Op {
                        op: IROp::Store { offset: (i as u32) * 8 },
                        args: vec![alloc, ev],
                    },
                );
            }
            alloc
        }
        Expr::List { elems, span } => {
            let ty = ctx.expr_ty(*span);
            // Lower each element first; SSA values for the elements need to
            // exist before we can push them onto the freshly-allocated list.
            let elem_vs: Vec<ValueId> =
                elems.iter().map(|e| lower_expr(fb, ctx, e)).collect();
            let list = fb.push_value(
                ty.clone(),
                ValueKind::Op { op: IROp::ArrayNew, args: vec![] },
            );
            for ev in elem_vs {
                fb.push_value(
                    Ty::Primitive(PrimTy::Unit),
                    ValueKind::Op { op: IROp::ListPush, args: vec![list, ev] },
                );
            }
            list
        }
        Expr::Dict { entries, span } => {
            // M7: lower `{}` (and `{k: v, ...}`) to a fresh DictRepr via
            // NativeFn::DictNew, then populate with NativeFn::DictSet calls.
            // Without this the dict was lowered to a null pointer and the
            // first `counts[w] = 1` in wordcount.spy trapped on null.
            let ty = ctx.expr_ty(*span);
            let dict = fb.push_value(
                ty.clone(),
                ValueKind::Op {
                    op: IROp::NativeCall { native_id: NativeFn::DictNew as u32 },
                    args: vec![],
                },
            );
            for (k, v) in entries {
                let kv = lower_expr(fb, ctx, k);
                let vv = lower_expr(fb, ctx, v);
                fb.push_value(
                    Ty::Primitive(PrimTy::Unit),
                    ValueKind::Op {
                        op: IROp::NativeCall { native_id: NativeFn::DictSet as u32 },
                        args: vec![dict, kv, vv],
                    },
                );
            }
            dict
        }
        Expr::Set { elems, span } => {
            for elt in elems {
                let _ = lower_expr(fb, ctx, elt);
            }
            let ty = ctx.expr_ty(*span);
            fb.push_value(ty, ValueKind::Const(IRConst::None))
        }
        Expr::Unary { op, operand, span } => {
            let v = lower_expr(fb, ctx, operand);
            let ty = ctx.expr_ty(*span);
            // real-world: nullable-narrowing audit — `ty` may be
            // `Nullable(F32)`/`Nullable(F64)` because the IR slot holds
            // the declared type even after the typechecker narrowed
            // inside `if x is not none:`. Match against the inner type
            // so `-x` for `x: f64?` correctly picks FNeg, not INeg.
            let dispatch_ty = match &ty {
                Ty::Nullable(inner) => (**inner).clone(),
                other => other.clone(),
            };
            let irop = match op {
                UnaryOp::Neg => match dispatch_ty {
                    Ty::Primitive(p) if p.is_float() => IROp::FNeg,
                    _ => IROp::INeg,
                },
                UnaryOp::Pos => IROp::Copy,
                UnaryOp::BitNot => IROp::INot,
                UnaryOp::Not => IROp::BoolNot,
            };
            fb.push_value(ty, ValueKind::Op { op: irop, args: vec![v] })
        }
        Expr::Binary { op, lhs, rhs, span } => {
            let ty = ctx.expr_ty(*span);
            // M13 (BUG-035): `and` / `or` must short-circuit. The right
            // operand must NOT be evaluated when the left already decides
            // the result, otherwise the standard guard idiom
            //     b > 0 and xs[b - 1] > 0
            // traps. Inspect the AST node BEFORE eagerly lowering both
            // operands and dispatch to a basic-block-splitting helper for
            // these two operators. Every other binop preserves the prior
            // eager-lower-then-emit path.
            if matches!(op, AstBinOp::And | AstBinOp::Or) {
                return lower_short_circuit(fb, ctx, *op, lhs, rhs, ty);
            }
            // M14 tuples: `t == u` / `t != u` lowers to element-wise compares.
            // We must inspect the operand type (not the result type) — the
            // result is always bool.  Fall back to the generic path for
            // non-tuple operands.
            if matches!(op, AstBinOp::Eq | AstBinOp::Ne) {
                let lhs_ty = ctx.expr_ty(expr_span(lhs));
                if let Ty::Tuple(elem_tys) = lhs_ty.clone() {
                    return lower_tuple_eq(fb, ctx, *op, lhs, rhs, &elem_tys);
                }
            }
            let l = lower_expr(fb, ctx, lhs);
            let r = lower_expr(fb, ctx, rhs);
            emit_binop(fb, *op, l, r, ty)
        }
        Expr::Call { callee, args, span } => lower_call(fb, ctx, callee, args, *span),
        Expr::MethodCall { receiver, method, args, span } => {
            lower_method_call(fb, ctx, receiver, method, args, *span)
        }
        Expr::Attr { obj, name, span } => {
            // M19: `sys.argv` / `sys.platform` — the obj is an Ident
            // whose symbol is a BuiltinModule registered via
            // `import sys` (or aliased via `import sys as s`).  Const
            // attrs lower to a 0-arg CallNative; function attrs are
            // handled in lower_call where the *Call* sees the Attr
            // callee.  This branch is for *read-as-value* — e.g.
            // `s := sys.platform`.
            if let Expr::Ident { name: mname, .. } = obj.as_ref() {
                let scope = ctx.typed.resolved.module_scope;
                if let Some(sid) = ctx.typed.resolved.symbols.lookup(scope, mname) {
                    if matches!(
                        ctx.typed.resolved.symbols.get(sid).kind,
                        SymbolKind::BuiltinModule
                    ) {
                        // Recover the real module name (may differ from
                        // the alias the user typed: `import sys as s`).
                        let mod_name = ctx.typed.resolved.module_alias
                            .get(&sid)
                            .cloned()
                            .unwrap_or_else(|| mname.clone());
                        if let Some(m) = ctx.typed.resolved.stdlib_modules.get(&mod_name) {
                            if let Some(item) = m.find(name) {
                                if matches!(item.kind, crate::resolver::StdlibItemKind::Const) {
                                    let ty = ctx.expr_ty(*span);
                                    return fb.push_value(
                                        ty,
                                        ValueKind::Op {
                                            op: IROp::NativeCall { native_id: item.native_id },
                                            args: vec![],
                                        },
                                    );
                                }
                                // Function attr — emitting it as a value
                                // would need a closure; v0.2 only
                                // supports the call form. Fall through
                                // to placeholder; the typechecker
                                // already accepted this so this only
                                // bites if user code passes
                                // `sys.exit` as a value, which we
                                // don't yet promise to support.
                            }
                        }
                    }
                }
            }
            let recv = lower_expr(fb, ctx, obj);
            let obj_ty = ctx.expr_ty(expr_span(obj));
            // M14 tuples: `t.N` — numeric attr on Ty::Tuple maps directly
            // to a Load at the synthetic 8-byte-per-elem offset.
            let offset = if let Ty::Tuple(_) = &obj_ty {
                name.parse::<u32>().map(|i| i * 8).unwrap_or(0)
            } else {
                field_offset(ctx.class_layouts, &obj_ty, name).unwrap_or(0)
            };
            let ty = ctx.expr_ty(*span);
            fb.push_value(ty, ValueKind::Op { op: IROp::Load { offset }, args: vec![recv] })
        }
        Expr::Index { obj, indices, span } => {
            // M7: dispatch on receiver type — ArrayGet is List-only, Dict
            // and str need their own natives.
            let recv_ty = ctx.expr_ty(expr_span(obj));
            let arr = lower_expr(fb, ctx, obj);
            let idx = if let Some(i) = indices.first() {
                lower_expr(fb, ctx, i)
            } else {
                fb.push_value(Ty::Primitive(PrimTy::I64), ValueKind::Const(IRConst::I64(0)))
            };
            let ty = ctx.expr_ty(*span);
            match &recv_ty {
                Ty::Generic { base: TypeCtor::Dict, .. } => fb.push_value(
                    ty,
                    ValueKind::Op {
                        op: IROp::NativeCall { native_id: NativeFn::DictGet as u32 },
                        args: vec![arr, idx],
                    },
                ),
                Ty::Primitive(PrimTy::Str) => fb.push_value(
                    ty,
                    ValueKind::Op {
                        op: IROp::NativeCall { native_id: NativeFn::StrCharAt as u32 },
                        args: vec![arr, idx],
                    },
                ),
                _ => fb.push_value(
                    ty,
                    ValueKind::Op { op: IROp::ArrayGet, args: vec![arr, idx] },
                ),
            }
        }
        Expr::NullCoalesce { lhs, rhs, span } => {
            // M21 fix (BUG-037): the old lowering was a placeholder that
            // returned the rhs unconditionally — `x ?? fallback` always
            // produced `fallback` regardless of `x`. The correct lowering
            // is `if x is none: rhs else: x`, with rhs evaluated only when
            // x IS none (short-circuit). Mirrors the M13 short-circuit
            // pattern (slot pre-seed + CondBranch + phi via slot read).
            let ty = ctx.expr_ty(*span);
            let l = lower_expr(fb, ctx, lhs);

            // Pre-seed result slot with `l` (the "not-none" path value).
            let slot_name = format!("__nc_{}", fb.slot_ty.len());
            let slot = fb.alloc_slot(&slot_name, ty.clone());
            fb.emit_write_local(slot, l);

            // Test if l is none.
            let none_val = fb.push_value(
                Ty::Primitive(PrimTy::Null),
                ValueKind::Const(IRConst::None),
            );
            let is_none = fb.push_value(
                Ty::Primitive(PrimTy::Bool),
                ValueKind::Op { op: IROp::RefEq, args: vec![l, none_val] },
            );

            let rhs_b = fb.new_block();
            let merge = fb.new_block();
            // If is_none → evaluate rhs; else → slot already has l, jump to merge.
            fb.terminate(Terminator::CondBranch { cond: is_none, t: rhs_b, f: merge });

            // Evaluate the rhs only in the "l was none" branch, then overwrite.
            fb.switch_to(rhs_b);
            let r = lower_expr(fb, ctx, rhs);
            fb.emit_write_local(slot, r);
            fb.terminate(Terminator::Branch { target: merge });

            fb.switch_to(merge);
            fb.emit_read_local(slot)
        }
        Expr::Ternary { cond, then_expr, else_expr, span } => {
            let c = lower_expr(fb, ctx, cond);
            let t = lower_expr(fb, ctx, then_expr);
            let f = lower_expr(fb, ctx, else_expr);
            let ty = ctx.expr_ty(*span);
            fb.push_value(ty, ValueKind::Op { op: IROp::Select, args: vec![c, t, f] })
        }
        Expr::Lambda { params, return_ty, body, span } => {
            lower_lambda(fb, ctx, params, return_ty, body, *span)
        }
        Expr::Cast { expr, span, .. } => {
            let v = lower_expr(fb, ctx, expr);
            let ty = ctx.expr_ty(*span);
            fb.push_value(ty, ValueKind::Op { op: IROp::Copy, args: vec![v] })
        }
    }
}

/// Lower an `Expr::Lambda` by lifting its body into a fresh `IRFunction`
/// whose parameter list is `[capture_0, ..., user_param_0, ...]`. At the
/// use site we emit a `ClosureNew` carrying the captured outer-scope
/// values as operand arguments.
fn lower_lambda(
    fb: &mut FuncBuilder,
    ctx: &mut LowerCtx,
    params: &[ast::Param],
    return_ty: &ast::Type,
    body: &Expr,
    span: Span,
) -> ValueId {
    // Find identifiers used inside the body that bind in an enclosing
    // function scope (i.e. they have a slot in the parent FuncBuilder
    // and are not shadowed by the lambda's own params).
    let param_names: std::collections::HashSet<&str> =
        params.iter().map(|p| p.name.as_str()).collect();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut captures: Vec<String> = Vec::new();
    collect_free_vars(body, &param_names, &mut seen, &mut captures);

    // Keep only the free vars that actually refer to a slot in the
    // enclosing FuncBuilder. Globals (functions, classes, module consts,
    // prelude names) are resolved by name at the use site and need no
    // capture.
    let mut capture_slots: Vec<(String, u16, Ty)> = Vec::new();
    for name in &captures {
        if let Some(slot) = fb.slot_for(name) {
            let ty = fb.slot_type(slot);
            capture_slots.push((name.clone(), slot, ty));
        }
    }

    // Emit reads in the parent function to materialise capture values
    // before the ClosureNew.
    let capture_vals: Vec<ValueId> = capture_slots
        .iter()
        .map(|(_, slot, _)| fb.emit_read_local(*slot))
        .collect();

    // Resolve the lambda's parameter and return types via the resolver's
    // ast→Ty map (best-effort; falls back to Unit).
    let mut lambda_param_tys: Vec<Ty> = Vec::new();
    // Captures come first.
    for (_, _, ty) in &capture_slots {
        lambda_param_tys.push(ty.clone());
    }
    let user_param_tys: Vec<Ty> = params
        .iter()
        .map(|p| {
            ctx.typed
                .resolved
                .ast_type_to_ty
                .get(&(ast_type_span(&p.ty).start, ast_type_span(&p.ty).end))
                .cloned()
                .unwrap_or(Ty::Primitive(PrimTy::Unit))
        })
        .collect();
    lambda_param_tys.extend(user_param_tys.clone());
    let ret_ty = ctx
        .typed
        .resolved
        .ast_type_to_ty
        .get(&(ast_type_span(return_ty).start, ast_type_span(return_ty).end))
        .cloned()
        .unwrap_or(Ty::Primitive(PrimTy::Unit));

    // Allocate the lifted FuncId.
    let lifted_id = FuncId(*ctx.next_fn_id);
    *ctx.next_fn_id += 1;
    let lifted_name = format!("__lambda_{}", lifted_id.0);

    // Build the lifted function's body. Each capture and each user param
    // becomes a slot at the head of the entry block; reads/writes inside
    // the body see the latest value through the slot model.
    let mut sub = FuncBuilder::new(lifted_id, &lifted_name);
    let entry = sub.new_block();
    sub.current = entry;
    let mut param_idx: u32 = 0;
    for (name, _, ty) in &capture_slots {
        let v = sub.push_value(ty.clone(), ValueKind::Param { idx: param_idx });
        let slot = sub.alloc_slot(name, ty.clone());
        sub.emit_write_local(slot, v);
        sub.params.push((name.clone(), ty.clone()));
        param_idx += 1;
    }
    for (p, ty) in params.iter().zip(user_param_tys.iter()) {
        let v = sub.push_value(ty.clone(), ValueKind::Param { idx: param_idx });
        let slot = sub.alloc_slot(&p.name, ty.clone());
        sub.emit_write_local(slot, v);
        sub.params.push((p.name.clone(), ty.clone()));
        param_idx += 1;
    }

    // Lower the body expression and Ret its value (lambda bodies are a
    // single expression per the AST).
    let bv = lower_expr(&mut sub, ctx, body);
    sub.terminate(Terminator::Ret { value: Some(bv) });
    // If body lowering opened a fall-through block, seal it.
    let cur_idx = sub.current.0 as usize;
    if matches!(sub.blocks[cur_idx].terminator, Terminator::Unreachable) {
        sub.blocks[cur_idx].terminator = Terminator::Ret { value: None };
    }

    let lifted_fn = IRFunction {
        id: lifted_id,
        name: lifted_name,
        params: lambda_param_tys,
        ret: ret_ty,
        blocks: sub.blocks,
    };
    ctx.lifted_functions.push(lifted_fn);

    // Emit the ClosureNew at the use site, carrying captures as args.
    let n_caps = capture_vals.len() as u32;
    let closure_ty = ctx.expr_ty(span);
    fb.push_value(
        closure_ty,
        ValueKind::Op {
            op: IROp::ClosureNew { fn_id: lifted_id, n_captures: n_caps },
            args: capture_vals,
        },
    )
}

/// Walk `e` collecting identifier names that are free w.r.t. `bound`
/// (the lambda's own params). Inserts each unique name into `captures`.
fn collect_free_vars(
    e: &Expr,
    bound: &std::collections::HashSet<&str>,
    seen: &mut std::collections::HashSet<String>,
    captures: &mut Vec<String>,
) {
    match e {
        Expr::Ident { name, .. } => {
            if !bound.contains(name.as_str()) && seen.insert(name.clone()) {
                captures.push(name.clone());
            }
        }
        Expr::Literal { .. } => {}
        Expr::Tuple { elems, .. } | Expr::List { elems, .. } | Expr::Set { elems, .. } => {
            for e in elems { collect_free_vars(e, bound, seen, captures); }
        }
        Expr::Dict { entries, .. } => {
            for (k, v) in entries {
                collect_free_vars(k, bound, seen, captures);
                collect_free_vars(v, bound, seen, captures);
            }
        }
        Expr::Unary { operand, .. } => collect_free_vars(operand, bound, seen, captures),
        Expr::Binary { lhs, rhs, .. }
        | Expr::NullCoalesce { lhs, rhs, .. } => {
            collect_free_vars(lhs, bound, seen, captures);
            collect_free_vars(rhs, bound, seen, captures);
        }
        Expr::Call { callee, args, .. } => {
            collect_free_vars(callee, bound, seen, captures);
            for a in args { collect_free_vars(&a.value, bound, seen, captures); }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_free_vars(receiver, bound, seen, captures);
            for a in args { collect_free_vars(&a.value, bound, seen, captures); }
        }
        Expr::Attr { obj, .. } => collect_free_vars(obj, bound, seen, captures),
        Expr::Index { obj, indices, .. } => {
            collect_free_vars(obj, bound, seen, captures);
            for i in indices { collect_free_vars(i, bound, seen, captures); }
        }
        Expr::Ternary { cond, then_expr, else_expr, .. } => {
            collect_free_vars(cond, bound, seen, captures);
            collect_free_vars(then_expr, bound, seen, captures);
            collect_free_vars(else_expr, bound, seen, captures);
        }
        Expr::Lambda { params, body, .. } => {
            // Inner lambda shadows its own params, plus the outer bound set.
            let mut inner_bound: std::collections::HashSet<&str> = bound.clone();
            for p in params { inner_bound.insert(p.name.as_str()); }
            collect_free_vars(body, &inner_bound, seen, captures);
        }
        Expr::Cast { expr, .. } => collect_free_vars(expr, bound, seen, captures),
    }
}

/// Fold a top-level `final` const initialiser to an [`IRConst`] when it's
/// a literal (the only kind currently allowed at module scope). Returns
/// `None` for non-literal initialisers; callers leave such consts
/// unfolded and references resolve to `None` at runtime.
fn literal_to_irconst(e: &Expr, ty: &Ty) -> Option<IRConst> {
    match e {
        Expr::Literal { lit, .. } => Some(match lit {
            Literal::Int { value, .. } => match ty {
                Ty::Primitive(PrimTy::I32) => IRConst::I32(*value as i32),
                Ty::Primitive(PrimTy::U32) => IRConst::U32(*value as u32),
                Ty::Primitive(PrimTy::I64) => IRConst::I64(*value as i64),
                Ty::Primitive(PrimTy::U64) => IRConst::U64(*value as u64),
                _ => IRConst::I64(*value as i64),
            },
            Literal::Float { value, .. } => match ty {
                Ty::Primitive(PrimTy::F32) => IRConst::F32(*value as f32),
                _ => IRConst::F64(*value),
            },
            Literal::Str(s) => IRConst::Str(s.clone()),
            Literal::Bool(b) => IRConst::Bool(*b),
            Literal::Char(c) => IRConst::Char(*c),
            Literal::None => IRConst::None,
            Literal::Bytes(_) => IRConst::None,
        }),
        // Unary minus applied to a literal — the parser does not fold
        // these, so handle one level of negation explicitly.
        Expr::Unary { op: UnaryOp::Neg, operand, .. } => {
            let inner = literal_to_irconst(operand, ty)?;
            match inner {
                IRConst::I32(v) => Some(IRConst::I32(v.wrapping_neg())),
                IRConst::I64(v) => Some(IRConst::I64(v.wrapping_neg())),
                IRConst::F32(v) => Some(IRConst::F32(-v)),
                IRConst::F64(v) => Some(IRConst::F64(-v)),
                _ => None,
            }
        }
        _ => None,
    }
}

fn lower_literal(
    fb: &mut FuncBuilder,
    ctx: &mut LowerCtx,
    lit: &Literal,
    span: Span,
) -> ValueId {
    let ty = ctx.expr_ty(span);
    let kind = match lit {
        Literal::Int { value, .. } => match &ty {
            Ty::Primitive(PrimTy::I32) => ValueKind::Const(IRConst::I32(*value as i32)),
            Ty::Primitive(PrimTy::I64) => ValueKind::Const(IRConst::I64(*value as i64)),
            Ty::Primitive(PrimTy::U32) => ValueKind::Const(IRConst::U32(*value as u32)),
            Ty::Primitive(PrimTy::U64) => ValueKind::Const(IRConst::U64(*value as u64)),
            _ => ValueKind::Const(IRConst::I64(*value as i64)),
        },
        Literal::Float { value, .. } => match &ty {
            Ty::Primitive(PrimTy::F32) => ValueKind::Const(IRConst::F32(*value as f32)),
            _ => ValueKind::Const(IRConst::F64(*value)),
        },
        Literal::Str(s) => {
            let _ = ctx.intern(s);
            ValueKind::Const(IRConst::Str(s.clone()))
        }
        Literal::Bytes(_) => ValueKind::Const(IRConst::None),
        Literal::Char(c) => ValueKind::Const(IRConst::Char(*c)),
        Literal::Bool(b) => ValueKind::Const(IRConst::Bool(*b)),
        Literal::None => ValueKind::Const(IRConst::None),
    };
    fb.push_value(ty, kind)
}

fn emit_binop(
    fb: &mut FuncBuilder,
    op: AstBinOp,
    l: ValueId,
    r: ValueId,
    ty: Ty,
) -> ValueId {
    // Look up operand type from one of the inputs to choose int vs float op.
    // real-world: csv_aggregate — unwrap Nullable() because the typechecker
    // narrows `prev: f64?` to `f64` inside an `else: prev + amount` branch
    // but the IR slot still carries the declared `f64?` type. Without this
    // unwrap, `lower_binop` saw `operand_ty = Nullable(f64)` and emitted an
    // integer IAdd on the raw bit pattern, silently returning garbage.
    let raw_operand_ty = find_value_ty(fb, l).unwrap_or(ty.clone());
    let operand_ty = match &raw_operand_ty {
        Ty::Nullable(inner) => (**inner).clone(),
        _ => raw_operand_ty,
    };
    let is_float = matches!(operand_ty, Ty::Primitive(p) if p.is_float());
    let is_str = matches!(operand_ty, Ty::Primitive(PrimTy::Str));
    let irop = match op {
        AstBinOp::Add => {
            if is_str {
                // String concat — emit a native call.
                return fb.push_value(
                    Ty::Primitive(PrimTy::Str),
                    ValueKind::Op {
                        op: IROp::NativeCall { native_id: NativeFn::StrConcat as u32 },
                        args: vec![l, r],
                    },
                );
            }
            if is_float { IROp::FAdd } else { IROp::IAdd }
        }
        AstBinOp::Sub => if is_float { IROp::FSub } else { IROp::ISub },
        AstBinOp::Mul => if is_float { IROp::FMul } else { IROp::IMul },
        AstBinOp::Div | AstBinOp::FloorDiv => if is_float { IROp::FDiv } else { IROp::IDiv },
        AstBinOp::Rem => IROp::IRem,
        AstBinOp::Pow => IROp::IMul, // placeholder
        AstBinOp::BitAnd => IROp::IAnd,
        AstBinOp::BitOr => IROp::IOr,
        AstBinOp::BitXor => IROp::IXor,
        AstBinOp::Shl => IROp::IShl,
        AstBinOp::Shr => IROp::IShr,
        AstBinOp::Eq => {
            if is_str { IROp::StrEq } else if is_float { IROp::FEq } else { IROp::IEq }
        }
        AstBinOp::Ne => {
            if is_str {
                // M12 fix (BUG-034): `Ne` had no `is_str` branch, so `a != b`
                // on strings fell through to `INe`, which compares the two
                // heap-pointer u64s — always distinct for separately-allocated
                // strings, so `str != str` was always true. Mirror `IsNot`
                // (BUG-008): lower as `StrEq` followed by `BoolNot`.
                let eq = fb.push_value(
                    Ty::Primitive(PrimTy::Bool),
                    ValueKind::Op { op: IROp::StrEq, args: vec![l, r] },
                );
                return fb.push_value(
                    ty,
                    ValueKind::Op { op: IROp::BoolNot, args: vec![eq] },
                );
            }
            if is_float { IROp::FNe } else { IROp::INe }
        }
        AstBinOp::Lt => if is_float { IROp::FLt } else { IROp::ILt },
        AstBinOp::Le => if is_float { IROp::FLe } else { IROp::ILe },
        AstBinOp::Gt => if is_float { IROp::FGt } else { IROp::IGt },
        AstBinOp::Ge => if is_float { IROp::FGe } else { IROp::IGe },
        AstBinOp::Is => IROp::RefEq,
        AstBinOp::IsNot => {
            // real-world: fix — the old lowering emitted `IROp::RefEq` for
            // both `is` and `is not`, so `x is not none` evaluated to the
            // same boolean as `x is none` (i.e. inverted). Lower `is not`
            // as `not (l is r)` by emitting `RefEq` followed by `BoolNot`.
            let eq = fb.push_value(
                Ty::Primitive(PrimTy::Bool),
                ValueKind::Op { op: IROp::RefEq, args: vec![l, r] },
            );
            return fb.push_value(
                ty,
                ValueKind::Op { op: IROp::BoolNot, args: vec![eq] },
            );
        }
        AstBinOp::In => {
            // M24 fix (BUG-039): `key in container` previously lowered to
            // `IEq` — comparing the key against the container's heap pointer
            // as i64. Always false for any separately-allocated key, and
            // segfaulted for `<i64> in Dict[i64, _]` (the IEq bit pattern
            // happened to look like a heap pointer that something dereferenced).
            // Same shape as BUG-008 (`is not` was `RefEq`), BUG-034 (`str !=`
            // had no is_str branch), BUG-037 (`??` was Copy(rhs)). FOURTH
            // instance of the placeholder-lowering pattern; every comparison
            // operator's IR lowering needs both forms tested.
            //
            // Dispatch by the RHS (container) type:
            //   key in Dict[K, V] -> DictHas(dict, key)
            //   x   in Set[T]     -> SetHas(set, x)
            //   x   in List[T]    -> still placeholder (v0.3 work: linear
            //                        scan or native NativeFn::ListContains).
            // Note: M5 Dict runtime is hardcoded to string keys (DictHas
            // calls arg_str on the key, so non-str keys segfault — distinct
            // from BUG-039 but the M24-B agent's probe `_probe_dict_in_i64`
            // exposed it). Only dispatch DictHas when the key type is str;
            // for Dict[i64, _] etc. fall back to the old placeholder
            // (returns false silently, same wrong behaviour as pre-fix, but
            // doesn't segfault). Dict with non-str keys is itself v0.3.
            let rhs_ty = find_value_ty(fb, r).unwrap_or(ty.clone());
            let rhs_inner = match &rhs_ty {
                Ty::Nullable(inner) => (**inner).clone(),
                other => other.clone(),
            };
            match &rhs_inner {
                Ty::Generic { base: TypeCtor::Dict, args }
                    if matches!(args.first(), Some(Ty::Primitive(PrimTy::Str))) =>
                {
                    return fb.push_value(
                        ty,
                        ValueKind::Op {
                            op: IROp::NativeCall { native_id: NativeFn::DictHas as u32 },
                            args: vec![r, l],
                        },
                    );
                }
                Ty::Generic { base: TypeCtor::Set, .. } => {
                    return fb.push_value(
                        ty,
                        ValueKind::Op {
                            op: IROp::NativeCall { native_id: NativeFn::SetHas as u32 },
                            args: vec![r, l],
                        },
                    );
                }
                _ => IROp::IEq, // list / non-str-keyed dict / others — still placeholder; v0.3
            }
        }
        AstBinOp::NotIn => {
            // Same dispatch as In, then BoolNot. Mirrors the IsNot precedent.
            let rhs_ty = find_value_ty(fb, r).unwrap_or(ty.clone());
            let rhs_inner = match &rhs_ty {
                Ty::Nullable(inner) => (**inner).clone(),
                other => other.clone(),
            };
            let native_id = match &rhs_inner {
                Ty::Generic { base: TypeCtor::Dict, args }
                    if matches!(args.first(), Some(Ty::Primitive(PrimTy::Str))) =>
                {
                    Some(NativeFn::DictHas as u32)
                }
                Ty::Generic { base: TypeCtor::Set, .. } => Some(NativeFn::SetHas as u32),
                _ => None,
            };
            if let Some(native_id) = native_id {
                let has = fb.push_value(
                    Ty::Primitive(PrimTy::Bool),
                    ValueKind::Op {
                        op: IROp::NativeCall { native_id },
                        args: vec![r, l],
                    },
                );
                return fb.push_value(
                    ty,
                    ValueKind::Op { op: IROp::BoolNot, args: vec![has] },
                );
            }
            IROp::INe // list/other — still placeholder; v0.3
        }
        // M13 (BUG-035): `and` / `or` are intercepted in `lower_expr` for
        // `Expr::Binary` (BEFORE both operands get lowered) and routed to
        // `lower_short_circuit`. The bitwise fallback below is kept only
        // as a defensive backstop: it preserves the pre-M13 behaviour for
        // any synthetic caller that constructs `and`/`or` from already-
        // lowered ValueIds (none today). User-visible programs never hit
        // this path — short-circuit semantics are guaranteed.
        AstBinOp::And => IROp::IAnd,
        AstBinOp::Or => IROp::IOr,
    };
    fb.push_value(ty, ValueKind::Op { op: irop, args: vec![l, r] })
}

/// Lower `a and b` and `a or b` with proper short-circuit semantics by
/// emitting a basic-block split mid-expression. This is the first place
/// in the compiler that creates new BlockIds while lowering an
/// expression; the pattern is intended to be reusable for future
/// mid-expression CFG features (e.g. try/except inside an expression).
///
/// Lowering shape (for `a and b`):
///
/// ```text
///     ; entry block (current)
///     %l = <lower a>
///     slot[t] := %l           ; provides the "false" predecessor
///     if %l then T else MERGE
///   T:
///     %r = <lower b>
///     slot[t] := %r
///     branch MERGE
///   MERGE:
///     %res = ReadLocal slot[t]
/// ```
///
/// For `a or b` the roles flip: the entry block already has `%l == true`
/// stored, so the truthy branch goes straight to MERGE and the rhs is
/// only evaluated on the false branch.
///
/// Phi-merge uses the slot-based ReadLocal/WriteLocal pattern that the
/// rest of the codebase uses for cross-block values (see M3.5's
/// loop-carried locals fix). The slot name is uniquified by current slot
/// count so nested `and`/`or` don't alias.
/// M14 tuples: lower `str((e0, e1, ..., eN))` to:
///   "(" + str(e0) + ", " + str(e1) + ... + ")"
///
/// Each element load is dispatched through the per-primitive-type
/// `str(...)` native (StrFromI32 / StrFromI64 / StrFromF64 / StrFromBool /
/// StrFromChar / identity-copy for str / StrFromAny fallback for class /
/// recursive lower_str_of_tuple for nested tuples).
fn lower_str_of_tuple(
    fb: &mut FuncBuilder,
    ctx: &mut LowerCtx,
    tup: ValueId,
    elem_tys: &[Ty],
) -> ValueId {
    let str_ty = Ty::Primitive(PrimTy::Str);
    let mut acc = fb.push_value(str_ty.clone(), ValueKind::Const(IRConst::Str("(".into())));
    for (i, et) in elem_tys.iter().enumerate() {
        if i > 0 {
            let sep = fb.push_value(str_ty.clone(), ValueKind::Const(IRConst::Str(", ".into())));
            acc = fb.push_value(
                str_ty.clone(),
                ValueKind::Op {
                    op: IROp::NativeCall { native_id: NativeFn::StrConcat as u32 },
                    args: vec![acc, sep],
                },
            );
        }
        let elem = fb.push_value(
            et.clone(),
            ValueKind::Op { op: IROp::Load { offset: (i as u32) * 8 }, args: vec![tup] },
        );
        let s = str_of_value(fb, ctx, elem, et);
        acc = fb.push_value(
            str_ty.clone(),
            ValueKind::Op {
                op: IROp::NativeCall { native_id: NativeFn::StrConcat as u32 },
                args: vec![acc, s],
            },
        );
    }
    let close = fb.push_value(str_ty.clone(), ValueKind::Const(IRConst::Str(")".into())));
    fb.push_value(
        str_ty,
        ValueKind::Op {
            op: IROp::NativeCall { native_id: NativeFn::StrConcat as u32 },
            args: vec![acc, close],
        },
    )
}

/// Stringify a single value at IR time, dispatching on its static type.
/// Mirrors the `str(...)` PrimType-call dispatch above; broken out so
/// `lower_str_of_tuple` can recurse on nested tuple elements.
fn str_of_value(fb: &mut FuncBuilder, ctx: &mut LowerCtx, v: ValueId, ty: &Ty) -> ValueId {
    let str_ty = Ty::Primitive(PrimTy::Str);
    match ty {
        Ty::Primitive(PrimTy::Str) => v,
        Ty::Primitive(PrimTy::I32) | Ty::Primitive(PrimTy::U32)
        | Ty::Primitive(PrimTy::I8)  | Ty::Primitive(PrimTy::I16)
        | Ty::Primitive(PrimTy::U8)  | Ty::Primitive(PrimTy::U16) => fb.push_value(
            str_ty,
            ValueKind::Op {
                op: IROp::NativeCall { native_id: NativeFn::StrFromI32 as u32 },
                args: vec![v],
            },
        ),
        Ty::Primitive(PrimTy::I64) | Ty::Primitive(PrimTy::U64) => fb.push_value(
            str_ty,
            ValueKind::Op {
                op: IROp::NativeCall { native_id: NativeFn::StrFromI64 as u32 },
                args: vec![v],
            },
        ),
        Ty::Primitive(PrimTy::F32) | Ty::Primitive(PrimTy::F64) => fb.push_value(
            str_ty,
            ValueKind::Op {
                op: IROp::NativeCall { native_id: NativeFn::StrFromF64 as u32 },
                args: vec![v],
            },
        ),
        Ty::Primitive(PrimTy::Bool) => fb.push_value(
            str_ty,
            ValueKind::Op {
                op: IROp::NativeCall { native_id: NativeFn::StrFromBool as u32 },
                args: vec![v],
            },
        ),
        Ty::Primitive(PrimTy::Char) => fb.push_value(
            str_ty,
            ValueKind::Op {
                op: IROp::NativeCall { native_id: NativeFn::StrFromChar as u32 },
                args: vec![v],
            },
        ),
        Ty::Tuple(inner) => lower_str_of_tuple(fb, ctx, v, inner),
        _ => fb.push_value(
            str_ty,
            ValueKind::Op {
                op: IROp::NativeCall { native_id: NativeFn::StrFromAny as u32 },
                args: vec![v],
            },
        ),
    }
}

/// M14 tuples: lower `t == u` / `t != u` for `Ty::Tuple` operands.
/// Strategy: lower both sides once, then for each element index `i`
/// emit a Load(8*i) on each side, compare element-wise via the
/// per-element-type `emit_binop` (which already handles str/float/int),
/// and AND the results.  `!=` is `not (a == a)`.
fn lower_tuple_eq(
    fb: &mut FuncBuilder,
    ctx: &mut LowerCtx,
    op: AstBinOp,
    lhs: &Expr,
    rhs: &Expr,
    elem_tys: &[Ty],
) -> ValueId {
    let l = lower_expr(fb, ctx, lhs);
    let r = lower_expr(fb, ctx, rhs);
    // Empty tuple: trivially equal.
    if elem_tys.is_empty() {
        let lit = fb.push_value(
            Ty::Primitive(PrimTy::Bool),
            ValueKind::Const(IRConst::Bool(matches!(op, AstBinOp::Eq))),
        );
        return lit;
    }
    let mut acc: Option<ValueId> = None;
    for (i, et) in elem_tys.iter().enumerate() {
        let lv = fb.push_value(
            et.clone(),
            ValueKind::Op { op: IROp::Load { offset: (i as u32) * 8 }, args: vec![l] },
        );
        let rv = fb.push_value(
            et.clone(),
            ValueKind::Op { op: IROp::Load { offset: (i as u32) * 8 }, args: vec![r] },
        );
        // Always compare-equal here; the != case inverts the final acc.
        let cmp = emit_binop(fb, AstBinOp::Eq, lv, rv, Ty::Primitive(PrimTy::Bool));
        acc = Some(match acc {
            None => cmp,
            Some(prev) => fb.push_value(
                Ty::Primitive(PrimTy::Bool),
                ValueKind::Op { op: IROp::IAnd, args: vec![prev, cmp] },
            ),
        });
    }
    let eq = acc.unwrap();
    match op {
        AstBinOp::Eq => eq,
        AstBinOp::Ne => fb.push_value(
            Ty::Primitive(PrimTy::Bool),
            ValueKind::Op { op: IROp::BoolNot, args: vec![eq] },
        ),
        _ => eq,
    }
}

fn lower_short_circuit(
    fb: &mut FuncBuilder,
    ctx: &mut LowerCtx,
    op: AstBinOp,
    lhs: &Expr,
    rhs: &Expr,
    ty: Ty,
) -> ValueId {
    debug_assert!(matches!(op, AstBinOp::And | AstBinOp::Or));

    // Lower the left operand in the entry block. CRITICAL invariant:
    // the right operand must NOT be touched until we're inside the
    // short-circuit "evaluate rhs" block — that's the whole point.
    let l = lower_expr(fb, ctx, lhs);

    // Allocate a result slot. Name uniquified by slot count so a nested
    // short-circuit expression doesn't clobber the outer slot.
    let slot_name = format!("__sc_{}", fb.slot_ty.len());
    let slot = fb.alloc_slot(&slot_name, ty.clone());

    // Pre-seed the slot with `l` — this is the "short-circuit value"
    // that the merge will read when we skip evaluating the rhs.
    fb.emit_write_local(slot, l);

    let rhs_b = fb.new_block();
    let merge = fb.new_block();

    // For `and`: evaluate rhs only when l is true; otherwise the slot
    // already holds the falsey `l`, so go straight to merge.
    // For `or`:  evaluate rhs only when l is false; otherwise the slot
    // already holds the truthy `l`, so go straight to merge.
    let (t_target, f_target) = match op {
        AstBinOp::And => (rhs_b, merge),
        AstBinOp::Or => (merge, rhs_b),
        _ => unreachable!(),
    };
    fb.terminate(Terminator::CondBranch { cond: l, t: t_target, f: f_target });

    // Evaluate the rhs in its own block and store into the result slot.
    fb.switch_to(rhs_b);
    let r = lower_expr(fb, ctx, rhs);
    fb.emit_write_local(slot, r);
    fb.terminate(Terminator::Branch { target: merge });

    // Merge block: read the slot. ReadLocal sees the most recent write
    // along whichever predecessor edge brought control here, which is
    // exactly the phi semantics we need (the same pattern that lets
    // loop-carried locals work across the back-edge in `while`).
    fb.switch_to(merge);
    fb.emit_read_local(slot)
}

/// M31: look up the per-instantiation (type_id, __init__ FuncId) for a
/// generic class. Returns `(u32::MAX, None)` if this `(class_id,
/// type_args)` hasn't been pre-registered by Pass 2.7 — meaning the
/// instantiation was discovered transitively while lowering another
/// generic body. v0.3 documents transitive generic-class construction
/// from inside another generic body as an open follow-up (the
/// typechecker DOES record the instantiation at every concrete site
/// in the user-visible call graph, so this path is rare in practice).
fn resolve_or_mint_class_inst(
    ctx: &LowerCtx,
    cid: ClassId,
    _type_args: &[Ty],
    key: &str,
) -> (u32, Option<FuncId>) {
    if let Some(tid) = ctx.class_inst_type_id.get(&(cid, key.to_string())).copied() {
        let init_fid = ctx.class_inst_init_fn.get(&(cid, key.to_string())).copied();
        return (tid, init_fid);
    }
    (u32::MAX, None)
}

/// M17: does `t` reference any `Ty::Var`? Used by Pass 2.6 to skip
/// typechecker-recorded instantiations that are still abstract.
fn has_unbound_var(t: &Ty) -> bool {
    match t {
        Ty::Var(_) => true,
        Ty::Generic { args, .. } | Ty::Tuple(args) => args.iter().any(has_unbound_var),
        Ty::Function { params, ret } => {
            params.iter().any(has_unbound_var) || has_unbound_var(ret)
        }
        Ty::Nullable(inner) => has_unbound_var(inner),
        _ => false,
    }
}

fn find_value_ty(fb: &FuncBuilder, v: ValueId) -> Option<Ty> {
    for b in &fb.blocks {
        for val in &b.values {
            if val.id == v.0 {
                return Some(val.ty.clone());
            }
        }
    }
    None
}

fn lower_call(
    fb: &mut FuncBuilder,
    ctx: &mut LowerCtx,
    callee: &Expr,
    args: &[ast::Arg],
    span: Span,
) -> ValueId {
    let ret_ty = ctx.expr_ty(span);

    // M19: namespaced stdlib call — `sys.exit(0)`.  Detect *before* the
    // generic Ident handling so the callee's Attr isn't lowered as a
    // field load (which would synthesise a bogus Load on the builtin-
    // module placeholder slot).  The args are lowered exactly as for
    // any other native call; we then emit a CallNative carrying the
    // item's `native_id`.
    if let Expr::Attr { obj, name: attr, .. } = callee {
        if let Expr::Ident { name: mname, .. } = obj.as_ref() {
            let scope = ctx.typed.resolved.module_scope;
            if let Some(sid) = ctx.typed.resolved.symbols.lookup(scope, mname) {
                if matches!(
                    ctx.typed.resolved.symbols.get(sid).kind,
                    SymbolKind::BuiltinModule
                ) {
                    let mod_name = ctx.typed.resolved.module_alias
                        .get(&sid)
                        .cloned()
                        .unwrap_or_else(|| mname.clone());
                    if let Some(m) = ctx.typed.resolved.stdlib_modules.get(&mod_name) {
                        if let Some(item) = m.find(attr) {
                            if matches!(item.kind, crate::resolver::StdlibItemKind::Function) {
                                let mut arg_vs: Vec<ValueId> = Vec::with_capacity(args.len());
                                for a in args {
                                    arg_vs.push(lower_expr(fb, ctx, &a.value));
                                }
                                return fb.push_value(
                                    ret_ty,
                                    ValueKind::Op {
                                        op: IROp::NativeCall { native_id: item.native_id },
                                        args: arg_vs,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // M19: bare call to a `from sys import exit` symbol — same path
    // as above but the callee is an Ident bound to an Import symbol
    // carrying a StdlibItem of kind Function.
    if let Expr::Ident { name, .. } = callee {
        let scope = ctx.typed.resolved.module_scope;
        if let Some(sid) = ctx.typed.resolved.symbols.lookup(scope, name) {
            if let Some(item) = ctx.typed.resolved.import_item.get(&sid).cloned() {
                if matches!(item.kind, crate::resolver::StdlibItemKind::Function) {
                    let mut arg_vs: Vec<ValueId> = Vec::with_capacity(args.len());
                    for a in args {
                        arg_vs.push(lower_expr(fb, ctx, &a.value));
                    }
                    return fb.push_value(
                        ret_ty,
                        ValueKind::Op {
                            op: IROp::NativeCall { native_id: item.native_id },
                            args: arg_vs,
                        },
                    );
                }
            }
        }
    }

    // M16: `isinstance(x, T)` is a *builtin* call whose second argument is a
    // type name, not a value. Lower the receiver, then emit an IROp::IsInstance
    // carrying the resolved class id. Doing this before the generic arg-lower
    // avoids accidentally treating the class name as an Ident expression.
    if let Expr::Ident { name, .. } = callee {
        if name == "isinstance" && args.len() == 2 {
            let obj = lower_expr(fb, ctx, &args[0].value);
            // Resolve the second arg as a class name.
            let mut class_tid: Option<u32> = None;
            if let Expr::Ident { name: tname, .. } = &args[1].value {
                if let Some(sid) = ctx.typed.resolved.symbols.lookup(
                    ctx.typed.resolved.module_scope,
                    tname,
                ) {
                    let sym = ctx.typed.resolved.symbols.get(sid);
                    if let Some(cid) = sym.class_id {
                        class_tid = Some(
                            ctx.class_type_id
                                .get(&cid.0)
                                .copied()
                                .unwrap_or(cid.0),
                        );
                    }
                }
            }
            let tid = class_tid.unwrap_or(u32::MAX);
            return fb.push_value(
                Ty::Primitive(PrimTy::Bool),
                ValueKind::Op {
                    op: IROp::IsInstance { class_id: tid },
                    args: vec![obj],
                },
            );
        }
    }

    // Lower args.
    let mut arg_vs: Vec<ValueId> = Vec::with_capacity(args.len());
    for a in args {
        arg_vs.push(lower_expr(fb, ctx, &a.value));
    }

    // Resolve callee.
    if let Expr::Ident { name, .. } = callee {
        // Constructor? Look up the symbol.
        if let Some(sid) = ctx.typed.resolved.symbols.lookup(
            ctx.typed.resolved.module_scope,
            name,
        ) {
            let sym = ctx.typed.resolved.symbols.get(sid);
            match sym.kind {
                SymbolKind::Class => {
                    if let Some(cid) = sym.class_id {
                        // stdlib: native runtime classes have no user-level
                        // __init__ — they're constructed via a NativeFn that
                        // allocates the handle-backed repr (e.g. ThreadNew →
                        // alloc_thread()). Skip the generic Alloc+__init__
                        // dance for these, otherwise we'd return a raw zeroed
                        // ObjectHeader rather than a real Thread/File handle.
                        if let Some(layout) = ctx.class_layouts.get(&cid) {
                            if layout.is_native {
                                let nid = match layout.name.as_str() {
                                    "Thread" => NativeFn::ThreadNew as u32,
                                    // io.File is constructed by `open(...)`,
                                    // not by `io.File(...)`, so no path here.
                                    _ => NativeFn::from_name(name)
                                        .map(|n| n as u32)
                                        .unwrap_or(NativeFn::Unknown as u32),
                                };
                                return fb.push_value(
                                    ret_ty,
                                    ValueKind::Op {
                                        op: IROp::NativeCall { native_id: nid },
                                        args: arg_vs,
                                    },
                                );
                            }
                            // M31: generic class — the call site's expr_ty
                            // is `Ty::Generic { base: TypeCtor::Class(cid),
                            // args: <concrete> }`. We resolve to the
                            // per-instantiation tid + __init__ FuncId,
                            // minting a fresh instantiation on the fly if
                            // this is the first call with these type args
                            // (e.g. inside another generic body, post
                            // active-subst).
                            if !layout.generic_tvars.is_empty() {
                                // The expr_ty for the call already had the
                                // active substitution applied in expr_ty,
                                // so it contains concrete types.
                                let call_ty = ret_ty.clone();
                                let type_args: Vec<Ty> = match &call_ty {
                                    Ty::Generic { base: TypeCtor::Class(c), args }
                                        if *c == cid => args.clone(),
                                    // Fallback — shouldn't normally happen.
                                    _ => Vec::new(),
                                };
                                if !type_args.is_empty()
                                    && !type_args.iter().any(has_unbound_var)
                                {
                                    let key = mangle_args_key(&type_args);
                                    let (tid, init_fid) =
                                        resolve_or_mint_class_inst(ctx, cid, &type_args, &key);
                                    let alloc = fb.push_value(
                                        Ty::Class(cid),
                                        ValueKind::Op {
                                            op: IROp::Alloc { class_id: tid },
                                            args: vec![],
                                        },
                                    );
                                    if let Some(FuncId(fid)) = init_fid {
                                        let mut call_args = vec![alloc];
                                        call_args.extend(arg_vs);
                                        fb.push_value(
                                            Ty::Primitive(PrimTy::Unit),
                                            ValueKind::Op {
                                                op: IROp::DirectCall {
                                                    fn_id: FuncId(fid),
                                                },
                                                args: call_args,
                                            },
                                        );
                                    }
                                    return alloc;
                                }
                            }
                        }
                        // Allocate + call __init__ (if present).
                        //
                        // M11 fix (BUG-N1 subsidiary): emit the runtime
                        // *type_id* rather than the resolver's class_id.
                        // The VM's `op_new` previously fell back to using
                        // class_id as a type-table index, but once enough
                        // user classes existed the class_id numerically
                        // collided with another class's type_id and the
                        // direct lookup returned the wrong RuntimeType
                        // (Pentagon got Shape's vtable, etc.).
                        let tid = ctx
                            .class_type_id
                            .get(&cid.0)
                            .copied()
                            .unwrap_or(cid.0);
                        let alloc = fb.push_value(
                            Ty::Class(cid),
                            ValueKind::Op { op: IROp::Alloc { class_id: tid }, args: vec![] },
                        );
                        // M34: JsonValue subclass constructors — these
                        // have no user-level `__init__`; we synthesise
                        // initialisation by calling the matching
                        // NativeFn::JsonJ*New handler with the allocated
                        // object as the first arg.  The handler does the
                        // field stores (and any sidecar list allocation
                        // for JList/JObject).
                        if let Some(nid) = m34_json_class_init_native_id(name) {
                            let mut call_args = vec![alloc];
                            call_args.extend(arg_vs);
                            fb.push_value(
                                Ty::Primitive(PrimTy::Unit),
                                ValueKind::Op {
                                    op: IROp::NativeCall { native_id: nid },
                                    args: call_args,
                                },
                            );
                            return alloc;
                        }
                        let init_name = format!("{}.__init__", name);
                        if let Some(FuncId(fid)) = ctx.fn_id_by_name.get(&init_name).copied() {
                            let mut call_args = vec![alloc];
                            call_args.extend(arg_vs);
                            fb.push_value(
                                Ty::Primitive(PrimTy::Unit),
                                ValueKind::Op {
                                    op: IROp::DirectCall { fn_id: FuncId(fid) },
                                    args: call_args,
                                },
                            );
                        }
                        return alloc;
                    }
                }
                SymbolKind::Function => {
                    // M17: generic function call — rebuild the substitution
                    // from this call's argument types and dispatch to the
                    // matching mangled FuncId. Argument types may themselves
                    // contain `Ty::Var` if the call appears inside another
                    // generic body; the current `type_subst` resolves those
                    // before unification, which yields the concrete type
                    // args for the callee. If the (sid, type_args) pair is
                    // brand new (no FuncId yet), mint one *here* and push
                    // onto the worklist so its body gets lowered later.
                    if let Some(gen_sid) = ctx.generic_fn_sid.get(name).copied() {
                        if let Some(sig) =
                            ctx.typed.resolved.function_sigs.get(&gen_sid).cloned()
                        {
                            let mut subst: HashMap<u32, Ty> = HashMap::new();
                            for (i, (_, ptype)) in sig.params.iter().enumerate() {
                                if let Some(a) = args.get(i) {
                                    let mut arg_ty = ctx.expr_ty(expr_span(&a.value));
                                    arg_ty = subst_ty(&arg_ty, &ctx.type_subst);
                                    let _ = unify_lower(ptype, &arg_ty, &mut subst);
                                }
                            }
                            let mut type_args: Vec<Ty> = Vec::new();
                            for tv in &sig.generic_tvars {
                                type_args.push(
                                    subst.get(&tv.0).cloned().unwrap_or(Ty::Never),
                                );
                            }
                            let key = mangle_args_key(&type_args);
                            let fid = if let Some(f) =
                                ctx.fn_id_for_inst.get(&(gen_sid, key.clone())).copied()
                            {
                                f
                            } else {
                                // Mint a FuncId on the fly. Caller-side
                                // worklist registration so the outer pass
                                // lowers the body next iteration.
                                let raw = *ctx.next_fn_id;
                                *ctx.next_fn_id += 1;
                                let f = FuncId(raw);
                                let src = ctx
                                    .typed
                                    .resolved
                                    .symbols
                                    .get(gen_sid)
                                    .name
                                    .clone();
                                let mangled = format!("{}__{}", src, key);
                                ctx.fn_id_for_inst
                                    .insert((gen_sid, key.clone()), f);
                                ctx.mangled_name_for_inst
                                    .insert((gen_sid, key.clone()), mangled);
                                ctx.inst_worklist.push((gen_sid, type_args));
                                f
                            };
                            return fb.push_value(
                                ret_ty,
                                ValueKind::Op {
                                    op: IROp::DirectCall { fn_id: fid },
                                    args: arg_vs,
                                },
                            );
                        }
                    }
                    // User-defined function?
                    if let Some(fid) = ctx.fn_id_by_name.get(name).copied() {
                        return fb.push_value(
                            ret_ty,
                            ValueKind::Op { op: IROp::DirectCall { fn_id: fid }, args: arg_vs },
                        );
                    }
                    // M7: dispatch `len(x)` on the argument's static type —
                    // the generic NativeFn::Len reads `length` at byte 16,
                    // which is correct for List/str but reads the `handle`
                    // field of a DictRepr instead of its true element count.
                    if name == "len" {
                        if let Some(arg0) = args.first() {
                            let arg_ty = ctx.expr_ty(expr_span(&arg0.value));
                            let nid = match arg_ty {
                                Ty::Generic { base: TypeCtor::Dict, .. } => {
                                    NativeFn::DictLen as u32
                                }
                                _ => NativeFn::Len as u32,
                            };
                            return fb.push_value(
                                ret_ty,
                                ValueKind::Op {
                                    op: IROp::NativeCall { native_id: nid },
                                    args: arg_vs,
                                },
                            );
                        }
                    }
                    // real-world: `sorted(xs)` needs an explicit type-tag
                    // operand so the VM can pick the right comparator.
                    // Infer the element type from the receiver List[T].
                    if name == "sorted" {
                        if let Some(arg0) = args.first() {
                            let arg_ty = ctx.expr_ty(expr_span(&arg0.value));
                            let tag = sort_type_tag_for(&arg_ty);
                            let tag_v = fb.push_value(
                                Ty::Primitive(PrimTy::I64),
                                ValueKind::Const(IRConst::I64(tag as i64)),
                            );
                            let mut sort_args = arg_vs;
                            sort_args.push(tag_v);
                            return fb.push_value(
                                ret_ty,
                                ValueKind::Op {
                                    op: IROp::NativeCall { native_id: NativeFn::ListSorted as u32 },
                                    args: sort_args,
                                },
                            );
                        }
                    }
                    // Otherwise a native.
                    let nid = NativeFn::from_name(name)
                        .map(|n| n as u32)
                        .unwrap_or(NativeFn::Unknown as u32);
                    return fb.push_value(
                        ret_ty,
                        ValueKind::Op { op: IROp::NativeCall { native_id: nid }, args: arg_vs },
                    );
                }
                SymbolKind::PrimType => {
                    // Conversion call: e.g. `i32(x)`, `f64(x)`, `str(x)`.
                    // For `str(x)`, dispatch to a type-specialised native
                    // when the argument's static type is a primitive — the
                    // generic StrFromAny heuristic in the VM segfaults on
                    // non-pointer bit patterns (e.g. f64 values look like
                    // wild pointers).
                    //
                    // M11 fix: the same per-arg-type dispatch is required
                    // for the *numeric* ctors `i32(x)`, `i64(x)`, `f64(x)`,
                    // `char(x)`. Previously `NativeFn::from_name("i32")`
                    // returned `I32FromF64` unconditionally, so `i32(i64)`
                    // reinterpreted the i64 bit pattern as f64 (tiny ints
                    // look like denormal f64s and truncate to 0).
                    let arg_ty = args.first().map(|a| ctx.expr_ty(expr_span(&a.value)));
                    // M14 tuples: `str((a, b, ...))` builds `"(a, b, ...)"`
                    // by element-wise `str(...)` then `+`-concat with ", "
                    // separators.  We emit this entirely at IR time so the
                    // VM doesn't need a new native — each element dispatches
                    // through the existing per-type str(...) handler we're
                    // sitting inside.
                    if name == "str" {
                        if let Some(Ty::Tuple(elem_tys)) = arg_ty.clone() {
                            let tup = arg_vs[0];
                            return lower_str_of_tuple(fb, ctx, tup, &elem_tys);
                        }
                    }
                    let nid = if name == "str" {
                        match arg_ty {
                            Some(Ty::Primitive(PrimTy::I32))
                            | Some(Ty::Primitive(PrimTy::U32))
                            | Some(Ty::Primitive(PrimTy::I8))
                            | Some(Ty::Primitive(PrimTy::I16))
                            | Some(Ty::Primitive(PrimTy::U8))
                            | Some(Ty::Primitive(PrimTy::U16)) => NativeFn::StrFromI32 as u32,
                            Some(Ty::Primitive(PrimTy::I64))
                            | Some(Ty::Primitive(PrimTy::U64)) => NativeFn::StrFromI64 as u32,
                            Some(Ty::Primitive(PrimTy::F32))
                            | Some(Ty::Primitive(PrimTy::F64)) => NativeFn::StrFromF64 as u32,
                            Some(Ty::Primitive(PrimTy::Bool)) => NativeFn::StrFromBool as u32,
                            // real-world: fix — str(char) used to fall
                            // through to StrFromAny which formatted the
                            // codepoint as a decimal integer (so `str('h')`
                            // returned "104"). Dispatch to StrFromChar so a
                            // single-codepoint string is allocated.
                            Some(Ty::Primitive(PrimTy::Char)) => NativeFn::StrFromChar as u32,
                            Some(Ty::Primitive(PrimTy::Str)) => {
                                // No conversion needed — emit a copy.
                                return fb.push_value(
                                    ret_ty,
                                    ValueKind::Op { op: IROp::Copy, args: arg_vs },
                                );
                            }
                            _ => NativeFn::StrFromAny as u32,
                        }
                    } else if name == "i32" {
                        match arg_ty {
                            Some(Ty::Primitive(PrimTy::I64))
                            | Some(Ty::Primitive(PrimTy::U64)) => NativeFn::I32FromI64 as u32,
                            Some(Ty::Primitive(PrimTy::F32))
                            | Some(Ty::Primitive(PrimTy::F64)) => NativeFn::I32FromF64 as u32,
                            // i32(char) reads the codepoint — char's storage
                            // already fits in 32 bits, so a no-op copy is fine.
                            Some(Ty::Primitive(PrimTy::Char)) => {
                                return fb.push_value(
                                    ret_ty,
                                    ValueKind::Op { op: IROp::Copy, args: arg_vs },
                                );
                            }
                            // Identity / widen-from-smaller-int — just copy.
                            Some(Ty::Primitive(PrimTy::I32))
                            | Some(Ty::Primitive(PrimTy::U32))
                            | Some(Ty::Primitive(PrimTy::I8))
                            | Some(Ty::Primitive(PrimTy::I16))
                            | Some(Ty::Primitive(PrimTy::U8))
                            | Some(Ty::Primitive(PrimTy::U16))
                            | Some(Ty::Primitive(PrimTy::Bool)) => {
                                return fb.push_value(
                                    ret_ty,
                                    ValueKind::Op { op: IROp::Copy, args: arg_vs },
                                );
                            }
                            _ => NativeFn::I32FromF64 as u32, // fallback (legacy)
                        }
                    } else if name == "i64" {
                        match arg_ty {
                            Some(Ty::Primitive(PrimTy::I32))
                            | Some(Ty::Primitive(PrimTy::U32))
                            | Some(Ty::Primitive(PrimTy::I8))
                            | Some(Ty::Primitive(PrimTy::I16))
                            | Some(Ty::Primitive(PrimTy::U8))
                            | Some(Ty::Primitive(PrimTy::U16))
                            | Some(Ty::Primitive(PrimTy::Bool)) => NativeFn::I64FromI32 as u32,
                            Some(Ty::Primitive(PrimTy::F32))
                            | Some(Ty::Primitive(PrimTy::F64)) => NativeFn::I64FromF64 as u32,
                            // i64(char) — codepoint zero-extended to 64 bits.
                            // Char storage already zero-extends so a copy works.
                            Some(Ty::Primitive(PrimTy::Char)) => {
                                return fb.push_value(
                                    ret_ty,
                                    ValueKind::Op { op: IROp::Copy, args: arg_vs },
                                );
                            }
                            // i64(i64): identity copy.
                            Some(Ty::Primitive(PrimTy::I64))
                            | Some(Ty::Primitive(PrimTy::U64)) => {
                                return fb.push_value(
                                    ret_ty,
                                    ValueKind::Op { op: IROp::Copy, args: arg_vs },
                                );
                            }
                            _ => NativeFn::I64FromI32 as u32,
                        }
                    } else if name == "f64" {
                        match arg_ty {
                            Some(Ty::Primitive(PrimTy::I32))
                            | Some(Ty::Primitive(PrimTy::U32))
                            | Some(Ty::Primitive(PrimTy::I8))
                            | Some(Ty::Primitive(PrimTy::I16))
                            | Some(Ty::Primitive(PrimTy::U8))
                            | Some(Ty::Primitive(PrimTy::U16))
                            | Some(Ty::Primitive(PrimTy::Bool)) => NativeFn::F64FromI32 as u32,
                            Some(Ty::Primitive(PrimTy::I64))
                            | Some(Ty::Primitive(PrimTy::U64)) => NativeFn::F64FromI64 as u32,
                            // f64(f64): identity copy.
                            Some(Ty::Primitive(PrimTy::F32))
                            | Some(Ty::Primitive(PrimTy::F64)) => {
                                return fb.push_value(
                                    ret_ty,
                                    ValueKind::Op { op: IROp::Copy, args: arg_vs },
                                );
                            }
                            _ => NativeFn::F64FromI64 as u32,
                        }
                    } else if name == "char" {
                        match arg_ty {
                            // char(i32) — already the canonical case.
                            Some(Ty::Primitive(PrimTy::I32))
                            | Some(Ty::Primitive(PrimTy::U32))
                            | Some(Ty::Primitive(PrimTy::I8))
                            | Some(Ty::Primitive(PrimTy::I16))
                            | Some(Ty::Primitive(PrimTy::U8))
                            | Some(Ty::Primitive(PrimTy::U16)) => NativeFn::CharFromI32 as u32,
                            // char(i64) — truncate the codepoint to 32 bits.
                            Some(Ty::Primitive(PrimTy::I64))
                            | Some(Ty::Primitive(PrimTy::U64)) => NativeFn::CharFromI32 as u32,
                            // char(char): identity copy.
                            Some(Ty::Primitive(PrimTy::Char)) => {
                                return fb.push_value(
                                    ret_ty,
                                    ValueKind::Op { op: IROp::Copy, args: arg_vs },
                                );
                            }
                            _ => NativeFn::CharFromI32 as u32,
                        }
                    } else {
                        NativeFn::from_name(name)
                            .map(|n| n as u32)
                            .unwrap_or(NativeFn::StrFromAny as u32)
                    };
                    return fb.push_value(
                        ret_ty,
                        ValueKind::Op { op: IROp::NativeCall { native_id: nid }, args: arg_vs },
                    );
                }
                _ => {}
            }
        }
    }

    // Subscripted callee like `Channel[i32](16)`: treat as a generic
    // constructor — dispatch to a native channel ctor.
    if let Expr::Index { obj, .. } = callee {
        if let Expr::Ident { name, .. } = &**obj {
            if name == "Channel" {
                return fb.push_value(
                    ret_ty,
                    ValueKind::Op {
                        op: IROp::NativeCall { native_id: NativeFn::ChannelNew as u32 },
                        args: arg_vs,
                    },
                );
            }
        }
    }

    // Fallback: lower callee as an expression and call indirectly.
    let cv = lower_expr(fb, ctx, callee);
    let mut call_args = vec![cv];
    call_args.extend(arg_vs);
    fb.push_value(
        ret_ty,
        ValueKind::Op { op: IROp::IndirectCall, args: call_args },
    )
}

fn lower_method_call(
    fb: &mut FuncBuilder,
    ctx: &mut LowerCtx,
    receiver: &Expr,
    method: &str,
    args: &[ast::Arg],
    span: Span,
) -> ValueId {
    // M19: `sys.exit(0)` parses as a MethodCall (the postfix parser
    // folds `Attr + LParen` into MethodCall). Detect a builtin-module
    // receiver *before* lowering the receiver — which would emit a
    // bogus `Const(None)` placeholder for the module Ident — and
    // dispatch straight to the stdlib item's NativeFn.
    let ret_ty = ctx.expr_ty(span);
    if let Expr::Ident { name: mname, .. } = receiver {
        let scope = ctx.typed.resolved.module_scope;
        if let Some(sid) = ctx.typed.resolved.symbols.lookup(scope, mname) {
            if matches!(
                ctx.typed.resolved.symbols.get(sid).kind,
                SymbolKind::BuiltinModule
            ) {
                let mod_name = ctx.typed.resolved.module_alias
                    .get(&sid)
                    .cloned()
                    .unwrap_or_else(|| mname.clone());
                if let Some(m) = ctx.typed.resolved.stdlib_modules.get(&mod_name) {
                    if let Some(item) = m.find(method) {
                        if matches!(item.kind, crate::resolver::StdlibItemKind::Function) {
                            let mut arg_vs: Vec<ValueId> = Vec::with_capacity(args.len());
                            for a in args {
                                arg_vs.push(lower_expr(fb, ctx, &a.value));
                            }
                            return fb.push_value(
                                ret_ty,
                                ValueKind::Op {
                                    op: IROp::NativeCall { native_id: item.native_id },
                                    args: arg_vs,
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    let recv = lower_expr(fb, ctx, receiver);
    let recv_ty = ctx.expr_ty(expr_span(receiver));
    let mut arg_vs = vec![recv];
    for a in args {
        arg_vs.push(lower_expr(fb, ctx, &a.value));
    }

    // M31: method dispatch on a parameterised generic class receiver.
    // The expr_ty for the receiver carries the concrete type args after
    // any active substitution; we mangle them to the same key
    // Pass 2.7/3.6 used and dispatch directly to the per-instantiation
    // mangled method FuncId. Each Box[i64] / Box[str] / etc. has its
    // own DirectCall target, never a vtable slot. (Generic classes do
    // not yet participate in inheritance hierarchies, so virtual
    // dispatch isn't needed in M31.)
    if let Ty::Generic { base: TypeCtor::Class(cid), args: targs } = &recv_ty {
        if !targs.iter().any(has_unbound_var) {
            let key = mangle_args_key(targs);
            if let Some(FuncId(fid)) = ctx
                .class_inst_method_fn
                .get(&(*cid, key.clone(), method.to_string()))
                .copied()
            {
                return fb.push_value(
                    ret_ty,
                    ValueKind::Op {
                        op: IROp::DirectCall { fn_id: FuncId(fid) },
                        args: arg_vs,
                    },
                );
            }
        }
    }

    // M34: JList / JObject method calls — these are non-native classes
    // (so they have real heap layouts that pattern-matching can use),
    // but their methods are NativeFn-backed (no user body to call).
    // We dispatch by *class name* via the receiver's class layout to
    // avoid clashing with the M11/M16 vtable path for user classes
    // that happen to share method names like "get" / "length".
    if let Ty::Class(cid) = &recv_ty {
        if let Some(layout) = ctx.class_layouts.get(cid) {
            if let Some(nid) = m34_json_class_method_native_id_by_name(
                layout.name.as_str(), method,
            ) {
                return fb.push_value(
                    ret_ty,
                    ValueKind::Op {
                        op: IROp::NativeCall { native_id: nid },
                        args: arg_vs,
                    },
                );
            }
        }
    }

    // If the receiver type names a known user class with this method, prefer
    // a virtual call (slot = index of method in the *virtual* method list,
    // i.e. layout.methods minus `__init__` — see resolver.rs).
    //
    // stdlib: skip the vtable path entirely for built-in runtime classes
    // (Channel, Thread, io.File, Dict, str) — they are handle-backed and
    // have no real vtable; fall through to the NativeCall path below so
    // `t.start()`, `f.read()`, `d.get(k)` etc. dispatch correctly. The
    // ClassLayout.is_native flag is set in resolver.rs::seed_prelude.
    if let Ty::Class(cid) = &recv_ty {
        if let Some(layout) = ctx.class_layouts.get(cid) {
            if !layout.is_native {
            if let Some(slot) = layout
                .methods
                .iter()
                .filter(|m| m.name != "__init__")
                .position(|m| m.name == method)
            {
                // Only devirtualise to a DirectCall when the receiver's
                // static type is `final` — for `open` *and* `sealed`
                // classes the actual runtime type may be a subclass that
                // overrides this method, and we must dispatch through the
                // vtable to see the override.
                //
                // M11 BUG-015 fix: previously this branch only checked
                // `!is_open`, so sealed-typed receivers dropped to the
                // base implementation even when the runtime instance was
                // a subclass override. `sealed` controls *who may define
                // subclasses*, not how methods are dispatched.
                let key = format!("{}.{}", layout.name, method);
                if !layout.is_open && !layout.is_sealed {
                    if let Some(fid) = ctx.fn_id_by_name.get(&key).copied() {
                        return fb.push_value(
                            ret_ty,
                            ValueKind::Op { op: IROp::DirectCall { fn_id: fid }, args: arg_vs },
                        );
                    }
                }
                return fb.push_value(
                    ret_ty,
                    ValueKind::Op {
                        op: IROp::VirtualCall { vtable_slot: slot as u32 },
                        args: arg_vs,
                    },
                );
            }
            }
        }
    }

    // real-world: `xs.sort()` over `List[T]` needs an extra type-tag
    // operand so the VM picks the right comparator. Inject it before
    // dispatching to NativeFn::ListSort.
    if method == "sort" {
        if let Ty::Generic { base: TypeCtor::List, .. } = &recv_ty {
            let tag = sort_type_tag_for(&recv_ty);
            let tag_v = fb.push_value(
                Ty::Primitive(PrimTy::I64),
                ValueKind::Const(IRConst::I64(tag as i64)),
            );
            arg_vs.push(tag_v);
            return fb.push_value(
                ret_ty,
                ValueKind::Op {
                    op: IROp::NativeCall { native_id: NativeFn::ListSort as u32 },
                    args: arg_vs,
                },
            );
        }
    }

    // Otherwise treat as a native method (e.g. Channel.send, List.append).
    // stdlib: a few method names are overloaded across runtime classes
    // (e.g. `close()` exists on both Channel and io.File). Disambiguate by
    // inspecting the receiver type — otherwise NativeFn::from_name's first
    // match wins and we end up calling FileClose on a Channel handle.
    let nid = resolve_native_method(&recv_ty, method);
    fb.push_value(
        ret_ty,
        ValueKind::Op { op: IROp::NativeCall { native_id: nid }, args: arg_vs },
    )
}

/// Map a `List[T]` (or any container type) to the TypeTag byte the
/// sort/sorted natives use to pick a comparator. Unknown types map to
/// TypeTag::Ref so the VM at least sorts something — but the VM will
/// raise TypeError for elements that aren't actually str pointers.
///
/// real-world: every sort callsite passes this tag as the final
/// argument.
fn sort_type_tag_for(ty: &Ty) -> u8 {
    use strictpy_shared::TypeTag;
    let elem = match ty {
        Ty::Generic { args, .. } if !args.is_empty() => &args[0],
        other => other,
    };
    // Strip nullable just in case — the audit caught this exact gotcha
    // elsewhere; sorting a List[T?] would otherwise misdispatch.
    let elem = match elem {
        Ty::Nullable(inner) => inner.as_ref(),
        other => other,
    };
    match elem {
        Ty::Primitive(PrimTy::I64) | Ty::Primitive(PrimTy::U64)
            | Ty::Primitive(PrimTy::I32) | Ty::Primitive(PrimTy::U32)
            | Ty::Primitive(PrimTy::I16) | Ty::Primitive(PrimTy::U16)
            | Ty::Primitive(PrimTy::I8)  | Ty::Primitive(PrimTy::U8)
            => TypeTag::I64 as u8,
        Ty::Primitive(PrimTy::F32) | Ty::Primitive(PrimTy::F64)
            => TypeTag::F64 as u8,
        // Strings are heap-allocated; their slot is a pointer.
        Ty::Primitive(PrimTy::Str) => TypeTag::Ref as u8,
        _ => TypeTag::Ref as u8,
    }
}

/// Pick the right `NativeFn` for `receiver.method(...)` given the static
/// receiver type. Falls back to `NativeFn::from_name` when the method name
/// is unambiguous across runtime classes.
fn resolve_native_method(recv_ty: &Ty, method: &str) -> u32 {
    // Channel-specific overrides (recv_ty is `Generic { Channel, [..] }`).
    if let Ty::Generic { base: TypeCtor::Channel, .. } = recv_ty {
        return match method {
            "send"     => NativeFn::ChannelSend     as u32,
            "recv"     => NativeFn::ChannelRecv     as u32,
            "try_recv" => NativeFn::ChannelTryRecv  as u32,
            "close"    => NativeFn::ChannelClose    as u32,
            _ => NativeFn::from_name(method)
                .map(|n| n as u32)
                .unwrap_or(NativeFn::Unknown as u32),
        };
    }
    // M32: Future[T] method dispatch (recv_ty is `Generic { Future, [..] }`).
    // The element type is type-erased at the native-call boundary — the
    // value lives in the Future slot as a u64 and the static type at the
    // call site (recv_ty's first arg) drives how the caller interprets it.
    if let Ty::Generic { base: TypeCtor::Future, .. } = recv_ty {
        return match method {
            "await"    => NativeFn::AsyncioFutureAwait   as u32,
            "is_ready" => NativeFn::AsyncioFutureIsReady as u32,
            _ => NativeFn::from_name(method)
                .map(|n| n as u32)
                .unwrap_or(NativeFn::Unknown as u32),
        };
    }
    // Dict-specific overrides (recv_ty is `Generic { Dict, [..] }`).
    if let Ty::Generic { base: TypeCtor::Dict, .. } = recv_ty {
        return match method {
            "get"    => NativeFn::DictGet    as u32,
            "has"    => NativeFn::DictHas    as u32,
            "keys"   => NativeFn::DictKeys   as u32,
            "values" => NativeFn::DictValues as u32,
            _ => NativeFn::from_name(method)
                .map(|n| n as u32)
                .unwrap_or(NativeFn::Unknown as u32),
        };
    }
    NativeFn::from_name(method)
        .map(|n| n as u32)
        .unwrap_or(NativeFn::Unknown as u32)
}

/// M34: map a constructor name to the matching `JsonJ*New` NativeFn id,
/// or `None` if `name` isn't one of the registered JsonValue subclasses.
/// Used by `lower_call` to route `JString("hi")` through a native
/// initialiser rather than the missing user `__init__`.
fn m34_json_class_init_native_id(name: &str) -> Option<u32> {
    Some(match name {
        "JNull"   => NativeFn::JsonJNullNew    as u32,
        "JBool"   => NativeFn::JsonJBoolNew    as u32,
        "JInt"    => NativeFn::JsonJIntNew     as u32,
        "JFloat"  => NativeFn::JsonJFloatNew   as u32,
        "JString" => NativeFn::JsonJStringNew  as u32,
        "JList"   => NativeFn::JsonJListNew    as u32,
        "JObject" => NativeFn::JsonJObjectNew  as u32,
        _ => return None,
    })
}

/// M34: dispatch a JList / JObject method by class name + method name.
/// Returns `None` for any other (class, method) pair so the caller falls
/// through to the regular dispatch.
///
/// M35 P4-C extension: streaming `Hasher` methods route through the
/// same path.  Keeping one dispatcher for all class-by-name stdlib
/// classes keeps the IR-side change small; the function name is
/// `m34_*` historically but it now serves M34+M35 stdlib classes.
fn m34_json_class_method_native_id_by_name(
    class_name: &str,
    method: &str,
) -> Option<u32> {
    Some(match (class_name, method) {
        ("JList",   "length") => NativeFn::JsonJListLength   as u32,
        ("JList",   "get")    => NativeFn::JsonJListGet      as u32,
        ("JList",   "items")  => NativeFn::JsonJListItems    as u32,
        ("JObject", "get")    => NativeFn::JsonJObjectGet    as u32,
        ("JObject", "has")    => NativeFn::JsonJObjectHas    as u32,
        ("JObject", "keys")   => NativeFn::JsonJObjectKeys   as u32,
        ("JObject", "length") => NativeFn::JsonJObjectLength as u32,
        // M35 P4-C: streaming Hasher.  The class is `is_native: true`
        // but we still route through this path (rather than
        // `resolve_native_method`) so the method names "update" /
        // "hexdigest" / "algorithm" don't have to compete with any
        // future stdlib class that uses the same names.
        ("Hasher",  "update")    => NativeFn::HasherUpdate    as u32,
        ("Hasher",  "hexdigest") => NativeFn::HasherHexdigest as u32,
        ("Hasher",  "algorithm") => NativeFn::HasherAlgorithm as u32,
        _ => return None,
    })
}

/// Pretty-print an IRFunction for debugging. Format is one line per value,
/// followed by the terminator. Designed for `eprintln!` in regression tests.
pub fn dump_function(f: &IRFunction) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let _ = writeln!(s, "fn {} (id={}) params={:?} ret={:?}", f.name, f.id.0, f.params, f.ret);
    for b in &f.blocks {
        let _ = writeln!(s, "  block {}:", b.id.0);
        for v in &b.values {
            let _ = writeln!(s, "    v{}: {:?} = {:?}", v.id, v.ty, v.kind);
        }
        let _ = writeln!(s, "    -> {:?}", b.terminator);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer, parser, resolver, typecheck};

    fn lower_src(src: &str) -> IRModule {
        let mut lx = lexer::Lexer::new(src);
        let mut toks = Vec::new();
        loop {
            let t = lx.next_token().unwrap();
            let eof = matches!(t.kind, lexer::TokenKind::Eof);
            toks.push(t);
            if eof { break; }
        }
        let module = parser::Parser::new(toks).parse_module().unwrap();
        let resolved = resolver::Resolver::new().resolve(module).unwrap();
        let typed = typecheck::TypeChecker::new().check(resolved).unwrap();
        lower(typed)
    }

    /// Regression: M3.5's local-slot lowering double-installed the implicit
    /// `self` of methods because the parser already records `self` as the
    /// first AST param. The second install overwrote slot 0 with `Unit`,
    /// turning every subsequent `self.field = ...` into a null-pointer
    /// store and segfaulting the VM. See M6 milestone notes.
    #[test]
    fn ctor_self_slot_not_clobbered() {
        let src = "\
final class Box:
    value: i64
    fn __init__(self, value: i64) -> None:
        self.value = value

fn main() -> i32:
    b: Box = Box(42)
    return 0
";
        let m = lower_src(src);
        let init = m
            .functions
            .iter()
            .find(|f| f.name == "Box.__init__")
            .expect("__init__ must be lowered");
        // Exactly two params: implicit `self` plus the user-declared `value`.
        assert_eq!(init.params.len(), 2, "got params: {:?}", init.params);
        assert!(matches!(init.params[0], Ty::Class(_)), "param[0] must be the class receiver, got {:?}", init.params[0]);
        assert!(matches!(init.params[1], Ty::Primitive(PrimTy::I64)), "param[1] = {:?}", init.params[1]);

        // The entry block must not contain any WriteLocal that overwrites
        // slot 0 with a Unit-typed operand (the old bug).
        let entry = &init.blocks[0];
        for v in &entry.values {
            if let ValueKind::Op { op: IROp::WriteLocal { slot: 0 }, args } = &v.kind {
                let src_val = args.first().expect("WriteLocal needs an operand");
                let src_ty = entry
                    .values
                    .iter()
                    .find(|sv| sv.id == src_val.0)
                    .map(|sv| sv.ty.clone());
                assert!(
                    !matches!(src_ty, Some(Ty::Primitive(PrimTy::Unit))),
                    "slot 0 must not be overwritten with a Unit value (saw {:?})",
                    src_ty,
                );
                let _ = src_val; // silence unused if branch falls through
            }
        }

        // main must allocate Box(42) and store the *Alloc* (not the
        // __init__'s Unit return value) into `b`'s slot.
        let main = m.functions.iter().find(|f| f.name == "main").expect("main");
        let mb = &main.blocks[0];
        let alloc_v = mb
            .values
            .iter()
            .find(|v| matches!(v.kind, ValueKind::Op { op: IROp::Alloc { .. }, .. }))
            .expect("main must contain an Alloc");
        let stored_into_b = mb.values.iter().find_map(|v| {
            if let ValueKind::Op { op: IROp::WriteLocal { .. }, args } = &v.kind {
                if args.first() == Some(&ValueId(alloc_v.id)) {
                    return Some(true);
                }
            }
            None
        });
        assert_eq!(stored_into_b, Some(true), "Alloc result must be WriteLocal'd into `b`");
    }

    /// Lambdas with free variables must be lifted into fresh module-level
    /// IRFunctions whose param list begins with the captures. The use site
    /// emits a `ClosureNew` carrying the capture values as operand args.
    #[test]
    fn lambda_lifts_free_var_into_capture_param() {
        let src = "\
fn main() -> i32:
    x: i32 = 5
    f: fn() -> i32 = fn() -> i32: x + 1
    return f()
";
        let m = lower_src(src);
        // Expect 2 IRFunctions: main + the lifted lambda.
        assert_eq!(m.functions.len(), 2, "want 2 fns, got {:?}",
            m.functions.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
        let lambda = m
            .functions
            .iter()
            .find(|f| f.name.starts_with("__lambda_"))
            .expect("lifted lambda function");
        // The lambda must have at least one capture param (`x`) at index 0.
        assert!(
            !lambda.params.is_empty(),
            "lambda must have at least one capture param, got {:?}",
            lambda.params,
        );
        assert!(matches!(lambda.params[0], Ty::Primitive(PrimTy::I32)), "lambda param[0] = {:?}", lambda.params[0]);

        // The use site in main must emit ClosureNew{fn_id == lambda.id}
        // with exactly one capture argument.
        let main = m.functions.iter().find(|f| f.name == "main").expect("main");
        let cn = main
            .blocks
            .iter()
            .flat_map(|b| b.values.iter())
            .find_map(|v| match &v.kind {
                ValueKind::Op { op: IROp::ClosureNew { fn_id, n_captures }, args } => {
                    Some((*fn_id, *n_captures, args.len()))
                }
                _ => None,
            })
            .expect("main must contain a ClosureNew");
        assert_eq!(cn.0.0, lambda.id.0, "fn_id must point at the lifted lambda");
        assert_eq!(cn.1, 1, "expect exactly one capture");
        assert_eq!(cn.2, 1, "expect exactly one capture arg");
    }

    /// tree.spy's `Branch.sum` calls `self.left.sum()` where `self.left`
    /// has static type `Node` (the open base class). The dispatch must go
    /// through a `VirtualCall` so subclass overrides (`Leaf.sum`,
    /// `Branch.sum`) actually run — devirtualising to `Node.sum` directly
    /// returns 0 and breaks the final sum (we saw `tree sum = 5` instead
    /// of `tree sum = 15`).
    #[test]
    fn open_class_method_call_is_virtual_not_direct() {
        let src = "\
open class Node:
    open fn sum(self) -> i64:
        return 0

final class Leaf(Node):
    value: i64
    fn __init__(self, value: i64) -> None:
        self.value = value
    fn sum(self) -> i64:
        return self.value

final class Branch(Node):
    left: Node
    value: i64
    fn __init__(self, left: Node, value: i64) -> None:
        self.left = left
        self.value = value
    fn sum(self) -> i64:
        return self.value + self.left.sum()

fn main() -> i32:
    return 0
";
        let m = lower_src(src);
        let branch_sum = m
            .functions
            .iter()
            .find(|f| f.name == "Branch.sum")
            .expect("Branch.sum lowered");
        // The body should contain at least one VirtualCall and no
        // DirectCall to Node.sum (which would devirt and skip overrides).
        let node_sum_id = m
            .functions
            .iter()
            .find(|f| f.name == "Node.sum")
            .map(|f| f.id.0)
            .expect("Node.sum lowered");
        let mut saw_virtual = false;
        for b in &branch_sum.blocks {
            for v in &b.values {
                if let ValueKind::Op { op: IROp::VirtualCall { .. }, .. } = &v.kind {
                    saw_virtual = true;
                }
                if let ValueKind::Op { op: IROp::DirectCall { fn_id }, .. } = &v.kind {
                    assert_ne!(
                        fn_id.0, node_sum_id,
                        "Branch.sum must NOT devirt to Node.sum"
                    );
                }
            }
        }
        assert!(saw_virtual, "Branch.sum must emit a VirtualCall for self.left.sum()");
    }

    /// producer.spy is the M6 acceptance program for threads + channels.
    /// The compiler must lower its two `Thread(fn() -> None: ...)` lambdas
    /// to real lifted IRFunctions with `ClosureNew` ops carrying the
    /// captured `ch` value. Whether the resulting bytecode actually *runs*
    /// is M6-B's territory (real threading); this test only asserts the
    /// compiler-side seam is wired.
    #[test]
    fn producer_lambdas_lift_to_real_fn_ids() {
        let src = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("examples/producer.spy"),
        )
        .expect("read producer.spy");
        let m = lower_src(&src);

        // Two lifted lambdas (one for producer, one for consumer).
        let lambdas: Vec<&IRFunction> = m
            .functions
            .iter()
            .filter(|f| f.name.starts_with("__lambda_"))
            .collect();
        assert_eq!(
            lambdas.len(),
            2,
            "expected 2 lifted lambdas, got names: {:?}",
            m.functions.iter().map(|f| &f.name).collect::<Vec<_>>(),
        );

        // main must contain two ClosureNew ops, each with a real
        // (non-u32::MAX) fn_id matching one of the lifted lambdas.
        let main = m.functions.iter().find(|f| f.name == "main").expect("main");
        let cns: Vec<(FuncId, u32)> = main
            .blocks
            .iter()
            .flat_map(|b| b.values.iter())
            .filter_map(|v| match &v.kind {
                ValueKind::Op { op: IROp::ClosureNew { fn_id, n_captures }, .. } => {
                    Some((*fn_id, *n_captures))
                }
                _ => None,
            })
            .collect();
        assert_eq!(cns.len(), 2, "main must emit 2 ClosureNew ops");
        for (fid, _) in &cns {
            assert_ne!(fid.0, u32::MAX, "ClosureNew must carry a real fn_id");
            assert!(
                lambdas.iter().any(|lf| lf.id.0 == fid.0),
                "ClosureNew fn_id {} not found in lifted lambda set",
                fid.0,
            );
        }
    }

    /// `for x: T in xs:` over a `List[T]` should desugar into the
    /// equivalent index-counted while-loop: i64 counter, ArrayLen,
    /// ILt header test, ArrayGet body load, IAdd-by-one step.
    ///
    /// real-world: every stress program (Game of Life, JSON parser,
    /// markov, ...) was hand-rolling this index loop. The lowering
    /// removes that boilerplate.
    #[test]
    fn for_in_list_desugars_to_indexed_while() {
        let src = "\
fn main() -> i32:
    xs: List[i64] = [10, 20, 30]
    total: i64 = 0
    for x: i64 in xs:
        total = total + x
    return 0
";
        let m = lower_src(src);
        let main = m.functions.iter().find(|f| f.name == "main").expect("main");

        let mut saw_arraylen = false;
        let mut saw_arrayget = false;
        let mut saw_ilt = false;
        let mut saw_iadd_step = false;
        for b in &main.blocks {
            for v in &b.values {
                if let ValueKind::Op { op, .. } = &v.kind {
                    match op {
                        IROp::ArrayLen => saw_arraylen = true,
                        IROp::ArrayGet => saw_arrayget = true,
                        IROp::ILt => saw_ilt = true,
                        // We can't easily distinguish the user-level
                        // `total + x` IAdd from the counter step, so
                        // just confirm at least one IAdd exists.
                        IROp::IAdd => saw_iadd_step = true,
                        _ => {}
                    }
                }
            }
        }
        assert!(saw_arraylen, "for-in must emit ArrayLen on the iterable");
        assert!(saw_arrayget, "for-in must emit ArrayGet inside the body");
        assert!(saw_ilt, "for-in must emit ILt for the header test");
        assert!(saw_iadd_step, "for-in must emit at least one IAdd (step or body)");

        // CondBranch with two distinct targets must exist (the loop
        // header).
        let has_condbr = main.blocks.iter().any(|b| {
            matches!(b.terminator, Terminator::CondBranch { .. })
        });
        assert!(has_condbr, "for-in must terminate the header with a CondBranch");
    }
}
