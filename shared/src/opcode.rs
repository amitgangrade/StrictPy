//! StrictPy bytecode opcodes. See spec §13.
//!
//! Numbering is part of the binary `.spyc` format and must not change without
//! bumping `file_format::VERSION_MAJOR`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[rustfmt::skip]
pub enum Opcode {
    // 0x01–0x09  Constants & moves
    ConstI32   = 0x01,
    ConstI64   = 0x02,
    ConstF32   = 0x03,
    ConstF64   = 0x04,
    ConstStr   = 0x05,
    ConstTrue  = 0x06,
    ConstFalse = 0x07,
    ConstNone  = 0x08,
    Move       = 0x09,

    // 0x0A–0x0C  M62b: generators (yield)
    /// `MakeGen dst:r16, fn_id:u32, argc:u8, args:r16×argc` — allocate a
    /// generator object capturing `fn_id` and the evaluated argument values.
    /// Does NOT run the body; the generator starts suspended-at-entry. Same
    /// operand encoding as `CallDirect`.
    MakeGen    = 0x0A,
    /// `Yield value:r16` — produce `value` from the current generator frame
    /// and suspend it (saving registers + pc back into the owning generator).
    Yield      = 0x0B,
    /// `GenNext value:r16, gen:r16, done:r16` — resume the generator in `gen`
    /// until its next `yield` (writing the yielded value to `value` and 0 to
    /// `done`) or until it finishes (writing 1 to `done`).
    GenNext    = 0x0C,

    // 0x10–0x1B  i32 arithmetic
    IAddI32 = 0x10, ISubI32 = 0x11, IMulI32 = 0x12, IDivI32 = 0x13,
    IRemI32 = 0x14, INegI32 = 0x15, IAndI32 = 0x16, IOrI32  = 0x17,
    IXorI32 = 0x18, IShlI32 = 0x19, IShrI32 = 0x1A, INotI32 = 0x1B,

    // 0x20–0x2B  i64 arithmetic
    IAddI64 = 0x20, ISubI64 = 0x21, IMulI64 = 0x22, IDivI64 = 0x23,
    IRemI64 = 0x24, INegI64 = 0x25, IAndI64 = 0x26, IOrI64  = 0x27,
    IXorI64 = 0x28, IShlI64 = 0x29, IShrI64 = 0x2A, INotI64 = 0x2B,

    // 0x30–0x3B  u32 arithmetic
    UAddU32 = 0x30, USubU32 = 0x31, UMulU32 = 0x32, UDivU32 = 0x33,
    URemU32 = 0x34, UNegU32 = 0x35, UAndU32 = 0x36, UOrU32  = 0x37,
    UXorU32 = 0x38, UShlU32 = 0x39, UShrU32 = 0x3A, UNotU32 = 0x3B,

    // 0x40–0x4B  u64 arithmetic
    UAddU64 = 0x40, USubU64 = 0x41, UMulU64 = 0x42, UDivU64 = 0x43,
    URemU64 = 0x44, UNegU64 = 0x45, UAndU64 = 0x46, UOrU64  = 0x47,
    UXorU64 = 0x48, UShlU64 = 0x49, UShrU64 = 0x4A, UNotU64 = 0x4B,

    // 0x50–0x5C  Float arithmetic
    FAddF32 = 0x50, FSubF32 = 0x51, FMulF32 = 0x52, FDivF32 = 0x53, FNegF32 = 0x54,
    FAddF64 = 0x58, FSubF64 = 0x59, FMulF64 = 0x5A, FDivF64 = 0x5B, FNegF64 = 0x5C,

