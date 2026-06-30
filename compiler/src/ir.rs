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
    self, BinOp as AstBinOp, Block, ComprehensionKind, ExceptHandler, Expr, FuncDecl, Literal,
    Lvalue, Span, Stmt, TopDecl, UnaryOp,
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

    // ── M62b: generators (yield) ─────────────────────────────────────────
    /// Allocate a generator object for generator function `fn_id`. Args are
    /// the evaluated call arguments (same shape as a `DirectCall`). Does not
    /// run the body. Lowered to `Opcode::MakeGen`.
    MakeGen { fn_id: FuncId },
    /// Produce `args[0]` from the current generator frame and suspend.
    /// Lowered to `Opcode::Yield`. Result type is Unit.
    Yield,
    /// Resume the generator `args[0]`; writes the yielded value into the
    /// destination register and the exhaustion flag (1 = done, 0 = produced a
    /// value) into local slot `done_slot`. Lowered to `Opcode::GenNext`.
    GenNext { done_slot: u16 },

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
                TopDecl::Const(_) => {
                    // Folded in Pass 1.5 below — a const initialiser may
                    // reference other consts in any declaration order, so
                    // a single in-order sweep is not enough.
                }
                _ => {}
            }
        }

        // Pass 1.5: fold every top-level `final` const to a literal IR
        // value so reference sites can substitute the constant directly.
        // Initialisers may reference other consts (`final B: f64 = A * 2.0`)
        // — including consts the module merger placed *after* the use —
        // so iterate to a fixed point rather than relying on declaration
        // order. Anything still unfolded after the fixed point was already
        // rejected by the typechecker (E3003), so no reference site can
        // observe a missing `module_consts` entry. (Historically only bare
        // literals were folded and `final SOLAR_MASS: f64 = 4.0 * PI * PI`
        // silently lowered to `None`/0.0 at every use.)
        let mut pending: Vec<_> = decls
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
                let ty = self
                    .lookup_ast_ty(&c.ty)
                    .unwrap_or(Ty::Primitive(PrimTy::Unit));
                match eval_const_expr(&c.value, &ty, &self.module_consts) {
                    Some(irc) => {
                        self.module_consts.insert(c.name.clone(), (irc, ty));
                    }
                    None => still_pending.push(c),
                }
            }
            pending = still_pending;
            if pending.len() == before {
                break;
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
        // Identify uniquely-owned `str` accumulators so `s = s + e` can lower
        // to an in-place append (amortised O(N)) instead of O(N) copies.
        fb.inplace_str_locals = self.str_inplace_candidates(f);
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

    /// Escape analysis for the in-place string-append optimisation. Returns the
    /// `str` locals that are uniquely-owned accumulators: every occurrence is an
    /// init, a self-append (`s = s + e` / `s += e`), or a bare `return s`.
    /// Anything that could alias the string or let it escape removes the name,
    /// so `StrAppendInPlace` can mutate its buffer in place soundly.
    fn str_inplace_candidates(&self, f: &FuncDecl) -> std::collections::HashSet<String> {
        let mut cand = std::collections::HashSet::new();
        self.collect_selfappend_candidates(&f.body, &mut cand);
        if cand.is_empty() {
            return cand;
        }
        let mut disq = std::collections::HashSet::new();
        disqualify_inplace_uses(&f.body, &cand, &mut disq);
        cand.retain(|n| !disq.contains(n));
        cand
    }

    /// Recorded type of `e` is `str` (direct lookup; the accumulator pattern is
    /// non-generic so no substitution is needed).
    fn expr_is_str(&self, e: &Expr) -> bool {
        let sp = expr_span(e);
        matches!(
            self.typed.expr_types.get(&(sp.start, sp.end)),
            Some(Ty::Primitive(PrimTy::Str))
        )
    }

    fn collect_selfappend_candidates(
        &self,
        block: &Block,
        out: &mut std::collections::HashSet<String>,
    ) {
        for s in &block.stmts {
            match s {
                Stmt::Assign { target: Lvalue::Ident { name, .. }, value, .. } => {
                    if is_str_self_append(value, name) && self.expr_is_str(value) {
                        out.insert(name.clone());
                    }
                }
                Stmt::AugAssign {
                    target: Lvalue::Ident { name, .. },
                    op: AstBinOp::Add,
                    value,
                    ..
                } => {
                    if !expr_mentions(value, name) && self.expr_is_str(value) {
                        out.insert(name.clone());
                    }
                }
                _ => {}
            }
            for b in stmt_child_blocks(s) {
                self.collect_selfappend_candidates(b, out);
            }
        }
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

/// True if `value` is a left-nested `name + e1 + e2 + ... + eK` Add chain
/// (K >= 1) rooted at `name`, with no `ei` mentioning `name` — the in-place-
/// appendable self-append shape. Strings round 2 widened this from the single
/// `name + e` form so chained appends (`s = s + a + b`) keep the amortised
/// O(N) path instead of silently reverting to O(n^2) StrConcat copies
/// (REPORT_V2 "Performance cliffs").
fn is_str_self_append(value: &Expr, name: &str) -> bool {
    str_self_append_operands(value, name).is_some()
}

/// If `value` is a left-nested `name + e1 + ... + eK` Add chain rooted at
/// `name` (each `ei` not mentioning `name`), return the appended operands in
/// source (append) order. `None` for any other shape — including a bare
/// `name` with no append, and a chain whose root is not exactly `name`.
///
/// The soundness contract matches the single-operand form exactly: every
/// returned operand is treated like the old `rhs` (it may not mention
/// `name`), and `disqualify_inplace_uses` walks the whole chain expression
/// via this same predicate, so other candidates appearing in any operand are
/// still disqualified.
fn str_self_append_operands<'a>(value: &'a Expr, name: &str) -> Option<Vec<&'a Expr>> {
    let mut rev_ops: Vec<&'a Expr> = Vec::new();
    let mut cur = value;
    loop {
        match cur {
            Expr::Binary { op: AstBinOp::Add, lhs, rhs, .. } => {
                if expr_mentions(rhs, name) {
                    return None;
                }
                rev_ops.push(rhs.as_ref());
                cur = lhs.as_ref();
            }
            Expr::Ident { name: l, .. } if l == name => {
                if rev_ops.is_empty() {
                    return None; // bare `s = s` is not an append
                }
                rev_ops.reverse();
                return Some(rev_ops);
            }
            _ => return None,
        }
    }
}

/// True if `name` appears anywhere in `e`. Exhaustive over `Expr` so adding a
/// variant forces an update (avoids silently missing a mention, which would be
/// unsound for the in-place-append gate).
fn expr_mentions(e: &Expr, name: &str) -> bool {
    match e {
        Expr::Literal { .. } => false,
        Expr::Ident { name: n, .. } => n == name,
        Expr::Tuple { elems, .. } | Expr::List { elems, .. } | Expr::Set { elems, .. } => {
            elems.iter().any(|x| expr_mentions(x, name))
        }
        Expr::Dict { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_mentions(k, name) || expr_mentions(v, name)),
        Expr::Unary { operand, .. } => expr_mentions(operand, name),
        Expr::Binary { lhs, rhs, .. } | Expr::NullCoalesce { lhs, rhs, .. } => {
            expr_mentions(lhs, name) || expr_mentions(rhs, name)
        }
        Expr::Call { callee, args, .. } => {
            expr_mentions(callee, name) || args.iter().any(|a| expr_mentions(&a.value, name))
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_mentions(receiver, name) || args.iter().any(|a| expr_mentions(&a.value, name))
        }
        Expr::Attr { obj, .. } => expr_mentions(obj, name),
        Expr::Index { obj, indices, .. } => {
            expr_mentions(obj, name) || indices.iter().any(|x| expr_mentions(x, name))
        }
        Expr::Ternary { cond, then_expr, else_expr, .. } => {
            expr_mentions(cond, name)
                || expr_mentions(then_expr, name)
                || expr_mentions(else_expr, name)
        }
        Expr::Lambda { body, .. } => expr_mentions(body, name),
        Expr::Cast { expr, .. } => expr_mentions(expr, name),
        Expr::Comprehension { iter, body, value, cond, .. } => {
            expr_mentions(iter, name)
                || expr_mentions(body, name)
                || value.as_ref().is_some_and(|v| expr_mentions(v, name))
                || cond.as_ref().is_some_and(|c| expr_mentions(c, name))
        }
        Expr::Slice { obj, lo, hi, step, .. } => {
            expr_mentions(obj, name)
                || lo.as_ref().is_some_and(|e| expr_mentions(e, name))
                || hi.as_ref().is_some_and(|e| expr_mentions(e, name))
                || step.as_ref().is_some_and(|e| expr_mentions(e, name))
        }
    }
}

/// Blocks directly owned by a statement (for recursion in candidate scan).
fn stmt_child_blocks(s: &Stmt) -> Vec<&Block> {
    let mut v = Vec::new();
    match s {
        Stmt::If { then_block, elifs, else_block, .. } => {
            v.push(then_block);
            for (_, b) in elifs {
                v.push(b);
            }
            if let Some(b) = else_block {
                v.push(b);
            }
        }
        Stmt::While { body, else_block, .. } | Stmt::For { body, else_block, .. } => {
            v.push(body);
            if let Some(b) = else_block {
                v.push(b);
            }
        }
        Stmt::Match { arms, .. } => {
            for a in arms {
                v.push(&a.body);
            }
        }
        Stmt::Try { body, handlers, else_block, finally_block, .. } => {
            v.push(body);
            for h in handlers {
                v.push(&h.body);
            }
            if let Some(b) = else_block {
                v.push(b);
            }
            if let Some(b) = finally_block {
                v.push(b);
            }
        }
        Stmt::With { body, .. } => v.push(body),
        _ => {}
    }
    v
}

fn pattern_binds(p: &ast::Pattern, name: &str) -> bool {
    match p {
        ast::Pattern::Identifier(n, _) => n == name,
        ast::Pattern::Constructor { fields, .. } => fields.iter().any(|f| pattern_binds(f, name)),
        ast::Pattern::Tuple(ps, _) => ps.iter().any(|f| pattern_binds(f, name)),
        _ => false,
    }
}

fn disq_expr(e: &Expr, cand: &std::collections::HashSet<String>, disq: &mut std::collections::HashSet<String>) {
    for c in cand {
        if !disq.contains(c) && expr_mentions(e, c) {
            disq.insert(c.clone());
        }
    }
}

/// Disqualify candidates mentioned in `e`, except `keep` (the allowed lhs of a
/// self-append).
fn disq_expr_except(
    e: &Expr,
    cand: &std::collections::HashSet<String>,
    disq: &mut std::collections::HashSet<String>,
    keep: &str,
) {
    for c in cand {
        if c != keep && !disq.contains(c) && expr_mentions(e, c) {
            disq.insert(c.clone());
        }
    }
}

