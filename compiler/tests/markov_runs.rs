//! Integration test for `examples/markov.spy` — an order-1 Markov chain
//! text generator over a small Alice-in-Wonderland corpus.
//!
//! The program is well-behaved: no class hierarchies, no virtual dispatch,
//! just Dict[str, List[str]] + a hand-rolled LCG. We run it through the
//! in-process VM via `run_file_capture` and check the generated line has
//! enough whitespace-separated tokens to look real.

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
fn markov_compiles() {
    let src_path = project_root().join("examples").join("markov.spy");
    let src = fs::read_to_string(&src_path).expect("read markov.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile markov.spy: {e}"));
}

#[test]
fn markov_generates_a_chain_starting_from_alice() {
    let src_path = project_root().join("examples").join("markov.spy");
    let src = fs::read_to_string(&src_path).expect("read markov.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile markov.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("markov.spyc");
    fs::write(&spyc_path, &bytes).expect("write markov.spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("markov must run");
    eprintln!("STDOUT:\n{out}");
    assert_eq!(code, 0, "exit code; stdout was: {out:?}");

    // The program prints the corpus size, the unique-token count, a
    // separator, and then the generated chain.
    assert!(
        out.contains("corpus words:"),
        "missing corpus-words line: {out:?}"
    );
    assert!(
        out.contains("unique tokens:"),
        "missing unique-tokens line: {out:?}"
    );
    assert!(
        out.contains("--- generated ---"),
        "missing separator line: {out:?}"
    );

    // The last non-empty line is the generated chain. The seed is "Alice".
    let last = out.lines().rev().find(|l| !l.is_empty()).expect("any line");
    assert!(
        last.starts_with("Alice"),
        "chain doesn't start with `Alice`: {last:?}"
    );
    let n_words = last.split_whitespace().count();
    assert!(
        n_words >= 30,
        "chain too short ({n_words} words): {last:?}"
    );
}
