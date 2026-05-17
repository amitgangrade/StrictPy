//! Stable native-function identifiers shared between the compiler and VM.
//!
//! Every prelude/builtin and stdlib symbol that the compiler lowers to a
//! `CALL_NATIVE` opcode is assigned a `u32` here. Numbering is in stable
//! blocks (1–29 core, 30–49 io, 50–59 channels, 60–69 threads, 70–89 math,
//! 90–119 list/dict/string ops, 120+ misc) so future additions don't
//! perturb existing IDs. Zero is reserved as "invalid".
//!
//! The VM (M4) dispatches `CALL_NATIVE` by switching on these ids.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum NativeFn {
    // ── 1–29: core builtins ─────────────────────────────────────────────
    Println       = 1,
    Print         = 2,
    Len           = 3,
    Range         = 4,
    Assert        = 5,
    StrFromI32    = 6,
    StrFromI64    = 7,
    StrFromF64    = 8,
    StrFromBool   = 9,
    StrFromChar   = 10,
    StrConcat     = 11,
    Abs           = 12,
    Min           = 13,
    Max           = 14,
    I32FromI64    = 15,
    I64FromI32    = 16,
    F64FromI32    = 17,
    F64FromI64    = 18,
    I32FromF64    = 19,
    StrFromBytes  = 20,
    StrFromAny    = 21, // generic `str(x)` fallback
    BoolFromAny   = 22,
    CharFromI32   = 23,
    StrSlice      = 24,
    StrAppendChar = 25,

    // ── 30–49: io ───────────────────────────────────────────────────────
    IoOpen     = 30,
    FileRead   = 31,
    FileWrite  = 32,
    FileClose  = 33,
    FileEnter  = 34, // for `with` desugaring (__enter__)
    FileExit   = 35,

    // ── 50–59: channels ─────────────────────────────────────────────────
    ChannelNew     = 50,
    ChannelSend    = 51,
    ChannelRecv    = 52,
    ChannelTryRecv = 53,
    ChannelClose   = 54,

    // ── 60–69: threads ──────────────────────────────────────────────────
    ThreadNew   = 60,
    ThreadStart = 61,
    ThreadJoin  = 62,

    // ── 70–89: math ─────────────────────────────────────────────────────
    MathSqrt = 70,
    MathSin  = 71,
    MathCos  = 72,
    MathTan  = 73,
    MathLog  = 74,
    MathExp  = 75,
    MathPow  = 76,
    MathFloor = 77,
    MathCeil = 78,
    MathAbsF = 79,

    // ── 90–119: list / dict / set ops not covered by dedicated opcodes ──
    ListAppend = 90,
    ListGet    = 91,
    ListSet    = 92,
    ListLen    = 93,
    DictGet    = 94,
    DictSet    = 95,
    DictHas    = 96,
    DictKeys   = 97,
    DictValues = 98,
    DictLen    = 99,
    SetAdd     = 100,
    SetHas     = 101,
    SetLen     = 102,
    /// M7: allocate a fresh empty dict. Used to lower `{}` dict literals
    /// so they materialise an actual DictRepr handle instead of a null
    /// pointer. See vm/src/interp.rs::alloc_dict.
    DictNew    = 103,
    /// M7: read `s[i]` for strings — returns the i-th char as u32. Mirrors
    /// the existing STR_CHAR_AT opcode but is reachable from the
    /// NativeCall path used by the Index-expr lowering.
    StrCharAt  = 104,

    // ── 120+: misc ──────────────────────────────────────────────────────
    /// Fallback for any unrecognised prelude/stdlib symbol the M3 lowerer
    /// encounters. The VM treats this as a runtime error.
    Unknown = 0xFFFF_FFFF,
}

