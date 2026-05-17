//! Compile each example and verify the M4 loader can parse it into a
//! [`strictpy_vm::loader::Module`] without error.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::loader;

fn examples_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.push("examples");
    p
}

#[test]
fn loader_parses_every_example() {
    let dir = examples_dir();
    let mut any = false;
    for entry in fs::read_dir(&dir).expect("read examples/") {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("spy") {
            continue;
        }
        any = true;
        let src = fs::read_to_string(&path).unwrap();
        let bytes = compile_source(path.display().to_string(), &src)
            .unwrap_or_else(|e| panic!("compile {:?}: {e}", path));
        let m = loader::load(&bytes)
            .unwrap_or_else(|e| panic!("loader rejected {:?}: {e}", path));
        assert!(
            !m.functions.is_empty(),
            "{:?} has no functions in its function table",
            path
        );
    }
    assert!(any, "no .spy examples found under {:?}", dir);
}
