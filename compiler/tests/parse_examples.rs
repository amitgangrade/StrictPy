//! Integration test: lex+parse every `examples/*.spy` and assert success.
//!
//! This test depends on the lexer being implemented. The lexer M1 has landed,
//! so the test is enabled by default.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::lexer::{Lexer, TokenKind};
use strictpy_compiler::parser::Parser;

fn examples_dir() -> PathBuf {
    // tests/ lives inside the `compiler` crate; examples/ is two levels up.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.push("examples");
    p
}

#[test]
fn parse_all_examples() {
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
        let mut lexer = Lexer::new(&src);
        let mut tokens = Vec::new();
        loop {
            let t = lexer
                .next_token()
                .unwrap_or_else(|e| panic!("lex error in {:?}: {}", path, e));
            let is_eof = matches!(t.kind, TokenKind::Eof);
            tokens.push(t);
            if is_eof {
                break;
            }
        }
        let mut parser = Parser::new(tokens);
        parser
            .parse_module()
            .unwrap_or_else(|e| panic!("parse error in {:?}: {}", path, e));
    }
    assert!(any, "no .spy examples found under {:?}", dir);
}
