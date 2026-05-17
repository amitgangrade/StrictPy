//! TEMPORARY: dump dot.spy bytecode for debugging.

use std::path::PathBuf;
use strictpy_compiler::{ir, optimize, resolver, typecheck, lexer, parser, ast};
use strictpy_shared::Opcode;

#[test]
fn dump_dot_ir() {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.push("examples");
    p.push("dot.spy");
    let src = std::fs::read_to_string(&p).expect("read");

    let mut l = lexer::Lexer::new(&src);
    let mut toks = vec![];
    loop {
        let t = l.next_token().unwrap();
        let is_eof = matches!(t.kind, lexer::TokenKind::Eof);
        toks.push(t);
        if is_eof { break; }
    }
    let mut parser = parser::Parser::new(toks);
    let module: ast::Module = parser.parse_module().unwrap();
    let resolved = resolver::Resolver::new().resolve(module).unwrap();
    let typed = typecheck::TypeChecker::new().check(resolved).unwrap();
    let ir = ir::lower(typed);
    let ir = optimize::run(ir);

    for f in &ir.functions {
        eprintln!("=== fn {} (id={}) params={:?} ret={:?}", f.name, f.id.0, f.params.len(), f.ret);
        for b in &f.blocks {
            eprintln!("  block {}:", b.id.0);
            for v in &b.values {
                eprintln!("    v{} ty={:?} kind={:?}", v.id, v.ty, v.kind);
            }
            eprintln!("    term: {:?}", b.terminator);
        }
    }
}
