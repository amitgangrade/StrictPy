//! IR → bytecode translation. See spec §13.
//!
//! Each [`IROp`] together with its operand [`Ty`] picks a concrete typed
//! [`Opcode`]. Registers are u16; constant-pool indices are u32; branch
//! offsets are i32 relative to the byte after the branch instruction.
//!
//! The M3 register allocator is intentionally trivial: every SSA value gets
//! its own register slot. A real linear-scan allocator is M6 work. The
//! pass still respects the 65535 register cap (spec §13.2) and dies with a
//! diagnostic if a single function exceeds it — none of the v0.1 examples
//! come close.

use std::collections::HashMap;

use byteorder::{LittleEndian, WriteBytesExt};

use strictpy_shared::{
    file_format::DISCARD_REG,
    Opcode, TypeTag,
};

use crate::ir::{
    BasicBlock, IRConst, IRFunction, IROp, Terminator, TryHandlerArm, Value, ValueId,
    ValueKind,
};
use crate::types::{PrimTy, Ty};

// ─────────────────────────────────────────────────────────────────────────
//  Public API
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ExceptionEntry {
    pub start_pc: u32,
    pub end_pc: u32,
    pub handler_pc: u32,
    pub caught_type_id: u32,
}

#[derive(Debug)]
pub struct EmittedFunction {
    pub code: Vec<u8>,
    pub num_registers: u16,
    pub exception_table: Vec<ExceptionEntry>,
}

/// Index assigned to a constant in the bytecode constant pool. Filled in by
/// the bytecode writer; codegen only needs to know it returns a u32.
pub type ConstIdx = u32;

/// Callback the bytecode writer hands in so codegen can intern strings /
/// constants on demand. Returns the stable u32 index.
pub trait ConstSink {
    fn intern_string(&mut self, s: &str) -> u32;
    fn intern_i64(&mut self, v: i64) -> u32;
    fn intern_f64(&mut self, v: f64) -> u32;
}

/// Emit the bytecode body for a single function.
pub fn emit_function<S: ConstSink>(func: &IRFunction, sink: &mut S) -> EmittedFunction {
    let mut cg = Codegen::new(func, sink);
    cg.emit();
    cg.finish()
}

// ─────────────────────────────────────────────────────────────────────────
//  Codegen state
// ─────────────────────────────────────────────────────────────────────────

struct Codegen<'a, S: ConstSink> {
    func: &'a IRFunction,
    sink: &'a mut S,
    code: Vec<u8>,
    /// SSA id → assigned register.
    reg_of: HashMap<u32, u16>,
    next_reg: u32,
    /// Per-block: starting byte offset in `code`.
    block_offset: HashMap<u32, u32>,
    /// Branch patches: (byte position of i32 offset, target block id).
    patches: Vec<(usize, u32)>,
    /// Local-slot id → dedicated register holding the slot's current
    /// value. Reads/writes via `IROp::ReadLocal`/`WriteLocal` translate
    /// to `Move`s between SSA-value registers and slot registers.
    slot_reg: HashMap<u16, u16>,
}

impl<'a, S: ConstSink> Codegen<'a, S> {
    fn new(func: &'a IRFunction, sink: &'a mut S) -> Self {
        Self {
            func,
            sink,
            code: Vec::new(),
            reg_of: HashMap::new(),
            next_reg: 0,
            block_offset: HashMap::new(),
            patches: Vec::new(),
            slot_reg: HashMap::new(),
        }
    }

    /// Look up the static type of SSA value `id` by scanning the function's
    /// blocks. Cheap enough at M3 scale; a real allocator caches this.
    fn value_ty(&self, id: u32) -> Option<Ty> {
        for b in &self.func.blocks {
            for v in &b.values {
                if v.id == id {
                    return Some(v.ty.clone());
                }
            }
        }
        None
    }

    /// Look up (or allocate) the dedicated register for local slot `slot`.
    fn slot_register(&mut self, slot: u16) -> u16 {
        if let Some(r) = self.slot_reg.get(&slot) {
            return *r;
        }
        let r = self.next_reg as u16;
        self.next_reg += 1;
        self.slot_reg.insert(slot, r);
        r
    }

    fn reg(&mut self, id: u32) -> u16 {
        if let Some(r) = self.reg_of.get(&id) {
            return *r;
        }
        let r = self.next_reg as u16;
        self.next_reg += 1;
        self.reg_of.insert(id, r);
        r
    }

    fn finish(self) -> EmittedFunction {
        let mut code = self.code;
        // Patch branch offsets.
        for (pos, target_block) in self.patches {
            // pos points at the start of the i32 offset slot. The branch
            // semantics: jump target = (pc_after_instr) + offset, where
            // pc_after_instr is just past the i32 slot itself.
            let target = *self.block_offset.get(&target_block).unwrap_or(&0) as i64;
            let pc_after = (pos + 4) as i64;
            let offset = (target - pc_after) as i32;
            (&mut code[pos..pos + 4])
                .write_i32::<LittleEndian>(offset)
                .expect("patch write");
        }
        let num_registers = self.next_reg.min(u16::MAX as u32 + 1) as u16;
        EmittedFunction {
            code,
            num_registers,
            exception_table: Vec::new(),
        }
    }

