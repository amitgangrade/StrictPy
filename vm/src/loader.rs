//! `.spyc` module loader. See spec §12.
//!
//! Parses a `.spyc` byte slice into an in-memory [`Module`]. The header
//! offsets locate each section; within a section M3 (the compiler) prefixes
//! the payload with a `u32` entry count for the constant pool, type table,
//! function table, and string table. The loader consumes those counts.
//!
//! ## Notes on the M3 quirks the loader handles
//!
//! * Each section starts with a `u32` entry count. The spec §12.5 doesn't
//!   require this — it was a "self-describing parse" convenience — but the
//!   compiler emits it, so we consume it.
//! * Constant-pool `String` payloads are `u32` indices into the string table
//!   (not inline bytes).
//! * `exception_table_offset` in the function record is a file offset, but
//!   M3 always writes `0` (no exception tables yet). The loader still parses
//!   the field but leaves `exception_table` empty.
//! * `Bool` and `None` constants are encoded as `I32` (see §12.5 / writer).

use byteorder::{ByteOrder, LittleEndian};

use strictpy_shared::file_format::{
    ConstTag, HEADER_SIZE, MAGIC, VERSION_MAJOR, VERSION_MINOR,
};

use crate::error::VmError;

/// A fully loaded `.spyc` module.
#[derive(Debug)]
pub struct Module {
    pub header: Header,
    pub constants: Vec<Constant>,
    pub types: Vec<TypeInfo>,
    pub functions: Vec<Function>,
    pub code: Vec<u8>,
    pub strings: Vec<String>,
}

/// Parsed 32-byte file header (spec §12.4).
#[derive(Debug, Clone, Copy)]
pub struct Header {
    pub version_major: u16,
    pub version_minor: u16,
    pub flags: u32,
    pub const_pool_offset: u32,
    pub type_table_offset: u32,
    pub function_table_offset: u32,
    pub code_offset: u32,
    pub string_table_offset: u32,
}

/// A constant-pool entry, mirroring [`ConstTag`].
#[derive(Debug, Clone)]
pub enum Constant {
    I32(i32),
    I64(i64),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    /// Index into the string table.
    String(u32),
    Bytes(Vec<u8>),
    Char(u32),
    /// Two's-complement big-endian bytes; left raw for the BigInt runtime to parse.
    BigInt(Vec<u8>),
}

/// A type-table entry (spec §12.6). The vtable / itable are loaded but not
/// yet interpreted; that happens during link.
#[derive(Debug, Clone)]
pub struct TypeInfo {
    pub type_id: u32,
    pub kind: u8,
    pub name_idx: u32,
    pub size: u32,
    pub vtable_len: u16,
    /// `NO_BASE_TYPE` sentinel means "no base".
    pub base_type: u32,
    pub fields: Vec<FieldInfo>,
    pub vtable: Vec<u32>,
    pub itable: Vec<ItableEntry>,
}

/// A field descriptor inside a [`TypeInfo`].
#[derive(Debug, Clone, Copy)]
pub struct FieldInfo {
    pub name_idx: u32,
    pub type_id: u32,
    pub offset: u32,
}

/// An itable entry mapping a protocol to a contiguous block of method ids
/// inside the parent type's itable function-id array.
#[derive(Debug, Clone, Copy)]
pub struct ItableEntry {
    pub protocol_type_id: u32,
    pub itable_fn_ids_offset: u32,
    pub method_count: u16,
}

/// A function-table entry (spec §12.7).
#[derive(Debug, Clone)]
pub struct Function {
    pub fn_id: u32,
    pub name_idx: u32,
    pub type_id: u32,
    pub code_offset: u32,
    pub code_length: u32,
    pub num_params: u16,
    pub num_locals: u16,
    pub num_registers: u16,
    pub flags: u16,
    pub exception_table: Vec<ExceptionEntry>,
    /// File offset of debug info, or 0 if absent.
    pub debug_info_offset: u32,
}

/// An exception-table entry (spec §12.8).
#[derive(Debug, Clone, Copy)]
pub struct ExceptionEntry {
    pub start_pc: u32,
    pub end_pc: u32,
    pub handler_pc: u32,
    /// `CATCH_ALL_TYPE` sentinel means catch-all.
    pub caught_type_id: u32,
}

/// Parse a `.spyc` byte slice into a [`Module`].
///
/// Verifies the magic and version, then parses every section in turn.
pub fn load(bytes: &[u8]) -> Result<Module, VmError> {
    let header = parse_header(bytes)?;

    let strings = parse_string_table(bytes, &header)?;
    let constants = parse_constant_pool(bytes, &header)?;
    let types = parse_type_table(bytes, &header)?;
    let functions = parse_function_table(bytes, &header)?;
    let code = parse_code_section(bytes, &header, &functions)?;

    Ok(Module {
        header,
        constants,
        types,
        functions,
        code,
        strings,
    })
}

