//! Lane B acceptance test: `examples/star_unpack_demo.spy` — iterable
//! star-unpacking with the star target at the front, middle, and end.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn star_unpack_demo_compiles() {
    let src_path = project_root().join("examples").join("star_unpack_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read star_unpack_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile star_unpack_demo.spy: {e}"));
}

#[test]
fn star_unpack_demo_runs_ok() {
    let src_path = project_root().join("examples").join("star_unpack_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read star_unpack_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile star_unpack_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("star_unpack_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run star_unpack_demo");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // star at end
    assert!(out.contains("first=1\n"), "stdout:\n{out}");
    assert!(out.contains("rest len=4\n"), "stdout:\n{out}");
    assert!(out.contains("rest[0]=2\n"), "stdout:\n{out}");
    assert!(out.contains("rest[3]=5\n"), "stdout:\n{out}");
    // star at front
    assert!(out.contains("last=5\n"), "stdout:\n{out}");
    assert!(out.contains("init len=4\n"), "stdout:\n{out}");
    assert!(out.contains("init[0]=1\n"), "stdout:\n{out}");
    assert!(out.contains("init[3]=4\n"), "stdout:\n{out}");
    // star in middle
    assert!(out.contains("head=1\n"), "stdout:\n{out}");
    assert!(out.contains("tail=5\n"), "stdout:\n{out}");
    assert!(out.contains("mid len=3\n"), "stdout:\n{out}");
    assert!(out.contains("mid[0]=2\n"), "stdout:\n{out}");
    assert!(out.contains("mid[2]=4\n"), "stdout:\n{out}");
    // empty star tail
    assert!(out.contains("a=7\n"), "stdout:\n{out}");
    assert!(out.contains("b=8\n"), "stdout:\n{out}");
    assert!(out.contains("empty len=0\n"), "stdout:\n{out}");

    assert!(
        out.contains("OK: star-unpack\n") || out.trim_end().ends_with("OK: star-unpack"),
        "stdout:\n{out}"
    );
}
