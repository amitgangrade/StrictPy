//! Subprocess integration test for `examples/list_dir.spy` (M20a).
//!
//! Compiles the example, runs it through `spy.exe`, and asserts that
//! at least one known top-level file in the project root (`Cargo.toml`)
//! is present in the listing.  `spy.exe` runs in the project root by
//! default, so `os.listdir(".")` returns the workspace top-level.

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
fn list_dir_compiles() {
    let src_path = project_root().join("examples").join("list_dir.spy");
    let src = fs::read_to_string(&src_path).expect("read list_dir.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile list_dir.spy: {e}"));
}

#[test]
fn list_dir_includes_known_project_file_via_spy_exe() {
    let src_path = project_root().join("examples").join("list_dir.spy");
    let src = fs::read_to_string(&src_path).expect("read list_dir.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile list_dir.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("list_dir.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        // Pin the subprocess cwd to the project root so the listing is
        // predictable (workspace top-level contains `Cargo.toml`).
        .current_dir(project_root())
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(output.status.success(),
        "spy.exe failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status);

    assert!(stdout.starts_with("cwd:"), "missing cwd banner; stdout:\n{stdout}");
    assert!(stdout.contains("Cargo.toml"),
        "Cargo.toml should appear in project-root listing; stdout:\n{stdout}");
    assert!(stdout.contains("examples"),
        "examples dir should appear in project-root listing; stdout:\n{stdout}");
}
