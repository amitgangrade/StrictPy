//! Subprocess integration test for `examples/str_methods_extra_demo.spy`
//! (LANE E: expanded str methods).

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
fn str_methods_extra_demo_compiles() {
    let src_path = project_root().join("examples").join("str_methods_extra_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read str_methods_extra_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile str_methods_extra_demo.spy: {e}"));
}

#[test]
fn str_methods_extra_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("str_methods_extra_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read str_methods_extra_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile str_methods_extra_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("str_methods_extra_demo.spyc");
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

    // search family
    assert!(stdout.contains("count=2"), "stdout:\n{stdout}");
    assert!(stdout.contains("rfind=12"), "stdout:\n{stdout}");
    assert!(stdout.contains("index=6"), "stdout:\n{stdout}");
    assert!(stdout.contains("rindex=15"), "stdout:\n{stdout}");
    // splitlines
    assert!(stdout.contains("lines=3"), "stdout:\n{stdout}");
    assert!(stdout.contains("line0=a"), "stdout:\n{stdout}");
    assert!(stdout.contains("line2=c"), "stdout:\n{stdout}");
    // partition
    assert!(stdout.contains("part0=a"), "stdout:\n{stdout}");
    assert!(stdout.contains("part1=="), "stdout:\n{stdout}");
    assert!(stdout.contains("part2=b=c"), "stdout:\n{stdout}");
    assert!(stdout.contains("rpart0=a=b"), "stdout:\n{stdout}");
    assert!(stdout.contains("rpart2=c"), "stdout:\n{stdout}");
    // padding
    assert!(stdout.contains("zfill=00042"), "stdout:\n{stdout}");
    assert!(stdout.contains("zfill_neg=-0042"), "stdout:\n{stdout}");
    assert!(stdout.contains("ljust=[hi   ]"), "stdout:\n{stdout}");
    assert!(stdout.contains("rjust=[   hi]"), "stdout:\n{stdout}");
    assert!(stdout.contains("center=[  hi  ]"), "stdout:\n{stdout}");
    // case transforms
    assert!(stdout.contains("title=Hello World"), "stdout:\n{stdout}");
    assert!(stdout.contains("swapcase=hELLO"), "stdout:\n{stdout}");
    assert!(stdout.contains("casefold=hello"), "stdout:\n{stdout}");
    assert!(stdout.contains("capitalize=Hello"), "stdout:\n{stdout}");
    // predicates
    assert!(stdout.contains("isdigit=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("isdigit_no=false"), "stdout:\n{stdout}");
    assert!(stdout.contains("isalpha=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("isalnum=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("isspace=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("isupper=true"), "stdout:\n{stdout}");
    assert!(stdout.contains("islower=true"), "stdout:\n{stdout}");
    // prefix/suffix/tabs
    assert!(stdout.contains("removeprefix=foo"), "stdout:\n{stdout}");
    assert!(stdout.contains("removesuffix=foo"), "stdout:\n{stdout}");
    assert!(stdout.contains("expandtabs=[a   b]"), "stdout:\n{stdout}");
}
