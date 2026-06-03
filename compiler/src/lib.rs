//! StrictPy compiler library.
//!
//! Orchestrates the pipeline described in `STRICTPY_SPEC.md` §10.1:
//!
//! ```text
//!   source → lex → parse → resolve → typecheck → lower → optimize → emit → write
//! ```
//!
//! This crate is intentional scaffolding: most stages are stubbed with
//! `todo!("M<N>: ...")` markers tied to the milestones in spec §19.

#![allow(dead_code)]

use std::fs;
use std::path::Path;

pub mod ast;
pub mod bytecode;
pub mod codegen;
pub mod error;
pub mod ir;
pub mod lexer;
pub mod modules;
pub mod optimize;
pub mod opts;
pub mod parser;
pub mod pretty;
pub mod resolver;
pub mod typecheck;
pub mod types;

pub use ast::Module;
pub use error::CompileError;
pub use ir::IRModule;
pub use typecheck::TypedModule;

/// Compile a single `.spy` source file at `input` and write a `.spyc` bytecode
/// module to `output`.
///
/// This is the canonical entry point used by the `spyc` binary and by
/// integration tests. The pipeline mirrors spec §10.1.
pub fn compile_file(input: &Path, output: &Path) -> Result<(), CompileError> {
    let source = fs::read_to_string(input).map_err(CompileError::Io)?;
    let bytes = compile_source(input.display().to_string(), &source)?;
    fs::write(output, bytes).map_err(CompileError::Io)?;
    Ok(())
}

/// Compile in-memory source to a `.spyc` byte buffer. Useful for tests that
/// want to avoid touching the filesystem.
///
/// `file_name` is the on-disk path of `source` (or a synthetic name). It is
/// used as the root for **relative user-module import resolution** (M60):
/// `import foo` resolves to `<dir-of-file_name>/foo.spy`, `import pkg.mod`
/// to `<dir>/pkg/mod.spy`. Imported sibling modules are parsed, namespaced,
/// and merged into a single compilation unit before resolution. If a path
/// has no on-disk directory (inline `-c` code, bare snippet names), no user
/// modules are reachable and behavior is identical to a single-file compile.
pub fn compile_source(file_name: String, source: &str) -> Result<Vec<u8>, CompileError> {
    // ── M60: parse + resolve user-module imports into one merged module ───
    let module: Module = modules::resolve_and_merge(&file_name, source)?;

    // ── M2: Resolver ─────────────────────────────────────────────────────
    let resolved = resolver::Resolver::new().resolve(module)?;

    // ── M2: Type checker ─────────────────────────────────────────────────
    let typed = typecheck::TypeChecker::new().check(resolved)?;

    // ── M3: IR lowering ──────────────────────────────────────────────────
    let ir = ir::lower(typed);

    // ── M6: Optimizer ────────────────────────────────────────────────────
    let ir = optimize::run(ir);

    // ── M3: Codegen + bytecode writer ────────────────────────────────────
    let mut writer = bytecode::BytecodeWriter::new();
    let bytes = writer.write_module(&ir);

    Ok(bytes)
}
