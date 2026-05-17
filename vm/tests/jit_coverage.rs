//! Per-example JIT coverage report.
//!
//! Compiles each of the 7 acceptance examples, decodes each function, and
//! reports which functions the JIT successfully compiled vs which fall
//! back to the interpreter. Run with `--no-capture` to see the report.

#![cfg(feature = "jit")]

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::decompile;
use strictpy_vm::loader;

fn examples_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.push("examples");
    p
}

fn report(name: &str) {
    let src_path = examples_dir().join(name);
    let src = fs::read_to_string(&src_path).expect("read example");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile {name}: {e}"));
    let module = loader::load(&bytes).expect("load");
    let mut jit_count = 0usize;
    let mut total = 0usize;
    let mut failed: Vec<(String, &'static str)> = vec![];
    for f in &module.functions {
        total += 1;
        let nm = module
            .strings
            .get(f.name_idx as usize)
            .map(|s| s.as_str())
            .unwrap_or("?")
            .to_string();
        match decompile::decode_function(&module, f.code_offset, f.code_length) {
            Ok(_) => jit_count += 1,
            Err(decompile::DecodeError::Unsupported(why)) => {
                failed.push((nm, why));
            }
            Err(_) => {
                failed.push((nm, "decode"));
            }
        }
    }
    println!("== {} — {}/{} functions JIT-eligible ==", name, jit_count, total);
    for (nm, why) in failed {
        println!("    interp  {} (uses {})", nm, why);
    }
}

#[test]
fn coverage_report() {
    for ex in ["hello.spy", "fib.spy", "dot.spy", "tree.spy", "mandelbrot.spy", "producer.spy", "wordcount.spy"] {
        report(ex);
    }
}
