//! `spyc` — the StrictPy compiler CLI driver.
//!
//! Usage: `spyc <input.spy> [-o output.spyc]`.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

/// StrictPy compiler: compile a `.spy` source file to a `.spyc` bytecode module.
#[derive(Debug, Parser)]
#[command(name = "spyc", version, about, long_about = None)]
struct Cli {
    /// Input `.spy` source file.
    input: PathBuf,

    /// Output `.spyc` file. Defaults to `<input>.spyc`.
    #[arg(short = 'o', long = "output")]
    output: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let output = cli.output.unwrap_or_else(|| cli.input.with_extension("spyc"));

    strictpy_compiler::compile_file(&cli.input, &output)
        .with_context(|| format!("failed to compile {}", cli.input.display()))?;

    Ok(())
}
