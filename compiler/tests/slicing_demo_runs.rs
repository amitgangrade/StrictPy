//! Lane B acceptance test: `examples/slicing_demo.spy` — slice syntax for
//! `str` and `List[T]` with negative bounds and step (including reverse).

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
fn slicing_demo_compiles() {
    let src_path = project_root().join("examples").join("slicing_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read slicing_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile slicing_demo.spy: {e}"));
}

#[test]
fn slicing_demo_runs_ok() {
    let src_path = project_root().join("examples").join("slicing_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read slicing_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile slicing_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("slicing_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run slicing_demo");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // str slices.
    assert!(out.contains("hello\n"), "stdout:\n{out}");
    assert!(out.contains("world\n"), "stdout:\n{out}");
    assert!(out.contains("hello world\n"), "stdout:\n{out}");
    assert!(out.contains("wor\n"), "stdout:\n{out}");
    assert!(out.contains("dlrow olleh\n"), "reverse; stdout:\n{out}");
    assert!(out.contains("hlowrd\n"), "step-2; stdout:\n{out}");
    assert!(out.contains("el o\n"), "1:8:2; stdout:\n{out}");

    // list slices.
    assert!(out.contains("mid len=3\n"), "stdout:\n{out}");
    assert!(out.contains("mid[0]=1\n"), "stdout:\n{out}");
    assert!(out.contains("mid[2]=3\n"), "stdout:\n{out}");
    assert!(out.contains("rev[0]=5\n"), "stdout:\n{out}");
    assert!(out.contains("rev[5]=0\n"), "stdout:\n{out}");
    assert!(out.contains("last_two[0]=4\n"), "stdout:\n{out}");
    assert!(out.contains("last_two[1]=5\n"), "stdout:\n{out}");

    assert!(
        out.contains("OK: slicing\n") || out.trim_end().ends_with("OK: slicing"),
        "stdout:\n{out}"
    );
}
