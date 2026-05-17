//! Bytecode → JIT-friendly instruction stream.
//!
//! The compiler's typed SSA IR is not preserved across the `.spyc` boundary,
//! but the bytecode itself carries enough information to JIT each function:
//! every opcode names its operand width (e.g. `IAddI64`), every constant
//! references the constant pool, and every branch is a self-relative offset.
//!
//! This module walks the bytes for one function and produces:
//!   1. A list of [`Op`]s in source order (already-decoded, typed instructions
//!      with raw register / constant indices).
//!   2. A set of block boundary PCs (= every branch target plus every
//!      instruction immediately after a branch / return). The JIT uses these
//!      to slice the op list into basic blocks.
//!
//! No attempt is made to reconstruct SSA form; the JIT keeps the
//! register-machine shape and emits a Cranelift [`Variable`] per register.
//!
//! If the decoder runs into an opcode it doesn't recognise *or* one the JIT
//! deliberately doesn't support (calls into the GC, vtables, exceptions, …),
//! it returns [`DecodeError::Unsupported`] and the caller leaves the function
//! to the interpreter.

use std::collections::BTreeSet;

use strictpy_shared::Opcode;

use crate::loader::Module;

/// Decoded instruction. PC is the byte offset of the opcode itself within
/// the module's code section; `next_pc` is the offset of the following
/// instruction (i.e. the value branch offsets are relative to).
#[derive(Debug, Clone)]
pub struct DecodedOp {
    pub pc: usize,
    pub next_pc: usize,
    pub kind: Op,
}

/// All ops the JIT can translate. Anything outside this set causes
/// [`decode_function`] to return [`DecodeError::Unsupported`].
#[derive(Debug, Clone)]
pub enum Op {
    // Constants
    ConstI32 { dst: u16, val: i32 },
    ConstI64 { dst: u16, idx: u32 },
    ConstF32 { dst: u16, bits: u32 },
    ConstF64 { dst: u16, idx: u32 },
    ConstTrue { dst: u16 },
    ConstFalse { dst: u16 },
    /// `dst = alloc_string_from_pool(idx)` — runtime allocation through the
    /// interpreter's heap via the JIT trampoline.
    ConstStr { dst: u16, idx: u32 },
    /// `dst = NONE_SENTINEL` — special "absent" sentinel used by
    /// `try_recv`/`dict.get` to signal a missing value. Lowered to a
    /// constant load via a runtime helper that reads the sentinel byte
    /// pattern (so JIT'd code stays in sync if it ever changes).
    ConstNoneSentinel { dst: u16 },
    Move { dst: u16, src: u16 },

    // Integer binops — width tagged by NumWidth.
    IBin { op: IntBinOp, w: NumWidth, dst: u16, a: u16, b: u16 },
    INeg { w: NumWidth, dst: u16, a: u16 },
    IDivChk { w: NumWidth, dst: u16, a: u16, b: u16 },
    IRemChk { w: NumWidth, dst: u16, a: u16, b: u16 },

    // Float binops
    FBin { op: FloatBinOp, w: FloatWidth, dst: u16, a: u16, b: u16 },
    FNeg { w: FloatWidth, dst: u16, a: u16 },

    // Comparisons
    ICmp { op: IntCmp, w: NumWidth, signed: bool, dst: u16, a: u16, b: u16 },
    FCmp { op: FloatCmp, w: FloatWidth, dst: u16, a: u16, b: u16 },

    // Conversions (i32/i64/f64 only — others fall back to interpreter)
    I32ToI64 { dst: u16, src: u16 },
    I64ToI32 { dst: u16, src: u16 },
    I32ToF64 { dst: u16, src: u16 },
    F64ToI32 { dst: u16, src: u16 },

    // Control flow
    Jump { target: usize },
    JumpIf { cond: u16, target: usize, fallthrough: usize },
    JumpIfNot { cond: u16, target: usize, fallthrough: usize },
    Ret { src: u16 },
    RetVoid,

    // Calls
    CallDirect { dst: u16, fn_id: u32, args: Vec<u16> },
    CallNative { dst: u16, native_id: u32, args: Vec<u16> },

