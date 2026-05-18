//! Torture tests for BUG-026 / BUG-027 (non-deterministic heap corruption +
//! position-sensitive crash). These bugs were *provisionally* closed in M11
//! after the BUG-016 (subclass-field-aliasing) fix landed: 5×5 manual runs
//! of calculator + json_parse came back clean where pre-M11 they were
//! 0-of-3. This file upgrades that to "confirmed fixed" by running each
//! canonical repro program 50-100 times in a single `cargo test` invocation
//! and asserting the success rate stays above ~95%.
//!
//! Structure (mirrors `calculator_runs.rs` / `json_parse_runs.rs` /
//! `lisp_runs.rs`):
//!
//!   1. Compile the .spy source ONCE per test, write a single .spyc to the
//!      cargo target tmpdir.
//!   2. Spawn `target/release/spy[.exe] <spyc>` N times sequentially.
//!   3. For each invocation, capture exit code + stdout. A run is "clean"
//!      iff exit 0 AND stdout contains every expected substring.
//!   4. Assert at least M of N runs are clean (5% slack for Windows
//!      process-spawn flakiness; if BUG-026 were back, the failure rate
//!      would be vastly higher).
//!
//! The existing per-program tests (calculator_runs / json_parse_runs /
//! lisp_runs) still live alongside — those have looser assertions written
//! when BUG-026 was open. We don't remove them; we extend them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

fn spy_binary() -> PathBuf {
    let name = if cfg!(windows) { "spy.exe" } else { "spy" };
    project_root().join("target").join("release").join(name)
}

/// Compile `examples/<src_name>` once and return the path to the .spyc.
fn compile_once(src_name: &str, spyc_name: &str) -> PathBuf {
    let src_path = project_root().join("examples").join(src_name);
    let src = fs::read_to_string(&src_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", src_path.display()));
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile {src_name}: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(spyc_name);
    fs::write(&spyc_path, &bytes)
        .unwrap_or_else(|e| panic!("write {}: {e}", spyc_path.display()));
    spyc_path
}

/// Record of a single invocation.
struct RunResult {
    index: usize,
    exit_ok: bool,
    exit_code: Option<i32>,
    stdout: String,
    missing: Vec<&'static str>,
}

impl RunResult {
    fn is_clean(&self) -> bool {
        self.exit_ok && self.missing.is_empty()
    }
}

/// Run `spy <spyc>` once, check exit + that every `expected` substring
/// appears in stdout.
fn run_once(
    spy_bin: &Path,
    spyc_path: &Path,
    expected: &[&'static str],
    index: usize,
) -> RunResult {
    let output = Command::new(spy_bin)
        .arg(spyc_path)
        .output()
        .expect("invoke spy binary");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let exit_ok = output.status.success();
    let exit_code = output.status.code();
    let missing: Vec<&'static str> = expected
        .iter()
        .copied()
        .filter(|needle| !stdout.contains(*needle))
        .collect();
    RunResult { index, exit_ok, exit_code, stdout, missing }
}

/// Run the same .spyc `n` times, collect results, and assert that at least
/// `min_clean` of `n` invocations are fully clean. On failure, log the
/// first 3 failed invocations (index / exit code / first 200 chars of
/// stdout) and panic with a summary.
fn torture(
    label: &str,
    spyc_path: &Path,
    expected: &[&'static str],
    n: usize,
    min_clean: usize,
) {
    let spy_bin = spy_binary();
    if !spy_bin.exists() {
        eprintln!(
            "skipping {label}: {} not present (run `cargo build --release` first)",
            spy_bin.display()
        );
        return;
    }

    let start = Instant::now();
    let mut clean = 0usize;
    let mut failures: Vec<RunResult> = Vec::new();
    for i in 0..n {
        let r = run_once(&spy_bin, spyc_path, expected, i);
        if r.is_clean() {
            clean += 1;
        } else if failures.len() < 3 {
            failures.push(r);
        }
    }
    let elapsed = start.elapsed();

    eprintln!(
        "[{label}] {clean}/{n} clean runs, threshold {min_clean}/{n}, elapsed {:.2}s",
        elapsed.as_secs_f64()
    );

    if !failures.is_empty() {
        eprintln!("[{label}] first {} failure(s):", failures.len());
        for f in &failures {
            let head: String = f.stdout.chars().take(200).collect();
            eprintln!(
                "  attempt #{}: exit_ok={} exit_code={:?} missing={:?} stdout[0..200]={:?}",
                f.index, f.exit_ok, f.exit_code, f.missing, head
            );
        }
    }

    assert!(
        clean >= min_clean,
        "{label}: only {clean}/{n} clean runs (need {min_clean}). \
         BUG-026 / BUG-027 may have regressed — see captured failures above.",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Per-program torture tests
//
// Each test compiles its example ONCE and reuses the .spyc across all
// iterations. We're testing whether a single process is now reliable, not
// throughput, so iterations are strictly sequential.
// ─────────────────────────────────────────────────────────────────────────────

/// `examples/calculator.spy` — recursive-descent calculator, the canonical
/// BUG-026 repro. main() drives 7 `show()` calls; expected outputs derived
/// from the source (see file header comment):
///   show("1 + 2")                  -> "1 + 2 = 3.0"
///   show("3 * 4 + 5")              -> "3 * 4 + 5 = 17.0"
///   show("(1 + 2) * 3")            -> "(1 + 2) * 3 = 9.0"
///   show("-5 + 10")                -> "-5 + 10 = 5.0"
///   show("2 * (3 + 4) - 8 / 2")    -> "2 * (3 + 4) - 8 / 2 = 10.0"
///   show("10 / 0")                 -> "10 / 0 = 0.0"   (sentinel, see source)
///   show("3.5 * 2")                -> "3.5 * 2 = 7.0"
#[test]
fn calculator_torture_100_runs() {
    let spyc = compile_once("calculator.spy", "calculator_torture.spyc");
    let expected: &[&str] = &[
        "1 + 2 = 3.0",
        "3 * 4 + 5 = 17.0",
        "(1 + 2) * 3 = 9.0",
        "-5 + 10 = 5.0",
        "2 * (3 + 4) - 8 / 2 = 10.0",
        "10 / 0 = 0.0",
        "3.5 * 2 = 7.0",
    ];
    torture("calculator", &spyc, expected, 100, 95);
}

/// `examples/json_parse.spy` — recursive-descent JSON parser. main() does
/// 3 atom round-trips: `null`, `true`, `false`. The sibling test
/// `json_parse_runs.rs::json_parse_atoms_round_trip` already asserts these
/// substrings appear; we ramp it to 100 sequential invocations.
#[test]
fn json_parse_torture_100_runs() {
    let spyc = compile_once("json_parse.spy", "json_parse_torture.spyc");
    let expected: &[&str] = &["null", "true", "false"];
    torture("json_parse", &spyc, expected, 100, 95);
}

/// `examples/lisp.spy` — toy Lisp interpreter, the most class-heavy of the
/// M11 stress programs. The embedded program prints `30` (from `(+ x y)`)
/// and `49` (from `(square 7)`). 50 iterations (not 100) because lisp is
/// the most expensive program — see `lisp_runs.rs`.
#[test]
fn lisp_torture_50_runs() {
    let spyc = compile_once("lisp.spy", "lisp_torture.spyc");
    let expected: &[&str] = &["30", "49"];
    torture("lisp", &spyc, expected, 50, 48);
}
