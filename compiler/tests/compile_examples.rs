//! Integration test: every `examples/*.spy` must compile end-to-end through
//! `compile_source`, producing a byte buffer that starts with `MAGIC` and is
//! at least `HEADER_SIZE` bytes long. We then hand the bytes to the VM
//! loader and verify it parses every section.
//!
//! (Before M4 the loader's per-section parsers were `todo!()` stubs and this
//! test asserted that `load` panicked. After M4 the loader is fully
//! implemented, so the assertion was inverted: `load` must succeed.)

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_shared::file_format::{HEADER_SIZE, MAGIC};
use strictpy_vm::loader;

fn examples_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.push("examples");
    p
}

#[test]
fn compile_all_examples_produces_valid_header() {
    let dir = examples_dir();
    let mut any = false;
    for entry in fs::read_dir(&dir).expect("read examples/") {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("spy") {
            continue;
        }
        any = true;

        let src = fs::read_to_string(&path).unwrap();
        let bytes = compile_source(path.display().to_string(), &src)
            .unwrap_or_else(|e| panic!("compile error in {:?}: {}", path, e));

        assert!(
            bytes.len() >= HEADER_SIZE,
            "{:?}: output {} bytes is shorter than the {}-byte header",
            path,
            bytes.len(),
            HEADER_SIZE
        );
        assert_eq!(&bytes[0..4], &MAGIC, "{:?}: bad magic", path);

        let header = loader::parse_header(&bytes)
            .unwrap_or_else(|e| panic!("{:?}: header parser rejected our output: {}", path, e));
        assert!((header.const_pool_offset as usize) >= HEADER_SIZE);
        assert!((header.const_pool_offset as usize) <= bytes.len());
        assert!((header.type_table_offset as usize) <= bytes.len());
        assert!((header.function_table_offset as usize) <= bytes.len());
        assert!((header.code_offset as usize) <= bytes.len());
        assert!((header.string_table_offset as usize) <= bytes.len());

        // Full M4 loader must succeed on every example.
        let m = loader::load(&bytes)
            .unwrap_or_else(|e| panic!("{:?}: loader rejected our output: {}", path, e));
        assert!(
            !m.functions.is_empty(),
            "{:?}: module has no functions",
            path
        );
    }
    assert!(any, "no .spy examples found under {:?}", dir);
}
