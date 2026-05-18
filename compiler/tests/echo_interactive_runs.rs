//! Subprocess integration test for `examples/echo_interactive.spy` (M20a).
//!
//! Pipes a line via the subprocess's stdin and asserts the program reads
//! it back through `io.input_with_prompt(...)`.  Also verifies the
//! prompt itself appears on stdout (i.e. that `io.flush_stdout` made it
//! visible to the test before EOF on stdin closed the pipe).

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn echo_interactive_compiles() {
    let src_path = project_root().join("examples").join("echo_interactive.spy");
    let src = fs::read_to_string(&src_path).expect("read echo_interactive.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile echo_interactive.spy: {e}"));
}

#[test]
fn echo_interactive_reads_a_piped_line_via_spy_exe() {
    let src_path = project_root().join("examples").join("echo_interactive.spy");
    let src = fs::read_to_string(&src_path).expect("read echo_interactive.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile echo_interactive.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("echo_interactive.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    let mut child = Command::new(&spy_bin)
        .arg(&spyc_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn spy.exe");

    {
        let stdin = child.stdin.as_mut().expect("stdin");
        stdin.write_all(b"hello stdin\n").expect("write stdin");
    }
    let output = child.wait_with_output().expect("wait spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(output.status.success(),
        "spy.exe failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status);

    assert!(stdout.contains("> "),
        "prompt missing from stdout; stdout:\n{stdout}");
    assert!(stdout.contains("echo: hello stdin"),
        "echo missing; stdout:\n{stdout}");
}