/// Remove from candidacy any name with a use that could alias the string or let
/// it escape. Allowed uses: init/reassign (value not mentioning the name),
/// self-append `s = s + e` / `s += e`, and bare `return s`. Everything else
/// disqualifies — conservative, so in-place mutation can never corrupt an alias.
fn disqualify_inplace_uses(
    block: &Block,
    cand: &std::collections::HashSet<String>,
    disq: &mut std::collections::HashSet<String>,
) {
    for s in &block.stmts {
        match s {
            Stmt::Let { init, .. } => disq_expr(init, cand, disq),
            Stmt::LetDestructure { names, init, .. } => {
                disq_expr(init, cand, disq);
                for n in names {
                    if cand.contains(n) {
                        disq.insert(n.clone());
                    }
                }
            }
            Stmt::LetStarDestructure { before, star, after, init, .. } => {
                disq_expr(init, cand, disq);
                for n in before.iter().chain(std::iter::once(star)).chain(after.iter()) {
                    if cand.contains(n) {
                        disq.insert(n.clone());
                    }
                }
            }
            Stmt::Assign { target, value, .. } => match target {
                Lvalue::Ident { name, .. } => {
                    if cand.contains(name) && is_str_self_append(value, name) {
                        disq_expr_except(value, cand, disq, name);
                    } else {
                        disq_expr(value, cand, disq);
                    }
                }
                Lvalue::Attr { obj, .. } => {
                    disq_expr(obj, cand, disq);
                    disq_expr(value, cand, disq);
                }
                Lvalue::Index { obj, indices, .. } => {
                    disq_expr(obj, cand, disq);
                    for i in indices {
                        disq_expr(i, cand, disq);
                    }
                    disq_expr(value, cand, disq);
                }
            },
            Stmt::AugAssign { target, op, value, .. } => match target {
                Lvalue::Ident { name, .. }
                    if cand.contains(name)
                        && *op == AstBinOp::Add
                        && !expr_mentions(value, name) =>
                {
                    disq_expr_except(value, cand, disq, name);
                }
                Lvalue::Ident { name, .. } => {
                    if cand.contains(name) {
                        disq.insert(name.clone());
                    }
                    disq_expr(value, cand, disq);
                }
                Lvalue::Attr { obj, .. } => {
                    disq_expr(obj, cand, disq);
                    disq_expr(value, cand, disq);
                }
                Lvalue::Index { obj, indices, .. } => {
                    disq_expr(obj, cand, disq);
                    for i in indices {
                        disq_expr(i, cand, disq);
                    }
                    disq_expr(value, cand, disq);
                }
            },
            Stmt::Return { value: Some(e), .. } => {
                if !matches!(e, Expr::Ident { .. }) {
                    disq_expr(e, cand, disq);
                }
            }
            Stmt::Return { value: None, .. } => {}
            Stmt::Yield { value, .. } => disq_expr(value, cand, disq),
            Stmt::If { cond, then_block, elifs, else_block, .. } => {
                disq_expr(cond, cand, disq);
                disqualify_inplace_uses(then_block, cand, disq);
                for (ec, eb) in elifs {
                    disq_expr(ec, cand, disq);
                    disqualify_inplace_uses(eb, cand, disq);
                }
                if let Some(eb) = else_block {
                    disqualify_inplace_uses(eb, cand, disq);
                }
            }
            Stmt::While { cond, body, else_block, .. } => {
                disq_expr(cond, cand, disq);
                disqualify_inplace_uses(body, cand, disq);
                if let Some(eb) = else_block {
                    disqualify_inplace_uses(eb, cand, disq);
                }
            }
            Stmt::For { var, iter, body, else_block, .. } => {
                disq_expr(iter, cand, disq);
                if cand.contains(var) {
                    disq.insert(var.clone());
                }
                disqualify_inplace_uses(body, cand, disq);
                if let Some(eb) = else_block {
                    disqualify_inplace_uses(eb, cand, disq);
                }
            }
            Stmt::Match { scrutinee, arms, .. } => {
                disq_expr(scrutinee, cand, disq);
                for a in arms {
                    for c in cand {
                        if pattern_binds(&a.pattern, c) {
                            disq.insert(c.clone());
                        }
                    }
                    if let Some(g) = &a.guard {
                        disq_expr(g, cand, disq);
                    }
                    disqualify_inplace_uses(&a.body, cand, disq);
                }
            }
            Stmt::Try { body, handlers, else_block, finally_block, .. } => {
                disqualify_inplace_uses(body, cand, disq);
                for h in handlers {
                    if let Some(b) = &h.binding {
                        if cand.contains(b) {
                            disq.insert(b.clone());
                        }
                    }
                    disqualify_inplace_uses(&h.body, cand, disq);
                }
                if let Some(b) = else_block {
                    disqualify_inplace_uses(b, cand, disq);
                }
                if let Some(b) = finally_block {
                    disqualify_inplace_uses(b, cand, disq);
                }
            }
            Stmt::With { expr, binding, body, .. } => {
                disq_expr(expr, cand, disq);
                if let Some((n, _)) = binding {
                    if cand.contains(n) {
                        disq.insert(n.clone());
                    }
                }
                disqualify_inplace_uses(body, cand, disq);
            }
            Stmt::Raise { exc, cause, .. } => {
                disq_expr(exc, cand, disq);
                if let Some(c) = cause {
                    disq_expr(c, cand, disq);
                }
            }
            Stmt::Assert { cond, msg, .. } => {
                disq_expr(cond, cand, disq);
                if let Some(m) = msg {
                    disq_expr(m, cand, disq);
                }
            }
            Stmt::Del { target, .. } => match target {
                Lvalue::Ident { name, .. } => {
                    if cand.contains(name) {
                        disq.insert(name.clone());
                    }
                }
                Lvalue::Attr { obj, .. } => disq_expr(obj, cand, disq),
                Lvalue::Index { obj, indices, .. } => {
                    disq_expr(obj, cand, disq);
                    for i in indices {
                        disq_expr(i, cand, disq);
                    }
                }
            },
            Stmt::Expr { expr, .. } => disq_expr(expr, cand, disq),
            Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::Pass { .. } => {}
        }
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
    /// Local `str` variables proven (by `str_inplace_candidates`) to be
    /// uniquely-owned accumulators, so `s = s + e` / `s += e` can lower to an
    /// in-place `StrAppendInPlace` instead of an O(N)-copy `StrConcat`.
    inplace_str_locals: std::collections::HashSet<String>,
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
            inplace_str_locals: std::collections::HashSet::new(),
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
        Stmt::LetStarDestructure { before, star, after, init, .. } => {
            // Lane B: `before, *star, after = xs` where `xs: List[T]`.
            //
            //   list = <init>            (a List[T])
            //   len  = ArrayLen(list)
            //   before[i] = list[i]                          for i in 0..B
            //   after[j]  = list[len - A + j]                 for j in 0..A
            //   star = list[B : len - A]                      (fresh List[T])
            //
            // The star list is built with ArrayNew + a copy loop. Fixed
            // elements use direct ArrayGet (which bounds-checks, so a too-short
            // list traps with IndexError on the first missing element).
            let list = lower_expr(fb, ctx, init);
            let list_ty = find_value_ty(fb, list)
                .or_else(|| Some(ctx.expr_ty(expr_span(init))))
                .unwrap_or(Ty::Primitive(PrimTy::Unit));
            let elem_ty = match &list_ty {
                Ty::Generic { base: TypeCtor::List, args } if args.len() == 1 => args[0].clone(),
                _ => Ty::Primitive(PrimTy::Unit),
            };
            let i64_ty = Ty::Primitive(PrimTy::I64);
            let b = before.len() as i64;
            let a = after.len() as i64;

            let len = fb.push_value(
                i64_ty.clone(),
                ValueKind::Op { op: IROp::ArrayLen, args: vec![list] },
            );

            // before[i] = list[i]
            for (i, n) in before.iter().enumerate() {
                let idx = fb.push_value(
                    i64_ty.clone(),
                    ValueKind::Const(IRConst::I64(i as i64)),
                );
                let v = fb.push_value(
                    elem_ty.clone(),
                    ValueKind::Op { op: IROp::ArrayGet, args: vec![list, idx] },
                );
                let slot = fb.alloc_slot(n, elem_ty.clone());
                fb.emit_write_local(slot, v);
            }

            // star = list[B : len - A]  — fresh list, copy loop.
            let star_ty = Ty::Generic { base: TypeCtor::List, args: vec![elem_ty.clone()] };
            let star_list = fb.push_value(
                star_ty.clone(),
                ValueKind::Op { op: IROp::ArrayNew, args: vec![] },
            );
            let star_slot = fb.alloc_slot(star, star_ty.clone());
            fb.emit_write_local(star_slot, star_list);

            // stop = len - A
            let a_const = fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(a)));
            let stop = fb.push_value(
                i64_ty.clone(),
                ValueKind::Op { op: IROp::ISub, args: vec![len, a_const] },
            );
            let stop_slot = {
                let nm = format!("__star_stop_{}", fb.slot_ty.len());
                let s = fb.alloc_slot(&nm, i64_ty.clone());
                fb.emit_write_local(s, stop);
                s
            };
            // i = B
            let i_slot = {
                let nm = format!("__star_i_{}", fb.slot_ty.len());
                let s = fb.alloc_slot(&nm, i64_ty.clone());
                let b_const = fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(b)));
                fb.emit_write_local(s, b_const);
                s
            };

            let header = fb.new_block();
            let body_b = fb.new_block();
            let exit = fb.new_block();
            fb.terminate(Terminator::Branch { target: header });

            // header: while i < stop
            fb.switch_to(header);
            let i_cur = fb.emit_read_local(i_slot);
            let stop_cur = fb.emit_read_local(stop_slot);
            let test = fb.push_value(
                Ty::Primitive(PrimTy::Bool),
                ValueKind::Op { op: IROp::ILt, args: vec![i_cur, stop_cur] },
            );
            fb.terminate(Terminator::CondBranch { cond: test, t: body_b, f: exit });

            // body: star.push(list[i]); i += 1
            fb.switch_to(body_b);
            let i_now = fb.emit_read_local(i_slot);
            let elt = fb.push_value(
                elem_ty.clone(),
                ValueKind::Op { op: IROp::ArrayGet, args: vec![list, i_now] },
            );
            let star_v = fb.emit_read_local(star_slot);
            fb.push_value(
                Ty::Primitive(PrimTy::Unit),
                ValueKind::Op { op: IROp::ListPush, args: vec![star_v, elt] },
            );
            let i_again = fb.emit_read_local(i_slot);
            let one = fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(1)));
            let next_i = fb.push_value(
                i64_ty.clone(),
                ValueKind::Op { op: IROp::IAdd, args: vec![i_again, one] },
            );
            fb.emit_write_local(i_slot, next_i);
            fb.terminate(Terminator::Branch { target: header });

            // exit: after[j] = list[len - A + j]
            fb.switch_to(exit);
            for (j, n) in after.iter().enumerate() {
                // idx = len - A + j  = stop + j
                let stop_v = fb.emit_read_local(stop_slot);
                let j_const = fb.push_value(
                    i64_ty.clone(),
                    ValueKind::Const(IRConst::I64(j as i64)),
                );
                let idx = fb.push_value(
                    i64_ty.clone(),
                    ValueKind::Op { op: IROp::IAdd, args: vec![stop_v, j_const] },
                );
                let v = fb.push_value(
                    elem_ty.clone(),
                    ValueKind::Op { op: IROp::ArrayGet, args: vec![list, idx] },
                );
                let slot = fb.alloc_slot(n, elem_ty.clone());
                fb.emit_write_local(slot, v);
            }
            Some(())
        }
        Stmt::Assign { target, value, .. } => {
            // In-place string accumulator: `s = s + e1 + ... + eK` (a
            // left-nested Add chain rooted at `s`) where `s` is a
            // proven-unique local (see `str_inplace_candidates`) lowers to a
            // sequence of StrAppendInPlace(s, ei) calls — amortised O(N) —
            // instead of fresh O(len) StrConcat copies each step. Strings
            // round 2 widened this from the single `s = s + e` form.
            if let Lvalue::Ident { name, .. } = target {
                if fb.inplace_str_locals.contains(name) {
                    if let Some(ops) = str_self_append_operands(value, name) {
                        let cur = lower_lvalue_load(fb, ctx, target);
                        // Evaluate every appended operand BEFORE the first
                        // in-place append (operands keep source order). If an
                        // operand raises, `s`'s buffer is still untouched —
                        // identical observable semantics to the StrConcat
                        // fallback. The appends themselves cannot raise a
                        // catchable exception.
                        let evs: Vec<ValueId> =
                            ops.iter().map(|e| lower_expr(fb, ctx, e)).collect();
                        let mut acc = cur;
                        for ev in evs {
                            acc = fb.push_value(
                                Ty::Primitive(PrimTy::Str),
                                ValueKind::Op {
                                    op: IROp::NativeCall {
                                        native_id: NativeFn::StrAppendInPlace as u32,
                                    },
                                    args: vec![acc, ev],
                                },
                            );
                        }
                        lower_lvalue_store(fb, ctx, target, acc);
                        return Some(());
                    }
                }
            }
            let v = lower_expr(fb, ctx, value);
            lower_lvalue_store(fb, ctx, target, v);
            Some(())
        }
        Stmt::AugAssign { target, op, value, .. } => {
            // `s += e` on a proven-unique str local: same in-place append.
            if let Lvalue::Ident { name, .. } = target {
                if *op == AstBinOp::Add && fb.inplace_str_locals.contains(name) {
                    let cur = lower_lvalue_load(fb, ctx, target);
                    let ev = lower_expr(fb, ctx, value);
                    let appended = fb.push_value(
                        Ty::Primitive(PrimTy::Str),
                        ValueKind::Op {
                            op: IROp::NativeCall {
                                native_id: NativeFn::StrAppendInPlace as u32,
                            },
                            args: vec![cur, ev],
                        },
                    );
                    lower_lvalue_store(fb, ctx, target, appended);
                    return Some(());
                }
            }
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
        Stmt::Yield { value, .. } => {
            // M62b: `yield <expr>` — produce a value from the current
            // generator frame and suspend. Lowered to a single `Yield`
            // instruction carrying the value register. The VM saves the live
            // frame (registers + pc) back into the owning generator object and
            // returns control to the `GenNext` driver. On the next resume,
            // execution continues at the instruction *after* this Yield, so
            // (unlike Return) we keep the current block open.
            let v = lower_expr(fb, ctx, value);
            fb.push_value(
                Ty::Primitive(PrimTy::Unit),
                ValueKind::Op { op: IROp::Yield, args: vec![v] },
            );
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

            // `for v: i64 in range(...)` — lower to a lazy integer counter loop
            // instead of materialising the range. Previously this hit the
            // run-once placeholder below (range's static type is `Range`, not
            // `List`), so the body executed exactly once with a garbage loop
            // var; the runtime range() also capped at 1M elements. The counter
            // loop is unbounded and never allocates. Handles range(stop),
            // range(start, stop), and range(start, stop, step) including a
            // negative constant step.
            if matches!(&iter_ty, Ty::Generic { base: TypeCtor::Range, .. }) {
                if let Some(rargs) = range_call_args(iter) {
                    let i64_ty = Ty::Primitive(PrimTy::I64);
                    // start / stop value, and the step expression (if explicit).
                    let (start_v, stop_v, step_expr): (ValueId, ValueId, Option<&Expr>) =
                        match rargs.len() {
                            1 => (
                                fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(0))),
                                lower_expr(fb, ctx, &rargs[0].value),
                                None,
                            ),
                            2 => (
                                lower_expr(fb, ctx, &rargs[0].value),
                                lower_expr(fb, ctx, &rargs[1].value),
                                None,
                            ),
                            _ => (
                                lower_expr(fb, ctx, &rargs[0].value),
                                lower_expr(fb, ctx, &rargs[1].value),
                                Some(&rargs[2].value),
                            ),
                        };
                    // Step value + its compile-time sign when it's a literal.
                    let (step_v, const_step): (ValueId, Option<i64>) = match step_expr {
                        None => (
                            fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(1))),
                            Some(1),
                        ),
                        Some(e) => (lower_expr(fb, ctx, e), const_i64(e)),
                    };

                    // Internal counter + invariant stop/step slots + user var.
                    let cnt_slot = {
                        let n = format!("__range_i_{}", fb.slot_ty.len());
                        let s = fb.alloc_slot(&n, i64_ty.clone());
                        fb.emit_write_local(s, start_v);
                        s
                    };
                    let stop_slot = {
                        let n = format!("__range_stop_{}", fb.slot_ty.len());
                        let s = fb.alloc_slot(&n, i64_ty.clone());
                        fb.emit_write_local(s, stop_v);
                        s
                    };
                    let step_slot = {
                        let n = format!("__range_step_{}", fb.slot_ty.len());
                        let s = fb.alloc_slot(&n, i64_ty.clone());
                        fb.emit_write_local(s, step_v);
                        s
                    };
                    let var_slot = fb.alloc_slot(var, i64_ty.clone());

                    let header = fb.new_block();
                    let body_b = fb.new_block();
                    let latch = fb.new_block();
                    let exit = fb.new_block();
                    fb.terminate(Terminator::Branch { target: header });

                    // header: continue-condition.
                    fb.switch_to(header);
                    let i_cur = fb.emit_read_local(cnt_slot);
                    let stop_cur = fb.emit_read_local(stop_slot);
                    let cond = match const_step {
                        Some(c) if c > 0 => {
                            emit_binop(fb, AstBinOp::Lt, i_cur, stop_cur, i64_ty.clone())
                        }
                        Some(c) if c < 0 => {
                            emit_binop(fb, AstBinOp::Gt, i_cur, stop_cur, i64_ty.clone())
                        }
                        // step == 0: zero iterations (avoids an infinite loop;
                        // the materialising range() would raise ValueError).
                        Some(_) => fb.push_value(
                            Ty::Primitive(PrimTy::Bool),
                            ValueKind::Const(IRConst::Bool(false)),
                        ),
                        // Non-literal step: continue while `(stop - i) * step > 0`,
                        // which is correct for either sign (and 0 → 0 iters).
                        None => {
                            let step_cur = fb.emit_read_local(step_slot);
                            let diff =
                                emit_binop(fb, AstBinOp::Sub, stop_cur, i_cur, i64_ty.clone());
                            let prod =
                                emit_binop(fb, AstBinOp::Mul, diff, step_cur, i64_ty.clone());
                            let zero = fb.push_value(
                                i64_ty.clone(),
                                ValueKind::Const(IRConst::I64(0)),
                            );
                            emit_binop(fb, AstBinOp::Gt, prod, zero, i64_ty.clone())
                        }
                    };
                    fb.terminate(Terminator::CondBranch { cond, t: body_b, f: exit });

                    // body: bind the user var to the counter, run body, go to latch.
                    fb.switch_to(body_b);
                    let i_for_var = fb.emit_read_local(cnt_slot);
                    fb.emit_write_local(var_slot, i_for_var);
                    // `continue` must land on the latch so the counter still steps.
                    fb.loop_stack.push((latch, exit));
                    lower_block(fb, ctx, body);
                    fb.loop_stack.pop();
                    fb.terminate(Terminator::Branch { target: latch });

                    // latch: i += step; back to header.
                    fb.switch_to(latch);
                    let i_inc = fb.emit_read_local(cnt_slot);
                    let step_inc = fb.emit_read_local(step_slot);
                    let nxt = emit_binop(fb, AstBinOp::Add, i_inc, step_inc, i64_ty.clone());
                    fb.emit_write_local(cnt_slot, nxt);
                    fb.terminate(Terminator::Branch { target: header });

                    fb.switch_to(exit);
                    return Some(());
                }
            }

            // M62b: `for v: T in gen():` over a generator (`Iterator[T]`).
            // Desugar into a GenNext-driven loop:
            //
            //     __g = <iter>            # MakeGen — allocates the generator
            //     __done: bool = false
            //     loop:
            //         v = GenNext(__g)    # also writes __done
            //         if __done: break
            //         <body>
            //
            // The VM resumes the saved generator frame on each GenNext until
            // it yields (writes the value, __done = 0) or finishes (__done = 1).
            //
            // Guard: only take this path when the iterable is a *direct call to
            // a generator function* (so `iter` lowers to a real `MakeGen`).
            // Other `Iterator[T]` shapes (e.g. `Dict.items()`) are a different
            // runtime representation and must NOT be driven via `GenNext` — they
            // fall through to the existing placeholder behaviour, exactly as
            // before M62b.
            if matches!(&iter_ty, Ty::Generic { base: TypeCtor::Iterator, .. })
                && iter_is_generator_call(ctx, iter)
            {
                let args = match &iter_ty {
                    Ty::Generic { base: TypeCtor::Iterator, args } => args.clone(),
                    _ => Vec::new(),
                };
                let args = &args;
                let elem_ty = args
                    .first()
                    .cloned()
                    .or_else(|| {
                        ctx.typed.resolved.ast_type_to_ty
                            .get(&(ast_type_span(var_ty).start, ast_type_span(var_ty).end))
                            .cloned()
                    })
                    .unwrap_or(Ty::Primitive(PrimTy::Unit));

                // Materialise the generator once, before the loop.
                let gen_v = lower_expr(fb, ctx, iter);
                let gen_ty = find_value_ty(fb, gen_v).unwrap_or_else(|| iter_ty.clone());
                let gen_slot = {
                    let n = format!("__for_gen_{}", fb.slot_ty.len());
                    let s = fb.alloc_slot(&n, gen_ty);
                    fb.emit_write_local(s, gen_v);
                    s
                };
                // Dedicated `done` slot the GenNext op writes into.
                let bool_ty = Ty::Primitive(PrimTy::Bool);
                let done_slot = {
                    let n = format!("__for_done_{}", fb.slot_ty.len());
                    fb.alloc_slot(&n, bool_ty.clone())
                };
                // User-visible loop variable.
                let var_slot = fb.alloc_slot(var, elem_ty.clone());

                let header = fb.new_block();
                let body_b = fb.new_block();
                let exit = fb.new_block();
                fb.terminate(Terminator::Branch { target: header });

                // header: v = GenNext(__g) [writes __done]; if __done -> exit
                fb.switch_to(header);
                let gen_cur = fb.emit_read_local(gen_slot);
                let nxt = fb.push_value(
                    elem_ty.clone(),
                    ValueKind::Op {
                        op: IROp::GenNext { done_slot },
                        args: vec![gen_cur],
                    },
                );
                fb.emit_write_local(var_slot, nxt);
                let done_cur = fb.emit_read_local(done_slot);
                // Branch: if done (true/non-zero) -> exit, else -> body.
                fb.terminate(Terminator::CondBranch { cond: done_cur, t: exit, f: body_b });

                // body: <body>; loop back to header.
                fb.switch_to(body_b);
                fb.loop_stack.push((header, exit));
                lower_block(fb, ctx, body);
                fb.loop_stack.pop();
                fb.terminate(Terminator::Branch { target: header });

                fb.switch_to(exit);
                return Some(());
            }

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
        Stmt::Try { body, handlers, else_block, finally_block, .. } => {
            lower_try(fb, ctx, body, handlers, else_block.as_ref(), finally_block.as_ref());
            Some(())
        }
        Stmt::Raise { exc, cause, .. } => {
            lower_raise(fb, ctx, exc, cause.as_ref());
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
        Stmt::Del { target, .. } => {
            // Spec §7.5: `del d[k]` removes a Dict entry. The typechecker
            // rejects every other del target (this arm used to lower the
            // statement to nothing, so deletion silently no-opped). The
            // native returns a bool (key was present) that the statement
            // form discards.
            if let Lvalue::Index { obj, indices, .. } = target {
                let recv_ty = ctx.expr_ty(expr_span(obj));
                if matches!(recv_ty, Ty::Generic { base: TypeCtor::Dict, .. }) {
                    let d = lower_expr(fb, ctx, obj);
                    let k = if let Some(i) = indices.first() {
                        lower_expr(fb, ctx, i)
                    } else {
                        return Some(());
                    };
                    fb.push_value(
                        Ty::Primitive(PrimTy::Bool),
                        ValueKind::Op {
                            op: IROp::NativeCall { native_id: NativeFn::DictRemove as u32 },
                            args: vec![d, k],
                        },
                    );
                }
            }
            Some(())
        }
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
    else_block: Option<&Block>,
    finally_block: Option<&Block>,
) {
    let after = fb.new_block();
    let finally_b: Option<BlockId> = finally_block.as_ref().map(|_| fb.new_block());
    // The `else` clause runs iff the try body completes with NO exception. It
    // is *not* protected by the handler frame (an exception inside `else`
    // propagates past this try). We give it its own block reached only from the
    // body's normal-completion edge; the handler arms branch straight to the
    // post-body target and skip it.
    let else_b: Option<BlockId> = else_block.as_ref().map(|_| fb.new_block());

    // Allocate handler blocks AND their bind slots up front, so the TryEnter
    // operand can reference them.
    //
    // M63a: the VM's handler matcher (`interp::propagate_exception`) compares
    // the raised exception's `type_name` string against each arm's filter for
    // *exact* equality (with `"Exception"` as a catch-all).  To make
    // `except Base` also catch a raised `Derived` — where `Derived` subclasses
    // `Base` — we expand a single source `except` clause into one VM arm per
    // matching type name: the filter class itself plus every user exception
    // class that transitively descends from it.  All expanded arms for one
    // clause share the same handler block and bind slot, and we keep them
    // contiguous and in source-clause order so the VM's first-match scan
    // preserves Python's "first matching except wins" semantics.  The
    // `"Exception"` catch-all needs no expansion (the VM matches it against
    // anything), and built-in exception aliases are handled VM-side.
    let mut handler_blocks: Vec<BlockId> = Vec::with_capacity(handlers.len());
    let mut arms: Vec<TryHandlerArm> = Vec::with_capacity(handlers.len());
    for h in handlers {
        let block = fb.new_block();
        handler_blocks.push(block);
        let bind_slot = if let Some(name) = &h.binding {
            // Bind slot type: the caught exception class (best-effort —
            // pulled from the AST type via the resolver's type lookup).
            let slot_ty = exception_filter_ty(ctx, &h.exc_ty);
            fb.alloc_slot(name, slot_ty) as u32
        } else {
            u32::MAX
        };
        // Filter name(s) for this clause — one for `except E:`, several for a
        // tuple `except (A, B):`. Each name additionally pulls in its subclass
        // names so `except Base` (or `except (Base, Other)`) also catches a
        // raised `Derived`. All expanded arms share this clause's handler block
        // and bind slot, and stay contiguous in clause order so the VM's
        // first-match scan preserves "first matching except wins".
        let mut match_names: Vec<String> = Vec::new();
        for filter_name in exception_filter_names(&h.exc_ty) {
            if !match_names.contains(&filter_name) {
                match_names.push(filter_name.clone());
            }
            if filter_name != "Exception" {
                for sub in exception_subclass_names(ctx, &filter_name) {
                    if !match_names.contains(&sub) {
                        match_names.push(sub);
                    }
                }
            }
        }
        for mn in match_names {
            let idx = ctx.intern(&mn);
            arms.push(TryHandlerArm {
                filter_str_idx: idx,
                handler_block: block,
                bind_slot,
            });
        }
    }

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
    // Normal-completion path: pop the handler frame (TryLeave), then run the
    // `else` clause (if any), then branch to finally (if any) else `after`.
    // The `else` clause runs *after* TryLeave so that an exception it raises is
    // NOT caught by this try's handlers — exactly Python's semantics.
    fb.push_value(
        Ty::Primitive(PrimTy::Unit),
        ValueKind::Op { op: IROp::TryLeave, args: vec![] },
    );
    let post_body_target = finally_b.unwrap_or(after);
    // Where the body's normal exit goes: into `else` if present, else straight
    // to the post-body (finally/after) target.
    let normal_exit_target = else_b.unwrap_or(post_body_target);
    fb.terminate(Terminator::Branch { target: normal_exit_target });

    // `else` block (if present): runs only on the body's no-exception path.
    if let (Some(eb_id), Some(eb)) = (else_b, else_block) {
        fb.switch_to(eb_id);
        lower_block(fb, ctx, eb);
        fb.terminate(Terminator::Branch { target: post_body_target });
    }

    // Handler arms — each starts in its own block, entered only via the VM's
    // exception dispatch. After running, branch to finally (if any) else after.
    // Handlers deliberately bypass the `else` block.
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
fn lower_raise(fb: &mut FuncBuilder, ctx: &mut LowerCtx, exc: &Expr, cause: Option<&Expr>) {
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
                    // message field (offset 8). `raise X("msg") from Y`
                    // appends a chained-cause suffix so the cause is preserved
                    // and observable via `e.message` (the 2-field exception
                    // object has no dedicated `__cause__` slot — full
                    // `__cause__` storage is deferred; see STRICTPY_SPEC §7.5).
                    let mut msg_v = lower_expr(fb, ctx, &args[0].value);
                    if let Some(c) = cause {
                        msg_v = lower_cause_chain(fb, ctx, msg_v, c);
                    }
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
    // Evaluate the cause too (if any) so its side effects are preserved and it
    // is not silently dropped; with a non-literal raised object we can't fold
    // the chain into its message without mutating a possibly-shared object, so
    // for this path we only guarantee evaluation. (`raise Builtin(..) from Y`
    // — the common case — does fold the chain in, above.)
    let v = lower_expr(fb, ctx, exc);
    if let Some(c) = cause {
        let _ = lower_expr(fb, ctx, c);
    }
    fb.terminate(Terminator::Throw { exc: v });
    let nb = fb.new_block();
    fb.switch_to(nb);
}

/// Build a chained-cause message value for `raise X(msg) from cause`.
///
/// Produces `"<msg> [caused by <CauseType>: <cause msg>]"` by concatenating
/// the base message with the cause exception's `type_name` (offset 0) and
/// `message` (offset 8) fields. `cause` must be an exception-typed value (the
/// typechecker enforces this in `Stmt::Raise`). Until the exception object
/// carries a real `__cause__` slot, this keeps the cause observable rather
/// than silently dropping it.
fn lower_cause_chain(
    fb: &mut FuncBuilder,
    ctx: &mut LowerCtx,
    base_msg: ValueId,
    cause: &Expr,
) -> ValueId {
    let cause_v = lower_expr(fb, ctx, cause);
    let str_ty = Ty::Primitive(PrimTy::Str);
    let mk_str = |fb: &mut FuncBuilder, s: &str| {
        fb.push_value(str_ty.clone(), ValueKind::Const(IRConst::Str(s.to_string())))
    };
    let concat = |fb: &mut FuncBuilder, a: ValueId, b: ValueId| {
        fb.push_value(
            str_ty.clone(),
            ValueKind::Op {
                op: IROp::NativeCall { native_id: NativeFn::StrConcat as u32 },
                args: vec![a, b],
            },
        )
    };
    // Load cause.type_name (offset 0) and cause.message (offset 8).
    let cause_tname = fb.push_value(
        str_ty.clone(),
        ValueKind::Op { op: IROp::Load { offset: 0 }, args: vec![cause_v] },
    );
    let cause_msg = fb.push_value(
        str_ty.clone(),
        ValueKind::Op { op: IROp::Load { offset: 8 }, args: vec![cause_v] },
    );
    let pre = mk_str(fb, " [caused by ");
    let sep = mk_str(fb, ": ");
    let post = mk_str(fb, "]");
    let mut acc = concat(fb, base_msg, pre);
    acc = concat(fb, acc, cause_tname);
    acc = concat(fb, acc, sep);
    acc = concat(fb, acc, cause_msg);
    acc = concat(fb, acc, post);
    acc
}

/// Extract the filter name(s) from an `except T as e:` clause's AST type.
///
/// - `except E:`        → `["E"]`
/// - `except (A, B):`   → `["A", "B"]`  (a tuple of exception types — each is
///                         matched independently, exactly like CPython)
/// - anything else      → `["Exception"]` (catch-all) — defensive.
///
/// Historically only the `Named` case was handled and a tuple silently
/// degraded to the universal `"Exception"` catch-all, swallowing exceptions
/// the program never meant to catch.  We now lower each element of the tuple
/// to its own VM handler arm (see `lower_try`), so `except (A, B)` catches A
/// or B but *not* an unrelated C.
fn exception_filter_names(ty: &ast::Type) -> Vec<String> {
    match ty {
        ast::Type::Named { name, .. } => vec![name.clone()],
        ast::Type::Tuple { elems, .. } => {
            let mut names = Vec::with_capacity(elems.len());
            for e in elems {
                if let ast::Type::Named { name, .. } = e {
                    names.push(name.clone());
                } else {
                    // A non-Named element inside the tuple is unexpected
                    // (the typechecker rejects non-exception filters); fall
                    // back to catch-all so we never silently drop the clause.
                    names.push("Exception".into());
                }
            }
            if names.is_empty() {
                vec!["Exception".into()]
            } else {
                names
            }
        }
        _ => vec!["Exception".into()],
    }
}

/// M63a: collect the names of every class in the module whose `base` chain
/// transitively reaches the class named `filter_name` (excluding `filter_name`
/// itself).  Used by `lower_try` to expand a single `except Base` clause into
/// one VM handler arm per matching `Derived` type, so the VM's exact-string
/// matcher catches subclasses.  Order is deterministic (sorted by class id)
/// so bytecode output is stable across runs.
fn exception_subclass_names(ctx: &LowerCtx, filter_name: &str) -> Vec<String> {
    // Resolve the filter class id (a user or built-in exception class).
    let base_cid = ctx
        .class_layouts
        .iter()
        .find(|(_, l)| l.name == filter_name)
        .map(|(id, _)| *id);
    let base_cid = match base_cid {
        Some(c) => c,
        None => return Vec::new(),
    };
    let mut hits: Vec<(u32, String)> = Vec::new();
    for (cid, layout) in ctx.class_layouts.iter() {
        if *cid == base_cid {
            continue;
        }
        // Walk this class's ancestry; record it if `base_cid` is an ancestor.
        let mut cur = layout.base;
        let mut depth = 0;
        while let Some(anc) = cur {
            if anc == base_cid {
                hits.push((cid.0, layout.name.clone()));
                break;
            }
            cur = ctx.class_layouts.get(&anc).and_then(|l| l.base);
            depth += 1;
            if depth > 1024 {
                break; // defensive cycle guard
            }
        }
    }
    hits.sort_by_key(|(id, _)| *id);
    hits.into_iter().map(|(_, name)| name).collect()
}

/// Resolve the static type used for the exception-binding slot. Pulled from
/// the resolver's `ast_type_to_ty` so the slot's type matches what the
/// typechecker recorded.
fn exception_filter_ty(ctx: &LowerCtx, ty: &ast::Type) -> Ty {
    // For `except (A, B) as e:` there is no single class span recorded for the
    // tuple; key the bind slot off the first listed type so the slot is at
    // least a heap reference (8-byte) and the GC scans it correctly. The bind
    // value at runtime is the original thrown object regardless of slot type.
    let lookup_ty: &ast::Type = match ty {
        ast::Type::Tuple { elems, .. } => elems.first().unwrap_or(ty),
        other => other,
    };
    let key = (ast_type_span(lookup_ty).start, ast_type_span(lookup_ty).end);
    ctx.typed
        .resolved
        .ast_type_to_ty
        .get(&key)
        .cloned()
        .unwrap_or(Ty::Primitive(PrimTy::Unit))
}

// ─────────────────────────────────────────────────────────────────────────
//  Negative-index normalization (Lane B)
// ─────────────────────────────────────────────────────────────────────────

/// Normalize a (possibly negative) list index so that `-1` maps to the last
/// element, `-2` to the second-to-last, etc. — matching Python semantics.
///
/// Emits `idx < 0 ? idx + len(arr) : idx` using `ArrayLen` + `Select`, so the
/// adjusted index is computed entirely in IR. This works on BOTH the
/// interpreter and the JIT (`Op::ArrayGet`/`Op::ArraySet` skip bounds checks
/// and never adjusted for negatives), without touching `vm/src/jit.rs`.
///
/// `idx` is an already-lowered SSA value; `arr` is the already-lowered list
/// pointer value. Non-negative indices flow through unchanged. Out-of-range
/// indices (after adjustment) still trap in the VM's bounds check exactly as
/// before. Strings are handled in the `StrCharAt` native instead (see
/// `vm/src/builtins.rs`) because string indexing never reaches the JIT.
fn normalize_neg_list_index(fb: &mut FuncBuilder, arr: ValueId, idx: ValueId) -> ValueId {
    let i64ty = Ty::Primitive(PrimTy::I64);
    // Pre-seed a slot with the non-negative-path value (`idx` unchanged).
    // `IROp::Select` CANNOT be used here: its codegen unconditionally moves
    // the `then` operand (see codegen.rs), which would corrupt non-negative
    // indices into `idx + len`. We use a real `CondBranch` + slot merge — the
    // same pattern `NullCoalesce` uses.
    let nm = format!("__idx_norm_{}", fb.slot_ty.len());
    let slot = fb.alloc_slot(&nm, i64ty.clone());
    fb.emit_write_local(slot, idx);

    let zero = fb.push_value(i64ty.clone(), ValueKind::Const(IRConst::I64(0)));
    let is_neg = fb.push_value(
        Ty::Primitive(PrimTy::Bool),
        ValueKind::Op { op: IROp::ILt, args: vec![idx, zero] },
    );

    let neg_b = fb.new_block();
    let merge = fb.new_block();
    fb.terminate(Terminator::CondBranch { cond: is_neg, t: neg_b, f: merge });

    // Negative branch: idx += len.
    fb.switch_to(neg_b);
    let len = fb.push_value(
        i64ty.clone(),
        ValueKind::Op { op: IROp::ArrayLen, args: vec![arr] },
    );
    let idx_plus = fb.push_value(
        i64ty.clone(),
        ValueKind::Op { op: IROp::IAdd, args: vec![idx, len] },
    );
    fb.emit_write_local(slot, idx_plus);
    fb.terminate(Terminator::Branch { target: merge });

    fb.switch_to(merge);
    fb.emit_read_local(slot)
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
                // Wave 2 / Lane C: aug-assign read `obj[k] += ..` on a user class
                // routes the load through `__getitem__(self, k)` (plain or
                // generic class receiver).
                _ if class_index_dunder_op(
                    ctx.class_layouts, ctx.fn_id_by_name, ctx.class_inst_method_fn,
                    &recv_ty, "__getitem__").is_some() =>
                {
                    let op = class_index_dunder_op(
                        ctx.class_layouts, ctx.fn_id_by_name, ctx.class_inst_method_fn,
                        &recv_ty, "__getitem__").expect("guard above proved Some");
                    fb.push_value(ty, ValueKind::Op { op, args: vec![arr, idx] })
                }
                _ => {
                    // Lane B: support `xs[-1]` (read for aug-assign) too.
                    let idx = normalize_neg_list_index(fb, arr, idx);
                    fb.push_value(
                        ty,
                        ValueKind::Op { op: IROp::ArrayGet, args: vec![arr, idx] },
                    )
                }
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
                // Wave 2 / Lane C: `obj[k] = v` on a user class dispatches to
                // `__setitem__(self, k, v)`. The typechecker has already verified
                // both the key and value types. Covers plain and generic classes.
                _ if class_index_dunder_op(
                    ctx.class_layouts, ctx.fn_id_by_name, ctx.class_inst_method_fn,
                    &recv_ty, "__setitem__").is_some() =>
                {
                    let op = class_index_dunder_op(
                        ctx.class_layouts, ctx.fn_id_by_name, ctx.class_inst_method_fn,
                        &recv_ty, "__setitem__").expect("guard above proved Some");
                    fb.push_value(
                        Ty::Primitive(PrimTy::Unit),
                        ValueKind::Op { op, args: vec![arr, idx, v] },
                    );
                }
                _ => {
                    // Lane B: support `xs[-1] = v` by normalizing negatives.
                    let idx = normalize_neg_list_index(fb, arr, idx);
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
        | Expr::Cast { span, .. }
        | Expr::Slice { span, .. }
        | Expr::Comprehension { span, .. } => *span,
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
            // Bare reference to a module-scope function used as a value
            // (`asyncio.spawn_i32(do_work)`, `map(double, xs)`, or binding
            // `f: fn() -> i32 = worker`). Materialise a zero-capture
            // ClosureNew so every fn-typed value is uniformly a ClosureRepr
            // heap pointer — the shape `extract_closure_target` /
            // `ClosureCall` / `call_callable` expect. Without this the name
            // fell through to the IRConst::None placeholder below, which
            // codegens to ConstNone = NONE_SENTINEL (0x8000_0000_0000_0000);
            // the asyncio/threading natives then dereferenced the sentinel
            // as a ClosureRepr and died with an access violation.
            if let Some(fid) = ctx.fn_id_by_name.get(name).copied() {
                let ty = ctx.expr_ty(*span);
                return fb.push_value(
                    ty,
                    ValueKind::Op {
                        op: IROp::ClosureNew { fn_id: fid, n_captures: 0 },
                        args: vec![],
                    },
                );
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
            // Sets are DictRepr-backed ("dict-with-unit-value"): allocate a
            // fresh dict slot via DictNew, then SetAdd each element. SetAdd
            // takes a trailing TypeTag operand so the VM canonicalises the
            // element by value (int/float/str) rather than by pointer.
            let ty = ctx.expr_ty(*span);
            let tag = set_elem_tag_for(&ty);
            let set = fb.push_value(
                ty.clone(),
                ValueKind::Op {
                    op: IROp::NativeCall { native_id: NativeFn::DictNew as u32 },
                    args: vec![],
                },
            );
            for elt in elems {
                let ev = lower_expr(fb, ctx, elt);
                let tag_v = fb.push_value(
                    Ty::Primitive(PrimTy::I64),
                    ValueKind::Const(IRConst::I64(tag as i64)),
                );
                fb.push_value(
                    Ty::Primitive(PrimTy::Unit),
                    ValueKind::Op {
                        op: IROp::NativeCall { native_id: NativeFn::SetAdd as u32 },
                        args: vec![set, ev, tag_v],
                    },
                );
            }
            set
        }
        Expr::Comprehension { .. } => lower_comprehension(fb, ctx, e),
        Expr::Slice { .. } => lower_slice(fb, ctx, e),
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
            let lty = ctx.expr_ty(expr_span(lhs));
            let rty = ctx.expr_ty(expr_span(rhs));
            let l = lower_expr(fb, ctx, lhs);
            let r = lower_expr(fb, ctx, rhs);
            // Lane A: numeric operand widening + Python-3 `/`-is-float. The
            // helper only rewrites the all-numeric case; everything else
            // (str/tuple/container ops) falls through to emit_binop unchanged.
            lower_binop_coerced(fb, *op, l, &lty, r, &rty, ty)
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
                // Wave 2 / Lane C: `obj[k]` on a user class dispatches to
                // `__getitem__(self, k)`. The typechecker has already verified
                // the key type and set `ty` to the dunder's return type. Covers
                // both plain (`Foo`) and generic (`Foo[T]`) class receivers.
                _ if class_index_dunder_op(
                    ctx.class_layouts, ctx.fn_id_by_name, ctx.class_inst_method_fn,
                    &recv_ty, "__getitem__").is_some() =>
                {
                    let op = class_index_dunder_op(
                        ctx.class_layouts, ctx.fn_id_by_name, ctx.class_inst_method_fn,
                        &recv_ty, "__getitem__").expect("guard above proved Some");
                    fb.push_value(ty, ValueKind::Op { op, args: vec![arr, idx] })
                }
                _ => {
                    // Lane B: support `xs[-1]` etc. by normalizing negatives.
                    let idx = normalize_neg_list_index(fb, arr, idx);
                    fb.push_value(
                        ty,
                        ValueKind::Op { op: IROp::ArrayGet, args: vec![arr, idx] },
                    )
                }
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
        Expr::Comprehension { var, iter, body, value, cond, .. } => {
            // The iterable is free in the enclosing scope; the body/value/cond
            // run with the loop variable bound.
            collect_free_vars(iter, bound, seen, captures);
            let mut inner_bound: std::collections::HashSet<&str> = bound.clone();
            inner_bound.insert(var.as_str());
            collect_free_vars(body, &inner_bound, seen, captures);
            if let Some(v) = value { collect_free_vars(v, &inner_bound, seen, captures); }
            if let Some(c) = cond { collect_free_vars(c, &inner_bound, seen, captures); }
        }
        Expr::Slice { obj, lo, hi, step, .. } => {
            collect_free_vars(obj, bound, seen, captures);
            if let Some(e) = lo { collect_free_vars(e, bound, seen, captures); }
            if let Some(e) = hi { collect_free_vars(e, bound, seen, captures); }
            if let Some(e) = step { collect_free_vars(e, bound, seen, captures); }
        }
    }
}

/// Evaluate a top-level `final` const initialiser at compile time.
///
/// Supports literals, references to already-evaluated consts (`consts`),
/// unary `+`/`-`/`not`, binary arithmetic/bitwise/shift operators over
/// same-typed numeric constants, and `str + str` concatenation. Semantics
/// mirror the VM exactly: integer ops wrap, `/` and `//` on integers both
/// truncate (`IDiv`), `//` on floats is plain division (it lowers to
/// `FDiv`), and a zero integer divisor refuses to fold (it must raise
/// `ZeroDivisionError` at runtime, not bake a value in).
///
/// Returns `None` when the expression is not const-evaluable; the
/// typechecker turns that into `E3003` after its fixed-point pass, so the
/// IR lowerer's reference sites never see a missing const.
pub(crate) fn eval_const_expr(
    e: &Expr,
    ty: &Ty,
    consts: &HashMap<String, (IRConst, Ty)>,
) -> Option<IRConst> {
    match e {
        Expr::Literal { .. } => literal_to_irconst(e, ty),
        Expr::Ident { name, .. } => consts.get(name).map(|(c, _)| c.clone()),
        Expr::Unary { op: UnaryOp::Neg, operand, .. } => {
            match eval_const_expr(operand, ty, consts)? {
                IRConst::I32(v) => Some(IRConst::I32(v.wrapping_neg())),
                IRConst::I64(v) => Some(IRConst::I64(v.wrapping_neg())),
                IRConst::F32(v) => Some(IRConst::F32(-v)),
                IRConst::F64(v) => Some(IRConst::F64(-v)),
                _ => None,
            }
        }
        Expr::Unary { op: UnaryOp::Pos, operand, .. } => {
            eval_const_expr(operand, ty, consts)
        }
        Expr::Unary { op: UnaryOp::Not, operand, .. } => {
            match eval_const_expr(operand, ty, consts)? {
                IRConst::Bool(b) => Some(IRConst::Bool(!b)),
                _ => None,
            }
        }
        Expr::Binary { op, lhs, rhs, .. } => {
            let l = eval_const_expr(lhs, ty, consts)?;
            let r = eval_const_expr(rhs, ty, consts)?;
            fold_const_binop(*op, l, r)
        }
        _ => None,
    }
}

/// Binary-operator folding for [`eval_const_expr`]. Only same-variant
/// operand pairs fold; mixed-type arithmetic is a type error upstream.
/// `Pow` never folds — runtime lowers it to a placeholder (`IMul`), so
/// there is no correct value to bake in.
fn fold_const_binop(op: AstBinOp, l: IRConst, r: IRConst) -> Option<IRConst> {
    use AstBinOp::*;
    macro_rules! fold_int {
        ($ctor:ident, $a:expr, $b:expr) => {
            match op {
                Add => IRConst::$ctor($a.wrapping_add($b)),
                Sub => IRConst::$ctor($a.wrapping_sub($b)),
                Mul => IRConst::$ctor($a.wrapping_mul($b)),
                // `/` and `//` both lower to truncating IDiv; division by
                // zero must raise at runtime, so leave it unfolded.
                Div | FloorDiv => {
                    if $b == 0 { return None; }
                    IRConst::$ctor($a.wrapping_div($b))
                }
                Rem => {
                    if $b == 0 { return None; }
                    IRConst::$ctor($a.wrapping_rem($b))
                }
                BitAnd => IRConst::$ctor($a & $b),
                BitOr => IRConst::$ctor($a | $b),
                BitXor => IRConst::$ctor($a ^ $b),
                Shl => IRConst::$ctor($a.wrapping_shl($b as u32)),
                Shr => IRConst::$ctor($a.wrapping_shr($b as u32)),
                _ => return None,
            }
        };
    }
    macro_rules! fold_float {
        ($ctor:ident, $a:expr, $b:expr) => {
            match op {
                Add => IRConst::$ctor($a + $b),
                Sub => IRConst::$ctor($a - $b),
                Mul => IRConst::$ctor($a * $b),
                // Float `//` lowers to plain FDiv (no floor) — mirror it.
                Div | FloorDiv => IRConst::$ctor($a / $b),
                _ => return None,
            }
        };
    }
    Some(match (l, r) {
        (IRConst::I32(a), IRConst::I32(b)) => fold_int!(I32, a, b),
        (IRConst::I64(a), IRConst::I64(b)) => fold_int!(I64, a, b),
        (IRConst::U32(a), IRConst::U32(b)) => fold_int!(U32, a, b),
        (IRConst::U64(a), IRConst::U64(b)) => fold_int!(U64, a, b),
        (IRConst::F32(a), IRConst::F32(b)) => fold_float!(F32, a, b),
        (IRConst::F64(a), IRConst::F64(b)) => fold_float!(F64, a, b),
        (IRConst::Str(a), IRConst::Str(b)) => match op {
            Add => IRConst::Str(format!("{a}{b}")),
            _ => return None,
        },
        _ => return None,
    })
}

/// Fold a `final` const initialiser leaf to an [`IRConst`] when it's a
/// literal. Compound initialisers are handled by [`eval_const_expr`],
/// which recurses down to this for the literal leaves.
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

/// Lane A: insert a lossless numeric cast so `v` (currently typed `from`)
/// is materialised at primitive type `to`. Reuses the existing conversion
/// `NativeFn`s / cast `IROp`s — no new opcode. Returns `v` unchanged when the
/// types already match or no cast is needed (small-int -> i32 is a no-op
/// because the register already holds the sign-extended value).
fn coerce_numeric(fb: &mut FuncBuilder, v: ValueId, from: PrimTy, to: PrimTy) -> ValueId {
    use PrimTy::*;
    if from == to {
        return v;
    }
    let native = match (from, to) {
        // integer -> i64
        (I8 | I16 | I32 | U8 | U16 | U32 | Bool, I64) => Some(NativeFn::I64FromI32 as u32),
        // small signed int -> f64
        (I8 | I16 | I32 | Bool, F64) => Some(NativeFn::F64FromI32 as u32),
        // i64 -> f64
        (I64, F64) => Some(NativeFn::F64FromI64 as u32),
        _ => None,
    };
    if let Some(native_id) = native {
        return fb.push_value(
            Ty::Primitive(to),
            ValueKind::Op { op: IROp::NativeCall { native_id }, args: vec![v] },
        );
    }
    // f32 -> f64 uses the dedicated widening opcode (FExt -> F32ToF64).
    if from == F32 && to == F64 {
        return fb.push_value(
            Ty::Primitive(F64),
            ValueKind::Op { op: IROp::FExt, args: vec![v] },
        );
    }
    // small signed int -> i32: the register already holds the sign-extended
    // value, so a re-typed copy is sufficient.
    if matches!(from, I8 | I16 | Bool) && to == I32 {
        return fb.push_value(
            Ty::Primitive(I32),
            ValueKind::Op { op: IROp::Copy, args: vec![v] },
        );
    }
    // No lossless cast available — leave `v` as-is. The typechecker only
    // permits operand pairs that `numeric_common_ty` accepts, so this is
    // unreachable for well-typed programs; keep it total for safety.
    v
}

/// Lane A: lower a numeric binary op with implicit operand widening + the
/// Python-3 `/`-is-float rule. Computes the common type, coerces both
/// operands to it, then defers to [`emit_binop`].
///
/// `lty` / `rty` are the *operand* types from the typechecker; `result_ty`
/// is the binop node's type. For `/` the operands are widened to f64 and the
/// op becomes a float divide.
fn lower_binop_coerced(
    fb: &mut FuncBuilder,
    op: AstBinOp,
    l: ValueId,
    lty: &Ty,
    r: ValueId,
    rty: &Ty,
    result_ty: Ty,
) -> ValueId {
    fn prim_of(t: &Ty) -> Option<PrimTy> {
        let inner = match t { Ty::Nullable(b) => b.as_ref(), other => other };
        match inner { Ty::Primitive(p) if p.is_numeric() => Some(*p), _ => None }
    }
    let lp = prim_of(lty);
    let rp = prim_of(rty);
    // Only the all-numeric case participates in widening; everything else
    // (str concat, comparisons on strings/tuples, container `in`, etc.) is
    // handled by emit_binop's own dispatch on the raw operands.
    if let (Some(lp), Some(rp)) = (lp, rp) {
        // True division always operates in f64.
        let target = if matches!(op, AstBinOp::Div) {
            PrimTy::F64
        } else if matches!(op, AstBinOp::Shl | AstBinOp::Shr) {
            // Shifts: keep operands at their own widths; result follows LHS.
            return emit_binop(fb, op, l, r, result_ty);
        } else {
            numeric_common_ty_ir(lp, rp).unwrap_or(lp)
        };
        let lc = coerce_numeric(fb, l, lp, target);
        let rc = coerce_numeric(fb, r, rp, target);
        // The emitted op's operand width is read from `lc`'s type, so pass a
        // result type at `target` (or f64 for `/`) so codegen picks the right
        // sized opcode.
        let emit_ty = if matches!(op, AstBinOp::Div) {
            Ty::Primitive(PrimTy::F64)
        } else {
            result_ty
        };
        return emit_binop(fb, op, lc, rc, emit_ty);
    }
    emit_binop(fb, op, l, r, result_ty)
}

/// IR-side mirror of `typecheck::numeric_common_ty` (kept in sync). Returns
/// the widened common type for two numeric primitives.
fn numeric_common_ty_ir(a: PrimTy, b: PrimTy) -> Option<PrimTy> {
    use PrimTy::*;
    if a == b { return Some(a); }
    if a == F64 || b == F64 { return Some(F64); }
    if a == F32 || b == F32 {
        let other = if a == F32 { b } else { a };
        if other.is_integer() { return Some(F64); }
        return None;
    }
    let signed_small = |p: PrimTy| matches!(p, I8 | I16 | I32);
    if a == I64 && signed_small(b) { return Some(I64); }
    if b == I64 && signed_small(a) { return Some(I64); }
    if matches!(a, I8 | I16) && b == I32 { return Some(I32); }
    if matches!(b, I8 | I16) && a == I32 { return Some(I32); }
    None
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
        // String ordering. Before this, `Lt`/`Le`/`Gt`/`Ge` had no `is_str`
        // branch and fell through to the integer `ILt`/`ILe`/`IGt`/`IGe`,
        // which compares the two heap-pointer u64s — meaningless for content
        // ordering (same bug class as BUG-034 `str !=`, BUG-008 `is not`,
        // BUG-039 `in`). Lower as `StrCmp(l, r) <relop> 0`.
        AstBinOp::Lt | AstBinOp::Le | AstBinOp::Gt | AstBinOp::Ge if is_str => {
            let cmp = fb.push_value(
                Ty::Primitive(PrimTy::I64),
                ValueKind::Op {
                    op: IROp::NativeCall { native_id: NativeFn::StrCmp as u32 },
                    args: vec![l, r],
                },
            );
            let zero = fb.push_value(
                Ty::Primitive(PrimTy::I64),
                ValueKind::Const(IRConst::I64(0)),
            );
            let relop = match op {
                AstBinOp::Lt => IROp::ILt,
                AstBinOp::Le => IROp::ILe,
                AstBinOp::Gt => IROp::IGt,
                _ => IROp::IGe,
            };
            return fb.push_value(ty, ValueKind::Op { op: relop, args: vec![cmp, zero] });
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
                    let tag = set_elem_tag_for(&rhs_inner);
                    let tag_v = fb.push_value(
                        Ty::Primitive(PrimTy::I64),
                        ValueKind::Const(IRConst::I64(tag as i64)),
                    );
                    return fb.push_value(
                        ty,
                        ValueKind::Op {
                            op: IROp::NativeCall { native_id: NativeFn::SetHas as u32 },
                            args: vec![r, l, tag_v],
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
                let mut has_args = vec![r, l];
                // SetHas wants the canonicalisation tag as a third operand.
                if let Ty::Generic { base: TypeCtor::Set, .. } = &rhs_inner {
                    let tag = set_elem_tag_for(&rhs_inner);
                    has_args.push(fb.push_value(
                        Ty::Primitive(PrimTy::I64),
                        ValueKind::Const(IRConst::I64(tag as i64)),
                    ));
                }
                let has = fb.push_value(
                    Ty::Primitive(PrimTy::Bool),
                    ValueKind::Op {
                        op: IROp::NativeCall { native_id },
                        args: has_args,
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
        // Wave-2 Lane A: a user-defined class stringifies through its
        // `__str__` (falling back to `__repr__`), or — if it defines
        // neither — a default `ClassName(f1=<v1>, ...)` field repr. The
        // old `StrFromAny` path read the instance *pointer* as either a
        // string or an i64, printing garbage. See `lower_str_of_class`.
        Ty::Class(cid) => lower_str_of_class(fb, ctx, v, *cid),
        Ty::Generic { base: TypeCtor::Class(cid), .. } => {
            lower_str_of_class(fb, ctx, v, *cid)
        }
        _ => fb.push_value(
            str_ty,
            ValueKind::Op {
                op: IROp::NativeCall { native_id: NativeFn::StrFromAny as u32 },
                args: vec![v],
            },
        ),
    }
}

/// Wave-2 Lane A: stringify a user-class instance `v` of class `cid`.
///
/// Resolution order (Python-conformant):
///   1. `__str__` if the class defines or inherits it.
///   2. else `__repr__` if defined or inherited.
///   3. else a synthesised default repr `ClassName(field1=<v1>, field2=<v2>, …)`
///      built entirely at IR time from the class's declared fields, recursing
///      through `str_of_value` per field so nested classes / tuples format too.
///
/// The dunder calls reuse the Lane-0 `class_dunder_dispatch` scaffold, so they
/// honour the exact same direct-vs-virtual devirtualisation rule as a written
/// `obj.__str__()` method call (final/non-open → DirectCall, open/sealed or
/// inherited → VirtualCall). `__str__`/`__repr__` take only `self`, so the
/// call args are `[v]`.
fn lower_str_of_class(
    fb: &mut FuncBuilder,
    ctx: &mut LowerCtx,
    v: ValueId,
    cid: ClassId,
) -> ValueId {
    let str_ty = Ty::Primitive(PrimTy::Str);
    // 1./2. __str__, then __repr__.
    for dunder in ["__str__", "__repr__"] {
        if let Some(op) =
            class_dunder_dispatch(ctx.class_layouts, ctx.fn_id_by_name, cid, dunder)
        {
            return fb.push_value(
                str_ty,
                ValueKind::Op { op, args: vec![v] },
            );
        }
    }
    // 3. Default field repr: `ClassName(f1=<v1>, f2=<v2>, …)`.
    lower_default_class_repr(fb, ctx, v, cid)
}

/// Build the default `ClassName(field=value, …)` repr for a class instance that
/// defines neither `__str__` nor `__repr__`. Each field is loaded at its layout
/// offset and stringified through `str_of_value` (so the per-type dispatch — and
/// nested class/tuple handling — is reused). A field-less class renders as
/// `ClassName()`. If the layout is somehow unavailable, falls back to the bare
/// class-less marker `<object>` rather than reading a wild pointer.
fn lower_default_class_repr(
    fb: &mut FuncBuilder,
    ctx: &mut LowerCtx,
    v: ValueId,
    cid: ClassId,
) -> ValueId {
    let str_ty = Ty::Primitive(PrimTy::Str);
    let layout = match ctx.class_layouts.get(&cid) {
        Some(l) => l.clone(),
        None => {
            return fb.push_value(
                str_ty,
                ValueKind::Const(IRConst::Str("<object>".into())),
            );
        }
    };
    // Open with `ClassName(`.
    let mut acc = fb.push_value(
        str_ty.clone(),
        ValueKind::Const(IRConst::Str(format!("{}(", layout.name))),
    );
    for (i, f) in layout.fields.iter().enumerate() {
        // `, ` separator between fields, then `name=`.
        let prefix = if i == 0 {
            format!("{}=", f.name)
        } else {
            format!(", {}=", f.name)
        };
        let pv = fb.push_value(str_ty.clone(), ValueKind::Const(IRConst::Str(prefix)));
        acc = fb.push_value(
            str_ty.clone(),
            ValueKind::Op {
                op: IROp::NativeCall { native_id: NativeFn::StrConcat as u32 },
                args: vec![acc, pv],
            },
        );
        // Load the field at its declared offset and stringify by static type.
        let fv = fb.push_value(
            f.ty.clone(),
            ValueKind::Op { op: IROp::Load { offset: f.offset }, args: vec![v] },
        );
        let fs = str_of_value(fb, ctx, fv, &f.ty);
        acc = fb.push_value(
            str_ty.clone(),
            ValueKind::Op {
                op: IROp::NativeCall { native_id: NativeFn::StrConcat as u32 },
                args: vec![acc, fs],
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

/// M62a: lower a list / dict / set comprehension by desugaring to a fresh
/// collection plus an index-counted loop over the (List) iterable that
/// appends each (optionally filtered) element. The shape mirrors the `for`
/// statement's List-only iteration:
///
/// ```text
/// __c = <fresh List / Dict>          # ArrayNew / DictNew
/// __i: i64 = 0
/// __n: i64 = ArrayLen(it)
/// while __i < __n:
///     x: T = it[__i]
///     if <cond>:                      # only when an `if` filter is present
///         append <body> (or set <body>: <value>) into __c
///     __i = __i + 1
/// __c
/// ```
///
/// Set comprehensions produce the same placeholder value as set *literals*
/// today (the VM has no set runtime yet — see `Expr::Set`), but the loop and
/// the body still lower so the element expressions are type-checked / emitted.
/// P2: helper — resolve a lambda parameter's static type from the
/// resolver's ast→Ty map, falling back to Unit.
fn lambda_param_ty(ctx: &LowerCtx, p: &ast::Param) -> Ty {
    ctx.typed
        .resolved
        .ast_type_to_ty
        .get(&(ast_type_span(&p.ty).start, ast_type_span(&p.ty).end))
        .cloned()
        .unwrap_or(Ty::Primitive(PrimTy::Unit))
}

/// P2: resolve a lambda's return type from the resolver's ast→Ty map.
fn lambda_ret_ty(ctx: &LowerCtx, return_ty: &ast::Type) -> Ty {
    ctx.typed
        .resolved
        .ast_type_to_ty
        .get(&(ast_type_span(return_ty).start, ast_type_span(return_ty).end))
        .cloned()
        .unwrap_or(Ty::Primitive(PrimTy::Unit))
}

/// P2: specialize `map`/`filter`/`reduce`/`sorted_by` with a *literal lambda*
/// callback by inlining the lambda body into an in-language loop — the same
/// shape comprehensions desugar to (see `lower_comprehension`). This removes
/// the per-element interpreter re-entry of the M61a native HOFs: the body
/// inlines and JITs as one function.
///
/// Returns `Some(value)` when it handled the call; `None` to fall through to
/// the native path (non-literal callback, unsupported arity, etc.). Captured
/// outer variables in the lambda body resolve naturally — `Expr::Ident`
/// lowering is name-based against the enclosing `fb`'s slots, exactly like a
/// comprehension's body reads enclosing-scope names.
fn try_lower_hof_literal_lambda(
    fb: &mut FuncBuilder,
    ctx: &mut LowerCtx,
    name: &str,
    args: &[ast::Arg],
    span: Span,
) -> Option<ValueId> {
    let result_ty = ctx.expr_ty(span);
    let i64_ty = Ty::Primitive(PrimTy::I64);

    // Pull a literal lambda out of `args[idx]`, or bail.
    let as_lambda = |a: &ast::Arg| -> bool { matches!(&a.value, Expr::Lambda { .. }) };

    match name {
        "map" | "filter" if args.len() == 2 && as_lambda(&args[0]) => {
            let is_map = name == "map";
            let (lam_params, lam_body): (&[ast::Param], &Expr) = match &args[0].value {
                Expr::Lambda { params, body, .. } => (params, body),
                _ => return None,
            };
            if lam_params.len() != 1 {
                return None;
            }
            let iter = &args[1].value;
            let elem_ty = lambda_param_ty(ctx, &lam_params[0]);

            // result collection (List). `map` → List[U]; `filter` → List[T].
            let coll = fb.push_value(
                result_ty.clone(),
                ValueKind::Op { op: IROp::ArrayNew, args: vec![] },
            );
            let coll_slot = {
                let n = format!("__hof_c_{}", fb.slot_ty.len());
                let s = fb.alloc_slot(&n, result_ty.clone());
                fb.emit_write_local(s, coll);
                s
            };

            let arr = lower_expr(fb, ctx, iter);
            let zero = fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(0)));
            let i_slot = {
                let n = format!("__hof_i_{}", fb.slot_ty.len());
                let s = fb.alloc_slot(&n, i64_ty.clone());
                fb.emit_write_local(s, zero);
                s
            };
            let len_v = fb.push_value(
                i64_ty.clone(),
                ValueKind::Op { op: IROp::ArrayLen, args: vec![arr] },
            );
            let n_slot = {
                let n = format!("__hof_n_{}", fb.slot_ty.len());
                let s = fb.alloc_slot(&n, i64_ty.clone());
                fb.emit_write_local(s, len_v);
                s
            };
            // Lambda parameter slot — the inlined body reads this by name.
            let var_slot = fb.alloc_slot(&lam_params[0].name, elem_ty.clone());

            let header = fb.new_block();
            let body_b = fb.new_block();
            let exit = fb.new_block();
            fb.terminate(Terminator::Branch { target: header });

            fb.switch_to(header);
            let i_cur = fb.emit_read_local(i_slot);
            let n_cur = fb.emit_read_local(n_slot);
            let test = fb.push_value(
                Ty::Primitive(PrimTy::Bool),
                ValueKind::Op { op: IROp::ILt, args: vec![i_cur, n_cur] },
            );
            fb.terminate(Terminator::CondBranch { cond: test, t: body_b, f: exit });

            fb.switch_to(body_b);
            let i_now = fb.emit_read_local(i_slot);
            let elt = fb.push_value(
                elem_ty.clone(),
                ValueKind::Op { op: IROp::ArrayGet, args: vec![arr, i_now] },
            );
            fb.emit_write_local(var_slot, elt);

            if is_map {
                // append fn(x) to coll
                let ev = lower_expr(fb, ctx, lam_body);
                let coll_v = fb.emit_read_local(coll_slot);
                fb.push_value(
                    Ty::Primitive(PrimTy::Unit),
                    ValueKind::Op { op: IROp::ListPush, args: vec![coll_v, ev] },
                );
            } else {
                // filter: if fn(x): append x
                let keep = lower_expr(fb, ctx, lam_body);
                let keep_b = fb.new_block();
                let skip_b = fb.new_block();
                fb.terminate(Terminator::CondBranch { cond: keep, t: keep_b, f: skip_b });
                fb.switch_to(keep_b);
                let x_v = fb.emit_read_local(var_slot);
                let coll_v = fb.emit_read_local(coll_slot);
                fb.push_value(
                    Ty::Primitive(PrimTy::Unit),
                    ValueKind::Op { op: IROp::ListPush, args: vec![coll_v, x_v] },
                );
                fb.terminate(Terminator::Branch { target: skip_b });
                fb.switch_to(skip_b);
            }

            let i_again = fb.emit_read_local(i_slot);
            let one = fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(1)));
            let next_i = fb.push_value(
                i64_ty.clone(),
                ValueKind::Op { op: IROp::IAdd, args: vec![i_again, one] },
            );
            fb.emit_write_local(i_slot, next_i);
            fb.terminate(Terminator::Branch { target: header });

            fb.switch_to(exit);
            Some(fb.emit_read_local(coll_slot))
        }

        "reduce" if args.len() == 3 && as_lambda(&args[0]) => {
            let (lam_params, lam_body): (&[ast::Param], &Expr) = match &args[0].value {
                Expr::Lambda { params, body, .. } => (params, body),
                _ => return None,
            };
            if lam_params.len() != 2 {
                return None;
            }
            let iter = &args[1].value;
            let acc_ty = lambda_param_ty(ctx, &lam_params[0]);
            let elem_ty = lambda_param_ty(ctx, &lam_params[1]);

            // init → accumulator slot (also the lambda's first param slot).
            let init_v = lower_expr(fb, ctx, &args[2].value);
            let acc_slot = fb.alloc_slot(&lam_params[0].name, acc_ty.clone());
            fb.emit_write_local(acc_slot, init_v);

            let arr = lower_expr(fb, ctx, iter);
            let zero = fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(0)));
            let i_slot = {
                let n = format!("__hof_i_{}", fb.slot_ty.len());
                let s = fb.alloc_slot(&n, i64_ty.clone());
                fb.emit_write_local(s, zero);
                s
            };
            let len_v = fb.push_value(
                i64_ty.clone(),
                ValueKind::Op { op: IROp::ArrayLen, args: vec![arr] },
            );
            let n_slot = {
                let n = format!("__hof_n_{}", fb.slot_ty.len());
                let s = fb.alloc_slot(&n, i64_ty.clone());
                fb.emit_write_local(s, len_v);
                s
            };
            let elem_slot = fb.alloc_slot(&lam_params[1].name, elem_ty.clone());

            let header = fb.new_block();
            let body_b = fb.new_block();
            let exit = fb.new_block();
            fb.terminate(Terminator::Branch { target: header });

            fb.switch_to(header);
            let i_cur = fb.emit_read_local(i_slot);
            let n_cur = fb.emit_read_local(n_slot);
            let test = fb.push_value(
                Ty::Primitive(PrimTy::Bool),
                ValueKind::Op { op: IROp::ILt, args: vec![i_cur, n_cur] },
            );
            fb.terminate(Terminator::CondBranch { cond: test, t: body_b, f: exit });

            fb.switch_to(body_b);
            let i_now = fb.emit_read_local(i_slot);
            let elt = fb.push_value(
                elem_ty.clone(),
                ValueKind::Op { op: IROp::ArrayGet, args: vec![arr, i_now] },
            );
            fb.emit_write_local(elem_slot, elt);
            // acc = fn(acc, x) — body reads acc/x slots by name.
            let nv = lower_expr(fb, ctx, lam_body);
            fb.emit_write_local(acc_slot, nv);

            let i_again = fb.emit_read_local(i_slot);
            let one = fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(1)));
            let next_i = fb.push_value(
                i64_ty.clone(),
                ValueKind::Op { op: IROp::IAdd, args: vec![i_again, one] },
            );
            fb.emit_write_local(i_slot, next_i);
            fb.terminate(Terminator::Branch { target: header });

            fb.switch_to(exit);
            Some(fb.emit_read_local(acc_slot))
        }

        "sorted_by" if args.len() == 2 && as_lambda(&args[1]) => {
            // Schwartzian: compute keys once into a parallel list via the
            // inlined key fn, copy the data into a fresh list, then sort the
            // copy in place using the precomputed keys (one native call total).
            let (lam_params, lam_body, lam_ret): (&[ast::Param], &Expr, Ty) =
                match &args[1].value {
                    Expr::Lambda { params, body, return_ty, .. } =>
                        (params, body, lambda_ret_ty(ctx, return_ty)),
                    _ => return None,
                };
            if lam_params.len() != 1 {
                return None;
            }
            let key_tag = sort_type_tag_for(&lam_ret);
            // v1 only sorts i64/f64/str keys; bail to native otherwise so the
            // native path emits the proper TypeError (keeps semantics identical).
            if !matches!(key_tag, 3 | 9 | 11) {
                return None;
            }
            let iter = &args[0].value;
            let elem_ty = lambda_param_ty(ctx, &lam_params[0]);
            let key_list_ty = Ty::Generic {
                base: TypeCtor::List,
                args: vec![lam_ret.clone()],
            };

            // Source list value.
            let arr = lower_expr(fb, ctx, iter);

            // dst = copy of source (fresh List[T]); we sort the copy in place.
            let dst = fb.push_value(
                result_ty.clone(),
                ValueKind::Op { op: IROp::ArrayNew, args: vec![] },
            );
            let dst_slot = {
                let n = format!("__hof_d_{}", fb.slot_ty.len());
                let s = fb.alloc_slot(&n, result_ty.clone());
                fb.emit_write_local(s, dst);
                s
            };
            // keys = fresh List[KeyTy].
            let keys = fb.push_value(
                key_list_ty.clone(),
                ValueKind::Op { op: IROp::ArrayNew, args: vec![] },
            );
            let keys_slot = {
                let n = format!("__hof_k_{}", fb.slot_ty.len());
                let s = fb.alloc_slot(&n, key_list_ty.clone());
                fb.emit_write_local(s, keys);
                s
            };

            let zero = fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(0)));
            let i_slot = {
                let n = format!("__hof_i_{}", fb.slot_ty.len());
                let s = fb.alloc_slot(&n, i64_ty.clone());
                fb.emit_write_local(s, zero);
                s
            };
            let len_v = fb.push_value(
                i64_ty.clone(),
                ValueKind::Op { op: IROp::ArrayLen, args: vec![arr] },
            );
            let n_slot = {
                let n = format!("__hof_n_{}", fb.slot_ty.len());
                let s = fb.alloc_slot(&n, i64_ty.clone());
                fb.emit_write_local(s, len_v);
                s
            };
            let var_slot = fb.alloc_slot(&lam_params[0].name, elem_ty.clone());

            let header = fb.new_block();
            let body_b = fb.new_block();
            let exit = fb.new_block();
            fb.terminate(Terminator::Branch { target: header });

            fb.switch_to(header);
            let i_cur = fb.emit_read_local(i_slot);
            let n_cur = fb.emit_read_local(n_slot);
            let test = fb.push_value(
                Ty::Primitive(PrimTy::Bool),
                ValueKind::Op { op: IROp::ILt, args: vec![i_cur, n_cur] },
            );
            fb.terminate(Terminator::CondBranch { cond: test, t: body_b, f: exit });

            fb.switch_to(body_b);
            let i_now = fb.emit_read_local(i_slot);
            let elt = fb.push_value(
                elem_ty.clone(),
                ValueKind::Op { op: IROp::ArrayGet, args: vec![arr, i_now] },
            );
            fb.emit_write_local(var_slot, elt);
            // push element into dst copy
            let dst_v = fb.emit_read_local(dst_slot);
            fb.push_value(
                Ty::Primitive(PrimTy::Unit),
                ValueKind::Op { op: IROp::ListPush, args: vec![dst_v, elt] },
            );
            // compute key = key_fn(x), push into keys
            let kv = lower_expr(fb, ctx, lam_body);
            let keys_v = fb.emit_read_local(keys_slot);
            fb.push_value(
                Ty::Primitive(PrimTy::Unit),
                ValueKind::Op { op: IROp::ListPush, args: vec![keys_v, kv] },
            );

            let i_again = fb.emit_read_local(i_slot);
            let one = fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(1)));
            let next_i = fb.push_value(
                i64_ty.clone(),
                ValueKind::Op { op: IROp::IAdd, args: vec![i_again, one] },
            );
            fb.emit_write_local(i_slot, next_i);
            fb.terminate(Terminator::Branch { target: header });

            fb.switch_to(exit);
            // sort dst in place by precomputed keys.
            let dst_v = fb.emit_read_local(dst_slot);
            let keys_v = fb.emit_read_local(keys_slot);
            let tag_v = fb.push_value(
                i64_ty.clone(),
                ValueKind::Const(IRConst::I64(key_tag as i64)),
            );
            fb.push_value(
                Ty::Primitive(PrimTy::Unit),
                ValueKind::Op {
                    op: IROp::NativeCall {
                        native_id: NativeFn::SortByPrecomputed as u32,
                    },
                    args: vec![dst_v, keys_v, tag_v],
                },
            );
            Some(fb.emit_read_local(dst_slot))
        }

        _ => None,
    }
}