impl NativeFn {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            1 => Some(Self::Println),
            2 => Some(Self::Print),
            3 => Some(Self::Len),
            4 => Some(Self::Range),
            5 => Some(Self::Assert),
            6 => Some(Self::StrFromI32),
            7 => Some(Self::StrFromI64),
            8 => Some(Self::StrFromF64),
            9 => Some(Self::StrFromBool),
            10 => Some(Self::StrFromChar),
            11 => Some(Self::StrConcat),
            12 => Some(Self::Abs),
            13 => Some(Self::Min),
            14 => Some(Self::Max),
            15 => Some(Self::I32FromI64),
            16 => Some(Self::I64FromI32),
            17 => Some(Self::F64FromI32),
            18 => Some(Self::F64FromI64),
            19 => Some(Self::I32FromF64),
            20 => Some(Self::StrFromBytes),
            21 => Some(Self::StrFromAny),
            22 => Some(Self::BoolFromAny),
            23 => Some(Self::CharFromI32),
            24 => Some(Self::StrSlice),
            25 => Some(Self::StrAppendChar),
            30 => Some(Self::IoOpen),
            31 => Some(Self::FileRead),
            32 => Some(Self::FileWrite),
            33 => Some(Self::FileClose),
            34 => Some(Self::FileEnter),
            35 => Some(Self::FileExit),
            50 => Some(Self::ChannelNew),
            51 => Some(Self::ChannelSend),
            52 => Some(Self::ChannelRecv),
            53 => Some(Self::ChannelTryRecv),
            54 => Some(Self::ChannelClose),
            60 => Some(Self::ThreadNew),
            61 => Some(Self::ThreadStart),
            62 => Some(Self::ThreadJoin),
            70 => Some(Self::MathSqrt),
            71 => Some(Self::MathSin),
            72 => Some(Self::MathCos),
            73 => Some(Self::MathTan),
            74 => Some(Self::MathLog),
            75 => Some(Self::MathExp),
            76 => Some(Self::MathPow),
            77 => Some(Self::MathFloor),
            78 => Some(Self::MathCeil),
            79 => Some(Self::MathAbsF),
            90 => Some(Self::ListAppend),
            91 => Some(Self::ListGet),
            92 => Some(Self::ListSet),
            93 => Some(Self::ListLen),
            94 => Some(Self::DictGet),
            95 => Some(Self::DictSet),
            96 => Some(Self::DictHas),
            97 => Some(Self::DictKeys),
            98 => Some(Self::DictValues),
            99 => Some(Self::DictLen),
            100 => Some(Self::SetAdd),
            101 => Some(Self::SetHas),
            102 => Some(Self::SetLen),
            103 => Some(Self::DictNew),
            104 => Some(Self::StrCharAt),
            0xFFFF_FFFF => Some(Self::Unknown),
            _ => None,
        }
    }

    /// Look up a native id by name. Used by the IR lowerer when it sees a
    /// call to a prelude-registered symbol.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "println"     => Some(Self::Println),
            "print"       => Some(Self::Print),
            "len"         => Some(Self::Len),
            "range"       => Some(Self::Range),
            "assert"      => Some(Self::Assert),
            "abs"         => Some(Self::Abs),
            "min"         => Some(Self::Min),
            "max"         => Some(Self::Max),

            "str"         => Some(Self::StrFromAny),
            "bool"        => Some(Self::BoolFromAny),
            "i32"         => Some(Self::I32FromF64),     // primitive ctor; codegen disambiguates by arg type
            "i64"         => Some(Self::I64FromI32),
            "f64"         => Some(Self::F64FromI64),
            "char"        => Some(Self::CharFromI32),

            "open"        => Some(Self::IoOpen),
            // io.File methods
            "read"        => Some(Self::FileRead),
            "write"       => Some(Self::FileWrite),
            "close"       => Some(Self::FileClose),

            // Channel methods (also covers method calls on Channel[T])
            "send"        => Some(Self::ChannelSend),
            "recv"        => Some(Self::ChannelRecv),
            "try_recv"    => Some(Self::ChannelTryRecv),

            // Thread methods
            "start"       => Some(Self::ThreadStart),
            "join"        => Some(Self::ThreadJoin),

            // math
            "sqrt"        => Some(Self::MathSqrt),
            "sin"         => Some(Self::MathSin),
            "cos"         => Some(Self::MathCos),

            // list/dict ops by method name
            "append"      => Some(Self::ListAppend),
            "get"         => Some(Self::DictGet),
            "has"         => Some(Self::DictHas),
            "keys"        => Some(Self::DictKeys),
            "values"      => Some(Self::DictValues),
            "slice"       => Some(Self::StrSlice),

            _ => None,
        }
    }
}
