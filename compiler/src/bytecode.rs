//! `.spyc` bytecode writer. See spec §12.
//!
//! Section layout (spec §12.3):
//!
//! ```text
//!   Header                32 bytes
//!   Constant pool         variable
//!   Type table            variable
//!   Function table        variable
//!   Code section          variable
//!   String table          variable
//! ```
//!
//! Little-endian throughout (§12.2). The header is written with placeholder
//! offsets, then each section is emitted and its real offset recorded back
//! into the header by [`patch_header_offsets`].

use std::collections::HashMap;

use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};

use strictpy_shared::file_format::{
    ConstTag, HEADER_SIZE, MAGIC, VERSION_MAJOR, VERSION_MINOR,
};

use crate::codegen::{self, ConstSink, EmittedFunction};
use crate::ir::{IRConst, IRModule, TypeFieldEntry, TypeTableEntry};

// ─────────────────────────────────────────────────────────────────────────
//  Internal const-pool builder
// ─────────────────────────────────────────────────────────────────────────

/// Owned, ordered, deduplicated constant pool built up while codegen runs.
#[derive(Debug, Default)]
struct ConstPool {
    entries: Vec<IRConst>,
    str_idx: HashMap<String, u32>,
    i64_idx: HashMap<i64, u32>,
    f64_idx: HashMap<u64 /*bits*/, u32>,
    /// Owns the string-table contents — the writer treats this as the
    /// canonical source. Codegen writes via `intern_string` which keeps the
    /// `string_table` and pool indices in sync.
    string_table: Vec<String>,
}

impl ConstPool {
    fn new(initial_strings: Vec<String>) -> Self {
        let mut me = Self::default();
        for s in initial_strings {
            me.intern_string_raw(&s);
        }
        me
    }

    fn intern_string_raw(&mut self, s: &str) -> u32 {
        if let Some(idx) = self.str_idx.get(s) {
            return *idx;
        }
        let idx = self.string_table.len() as u32;
        self.string_table.push(s.to_string());
        self.str_idx.insert(s.to_string(), idx);
        idx
    }
}

impl ConstSink for ConstPool {
    fn intern_string(&mut self, s: &str) -> u32 {
        // Pool entry: tag=String + payload=string_table_idx.
        let str_idx = self.intern_string_raw(s);
        let pool_idx = self.entries.len() as u32;
        self.entries.push(IRConst::Str(s.to_string()));
        let _ = str_idx;
        pool_idx
    }
    fn intern_i64(&mut self, v: i64) -> u32 {
        if let Some(idx) = self.i64_idx.get(&v) {
            return *idx;
        }
        let idx = self.entries.len() as u32;
        self.entries.push(IRConst::I64(v));
        self.i64_idx.insert(v, idx);
        idx
    }
    fn intern_f64(&mut self, v: f64) -> u32 {
        let bits = v.to_bits();
        if let Some(idx) = self.f64_idx.get(&bits) {
            return *idx;
        }
        let idx = self.entries.len() as u32;
        self.entries.push(IRConst::F64(v));
        self.f64_idx.insert(bits, idx);
        idx
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Writer
// ─────────────────────────────────────────────────────────────────────────

pub struct BytecodeWriter {
    buf: Vec<u8>,
    flags: u32,
}

impl BytecodeWriter {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(HEADER_SIZE),
            flags: 0,
        }
    }

    /// Serialize `ir` into a full `.spyc` byte buffer.
    pub fn write_module(&mut self, ir: &IRModule) -> Vec<u8> {
        // 1. Write placeholder header so subsequent section emitters can
        //    record their real offsets.
        self.write_header(0, 0, 0, 0, 0);
        debug_assert_eq!(self.buf.len(), HEADER_SIZE);

        // 2. Initialise const pool with the strings the IR lowerer collected
        //    (so the pool index space and string-table index space are
        //    already populated for things the lowerer saw).
        let mut pool = ConstPool::new(ir.string_table.clone());

        // 3. Emit each function's code first into a side buffer so we know
        //    each function's code_length up front. This lets us populate the
        //    function table without seeks. While codegen runs it interns
        //    strings/constants into `pool`.
        let mut emitted: Vec<(u32 /*offset*/, EmittedFunction)> = Vec::with_capacity(ir.functions.len());
        let mut code_section: Vec<u8> = Vec::new();
        for f in &ir.functions {
            let ef = codegen::emit_function(f, &mut pool);
            let offset = code_section.len() as u32;
            code_section.extend_from_slice(&ef.code);
            emitted.push((offset, ef));
        }

        // 4. Constant pool.
        let const_pool_offset = self.buf.len() as u32;
        self.write_const_pool(&pool);

        // 5. Type table.
        let type_table_offset = self.buf.len() as u32;
        self.write_type_table(ir);

        // 6. Function table.
        let function_table_offset = self.buf.len() as u32;
        self.write_function_table(ir, &emitted);

        // 7. Code section.
        let code_offset = self.buf.len() as u32;
        self.buf.extend_from_slice(&code_section);

        // 8. String table.
        let string_table_offset = self.buf.len() as u32;
        self.write_string_table(&pool.string_table);

        // 9. Patch real offsets into the header.
        self.patch_header_offsets(
            const_pool_offset,
            type_table_offset,
            function_table_offset,
            code_offset,
            string_table_offset,
        );

        std::mem::take(&mut self.buf)
    }

