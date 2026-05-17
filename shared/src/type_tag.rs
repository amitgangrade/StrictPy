//! Type tags used as inline operand bytes in opcodes that work on multiple
//! primitive widths (e.g. `LOAD_FIELD`, `ARRAY_GET`). See spec §13.3.6.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TypeTag {
    I8 = 0,
    I16 = 1,
    I32 = 2,
    I64 = 3,
    U8 = 4,
    U16 = 5,
    U32 = 6,
    U64 = 7,
    F32 = 8,
    F64 = 9,
    Bool = 10,
    Ref = 11,
}

impl TypeTag {
    pub fn size_bytes(self) -> usize {
        match self {
            TypeTag::I8 | TypeTag::U8 | TypeTag::Bool => 1,
            TypeTag::I16 | TypeTag::U16 => 2,
            TypeTag::I32 | TypeTag::U32 | TypeTag::F32 => 4,
            TypeTag::I64 | TypeTag::U64 | TypeTag::F64 | TypeTag::Ref => 8,
        }
    }

    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            0 => Some(TypeTag::I8),
            1 => Some(TypeTag::I16),
            2 => Some(TypeTag::I32),
            3 => Some(TypeTag::I64),
            4 => Some(TypeTag::U8),
            5 => Some(TypeTag::U16),
            6 => Some(TypeTag::U32),
            7 => Some(TypeTag::U64),
            8 => Some(TypeTag::F32),
            9 => Some(TypeTag::F64),
            10 => Some(TypeTag::Bool),
            11 => Some(TypeTag::Ref),
            _ => None,
        }
    }
}
