//! Subprocess integration test for `examples/fizzbuzz_v2.spy` (M20b).
//!
//! Compiles + runs the FizzBuzz-with-random program through `spy.exe`
//! and asserts a Fizz/Buzz/FizzBuzz/number on each output line.  The
//! LCG seed is pinned in-source (42), so the output is deterministic
//! across hosts.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn fizzbuzz_v2_compiles() {
    let src_path = project_root().join("examples").join("fizzbuzz_v2.spy");
    let src = fs::read_to_string(&src_path).expect("read fizzbuzz_v2.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile fizzbuzz_v2.spy: {e}"));
}

#[test]
fn fizzbuzz_v2_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("fizzbuzz_v2.spy");
    let src = fs::read_to_string(&src_path).expect("read fizzbuzz_v2.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile fizzbuzz_v2.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fizzbuzz_v2.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .current_dir(project_root())
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "spy.exe failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );

    // 20 output lines (one per iteration).
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 20, "expected 20 lines; stdout:\n{stdout}");

    // Each line is "Fizz", "Buzz", "FizzBuzz", or a stringified integer
    // 1..=100.  Validate that classification.
    for (i, line) in lines.iter().enumerate() {
        match *line {
            "Fizz" | "Buzz" | "FizzBuzz" => {}
            other => {
                let n: i64 = other.parse().unwrap_or_else(|_| {
                    panic!("line {i}: not Fizz/Buzz/FizzBuzz nor an integer: {other:?}")
                });
                assert!(
                    (1..=100).contains(&n),
                    "line {i}: integer out of range [1,100]: {n}"
                );
                // And the classification rule must hold: not divisible
                // by 3 or 5 (else we'd have printed Fizz/Buzz/FizzBuzz).
                assert!(
                    n % 3 != 0 && n % 5 != 0,
                    "line {i}: integer {n} should have been classified as Fizz/Buzz/FizzBuzz"
                );
            }
        }
    }
}