    // ─────────────────────────────────────────────────────────────────
    //  Header
    // ─────────────────────────────────────────────────────────────────

    /// Emit the 32-byte header per spec §12.4 (with placeholder offsets).
    fn write_header(
        &mut self,
        const_pool_offset: u32,
        type_table_offset: u32,
        function_table_offset: u32,
        code_offset: u32,
        string_table_offset: u32,
    ) {
        let start = self.buf.len();

        self.buf.extend_from_slice(&MAGIC);
        self.buf.write_u16::<LittleEndian>(VERSION_MAJOR).unwrap();
        self.buf.write_u16::<LittleEndian>(VERSION_MINOR).unwrap();
        self.buf.write_u32::<LittleEndian>(self.flags).unwrap();
        self.buf.write_u32::<LittleEndian>(const_pool_offset).unwrap();
        self.buf.write_u32::<LittleEndian>(type_table_offset).unwrap();
        self.buf.write_u32::<LittleEndian>(function_table_offset).unwrap();
        self.buf.write_u32::<LittleEndian>(code_offset).unwrap();
        self.buf.write_u32::<LittleEndian>(string_table_offset).unwrap();

        debug_assert_eq!(self.buf.len() - start, HEADER_SIZE);
    }

    /// Patch the four section offsets after the sections have been laid down.
    fn patch_header_offsets(
        &mut self,
        const_pool_offset: u32,
        type_table_offset: u32,
        function_table_offset: u32,
        code_offset: u32,
        string_table_offset: u32,
    ) {
        // Header layout (§12.4): magic 0..4, ver 4..8, flags 8..12,
        // const 12..16, type 16..20, func 20..24, code 24..28, str 28..32.
        LittleEndian::write_u32(&mut self.buf[12..16], const_pool_offset);
        LittleEndian::write_u32(&mut self.buf[16..20], type_table_offset);
        LittleEndian::write_u32(&mut self.buf[20..24], function_table_offset);
        LittleEndian::write_u32(&mut self.buf[24..28], code_offset);
        LittleEndian::write_u32(&mut self.buf[28..32], string_table_offset);
    }

    // ─────────────────────────────────────────────────────────────────
    //  Constant pool (spec §12.5)
    // ─────────────────────────────────────────────────────────────────

