//! `spy` — the StrictPy unified CLI (M25).
//!
//! Modelled after CPython's `python` command:
//!
//! ```text
//!   spy SCRIPT [args...]              # Compile-if-stale + run.
//!                                     # SCRIPT may be .spy or .spyc.
//!   spy -c CODE [args...]             # Compile inline + run.
//!   spy --compile-only SCRIPT [-o OUT]
//!                                     # Compile only; do not run.
//!                                     # Default OUT is
//!                                     # `<dir>/__spycache__/<basename>.spyc`.
//! ```
//!
//! Cache rules (matching CPython's `__pycache__`):
//!
//! - When a `.spy` source is run, the cached `.spyc` lives at
//!   `<dir-of-source>/__spycache__/<basename>.spyc`.
//! - If the cache is missing OR the source's mtime is strictly
//!   greater than the cache's mtime, the source is recompiled and
//!   the cache is rewritten.
//! - If the cache is fresh, the source is NOT re-parsed; the cached
//!   bytecode is loaded directly.
//!
//! `-c` runs are never cached.
//!
//! When a `.spyc` is passed directly, the CLI runs it without
//! consulting any cache (the file IS the cache).
//!
//! `sys.argv[0]` is set to the script path the user typed (or
//! `"-c"` for inline mode); trailing tokens after the script are
//! `sys.argv[1..]`.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;

use strictpy_vm::{run_bytes_with_args, run_file_with_args, VmError};

// The `spy` runtime is allocation-heavy: every `str(int)`, string concat,
// list/dict entry, and closure is a separate small heap object, and JIT'd
// programs allocate without ever triggering the (interpreter-driven) GC, so
// freelist quality dominates. The platform default allocator (notably the
// Windows process heap) is slow under that churn; mimalloc is markedly
// faster for many small short-lived allocations and works on MSVC (unlike
// jemalloc). Scoped to the `spy` binary only — the library and its test
// harness keep the system allocator.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Debug, Parser)]
#[command(
    name = "spy",
    version,
    about = "StrictPy: compile and run .spy programs",
    long_about = "Compile and run a StrictPy program.\n\
                  \n\
                  Pass a .spy source to compile-if-stale + run; pass a .spyc\n\
                  to run a precompiled module directly; use -c to run an\n\
                  inline string; use --compile-only to emit a .spyc without\n\
                  executing it.\n\
                  \n\
                  Source files compiled via the `SCRIPT` form are cached\n\
                  next to the source at __spycache__/<basename>.spyc and\n\
                  reused when fresh (mtime-based)."
)]
struct Cli {
    /// Source `.spy` file or compiled `.spyc` module.
    ///
    /// When omitted, `-c CODE` must be supplied instead.
    #[arg(
        required_unless_present = "code",
        conflicts_with = "code",
        value_name = "SCRIPT"
    )]
    script: Option<PathBuf>,

    /// Compile and run an inline program string.
    ///
    /// Mutually exclusive with the positional SCRIPT.  Mirrors
    /// `python -c "code"`.  `sys.argv[0]` is set to the literal
    /// `"-c"`; trailing tokens become `sys.argv[1..]`.
    #[arg(short = 'c', long = "command", value_name = "CODE")]
    code: Option<String>,

    /// Compile SCRIPT to bytecode but do not execute it.
    ///
    /// Requires a `.spy` SCRIPT.  Default output path is
    /// `<dir-of-source>/__spycache__/<basename>.spyc`, overridable
    /// with `-o`.
    #[arg(long = "compile-only", conflicts_with = "code")]
    compile_only: bool,

    /// Output path for `--compile-only`.
    ///
    /// Ignored unless `--compile-only` is set.  When omitted, the
    /// `__spycache__` default applies.
    #[arg(short = 'o', long = "output", value_name = "OUTPUT", requires = "compile_only")]
    output: Option<PathBuf>,

    /// Trailing arguments forwarded to the program as `sys.argv[1..]`.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match dispatch(cli) {
        Ok(code) => ExitCode::from((code & 0xFF) as u8),
        Err(e) => {
            eprintln!("spy: {e}");
            ExitCode::from(1)
        }
    }
}

