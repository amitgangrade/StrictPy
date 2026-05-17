//! Integration test: every `examples/*.spy` must lex+parse+resolve+typecheck.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::lexer::{Lexer, TokenKind};
use strictpy_compiler::parser::Parser;
use strictpy_compiler::resolver::Resolver;
use strictpy_compiler::typecheck::TypeChecker;

fn examples_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.push("examples");
    p
}

#[test]
fn typecheck_all_examples() {
    let dir = examples_dir();
    let mut any = false;
    for entry in fs::read_dir(&dir).expect("read examples/") {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("spy") { continue; }
        any = true;
        let src = fs::read_to_string(&path).unwrap();
        let mut lx = Lexer::new(&src);
        let mut toks = Vec::new();
        loop {
            let t = lx.next_token()
                .unwrap_or_else(|e| panic!("lex error in {:?}: {}", path, e));
            let eof = matches!(t.kind, TokenKind::Eof);
            toks.push(t);
            if eof { break; }
        }
        let module = Parser::new(toks)
            .parse_module()
            .unwrap_or_else(|e| panic!("parse error in {:?}: {}", path, e));
        let resolved = Resolver::new().resolve(module)
            .unwrap_or_else(|e| panic!("resolve error in {:?}: {}", path, e));
        TypeChecker::new().check(resolved)
            .unwrap_or_else(|e| panic!("typecheck error in {:?}: {}", path, e));
    }
    assert!(any, "no .spy examples found under {:?}", dir);
}