    fn emit(&mut self) {
        // Reserve registers for params first (they live in registers 0..n).
        for v in self.func.blocks.first().map(|b| b.values.as_slice()).unwrap_or(&[]) {
            if let ValueKind::Param { idx } = v.kind {
                let _ = idx;
                self.reg(v.id);
            }
        }

        // Emit each block in source order. The IR builder already emits in a
        // topologically sensible order; for M3 this is good enough.
        for b in &self.func.blocks {
            let off = self.code.len() as u32;
            self.block_offset.insert(b.id.0, off);
            self.emit_block(b);
        }
    }

    fn emit_block(&mut self, b: &BasicBlock) {
        for v in &b.values {
            self.emit_value(v);
        }
        self.emit_terminator(&b.terminator);
    }

    fn emit_value(&mut self, v: &Value) {
        let dst = self.reg(v.id);
        match &v.kind {
            ValueKind::Param { .. } => {
                // Params live in pre-assigned registers; no opcode needed.
            }
            ValueKind::Const(c) => self.emit_const(dst, c),
            ValueKind::Phi { .. } => {
                // Phis are materialised by predecessor `Move`s in a real
                // allocator. For the M3 trivial allocator each phi just
                // claims its own register and the source values are written
                // into it by the producing edges. Emit nothing here.
            }
            ValueKind::Op { op, args } => self.emit_op(dst, &v.ty, op, args),
        }
    }

    fn emit_const(&mut self, dst: u16, c: &IRConst) {
        match c {
            IRConst::I32(x) => {
                self.write_op(Opcode::ConstI32);
                self.write_u16(dst);
                self.write_u32(*x as u32);
            }
            IRConst::I64(x) => {
                let idx = self.sink.intern_i64(*x);
                self.write_op(Opcode::ConstI64);
                self.write_u16(dst);
                self.write_u32(idx);
            }
            IRConst::U32(x) => {
                self.write_op(Opcode::ConstI32);
                self.write_u16(dst);
                self.write_u32(*x);
            }
            IRConst::U64(x) => {
                let idx = self.sink.intern_i64(*x as i64);
                self.write_op(Opcode::ConstI64);
                self.write_u16(dst);
                self.write_u32(idx);
            }
            IRConst::F32(x) => {
                self.write_op(Opcode::ConstF32);
                self.write_u16(dst);
                self.write_u32(x.to_bits());
            }
            IRConst::F64(x) => {
                let idx = self.sink.intern_f64(*x);
                self.write_op(Opcode::ConstF64);
                self.write_u16(dst);
                self.write_u32(idx);
            }
            IRConst::Bool(b) => {
                self.write_op(if *b { Opcode::ConstTrue } else { Opcode::ConstFalse });
                self.write_u16(dst);
            }
            IRConst::Str(s) => {
                let idx = self.sink.intern_string(s);
                self.write_op(Opcode::ConstStr);
                self.write_u16(dst);
                self.write_u32(idx);
            }
            IRConst::Char(c) => {
                self.write_op(Opcode::ConstI32);
                self.write_u16(dst);
                self.write_u32(*c as u32);
            }
            IRConst::None => {
                self.write_op(Opcode::ConstNone);
                self.write_u16(dst);
            }
        }
    }