    fn write_const_pool(&mut self, pool: &ConstPool) {
        // Per spec the section is just a concatenation of [tag][payload]
        // entries. Readers know how many to consume from the type-table /
        // function-table indices. We prefix with a u32 entry count to make
        // the section self-describing (this is permitted by §12.5 since the
        // header offset alone is enough; downstream parsers can choose to
        // read or ignore it). M4 loader can be taught to skip it.
        self.buf.write_u32::<LittleEndian>(pool.entries.len() as u32).unwrap();
        for c in &pool.entries {
            match c {
                IRConst::I32(v) => {
                    self.buf.push(ConstTag::I32 as u8);
                    self.buf.write_i32::<LittleEndian>(*v).unwrap();
                }
                IRConst::I64(v) => {
                    self.buf.push(ConstTag::I64 as u8);
                    self.buf.write_i64::<LittleEndian>(*v).unwrap();
                }
                IRConst::U32(v) => {
                    self.buf.push(ConstTag::U32 as u8);
                    self.buf.write_u32::<LittleEndian>(*v).unwrap();
                }
                IRConst::U64(v) => {
                    self.buf.push(ConstTag::U64 as u8);
                    self.buf.write_u64::<LittleEndian>(*v).unwrap();
                }
                IRConst::F32(v) => {
                    self.buf.push(ConstTag::F32 as u8);
                    self.buf.write_f32::<LittleEndian>(*v).unwrap();
                }
                IRConst::F64(v) => {
                    self.buf.push(ConstTag::F64 as u8);
                    self.buf.write_f64::<LittleEndian>(*v).unwrap();
                }
                IRConst::Str(s) => {
                    // String entries reference the string table by index.
                    let idx = pool
                        .str_idx
                        .get(s)
                        .copied()
                        .unwrap_or(u32::MAX);
                    self.buf.push(ConstTag::String as u8);
                    self.buf.write_u32::<LittleEndian>(idx).unwrap();
                }
                IRConst::Bool(b) => {
                    // Booleans don't have a dedicated tag; encode as i32.
                    self.buf.push(ConstTag::I32 as u8);
                    self.buf.write_i32::<LittleEndian>(*b as i32).unwrap();
                }
                IRConst::Char(c) => {
                    self.buf.push(ConstTag::Char as u8);
                    self.buf.write_u32::<LittleEndian>(*c as u32).unwrap();
                }
                IRConst::None => {
                    // No dedicated tag; encode as i32(0).
                    self.buf.push(ConstTag::I32 as u8);
                    self.buf.write_i32::<LittleEndian>(0).unwrap();
                }
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────
    //  Type table (spec §12.6)
    // ─────────────────────────────────────────────────────────────────

    fn write_type_table(&mut self, ir: &IRModule) {
        self.buf.write_u32::<LittleEndian>(ir.type_table.len() as u32).unwrap();
        for t in &ir.type_table {
            self.write_type_entry(t);
        }
    }

    fn write_type_entry(&mut self, t: &TypeTableEntry) {
        self.buf.write_u32::<LittleEndian>(t.type_id).unwrap();
        self.buf.push(t.kind);
        self.buf.write_u32::<LittleEndian>(t.name_idx).unwrap();
        self.buf.write_u32::<LittleEndian>(t.size).unwrap();
        self.buf.write_u16::<LittleEndian>(t.fields.len() as u16).unwrap();
        self.buf.write_u16::<LittleEndian>(t.vtable.len() as u16).unwrap();
        self.buf.write_u32::<LittleEndian>(t.base_type).unwrap();
        for f in &t.fields {
            self.write_field_entry(f);
        }
        for m in &t.vtable {
            self.buf.write_u32::<LittleEndian>(*m).unwrap();
        }
        // itable_count = 0 for M3 (no protocol dispatch yet).
        self.buf.write_u16::<LittleEndian>(0).unwrap();
    }

    fn write_field_entry(&mut self, f: &TypeFieldEntry) {
        self.buf.write_u32::<LittleEndian>(f.name_idx).unwrap();
        self.buf.write_u32::<LittleEndian>(f.type_id).unwrap();
        self.buf.write_u32::<LittleEndian>(f.offset).unwrap();
    }

    // ─────────────────────────────────────────────────────────────────
    //  Function table (spec §12.7)
    // ─────────────────────────────────────────────────────────────────

    fn write_function_table(
        &mut self,
        ir: &IRModule,
        emitted: &[(u32, EmittedFunction)],
    ) {
        self.buf.write_u32::<LittleEndian>(ir.function_table.len() as u32).unwrap();
        for (entry, (off, ef)) in ir.function_table.iter().zip(emitted.iter()) {
            self.buf.write_u32::<LittleEndian>(entry.fn_id).unwrap();
            self.buf.write_u32::<LittleEndian>(entry.name_idx).unwrap();
            self.buf.write_u32::<LittleEndian>(entry.type_id).unwrap();
            self.buf.write_u32::<LittleEndian>(*off).unwrap();
            self.buf.write_u32::<LittleEndian>(ef.code.len() as u32).unwrap();
            self.buf.write_u16::<LittleEndian>(entry.num_params).unwrap();
            self.buf.write_u16::<LittleEndian>(entry.num_locals).unwrap();
            self.buf.write_u16::<LittleEndian>(ef.num_registers).unwrap();
            self.buf.write_u16::<LittleEndian>(entry.flags).unwrap();
            // exception_table_offset (u32) — 0 means absent for M3.
            self.buf.write_u32::<LittleEndian>(0).unwrap();
            // debug_info_offset (u32) — 0 means absent.
            self.buf.write_u32::<LittleEndian>(0).unwrap();
        }
    }

    // ─────────────────────────────────────────────────────────────────
    //  String table (spec §12.10)
    // ─────────────────────────────────────────────────────────────────

    fn write_string_table(&mut self, strings: &[String]) {
        // Prefix with u32 count so M4 can read forwards without a header
        // count field. (Strictly speaking the section is a flat sequence of
        // length-prefixed UTF-8 blobs; the count is self-describing extra.)
        self.buf.write_u32::<LittleEndian>(strings.len() as u32).unwrap();
        for s in strings {
            let bytes = s.as_bytes();
            self.buf.write_u32::<LittleEndian>(bytes.len() as u32).unwrap();
            self.buf.extend_from_slice(bytes);
        }
    }
}

impl Default for BytecodeWriter {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::IRModule;

    #[test]
    fn empty_module_has_valid_header() {
        let ir = IRModule::default();
        let bytes = BytecodeWriter::new().write_module(&ir);
        assert!(bytes.len() >= HEADER_SIZE);
        assert_eq!(&bytes[0..4], &MAGIC);
        // Constant pool offset must point past the header.
        let cp = LittleEndian::read_u32(&bytes[12..16]);
        assert!(cp as usize >= HEADER_SIZE);
    }
}
