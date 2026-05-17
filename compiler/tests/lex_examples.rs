//! Integration test: every file in `examples/*.spy` must lex without error.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::lexer::{Lexer, TokenKind};

fn examples_dir() -> PathBuf {
    // Cargo runs integration tests with CWD = crate root (compiler/).
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // strip "compiler"
    p.push("examples");
    p
}

fn lex_file(path: &std::path::Path) {
    let src = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut lex = Lexer::new(&src);
    loop {
        let t = lex.next_token()
            .unwrap_or_else(|e| panic!("lex error in {}: {e}", path.display()));
        if matches!(t.kind, TokenKind::Eof) { break; }
    }
}

#[test]
fn lex_all_examples() {
    let dir = examples_dir();
    let entries: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "spy").unwrap_or(false))
        .collect();
    assert!(!entries.is_empty(), "expected at least one .spy example");
    for e in entries {
        let p = e.path();
        println!("lexing {}", p.display());
        lex_file(&p);
    }
}
