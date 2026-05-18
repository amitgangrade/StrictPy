//! Subprocess integration test for `examples/math_demo.spy` (M20b).
//!
//! Compiles + runs the math-module demo through `spy.exe` and asserts
//! the printed values match expectation.

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
fn math_demo_compiles() {
    let src_path = project_root().join("examples").join("math_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read math_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile math_demo.spy: {e}"));
}

#[test]
fn math_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("math_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read math_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile math_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("math_demo.spyc");
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

    // Pi banner — accept any f64 string that starts with "3.14159".
    assert!(
        stdout.contains("pi=3.14159"),
        "expected pi banner; stdout:\n{stdout}"
    );
    // tau - 2*pi should be ~0.
    assert!(
        stdout.contains("tau-2pi=0"),
        "expected tau-2pi ~ 0; stdout:\n{stdout}"
    );
    // sqrt(16) = 4.
    assert!(
        stdout.contains("sqrt-16=4"),
        "expected sqrt-16 result; stdout:\n{stdout}"
    );
    // log2(1024) = 10.
    assert!(
        stdout.contains("log2-1024=10"),
        "expected log2-1024 result; stdout:\n{stdout}"
    );
    // gcd(12, 18) = 6.
    assert!(
        stdout.contains("gcd-12-18=6"),
        "expected gcd result; stdout:\n{stdout}"
    );
    // factorial(5) = 120, factorial(10) = 3628800.
    assert!(
        stdout.contains("factorial-5=120"),
        "expected factorial-5; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("factorial-10=3628800"),
        "expected factorial-10; stdout:\n{stdout}"
    );
    // Predicates.
    assert!(
        stdout.contains("is-nan-nan=true"),
        "is_nan(nan) should be true; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("is-nan-0=false"),
        "is_nan(0) should be false; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("is-inf-inf=true"),
        "is_inf(inf) should be true; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("is-inf-finite=false"),
        "is_inf(1.5) should be false; stdout:\n{stdout}"
    );
}