fn lower_comprehension(fb: &mut FuncBuilder, ctx: &mut LowerCtx, e: &Expr) -> ValueId {
    let (kind, var, var_ty, iter, body, value, cond, span) = match e {
        Expr::Comprehension { kind, var, var_ty, iter, body, value, cond, span, .. } =>
            (*kind, var, var_ty, iter.as_ref(), body.as_ref(),
             value.as_deref(), cond.as_deref(), *span),
        _ => unreachable!("lower_comprehension on non-comprehension"),
    };

    let result_ty = ctx.expr_ty(span);

    // Iterable element type, from the iterable's `List[T]`; fall back to the
    // declared loop-var annotation if the args slot is somehow empty.
    let iter_ty = ctx.expr_ty(expr_span(iter));
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

    // Allocate the fresh result collection BEFORE the loop header.
    let coll = match kind {
        ComprehensionKind::List => fb.push_value(
            result_ty.clone(),
            ValueKind::Op { op: IROp::ArrayNew, args: vec![] },
        ),
        ComprehensionKind::Dict => fb.push_value(
            result_ty.clone(),
            ValueKind::Op {
                op: IROp::NativeCall { native_id: NativeFn::DictNew as u32 },
                args: vec![],
            },
        ),
        // Sets are DictRepr-backed, same as set literals: fresh dict slot,
        // populated via SetAdd in the loop body below.
        ComprehensionKind::Set => fb.push_value(
            result_ty.clone(),
            ValueKind::Op {
                op: IROp::NativeCall { native_id: NativeFn::DictNew as u32 },
                args: vec![],
            },
        ),
    };
    // Stash the collection in a slot so the loop-body block reads a stable
    // value across the back-edge (same phi-via-slot trick the for-loop uses).
    let coll_slot = {
        let n = format!("__comp_c_{}", fb.slot_ty.len());
        let s = fb.alloc_slot(&n, result_ty.clone());
        fb.emit_write_local(s, coll);
        s
    };

    // Materialise the iterable once, before the loop header.
    let arr = lower_expr(fb, ctx, iter);
    let i64_ty = Ty::Primitive(PrimTy::I64);

    // __i: i64 = 0
    let zero = fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(0)));
    let i_slot = {
        let n = format!("__comp_i_{}", fb.slot_ty.len());
        let s = fb.alloc_slot(&n, i64_ty.clone());
        fb.emit_write_local(s, zero);
        s
    };
    // __n: i64 = ArrayLen(it)
    let len_v = fb.push_value(
        i64_ty.clone(),
        ValueKind::Op { op: IROp::ArrayLen, args: vec![arr] },
    );
    let n_slot = {
        let n = format!("__comp_n_{}", fb.slot_ty.len());
        let s = fb.alloc_slot(&n, i64_ty.clone());
        fb.emit_write_local(s, len_v);
        s
    };

    // Loop variable slot.
    let var_slot = fb.alloc_slot(var, elem_ty.clone());

    let header = fb.new_block();
    let body_b = fb.new_block();
    let exit = fb.new_block();
    fb.terminate(Terminator::Branch { target: header });

    // header: __i < __n
    fb.switch_to(header);
    let i_cur = fb.emit_read_local(i_slot);
    let n_cur = fb.emit_read_local(n_slot);
    let test = fb.push_value(
        Ty::Primitive(PrimTy::Bool),
        ValueKind::Op { op: IROp::ILt, args: vec![i_cur, n_cur] },
    );
    fb.terminate(Terminator::CondBranch { cond: test, t: body_b, f: exit });

    // body: x = it[__i]; [if cond:] append; __i += 1
    fb.switch_to(body_b);
    let i_now = fb.emit_read_local(i_slot);
    let elt = fb.push_value(
        elem_ty.clone(),
        ValueKind::Op { op: IROp::ArrayGet, args: vec![arr, i_now] },
    );
    fb.emit_write_local(var_slot, elt);

    // Emit the per-element append, wrapped in the optional `if` filter.
    let emit_append = |fb: &mut FuncBuilder, ctx: &mut LowerCtx| {
        let coll_v = fb.emit_read_local(coll_slot);
        match kind {
            ComprehensionKind::List | ComprehensionKind::Set => {
                let ev = lower_expr(fb, ctx, body);
                if matches!(kind, ComprehensionKind::List) {
                    fb.push_value(
                        Ty::Primitive(PrimTy::Unit),
                        ValueKind::Op { op: IROp::ListPush, args: vec![coll_v, ev] },
                    );
                } else {
                    let tag = set_elem_tag_for(&result_ty);
                    let tag_v = fb.push_value(
                        Ty::Primitive(PrimTy::I64),
                        ValueKind::Const(IRConst::I64(tag as i64)),
                    );
                    fb.push_value(
                        Ty::Primitive(PrimTy::Unit),
                        ValueKind::Op {
                            op: IROp::NativeCall { native_id: NativeFn::SetAdd as u32 },
                            args: vec![coll_v, ev, tag_v],
                        },
                    );
                }
            }
            ComprehensionKind::Dict => {
                let kv = lower_expr(fb, ctx, body);
                let vv = lower_expr(fb, ctx, value.expect("dict comprehension value"));
                fb.push_value(
                    Ty::Primitive(PrimTy::Unit),
                    ValueKind::Op {
                        op: IROp::NativeCall { native_id: NativeFn::DictSet as u32 },
                        args: vec![coll_v, kv, vv],
                    },
                );
            }
        }
    };

    if let Some(c) = cond {
        let cv = lower_expr(fb, ctx, c);
        let keep = fb.new_block();
        let skip = fb.new_block();
        fb.terminate(Terminator::CondBranch { cond: cv, t: keep, f: skip });
        fb.switch_to(keep);
        emit_append(fb, ctx);
        fb.terminate(Terminator::Branch { target: skip });
        fb.switch_to(skip);
    } else {
        emit_append(fb, ctx);
    }

    // __i = __i + 1
    let i_again = fb.emit_read_local(i_slot);
    let one = fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(1)));
    let next_i = fb.push_value(
        i64_ty.clone(),
        ValueKind::Op { op: IROp::IAdd, args: vec![i_again, one] },
    );
    fb.emit_write_local(i_slot, next_i);
    fb.terminate(Terminator::Branch { target: header });

    // exit: read back the populated collection.
    fb.switch_to(exit);
    fb.emit_read_local(coll_slot)
}

