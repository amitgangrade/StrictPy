//! `.spyc` file format constants. See spec §12.

/// Magic bytes at the start of every `.spyc` file: ASCII "SPYC".
pub const MAGIC: [u8; 4] = *b"SPYC";

pub const VERSION_MAJOR: u16 = 0;
pub const VERSION_MINOR: u16 = 1;

/// Header is 32 bytes; see spec §12.4.
pub const HEADER_SIZE: usize = 32;

/// Flags bit positions in the header `flags` field.
pub mod flags {
    pub const HAS_DEBUG: u32 = 1 << 0;
    pub const JIT_PREWARMED: u32 = 1 << 1;
}

/// Constant pool entry tags. See spec §12.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ConstTag {
    I32 = 0x01,
    I64 = 0x02,
    U32 = 0x03,
    U64 = 0x04,
    F32 = 0x05,
    F64 = 0x06,
    String = 0x07,
    Bytes = 0x08,
    Char = 0x09,
    BigInt = 0x0A,
}

/// Sentinel for "no base type" in a type table entry.
pub const NO_BASE_TYPE: u32 = 0xFFFF_FFFF;

/// Sentinel for "catch-all" in an exception table entry.
pub const CATCH_ALL_TYPE: u32 = 0xFFFF_FFFF;

/// Sentinel for "discard result" in a call opcode's destination register.
pub const DISCARD_REG: u16 = 0xFFFF;