    // 0x60–0x65  i32 comparisons
    IEqI32 = 0x60, INeI32 = 0x61, ILtI32 = 0x62, ILeI32 = 0x63, IGtI32 = 0x64, IGeI32 = 0x65,
    // 0x68–0x6D  i64 comparisons
    IEqI64 = 0x68, INeI64 = 0x69, ILtI64 = 0x6A, ILeI64 = 0x6B, IGtI64 = 0x6C, IGeI64 = 0x6D,
    // 0x70–0x75  u32 comparisons
    UEqU32 = 0x70, UNeU32 = 0x71, ULtU32 = 0x72, ULeU32 = 0x73, UGtU32 = 0x74, UGeU32 = 0x75,
    // 0x78–0x7D  u64 comparisons
    UEqU64 = 0x78, UNeU64 = 0x79, ULtU64 = 0x7A, ULeU64 = 0x7B, UGtU64 = 0x7C, UGeU64 = 0x7D,
    // 0x80–0x85  f32 comparisons
    FEqF32 = 0x80, FNeF32 = 0x81, FLtF32 = 0x82, FLeF32 = 0x83, FGtF32 = 0x84, FGeF32 = 0x85,
    // 0x88–0x8D  f64 comparisons
    FEqF64 = 0x88, FNeF64 = 0x89, FLtF64 = 0x8A, FLeF64 = 0x8B, FGtF64 = 0x8C, FGeF64 = 0x8D,
    // 0x90–0x91  str / ref comparisons
    StrEq = 0x90, RefEq = 0x91,

    // 0xA0–0xA7  Conversions
    I32ToI64    = 0xA0,
    I64ToI32    = 0xA1,
    I32ToF64    = 0xA2,
    F64ToI32    = 0xA3,
    F32ToF64    = 0xA4,
    F64ToF32    = 0xA5,
    I32ToBigInt = 0xA6,
    BoolToI32   = 0xA7,

    // 0xB0–0xB7  Object / heap
    New         = 0xB0,
    NewInit     = 0xB1,
    LoadField   = 0xB2,
    StoreField  = 0xB3,
    LoadVtable  = 0xB4,
    IsInstance  = 0xB5,
    CastChecked = 0xB6,
    NullCheck   = 0xB7,

    // 0xC0–0xC5  Arrays
    ArrayNew          = 0xC0,
    ArrayLen          = 0xC1,
    ArrayGet          = 0xC2,
    ArraySet          = 0xC3,
    ArrayGetUnchecked = 0xC4,
    ArraySetUnchecked = 0xC5,

    // 0xD0–0xD5  Calls
    CallDirect     = 0xD0,
    CallVirtual    = 0xD1,
    CallIface      = 0xD2,
    CallIndirect   = 0xD3,
    TailCallDirect = 0xD4,
    CallNative     = 0xD5,

    // 0xE0–0xE9  Control flow
    Jump       = 0xE0,
    JumpIf     = 0xE1,
    JumpIfNot  = 0xE2,
    Ret        = 0xE3,
    RetVoid    = 0xE4,
    Throw      = 0xE5,
    EnterTry   = 0xE6,
    LeaveTry   = 0xE7,
    Rethrow    = 0xE8,
    Switch     = 0xE9,

    // 0xF0–0xF9  List / Dict / String builtins
    ListNew    = 0xF0,
    ListPush   = 0xF1,
    ListPop    = 0xF2,
    DictNew    = 0xF3,
    DictGet    = 0xF4,
    DictSet    = 0xF5,
    DictHas    = 0xF6,
    StrConcat  = 0xF7,
    StrLen     = 0xF8,
    StrCharAt  = 0xF9,

    // 0xFA–0xFB  Closures
    ClosureNew  = 0xFA,
    ClosureCall = 0xFB,

    // 0xFC–0xFF  GC / runtime / debug
    GcSafepoint    = 0xFC,
    GcWriteBarrier = 0xFD,
    DebugNop       = 0xFE,
    Halt           = 0xFF,
}

