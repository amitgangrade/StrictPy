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
    self, BinOp as AstBinOp, Block, Expr, FuncDecl, Literal, Lvalue,
    Span, Stmt, TopDecl, UnaryOp,
};
use crate::resolver::{SymbolId, SymbolKind};
use crate::typecheck::TypedModule;
use crate::types::{ClassId, ClassLayout, PrimTy, Ty, TypeCtor};

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
    /// Top-level function name → assigned fn id.
    fn_id_by_name: HashMap<String, FuncId>,
    /// Class id → assigned type-table type_id.
    class_type_id: HashMap<u32, u32>,
    /// Top-level `final` const declarations folded to their literal value.
    /// Used at every reference site (see `Expr::Ident` lowering).
    module_consts: HashMap<String, (IRConst, Ty)>,
    /// Next free fn id.
    next_fn_id: u32,
    /// Next free type id (after primitives 0..15).
    next_type_id: u32,
}

impl Lowerer {
    fn new(typed: TypedModule) -> Self {
        Self {
            typed,
            out: IRModule::default(),
            str_intern: HashMap::new(),
            fn_id_by_name: HashMap::new(),
            class_type_id: HashMap::new(),
            module_consts: HashMap::new(),
            next_fn_id: 0,
            next_type_id: 16,
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
                    let fid = self.fresh_fn_id();
                    self.fn_id_by_name.insert(f.name.clone(), fid);
                }
                TopDecl::Class(c) => {
                    // Pre-assign type id for the class so cross-references work.
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
            let mut vtable = Vec::new();
            for m in &layout.methods {
                if m.name == "__init__" {
                    continue;
                }
                let key = format!("{}.{}", layout.name, m.name);
                if let Some(FuncId(fid)) = self.fn_id_by_name.get(&key) {
                    vtable.push(*fid);
                } else {
                    vtable.push(u32::MAX); // unresolved (e.g. inherited prelude method)
                }
            }
            let base_type = layout
                .base
                .and_then(|b| self.class_type_id.get(&b.0).copied())
                .unwrap_or(strictpy_shared::file_format::NO_BASE_TYPE);
            // Conservative size: 16 (object header) + 8 per field.
            let size = 16 + (layout.fields.len() as u32) * 8;
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

        // Pass 3: lower function bodies.
        for d in &decls {
            match d {
                TopDecl::Func(f) => {
                    let fid = *self.fn_id_by_name.get(&f.name).unwrap();
                    let irfn = self.lower_func(fid, f, None);
                    self.register_fn_table(&irfn);
                    self.out.functions.push(irfn);
                }
                TopDecl::Class(c) => {
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
        let mut ctx = LowerCtx {
            typed: &self.typed,
            str_intern: &mut self.str_intern,
            string_table: &mut self.out.string_table,
            fn_id_by_name: &self.fn_id_by_name,
            class_layouts: &self.typed.resolved.class_layouts,
            class_type_id: &self.class_type_id,
            module_consts: &self.module_consts,
            next_fn_id: &mut self.next_fn_id,
            lifted_functions: &mut lifted,
        };
        let _ = lower_block(&mut fb, &mut ctx, &f.body);

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
    /// Top-level `final` consts folded to their literal IR value.
    module_consts: &'a HashMap<String, (IRConst, Ty)>,
    /// Mutable handle to the lowerer's fn-id allocator so lifted lambdas
    /// can claim a fresh id.
    next_fn_id: &'a mut u32,
    /// Lambdas lowered as the body of the current function go here; the
    /// outer lowerer flushes them into the module's function list once
    /// the parent function finishes.
    lifted_functions: &'a mut Vec<IRFunction>,
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
        self.typed
            .expr_types
            .get(&(span.start, span.end))
            .cloned()
            .unwrap_or(Ty::Primitive(PrimTy::Unit))
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
        Stmt::For { var, iter, body, .. } => {
            // Best-effort: lower iter, then run body once with `var` bound
            // to a const-none placeholder. This is enough to keep IR
            // structurally valid; full __iter__/__next__ lowering is M3+.
            let _ = lower_expr(fb, ctx, iter);
            let placeholder_ty = Ty::Primitive(PrimTy::Unit);
            let v = fb.push_value(placeholder_ty.clone(), ValueKind::Const(IRConst::None));
            let slot = fb.alloc_slot(var, placeholder_ty);
            fb.emit_write_local(slot, v);
            lower_block(fb, ctx, body);
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
            // Best-effort: lower body in current block, then each handler
            // body, then finally. Real exception edges are deferred.
            lower_block(fb, ctx, body);
            for h in handlers {
                lower_block(fb, ctx, &h.body);
            }
            if let Some(fin) = finally_block {
                lower_block(fb, ctx, fin);
            }
            Some(())
        }
        Stmt::Raise { exc, .. } => {
            let v = lower_expr(fb, ctx, exc);
            fb.terminate(Terminator::Throw { exc: v });
            let nb = fb.new_block();
            fb.switch_to(nb);
            Some(())
        }
        Stmt::Assert { cond, .. } => {
            let _ = lower_expr(fb, ctx, cond);
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
        Stmt::Match { .. } => Some(()), // M4
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
    if let Ty::Class(cid) = obj_ty {
        if let Some(layout) = class_layouts.get(cid) {
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
            // Unknown ident (likely a prelude/builtin/class) — placeholder.
            let ty = ctx.expr_ty(*span);
            fb.push_value(ty, ValueKind::Const(IRConst::None))
        }
        Expr::Tuple { elems, span } => {
            for elt in elems {
                let _ = lower_expr(fb, ctx, elt);
            }
            let ty = ctx.expr_ty(*span);
            fb.push_value(ty, ValueKind::Const(IRConst::None))
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
            let irop = match op {
                UnaryOp::Neg => match ty {
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
            // Short-circuit `and`/`or` left for codegen as plain ops in M3.
            let l = lower_expr(fb, ctx, lhs);
            let r = lower_expr(fb, ctx, rhs);
            let ty = ctx.expr_ty(*span);
            emit_binop(fb, *op, l, r, ty)
        }
        Expr::Call { callee, args, span } => lower_call(fb, ctx, callee, args, *span),
        Expr::MethodCall { receiver, method, args, span } => {
            lower_method_call(fb, ctx, receiver, method, args, *span)
        }
        Expr::Attr { obj, name, span } => {
            let recv = lower_expr(fb, ctx, obj);
            let obj_ty = ctx.expr_ty(expr_span(obj));
            let offset = field_offset(ctx.class_layouts, &obj_ty, name).unwrap_or(0);
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
            let _ = lower_expr(fb, ctx, lhs);
            let r = lower_expr(fb, ctx, rhs);
            let ty = ctx.expr_ty(*span);
            fb.push_value(ty, ValueKind::Op { op: IROp::Copy, args: vec![r] })
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
    let operand_ty = find_value_ty(fb, l).unwrap_or(ty.clone());
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
        AstBinOp::Ne => if is_float { IROp::FNe } else { IROp::INe },
        AstBinOp::Lt => if is_float { IROp::FLt } else { IROp::ILt },
        AstBinOp::Le => if is_float { IROp::FLe } else { IROp::ILe },
        AstBinOp::Gt => if is_float { IROp::FGt } else { IROp::IGt },
        AstBinOp::Ge => if is_float { IROp::FGe } else { IROp::IGe },
        AstBinOp::Is => IROp::RefEq,
        AstBinOp::IsNot => IROp::RefEq, // codegen could invert
        AstBinOp::In => IROp::IEq,      // placeholder
        AstBinOp::NotIn => IROp::INe,   // placeholder
        AstBinOp::And => IROp::IAnd,    // bitwise approximation
        AstBinOp::Or => IROp::IOr,
    };
    fb.push_value(ty, ValueKind::Op { op: irop, args: vec![l, r] })
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
                        }
                        // Allocate + call __init__ (if present).
                        let alloc = fb.push_value(
                            Ty::Class(cid),
                            ValueKind::Op { op: IROp::Alloc { class_id: cid.0 }, args: vec![] },
                        );
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
                    let nid = if name == "str" {
                        let arg_ty = args
                            .first()
                            .map(|a| ctx.expr_ty(expr_span(&a.value)));
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
                            Some(Ty::Primitive(PrimTy::Str)) => {
                                // No conversion needed — emit a copy.
                                return fb.push_value(
                                    ret_ty,
                                    ValueKind::Op { op: IROp::Copy, args: arg_vs },
                                );
                            }
                            _ => NativeFn::StrFromAny as u32,
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
    let recv = lower_expr(fb, ctx, receiver);
    let recv_ty = ctx.expr_ty(expr_span(receiver));
    let ret_ty = ctx.expr_ty(span);
    let mut arg_vs = vec![recv];
    for a in args {
        arg_vs.push(lower_expr(fb, ctx, &a.value));
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
                // static type is `final` — for `open` classes (and any
                // class with subclasses in scope) the actual runtime type
                // may be a subclass that overrides this method, and we
                // must dispatch through the vtable to see the override.
                let key = format!("{}.{}", layout.name, method);
                if !layout.is_open {
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
}
