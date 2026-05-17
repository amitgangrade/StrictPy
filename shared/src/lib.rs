//! Shared definitions for the StrictPy compiler and VM.
//!
//! Anything that must agree byte-for-byte between the two — opcode numbers,
//! the `.spyc` file layout, type tags — lives here. See `STRICTPY_SPEC.md`
//! sections 12 and 13 for the canonical specification.

pub mod opcode;
pub mod file_format;
pub mod type_tag;
pub mod native;

pub use opcode::Opcode;
pub use type_tag::TypeTag;
pub use native::NativeFn;
