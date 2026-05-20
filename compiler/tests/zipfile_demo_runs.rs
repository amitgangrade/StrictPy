//! Subprocess integration test for `examples/zipfile_demo.spy`
//! (M27 P3c-D).
//!
//! Compiles + runs the zipfile demo through `spy.exe` and asserts the
//! printed output is consistent with an end-to-end create / close /
//! reopen / read round-trip including high-byte (codepoint 0..255)
//! payloads and the `info` triple's behaviour on a missing entry.

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
fn zipfile_demo_compiles() {
    let src_path = project_root().join("examples").join("zipfile_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read zipfile_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile zipfile_demo.spy: {e}"));
}

#[test]
fn zipfile_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("zipfile_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read zipfile_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile zipfile_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("zipfile_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    // The demo writes target/zipfile_demo_round_trip.zip, so it needs
    // to run with cwd=project_root() (where target/ lives).
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

    // is_zipfile probes
    assert!(
        stdout.contains("is_zipfile: true"),
        "is_zipfile true; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("is_zipfile_missing: false"),
        "is_zipfile missing false; stdout:\n{stdout}"
    );

    // 3 entries listed.
    assert!(
        stdout.contains("name_count: 3"),
        "name_count; stdout:\n{stdout}"
    );

    // All three entries round-trip exactly (including the 256-byte
    // packed-bytes buffer).
    assert!(stdout.contains("rt_a: ok"), "rt_a; stdout:\n{stdout}");
    assert!(stdout.contains("rt_b: ok"), "rt_b; stdout:\n{stdout}");
    assert!(
        stdout.contains("rt_c_len: 256"),
        "rt_c_len; stdout:\n{stdout}"
    );

    // info() returns the exact uncompressed_size + non-negative cs/crc.
    assert!(
        stdout.contains("info_a_size: ok"),
        "info_a_size; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("info_a_cs_nonneg: ok"),
        "info_a_cs_nonneg; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("info_a_crc_nonneg: ok"),
        "info_a_crc_nonneg; stdout:\n{stdout}"
    );

    // Missing-entry probe returns (-1, -1, -1).
    assert!(
        stdout.contains("info_missing: ok"),
        "info_missing; stdout:\n{stdout}"
    );

    // ValueError on read of missing entry.
    assert!(
        stdout.contains("missing_read: caught ValueError"),
        "missing_read; stdout:\n{stdout}"
    );

    assert!(stdout.contains("done"), "done; stdout:\n{stdout}");
}