/// Lane B: lower a slice expression `obj[lo:hi:step]` for `str` and
/// `List[T]` receivers, with full Python semantics:
///   * any bound may be omitted (`[:]`, `[1:]`, `[:n]`, `[::2]`, `[::-1]`);
///   * negative bounds count from the end;
///   * `step` may be negative (reverse) but must be non-zero at runtime
///     (a zero step raises ValueError).
///
/// The normalized (start, stop) bounds are computed at runtime in IR with
/// `slice.indices(len)`-equivalent clamping (the defaults and clamp limits
/// depend on the sign of `step`), then a single loop materialises the result:
///   * `List[T]` → `ArrayNew` + `ListPush(obj[i])`;
///   * `str`     → empty string + `StrAppendChar(char_at(obj, i))`.
///
/// Everything is built from existing IR ops / natives — no new opcode or
/// native is introduced, so the slice works identically on the interpreter
/// and the JIT.
fn lower_slice(fb: &mut FuncBuilder, ctx: &mut LowerCtx, e: &Expr) -> ValueId {
    let (obj, lo, hi, step, span) = match e {
        Expr::Slice { obj, lo, hi, step, span } =>
            (obj.as_ref(), lo.as_deref(), hi.as_deref(), step.as_deref(), *span),
        _ => unreachable!("lower_slice on non-slice"),
    };

    let i64_ty = Ty::Primitive(PrimTy::I64);
    let bool_ty = Ty::Primitive(PrimTy::Bool);
    let result_ty = ctx.expr_ty(span);
    let recv_ty = ctx.expr_ty(expr_span(obj));
    let is_str = matches!(recv_ty, Ty::Primitive(PrimTy::Str));
    let elem_ty = match &result_ty {
        Ty::Generic { base: TypeCtor::List, args } if args.len() == 1 => args[0].clone(),
        _ => Ty::Primitive(PrimTy::Unit),
    };

    // Materialise the receiver once.
    let src = lower_expr(fb, ctx, obj);

    // len = len(obj)  — Len reads `length` (code-point count for str,
    // element count for List), which is exactly what we want here.
    let len = fb.push_value(
        i64_ty.clone(),
        ValueKind::Op {
            op: if is_str {
                IROp::NativeCall { native_id: NativeFn::Len as u32 }
            } else {
                IROp::ArrayLen
            },
            args: vec![src],
        },
    );

    // Small IR helpers.
    let kconst = |fb: &mut FuncBuilder, v: i64| {
        fb.push_value(Ty::Primitive(PrimTy::I64), ValueKind::Const(IRConst::I64(v)))
    };

    // NOTE: `IROp::Select` is a codegen placeholder that unconditionally
    // moves its `then` operand (see codegen.rs), so it CANNOT be used for any
    // value that actually depends on the condition. We instead realise every
    // conditional as a real `CondBranch` + slot merge via `emit_select`
    // (the same pattern `NullCoalesce` uses). `then`/`else` are pre-computed
    // pure-arithmetic values, so eager evaluation of both is side-effect-free.
    let emit_select =
        |fb: &mut FuncBuilder, cond: ValueId, then_v: ValueId, else_v: ValueId| -> ValueId {
            let nm = format!("__slice_sel_{}", fb.slot_ty.len());
            let slot = fb.alloc_slot(&nm, Ty::Primitive(PrimTy::I64));
            fb.emit_write_local(slot, else_v);
            let t_b = fb.new_block();
            let merge = fb.new_block();
            fb.terminate(Terminator::CondBranch { cond, t: t_b, f: merge });
            fb.switch_to(t_b);
            fb.emit_write_local(slot, then_v);
            fb.terminate(Terminator::Branch { target: merge });
            fb.switch_to(merge);
            fb.emit_read_local(slot)
        };

    // step (default 1).
    let step_v = match step {
        Some(e) => lower_expr(fb, ctx, e),
        None => kconst(fb, 1),
    };
    let zero = kconst(fb, 0);
    let one = kconst(fb, 1);
    let neg_one = kconst(fb, -1);

    // step_is_neg = step < 0
    let step_is_neg = fb.push_value(
        bool_ty.clone(),
        ValueKind::Op { op: IROp::ILt, args: vec![step_v, zero] },
    );

    // adjust(x) = x < 0 ? x + len : x  — fold a negative bound from the end.
    let adjust = |fb: &mut FuncBuilder,
                  emit_select: &dyn Fn(&mut FuncBuilder, ValueId, ValueId, ValueId) -> ValueId,
                  x: ValueId,
                  len: ValueId|
     -> ValueId {
        let x_plus = fb.push_value(
            Ty::Primitive(PrimTy::I64),
            ValueKind::Op { op: IROp::IAdd, args: vec![x, len] },
        );
        let z = fb.push_value(
            Ty::Primitive(PrimTy::I64),
            ValueKind::Const(IRConst::I64(0)),
        );
        let neg = fb.push_value(
            Ty::Primitive(PrimTy::Bool),
            ValueKind::Op { op: IROp::ILt, args: vec![x, z] },
        );
        emit_select(fb, neg, x_plus, x)
    };

    // clamp(x, clo, chi) = max(clo, min(x, chi)).
    let clamp = |fb: &mut FuncBuilder,
                 emit_select: &dyn Fn(&mut FuncBuilder, ValueId, ValueId, ValueId) -> ValueId,
                 x: ValueId,
                 clo: ValueId,
                 chi: ValueId|
     -> ValueId {
        // m = x > chi ? chi : x
        let gt = fb.push_value(
            Ty::Primitive(PrimTy::Bool),
            ValueKind::Op { op: IROp::IGt, args: vec![x, chi] },
        );
        let m = emit_select(fb, gt, chi, x);
        // r = m < clo ? clo : m
        let lt = fb.push_value(
            Ty::Primitive(PrimTy::Bool),
            ValueKind::Op { op: IROp::ILt, args: vec![m, clo] },
        );
        emit_select(fb, lt, clo, m)
    };

    // len - 1 (used for backward-step default/clamp).
    let len_minus_1 = fb.push_value(
        i64_ty.clone(),
        ValueKind::Op { op: IROp::ISub, args: vec![len, one] },
    );

    // ── start ──────────────────────────────────────────────────────────
    // Present: adjust then clamp; clamp range depends on step sign:
    //   step>0 → [0, len];   step<0 → [-1, len-1].
    // Absent:  step>0 → 0;   step<0 → len-1.
    let start = match lo {
        Some(e) => {
            let raw = lower_expr(fb, ctx, e);
            let adj = adjust(fb, &emit_select, raw, len);
            let fwd = clamp(fb, &emit_select, adj, zero, len);
            let bwd = clamp(fb, &emit_select, adj, neg_one, len_minus_1);
            emit_select(fb, step_is_neg, bwd, fwd)
        }
        None => emit_select(fb, step_is_neg, len_minus_1, zero),
    };

    // ── stop ───────────────────────────────────────────────────────────
    // Present: same clamping as start.
    // Absent:  step>0 → len;   step<0 → -1.
    let stop = match hi {
        Some(e) => {
            let raw = lower_expr(fb, ctx, e);
            let adj = adjust(fb, &emit_select, raw, len);
            let fwd = clamp(fb, &emit_select, adj, zero, len);
            let bwd = clamp(fb, &emit_select, adj, neg_one, len_minus_1);
            emit_select(fb, step_is_neg, bwd, fwd)
        }
        None => emit_select(fb, step_is_neg, neg_one, len),
    };

    // ── result collection ──────────────────────────────────────────────
    let result_slot = {
        let n = format!("__slice_r_{}", fb.slot_ty.len());
        let s = fb.alloc_slot(&n, result_ty.clone());
        let init = if is_str {
            // empty string constant — the accumulator the loop appends to.
            fb.push_value(
                Ty::Primitive(PrimTy::Str),
                ValueKind::Const(IRConst::Str(String::new())),
            )
        } else {
            fb.push_value(
                result_ty.clone(),
                ValueKind::Op { op: IROp::ArrayNew, args: vec![] },
            )
        };
        fb.emit_write_local(s, init);
        s
    };

    // i = start
    let i_slot = {
        let n = format!("__slice_i_{}", fb.slot_ty.len());
        let s = fb.alloc_slot(&n, i64_ty.clone());
        fb.emit_write_local(s, start);
        s
    };
    let stop_slot = {
        let n = format!("__slice_stop_{}", fb.slot_ty.len());
        let s = fb.alloc_slot(&n, i64_ty.clone());
        fb.emit_write_local(s, stop);
        s
    };
    let step_slot = {
        let n = format!("__slice_step_{}", fb.slot_ty.len());
        let s = fb.alloc_slot(&n, i64_ty.clone());
        fb.emit_write_local(s, step_v);
        s
    };

    let header = fb.new_block();
    let header_fwd = fb.new_block();
    let header_bwd = fb.new_block();
    let body_b = fb.new_block();
    let exit = fb.new_block();
    fb.terminate(Terminator::Branch { target: header });

    // header: branch on the sign of step into the forward / backward test.
    // We can't use `IROp::Select` for the loop condition (it ignores its
    // condition — see the note at `emit_select`), so split into real blocks.
    fb.switch_to(header);
    let step_cur = fb.emit_read_local(step_slot);
    let zero_h = fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(0)));
    let neg_h = fb.push_value(
        bool_ty.clone(),
        ValueKind::Op { op: IROp::ILt, args: vec![step_cur, zero_h] },
    );
    fb.terminate(Terminator::CondBranch { cond: neg_h, t: header_bwd, f: header_fwd });

    // forward: continue while i < stop.
    fb.switch_to(header_fwd);
    let i_fwd = fb.emit_read_local(i_slot);
    let stop_fwd = fb.emit_read_local(stop_slot);
    let lt_test = fb.push_value(
        bool_ty.clone(),
        ValueKind::Op { op: IROp::ILt, args: vec![i_fwd, stop_fwd] },
    );
    fb.terminate(Terminator::CondBranch { cond: lt_test, t: body_b, f: exit });

    // backward: continue while i > stop.
    fb.switch_to(header_bwd);
    let i_bwd = fb.emit_read_local(i_slot);
    let stop_bwd = fb.emit_read_local(stop_slot);
    let gt_test = fb.push_value(
        bool_ty.clone(),
        ValueKind::Op { op: IROp::IGt, args: vec![i_bwd, stop_bwd] },
    );
    fb.terminate(Terminator::CondBranch { cond: gt_test, t: body_b, f: exit });

    // body: append obj[i]; i += step
    fb.switch_to(body_b);
    let i_now = fb.emit_read_local(i_slot);
    let coll_v = fb.emit_read_local(result_slot);
    if is_str {
        let ch = fb.push_value(
            Ty::Primitive(PrimTy::Char),
            ValueKind::Op {
                op: IROp::NativeCall { native_id: NativeFn::StrCharAt as u32 },
                args: vec![src, i_now],
            },
        );
        let appended = fb.push_value(
            Ty::Primitive(PrimTy::Str),
            ValueKind::Op {
                op: IROp::NativeCall { native_id: NativeFn::StrAppendChar as u32 },
                args: vec![coll_v, ch],
            },
        );
        fb.emit_write_local(result_slot, appended);
    } else {
        let elt = fb.push_value(
            elem_ty.clone(),
            ValueKind::Op { op: IROp::ArrayGet, args: vec![src, i_now] },
        );
        fb.push_value(
            Ty::Primitive(PrimTy::Unit),
            ValueKind::Op { op: IROp::ListPush, args: vec![coll_v, elt] },
        );
    }
    let i_again = fb.emit_read_local(i_slot);
    let step_again = fb.emit_read_local(step_slot);
    let next_i = fb.push_value(
        i64_ty.clone(),
        ValueKind::Op { op: IROp::IAdd, args: vec![i_again, step_again] },
    );
    fb.emit_write_local(i_slot, next_i);
    fb.terminate(Terminator::Branch { target: header });

    fb.switch_to(exit);
    fb.emit_read_local(result_slot)
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