/// Real header parser. Verifies the magic + version, then unpacks the 32 byte
/// header into a [`Header`] struct. Exposed at crate root for the header
/// round-trip integration test.
pub fn parse_header(bytes: &[u8]) -> Result<Header, VmError> {
    if bytes.len() < HEADER_SIZE {
        return Err(VmError::Corrupt(format!(
            "file is {} bytes, shorter than the {}-byte header",
            bytes.len(),
            HEADER_SIZE
        )));
    }

    if bytes[0..4] != MAGIC {
        return Err(VmError::BadMagic);
    }

    let version_major = LittleEndian::read_u16(&bytes[4..6]);
    let version_minor = LittleEndian::read_u16(&bytes[6..8]);
    if version_major != VERSION_MAJOR || version_minor != VERSION_MINOR {
        return Err(VmError::UnsupportedVersion {
            found: (version_major, version_minor),
            expected: (VERSION_MAJOR, VERSION_MINOR),
        });
    }

    let flags = LittleEndian::read_u32(&bytes[8..12]);
    let const_pool_offset = LittleEndian::read_u32(&bytes[12..16]);
    let type_table_offset = LittleEndian::read_u32(&bytes[16..20]);
    let function_table_offset = LittleEndian::read_u32(&bytes[20..24]);
    let code_offset = LittleEndian::read_u32(&bytes[24..28]);
    let string_table_offset = LittleEndian::read_u32(&bytes[28..32]);

    Ok(Header {
        version_major,
        version_minor,
        flags,
        const_pool_offset,
        type_table_offset,
        function_table_offset,
        code_offset,
        string_table_offset,
    })
}

// ─────────────────────────────────────────────────────────────────────────
//  Cursor helpers
// ─────────────────────────────────────────────────────────────────────────