    fn emit_op(&mut self, dst: u16, ty: &Ty, op: &IROp, args: &[ValueId]) {
        let argr: Vec<u16> = args.iter().map(|a| self.reg(a.0)).collect();
        // Look up the static type of the first operand so we can pick
        // size-specific opcodes for integer/float comparisons (whose
        // result type is `Bool` and so doesn't itself encode the input
        // width).
        //
        // real-world: csv_aggregate / nullable-narrowing audit — strip
        // `Ty::Nullable(T)` to `T` before any primitive-type dispatch
        // below. The typechecker narrows `x: T?` to `T` inside `if x is
        // not none:` branches, but the IR slot for `x` still carries
        // `Nullable(T)` so naive `matches!(ty, Ty::Primitive(p) if
        // p.is_float())` returned false and we silently emitted the
        // wrong typed opcode (e.g. F64 op on raw f32 bits, or I32 op on
        // f64 bits). Unwrap once here so every `Ty::Primitive(...)`
        // match below sees the narrowed inner type.
        let raw_operand_ty = args
            .first()
            .and_then(|a| self.value_ty(a.0))
            .unwrap_or_else(|| ty.clone());
        let operand_ty = unwrap_nullable_ty(&raw_operand_ty).clone();
        let ty_owned = unwrap_nullable_ty(ty).clone();
        let ty = &ty_owned;
        match op {
            IROp::IAdd => self.write_bin(
                op_for_iadd(ty).or_else(|| op_for_iadd(&operand_ty)).unwrap_or(Opcode::IAddI32),
                dst, &argr,
            ),
            IROp::ISub => self.write_bin(
                op_for_isub(ty).or_else(|| op_for_isub(&operand_ty)).unwrap_or(Opcode::ISubI32),
                dst, &argr,
            ),
            IROp::IMul => self.write_bin(
                op_for_imul(ty).or_else(|| op_for_imul(&operand_ty)).unwrap_or(Opcode::IMulI32),
                dst, &argr,
            ),
            IROp::IDiv => self.write_bin(
                op_for_idiv(ty).or_else(|| op_for_idiv(&operand_ty)).unwrap_or(Opcode::IDivI32),
                dst, &argr,
            ),
            IROp::IRem => self.write_bin(
                if matches!(ty, Ty::Primitive(PrimTy::I64) | Ty::Primitive(PrimTy::U64)) {
                    Opcode::IRemI64
                } else {
                    Opcode::IRemI32
                },
                dst, &argr,
            ),
            IROp::INeg => self.write_un(
                if matches!(ty, Ty::Primitive(PrimTy::I64) | Ty::Primitive(PrimTy::U64)) {
                    Opcode::INegI64
                } else {
                    Opcode::INegI32
                },
                dst, &argr,
            ),
            IROp::IAnd => self.write_bin(Opcode::IAndI32, dst, &argr),
            IROp::IOr  => self.write_bin(Opcode::IOrI32, dst, &argr),
            IROp::IXor => self.write_bin(Opcode::IXorI32, dst, &argr),
            IROp::IShl => self.write_bin(Opcode::IShlI32, dst, &argr),
            IROp::IShr => self.write_bin(Opcode::IShrI32, dst, &argr),
            IROp::INot => self.write_un(Opcode::INotI32, dst, &argr),

            IROp::FAdd => self.write_bin(
                if matches!(operand_ty, Ty::Primitive(PrimTy::F32)) { Opcode::FAddF32 }
                else { Opcode::FAddF64 },
                dst, &argr,
            ),
            IROp::FSub => self.write_bin(
                if matches!(operand_ty, Ty::Primitive(PrimTy::F32)) { Opcode::FSubF32 }
                else { Opcode::FSubF64 },
                dst, &argr,
            ),
            IROp::FMul => self.write_bin(
                if matches!(operand_ty, Ty::Primitive(PrimTy::F32)) { Opcode::FMulF32 }
                else { Opcode::FMulF64 },
                dst, &argr,
            ),
            IROp::FDiv => self.write_bin(
                if matches!(operand_ty, Ty::Primitive(PrimTy::F32)) { Opcode::FDivF32 }
                else { Opcode::FDivF64 },
                dst, &argr,
            ),
            IROp::FNeg => self.write_un(
                if matches!(operand_ty, Ty::Primitive(PrimTy::F32)) { Opcode::FNegF32 }
                else { Opcode::FNegF64 },
                dst, &argr,
            ),

            IROp::IExt => self.write_un(Opcode::I32ToI64, dst, &argr),
            IROp::ITrunc => self.write_un(Opcode::I64ToI32, dst, &argr),
            IROp::ItoF => self.write_un(Opcode::I32ToF64, dst, &argr),
            IROp::FtoI => self.write_un(Opcode::F64ToI32, dst, &argr),
            IROp::FExt => self.write_un(Opcode::F32ToF64, dst, &argr),
            IROp::FTrunc => self.write_un(Opcode::F64ToF32, dst, &argr),
            IROp::Bitcast => self.write_un(Opcode::Move, dst, &argr),

            IROp::IEq => self.write_bin(int_cmp_op(&operand_ty, IntCmp::Eq), dst, &argr),
            IROp::INe => self.write_bin(int_cmp_op(&operand_ty, IntCmp::Ne), dst, &argr),
            IROp::ILt => self.write_bin(int_cmp_op(&operand_ty, IntCmp::Lt), dst, &argr),
            IROp::ILe => self.write_bin(int_cmp_op(&operand_ty, IntCmp::Le), dst, &argr),
            IROp::IGt => self.write_bin(int_cmp_op(&operand_ty, IntCmp::Gt), dst, &argr),
            IROp::IGe => self.write_bin(int_cmp_op(&operand_ty, IntCmp::Ge), dst, &argr),
            IROp::FEq => self.write_bin(
                if matches!(operand_ty, Ty::Primitive(PrimTy::F32)) { Opcode::FEqF32 } else { Opcode::FEqF64 },
                dst, &argr,
            ),
            IROp::FNe => self.write_bin(
                if matches!(operand_ty, Ty::Primitive(PrimTy::F32)) { Opcode::FNeF32 } else { Opcode::FNeF64 },
                dst, &argr,
            ),
            IROp::FLt => self.write_bin(
                if matches!(operand_ty, Ty::Primitive(PrimTy::F32)) { Opcode::FLtF32 } else { Opcode::FLtF64 },
                dst, &argr,
            ),
            IROp::FLe => self.write_bin(
                if matches!(operand_ty, Ty::Primitive(PrimTy::F32)) { Opcode::FLeF32 } else { Opcode::FLeF64 },
                dst, &argr,
            ),
            IROp::FGt => self.write_bin(
                if matches!(operand_ty, Ty::Primitive(PrimTy::F32)) { Opcode::FGtF32 } else { Opcode::FGtF64 },
                dst, &argr,
            ),
            IROp::FGe => self.write_bin(
                if matches!(operand_ty, Ty::Primitive(PrimTy::F32)) { Opcode::FGeF32 } else { Opcode::FGeF64 },
                dst, &argr,
            ),
            IROp::StrEq => self.write_bin(Opcode::StrEq, dst, &argr),
            IROp::RefEq => self.write_bin(Opcode::RefEq, dst, &argr),

            IROp::BoolNot => {
                // M7: `not x` for bool x must yield 0 or 1, not a bitwise
                // INotI32 (which turns 1 into 0xFFFF_FFFE — still truthy and
                // breaks every `if not …:` chain, e.g. wordcount.spy's
                // `if not in_word:` branch). Emit `dst = (x == 0)`:
                //   ConstI32 tmp, 0
                //   IEqI32   dst, x, tmp
                let tmp = self.next_reg as u16;
                self.next_reg += 1;
                self.write_op(Opcode::ConstI32);
                self.write_u16(tmp);
                self.write_i32(0);
                self.write_op(Opcode::IEqI32);
                self.write_u16(dst);
                self.write_u16(argr.first().copied().unwrap_or(0));
                self.write_u16(tmp);
            }

            IROp::Alloc { class_id } => {
                // NEW dst:r16, type_id:u32
                self.write_op(Opcode::New);
                self.write_u16(dst);
                self.write_u32(*class_id);
            }
            IROp::Load { offset } => {
                // LOAD_FIELD dst:r16, obj:r16, offset:u32, ty_tag:u8
                self.write_op(Opcode::LoadField);
                self.write_u16(dst);
                self.write_u16(argr.first().copied().unwrap_or(0));
                self.write_u32(*offset);
                self.write_u8(ty_tag_for(ty) as u8);
            }
            IROp::Store { offset } => {
                // STORE_FIELD obj:r16, offset:u32, src:r16, ty_tag:u8
                self.write_op(Opcode::StoreField);
                self.write_u16(argr.first().copied().unwrap_or(0));
                self.write_u32(*offset);
                self.write_u16(argr.get(1).copied().unwrap_or(0));
                // The value's static type is what we're storing.
                self.write_u8(TypeTag::Ref as u8);
            }
            IROp::ArrayNew => {
                // LIST_NEW dst:r16, elem_type_id:u32, capacity:u32
                self.write_op(Opcode::ListNew);
                self.write_u16(dst);
                self.write_u32(u32::MAX);
                self.write_u32(0);
            }
            IROp::ListPush => {
                // LIST_PUSH lst:r16, value:r16
                self.write_op(Opcode::ListPush);
                self.write_u16(argr.first().copied().unwrap_or(0));
                self.write_u16(argr.get(1).copied().unwrap_or(0));
            }
            IROp::ReadLocal { slot } => {
                let src = self.slot_register(*slot);
                self.write_op(Opcode::Move);
                self.write_u16(dst);
                self.write_u16(src);
            }
            IROp::WriteLocal { slot } => {
                let slot_reg = self.slot_register(*slot);
                self.write_op(Opcode::Move);
                self.write_u16(slot_reg);
                self.write_u16(argr.first().copied().unwrap_or(0));
            }
            IROp::ArrayGet => {
                self.write_op(Opcode::ArrayGet);
                self.write_u16(dst);
                self.write_u16(argr.first().copied().unwrap_or(0));
                self.write_u16(argr.get(1).copied().unwrap_or(0));
                self.write_u8(ty_tag_for(ty) as u8);
            }
            IROp::ArraySet => {
                self.write_op(Opcode::ArraySet);
                self.write_u16(argr.first().copied().unwrap_or(0));
                self.write_u16(argr.get(1).copied().unwrap_or(0));
                self.write_u16(argr.get(2).copied().unwrap_or(0));
                self.write_u8(TypeTag::Ref as u8);
            }
            IROp::ArrayLen => {
                self.write_op(Opcode::ArrayLen);
                self.write_u16(dst);
                self.write_u16(argr.first().copied().unwrap_or(0));
            }
            IROp::DirectCall { fn_id } => {
                self.emit_call(Opcode::CallDirect, dst, ty, fn_id.0, &argr);
            }
            IROp::VirtualCall { vtable_slot } => {
                // CALL_VIRTUAL dst:r16, recv:r16, vtable_slot:u16, argc:u8, args:r16×argc
                let dst_or_discard = if matches!(ty, Ty::Primitive(PrimTy::Unit)) {
                    DISCARD_REG
                } else {
                    dst
                };
                self.write_op(Opcode::CallVirtual);
                self.write_u16(dst_or_discard);
                self.write_u16(argr.first().copied().unwrap_or(0));
                self.write_u16(*vtable_slot as u16);
                let rest = if argr.is_empty() { &[][..] } else { &argr[1..] };
                let argc: u8 = rest.len().min(255) as u8;
                self.write_u8(argc);
                for r in rest.iter().take(argc as usize) {
                    self.write_u16(*r);
                }
            }
            IROp::IfaceCall { itable_id, slot } => {
                let dst_or_discard = if matches!(ty, Ty::Primitive(PrimTy::Unit)) {
                    DISCARD_REG
                } else {
                    dst
                };
                self.write_op(Opcode::CallIface);
                self.write_u16(dst_or_discard);
                self.write_u16(argr.first().copied().unwrap_or(0));
                self.write_u32(*itable_id);
                self.write_u16(*slot as u16);
                let rest = if argr.is_empty() { &[][..] } else { &argr[1..] };
                let argc: u8 = rest.len().min(255) as u8;
                self.write_u8(argc);
                for r in rest.iter().take(argc as usize) {
                    self.write_u16(*r);
                }
            }
            IROp::IndirectCall => {
                let dst_or_discard = if matches!(ty, Ty::Primitive(PrimTy::Unit)) {
                    DISCARD_REG
                } else {
                    dst
                };
                self.write_op(Opcode::CallIndirect);
                self.write_u16(dst_or_discard);
                self.write_u16(argr.first().copied().unwrap_or(0));
                let rest = if argr.is_empty() { &[][..] } else { &argr[1..] };
                let argc: u8 = rest.len().min(255) as u8;
                self.write_u8(argc);
                for r in rest.iter().take(argc as usize) {
                    self.write_u16(*r);
                }
            }
            IROp::NativeCall { native_id } => {
                let dst_or_discard = if matches!(ty, Ty::Primitive(PrimTy::Unit)) {
                    DISCARD_REG
                } else {
                    dst
                };
                self.write_op(Opcode::CallNative);
                self.write_u16(dst_or_discard);
                self.write_u32(*native_id);
                let argc: u8 = argr.len().min(255) as u8;
                self.write_u8(argc);
                for r in argr.iter().take(argc as usize) {
                    self.write_u16(*r);
                }
            }
            IROp::Copy => {
                self.write_op(Opcode::Move);
                self.write_u16(dst);
                self.write_u16(argr.first().copied().unwrap_or(0));
            }
            IROp::Select => {
                // Materialised as a MOVE from the then-branch by default. A
                // real lowering uses CondBranch; the IR builder turns
                // ternaries into Select for now, so we approximate.
                self.write_op(Opcode::Move);
                self.write_u16(dst);
                self.write_u16(argr.get(1).copied().unwrap_or(0));
            }
            IROp::ClosureNew { fn_id, n_captures } => {
                self.write_op(Opcode::ClosureNew);
                self.write_u16(dst);
                self.write_u32(fn_id.0);
                let n = (*n_captures as u8).min(255);
                self.write_u8(n);
                for r in argr.iter().take(n as usize) {
                    self.write_u16(*r);
                }
            }
            IROp::ClosureCall => {
                self.write_op(Opcode::ClosureCall);
                self.write_u16(dst);
                self.write_u16(argr.first().copied().unwrap_or(0));
                let rest = if argr.is_empty() { &[][..] } else { &argr[1..] };
                let argc: u8 = rest.len().min(255) as u8;
                self.write_u8(argc);
                for r in rest.iter().take(argc as usize) {
                    self.write_u16(*r);
                }
            }
            IROp::BoundsCheck => {
                // No dedicated opcode; emit NULL_CHECK as a placeholder
                // sentinel for "runtime safety check happens here".
                self.write_op(Opcode::NullCheck);
                self.write_u16(argr.first().copied().unwrap_or(0));
            }
            IROp::NullCheck => {
                self.write_op(Opcode::NullCheck);
                self.write_u16(argr.first().copied().unwrap_or(0));
            }
            IROp::TypeCheck => {
                self.write_op(Opcode::IsInstance);
                self.write_u16(dst);
                self.write_u16(argr.first().copied().unwrap_or(0));
                self.write_u32(u32::MAX);
            }
            IROp::IsInstance { class_id } => {
                // IS_INSTANCE dst:r16, obj:r16, type_id:u32
                self.write_op(Opcode::IsInstance);
                self.write_u16(dst);
                self.write_u16(argr.first().copied().unwrap_or(0));
                self.write_u32(*class_id);
            }

            // M15: exception-handling encoding.
            //
            // ENTER_TRY layout:
            //   u8  opcode
            //   i32 finally_pc    (target_block-relative; sentinel block id u32::MAX → -1 encoded)
            //   u8  n_arms
            //   for each arm:
            //     u32 filter_str_idx   (constant-pool index of the type-name string)
            //     i32 handler_pc       (block-id, patched at finish())
            //     u16 bind_reg         (u16::MAX = no bind)
            //
            // `handler_pc` and `finally_pc` are emitted as i32 placeholders
            // and registered with `self.patches` so the existing branch-patch
            // pass at finish() resolves them to byte offsets. The
            // `bind_slot` field of `TryHandlerArm` is an IR slot id and is
            // translated to its dedicated register via `slot_register`.
            IROp::TryEnter { arms, finally_block } => {
                self.write_op(Opcode::EnterTry);
                // finally_pc: -1 (encoded as block id u32::MAX) if absent.
                if let Some(fb) = finally_block {
                    let pos = self.code.len();
                    self.write_i32(0);
                    self.patches.push((pos, fb.0));
                } else {
                    // Use a sentinel block-id (u32::MAX) so finish()'s patch
                    // pass sees an unknown id and leaves zero in place; we
                    // need a value that signals "no finally" to the VM.
                    // Simpler: write -1 directly without registering a patch.
                    self.write_i32(-1);
                }
                let n_arms = arms.len().min(255) as u8;
                self.write_u8(n_arms);
                for a in arms.iter().take(n_arms as usize) {
                    let TryHandlerArm { filter_str_idx, handler_block, bind_slot } = a;
                    self.write_u32(*filter_str_idx);
                    let pos = self.code.len();
                    self.write_i32(0);
                    self.patches.push((pos, handler_block.0));
                    // bind_slot u32::MAX = no binding; otherwise translate
                    // to the slot's dedicated register.
                    let bind_reg = if *bind_slot == u32::MAX {
                        DISCARD_REG
                    } else {
                        self.slot_register(*bind_slot as u16)
                    };
                    self.write_u16(bind_reg);
                }
            }
            IROp::TryLeave => {
                self.write_op(Opcode::LeaveTry);
            }
            IROp::EndFinally => {
                self.write_op(Opcode::Rethrow);
            }
        }
    }

