//! Integration test: build a real 32-byte header, hand it to the loader,
//! and confirm the header parser accepts magic + version.
//!
//! After M4 the loader can also consume an empty-ish trailer; the file we
//! build here has all-zero offsets, so each section parser will see a
//! zero `*_offset` and return an empty section. The test asserts that
//! `load` succeeds with zero functions/constants/etc.

use strictpy_shared::file_format::{HEADER_SIZE, MAGIC, VERSION_MAJOR, VERSION_MINOR};

use strictpy_vm::loader;

fn build_minimal_header() -> Vec<u8> {
    let mut buf = vec![0u8; HEADER_SIZE];
    buf[0..4].copy_from_slice(&MAGIC);
    buf[4..6].copy_from_slice(&VERSION_MAJOR.to_le_bytes());
    buf[6..8].copy_from_slice(&VERSION_MINOR.to_le_bytes());
    // flags + every offset are zero — already initialised by `vec![0u8; ...]`.
    buf
}

#[test]
fn header_parser_accepts_minimal_header() {
    let bytes = build_minimal_header();
    let header = loader::parse_header(&bytes).expect("header parser must accept the bytes");
    assert_eq!(header.version_major, VERSION_MAJOR);
    assert_eq!(header.version_minor, VERSION_MINOR);
}

#[test]
fn load_returns_empty_module_for_zero_offsets() {
    let bytes = build_minimal_header();
    let m = loader::load(&bytes).expect("load should succeed for zero-offset header");
    assert!(m.functions.is_empty());
    assert!(m.constants.is_empty());
    assert!(m.strings.is_empty());
    assert!(m.types.is_empty());
}
