//! Smoke test: `run_file` on a non-existent path returns an `Io` error.

use std::path::PathBuf;

use strictpy_vm::{run_file, VmError};

#[test]
fn missing_file_returns_io_error() {
    let mut path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    path.push("strictpy_vm_does_not_exist_12345.spyc");

    let err = run_file(&path).expect_err("expected an error for a missing file");
    match err {
        VmError::Io(_) => {}
        other => panic!("expected VmError::Io, got: {other:?}"),
    }
}