// ── M61b: call-site argument normalisation ───────────────────────────────
//
// At lowering time we rewrite a call's argument list — a mix of positional
// and `name=value` keyword arguments, possibly with omitted trailing
// parameters — into a flat positional list in declaration order, with each
// omitted parameter replaced by its declared default expression. The rest of
// `lower_call` then sees the existing all-positional ABI unchanged.
//
// The binding decisions were already validated by the type checker (same
// [`argbind`] algorithm), so any error here would be an internal
// inconsistency; we fall back to the original args rather than panicking.

/// AST parameters of an `Ident` callee that is a user free function or a
/// constructor (`__init__`), with `self` stripped. Returns `None` for
/// builtins, lambdas, native classes, etc. — those stay positional.
fn callee_ast_params(ctx: &LowerCtx, callee: &Expr) -> Option<Vec<ast::Param>> {
    let name = match callee {
        Expr::Ident { name, .. } => name,
        _ => return None,
    };
    let r = &ctx.typed.resolved;
    let sid = r.symbols.lookup(r.module_scope, name)?;
    match r.symbols.get(sid).kind {
        SymbolKind::Function => {
            for d in &r.module.decls {
                if let TopDecl::Func(f) = d {
                    if &f.name == name {
                        return Some(f.params.clone());
                    }
                }
            }
            None
        }
        SymbolKind::Class => {
            for d in &r.module.decls {
                if let TopDecl::Class(c) = d {
                    if &c.name == name {
                        let init = c.init.as_ref()?;
                        let params = &init.params;
                        let stripped = match params.first() {
                            Some(p) if p.name == "self" => params[1..].to_vec(),
                            _ => params.clone(),
                        };
                        return Some(stripped);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Normalise a call's args into positional order, materialising defaults for
/// omitted parameters. Returns `None` when no rewrite is needed (all
/// positional and already in order) or when the callee's params can't be
/// recovered (builtins etc.), so the caller keeps the original slice.
fn normalize_call_args(ctx: &LowerCtx, callee: &Expr, args: &[ast::Arg]) -> Option<Vec<ast::Arg>> {
    let params = callee_ast_params(ctx, callee)?;
    // Fast path: every arg positional and count matches — nothing to do.
    let all_positional = args.iter().all(|a| a.name.is_none());
    if all_positional && args.len() == params.len() {
        return None;
    }
    let infos = crate::argbind::ParamInfo::from_params(&params);
    let span = args.first().map(|a| a.span).unwrap_or(Span::DUMMY);
    let slots = crate::argbind::bind(&infos, args, span, "call").ok()?;
    let mut out: Vec<ast::Arg> = Vec::with_capacity(slots.len());
    for (pidx, slot) in slots.iter().enumerate() {
        match slot {
            crate::argbind::Slot::Arg(ai) => {
                // Strip the keyword label; downstream only cares about value.
                out.push(ast::Arg { name: None, value: args[*ai].value.clone(), span: args[*ai].span });
            }
            crate::argbind::Slot::Default => {
                let def = params[pidx].default.clone()?;
                let dspan = expr_span(&def);
                out.push(ast::Arg { name: None, value: def, span: dspan });
            }
        }
    }
    Some(out)
}

/// Like [`normalize_call_args`] but for a user-class method, resolved by the
/// receiver's class name (walking the inheritance chain).
fn normalize_method_args(
    ctx: &LowerCtx,
    class_name: &str,
    method: &str,
    args: &[ast::Arg],
) -> Option<Vec<ast::Arg>> {
    let r = &ctx.typed.resolved;
    let mut found: Option<Vec<ast::Param>> = None;
    for d in &r.module.decls {
        if let TopDecl::Class(c) = d {
            if c.name == class_name {
                if let Some(m) = c.methods.iter().find(|m| m.name == method) {
                    let params = &m.params;
                    found = Some(match params.first() {
                        Some(p) if p.name == "self" => params[1..].to_vec(),
                        _ => params.clone(),
                    });
                }
                break;
            }
        }
    }
    let params = found?;
    let all_positional = args.iter().all(|a| a.name.is_none());
    if all_positional && args.len() == params.len() {
        return None;
    }
    let infos = crate::argbind::ParamInfo::from_params(&params);
    let span = args.first().map(|a| a.span).unwrap_or(Span::DUMMY);
    let slots = crate::argbind::bind(&infos, args, span, "method call").ok()?;
    let mut out: Vec<ast::Arg> = Vec::with_capacity(slots.len());
    for (pidx, slot) in slots.iter().enumerate() {
        match slot {
            crate::argbind::Slot::Arg(ai) => {
                out.push(ast::Arg { name: None, value: args[*ai].value.clone(), span: args[*ai].span });
            }
            crate::argbind::Slot::Default => {
                let def = params[pidx].default.clone()?;
                let dspan = expr_span(&def);
                out.push(ast::Arg { name: None, value: def, span: dspan });
            }
        }
    }
    Some(out)
}

/// M62b: is `iter` a direct call to a generator function (declared
/// `-> Iterator[T]`)? Such a call lowers to `MakeGen`, producing a generator
/// object that the for-loop drives via `GenNext`. Used by the `for` lowering to
/// distinguish real generators from other `Iterator[T]`-typed expressions
/// (e.g. `Dict.items()`), which must NOT be driven via `GenNext`.
/// If `e` is a direct call `range(...)` with 1–3 args, return its argument
/// list. Used by the `for` lowering to emit a lazy integer counter loop.
fn range_call_args(e: &Expr) -> Option<&[ast::Arg]> {
    if let Expr::Call { callee, args, .. } = e {
        if let Expr::Ident { name, .. } = callee.as_ref() {
            if name == "range" && !args.is_empty() && args.len() <= 3 {
                return Some(args);
            }
        }
    }
    None
}

/// Compile-time constant `i64` value of `e` when it is an integer literal
/// (optionally a single unary negation), else `None`. Lets the range loop pick
/// the comparison direction (`<` vs `>`) without a runtime sign test.
fn const_i64(e: &Expr) -> Option<i64> {
    match e {
        Expr::Literal { lit: Literal::Int { value, .. }, .. } => Some(*value as i64),
        Expr::Unary { op: UnaryOp::Neg, operand, .. } => const_i64(operand).map(|v| -v),
        _ => None,
    }
}

fn iter_is_generator_call(ctx: &LowerCtx, iter: &Expr) -> bool {
    let Expr::Call { callee, .. } = iter else {
        return false;
    };
    let Expr::Ident { name, .. } = callee.as_ref() else {
        return false;
    };
    let Some(sid) = ctx
        .typed
        .resolved
        .symbols
        .lookup(ctx.typed.resolved.module_scope, name)
    else {
        return false;
    };
    if !matches!(
        ctx.typed.resolved.symbols.get(sid).kind,
        SymbolKind::Function
    ) {
        return false;
    }
    ctx.typed
        .resolved
        .function_sigs
        .get(&sid)
        .map(|sig| matches!(&sig.ret, Ty::Generic { base: TypeCtor::Iterator, .. }))
        .unwrap_or(false)
}

fn lower_call(
    fb: &mut FuncBuilder,
    ctx: &mut LowerCtx,
    callee: &Expr,
    args: &[ast::Arg],
    span: Span,
) -> ValueId {
    let ret_ty = ctx.expr_ty(span);

    // M61b: rewrite keyword / defaulted calls into positional form before any
    // arg lowering, so every existing call path below sees the unchanged ABI.
    let normalized = normalize_call_args(ctx, callee, args);
    let args: &[ast::Arg] = normalized.as_deref().unwrap_or(args);

    // P2: specialize `map`/`filter`/`reduce`/`sorted_by` when the callback is a
    // *literal lambda*. Instead of emitting a NativeCall that re-enters the
    // interpreter once per element (a boundary that dominates the M61a HOF
    // builtins), inline the lambda body into the same in-language loop that
    // comprehensions desugar to — the body then JITs as one function with no
    // per-element call. Non-literal callbacks (a closure in a variable) keep
    // the native fallback below. Only applies to the prelude builtins, never a
    // user function that happens to share the name.
    if let Expr::Ident { name, .. } = callee {
        if ctx.fn_id_by_name.get(name).is_none() {
            if let Some(v) = try_lower_hof_literal_lambda(fb, ctx, name, args, span) {
                return v;
            }
        }
    }

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

    // Wave-2 Lane A: `print(obj)` / `println(obj)` take the *raw* argument and
    // the VM's `arg_str` reads it as a string pointer. For a user-class
    // instance that pointer is an ObjectHeader, not a StringRepr, so the old
    // path printed garbage. Pre-stringify any class-typed argument through
    // `str_of_value` (→ `__str__`/`__repr__`/default repr) before the native
    // print sees it. Guarded on `print`/`println` NOT being shadowed by a
    // user function (mirrors the HOF-builtin guard above).
    if let Expr::Ident { name, .. } = callee {
        if (name == "print" || name == "println")
            && ctx.fn_id_by_name.get(name).is_none()
        {
            for (i, a) in args.iter().enumerate() {
                let aty = ctx.expr_ty(expr_span(&a.value));
                if matches!(
                    aty,
                    Ty::Class(_) | Ty::Generic { base: TypeCtor::Class(_), .. }
                ) {
                    arg_vs[i] = str_of_value(fb, ctx, arg_vs[i], &aty);
                }
            }
        }
    }

    // M62b: a call to a generator function (declared `-> Iterator[T]`) does
    // NOT run the body — it allocates a generator object. Detect this by the
    // callee's resolved return type and emit `MakeGen` instead of the usual
    // `DirectCall`. The argument values become the generator's initial
    // register window (parameters), exactly like a normal call's arguments.
    if let Expr::Ident { name, .. } = callee {
        if let Some(sid) = ctx
            .typed
            .resolved
            .symbols
            .lookup(ctx.typed.resolved.module_scope, name)
        {
            if matches!(
                ctx.typed.resolved.symbols.get(sid).kind,
                SymbolKind::Function
            ) {
                let is_generator = ctx
                    .typed
                    .resolved
                    .function_sigs
                    .get(&sid)
                    .map(|sig| matches!(&sig.ret, Ty::Generic { base: TypeCtor::Iterator, .. }))
                    .unwrap_or(false);
                if is_generator {
                    if let Some(fid) = ctx.fn_id_by_name.get(name).copied() {
                        return fb.push_value(
                            ret_ty,
                            ValueKind::Op {
                                op: IROp::MakeGen { fn_id: fid },
                                args: arg_vs,
                            },
                        );
                    }
                }
            }
        }
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
                        // M63a: every user-defined exception subclass carries
                        // an inherited `type_name` field at offset 0 (from the
                        // built-in `Exception` base).  Stamp it with the
                        // concrete class name *before* running `__init__`, so a
                        // later `raise inst` / `Throw` reads the right tag and
                        // the top-level handler / `except X` matching sees the
                        // correct type — identical to how built-in exceptions
                        // are materialised in `lower_raise`.  The user's
                        // `__init__` populates `message` (offset 8) and any
                        // extra fields.
                        if crate::types::class_is_exception(cid, ctx.class_layouts) {
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
                        }
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
                        // M35 P4-B: typed sqlite3.Connection / Cursor
                        // constructors — same shape as the M34 hook
                        // above.  Users don't normally write
                        // `Connection(h)` literally; this path keeps
                        // the IR uniform if they do, and is also used
                        // internally by the `sqlite3.open` /
                        // `Connection.query` paths through Alloc +
                        // NativeCall(Init).
                        if let Some(nid) = m35_p4b_sqlite_class_init_native_id(name) {
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
                                // Sets are DictRepr-backed too — count via
                                // the side table, not the offset-16 read.
                                Ty::Generic { base: TypeCtor::Set, .. } => {
                                    NativeFn::SetLen as u32
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
                    // M61a: higher-order builtins. `map(fn, xs)` and
                    // `filter(fn, xs)` lower 1:1 to their NativeFn with
                    // `[closure, list]`; `reduce(fn, xs, init)` to
                    // `[closure, list, init]`. The closure value is already
                    // in `arg_vs[0]` (a ClosureNew pointer or a captured
                    // function value). The VM re-enters the interpreter per
                    // element via `call_callable`.
                    if name == "map" || name == "filter" || name == "reduce" {
                        let nid = match name.as_str() {
                            "map" => NativeFn::Map as u32,
                            "filter" => NativeFn::Filter as u32,
                            _ => NativeFn::Reduce as u32,
                        };
                        return fb.push_value(
                            ret_ty,
                            ValueKind::Op {
                                op: IROp::NativeCall { native_id: nid },
                                args: arg_vs,
                            },
                        );
                    }
                    // `sorted_by(xs, key_fn)` → NativeFn::SortedBy with
                    // `[list, closure, key_tag]`. The trailing tag tells the
                    // VM how to compare the keys `key_fn` returns; infer it
                    // from the key fn's return type.
                    if name == "sorted_by" && args.len() == 2 {
                        let key_fn_ty = ctx.expr_ty(expr_span(&args[1].value));
                        let key_ret = match &key_fn_ty {
                            Ty::Function { ret, .. } => (**ret).clone(),
                            other => other.clone(),
                        };
                        let tag = sort_type_tag_for(&key_ret);
                        let tag_v = fb.push_value(
                            Ty::Primitive(PrimTy::I64),
                            ValueKind::Const(IRConst::I64(tag as i64)),
                        );
                        // arg_vs is [xs, key_fn]; NativeFn wants
                        // [list, closure, key_tag].
                        let mut sb_args = arg_vs;
                        sb_args.push(tag_v);
                        return fb.push_value(
                            ret_ty,
                            ValueKind::Op {
                                op: IROp::NativeCall { native_id: NativeFn::SortedBy as u32 },
                                args: sb_args,
                            },
                        );
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
                        // Wave-2 Lane A: `str(obj)` on a user class instance
                        // dispatches `__str__`/`__repr__` (or a default field
                        // repr) instead of falling through to `StrFromAny`,
                        // which reinterpreted the instance pointer as a string
                        // or i64 and printed garbage.
                        match arg_ty.clone() {
                            Some(Ty::Class(cid))
                            | Some(Ty::Generic { base: TypeCtor::Class(cid), .. }) => {
                                let obj = arg_vs[0];
                                return lower_str_of_class(fb, ctx, obj, cid);
                            }
                            _ => {}
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

    let recv_ty = ctx.expr_ty(expr_span(receiver));
    // M61b: rewrite keyword / defaulted user-method calls into positional
    // form before lowering args. Builtin / native methods don't carry AST
    // params and stay positional.
    let recv_class_name = match &recv_ty {
        Ty::Class(cid) | Ty::Generic { base: TypeCtor::Class(cid), .. } => {
            ctx.class_layouts.get(cid).map(|l| l.name.clone())
        }
        _ => None,
    };
    let normalized_m = recv_class_name
        .as_deref()
        .and_then(|cn| normalize_method_args(ctx, cn, method, args));
    let args: &[ast::Arg] = normalized_m.as_deref().unwrap_or(args);

    let recv = lower_expr(fb, ctx, receiver);
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
    //
    // M35 P4-A: re.Pattern method dispatch piggybacks on the same
    // hook — Pattern is is_native (handle-backed) so the vtable path
    // would skip it, and the method names ("split" / "find" / etc.)
    // collide with str-method NativeFn entries, so name-only dispatch
    // via `from_name` would misfire.
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
            // M35 P4-B: typed sqlite3.Connection / Cursor method
            // dispatch — same class-name + method-name shape as M34.
            // Connection / Cursor are `is_native: true` so the M11
            // vtable path below is skipped; we intercept here so the
            // NativeFn ids 802-808 / 812-815 fire instead of going
            // through `resolve_native_method`'s NativeFn::from_name
            // lookup (which would collide on method names like
            // `close` and dispatch the FileClose handler).
            if let Some(nid) = m35_p4b_sqlite_class_method_native_id_by_name(
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
            // M35 P4-A: re.Pattern method dispatch (same shape as P4-B).
            if let Some(nid) = m35_re_pattern_method_native_id_by_name(
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
            // M37: `tabular` Column / DataFrame method dispatch.  Same
            // class-name + method-name shape as M34/M35 above.  We
            // intercept here because methods like `select`/`drop`/`head`/
            // `tail`/`get`/`length` would otherwise be misdispatched by
            // the M11 vtable path or by `resolve_native_method`'s
            // name-only `from_name` lookup.
            if let Some(nid) = m37_tabular_class_method_native_id_by_name(
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
            // M38: `tabular` round-out method dispatch — typed
            // accessors, restored comparison ops, per-column aggs,
            // describe / fill_null, group_by, and GroupedDataFrame
            // methods.  Same class-name + method-name shape as M37.
            if let Some(nid) = m38_tabular_class_method_native_id_by_name(
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
            // M39: `tabular` reshape methods — typed `unique_*` per dtype,
            // `value_counts`, `merge`, `pivot`, `melt`.  Same class-name +
            // method-name shape as M37/M38.
            if let Some(nid) = m39_tabular_class_method_native_id_by_name(
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
            // M40: `tabular` time-series + cumulative + null + iloc methods.
            // Same class-name + method-name shape as M37/M38/M39.
            if let Some(nid) = m40_tabular_class_method_native_id_by_name(
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
            // M41: `tabular` DatetimeIndex + pivot_table methods.  Same
            // class-name + method-name shape as M37/M38/M39/M40.  All
            // dispatched on DataFrame.
            if let Some(nid) = m41_tabular_class_method_native_id_by_name(
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
            // M51: chainable RollingWindow.  `df.rolling*` constructors
            // live on DataFrame and return a RollingWindow; the
            // aggregator methods (`.sum/.mean/...`) live on
            // RollingWindow itself and return a DataFrame.
            if let Some(nid) = m51_tabular_class_method_native_id_by_name(
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

    // Set methods: `add`/`has` need the canonicalisation TypeTag appended
    // (same shape as the sort tag injection below); `length` maps to the
    // side-table count.
    if let Ty::Generic { base: TypeCtor::Set, .. } = &recv_ty {
        let nid = match method {
            "add" => Some(NativeFn::SetAdd),
            "has" | "contains" => Some(NativeFn::SetHas),
            _ => None,
        };
        if let Some(nid) = nid {
            let tag = set_elem_tag_for(&recv_ty);
            let tag_v = fb.push_value(
                Ty::Primitive(PrimTy::I64),
                ValueKind::Const(IRConst::I64(tag as i64)),
            );
            arg_vs.push(tag_v);
            return fb.push_value(
                ret_ty,
                ValueKind::Op {
                    op: IROp::NativeCall { native_id: nid as u32 },
                    args: arg_vs,
                },
            );
        }
        if method == "length" || method == "len" {
            return fb.push_value(
                ret_ty,
                ValueKind::Op {
                    op: IROp::NativeCall { native_id: NativeFn::SetLen as u32 },
                    args: arg_vs,
                },
            );
        }
    }

    // `.length()` on the built-in containers (guide §6.3/§6.4 documents it
    // alongside `len(x)`): dispatch on the receiver type — the name has no
    // NativeFn::from_name entry, so without this it lands on Unknown.
    if method == "length" {
        let nid = match &recv_ty {
            Ty::Generic { base: TypeCtor::List, .. } => Some(NativeFn::ListLen as u32),
            Ty::Generic { base: TypeCtor::Dict, .. } => Some(NativeFn::DictLen as u32),
            Ty::Primitive(PrimTy::Str) => Some(NativeFn::Len as u32),
            _ => None,
        };
        if let Some(nid) = nid {
            return fb.push_value(
                ret_ty,
                ValueKind::Op { op: IROp::NativeCall { native_id: nid }, args: arg_vs },
            );
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

    // M61a: `xs.sort_by(key_fn)` over `List[T]` — in-place sort by a user
    // key function. NativeFn::ListSortBy wants [list, closure, key_tag];
    // `arg_vs` is already [list, closure], so append the key-type tag
    // inferred from the key fn's return type.
    // P2: in-place `xs.sort_by(fn(s) -> K: ...)` with a *literal lambda* key.
    // Schwartzian transform inlined: build a parallel key list via the inlined
    // key fn (no per-element interpreter re-entry), then sort `xs` in place by
    // the precomputed keys (one native call). `recv` is the already-lowered
    // list value. Non-literal callbacks fall through to the native path below.
    if method == "sort_by" && args.len() == 1 {
        if let Ty::Generic { base: TypeCtor::List, .. } = &recv_ty {
            if let Expr::Lambda { params, body, return_ty, .. } = &args[0].value {
                if params.len() == 1 {
                    let lam_ret = lambda_ret_ty(ctx, return_ty);
                    let key_tag = sort_type_tag_for(&lam_ret);
                    if matches!(key_tag, 3 | 9 | 11) {
                        let i64_ty = Ty::Primitive(PrimTy::I64);
                        let elem_ty = lambda_param_ty(ctx, &params[0]);
                        let key_list_ty = Ty::Generic {
                            base: TypeCtor::List,
                            args: vec![lam_ret.clone()],
                        };
                        // keys = fresh List[KeyTy]
                        let keys = fb.push_value(
                            key_list_ty.clone(),
                            ValueKind::Op { op: IROp::ArrayNew, args: vec![] },
                        );
                        let keys_slot = {
                            let n = format!("__hof_k_{}", fb.slot_ty.len());
                            let s = fb.alloc_slot(&n, key_list_ty.clone());
                            fb.emit_write_local(s, keys);
                            s
                        };
                        let zero =
                            fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(0)));
                        let i_slot = {
                            let n = format!("__hof_i_{}", fb.slot_ty.len());
                            let s = fb.alloc_slot(&n, i64_ty.clone());
                            fb.emit_write_local(s, zero);
                            s
                        };
                        let len_v = fb.push_value(
                            i64_ty.clone(),
                            ValueKind::Op { op: IROp::ArrayLen, args: vec![recv] },
                        );
                        let n_slot = {
                            let n = format!("__hof_n_{}", fb.slot_ty.len());
                            let s = fb.alloc_slot(&n, i64_ty.clone());
                            fb.emit_write_local(s, len_v);
                            s
                        };
                        let var_slot = fb.alloc_slot(&params[0].name, elem_ty.clone());

                        let header = fb.new_block();
                        let body_b = fb.new_block();
                        let exit = fb.new_block();
                        fb.terminate(Terminator::Branch { target: header });

                        fb.switch_to(header);
                        let i_cur = fb.emit_read_local(i_slot);
                        let n_cur = fb.emit_read_local(n_slot);
                        let test = fb.push_value(
                            Ty::Primitive(PrimTy::Bool),
                            ValueKind::Op { op: IROp::ILt, args: vec![i_cur, n_cur] },
                        );
                        fb.terminate(Terminator::CondBranch {
                            cond: test,
                            t: body_b,
                            f: exit,
                        });

                        fb.switch_to(body_b);
                        let i_now = fb.emit_read_local(i_slot);
                        let elt = fb.push_value(
                            elem_ty.clone(),
                            ValueKind::Op { op: IROp::ArrayGet, args: vec![recv, i_now] },
                        );
                        fb.emit_write_local(var_slot, elt);
                        let kv = lower_expr(fb, ctx, body);
                        let keys_v = fb.emit_read_local(keys_slot);
                        fb.push_value(
                            Ty::Primitive(PrimTy::Unit),
                            ValueKind::Op { op: IROp::ListPush, args: vec![keys_v, kv] },
                        );

                        let i_again = fb.emit_read_local(i_slot);
                        let one =
                            fb.push_value(i64_ty.clone(), ValueKind::Const(IRConst::I64(1)));
                        let next_i = fb.push_value(
                            i64_ty.clone(),
                            ValueKind::Op { op: IROp::IAdd, args: vec![i_again, one] },
                        );
                        fb.emit_write_local(i_slot, next_i);
                        fb.terminate(Terminator::Branch { target: header });

                        fb.switch_to(exit);
                        let keys_v = fb.emit_read_local(keys_slot);
                        let tag_v = fb.push_value(
                            i64_ty.clone(),
                            ValueKind::Const(IRConst::I64(key_tag as i64)),
                        );
                        return fb.push_value(
                            ret_ty,
                            ValueKind::Op {
                                op: IROp::NativeCall {
                                    native_id: NativeFn::SortByPrecomputed as u32,
                                },
                                args: vec![recv, keys_v, tag_v],
                            },
                        );
                    }
                }
            }
        }
    }

    if method == "sort_by" {
        if let Ty::Generic { base: TypeCtor::List, .. } = &recv_ty {
            if let Some(key_arg) = args.first() {
                let key_fn_ty = ctx.expr_ty(expr_span(&key_arg.value));
                let key_ret = match &key_fn_ty {
                    Ty::Function { ret, .. } => (**ret).clone(),
                    other => other.clone(),
                };
                let tag = sort_type_tag_for(&key_ret);
                let tag_v = fb.push_value(
                    Ty::Primitive(PrimTy::I64),
                    ValueKind::Const(IRConst::I64(tag as i64)),
                );
                arg_vs.push(tag_v);
                return fb.push_value(
                    ret_ty,
                    ValueKind::Op {
                        op: IROp::NativeCall { native_id: NativeFn::ListSortBy as u32 },
                        args: arg_vs,
                    },
                );
            }
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

/// WAVE-2 LANE-0 (scaffold): resolve a dunder method on a user class to the
/// call dispatch target a feature lane should emit.
///
/// This is the reusable foundation for operator-overloading / protocol lanes
/// (`__str__`, `__add__`, `__getitem__`, `__iter__`, `__next__`, `__eq__`,
/// `__lt__`, …). It mirrors the user-class method-resolution path inside
/// [`lower_method_call`] (ir.rs ~6957-6996) **exactly**, so the dispatch
/// target it returns is bit-for-bit what a hand-written `recv.__dunder__(...)`
/// method call would lower to.
///
/// Contract:
/// - Returns [`IROp::DirectCall`] `{ fn_id }` when the receiver class is
///   neither `open` nor `sealed` (the devirtualisation rule at ir.rs:6978)
///   **and** the dunder is defined directly under that class's own name
///   (`"Class.__dunder__"` is present in `fn_id_by_name`). This is the same
///   "only devirtualise when the static type forbids overriding subclasses"
///   condition the normal method path uses — an inherited-but-not-overridden
///   dunder on a final class therefore falls through to a `VirtualCall`,
///   identically to a normal inherited method (see the vtable-fill walk in
///   Pass 2, ir.rs ~556-577).
/// - Returns [`IROp::VirtualCall`] `{ vtable_slot }` otherwise, where
///   `vtable_slot` is the index of the dunder in the class's *virtual* method
///   list — i.e. `methods` with `__init__` filtered out — matching the slot
///   numbering codegen/resolver use.
/// - Returns `None` when the class does not define **or inherit** the dunder.
///   (Inherited dunders are already flattened into `layout.methods` by the
///   resolver — see resolver.rs ~7532-7568 — so a plain name lookup resolves
///   the parent chain.)
/// - Returns `None` for built-in / native runtime classes (`is_native`),
///   whose methods dispatch through `NativeFn`, not a vtable.
///
/// A feature lane evaluates the operands, then drops the returned `IROp`
/// straight into `fb.push_value(ret_ty, ValueKind::Op { op, args })`.
///
/// `fn_id_by_name` and `class_layouts` are the same maps carried by
/// [`LowerCtx`]; pass `ctx.fn_id_by_name` and `ctx.class_layouts`.
#[allow(dead_code)]
fn class_dunder_dispatch(
    class_layouts: &HashMap<ClassId, ClassLayout>,
    fn_id_by_name: &HashMap<String, FuncId>,
    cid: ClassId,
    dunder: &str,
) -> Option<IROp> {
    let layout = class_layouts.get(&cid)?;
    // Built-in handle-backed classes have no real vtable; never dispatch a
    // dunder through one (mirrors the `!layout.is_native` guard upstream).
    if layout.is_native {
        return None;
    }
    // Slot = index in the *virtual* method list (`__init__` excluded), exactly
    // as `lower_method_call` and the codegen vtable builder compute it.
    let slot = layout
        .methods
        .iter()
        .filter(|m| m.name != "__init__")
        .position(|m| m.name == dunder)?;
    // Devirtualise to a DirectCall only when the static receiver type forbids
    // overriding subclasses (neither `open` nor `sealed`) and the dunder is
    // defined directly under this class's name. Otherwise dispatch virtually
    // so a subclass override is honoured.
    if !layout.is_open && !layout.is_sealed {
        let key = format!("{}.{}", layout.name, dunder);
        if let Some(fid) = fn_id_by_name.get(&key).copied() {
            return Some(IROp::DirectCall { fn_id: fid });
        }
    }
    Some(IROp::VirtualCall { vtable_slot: slot as u32 })
}

/// Wave 2 / Lane C: resolve the dispatch op for an index dunder
/// (`__getitem__` / `__setitem__`) on a user-class receiver, covering both
/// plain and parameterised generic classes.
///
///  - Plain class (`Ty::Class(cid)`): delegate to `class_dunder_dispatch`,
///    which emits a `DirectCall` (final class, own method) or `VirtualCall`.
///  - Generic instance (`Ty::Generic { Class(cid), targs }`): mirror the M31
///    method-call path (ir.rs ~6817) — each fully-applied instantiation has its
///    own monomorphised method body, looked up in `class_inst_method_fn` by
///    `(cid, mangle_args_key(targs), dunder)`, and dispatched via `DirectCall`.
///
/// Returns `None` for any non-class receiver, a class lacking the dunder, or a
/// generic instance whose type args aren't fully resolved yet.
fn class_index_dunder_op(
    class_layouts: &HashMap<ClassId, ClassLayout>,
    fn_id_by_name: &HashMap<String, FuncId>,
    class_inst_method_fn: &HashMap<(ClassId, String, String), FuncId>,
    recv_ty: &Ty,
    dunder: &str,
) -> Option<IROp> {
    match recv_ty {
        Ty::Class(cid) => {
            class_dunder_dispatch(class_layouts, fn_id_by_name, *cid, dunder)
        }
        Ty::Generic { base: TypeCtor::Class(cid), args: targs } => {
            if targs.iter().any(has_unbound_var) {
                return None;
            }
            let key = mangle_args_key(targs);
            class_inst_method_fn
                .get(&(*cid, key, dunder.to_string()))
                .copied()
                .map(|fid| IROp::DirectCall { fn_id: fid })
        }
        _ => None,
    }
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

/// Map a `Set[T]` (or a bare element type) to the TypeTag operand
/// `SetAdd`/`SetHas` expect — see vm/src/builtins.rs::set_elem_key.
/// Integer widths, bool and char canonicalise via their value (I64),
/// floats via their bit pattern (F64), strings via their content (Ref).
/// The typechecker restricts set elements to these types; the I64
/// fallback keys anything that slips through by raw bits, which never
/// dereferences the value.
fn set_elem_tag_for(set_ty: &Ty) -> u8 {
    use strictpy_shared::TypeTag;
    let elem = match set_ty {
        Ty::Generic { args, .. } if !args.is_empty() => &args[0],
        other => other,
    };
    let elem = match elem {
        Ty::Nullable(inner) => inner.as_ref(),
        other => other,
    };
    match elem {
        Ty::Primitive(p) if p.is_float() => TypeTag::F64 as u8,
        Ty::Primitive(PrimTy::Str) => TypeTag::Ref as u8,
        _ => TypeTag::I64 as u8,
    }
}

/// Pick the right `NativeFn` for `receiver.method(...)` given the static
/// receiver type. Falls back to `NativeFn::from_name` when the method name
/// is unambiguous across runtime classes.  (Pattern + JList + JObject
/// method dispatch is handled by class-name-based shortcircuits in
/// `lower_method_call` upstream of this function — see
/// `m35_re_pattern_method_native_id_by_name` and
/// `m34_json_class_method_native_id_by_name`.)
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
            "remove" => NativeFn::DictRemove as u32,
            _ => NativeFn::from_name(method)
                .map(|n| n as u32)
                .unwrap_or(NativeFn::Unknown as u32),
        };
    }
    // Strings round 2: str-receiver overrides, intercepted BEFORE the
    // bare from_name fallback. `join` would otherwise resolve to
    // ThreadJoin (from_name's first match); `char_at` historically had no
    // from_name entry at all, so `s.char_at(i)` compiled to
    // NativeFn::Unknown and trapped at runtime with "unknown native id"
    // (REPORT_V2 bug #7).
    if let Ty::Primitive(PrimTy::Str) = recv_ty {
        return match method {
            "join"    => NativeFn::StrJoin   as u32,
            "lower"   => NativeFn::StrLower  as u32,
            "upper"   => NativeFn::StrUpper  as u32,
            "repeat"  => NativeFn::StrRepeat as u32,
            "char_at" => NativeFn::StrCharAt as u32,
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

/// M35 P4-B: map a constructor name to the matching
/// `Sqlite3{Connection,Cursor}Init` NativeFn id, or `None` if `name`
/// isn't one of the typed sqlite3 classes.  Mirrors
/// `m34_json_class_init_native_id` — used by `lower_call` to route
/// `Connection(handle)` / `Cursor(handle)` through a native initialiser
/// instead of looking for a user `__init__`.  Programs do not
/// normally call these constructors directly (the `sqlite3.open` /
/// `Connection.query` paths allocate them internally), but having the
/// constructor hook here keeps the IR shape uniform with the M34
/// JsonValue family.
fn m35_p4b_sqlite_class_init_native_id(name: &str) -> Option<u32> {
    Some(match name {
        "Connection" => NativeFn::Sqlite3ConnectionInit as u32,
        "Cursor"     => NativeFn::Sqlite3CursorInit     as u32,
        _ => return None,
    })
}

/// M35 P4-B: dispatch a typed sqlite3 class method by class name +
/// method name.  Same shape as M34's JList/JObject dispatcher.
fn m35_p4b_sqlite_class_method_native_id_by_name(
    class_name: &str,
    method: &str,
) -> Option<u32> {
    Some(match (class_name, method) {
        ("Connection", "execute")            => NativeFn::Sqlite3ConnectionExecute            as u32,
        ("Connection", "execute_params")     => NativeFn::Sqlite3ConnectionExecuteParams     as u32,
        ("Connection", "query")              => NativeFn::Sqlite3ConnectionQuery              as u32,
        ("Connection", "query_params")       => NativeFn::Sqlite3ConnectionQueryParams       as u32,
        ("Connection", "last_insert_rowid")  => NativeFn::Sqlite3ConnectionLastInsertRowid  as u32,
        ("Connection", "changes")            => NativeFn::Sqlite3ConnectionChanges            as u32,
        ("Connection", "close")              => NativeFn::Sqlite3ConnectionClose              as u32,
        ("Cursor",     "fetchone")           => NativeFn::Sqlite3CursorFetchOne           as u32,
        ("Cursor",     "fetchall")           => NativeFn::Sqlite3CursorFetchAll           as u32,
        ("Cursor",     "column_names")       => NativeFn::Sqlite3CursorColumnNames       as u32,
        ("Cursor",     "row_count")          => NativeFn::Sqlite3CursorRowCount          as u32,
        _ => return None,
    })
}

/// M37: dispatch a `tabular` Column / DataFrame method by class name
/// + method name.  All classes are non-native (real heap layouts) so
/// the M11 vtable path would normally apply, but their method names
/// collide with str / list / Dict methods (notably `get`, `length`,
/// `keys`, `select`, `drop`, `head`, `tail`), so we intercept here
/// before reaching `resolve_native_method`.  Returns `None` for any
/// other class so the caller continues through the regular dispatch.
///
/// Per the M37 brief, all locals in shared compiler files use the
/// `m37_` prefix.
fn m37_tabular_class_method_native_id_by_name(
    class_name: &str,
    method: &str,
) -> Option<u32> {
    Some(match (class_name, method) {
        // Shared per-Column inspection (same handler for all 5 subclasses
        // — the handler reads payload offsets that are identical across
        // every Column subclass layout).
        ("ColumnI64",         "length") => NativeFn::M37TabColLength as u32,
        ("ColumnF64",         "length") => NativeFn::M37TabColLength as u32,
        ("ColumnStr",         "length") => NativeFn::M37TabColLength as u32,
        ("ColumnBool",        "length") => NativeFn::M37TabColLength as u32,
        ("ColumnDateTime",    "length") => NativeFn::M37TabColLength as u32,
        // M47: ColumnCategorical uses the same length handler — its
        // payload's `length` slot is at offset 16, matching the M37
        // Column layout exactly (see m47_alloc_col_categorical).
        ("ColumnCategorical", "length") => NativeFn::M37TabColLength as u32,
        ("ColumnI64",         "dtype")  => NativeFn::M37TabColDtype as u32,
        ("ColumnF64",         "dtype")  => NativeFn::M37TabColDtype as u32,
        ("ColumnStr",         "dtype")  => NativeFn::M37TabColDtype as u32,
        ("ColumnBool",        "dtype")  => NativeFn::M37TabColDtype as u32,
        ("ColumnDateTime",    "dtype")  => NativeFn::M37TabColDtype as u32,
        ("ColumnCategorical", "dtype")  => NativeFn::M37TabColDtype as u32,
        ("ColumnI64",         "is_null") => NativeFn::M37TabColIsNull as u32,
        ("ColumnF64",         "is_null") => NativeFn::M37TabColIsNull as u32,
        ("ColumnStr",         "is_null") => NativeFn::M37TabColIsNull as u32,
        ("ColumnBool",        "is_null") => NativeFn::M37TabColIsNull as u32,
        ("ColumnDateTime",    "is_null") => NativeFn::M37TabColIsNull as u32,
        ("ColumnCategorical", "is_null") => NativeFn::M37TabColIsNull as u32,
        ("ColumnI64",         "null_count") => NativeFn::M37TabColNullCount as u32,
        ("ColumnF64",         "null_count") => NativeFn::M37TabColNullCount as u32,
        ("ColumnStr",         "null_count") => NativeFn::M37TabColNullCount as u32,
        ("ColumnBool",        "null_count") => NativeFn::M37TabColNullCount as u32,
        ("ColumnDateTime",    "null_count") => NativeFn::M37TabColNullCount as u32,
        ("ColumnCategorical", "null_count") => NativeFn::M37TabColNullCount as u32,
        // Per-type typed getters (return T?).
        ("ColumnI64",      "get") => NativeFn::M37TabColI64Get as u32,
        ("ColumnF64",      "get") => NativeFn::M37TabColF64Get as u32,
        ("ColumnStr",      "get") => NativeFn::M37TabColStrGet as u32,
        ("ColumnBool",     "get") => NativeFn::M37TabColBoolGet as u32,
        ("ColumnDateTime", "get_ms") => NativeFn::M37TabColDateTimeGetMs as u32,
        // M47: categorical get returns the category string (or none).
        ("ColumnCategorical", "get") => NativeFn::M47TabColCategoricalGet as u32,
        // Phase C: per-column comparison ops → ColumnBool mask.
        ("ColumnI64",  "eq")       => NativeFn::M37TabColI64Eq as u32,
        ("ColumnI64",  "gt")       => NativeFn::M37TabColI64Gt as u32,
        ("ColumnI64",  "lt")       => NativeFn::M37TabColI64Lt as u32,
        ("ColumnF64",  "eq")       => NativeFn::M37TabColF64Eq as u32,
        ("ColumnF64",  "gt")       => NativeFn::M37TabColF64Gt as u32,
        ("ColumnF64",  "lt")       => NativeFn::M37TabColF64Lt as u32,
        ("ColumnStr",  "eq")       => NativeFn::M37TabColStrEq as u32,
        ("ColumnStr",  "contains") => NativeFn::M37TabColStrContains as u32,
        ("ColumnBool", "and_")       => NativeFn::M37TabMaskAnd as u32,
        ("ColumnBool", "or_")        => NativeFn::M37TabMaskOr as u32,
        ("ColumnBool", "not_")       => NativeFn::M37TabMaskNot as u32,
        ("ColumnBool", "count_true") => NativeFn::M37TabMaskCountTrue as u32,
        // DataFrame methods (handler reads the layout offsets directly).
        ("DataFrame", "length")     => NativeFn::M37TabDfLength as u32,
        ("DataFrame", "ncols")      => NativeFn::M37TabDfNcols as u32,
        ("DataFrame", "columns")    => NativeFn::M37TabDfColumns as u32,
        ("DataFrame", "dtypes")     => NativeFn::M37TabDfDtypes as u32,
        ("DataFrame", "has_column") => NativeFn::M37TabDfHasColumn as u32,
        ("DataFrame", "show")       => NativeFn::M37TabDfShow as u32,
        ("DataFrame", "filter")     => NativeFn::M37TabDfFilter as u32,
        ("DataFrame", "select")     => NativeFn::M37TabDfSelect as u32,
        ("DataFrame", "drop")       => NativeFn::M37TabDfDrop as u32,
        ("DataFrame", "head")       => NativeFn::M37TabDfHead as u32,
        ("DataFrame", "tail")       => NativeFn::M37TabDfTail as u32,
        ("DataFrame", "row")        => NativeFn::M37TabDfRow as u32,
        ("DataFrame", "sort_by")    => NativeFn::M37TabDfSortBy as u32,
        _ => return None,
    })
}

/// M38: dispatch a `tabular` Column / DataFrame / GroupedDataFrame
/// method *added by M38* by class name + method name.  Mirrors
/// `m37_tabular_class_method_native_id_by_name` — every method here
/// is either a typed accessor (Phase A), a restored comparison op
/// (Phase A), a per-column aggregation (Phase B), `df.describe`
/// (Phase C), `Column.fill_null` (Phase C), `df.group_by` /
/// `GroupedDataFrame.*` (Phase D).
///
/// Per the M38 brief, all locals in shared compiler files use the
/// `m38_` prefix.
fn m38_tabular_class_method_native_id_by_name(
    class_name: &str,
    method: &str,
) -> Option<u32> {
    Some(match (class_name, method) {
        // ── Phase A: typed DataFrame accessors ──
        ("DataFrame", "get_column_i64")      => NativeFn::M38TabDfGetColumnI64 as u32,
        ("DataFrame", "get_column_f64")      => NativeFn::M38TabDfGetColumnF64 as u32,
        ("DataFrame", "get_column_str")      => NativeFn::M38TabDfGetColumnStr as u32,
        ("DataFrame", "get_column_bool")     => NativeFn::M38TabDfGetColumnBool as u32,
        ("DataFrame", "get_column_datetime") => NativeFn::M38TabDfGetColumnDateTime as u32,
        // ── Phase A: restored Phase C comparison ops ──
        ("ColumnI64", "ne")      => NativeFn::M38TabColI64Ne as u32,
        ("ColumnI64", "ge")      => NativeFn::M38TabColI64Ge as u32,
        ("ColumnI64", "le")      => NativeFn::M38TabColI64Le as u32,
        ("ColumnI64", "between") => NativeFn::M38TabColI64Between as u32,
        ("ColumnF64", "ne")      => NativeFn::M38TabColF64Ne as u32,
        ("ColumnF64", "ge")      => NativeFn::M38TabColF64Ge as u32,
        ("ColumnF64", "le")      => NativeFn::M38TabColF64Le as u32,
        ("ColumnF64", "between") => NativeFn::M38TabColF64Between as u32,
        ("ColumnStr", "starts_with") => NativeFn::M38TabColStrStartsWith as u32,
        ("ColumnStr", "ends_with")   => NativeFn::M38TabColStrEndsWith as u32,
        // ── Phase A: rename ──
        ("DataFrame", "rename") => NativeFn::M38TabDfRename as u32,
        // ── Phase B: per-column aggregations ──
        ("ColumnI64", "sum")    => NativeFn::M38TabColI64Sum as u32,
        ("ColumnI64", "mean")   => NativeFn::M38TabColI64Mean as u32,
        ("ColumnI64", "min")    => NativeFn::M38TabColI64Min as u32,
        ("ColumnI64", "max")    => NativeFn::M38TabColI64Max as u32,
        ("ColumnI64", "count")  => NativeFn::M38TabColI64Count as u32,
        ("ColumnI64", "std")    => NativeFn::M38TabColI64Std as u32,
        ("ColumnI64", "var")    => NativeFn::M38TabColI64Var as u32,
        ("ColumnI64", "median") => NativeFn::M38TabColI64Median as u32,
        ("ColumnF64", "sum")    => NativeFn::M38TabColF64Sum as u32,
        ("ColumnF64", "mean")   => NativeFn::M38TabColF64Mean as u32,
        ("ColumnF64", "min")    => NativeFn::M38TabColF64Min as u32,
        ("ColumnF64", "max")    => NativeFn::M38TabColF64Max as u32,
        ("ColumnF64", "count")  => NativeFn::M38TabColF64Count as u32,
        ("ColumnF64", "std")    => NativeFn::M38TabColF64Std as u32,
        ("ColumnF64", "var")    => NativeFn::M38TabColF64Var as u32,
        ("ColumnF64", "median") => NativeFn::M38TabColF64Median as u32,
        ("ColumnStr", "count")  => NativeFn::M38TabColStrCount as u32,
        ("ColumnStr", "min")    => NativeFn::M38TabColStrMin as u32,
        ("ColumnStr", "max")    => NativeFn::M38TabColStrMax as u32,
        ("ColumnBool", "count") => NativeFn::M38TabColBoolCount as u32,
        ("ColumnDateTime", "count") => NativeFn::M38TabColDtCount as u32,
        ("ColumnDateTime", "min")   => NativeFn::M38TabColDtMin as u32,
        ("ColumnDateTime", "max")   => NativeFn::M38TabColDtMax as u32,
        // ── Phase C ──
        ("DataFrame", "describe") => NativeFn::M38TabDfDescribe as u32,
        ("ColumnI64", "fill_null")      => NativeFn::M38TabColI64FillNull as u32,
        ("ColumnF64", "fill_null")      => NativeFn::M38TabColF64FillNull as u32,
        ("ColumnStr", "fill_null")      => NativeFn::M38TabColStrFillNull as u32,
        ("ColumnBool", "fill_null")     => NativeFn::M38TabColBoolFillNull as u32,
        ("ColumnDateTime", "fill_null") => NativeFn::M38TabColDtFillNull as u32,
        // ── Phase D: group-by ──
        ("DataFrame", "group_by")     => NativeFn::M38TabDfGroupBy as u32,
        ("GroupedDataFrame", "size")  => NativeFn::M38TabGdfSize as u32,
        ("GroupedDataFrame", "keys")  => NativeFn::M38TabGdfKeys as u32,
        ("GroupedDataFrame", "sum")   => NativeFn::M38TabGdfSum as u32,
        ("GroupedDataFrame", "mean")  => NativeFn::M38TabGdfMean as u32,
        ("GroupedDataFrame", "min")   => NativeFn::M38TabGdfMin as u32,
        ("GroupedDataFrame", "max")   => NativeFn::M38TabGdfMax as u32,
        ("GroupedDataFrame", "count") => NativeFn::M38TabGdfCount as u32,
        ("GroupedDataFrame", "agg")   => NativeFn::M38TabGdfAgg as u32,
        _ => return None,
    })
}

/// M39: dispatch a `tabular` DataFrame reshape method by class name +
/// method name.  Mirrors `m37_tabular_class_method_native_id_by_name`
/// and `m38_tabular_class_method_native_id_by_name` — every method
/// here is one of the M39 Phase 4 reshape operations.
///
/// Per the M39 brief, all locals in shared compiler files use the
/// `m39_` prefix.
fn m39_tabular_class_method_native_id_by_name(
    class_name: &str,
    method: &str,
) -> Option<u32> {
    Some(match (class_name, method) {
        // ── Phase A: typed unique accessors (one per dtype) ──
        ("DataFrame", "unique_i64")      => NativeFn::M39TabDfUniqueI64 as u32,
        ("DataFrame", "unique_f64")      => NativeFn::M39TabDfUniqueF64 as u32,
        ("DataFrame", "unique_str")      => NativeFn::M39TabDfUniqueStr as u32,
        ("DataFrame", "unique_bool")     => NativeFn::M39TabDfUniqueBool as u32,
        ("DataFrame", "unique_datetime") => NativeFn::M39TabDfUniqueDateTime as u32,
        // ── Phase A: value_counts ──
        ("DataFrame", "value_counts")    => NativeFn::M39TabDfValueCounts as u32,
        // ── Phase B: merge ──
        ("DataFrame", "merge")           => NativeFn::M39TabDfMerge as u32,
        // ── Phase C: pivot + melt ──
        ("DataFrame", "pivot")           => NativeFn::M39TabDfPivot as u32,
        ("DataFrame", "melt")            => NativeFn::M39TabDfMelt as u32,
        _ => return None,
    })
}

/// M40: dispatch a `tabular` time-series / cumulative / null-handling /
/// iloc method by class name + method name.  Mirrors
/// `m39_tabular_class_method_native_id_by_name`.
///
/// Per the M40 brief, all locals in shared compiler files use the
/// `m40_` prefix.
fn m40_tabular_class_method_native_id_by_name(
    class_name: &str,
    method: &str,
) -> Option<u32> {
    Some(match (class_name, method) {
        // ── Phase A: cumulative reductions ──
        ("ColumnI64", "cumsum")  => NativeFn::M40TabColI64Cumsum  as u32,
        ("ColumnI64", "cumprod") => NativeFn::M40TabColI64Cumprod as u32,
        ("ColumnI64", "cummax")  => NativeFn::M40TabColI64Cummax  as u32,
        ("ColumnI64", "cummin")  => NativeFn::M40TabColI64Cummin  as u32,
        ("ColumnF64", "cumsum")  => NativeFn::M40TabColF64Cumsum  as u32,
        ("ColumnF64", "cumprod") => NativeFn::M40TabColF64Cumprod as u32,
        ("ColumnF64", "cummax")  => NativeFn::M40TabColF64Cummax  as u32,
        ("ColumnF64", "cummin")  => NativeFn::M40TabColF64Cummin  as u32,
        // ── Phase A: whole-frame null handling ──
        ("DataFrame", "dropna")          => NativeFn::M40TabDfDropna         as u32,
        ("DataFrame", "dropna_subset")   => NativeFn::M40TabDfDropnaSubset   as u32,
        ("DataFrame", "fillna_i64")      => NativeFn::M40TabDfFillnaI64      as u32,
        ("DataFrame", "fillna_f64")      => NativeFn::M40TabDfFillnaF64      as u32,
        ("DataFrame", "fillna_str")      => NativeFn::M40TabDfFillnaStr      as u32,
        ("DataFrame", "fillna_bool")     => NativeFn::M40TabDfFillnaBool     as u32,
        ("DataFrame", "fillna_datetime") => NativeFn::M40TabDfFillnaDateTime as u32,
        // ── Phase A: range slicing ──
        ("DataFrame", "iloc") => NativeFn::M40TabDfIloc as u32,
        // ── Phase B: rolling-window aggregations ──
        ("ColumnI64", "rolling_sum")  => NativeFn::M40TabColI64RollingSum  as u32,
        ("ColumnI64", "rolling_mean") => NativeFn::M40TabColI64RollingMean as u32,
        ("ColumnI64", "rolling_min")  => NativeFn::M40TabColI64RollingMin  as u32,
        ("ColumnI64", "rolling_max")  => NativeFn::M40TabColI64RollingMax  as u32,
        ("ColumnI64", "rolling_std")  => NativeFn::M40TabColI64RollingStd  as u32,
        ("ColumnF64", "rolling_sum")  => NativeFn::M40TabColF64RollingSum  as u32,
        ("ColumnF64", "rolling_mean") => NativeFn::M40TabColF64RollingMean as u32,
        ("ColumnF64", "rolling_min")  => NativeFn::M40TabColF64RollingMin  as u32,
        ("ColumnF64", "rolling_max")  => NativeFn::M40TabColF64RollingMax  as u32,
        ("ColumnF64", "rolling_std")  => NativeFn::M40TabColF64RollingStd  as u32,
        // ── Phase C: time-series ops ──
        ("DataFrame", "resample")    => NativeFn::M40TabDfResample   as u32,
        ("DataFrame", "asof_merge")  => NativeFn::M40TabDfAsofMerge  as u32,
        _ => return None,
    })
}

/// M41: dispatch a `tabular` DatetimeIndex / pivot_table method by class
/// name + method name.  Mirrors `m40_tabular_class_method_native_id_by_name`.
///
/// Per the M41 brief, all locals in shared compiler files use the
/// `m41_` prefix.
fn m41_tabular_class_method_native_id_by_name(
    class_name: &str,
    method: &str,
) -> Option<u32> {
    Some(match (class_name, method) {
        // ── Phase A: index storage + accessors + sort_index ──
        ("DataFrame", "set_index")    => NativeFn::M41TabDfSetIndex    as u32,
        ("DataFrame", "reset_index")  => NativeFn::M41TabDfResetIndex  as u32,
        ("DataFrame", "has_index")    => NativeFn::M41TabDfHasIndex    as u32,
        ("DataFrame", "index")        => NativeFn::M41TabDfIndex       as u32,
        ("DataFrame", "index_name")   => NativeFn::M41TabDfIndexName   as u32,
        ("DataFrame", "sort_index")   => NativeFn::M41TabDfSortIndex   as u32,
        // ── Phase B: index-aware time-series + select by label ──
        ("DataFrame", "resample_index")    => NativeFn::M41TabDfResampleIndex      as u32,
        ("DataFrame", "asof_merge_index")  => NativeFn::M41TabDfAsofMergeIndex     as u32,
        ("DataFrame", "select_by_label_i64")      => NativeFn::M41TabDfSelectByLabelI64      as u32,
        ("DataFrame", "select_by_label_str")      => NativeFn::M41TabDfSelectByLabelStr      as u32,
        ("DataFrame", "select_by_label_datetime") => NativeFn::M41TabDfSelectByLabelDateTime as u32,
        // ── Phase C: pivot_table ──
        ("DataFrame", "pivot_table")  => NativeFn::M41TabDfPivotTable  as u32,
        // ── M44 Phase A: MultiIndex storage + accessors + sort_index_multi ──
        ("DataFrame", "set_index_multi")    => NativeFn::M44TabDfSetIndexMulti    as u32,
        ("DataFrame", "reset_index_multi")  => NativeFn::M44TabDfResetIndexMulti  as u32,
        ("DataFrame", "index_nlevels")      => NativeFn::M44TabDfIndexNlevels     as u32,
        ("DataFrame", "index_level")        => NativeFn::M44TabDfIndexLevel       as u32,
        ("DataFrame", "index_level_name")   => NativeFn::M44TabDfIndexLevelName   as u32,
        ("DataFrame", "sort_index_multi")   => NativeFn::M44TabDfSortIndexMulti   as u32,
        // ── M46: stack/unstack + loc_range + set_index_list + pivot_table extras ──
        ("DataFrame", "stack")                  => NativeFn::M46TabDfStack                  as u32,
        ("DataFrame", "unstack")                => NativeFn::M46TabDfUnstack                as u32,
        ("DataFrame", "loc_range_i64")          => NativeFn::M46TabDfLocRangeI64            as u32,
        ("DataFrame", "loc_range_f64")          => NativeFn::M46TabDfLocRangeF64            as u32,
        ("DataFrame", "loc_range_str")          => NativeFn::M46TabDfLocRangeStr            as u32,
        ("DataFrame", "loc_range_bool")         => NativeFn::M46TabDfLocRangeBool           as u32,
        ("DataFrame", "loc_range_datetime")     => NativeFn::M46TabDfLocRangeDateTime       as u32,
        ("DataFrame", "set_index_list")         => NativeFn::M46TabDfSetIndexList           as u32,
        ("DataFrame", "pivot_table_aggfunc_list") => NativeFn::M46TabDfPivotTableAggfuncList as u32,
        ("DataFrame", "pivot_table_margins")    => NativeFn::M46TabDfPivotTableMargins      as u32,
        // ── M47: tabular polish — iloc_2d + rolling_*_min_periods +
        //   ColumnCategorical + df.get_column_categorical ──
        ("DataFrame", "iloc_2d")                => NativeFn::M47TabDfIloc2d                  as u32,
        ("ColumnI64", "rolling_sum_min_periods")  => NativeFn::M47TabColI64RollingSumMinPeriods  as u32,
        ("ColumnI64", "rolling_mean_min_periods") => NativeFn::M47TabColI64RollingMeanMinPeriods as u32,
        ("ColumnI64", "rolling_min_min_periods")  => NativeFn::M47TabColI64RollingMinMinPeriods  as u32,
        ("ColumnI64", "rolling_max_min_periods")  => NativeFn::M47TabColI64RollingMaxMinPeriods  as u32,
        ("ColumnI64", "rolling_std_min_periods")  => NativeFn::M47TabColI64RollingStdMinPeriods  as u32,
        ("ColumnF64", "rolling_sum_min_periods")  => NativeFn::M47TabColF64RollingSumMinPeriods  as u32,
        ("ColumnF64", "rolling_mean_min_periods") => NativeFn::M47TabColF64RollingMeanMinPeriods as u32,
        ("ColumnF64", "rolling_min_min_periods")  => NativeFn::M47TabColF64RollingMinMinPeriods  as u32,
        ("ColumnF64", "rolling_max_min_periods")  => NativeFn::M47TabColF64RollingMaxMinPeriods  as u32,
        ("ColumnF64", "rolling_std_min_periods")  => NativeFn::M47TabColF64RollingStdMinPeriods  as u32,
        ("ColumnCategorical", "codes")          => NativeFn::M47TabColCategoricalCodes      as u32,
        ("ColumnCategorical", "categories")     => NativeFn::M47TabColCategoricalCategories as u32,
        ("ColumnCategorical", "to_strings")     => NativeFn::M47TabColCategoricalToStrings  as u32,
        ("DataFrame", "get_column_categorical") => NativeFn::M47TabDfGetColumnCategorical   as u32,
        // ── M49: ColumnCategorical.is_ordered() ──
        ("ColumnCategorical", "is_ordered")     => NativeFn::M49TabColCategoricalIsOrdered  as u32,
        // ── M49: loc_range_multi_* on MultiIndex ──
        ("DataFrame", "loc_range_multi_i64")      => NativeFn::M49TabDfLocRangeMultiI64       as u32,
        ("DataFrame", "loc_range_multi_str")      => NativeFn::M49TabDfLocRangeMultiStr       as u32,
        ("DataFrame", "loc_range_multi_datetime") => NativeFn::M49TabDfLocRangeMultiDateTime  as u32,
        _ => return None,
    })
}

/// M51: dispatch the chainable RollingWindow surface.  Two halves:
///   - `df.rolling*` constructors on DataFrame → return RollingWindow.
///   - `.sum/.mean/.min/.max/.std/.count/.window/.min_periods/
///      .is_centered` aggregators on RollingWindow → return DataFrame
///      (or i64 / bool for the introspection getters).
/// Same shape as `m41_tabular_class_method_native_id_by_name`.
fn m51_tabular_class_method_native_id_by_name(
    class_name: &str,
    method: &str,
) -> Option<u32> {
    Some(match (class_name, method) {
        // ── DataFrame.rolling* constructors ──
        ("DataFrame", "rolling")
            => NativeFn::M51TabDfRolling                   as u32,
        ("DataFrame", "rolling_centered")
            => NativeFn::M51TabDfRollingCentered           as u32,
        ("DataFrame", "rolling_min_periods")
            => NativeFn::M51TabDfRollingMinPeriods         as u32,
        ("DataFrame", "rolling_centered_min_periods")
            => NativeFn::M51TabDfRollingCenteredMinPeriods as u32,
        // ── M51 Phase D: loc_range_level_* (chosen MultiIndex level) ──
        ("DataFrame", "loc_range_level_i64")      => NativeFn::M51TabDfLocRangeLevelI64      as u32,
        ("DataFrame", "loc_range_level_str")      => NativeFn::M51TabDfLocRangeLevelStr      as u32,
        ("DataFrame", "loc_range_level_datetime") => NativeFn::M51TabDfLocRangeLevelDateTime as u32,
        // ── RollingWindow aggregators / introspection ──
        ("RollingWindow", "sum")         => NativeFn::M51TabRwSum         as u32,
        ("RollingWindow", "mean")        => NativeFn::M51TabRwMean        as u32,
        ("RollingWindow", "min")         => NativeFn::M51TabRwMin         as u32,
        ("RollingWindow", "max")         => NativeFn::M51TabRwMax         as u32,
        ("RollingWindow", "std")         => NativeFn::M51TabRwStd         as u32,
        ("RollingWindow", "count")       => NativeFn::M51TabRwCount       as u32,
        ("RollingWindow", "window")      => NativeFn::M51TabRwWindow      as u32,
        ("RollingWindow", "min_periods") => NativeFn::M51TabRwMinPeriods  as u32,
        ("RollingWindow", "is_centered") => NativeFn::M51TabRwIsCentered  as u32,
        _ => return None,
    })
}

/// M35 P4-A: dispatch a `re.Pattern` method by class name + method
/// name.  Pattern is `is_native: true` (slot-backed) and its method
/// names collide with str-methods registered in `NativeFn::from_name`
/// (notably `split`), so we resolve by exact (class, method) pairing
/// before reaching the name-only fallback.  Returns `None` for any
/// other class so the caller continues through the regular dispatch.
fn m35_re_pattern_method_native_id_by_name(
    class_name: &str,
    method: &str,
) -> Option<u32> {
    Some(match (class_name, method) {
        ("Pattern", "matches")     => NativeFn::PatternMatches    as u32,
        ("Pattern", "find")        => NativeFn::PatternFind       as u32,
        ("Pattern", "find_all")    => NativeFn::PatternFindAll    as u32,
        ("Pattern", "replace")     => NativeFn::PatternReplace    as u32,
        ("Pattern", "replace_all") => NativeFn::PatternReplaceAll as u32,
        ("Pattern", "split")       => NativeFn::PatternSplit      as u32,
        ("Pattern", "source")      => NativeFn::PatternSource     as u32,
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

    /// Regression: a top-level `final` whose initialiser references another
    /// `final` (`SOLAR_MASS = 4.0 * PI * PI`) used to fall out of
    /// `module_consts` — only bare literals were folded — so every reference
    /// site silently lowered to `Const(None)`, i.e. 0.0. Discovered via an
    /// n-body benchmark where all planet masses became 0.
    #[test]
    fn module_const_referencing_const_folds_to_value() {
        let src = "\
final PI: f64 = 3.141592653589793
final SOLAR_MASS: f64 = 4.0 * PI * PI

fn main() -> i32:
    x: f64 = SOLAR_MASS
    return 0
";
        let ir = lower_src(src);
        let main = ir.functions.iter().find(|f| f.name == "main").unwrap();
        let expected = 4.0 * 3.141592653589793_f64 * 3.141592653589793_f64;
        let folded = main.blocks.iter().flat_map(|b| &b.values).any(|v| {
            matches!(
                &v.kind,
                ValueKind::Const(IRConst::F64(x)) if (x - expected).abs() < 1e-9
            )
        });
        assert!(
            folded,
            "SOLAR_MASS reference did not fold to {expected}:\n{}",
            dump_function(main)
        );
    }

    /// Const-to-const folding must not depend on declaration order — the
    /// module merger appends imported modules' decls after the root's, so
    /// a use can precede its definition in the merged decl list.
    #[test]
    fn module_const_forward_reference_folds() {
        let src = "\
final AREA: i64 = WIDTH * HEIGHT
final WIDTH: i64 = 60
final HEIGHT: i64 = 30

fn main() -> i32:
    n: i64 = AREA
    return 0
";
        let ir = lower_src(src);
        let main = ir.functions.iter().find(|f| f.name == "main").unwrap();
        let folded = main.blocks.iter().flat_map(|b| &b.values).any(|v| {
            matches!(&v.kind, ValueKind::Const(IRConst::I64(1800)))
        });
        assert!(
            folded,
            "AREA reference did not fold to 1800:\n{}",
            dump_function(main)
        );
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

    // ── WAVE-2 LANE-0: class_dunder_dispatch scaffold ────────────────────

    /// Build the two maps `class_dunder_dispatch` needs from source: the
    /// resolver's `class_layouts` and a `Class.method -> FuncId` map
    /// reconstructed from the lowered IR (function names are `"Class.method"`,
    /// matching the `fn_id_by_name` keying used during lowering).
    fn dunder_dispatch_fixture(
        src: &str,
    ) -> (
        HashMap<ClassId, ClassLayout>,
        HashMap<String, FuncId>,
        HashMap<String, ClassId>,
    ) {
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
        let class_layouts = resolved.class_layouts.clone();
        let name_to_cid: HashMap<String, ClassId> = class_layouts
            .iter()
            .map(|(cid, l)| (l.name.clone(), *cid))
            .collect();
        let typed = typecheck::TypeChecker::new().check(resolved).unwrap();
        let ir = lower(typed);
        let fn_id_by_name: HashMap<String, FuncId> = ir
            .functions
            .iter()
            .map(|f| (f.name.clone(), f.id))
            .collect();
        (class_layouts, fn_id_by_name, name_to_cid)
    }

    #[test]
    fn class_dunder_dispatch_final_class_is_direct() {
        let src = "\
final class Point:
    x: i64
    fn __init__(self, x: i64) -> None:
        self.x = x
    fn __str__(self) -> str:
        return \"pt\"

fn main() -> i32:
    return 0
";
        let (layouts, fns, cids) = dunder_dispatch_fixture(src);
        let cid = cids["Point"];
        let target = class_dunder_dispatch(&layouts, &fns, cid, "__str__")
            .expect("Point defines __str__");
        match target {
            IROp::DirectCall { fn_id } => {
                assert_eq!(fn_id, fns["Point.__str__"], "must direct-call Point.__str__");
            }
            other => panic!("final class must devirtualise to DirectCall, got {other:?}"),
        }
        // A class without the dunder yields None.
        assert!(
            class_dunder_dispatch(&layouts, &fns, cid, "__add__").is_none(),
            "Point does not define __add__"
        );
    }

    #[test]
    fn class_dunder_dispatch_open_class_is_virtual() {
        let src = "\
open class Animal:
    open fn __str__(self) -> str:
        return \"animal\"

fn main() -> i32:
    return 0
";
        let (layouts, fns, cids) = dunder_dispatch_fixture(src);
        let cid = cids["Animal"];
        let target = class_dunder_dispatch(&layouts, &fns, cid, "__str__")
            .expect("Animal defines __str__");
        // Open class: must dispatch virtually so subclass overrides win.
        // __str__ is the only virtual method, so slot 0.
        assert!(
            matches!(target, IROp::VirtualCall { vtable_slot: 0 }),
            "open class must dispatch __str__ via VirtualCall slot 0, got {target:?}"
        );
    }

    #[test]
    fn class_dunder_dispatch_inherited_resolves() {
        // Subclass `Dog` inherits `__str__` from open base `Animal` without
        // overriding it. `class_dunder_dispatch` must still resolve the dunder
        // (it lives in `Dog`'s flattened `methods`) and — because `Dog` is
        // final but does not define `Dog.__str__` itself — fall through to a
        // VirtualCall, exactly as an inherited normal method would.
        let src = "\
open class Animal:
    open fn __str__(self) -> str:
        return \"animal\"

final class Dog(Animal):
    fn bark(self) -> str:
        return \"woof\"

fn main() -> i32:
    return 0
";
        let (layouts, fns, cids) = dunder_dispatch_fixture(src);
        let dog = cids["Dog"];
        let target = class_dunder_dispatch(&layouts, &fns, dog, "__str__")
            .expect("Dog inherits __str__ from Animal");
        match target {
            IROp::VirtualCall { vtable_slot } => {
                // Slot must match __str__'s index in Dog's virtual method list.
                let expected = layouts[&dog]
                    .methods
                    .iter()
                    .filter(|m| m.name != "__init__")
                    .position(|m| m.name == "__str__")
                    .unwrap() as u32;
                assert_eq!(vtable_slot, expected, "inherited dunder slot mismatch");
            }
            other => panic!("inherited (non-overridden) dunder must be VirtualCall, got {other:?}"),
        }
        // The override case: a final subclass that *does* define the dunder
        // directly devirtualises to a DirectCall on its own impl.
        let src2 = "\
open class Animal:
    open fn __str__(self) -> str:
        return \"animal\"

final class Cat(Animal):
    fn __str__(self) -> str:
        return \"meow\"

fn main() -> i32:
    return 0
";
        let (layouts2, fns2, cids2) = dunder_dispatch_fixture(src2);
        let cat = cids2["Cat"];
        let target2 = class_dunder_dispatch(&layouts2, &fns2, cat, "__str__")
            .expect("Cat overrides __str__");
        assert!(
            matches!(target2, IROp::DirectCall { fn_id } if fn_id == fns2["Cat.__str__"]),
            "final subclass overriding a dunder must DirectCall its own impl, got {target2:?}"
        );
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

    /// Lane A: the IR-side widening table mirrors the typechecker's.
    #[test]
    fn numeric_common_ty_ir_table() {
        use PrimTy::*;
        assert_eq!(numeric_common_ty_ir(I32, I64), Some(I64));
        assert_eq!(numeric_common_ty_ir(I64, I32), Some(I64));
        assert_eq!(numeric_common_ty_ir(I32, F64), Some(F64));
        assert_eq!(numeric_common_ty_ir(I64, F64), Some(F64));
        assert_eq!(numeric_common_ty_ir(F32, I64), Some(F64));
        assert_eq!(numeric_common_ty_ir(F32, F64), Some(F64));
        assert_eq!(numeric_common_ty_ir(U32, I32), None);
    }

    /// Lane A: `7 / 2` lowers to a float divide (FDiv), not IDiv — true
    /// division coerces the integer operands to f64.
    #[test]
    fn true_division_lowers_to_fdiv() {
        let src = "\
fn main() -> i32:
    a: i64 = 7
    b: i64 = 2
    q: f64 = a / b
    return 0
";
        let m = lower_src(src);
        let main = m.functions.iter().find(|f| f.name == "main").unwrap();
        let mut saw_fdiv = false;
        let mut saw_idiv = false;
        for b in &main.blocks {
            for v in &b.values {
                if let ValueKind::Op { op, .. } = &v.kind {
                    match op {
                        IROp::FDiv => saw_fdiv = true,
                        IROp::IDiv => saw_idiv = true,
                        _ => {}
                    }
                }
            }
        }
        assert!(saw_fdiv, "`/` on integers must lower to FDiv");
        assert!(!saw_idiv, "`/` must not lower to integer IDiv");
    }

    /// Lane A: `7 // 2` keeps integer (truncating) division — IDiv, not FDiv.
    #[test]
    fn floor_division_lowers_to_idiv() {
        let src = "\
fn main() -> i32:
    a: i64 = 7
    b: i64 = 2
    q: i64 = a // b
    return 0
";
        let m = lower_src(src);
        let main = m.functions.iter().find(|f| f.name == "main").unwrap();
        let saw_idiv = main.blocks.iter().any(|b| b.values.iter().any(|v|
            matches!(&v.kind, ValueKind::Op { op: IROp::IDiv, .. })));
        assert!(saw_idiv, "`//` on integers must lower to IDiv");
    }

    // ── Wave-1 Lane D: try/except/else + except-tuple + raise-from ──────

    fn named_ty(name: &str) -> ast::Type {
        ast::Type::Named { name: name.into(), args: vec![], span: crate::ast::Span::DUMMY }
    }

    #[test]
    fn exception_filter_names_single() {
        assert_eq!(exception_filter_names(&named_ty("ValueError")), vec!["ValueError"]);
    }

    #[test]
    fn exception_filter_names_tuple_expands() {
        // `except (A, B)` must yield both names — historically it degraded to
        // the universal `"Exception"` catch-all.
        let tup = ast::Type::Tuple {
            elems: vec![named_ty("ValueError"), named_ty("KeyError")],
            span: crate::ast::Span::DUMMY,
        };
        assert_eq!(exception_filter_names(&tup), vec!["ValueError", "KeyError"]);
    }

    /// `try/except/else` must lower the `else` block (it used to be dropped):
    /// the const string from the else body must appear in the lowered IR.
    #[test]
    fn try_else_block_is_lowered() {
        let src = "\
fn main() -> i32:
    try:
        x: i32 = 1
    except ValueError as e:
        print(\"caught\")
    else:
        print(\"else-ran-marker\")
    return 0
";
        let ir = lower_src(src);
        let saw_marker = ir.functions.iter().any(|f| {
            f.blocks.iter().any(|b| {
                b.values.iter().any(|v| {
                    matches!(&v.kind, ValueKind::Const(IRConst::Str(s)) if s == "else-ran-marker")
                })
            })
        });
        assert!(saw_marker, "the try/else block body was dropped at lowering");
    }

    /// `except (A, B)` must produce a VM handler arm for each listed type, so
    /// it does not silently degrade to a catch-all. We assert the lowered
    /// TryEnter carries arms filtering on both names.
    #[test]
    fn except_tuple_lowers_one_arm_per_type() {
        let src = "\
fn main() -> i32:
    try:
        raise ValueError(\"x\")
    except (ValueError, KeyError) as e:
        print(e.message)
    return 0
";
        let ir = lower_src(src);
        let mut filters: Vec<String> = Vec::new();
        for f in &ir.functions {
            for b in &f.blocks {
                for v in &b.values {
                    if let ValueKind::Op { op: IROp::TryEnter { arms, .. }, .. } = &v.kind {
                        for a in arms {
                            filters.push(ir.string_table[a.filter_str_idx as usize].clone());
                        }
                    }
                }
            }
        }
        assert!(filters.iter().any(|s| s == "ValueError"),
            "tuple-except must filter on ValueError; got {filters:?}");
        assert!(filters.iter().any(|s| s == "KeyError"),
            "tuple-except must filter on KeyError; got {filters:?}");
        // It must NOT have degraded to a bare `Exception` catch-all.
        assert!(!filters.iter().any(|s| s == "Exception"),
            "tuple-except must not degrade to a catch-all; got {filters:?}");
    }

    // ── WAVE-2 LANE-A: str()/print() on a user class ─────────────────────

    /// Collect every `Const(Str(_))` literal emitted in `fn_name`'s body.
    fn str_consts_of(ir: &IRModule, fn_name: &str) -> Vec<String> {
        let f = ir.functions.iter().find(|f| f.name == fn_name).unwrap();
        f.blocks
            .iter()
            .flat_map(|b| &b.values)
            .filter_map(|v| match &v.kind {
                ValueKind::Const(IRConst::Str(s)) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    /// `main` must NOT lower any `str(obj)`/`print(obj)` on a class through
    /// `StrFromAny` (native id) — that's the garbage path we replaced.
    fn main_has_str_from_any(ir: &IRModule) -> bool {
        let f = ir.functions.iter().find(|f| f.name == "main").unwrap();
        f.blocks.iter().flat_map(|b| &b.values).any(|v| matches!(
            &v.kind,
            ValueKind::Op { op: IROp::NativeCall { native_id }, .. }
                if *native_id == NativeFn::StrFromAny as u32
        ))
    }

    /// `str(p)` on a final class with `__str__` devirtualises to a
    /// DirectCall on `Point.__str__`, never StrFromAny.
    #[test]
    fn str_of_class_with_dunder_str_direct_calls_it() {
        let src = "\
final class Point:
    x: i64
    fn __init__(self, x: i64) -> None:
        self.x = x
    fn __str__(self) -> str:
        return \"pt\"

fn main() -> i32:
    p: Point = Point(1)
    s: str = str(p)
    println(p)
    return 0
";
        let ir = lower_src(src);
        let str_fid = ir.functions.iter().find(|f| f.name == "Point.__str__").unwrap().id;
        let main = ir.functions.iter().find(|f| f.name == "main").unwrap();
        let direct_calls: Vec<_> = main
            .blocks
            .iter()
            .flat_map(|b| &b.values)
            .filter_map(|v| match &v.kind {
                ValueKind::Op { op: IROp::DirectCall { fn_id }, .. } => Some(*fn_id),
                _ => None,
            })
            .collect();
        // Both `str(p)` and `println(p)` must route through Point.__str__.
        let n = direct_calls.iter().filter(|f| **f == str_fid).count();
        assert!(n >= 2, "str(p) AND println(p) must DirectCall Point.__str__ (saw {n})");
        assert!(!main_has_str_from_any(&ir), "must not fall through to StrFromAny");
    }

    /// `__repr__` is the fallback when `__str__` is absent.
    #[test]
    fn str_of_class_falls_back_to_dunder_repr() {
        let src = "\
final class Money:
    cents: i64
    fn __init__(self, cents: i64) -> None:
        self.cents = cents
    fn __repr__(self) -> str:
        return \"m\"

fn main() -> i32:
    m: Money = Money(1)
    s: str = str(m)
    return 0
";
        let ir = lower_src(src);
        let repr_fid = ir.functions.iter().find(|f| f.name == "Money.__repr__").unwrap().id;
        let main = ir.functions.iter().find(|f| f.name == "main").unwrap();
        let calls_repr = main.blocks.iter().flat_map(|b| &b.values).any(|v| matches!(
            &v.kind,
            ValueKind::Op { op: IROp::DirectCall { fn_id }, .. } if *fn_id == repr_fid
        ));
        assert!(calls_repr, "str(m) must DirectCall Money.__repr__");
        assert!(!main_has_str_from_any(&ir), "must not fall through to StrFromAny");
    }

    /// A class with neither dunder gets a default `ClassName(field=value, …)`
    /// repr: the literal pieces `Color(`, `r=`, `, g=`, `, b=`, `)` must all
    /// be emitted, and no StrFromAny.
    #[test]
    fn str_of_class_without_dunder_builds_default_repr() {
        let src = "\
final class Color:
    r: i64
    g: i64
    b: i64
    fn __init__(self, r: i64, g: i64, b: i64) -> None:
        self.r = r
        self.g = g
        self.b = b

fn main() -> i32:
    c: Color = Color(1, 2, 3)
    s: str = str(c)
    return 0
";
        let ir = lower_src(src);
        let consts = str_consts_of(&ir, "main");
        for want in ["Color(", "r=", ", g=", ", b=", ")"] {
            assert!(
                consts.iter().any(|s| s == want),
                "default repr missing literal {want:?}; got {consts:?}"
            );
        }
        assert!(!main_has_str_from_any(&ir), "default repr must not use StrFromAny");
    }

    /// `print(obj)` on a class with no dunder also gets the default repr —
    /// the print path pre-stringifies the class arg rather than handing the
    /// raw pointer to the native print (which would read it as a string).
    #[test]
    fn print_of_class_without_dunder_builds_default_repr() {
        let src = "\
final class Tag:
    name: str
    fn __init__(self, name: str) -> None:
        self.name = name

fn main() -> i32:
    t: Tag = Tag(\"x\")
    println(t)
    return 0
";
        let ir = lower_src(src);
        let consts = str_consts_of(&ir, "main");
        assert!(consts.iter().any(|s| s == "Tag("), "print default repr missing 'Tag('; got {consts:?}");
        assert!(consts.iter().any(|s| s == "name="), "print default repr missing 'name='; got {consts:?}");
        assert!(!main_has_str_from_any(&ir), "print default repr must not use StrFromAny");
    }
}
