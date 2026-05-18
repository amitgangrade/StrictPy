//! `spy` — the StrictPy VM command-line driver.
//!
//! Usage: `spy <module.spyc> [args...]`
//!
//! Loads the named compiled module, invokes its `main` function, and exits
//! with the function's return code. Any uncaught exception is printed to
//! stderr and produces exit code 1.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use strictpy_vm::{run_file_with_args, VmError};

#[derive(Debug, Parser)]
#[command(
    name = "spy",
    version,
    about = "StrictPy bytecode VM",
    long_about = "Loads a compiled .spyc module, runs its main function, and exits with its return code."
)]
struct Cli {
    /// Path to a compiled `.spyc` module.
    module: PathBuf,

    /// Arguments forwarded to the program as `sys.argv[1..]`.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // M19: trailing args are now plumbed through to `sys.argv`. The
    // VM's `run_file_with_args` handles `VmError::Exit(code)` → exit
    // code translation internally; only true `UncaughtException` and
    // other VM-level errors reach this arm.
    match run_file_with_args(&cli.module, cli.args) {
        Ok(code) => {
            // Clamp into u8; conventionally Unix-style exit codes are 0..=255.
            ExitCode::from((code & 0xFF) as u8)
        }
        Err(VmError::UncaughtException { type_name, message }) => {
            eprintln!("Uncaught {type_name}: {message}");
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("spy: {e}");
            ExitCode::from(1)
        }
    }
}
