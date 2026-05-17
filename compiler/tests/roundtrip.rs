//! Round-trip test: source → AST → pretty-printed source → AST → pretty.
//!
//! Equality is checked at the *pretty-printed string* level rather than at the
//! AST level, because:
//!   1. Spans differ between the two parses (and we don't want to write a
//!      span-stripping walk for every node).
//!   2. String comparison shows a clean diff when something drifts.
//!
//! The invariant we actually need is **idempotence of pretty-printing**:
//! `pretty(parse(pretty(parse(s)))) == pretty(parse(s))`. The first pretty
//! is canonicalization; the second should be a fixed point.

use std::path::Path;

use strictpy_compiler::ast::Module;
use strictpy_compiler::lexer::Lexer;
use strictpy_compiler::parser::Parser;
use strictpy_compiler::pretty::print_module;

fn parse_str(src: &str) -> Module {
    let mut lexer = Lexer::new(src);
    let mut tokens = Vec::new();
    loop {
        let tok = lexer
            .next_token()
            .unwrap_or_else(|e| panic!("lex error: {e:?}"));
        let is_eof = matches!(tok.kind, strictpy_compiler::lexer::TokenKind::Eof);
        tokens.push(tok);
        if is_eof {
            break;
        }
    }
    let mut parser = Parser::new(tokens);
    parser
        .parse_module()
        .unwrap_or_else(|e| panic!("parse error: {e:?}"))
}

fn roundtrip_one(path: &Path) {
    let src = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    let first_ast = parse_str(&src);
    let printed_once = print_module(&first_ast);

    let second_ast = parse_str(&printed_once);
    let printed_twice = print_module(&second_ast);

    if printed_once != printed_twice {
        // Show a useful failure: where the two outputs diverge.
        let diff_line = printed_once
            .lines()
            .zip(printed_twice.lines())
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        panic!(
            "pretty-print not idempotent for {}: first divergence at line {diff_line}\n\
             --- first  pretty ---\n{printed_once}\n\
             --- second pretty ---\n{printed_twice}",
            path.display(),
        );
    }
}

#[test]
fn roundtrip_all_examples() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("examples");

    let mut paths: Vec<_> = std::fs::read_dir(&examples)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", examples.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("spy"))
        .collect();
    paths.sort();

    assert!(!paths.is_empty(), "no .spy files in {}", examples.display());

    for path in paths {
        roundtrip_one(&path);
    }
}
