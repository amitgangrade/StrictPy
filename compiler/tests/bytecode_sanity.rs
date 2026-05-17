//! Sanity test: compiling `examples/hello.spy` must produce a `.spyc` whose
//! string table contains the literal "Hello, StrictPy!". This proves the
//! compiler is at minimum interning string literals through the pipeline.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;

#[test]
fn hello_world_string_appears_in_output() {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.push("examples/hello.spy");
    let src = fs::read_to_string(&p).expect("read hello.spy");
    let bytes = compile_source(p.display().to_string(), &src).expect("compile hello.spy");

    let needle = b"Hello, StrictPy!";
    let found = bytes.windows(needle.len()).any(|w| w == needle);
    assert!(
        found,
        "expected the literal {:?} somewhere in the compiled bytes",
        String::from_utf8_lossy(needle)
    );
}