    // List intrinsics
    ArrayLen { dst: u16, list: u16 },
    /// `dst = list[idx]`, interpreting the element via `elem_tag`.
    ArrayGet { dst: u16, list: u16, idx: u16, elem_tag: u8 },
    /// `list[idx] = src`. M9 only stores 8-byte slots (matches the
    /// type-erased list representation).
    ArraySet { list: u16, idx: u16, src: u16, elem_tag: u8 },
    /// `dst = new_list(capacity=initial_capacity)`. The compiler emits this
    /// via `ListNew` with the initial capacity in the second u32 operand;
    /// the JIT calls `rt_list_new` to allocate.
    ListNew { dst: u16, capacity: u32 },
    /// `list.append(value)`. Calls `rt_list_push`.
    ListPush { list: u16, value: u16 },
    /// `dst = new_array(length)` — pre-sized list with `length = capacity`,
    /// every slot zeroed. Calls `rt_array_new`.
    ArrayNew { dst: u16, length: u16 },

    // User-class object ops (M9 Tier 2)
    /// `dst = alloc(class_id)`. Calls `rt_alloc` which looks up the runtime
    /// type and asks the heap for `sizeof(type)` bytes with the right vtable
    /// installed.
    New { dst: u16, type_id: u32 },
    /// `dst = *(obj + HDR + offset) :: ty_tag`. Inline load with width
    /// selected by `ty_tag`.
    LoadField { dst: u16, obj: u16, offset: u32, ty_tag: u8 },
    /// `*(obj + HDR + offset) :: ty_tag = src`. Inline store.
    StoreField { obj: u16, offset: u32, src: u16, ty_tag: u8 },
    /// `dst = obj.vtable[slot](obj, args...)` — indirect call through the
    /// object's vtable pointer. Args follow the receiver in `args`.
    VirtualCall { dst: u16, recv: u16, vtable_slot: u16, args: Vec<u16> },

