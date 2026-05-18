//! Integration test for `examples/minigrep.spy` — the M21 cross-module
//! demonstration program. Exercises sys + os + io + re + time + try/except
//! + tuples together. Used by the orchestrator as evidence that the
//! M19-M20 stdlib composes ergonomically.
//!
//! Runs the program out-of-process via spy.exe with a pattern + a known
//! file input. Asserts on matching-line output and on the stderr summary.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

fn compile_minigrep() -> PathBuf {
    let src_path = project_root().join("examples").join("minigrep.spy");
    let src = fs::read_to_string(&src_path).expect("read minigrep.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile minigrep.spy: {e}"));
    let spyc = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("minigrep.spyc");
    fs::write(&spyc, &bytes).expect("write spyc");
    spyc
}

fn spy_bin() -> Option<PathBuf> {
    let p = project_root().join("target").join("release");
    let candidate = if cfg!(windows) {
        p.join("spy.exe")
    } else {
        p.join("spy")
    };
    if candidate.exists() {
        Some(candidate)
    } else {
        None
    }
}

#[test]
fn minigrep_compiles() {
    let _ = compile_minigrep();
}

#[test]
fn minigrep_reads_file_and_finds_pattern() {
    let spy = match spy_bin() {
        Some(p) => p,
        None => {
            eprintln!("skipping: spy binary not built");
            return;
        }
    };
    let spyc = compile_minigrep();

    // Write a small input file with a known set of lines.
    let input_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("minigrep_input.txt");
    let input_text = "alpha line\nbeta 42 line\ngamma\ndelta 99 line\nepsilon\n";
    fs::write(&input_path, input_text).expect("write input file");

    // Search for lines containing a digit.
    let output = Command::new(&spy)
        .arg(&spyc)
        .arg(r"[0-9]+")
        .arg(input_path.to_string_lossy().to_string())
        .output()
        .expect("invoke spy");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    eprintln!("STDOUT:\n{stdout}\nSTDERR:\n{stderr}");
    assert!(output.status.success(), "non-zero exit: {:?}", output.status);

    // Two lines (beta + delta) contain digits.
    assert!(
        stdout.contains("beta 42 line"),
        "missing beta line in stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("delta 99 line"),
        "missing delta line in stdout:\n{stdout}"
    );
    // Non-matching lines must NOT appear.
    assert!(
        !stdout.contains("alpha line"),
        "alpha line should NOT match:\n{stdout}"
    );

    // Summary on stderr.
    assert!(
        stderr.contains("2/5 matched"),
        "expected '2/5 matched' on stderr:\n{stderr}"
    );
}

#[test]
fn minigrep_handles_missing_file_via_try_except() {
    let spy = match spy_bin() {
        Some(p) => p,
        None => {
            eprintln!("skipping: spy binary not built");
            return;
        }
    };
    let spyc = compile_minigrep();

    let output = Command::new(&spy)
        .arg(&spyc)
        .arg(r"x")
        .arg("definitely_does_not_exist_minigrep.txt")
        .output()
        .expect("invoke spy");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    eprintln!("STDERR:\n{stderr}");

    assert_eq!(
        output.status.code(),
        Some(2),
        "expected exit code 2 for missing-file path"
    );
    assert!(
        stderr.contains("minigrep: no such file"),
        "expected 'no such file' message:\n{stderr}"
    );
}

#[test]
fn minigrep_rejects_bad_pattern() {
    let spy = match spy_bin() {
        Some(p) => p,
        None => {
            eprintln!("skipping: spy binary not built");
            return;
        }
    };
    let spyc = compile_minigrep();

    let output = Command::new(&spy)
        .arg(&spyc)
        .arg(r"[unterminated")
        .output()
        .expect("invoke spy");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    eprintln!("STDERR:\n{stderr}");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr.contains("bad regex"),
        "expected 'bad regex' message:\n{stderr}"
    );
}

#[test]
fn minigrep_usage_when_no_args() {
    let spy = match spy_bin() {
        Some(p) => p,
        None => {
            eprintln!("skipping: spy binary not built");
            return;
        }
    };
    let spyc = compile_minigrep();

    let mut child = Command::new(&spy)
        .arg(&spyc)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("invoke spy");
    // No stdin piped; the program should die at usage check before reading.
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("wait");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr.contains("usage"), "expected usage line:\n{stderr}");
}
