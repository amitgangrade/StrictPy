//! Subprocess integration test for `examples/tarfile_demo.spy`
//! (M27 P3c-D).
//!
//! Compiles + runs the tarfile demo through `spy.exe` and asserts the
//! printed output for the three transparent-compression modes (plain,
//! gz, bz2) plus the is_tarfile / bad-mode / missing-entry probes.

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
fn tarfile_demo_compiles() {
    let src_path = project_root().join("examples").join("tarfile_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tarfile_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tarfile_demo.spy: {e}"));
}

#[test]
fn tarfile_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("tarfile_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read tarfile_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile tarfile_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("tarfile_demo.spyc");
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

    // Plain mode round-trip.
    assert!(
        stdout.contains("plain_is_tarfile: ok"),
        "plain_is_tarfile; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("plain_name_count: 2"),
        "plain_name_count; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("plain_rt_a: ok"),
        "plain_rt_a; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("plain_rt_b: ok"),
        "plain_rt_b; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("plain_missing: caught ValueError"),
        "plain_missing; stdout:\n{stdout}"
    );

    // Gzip-wrapped round-trip.
    assert!(
        stdout.contains("gz_is_tarfile_raw: false (expected)"),
        "gz_is_tarfile_raw; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("gz_name_count: 2"),
        "gz_name_count; stdout:\n{stdout}"
    );
    assert!(stdout.contains("gz_rt_c: ok"), "gz_rt_c; stdout:\n{stdout}");
    assert!(stdout.contains("gz_rt_d: ok"), "gz_rt_d; stdout:\n{stdout}");

    // Bz2-wrapped round-trip.
    assert!(stdout.contains("bz2_rt_e: ok"), "bz2_rt_e; stdout:\n{stdout}");

    // Bad-mode probe rejected with ValueError.
    assert!(
        stdout.contains("bad_mode: caught ValueError"),
        "bad_mode; stdout:\n{stdout}"
    );

    // Missing-file is_tarfile is false (not raised).
    assert!(
        stdout.contains("missing_is_tarfile: false"),
        "missing_is_tarfile; stdout:\n{stdout}"
    );

    assert!(stdout.contains("done"), "done; stdout:\n{stdout}");
}