/// Tiny little-endian byte cursor with bounds-checked reads.
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], pos: usize) -> Self {
        Self { bytes, pos }
    }

    fn need(&self, n: usize) -> Result<(), VmError> {
        if self.pos.checked_add(n).map_or(true, |end| end > self.bytes.len()) {
            return Err(VmError::Corrupt(format!(
                "unexpected end of file at offset {} (need {n} more bytes)",
                self.pos
            )));
        }
        Ok(())
    }

    fn u8(&mut self) -> Result<u8, VmError> {
        self.need(1)?;
        let v = self.bytes[self.pos];
        self.pos += 1;
        Ok(v)
    }

    fn u16(&mut self) -> Result<u16, VmError> {
        self.need(2)?;
        let v = LittleEndian::read_u16(&self.bytes[self.pos..self.pos + 2]);
        self.pos += 2;
        Ok(v)
    }

    fn u32(&mut self) -> Result<u32, VmError> {
        self.need(4)?;
        let v = LittleEndian::read_u32(&self.bytes[self.pos..self.pos + 4]);
        self.pos += 4;
        Ok(v)
    }

    fn i32(&mut self) -> Result<i32, VmError> {
        self.u32().map(|x| x as i32)
    }

    fn u64(&mut self) -> Result<u64, VmError> {
        self.need(8)?;
        let v = LittleEndian::read_u64(&self.bytes[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(v)
    }

    fn i64(&mut self) -> Result<i64, VmError> {
        self.u64().map(|x| x as i64)
    }

    fn f32(&mut self) -> Result<f32, VmError> {
        self.u32().map(f32::from_bits)
    }

    fn f64(&mut self) -> Result<f64, VmError> {
        self.u64().map(f64::from_bits)
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], VmError> {
        self.need(n)?;
        let s = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Section parsers
// ─────────────────────────────────────────────────────────────────────────

fn parse_string_table(bytes: &[u8], header: &Header) -> Result<Vec<String>, VmError> {
    if header.string_table_offset == 0 {
        return Ok(Vec::new());
    }
    let mut c = Cursor::new(bytes, header.string_table_offset as usize);
    let count = c.u32()? as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let len = c.u32()? as usize;
        let raw = c.bytes(len)?;
        let s = std::str::from_utf8(raw)
            .map_err(|e| VmError::Corrupt(format!("string {i} not valid UTF-8: {e}")))?
            .to_string();
        out.push(s);
    }
    Ok(out)
}

fn parse_constant_pool(bytes: &[u8], header: &Header) -> Result<Vec<Constant>, VmError> {
    if header.const_pool_offset == 0 {
        return Ok(Vec::new());
    }
    let mut c = Cursor::new(bytes, header.const_pool_offset as usize);
    let count = c.u32()? as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let tag = c.u8()?;
        let entry = match tag {
            t if t == ConstTag::I32 as u8 => Constant::I32(c.i32()?),
            t if t == ConstTag::I64 as u8 => Constant::I64(c.i64()?),
            t if t == ConstTag::U32 as u8 => Constant::U32(c.u32()?),
            t if t == ConstTag::U64 as u8 => Constant::U64(c.u64()?),
            t if t == ConstTag::F32 as u8 => Constant::F32(c.f32()?),
            t if t == ConstTag::F64 as u8 => Constant::F64(c.f64()?),
            t if t == ConstTag::String as u8 => Constant::String(c.u32()?),
            t if t == ConstTag::Bytes as u8 => {
                let n = c.u32()? as usize;
                Constant::Bytes(c.bytes(n)?.to_vec())
            }
            t if t == ConstTag::Char as u8 => Constant::Char(c.u32()?),
            t if t == ConstTag::BigInt as u8 => {
                let n = c.u32()? as usize;
                Constant::BigInt(c.bytes(n)?.to_vec())
            }
            other => {
                return Err(VmError::Corrupt(format!(
                    "constant {i}: unknown tag 0x{other:02X}"
                )));
            }
        };
        out.push(entry);
    }
    Ok(out)
}

fn parse_type_table(bytes: &[u8], header: &Header) -> Result<Vec<TypeInfo>, VmError> {
    if header.type_table_offset == 0 {
        return Ok(Vec::new());
    }
    let mut c = Cursor::new(bytes, header.type_table_offset as usize);
    let count = c.u32()? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let type_id = c.u32()?;
        let kind = c.u8()?;
        let name_idx = c.u32()?;
        let size = c.u32()?;
        let field_count = c.u16()? as usize;
        let vtable_len = c.u16()?;
        let base_type = c.u32()?;
        let mut fields = Vec::with_capacity(field_count);
        for _ in 0..field_count {
            let name_idx = c.u32()?;
            let type_id = c.u32()?;
            let offset = c.u32()?;
            fields.push(FieldInfo { name_idx, type_id, offset });
        }
        let mut vtable = Vec::with_capacity(vtable_len as usize);
        for _ in 0..vtable_len {
            vtable.push(c.u32()?);
        }
        let itable_count = c.u16()? as usize;
        let mut itable = Vec::with_capacity(itable_count);
        for _ in 0..itable_count {
            let protocol_type_id = c.u32()?;
            let itable_fn_ids_offset = c.u32()?;
            let method_count = c.u16()?;
            itable.push(ItableEntry {
                protocol_type_id,
                itable_fn_ids_offset,
                method_count,
            });
        }
        out.push(TypeInfo {
            type_id,
            kind,
            name_idx,
            size,
            vtable_len,
            base_type,
            fields,
            vtable,
            itable,
        });
    }
    Ok(out)
}

fn parse_function_table(bytes: &[u8], header: &Header) -> Result<Vec<Function>, VmError> {
    if header.function_table_offset == 0 {
        return Ok(Vec::new());
    }
    let mut c = Cursor::new(bytes, header.function_table_offset as usize);
    let count = c.u32()? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let fn_id = c.u32()?;
        let name_idx = c.u32()?;
        let type_id = c.u32()?;
        let code_offset = c.u32()?;
        let code_length = c.u32()?;
        let num_params = c.u16()?;
        let num_locals = c.u16()?;
        let num_registers = c.u16()?;
        let flags = c.u16()?;
        let _exception_table_offset = c.u32()?; // M3 always emits 0
        let debug_info_offset = c.u32()?;
        out.push(Function {
            fn_id,
            name_idx,
            type_id,
            code_offset,
            code_length,
            num_params,
            num_locals,
            num_registers,
            flags,
            // Spec leaves the on-disk exception-table format open; M3 doesn't
            // emit any, so this is always empty for now.
            exception_table: Vec::new(),
            debug_info_offset,
        });
    }
    Ok(out)
}

fn parse_code_section(
    bytes: &[u8],
    header: &Header,
    functions: &[Function],
) -> Result<Vec<u8>, VmError> {
    // The code section is a concatenated blob; each function's
    // `code_offset` is relative to the start of the section.
    if header.code_offset == 0 {
        return Ok(Vec::new());
    }
    let start = header.code_offset as usize;
    // Compute end-of-code as max(code_offset + code_length) across all
    // functions. If there are no functions, the section is empty.
    let mut end = start;
    for f in functions {
        let f_end = start
            .checked_add(f.code_offset as usize)
            .and_then(|s| s.checked_add(f.code_length as usize))
            .ok_or_else(|| VmError::Corrupt("function code offset overflow".into()))?;
        if f_end > end {
            end = f_end;
        }
    }
    if end > bytes.len() {
        return Err(VmError::Corrupt(format!(
            "code section ends at {end} but file is only {} bytes",
            bytes.len()
        )));
    }
    Ok(bytes[start..end].to_vec())
}

