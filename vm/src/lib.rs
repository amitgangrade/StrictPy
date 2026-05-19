//! StrictPy virtual machine.
//!
//! This crate implements the runtime side of StrictPy. It is composed of
//! four cooperating subsystems:
//!
//! 1. **Loader** ([`loader`]) — reads a `.spyc` file, verifies its magic
//!    and version, and parses the constant pool, type table, function
//!    table, code section, and string table into in-memory structures.
//! 2. **Interpreter** ([`interp`]) — a register-based bytecode interpreter
//!    that drives a call-frame stack. See spec §14.
//! 3. **Object model** ([`object`]) — the runtime layout of every heap
//!    value: object headers, vtables, lists, strings, closures. See
//!    spec §8.
//! 4. **Garbage collector** ([`gc`]) — a stop-the-world mark-and-sweep
//!    collector in M4; generational + write barriers land in M6.
//!
//! Supporting modules: [`builtins`] dispatches `CALL_NATIVE`, [`ffi`] is the
//! placeholder for `@extern("c", ...)`, and [`error`] hosts the central
//! [`VmError`] type.
//!
//! The public entry point is [`run_file`], which loads a module, invokes
//! its `main` function, and returns the process exit code.

#![allow(dead_code)]

pub mod builtins;
pub mod decompile;
pub mod error;
pub mod ffi;
pub mod gc;
pub mod interp;
#[cfg(feature = "jit")]
pub mod jit;
#[cfg(feature = "jit")]
pub mod jit_runtime;
pub mod loader;
pub mod object;

use std::path::Path;

pub use error::VmError;

/// Load `.spyc` bytecode from `path`, link it, execute its `main` function,
/// and return the integer exit code.
pub fn run_file(path: &Path) -> Result<i32, VmError> {
    run_file_with_args(path, Vec::new())
}

/// Like [`run_file`] but plumbs `args` through to `sys.argv` (M19).
/// `argv[0]` is conventionally the program path (matching Python); the
/// CLI driver constructs it from the `.spyc` path. `args` is everything
/// after that — the trailing args the user typed on the command line.
pub fn run_file_with_args(path: &Path, args: Vec<String>) -> Result<i32, VmError> {
    let bytes = std::fs::read(path).map_err(VmError::Io)?;
    run_bytes_with_args(&bytes, path.to_string_lossy().into_owned(), args)
}

/// Like [`run_file_with_args`] but takes an already-loaded `.spyc`
/// byte buffer.  Used by the M25 unified `spy` CLI when compiling
/// `.spy` sources in memory (the `-c` inline-execute path skips
/// touching the filesystem altogether).
///
/// `argv0` is the program path placed at `sys.argv[0]`; for inline
/// `-c` mode this is conventionally the literal string `"-c"`,
/// matching CPython.
pub fn run_bytes_with_args(
    bytes: &[u8],
    argv0: String,
    args: Vec<String>,
) -> Result<i32, VmError> {
    let module = loader::load(bytes)?;
    let mut interp = interp::Interpreter::new(module);
    builtins::register(&mut interp);
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push(argv0);
    argv.extend(args);
    interp.set_argv(argv);
    match interp.run_main() {
        Ok(code) => Ok(code),
        Err(VmError::Exit(code)) => Ok(code),
        Err(e) => Err(e),
    }
}

/// Like [`run_file`] but writes program stdout into `sink` instead of the
/// process's real stdout. Used by integration tests to capture output.
pub fn run_file_capture(path: &Path) -> Result<(i32, String), VmError> {
    run_file_capture_with_args(path, Vec::new())
}

/// Like [`run_file_capture`] but also plumbs `args` through to `sys.argv`.
/// M19: lets in-process tests verify `import sys; sys.exit(N)` without
/// spawning a subprocess. The Exit code is translated to the integer
/// part of the tuple the same way `run_file_with_args` does.
pub fn run_file_capture_with_args(
    path: &Path,
    args: Vec<String>,
) -> Result<(i32, String), VmError> {
    let bytes = std::fs::read(path).map_err(VmError::Io)?;
    let module = loader::load(&bytes)?;
    let mut interp = interp::Interpreter::new(module);
    builtins::register(&mut interp);
    let buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let sink = CaptureStdout { buf: buf.clone() };
    interp.set_stdout(Box::new(sink));
    let mut argv = Vec::with_capacity(args.len() + 1);
    argv.push(path.to_string_lossy().into_owned());
    argv.extend(args);
    interp.set_argv(argv);
    let code = match interp.run_main() {
        Ok(c) => c,
        Err(VmError::Exit(c)) => c,
        Err(e) => return Err(e),
    };
    let out = buf.lock().unwrap().clone();
    Ok((code, out))
}

struct CaptureStdout {
    buf: std::sync::Arc<std::sync::Mutex<String>>,
}
impl interp::Stdout for CaptureStdout {
    fn write_str(&mut self, s: &str) {
        self.buf.lock().unwrap().push_str(s);
    }
}