fn dispatch(cli: Cli) -> Result<i32, CliError> {
    // -c "inline code" path.
    if let Some(code) = cli.code {
        let bytes = strictpy_compiler::compile_source("<-c>".to_string(), &code)
            .map_err(|e| CliError::Compile(format!("{e}")))?;
        return run_bytes_with_args(&bytes, "-c".to_string(), cli.args)
            .map_err(CliError::Vm);
    }

    // SCRIPT path (.spy or .spyc, with or without --compile-only).
    let script = cli.script.expect("clap ensures script or -c is present");

    if cli.compile_only {
        let output = match cli.output {
            Some(p) => p,
            None => cached_spyc_path(&script),
        };
        compile_to(&script, &output)?;
        return Ok(0);
    }

    match script_kind(&script) {
        ScriptKind::Source => {
            let cached = cached_spyc_path(&script);
            if needs_recompile(&script, &cached) {
                compile_to(&script, &cached)?;
            }
            // sys.argv[0] is the user-visible source path, NOT the
            // cached .spyc path.  Matches Python (`python script.py`
            // sets argv[0] to "script.py", never the cached .pyc).
            let bytes = fs::read(&cached).map_err(CliError::Io)?;
            run_bytes_with_args(&bytes, script.to_string_lossy().into_owned(), cli.args)
                .map_err(CliError::Vm)
        }
        ScriptKind::Bytecode => run_file_with_args(&script, cli.args).map_err(CliError::Vm),
        ScriptKind::Unknown => Err(CliError::UnknownExt(script)),
    }
}

fn compile_to(source: &Path, output: &Path) -> Result<(), CliError> {
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(CliError::Io)?;
        }
    }
    strictpy_compiler::compile_file(source, output)
        .map_err(|e| CliError::Compile(format!("{e}")))
}

/// `<dir-of-source>/__spycache__/<basename>.spyc`.
fn cached_spyc_path(source: &Path) -> PathBuf {
    let parent = source.parent().filter(|p| !p.as_os_str().is_empty()).map_or_else(
        || PathBuf::from("."),
        PathBuf::from,
    );
    let stem = source.file_stem().unwrap_or_else(|| source.as_os_str());
    let mut filename = OsString::from(stem);
    filename.push(".spyc");
    parent.join("__spycache__").join(filename)
}

fn needs_recompile(source: &Path, cached: &Path) -> bool {
    let cache_meta = match fs::metadata(cached) {
        Ok(m) => m,
        Err(_) => return true,
    };
    let cache_mt = match cache_meta.modified() {
        Ok(t) => t,
        Err(_) => return true,
    };
    // The source itself.
    let src_meta = match fs::metadata(source) {
        Ok(m) => m,
        Err(_) => return true,
    };
    match src_meta.modified() {
        Ok(src_mt) if src_mt > cache_mt => return true,
        Err(_) => return true,
        _ => {}
    }
    // M60: a merged compile inlines sibling user modules, so the cache is
    // also stale if any imported dependency is newer than it.
    if let Ok(src_text) = fs::read_to_string(source) {
        for dep in
            strictpy_compiler::modules::collect_import_deps(&source.to_string_lossy(), &src_text)
        {
            match fs::metadata(&dep).and_then(|m| m.modified()) {
                Ok(dep_mt) if dep_mt > cache_mt => return true,
                Err(_) => return true,
                _ => {}
            }
        }
    }
    false
}

enum ScriptKind {
    Source,
    Bytecode,
    Unknown,
}

fn script_kind(path: &Path) -> ScriptKind {
    match path.extension().and_then(|e| e.to_str()) {
        Some("spy") => ScriptKind::Source,
        Some("spyc") => ScriptKind::Bytecode,
        _ => ScriptKind::Unknown,
    }
}

#[derive(Debug)]
enum CliError {
    Compile(String),
    Vm(VmError),
    Io(std::io::Error),
    UnknownExt(PathBuf),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Compile(msg) => write!(f, "compile error: {msg}"),
            CliError::Vm(VmError::UncaughtException { type_name, message }) => {
                write!(f, "Uncaught {type_name}: {message}")
            }
            CliError::Vm(e) => write!(f, "vm error: {e}"),
            CliError::Io(e) => write!(f, "i/o error: {e}"),
            CliError::UnknownExt(p) => write!(
                f,
                "unrecognised file extension for `{}` — expected .spy or .spyc",
                p.display()
            ),
        }
    }
}
