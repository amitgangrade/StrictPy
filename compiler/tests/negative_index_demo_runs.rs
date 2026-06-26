//! Lane B acceptance test: `examples/negative_index_demo.spy` — negative
//! indexing for `str` and `List[T]`, on read, write, and aug-assign.

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
fn negative_index_demo_compiles() {
    let src_path = project_root().join("examples").join("negative_index_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read negative_index_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile negative_index_demo.spy: {e}"));
}

#[test]
fn negative_index_demo_runs_ok() {
    let src_path = project_root().join("examples").join("negative_index_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read negative_index_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile negative_index_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("negative_index_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run negative_index_demo");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    assert!(out.contains("xs[-1]=40\n"), "stdout:\n{out}");
    assert!(out.contains("xs[-2]=30\n"), "stdout:\n{out}");
    assert!(out.contains("xs[-4]=10\n"), "stdout:\n{out}");
    assert!(out.contains("after xs[-1]=99: 99\n"), "stdout:\n{out}");
    assert!(out.contains("xs[2]=35\n"), "aug-assign; stdout:\n{out}");
    assert!(out.contains("s[-1]=f\n"), "stdout:\n{out}");
    assert!(out.contains("s[-6]=a\n"), "stdout:\n{out}");
    assert!(
        out.contains("OK: negative-index\n") || out.trim_end().ends_with("OK: negative-index"),
        "stdout:\n{out}"
    );
}