impl Opcode {
    /// Try to decode a raw byte as an opcode. Returns `None` for unassigned bytes.
    pub fn from_u8(b: u8) -> Option<Self> {
        // SAFETY: every defined discriminant is checked individually; this is the
        // mechanical inverse of the enum declaration above. Kept as a match for
        // exhaustiveness and clarity, not transmute, so unassigned bytes return None.
        match b {
            0x01 => Some(Self::ConstI32),   0x02 => Some(Self::ConstI64),
            0x03 => Some(Self::ConstF32),   0x04 => Some(Self::ConstF64),
            0x05 => Some(Self::ConstStr),   0x06 => Some(Self::ConstTrue),
            0x07 => Some(Self::ConstFalse), 0x08 => Some(Self::ConstNone),
            0x09 => Some(Self::Move),
            0x0A => Some(Self::MakeGen), 0x0B => Some(Self::Yield),
            0x0C => Some(Self::GenNext),

            0x10 => Some(Self::IAddI32), 0x11 => Some(Self::ISubI32),
            0x12 => Some(Self::IMulI32), 0x13 => Some(Self::IDivI32),
            0x14 => Some(Self::IRemI32), 0x15 => Some(Self::INegI32),
            0x16 => Some(Self::IAndI32), 0x17 => Some(Self::IOrI32),
            0x18 => Some(Self::IXorI32), 0x19 => Some(Self::IShlI32),
            0x1A => Some(Self::IShrI32), 0x1B => Some(Self::INotI32),

            0x20 => Some(Self::IAddI64), 0x21 => Some(Self::ISubI64),
            0x22 => Some(Self::IMulI64), 0x23 => Some(Self::IDivI64),
            0x24 => Some(Self::IRemI64), 0x25 => Some(Self::INegI64),
            0x26 => Some(Self::IAndI64), 0x27 => Some(Self::IOrI64),
            0x28 => Some(Self::IXorI64), 0x29 => Some(Self::IShlI64),
            0x2A => Some(Self::IShrI64), 0x2B => Some(Self::INotI64),

            0x30 => Some(Self::UAddU32), 0x31 => Some(Self::USubU32),
            0x32 => Some(Self::UMulU32), 0x33 => Some(Self::UDivU32),
            0x34 => Some(Self::URemU32), 0x35 => Some(Self::UNegU32),
            0x36 => Some(Self::UAndU32), 0x37 => Some(Self::UOrU32),
            0x38 => Some(Self::UXorU32), 0x39 => Some(Self::UShlU32),
            0x3A => Some(Self::UShrU32), 0x3B => Some(Self::UNotU32),

            0x40 => Some(Self::UAddU64), 0x41 => Some(Self::USubU64),
            0x42 => Some(Self::UMulU64), 0x43 => Some(Self::UDivU64),
            0x44 => Some(Self::URemU64), 0x45 => Some(Self::UNegU64),
            0x46 => Some(Self::UAndU64), 0x47 => Some(Self::UOrU64),
            0x48 => Some(Self::UXorU64), 0x49 => Some(Self::UShlU64),
            0x4A => Some(Self::UShrU64), 0x4B => Some(Self::UNotU64),

            0x50 => Some(Self::FAddF32), 0x51 => Some(Self::FSubF32),
            0x52 => Some(Self::FMulF32), 0x53 => Some(Self::FDivF32),
            0x54 => Some(Self::FNegF32),
            0x58 => Some(Self::FAddF64), 0x59 => Some(Self::FSubF64),
            0x5A => Some(Self::FMulF64), 0x5B => Some(Self::FDivF64),
            0x5C => Some(Self::FNegF64),

            0x60 => Some(Self::IEqI32), 0x61 => Some(Self::INeI32),
            0x62 => Some(Self::ILtI32), 0x63 => Some(Self::ILeI32),
            0x64 => Some(Self::IGtI32), 0x65 => Some(Self::IGeI32),
            0x68 => Some(Self::IEqI64), 0x69 => Some(Self::INeI64),
            0x6A => Some(Self::ILtI64), 0x6B => Some(Self::ILeI64),
            0x6C => Some(Self::IGtI64), 0x6D => Some(Self::IGeI64),
            0x70 => Some(Self::UEqU32), 0x71 => Some(Self::UNeU32),
            0x72 => Some(Self::ULtU32), 0x73 => Some(Self::ULeU32),
            0x74 => Some(Self::UGtU32), 0x75 => Some(Self::UGeU32),
            0x78 => Some(Self::UEqU64), 0x79 => Some(Self::UNeU64),
            0x7A => Some(Self::ULtU64), 0x7B => Some(Self::ULeU64),
            0x7C => Some(Self::UGtU64), 0x7D => Some(Self::UGeU64),
            0x80 => Some(Self::FEqF32), 0x81 => Some(Self::FNeF32),
            0x82 => Some(Self::FLtF32), 0x83 => Some(Self::FLeF32),
            0x84 => Some(Self::FGtF32), 0x85 => Some(Self::FGeF32),
            0x88 => Some(Self::FEqF64), 0x89 => Some(Self::FNeF64),
            0x8A => Some(Self::FLtF64), 0x8B => Some(Self::FLeF64),
            0x8C => Some(Self::FGtF64), 0x8D => Some(Self::FGeF64),
            0x90 => Some(Self::StrEq),  0x91 => Some(Self::RefEq),

            0xA0 => Some(Self::I32ToI64),    0xA1 => Some(Self::I64ToI32),
            0xA2 => Some(Self::I32ToF64),    0xA3 => Some(Self::F64ToI32),
            0xA4 => Some(Self::F32ToF64),    0xA5 => Some(Self::F64ToF32),
            0xA6 => Some(Self::I32ToBigInt), 0xA7 => Some(Self::BoolToI32),

            0xB0 => Some(Self::New),         0xB1 => Some(Self::NewInit),
            0xB2 => Some(Self::LoadField),   0xB3 => Some(Self::StoreField),
            0xB4 => Some(Self::LoadVtable),  0xB5 => Some(Self::IsInstance),
            0xB6 => Some(Self::CastChecked), 0xB7 => Some(Self::NullCheck),

            0xC0 => Some(Self::ArrayNew),          0xC1 => Some(Self::ArrayLen),
            0xC2 => Some(Self::ArrayGet),          0xC3 => Some(Self::ArraySet),
            0xC4 => Some(Self::ArrayGetUnchecked), 0xC5 => Some(Self::ArraySetUnchecked),

            0xD0 => Some(Self::CallDirect),     0xD1 => Some(Self::CallVirtual),
            0xD2 => Some(Self::CallIface),      0xD3 => Some(Self::CallIndirect),
            0xD4 => Some(Self::TailCallDirect), 0xD5 => Some(Self::CallNative),

            0xE0 => Some(Self::Jump),       0xE1 => Some(Self::JumpIf),
            0xE2 => Some(Self::JumpIfNot),  0xE3 => Some(Self::Ret),
            0xE4 => Some(Self::RetVoid),    0xE5 => Some(Self::Throw),
            0xE6 => Some(Self::EnterTry),   0xE7 => Some(Self::LeaveTry),
            0xE8 => Some(Self::Rethrow),    0xE9 => Some(Self::Switch),

            0xF0 => Some(Self::ListNew),   0xF1 => Some(Self::ListPush),
            0xF2 => Some(Self::ListPop),   0xF3 => Some(Self::DictNew),
            0xF4 => Some(Self::DictGet),   0xF5 => Some(Self::DictSet),
            0xF6 => Some(Self::DictHas),   0xF7 => Some(Self::StrConcat),
            0xF8 => Some(Self::StrLen),    0xF9 => Some(Self::StrCharAt),

            0xFA => Some(Self::ClosureNew),    0xFB => Some(Self::ClosureCall),
            0xFC => Some(Self::GcSafepoint),   0xFD => Some(Self::GcWriteBarrier),
            0xFE => Some(Self::DebugNop),      0xFF => Some(Self::Halt),

            _ => None,
        }
    }
}