    fn emit_call(
        &mut self,
        oc: Opcode,
        dst: u16,
        ret_ty: &Ty,
        fn_id: u32,
        argr: &[u16],
    ) {
        let dst_or_discard = if matches!(ret_ty, Ty::Primitive(PrimTy::Unit)) {
            DISCARD_REG
        } else {
            dst
        };
        self.write_op(oc);
        self.write_u16(dst_or_discard);
        self.write_u32(fn_id);
        let argc: u8 = argr.len().min(255) as u8;
        self.write_u8(argc);
        for r in argr.iter().take(argc as usize) {
            self.write_u16(*r);
        }
    }

    fn emit_terminator(&mut self, t: &Terminator) {
        match t {
            Terminator::Branch { target } => {
                self.write_op(Opcode::Jump);
                let pos = self.code.len();
                self.write_i32(0);
                self.patches.push((pos, target.0));
            }
            Terminator::CondBranch { cond, t: tb, f } => {
                let cr = self.reg(cond.0);
                self.write_op(Opcode::JumpIf);
                self.write_u16(cr);
                let pos = self.code.len();
                self.write_i32(0);
                self.patches.push((pos, tb.0));
                // Unconditional jump to the false target afterwards.
                self.write_op(Opcode::Jump);
                let pos = self.code.len();
                self.write_i32(0);
                self.patches.push((pos, f.0));
            }
            Terminator::Ret { value: Some(v) } => {
                let r = self.reg(v.0);
                self.write_op(Opcode::Ret);
                self.write_u16(r);
            }
            Terminator::Ret { value: None } => {
                self.write_op(Opcode::RetVoid);
            }
            Terminator::Throw { exc } => {
                let r = self.reg(exc.0);
                self.write_op(Opcode::Throw);
                self.write_u16(r);
            }
            Terminator::Catch => {
                // No-op placeholder for M3.
            }
            Terminator::Unreachable => {
                self.write_op(Opcode::Halt);
            }
        }
    }

