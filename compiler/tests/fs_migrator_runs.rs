//! Subprocess integration test for `examples/fs_migrator.spy` (M24-D).
//!
//! Compiles + runs the filesystem migration tool through `spy.exe` and
//! asserts that the 10 Phase-3a probes report PASS (or document their
//! cross-platform fallback for probe 8 / probe 1).  The program creates
//! and cleans up its own test directory tree under cwd, so the test is
//! hermetic as long as the cwd is writeable.

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
fn fs_migrator_compiles() {
    let src_path = project_root().join("examples").join("fs_migrator.spy");
    let src = fs::read_to_string(&src_path).expect("read fs_migrator.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile fs_migrator.spy: {e}"));
}

#[test]
fn fs_migrator_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("fs_migrator.spy");
    let src = fs::read_to_string(&src_path).expect("read fs_migrator.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile fs_migrator.spy: {e}"));

    let tmpdir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let spyc_path = tmpdir.join("fs_migrator.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    // Run from CARGO_TARGET_TMPDIR so the test-tree creation doesn't
    // pollute the project root (and parallel runs don't collide).
    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .current_dir(&tmpdir)
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "spy.exe failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );

    // For local-debugging: dump captured stdout next to the spyc so a
    // human can read what the program actually produced.  This is a
    // diagnostic side-effect; the assertions below are the real test.
    let _ = fs::write(tmpdir.join("fs_migrator.stdout.txt"), &stdout);

    // Probe 1: recursive listdir over 3 levels found all 5 files.
    assert!(
        stdout.contains("probe1.recursive_listdir=PASS"),
        "probe 1 failed; stdout:\n{stdout}"
    );
    // Probe 2: pathlib.join with mixed separators didn't crash.
    assert!(
        stdout.contains("probe2.join_mixed=PASS"),
        "probe 2 failed; stdout:\n{stdout}"
    );
    // Probe 3: parts round-trip.
    assert!(
        stdout.contains("probe3.parts=PASS"),
        "probe 3 failed; stdout:\n{stdout}"
    );
    // Probe 4: stem/suffix edges all three cases.
    assert!(
        stdout.contains("probe4.stem_suffix=PASS"),
        "probe 4 failed; stdout:\n{stdout}"
    );
    // Probe 5: with_suffix on extensionless file.
    assert!(
        stdout.contains("probe5.with_suffix=PASS"),
        "probe 5 failed; stdout:\n{stdout}"
    );
    // Probe 6: relative_to happy path + ValueError.
    assert!(
        stdout.contains("probe6.relative_to=PASS"),
        "probe 6 happy path failed; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("probe6.value_error=PASS"),
        "probe 6 ValueError path failed; stdout:\n{stdout}"
    );
    // Probe 7: diff_days correctness + negative diff.
    assert!(
        stdout.contains("probe7.diff_days=PASS"),
        "probe 7 forward diff failed; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("probe7.diff_days_negative=PASS"),
        "probe 7 negative diff failed; stdout:\n{stdout}"
    );
    // Probe 8: subprocess mtime — may PASS or FAIL depending on
    // platform date locale.  We only assert the probe was REPORTED
    // (one of the two outcomes appears).
    let probe8_reported = stdout.contains("probe8.subprocess_mtime=PASS")
        || stdout.contains("probe8.subprocess_mtime=FAIL");
    assert!(
        probe8_reported,
        "probe 8 not reported; stdout:\n{stdout}"
    );
    // Probe 9: UTF-8 round-trip through read_text.
    assert!(
        stdout.contains("probe9.utf8_roundtrip=PASS"),
        "probe 9 failed; stdout:\n{stdout}"
    );
    // Probe 10: mkdir + remove dance — all three sub-steps PASS.
    assert!(
        stdout.contains("probe10.mkdir=PASS"),
        "probe 10 mkdir failed; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("probe10.write=PASS"),
        "probe 10 write failed; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("probe10.remove=PASS"),
        "probe 10 remove failed; stdout:\n{stdout}"
    );
    // Cleanup walked + removed every file from the test tree.
    assert!(
        stdout.contains("cleanup=PASS"),
        "cleanup failed; stdout:\n{stdout}"
    );
    // Program ran end-to-end.
    assert!(stdout.contains("DONE"), "DONE marker missing; stdout:\n{stdout}");
}
