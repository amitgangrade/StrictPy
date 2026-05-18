//! Subprocess integration test for `examples/itertools_demo.spy` (M22 P2C).
//!
//! Compiles + runs the itertools demo through `spy.exe` and asserts the
//! printed values match expectation.

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
fn itertools_demo_compiles() {
    let src_path = project_root().join("examples").join("itertools_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read itertools_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile itertools_demo.spy: {e}"));
}

#[test]
fn itertools_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("itertools_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read itertools_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile itertools_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("itertools_demo.spyc");
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

    // range_step(2, 11, 3) = [2, 5, 8].
    assert!(
        stdout.contains("range_step=[2,5,8]"),
        "expected range_step result; stdout:\n{stdout}"
    );
    // enumerate over ["alpha", "beta", "gamma"].
    assert!(
        stdout.contains("enumerate=(0,alpha)(1,beta)(2,gamma)"),
        "expected enumerate result; stdout:\n{stdout}"
    );
    // zip(["a","b","c","d"], ["1","2","3"]) truncates to length 3.
    assert!(
        stdout.contains("zip=(a,1)(b,2)(c,3)"),
        "expected zip result; stdout:\n{stdout}"
    );
    // chain(["x","y"], ["z","w"]) = ["x","y","z","w"].
    assert!(
        stdout.contains("chain=[x,y,z,w]"),
        "expected chain result; stdout:\n{stdout}"
    );
    // take(fruits, 2) = ["apple","banana"].
    assert!(
        stdout.contains("take=[apple,banana]"),
        "expected take result; stdout:\n{stdout}"
    );
    // drop(fruits, 3) = ["date","elder"].
    assert!(
        stdout.contains("drop=[date,elder]"),
        "expected drop result; stdout:\n{stdout}"
    );
    // pairwise(["a","b","c","d"]) = [(a,b), (b,c), (c,d)].
    assert!(
        stdout.contains("pairwise=(a,b)(b,c)(c,d)"),
        "expected pairwise result; stdout:\n{stdout}"
    );
    // accumulate([1,2,3,4]) = [1,3,6,10].
    assert!(
        stdout.contains("accumulate=[1,3,6,10]"),
        "expected accumulate result; stdout:\n{stdout}"
    );
    // flatten([["a","b"], ["c"], ["d","e","f"]]) = ["a","b","c","d","e","f"].
    assert!(
        stdout.contains("flatten=[a,b,c,d,e,f]"),
        "expected flatten result; stdout:\n{stdout}"
    );
}
