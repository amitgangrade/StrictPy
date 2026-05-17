//! # StrictPy conformance suite — positive cases
//!
//! Loops over every `examples/*.spy` file at the repository root and
//! asserts that the program is accepted by the front end (lex → parse →
//! resolve → typecheck). The example programs are written to be the
//! canonical "good citizen" inputs for the StrictPy compiler — if any of
//! them is rejected by M2, that is a regression.
//!
//! ## Why `catch_unwind`?
//!
//! At time of writing, post-typecheck stages (lower / optimize / emit)
//! are still `todo!("M3: ...")` stubs. A successful M2 pass therefore
//! ends with a panic from one of those stubs, not with `Ok`. The harness
//! catches the panic and treats `M3`/`M4`/.../`M7` markers as success —
//! anything earlier (or a string that doesn't carry a milestone marker)
//! is a real failure to be reported.
//!
//! Once the rest of the pipeline lands, the panic branch can be removed
//! and the test can simply assert `Ok`.

use std::fs;
use std::panic;
use std::path::{Path, PathBuf};

use strictpy_compiler::compile_source;

fn examples_dir() -> PathBuf {
    // tests/ lives inside the `compiler` crate; examples/ is one level up
    // from the crate root.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.push("examples");
    p
}

fn check_one(path: &Path) {
    let src = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
    let display = path.display().to_string();
    let src_for_closure = src.clone();
    let display_for_closure = display.clone();

    // Stage stubs use `todo!("Mn: ...")` which panics. Catch the panic so
    // we can distinguish "got past M2" from "M2 itself blew up".
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        compile_source(display_for_closure, &src_for_closure)
    }));

    match result {
        Err(panic_info) => {
            let msg = panic_info
                .downcast_ref::<String>()
                .map(|s| s.as_str())
                .or_else(|| panic_info.downcast_ref::<&'static str>().copied())
                .unwrap_or("<non-string panic>");
            // Acceptable: a stub panic from a milestone strictly *after* M2.
            let from_later_milestone = msg.contains("M3")
                || msg.contains("M4")
                || msg.contains("M5")
                || msg.contains("M6")
                || msg.contains("M7");
            assert!(
                from_later_milestone,
                "{}: panicked at a pre-M3 stage (M2 or earlier): {}",
                display, msg
            );
        }
        Ok(Ok(_bytes)) => {
            // Whole pipeline succeeded — fine, nothing more to assert.
        }
        Ok(Err(err)) => {
            panic!(
                "{}: M2 rejected a program that should typecheck: {:?}",
                display, err
            );
        }
    }
}

#[test]
fn positive_conformance() {
    let dir = examples_dir();
    let mut any = false;
    for entry in fs::read_dir(&dir).expect("read examples/") {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("spy") {
            continue;
        }
        any = true;
        check_one(&path);
    }
    assert!(any, "no .spy examples found under {}", dir.display());
}