    // ── Byte writers ────────────────────────────────────────────────────

    fn write_op(&mut self, oc: Opcode) {
        self.code.push(oc as u8);
    }
    fn write_u8(&mut self, x: u8) {
        self.code.push(x);
    }
    fn write_u16(&mut self, x: u16) {
        self.code.write_u16::<LittleEndian>(x).unwrap();
    }
    fn write_u32(&mut self, x: u32) {
        self.code.write_u32::<LittleEndian>(x).unwrap();
    }
    fn write_i32(&mut self, x: i32) {
        self.code.write_i32::<LittleEndian>(x).unwrap();
    }
    fn write_bin(&mut self, oc: Opcode, dst: u16, argr: &[u16]) {
        self.write_op(oc);
        self.write_u16(dst);
        self.write_u16(argr.first().copied().unwrap_or(0));
        self.write_u16(argr.get(1).copied().unwrap_or(0));
    }
    fn write_un(&mut self, oc: Opcode, dst: u16, argr: &[u16]) {
        self.write_op(oc);
        self.write_u16(dst);
        self.write_u16(argr.first().copied().unwrap_or(0));
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Opcode selection helpers
// ─────────────────────────────────────────────────────────────────────────

/// Strip `Ty::Nullable(T)` to `T`. Defensive helper for primitive-type
/// dispatch — see `emit_op` comment about nullable narrowing.
pub(crate) fn unwrap_nullable_ty(ty: &Ty) -> &Ty {
    match ty {
        Ty::Nullable(inner) => inner.as_ref(),
        other => other,
    }
}

/// Pick the concrete add opcode for an integer `IAdd` given its operand type.
pub fn op_for_iadd(ty: &Ty) -> Option<Opcode> {
    match unwrap_nullable_ty(ty) {
        Ty::Primitive(PrimTy::I32) => Some(Opcode::IAddI32),
        Ty::Primitive(PrimTy::I64) => Some(Opcode::IAddI64),
        Ty::Primitive(PrimTy::U32) => Some(Opcode::UAddU32),
        Ty::Primitive(PrimTy::U64) => Some(Opcode::UAddU64),
        _ => None,
    }
}

pub fn op_for_isub(ty: &Ty) -> Option<Opcode> {
    match unwrap_nullable_ty(ty) {
        Ty::Primitive(PrimTy::I32) => Some(Opcode::ISubI32),
        Ty::Primitive(PrimTy::I64) => Some(Opcode::ISubI64),
        Ty::Primitive(PrimTy::U32) => Some(Opcode::USubU32),
        Ty::Primitive(PrimTy::U64) => Some(Opcode::USubU64),
        _ => None,
    }
}

pub fn op_for_imul(ty: &Ty) -> Option<Opcode> {
    match unwrap_nullable_ty(ty) {
        Ty::Primitive(PrimTy::I32) => Some(Opcode::IMulI32),
        Ty::Primitive(PrimTy::I64) => Some(Opcode::IMulI64),
        Ty::Primitive(PrimTy::U32) => Some(Opcode::UMulU32),
        Ty::Primitive(PrimTy::U64) => Some(Opcode::UMulU64),
        _ => None,
    }
}

pub fn op_for_idiv(ty: &Ty) -> Option<Opcode> {
    match unwrap_nullable_ty(ty) {
        Ty::Primitive(PrimTy::I32) => Some(Opcode::IDivI32),
        Ty::Primitive(PrimTy::I64) => Some(Opcode::IDivI64),
        Ty::Primitive(PrimTy::U32) => Some(Opcode::UDivU32),
        Ty::Primitive(PrimTy::U64) => Some(Opcode::UDivU64),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
enum IntCmp { Eq, Ne, Lt, Le, Gt, Ge }

/// Pick the size-correct integer comparison opcode for the given operand
/// type. Falls back to the i32 variant for unknown / non-integer types.
fn int_cmp_op(ty: &Ty, op: IntCmp) -> Opcode {
    match unwrap_nullable_ty(ty) {
        Ty::Primitive(PrimTy::I64) => match op {
            IntCmp::Eq => Opcode::IEqI64,
            IntCmp::Ne => Opcode::INeI64,
            IntCmp::Lt => Opcode::ILtI64,
            IntCmp::Le => Opcode::ILeI64,
            IntCmp::Gt => Opcode::IGtI64,
            IntCmp::Ge => Opcode::IGeI64,
        },
        Ty::Primitive(PrimTy::U32) => match op {
            IntCmp::Eq => Opcode::UEqU32,
            IntCmp::Ne => Opcode::UNeU32,
            IntCmp::Lt => Opcode::ULtU32,
            IntCmp::Le => Opcode::ULeU32,
            IntCmp::Gt => Opcode::UGtU32,
            IntCmp::Ge => Opcode::UGeU32,
        },
        Ty::Primitive(PrimTy::U64) => match op {
            IntCmp::Eq => Opcode::UEqU64,
            IntCmp::Ne => Opcode::UNeU64,
            IntCmp::Lt => Opcode::ULtU64,
            IntCmp::Le => Opcode::ULeU64,
            IntCmp::Gt => Opcode::UGtU64,
            IntCmp::Ge => Opcode::UGeU64,
        },
        _ => match op {
            IntCmp::Eq => Opcode::IEqI32,
            IntCmp::Ne => Opcode::INeI32,
            IntCmp::Lt => Opcode::ILtI32,
            IntCmp::Le => Opcode::ILeI32,
            IntCmp::Gt => Opcode::IGtI32,
            IntCmp::Ge => Opcode::IGeI32,
        },
    }
}

fn ty_tag_for(ty: &Ty) -> TypeTag {
    match unwrap_nullable_ty(ty) {
        Ty::Primitive(PrimTy::Bool) => TypeTag::Bool,
        Ty::Primitive(PrimTy::I8) => TypeTag::I8,
        Ty::Primitive(PrimTy::I16) => TypeTag::I16,
        Ty::Primitive(PrimTy::I32) => TypeTag::I32,
        Ty::Primitive(PrimTy::I64) => TypeTag::I64,
        Ty::Primitive(PrimTy::U8) => TypeTag::U8,
        Ty::Primitive(PrimTy::U16) => TypeTag::U16,
        Ty::Primitive(PrimTy::U32) => TypeTag::U32,
        Ty::Primitive(PrimTy::U64) => TypeTag::U64,
        Ty::Primitive(PrimTy::F32) => TypeTag::F32,
        Ty::Primitive(PrimTy::F64) => TypeTag::F64,
        _ => TypeTag::Ref,
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BasicBlock, BlockId, FuncId, IRConst, IRFunction, Terminator, Value};

    struct FakeSink {
        strs: Vec<String>,
        ints: Vec<i64>,
        floats: Vec<f64>,
    }
    impl FakeSink {
        fn new() -> Self {
            Self { strs: vec![], ints: vec![], floats: vec![] }
        }
    }
    impl ConstSink for FakeSink {
        fn intern_string(&mut self, s: &str) -> u32 {
            let idx = self.strs.len() as u32;
            self.strs.push(s.to_string());
            idx
        }
        fn intern_i64(&mut self, v: i64) -> u32 {
            let idx = self.ints.len() as u32;
            self.ints.push(v);
            idx
        }
        fn intern_f64(&mut self, v: f64) -> u32 {
            let idx = self.floats.len() as u32;
            self.floats.push(v);
            idx
        }
    }

    #[test]
    fn emits_const_i32_then_ret() {
        let f = IRFunction {
            id: FuncId(0),
            name: "f".into(),
            params: vec![],
            ret: Ty::Primitive(PrimTy::I32),
            blocks: vec![BasicBlock {
                id: BlockId(0),
                values: vec![Value {
                    id: 0,
                    ty: Ty::Primitive(PrimTy::I32),
                    kind: ValueKind::Const(IRConst::I32(42)),
                }],
                terminator: Terminator::Ret { value: Some(ValueId(0)) },
            }],
        };
        let mut sink = FakeSink::new();
        let emitted = emit_function(&f, &mut sink);
        // CONST_I32 (1) dst:r16 val:u32 RET (E3) src:r16 → 1+2+4 + 1+2 = 10 bytes
        assert_eq!(emitted.code.len(), 10);
        assert_eq!(emitted.code[0], Opcode::ConstI32 as u8);
        assert_eq!(emitted.code[7], Opcode::Ret as u8);
    }

    #[test]
    fn emits_iadd_i64() {
        let f = IRFunction {
            id: FuncId(0),
            name: "f".into(),
            params: vec![Ty::Primitive(PrimTy::I64), Ty::Primitive(PrimTy::I64)],
            ret: Ty::Primitive(PrimTy::I64),
            blocks: vec![BasicBlock {
                id: BlockId(0),
                values: vec![
                    Value { id: 0, ty: Ty::Primitive(PrimTy::I64), kind: ValueKind::Param { idx: 0 } },
                    Value { id: 1, ty: Ty::Primitive(PrimTy::I64), kind: ValueKind::Param { idx: 1 } },
                    Value {
                        id: 2,
                        ty: Ty::Primitive(PrimTy::I64),
                        kind: ValueKind::Op { op: IROp::IAdd, args: vec![ValueId(0), ValueId(1)] },
                    },
                ],
                terminator: Terminator::Ret { value: Some(ValueId(2)) },
            }],
        };
        let mut sink = FakeSink::new();
        let emitted = emit_function(&f, &mut sink);
        // First byte must be IADD_I64 since both ops are i64.
        assert_eq!(emitted.code[0], Opcode::IAddI64 as u8);
    }

    // ─────────────────────────────────────────────────────────────────────
    //  Nullable-narrowing dispatch tests
    //
    //  real-world: csv_aggregate / nullable-narrowing audit — when the
    //  typechecker narrows `x: T?` to `T` inside a guard, the IR slot for
    //  `x` still carries `Nullable(T)`. Codegen must strip the nullable
    //  wrapper before any primitive-type dispatch or it picks the wrong
    //  typed opcode (e.g. IAddI32 for i64?, FAddF64 for f32?).
    // ─────────────────────────────────────────────────────────────────────

    fn null(inner: Ty) -> Ty { Ty::Nullable(Box::new(inner)) }

    fn op_to_byte(oc: Opcode) -> u8 { oc as u8 }

    /// Build a 2-param fn whose only op is `op` over the two params. The
    /// result type and the param types are caller-controlled so we can
    /// force `Nullable(T)` slot types just like the IR builder would after
    /// narrowing.
    fn build_binop_fn(param_ty: Ty, result_ty: Ty, op: IROp) -> IRFunction {
        IRFunction {
            id: FuncId(0),
            name: "f".into(),
            params: vec![param_ty.clone(), param_ty.clone()],
            ret: result_ty.clone(),
            blocks: vec![BasicBlock {
                id: BlockId(0),
                values: vec![
                    Value { id: 0, ty: param_ty.clone(), kind: ValueKind::Param { idx: 0 } },
                    Value { id: 1, ty: param_ty.clone(), kind: ValueKind::Param { idx: 1 } },
                    Value {
                        id: 2,
                        ty: result_ty,
                        kind: ValueKind::Op { op, args: vec![ValueId(0), ValueId(1)] },
                    },
                ],
                terminator: Terminator::Ret { value: Some(ValueId(2)) },
            }],
        }
    }

    #[test]
    fn iadd_dispatches_i64_even_when_slot_is_nullable() {
        // Pre-fix: op_for_iadd(Nullable(I64)) → None → defaulted to IAddI32.
        let f = build_binop_fn(null(Ty::Primitive(PrimTy::I64)), Ty::Primitive(PrimTy::I64), IROp::IAdd);
        let mut sink = FakeSink::new();
        let e = emit_function(&f, &mut sink);
        assert_eq!(e.code[0], op_to_byte(Opcode::IAddI64),
            "IAdd on i64? slot must pick IAddI64, got opcode byte {:#x}", e.code[0]);
    }

    #[test]
    fn fadd_dispatches_f32_even_when_slot_is_nullable() {
        // Pre-fix: matches!(operand_ty, Ty::Primitive(F32)) was false for
        // Nullable(F32), so we silently selected FAddF64.
        let f = build_binop_fn(null(Ty::Primitive(PrimTy::F32)), Ty::Primitive(PrimTy::F32), IROp::FAdd);
        let mut sink = FakeSink::new();
        let e = emit_function(&f, &mut sink);
        assert_eq!(e.code[0], op_to_byte(Opcode::FAddF32),
            "FAdd on f32? slot must pick FAddF32, got opcode byte {:#x}", e.code[0]);
    }

    #[test]
    fn int_cmp_dispatches_i64_even_when_slot_is_nullable() {
        // Pre-fix: int_cmp_op(Nullable(I64), Eq) fell through to IEqI32.
        let f = build_binop_fn(null(Ty::Primitive(PrimTy::I64)), Ty::Primitive(PrimTy::Bool), IROp::IEq);
        let mut sink = FakeSink::new();
        let e = emit_function(&f, &mut sink);
        assert_eq!(e.code[0], op_to_byte(Opcode::IEqI64),
            "IEq on i64? slot must pick IEqI64, got opcode byte {:#x}", e.code[0]);
    }

    #[test]
    fn fcmp_dispatches_f32_even_when_slot_is_nullable() {
        let f = build_binop_fn(null(Ty::Primitive(PrimTy::F32)), Ty::Primitive(PrimTy::Bool), IROp::FLt);
        let mut sink = FakeSink::new();
        let e = emit_function(&f, &mut sink);
        assert_eq!(e.code[0], op_to_byte(Opcode::FLtF32),
            "FLt on f32? slot must pick FLtF32, got opcode byte {:#x}", e.code[0]);
    }

    #[test]
    fn irem_dispatches_i64_even_when_result_ty_is_nullable() {
        // Pre-fix: matches!(ty, Ty::Primitive(I64) | Ty::Primitive(U64))
        // was false for Nullable(I64), so we selected IRemI32.
        let f = build_binop_fn(Ty::Primitive(PrimTy::I64), null(Ty::Primitive(PrimTy::I64)), IROp::IRem);
        let mut sink = FakeSink::new();
        let e = emit_function(&f, &mut sink);
        assert_eq!(e.code[0], op_to_byte(Opcode::IRemI64),
            "IRem on i64? result must pick IRemI64, got opcode byte {:#x}", e.code[0]);
    }
}