    // NullCheck — used both as null guard and as the placeholder for
    // BoundsCheck the compiler emits. Safe to lower as a "trap if zero".
    NullCheck { src: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumWidth { W32, W64 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatWidth { F32, F64 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntBinOp { Add, Sub, Mul, And, Or, Xor, Shl, ShrSigned, ShrUnsigned }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatBinOp { Add, Sub, Mul, Div }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntCmp { Eq, Ne, Lt, Le, Gt, Ge }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatCmp { Eq, Ne, Lt, Le, Gt, Ge }

#[derive(Debug)]
pub enum DecodeError {
    Unsupported(&'static str),
    Truncated,
    BadByte(u8),
}

pub struct DecodedFunction {
    /// Instructions in source order.
    pub ops: Vec<DecodedOp>,
    /// Set of pcs that start a basic block. Always contains the function's
    /// entry pc.
    pub block_starts: BTreeSet<usize>,
}

/// Decode a single function's bytecode region into [`DecodedFunction`].
pub fn decode_function(
    module: &Module,
    code_offset: u32,
    code_length: u32,
) -> Result<DecodedFunction, DecodeError> {
    let start = code_offset as usize;
    let end = start + code_length as usize;
    let bytes = module.code.as_slice();
    if end > bytes.len() {
        return Err(DecodeError::Truncated);
    }
    let mut ops: Vec<DecodedOp> = Vec::new();
    let mut block_starts: BTreeSet<usize> = BTreeSet::new();
    block_starts.insert(start);

    let mut pc = start;
    while pc < end {
        let pc_op = pc;
        let byte = bytes[pc];
        pc += 1;
        let oc = Opcode::from_u8(byte).ok_or(DecodeError::BadByte(byte))?;
        let kind = match oc {
            Opcode::ConstI32 => {
                let dst = read_u16(bytes, &mut pc, end)?;
                let val = read_u32(bytes, &mut pc, end)? as i32;
                Op::ConstI32 { dst, val }
            }
            Opcode::ConstI64 => {
                let dst = read_u16(bytes, &mut pc, end)?;
                let idx = read_u32(bytes, &mut pc, end)?;
                Op::ConstI64 { dst, idx }
            }
            Opcode::ConstF32 => {
                let dst = read_u16(bytes, &mut pc, end)?;
                let bits = read_u32(bytes, &mut pc, end)?;
                Op::ConstF32 { dst, bits }
            }
            Opcode::ConstF64 => {
                let dst = read_u16(bytes, &mut pc, end)?;
                let idx = read_u32(bytes, &mut pc, end)?;
                Op::ConstF64 { dst, idx }
            }
            Opcode::ConstTrue => {
                let dst = read_u16(bytes, &mut pc, end)?;
                Op::ConstTrue { dst }
            }
            Opcode::ConstFalse => {
                let dst = read_u16(bytes, &mut pc, end)?;
                Op::ConstFalse { dst }
            }
            Opcode::ConstStr => {
                let dst = read_u16(bytes, &mut pc, end)?;
                let idx = read_u32(bytes, &mut pc, end)?;
                Op::ConstStr { dst, idx }
            }
            Opcode::ConstNone => {
                let dst = read_u16(bytes, &mut pc, end)?;
                Op::ConstNoneSentinel { dst }
            }
            Opcode::Move => {
                let dst = read_u16(bytes, &mut pc, end)?;
                let src = read_u16(bytes, &mut pc, end)?;
                Op::Move { dst, src }
            }

            // ─── Int binops ────────────────────────────────────────────
            Opcode::IAddI32 => decode_ibin(IntBinOp::Add, NumWidth::W32, bytes, &mut pc, end)?,
            Opcode::ISubI32 => decode_ibin(IntBinOp::Sub, NumWidth::W32, bytes, &mut pc, end)?,
            Opcode::IMulI32 => decode_ibin(IntBinOp::Mul, NumWidth::W32, bytes, &mut pc, end)?,
            Opcode::IAndI32 => decode_ibin(IntBinOp::And, NumWidth::W32, bytes, &mut pc, end)?,
            Opcode::IOrI32  => decode_ibin(IntBinOp::Or,  NumWidth::W32, bytes, &mut pc, end)?,
            Opcode::IXorI32 => decode_ibin(IntBinOp::Xor, NumWidth::W32, bytes, &mut pc, end)?,
            Opcode::IShlI32 => decode_ibin(IntBinOp::Shl, NumWidth::W32, bytes, &mut pc, end)?,
            Opcode::IShrI32 => decode_ibin(IntBinOp::ShrSigned, NumWidth::W32, bytes, &mut pc, end)?,
            Opcode::INegI32 => decode_ineg(NumWidth::W32, bytes, &mut pc, end)?,
            Opcode::IDivI32 => decode_idiv(NumWidth::W32, bytes, &mut pc, end)?,
            Opcode::IRemI32 => decode_irem(NumWidth::W32, bytes, &mut pc, end)?,

            Opcode::IAddI64 => decode_ibin(IntBinOp::Add, NumWidth::W64, bytes, &mut pc, end)?,
            Opcode::ISubI64 => decode_ibin(IntBinOp::Sub, NumWidth::W64, bytes, &mut pc, end)?,
            Opcode::IMulI64 => decode_ibin(IntBinOp::Mul, NumWidth::W64, bytes, &mut pc, end)?,
            Opcode::IAndI64 => decode_ibin(IntBinOp::And, NumWidth::W64, bytes, &mut pc, end)?,
            Opcode::IOrI64  => decode_ibin(IntBinOp::Or,  NumWidth::W64, bytes, &mut pc, end)?,
            Opcode::IXorI64 => decode_ibin(IntBinOp::Xor, NumWidth::W64, bytes, &mut pc, end)?,
            Opcode::IShlI64 => decode_ibin(IntBinOp::Shl, NumWidth::W64, bytes, &mut pc, end)?,
            Opcode::IShrI64 => decode_ibin(IntBinOp::ShrSigned, NumWidth::W64, bytes, &mut pc, end)?,
            Opcode::INegI64 => decode_ineg(NumWidth::W64, bytes, &mut pc, end)?,
            Opcode::IDivI64 => decode_idiv(NumWidth::W64, bytes, &mut pc, end)?,
            Opcode::IRemI64 => decode_irem(NumWidth::W64, bytes, &mut pc, end)?,

            // Unsigned arithmetic (same bit-ops as signed except shr is logical)
            Opcode::UAddU32 => decode_ibin(IntBinOp::Add, NumWidth::W32, bytes, &mut pc, end)?,
            Opcode::USubU32 => decode_ibin(IntBinOp::Sub, NumWidth::W32, bytes, &mut pc, end)?,
            Opcode::UMulU32 => decode_ibin(IntBinOp::Mul, NumWidth::W32, bytes, &mut pc, end)?,
            Opcode::UShrU32 => decode_ibin(IntBinOp::ShrUnsigned, NumWidth::W32, bytes, &mut pc, end)?,
            Opcode::UAddU64 => decode_ibin(IntBinOp::Add, NumWidth::W64, bytes, &mut pc, end)?,
            Opcode::USubU64 => decode_ibin(IntBinOp::Sub, NumWidth::W64, bytes, &mut pc, end)?,
            Opcode::UMulU64 => decode_ibin(IntBinOp::Mul, NumWidth::W64, bytes, &mut pc, end)?,
            Opcode::UShrU64 => decode_ibin(IntBinOp::ShrUnsigned, NumWidth::W64, bytes, &mut pc, end)?,

            // ─── Float binops ──────────────────────────────────────────
            Opcode::FAddF32 => decode_fbin(FloatBinOp::Add, FloatWidth::F32, bytes, &mut pc, end)?,
            Opcode::FSubF32 => decode_fbin(FloatBinOp::Sub, FloatWidth::F32, bytes, &mut pc, end)?,
            Opcode::FMulF32 => decode_fbin(FloatBinOp::Mul, FloatWidth::F32, bytes, &mut pc, end)?,
            Opcode::FDivF32 => decode_fbin(FloatBinOp::Div, FloatWidth::F32, bytes, &mut pc, end)?,
            Opcode::FNegF32 => decode_fneg(FloatWidth::F32, bytes, &mut pc, end)?,
            Opcode::FAddF64 => decode_fbin(FloatBinOp::Add, FloatWidth::F64, bytes, &mut pc, end)?,
            Opcode::FSubF64 => decode_fbin(FloatBinOp::Sub, FloatWidth::F64, bytes, &mut pc, end)?,
            Opcode::FMulF64 => decode_fbin(FloatBinOp::Mul, FloatWidth::F64, bytes, &mut pc, end)?,
            Opcode::FDivF64 => decode_fbin(FloatBinOp::Div, FloatWidth::F64, bytes, &mut pc, end)?,
            Opcode::FNegF64 => decode_fneg(FloatWidth::F64, bytes, &mut pc, end)?,

            // ─── Comparisons ───────────────────────────────────────────
            Opcode::IEqI32 => decode_icmp(IntCmp::Eq, NumWidth::W32, true, bytes, &mut pc, end)?,
            Opcode::INeI32 => decode_icmp(IntCmp::Ne, NumWidth::W32, true, bytes, &mut pc, end)?,
            Opcode::ILtI32 => decode_icmp(IntCmp::Lt, NumWidth::W32, true, bytes, &mut pc, end)?,
            Opcode::ILeI32 => decode_icmp(IntCmp::Le, NumWidth::W32, true, bytes, &mut pc, end)?,
            Opcode::IGtI32 => decode_icmp(IntCmp::Gt, NumWidth::W32, true, bytes, &mut pc, end)?,
            Opcode::IGeI32 => decode_icmp(IntCmp::Ge, NumWidth::W32, true, bytes, &mut pc, end)?,
            Opcode::IEqI64 => decode_icmp(IntCmp::Eq, NumWidth::W64, true, bytes, &mut pc, end)?,
            Opcode::INeI64 => decode_icmp(IntCmp::Ne, NumWidth::W64, true, bytes, &mut pc, end)?,
            Opcode::ILtI64 => decode_icmp(IntCmp::Lt, NumWidth::W64, true, bytes, &mut pc, end)?,
            Opcode::ILeI64 => decode_icmp(IntCmp::Le, NumWidth::W64, true, bytes, &mut pc, end)?,
            Opcode::IGtI64 => decode_icmp(IntCmp::Gt, NumWidth::W64, true, bytes, &mut pc, end)?,
            Opcode::IGeI64 => decode_icmp(IntCmp::Ge, NumWidth::W64, true, bytes, &mut pc, end)?,
            Opcode::UEqU32 => decode_icmp(IntCmp::Eq, NumWidth::W32, false, bytes, &mut pc, end)?,
            Opcode::UNeU32 => decode_icmp(IntCmp::Ne, NumWidth::W32, false, bytes, &mut pc, end)?,
            Opcode::ULtU32 => decode_icmp(IntCmp::Lt, NumWidth::W32, false, bytes, &mut pc, end)?,
            Opcode::ULeU32 => decode_icmp(IntCmp::Le, NumWidth::W32, false, bytes, &mut pc, end)?,
            Opcode::UGtU32 => decode_icmp(IntCmp::Gt, NumWidth::W32, false, bytes, &mut pc, end)?,
            Opcode::UGeU32 => decode_icmp(IntCmp::Ge, NumWidth::W32, false, bytes, &mut pc, end)?,
            Opcode::UEqU64 => decode_icmp(IntCmp::Eq, NumWidth::W64, false, bytes, &mut pc, end)?,
            Opcode::UNeU64 => decode_icmp(IntCmp::Ne, NumWidth::W64, false, bytes, &mut pc, end)?,
            Opcode::ULtU64 => decode_icmp(IntCmp::Lt, NumWidth::W64, false, bytes, &mut pc, end)?,
            Opcode::ULeU64 => decode_icmp(IntCmp::Le, NumWidth::W64, false, bytes, &mut pc, end)?,
            Opcode::UGtU64 => decode_icmp(IntCmp::Gt, NumWidth::W64, false, bytes, &mut pc, end)?,
            Opcode::UGeU64 => decode_icmp(IntCmp::Ge, NumWidth::W64, false, bytes, &mut pc, end)?,

            Opcode::FEqF32 => decode_fcmp(FloatCmp::Eq, FloatWidth::F32, bytes, &mut pc, end)?,
            Opcode::FNeF32 => decode_fcmp(FloatCmp::Ne, FloatWidth::F32, bytes, &mut pc, end)?,
            Opcode::FLtF32 => decode_fcmp(FloatCmp::Lt, FloatWidth::F32, bytes, &mut pc, end)?,
            Opcode::FLeF32 => decode_fcmp(FloatCmp::Le, FloatWidth::F32, bytes, &mut pc, end)?,
            Opcode::FGtF32 => decode_fcmp(FloatCmp::Gt, FloatWidth::F32, bytes, &mut pc, end)?,
            Opcode::FGeF32 => decode_fcmp(FloatCmp::Ge, FloatWidth::F32, bytes, &mut pc, end)?,
            Opcode::FEqF64 => decode_fcmp(FloatCmp::Eq, FloatWidth::F64, bytes, &mut pc, end)?,
            Opcode::FNeF64 => decode_fcmp(FloatCmp::Ne, FloatWidth::F64, bytes, &mut pc, end)?,
            Opcode::FLtF64 => decode_fcmp(FloatCmp::Lt, FloatWidth::F64, bytes, &mut pc, end)?,
            Opcode::FLeF64 => decode_fcmp(FloatCmp::Le, FloatWidth::F64, bytes, &mut pc, end)?,
            Opcode::FGtF64 => decode_fcmp(FloatCmp::Gt, FloatWidth::F64, bytes, &mut pc, end)?,
            Opcode::FGeF64 => decode_fcmp(FloatCmp::Ge, FloatWidth::F64, bytes, &mut pc, end)?,

            // ─── Conversions ───────────────────────────────────────────
            Opcode::I32ToI64 => {
                let dst = read_u16(bytes, &mut pc, end)?;
                let src = read_u16(bytes, &mut pc, end)?;
                Op::I32ToI64 { dst, src }
            }
            Opcode::I64ToI32 => {
                let dst = read_u16(bytes, &mut pc, end)?;
                let src = read_u16(bytes, &mut pc, end)?;
                Op::I64ToI32 { dst, src }
            }
            Opcode::I32ToF64 => {
                let dst = read_u16(bytes, &mut pc, end)?;
                let src = read_u16(bytes, &mut pc, end)?;
                Op::I32ToF64 { dst, src }
            }
            Opcode::F64ToI32 => {
                let dst = read_u16(bytes, &mut pc, end)?;
                let src = read_u16(bytes, &mut pc, end)?;
                Op::F64ToI32 { dst, src }
            }

            // ─── Control flow ──────────────────────────────────────────
            Opcode::Jump => {
                let off = read_i32(bytes, &mut pc, end)?;
                let target = ((pc as isize) + off as isize) as usize;
                block_starts.insert(target);
                block_starts.insert(pc); // fallthrough won't be reached but it terminates the block
                Op::Jump { target }
            }
            Opcode::JumpIf => {
                let cond = read_u16(bytes, &mut pc, end)?;
                let off = read_i32(bytes, &mut pc, end)?;
                let target = ((pc as isize) + off as isize) as usize;
                let fallthrough = pc;
                block_starts.insert(target);
                block_starts.insert(fallthrough);
                Op::JumpIf { cond, target, fallthrough }
            }
            Opcode::JumpIfNot => {
                let cond = read_u16(bytes, &mut pc, end)?;
                let off = read_i32(bytes, &mut pc, end)?;
                let target = ((pc as isize) + off as isize) as usize;
                let fallthrough = pc;
                block_starts.insert(target);
                block_starts.insert(fallthrough);
                Op::JumpIfNot { cond, target, fallthrough }
            }
            Opcode::Ret => {
                let src = read_u16(bytes, &mut pc, end)?;
                if pc < end {
                    block_starts.insert(pc);
                }
                Op::Ret { src }
            }
            Opcode::RetVoid => {
                if pc < end {
                    block_starts.insert(pc);
                }
                Op::RetVoid
            }

            // ─── Calls ─────────────────────────────────────────────────
            Opcode::CallDirect => {
                let dst = read_u16(bytes, &mut pc, end)?;
                let fn_id = read_u32(bytes, &mut pc, end)?;
                let argc = read_u8(bytes, &mut pc, end)? as usize;
                let mut args = Vec::with_capacity(argc);
                for _ in 0..argc {
                    args.push(read_u16(bytes, &mut pc, end)?);
                }
                Op::CallDirect { dst, fn_id, args }
            }
            Opcode::CallNative => {
                let dst = read_u16(bytes, &mut pc, end)?;
                let native_id = read_u32(bytes, &mut pc, end)?;
                let argc = read_u8(bytes, &mut pc, end)? as usize;
                let mut args = Vec::with_capacity(argc);
                for _ in 0..argc {
                    args.push(read_u16(bytes, &mut pc, end)?);
                }
                Op::CallNative { dst, native_id, args }
            }

            // ─── List intrinsics ───────────────────────────────────────
            Opcode::ArrayLen => {
                let dst = read_u16(bytes, &mut pc, end)?;
                let list = read_u16(bytes, &mut pc, end)?;
                Op::ArrayLen { dst, list }
            }
            Opcode::ArrayGet | Opcode::ArrayGetUnchecked => {
                let dst = read_u16(bytes, &mut pc, end)?;
                let list = read_u16(bytes, &mut pc, end)?;
                let idx = read_u16(bytes, &mut pc, end)?;
                let elem_tag = read_u8(bytes, &mut pc, end)?;
                Op::ArrayGet { dst, list, idx, elem_tag }
            }
            Opcode::ArraySet | Opcode::ArraySetUnchecked => {
                // ARRAY_SET arr:r16, idx:r16, src:r16, ty_tag:u8
                let list = read_u16(bytes, &mut pc, end)?;
                let idx = read_u16(bytes, &mut pc, end)?;
                let src = read_u16(bytes, &mut pc, end)?;
                let elem_tag = read_u8(bytes, &mut pc, end)?;
                Op::ArraySet { list, idx, src, elem_tag }
            }
            Opcode::ListNew => {
                // LIST_NEW dst:r16, elem_type_id:u32, capacity:u32
                let dst = read_u16(bytes, &mut pc, end)?;
                let _elem_type_id = read_u32(bytes, &mut pc, end)?;
                let capacity = read_u32(bytes, &mut pc, end)?;
                Op::ListNew { dst, capacity }
            }
            Opcode::ListPush => {
                // The interpreter and compiler both emit LIST_PUSH as a
                // 4-byte payload (list:r16, value:r16). The spec mentions
                // a ty_tag but the compiler doesn't write one — match the
                // observed bytecode rather than the spec note.
                let list = read_u16(bytes, &mut pc, end)?;
                let value = read_u16(bytes, &mut pc, end)?;
                Op::ListPush { list, value }
            }
            Opcode::ArrayNew => {
                // ARRAY_NEW dst:r16, elem_type_id:u32, len:r16
                let dst = read_u16(bytes, &mut pc, end)?;
                let _elem_type_id = read_u32(bytes, &mut pc, end)?;
                let length = read_u16(bytes, &mut pc, end)?;
                Op::ArrayNew { dst, length }
            }

            // ─── Object / heap ─────────────────────────────────────────
            Opcode::New => {
                // NEW dst:r16, type_id:u32
                let dst = read_u16(bytes, &mut pc, end)?;
                let type_id = read_u32(bytes, &mut pc, end)?;
                Op::New { dst, type_id }
            }
            Opcode::LoadField => {
                // LOAD_FIELD dst:r16, obj:r16, offset:u32, ty_tag:u8
                let dst = read_u16(bytes, &mut pc, end)?;
                let obj = read_u16(bytes, &mut pc, end)?;
                let offset = read_u32(bytes, &mut pc, end)?;
                let ty_tag = read_u8(bytes, &mut pc, end)?;
                Op::LoadField { dst, obj, offset, ty_tag }
            }
            Opcode::StoreField => {
                // STORE_FIELD obj:r16, offset:u32, src:r16, ty_tag:u8
                let obj = read_u16(bytes, &mut pc, end)?;
                let offset = read_u32(bytes, &mut pc, end)?;
                let src = read_u16(bytes, &mut pc, end)?;
                let ty_tag = read_u8(bytes, &mut pc, end)?;
                Op::StoreField { obj, offset, src, ty_tag }
            }
            Opcode::CallVirtual => {
                // CALL_VIRTUAL dst:r16, recv:r16, vtable_slot:u16, argc:u8, args:r16×argc
                let dst = read_u16(bytes, &mut pc, end)?;
                let recv = read_u16(bytes, &mut pc, end)?;
                let vtable_slot = read_u16(bytes, &mut pc, end)?;
                let argc = read_u8(bytes, &mut pc, end)? as usize;
                let mut args = Vec::with_capacity(argc);
                for _ in 0..argc {
                    args.push(read_u16(bytes, &mut pc, end)?);
                }
                Op::VirtualCall { dst, recv, vtable_slot, args }
            }

            // ─── Misc ──────────────────────────────────────────────────
            Opcode::NullCheck => {
                let src = read_u16(bytes, &mut pc, end)?;
                Op::NullCheck { src }
            }
            Opcode::DebugNop => continue,

            // Everything else falls back to the interpreter.
            _ => return Err(DecodeError::Unsupported(opcode_name(oc))),
        };
        ops.push(DecodedOp { pc: pc_op, next_pc: pc, kind });
    }

    Ok(DecodedFunction { ops, block_starts })
}

fn opcode_name(oc: Opcode) -> &'static str {
    // Just enough to make error messages useful when triaging which op
    // blocked JIT compilation.
    match oc {
        Opcode::New => "New",
        Opcode::NewInit => "NewInit",
        Opcode::LoadField => "LoadField",
        Opcode::StoreField => "StoreField",
        Opcode::LoadVtable => "LoadVtable",
        Opcode::IsInstance => "IsInstance",
        Opcode::CastChecked => "CastChecked",
        Opcode::ArrayNew => "ArrayNew",
        Opcode::ArraySet | Opcode::ArraySetUnchecked => "ArraySet",
        Opcode::CallVirtual => "CallVirtual",
        Opcode::CallIface => "CallIface",
        Opcode::CallIndirect => "CallIndirect",
        Opcode::TailCallDirect => "TailCallDirect",
        Opcode::Throw => "Throw",
        Opcode::EnterTry | Opcode::LeaveTry | Opcode::Rethrow => "TryCatch",
        Opcode::Switch => "Switch",
        Opcode::ListNew => "ListNew",
        Opcode::ListPush => "ListPush",
        Opcode::ListPop => "ListPop",
        Opcode::DictNew | Opcode::DictGet | Opcode::DictSet | Opcode::DictHas => "Dict",
        Opcode::StrConcat => "StrConcat",
        Opcode::StrLen => "StrLen",
        Opcode::StrCharAt => "StrCharAt",
        Opcode::StrEq => "StrEq",
        Opcode::RefEq => "RefEq",
        Opcode::ClosureNew | Opcode::ClosureCall => "Closure",
        Opcode::GcSafepoint | Opcode::GcWriteBarrier => "Gc",
        Opcode::ConstStr => "ConstStr",
        Opcode::ConstNone => "ConstNone",
        Opcode::I32ToBigInt | Opcode::F32ToF64 | Opcode::F64ToF32 | Opcode::BoolToI32 => "Conv",
        Opcode::Halt => "Halt",
        _ => "Other",
    }
}

fn decode_ibin(
    op: IntBinOp,
    w: NumWidth,
    bytes: &[u8],
    pc: &mut usize,
    end: usize,
) -> Result<Op, DecodeError> {
    let dst = read_u16(bytes, pc, end)?;
    let a = read_u16(bytes, pc, end)?;
    let b = read_u16(bytes, pc, end)?;
    Ok(Op::IBin { op, w, dst, a, b })
}

fn decode_ineg(
    w: NumWidth,
    bytes: &[u8],
    pc: &mut usize,
    end: usize,
) -> Result<Op, DecodeError> {
    let dst = read_u16(bytes, pc, end)?;
    let a = read_u16(bytes, pc, end)?;
    Ok(Op::INeg { w, dst, a })
}

fn decode_idiv(
    w: NumWidth,
    bytes: &[u8],
    pc: &mut usize,
    end: usize,
) -> Result<Op, DecodeError> {
    let dst = read_u16(bytes, pc, end)?;
    let a = read_u16(bytes, pc, end)?;
    let b = read_u16(bytes, pc, end)?;
    Ok(Op::IDivChk { w, dst, a, b })
}

fn decode_irem(
    w: NumWidth,
    bytes: &[u8],
    pc: &mut usize,
    end: usize,
) -> Result<Op, DecodeError> {
    let dst = read_u16(bytes, pc, end)?;
    let a = read_u16(bytes, pc, end)?;
    let b = read_u16(bytes, pc, end)?;
    Ok(Op::IRemChk { w, dst, a, b })
}

fn decode_fbin(
    op: FloatBinOp,
    w: FloatWidth,
    bytes: &[u8],
    pc: &mut usize,
    end: usize,
) -> Result<Op, DecodeError> {
    let dst = read_u16(bytes, pc, end)?;
    let a = read_u16(bytes, pc, end)?;
    let b = read_u16(bytes, pc, end)?;
    Ok(Op::FBin { op, w, dst, a, b })
}

fn decode_fneg(
    w: FloatWidth,
    bytes: &[u8],
    pc: &mut usize,
    end: usize,
) -> Result<Op, DecodeError> {
    let dst = read_u16(bytes, pc, end)?;
    let a = read_u16(bytes, pc, end)?;
    Ok(Op::FNeg { w, dst, a })
}

fn decode_icmp(
    op: IntCmp,
    w: NumWidth,
    signed: bool,
    bytes: &[u8],
    pc: &mut usize,
    end: usize,
) -> Result<Op, DecodeError> {
    let dst = read_u16(bytes, pc, end)?;
    let a = read_u16(bytes, pc, end)?;
    let b = read_u16(bytes, pc, end)?;
    Ok(Op::ICmp { op, w, signed, dst, a, b })
}

fn decode_fcmp(
    op: FloatCmp,
    w: FloatWidth,
    bytes: &[u8],
    pc: &mut usize,
    end: usize,
) -> Result<Op, DecodeError> {
    let dst = read_u16(bytes, pc, end)?;
    let a = read_u16(bytes, pc, end)?;
    let b = read_u16(bytes, pc, end)?;
    Ok(Op::FCmp { op, w, dst, a, b })
}

fn read_u8(bytes: &[u8], pc: &mut usize, end: usize) -> Result<u8, DecodeError> {
    if *pc >= end {
        return Err(DecodeError::Truncated);
    }
    let v = bytes[*pc];
    *pc += 1;
    Ok(v)
}

fn read_u16(bytes: &[u8], pc: &mut usize, end: usize) -> Result<u16, DecodeError> {
    if *pc + 2 > end {
        return Err(DecodeError::Truncated);
    }
    let v = u16::from_le_bytes([bytes[*pc], bytes[*pc + 1]]);
    *pc += 2;
    Ok(v)
}

fn read_u32(bytes: &[u8], pc: &mut usize, end: usize) -> Result<u32, DecodeError> {
    if *pc + 4 > end {
        return Err(DecodeError::Truncated);
    }
    let v = u32::from_le_bytes([
        bytes[*pc], bytes[*pc + 1], bytes[*pc + 2], bytes[*pc + 3],
    ]);
    *pc += 4;
    Ok(v)
}

fn read_i32(bytes: &[u8], pc: &mut usize, end: usize) -> Result<i32, DecodeError> {
    read_u32(bytes, pc, end).map(|v| v as i32)
}