// ─────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use byteorder::WriteBytesExt;

    /// Build a synthetic .spyc with just enough to exercise every section
    /// parser: one i64 constant, no types, one function "main" of 1 byte
    /// (a HALT opcode), one string.
    fn build_minimal_module() -> Vec<u8> {
        let mut buf = Vec::new();
        // Header placeholders.
        buf.extend_from_slice(&MAGIC);
        buf.write_u16::<LittleEndian>(VERSION_MAJOR).unwrap();
        buf.write_u16::<LittleEndian>(VERSION_MINOR).unwrap();
        buf.write_u32::<LittleEndian>(0).unwrap(); // flags
        // Placeholder offsets — patch later.
        let off_const = buf.len();
        buf.write_u32::<LittleEndian>(0).unwrap();
        let off_type = buf.len();
        buf.write_u32::<LittleEndian>(0).unwrap();
        let off_func = buf.len();
        buf.write_u32::<LittleEndian>(0).unwrap();
        let off_code = buf.len();
        buf.write_u32::<LittleEndian>(0).unwrap();
        let off_str = buf.len();
        buf.write_u32::<LittleEndian>(0).unwrap();
        assert_eq!(buf.len(), HEADER_SIZE);

        // Constant pool: 1 i64.
        let const_pool_off = buf.len() as u32;
        buf.write_u32::<LittleEndian>(1).unwrap();
        buf.push(ConstTag::I64 as u8);
        buf.write_i64::<LittleEndian>(42).unwrap();

        // Type table: empty.
        let type_table_off = buf.len() as u32;
        buf.write_u32::<LittleEndian>(0).unwrap();

        // Function table: 1 function.
        let func_table_off = buf.len() as u32;
        buf.write_u32::<LittleEndian>(1).unwrap();
        buf.write_u32::<LittleEndian>(0).unwrap(); // fn_id
        buf.write_u32::<LittleEndian>(0).unwrap(); // name_idx
        buf.write_u32::<LittleEndian>(0).unwrap(); // type_id
        buf.write_u32::<LittleEndian>(0).unwrap(); // code_offset
        buf.write_u32::<LittleEndian>(1).unwrap(); // code_length
        buf.write_u16::<LittleEndian>(0).unwrap(); // num_params
        buf.write_u16::<LittleEndian>(0).unwrap(); // num_locals
        buf.write_u16::<LittleEndian>(1).unwrap(); // num_registers
        buf.write_u16::<LittleEndian>(0).unwrap(); // flags
        buf.write_u32::<LittleEndian>(0).unwrap(); // exception_table_offset
        buf.write_u32::<LittleEndian>(0).unwrap(); // debug_info_offset

        // Code section: HALT (0xFF).
        let code_off = buf.len() as u32;
        buf.push(0xFF);

        // String table: 1 string "main".
        let str_off = buf.len() as u32;
        buf.write_u32::<LittleEndian>(1).unwrap();
        buf.write_u32::<LittleEndian>(4).unwrap();
        buf.extend_from_slice(b"main");

        // Patch offsets.
        LittleEndian::write_u32(&mut buf[off_const..off_const + 4], const_pool_off);
        LittleEndian::write_u32(&mut buf[off_type..off_type + 4], type_table_off);
        LittleEndian::write_u32(&mut buf[off_func..off_func + 4], func_table_off);
        LittleEndian::write_u32(&mut buf[off_code..off_code + 4], code_off);
        LittleEndian::write_u32(&mut buf[off_str..off_str + 4], str_off);

        buf
    }

    #[test]
    fn loads_minimal_module() {
        let bytes = build_minimal_module();
        let m = load(&bytes).expect("module should load");
        assert_eq!(m.constants.len(), 1);
        assert!(matches!(m.constants[0], Constant::I64(42)));
        assert_eq!(m.strings, vec!["main".to_string()]);
        assert_eq!(m.functions.len(), 1);
        assert_eq!(m.functions[0].code_length, 1);
        assert_eq!(m.code.len(), 1);
        assert_eq!(m.code[0], 0xFF);
        assert!(m.types.is_empty());
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = build_minimal_module();
        bytes[0] = b'X';
        match load(&bytes) {
            Err(VmError::BadMagic) => {}
            other => panic!("expected BadMagic, got {other:?}"),
        }
    }
}
