//! VM error type. All fallible VM operations surface as one of these variants.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VmError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("bad magic: file is not a .spyc module")]
    BadMagic,

    #[error(
        "unsupported .spyc version: found {}.{}, expected {}.{}",
        found.0, found.1, expected.0, expected.1
    )]
    UnsupportedVersion {
        found: (u16, u16),
        expected: (u16, u16),
    },

    #[error("corrupt .spyc: {0}")]
    Corrupt(String),

    #[error("unknown opcode byte: 0x{0:02X}")]
    BadOpcode(u8),

    #[error("link error: {0}")]
    LinkError(String),

    #[error("uncaught exception: {type_name}: {message}")]
    UncaughtException { type_name: String, message: String },

    #[error("VM trap: {0}")]
    Trap(String),

    /// M19: `sys.exit(code)` — non-catchable program termination.
    ///
    /// The interpreter's `propagate_exception` loop only catches
    /// `UncaughtException`; an `Exit` walks straight up the call stack
    /// past every active try/except and is translated to a process
    /// exit code by the CLI driver. Mirrors Python's `SystemExit`
    /// (a `BaseException`, not an `Exception`).
    #[error("program called sys.exit({0})")]
    Exit(i32),
}
