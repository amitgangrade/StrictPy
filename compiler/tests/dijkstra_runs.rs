//! Integration test for examples/dijkstra.spy.
//!
//! Compiles the program, runs it through the VM with stdout captured,
//! and asserts:
//!   - exits 0
//!   - prints "OK: 5/5" (all four canonical Dijkstra tests + the bonus
//!     "for v in g.adj_node[u]:" probe pass)
//!   - no "FAIL" anywhere in stdout
//!
//! This is the M12 stress test for the class-with-list-fields pattern
//! plus a hand-rolled min-heap exercising recursive method calls. If a
//! field-offset / vtable bug regresses post-M11 it shows up here as a
//! "FAIL test=N expected=X got=Y" line, NOT as a heap-corruption exit
//! code — the program is structured to render mis-computation
//! deterministically.

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
fn dijkstra_compiles() {
    let src_path = project_root().join("examples").join("dijkstra.spy");
    let src = fs::read_to_string(&src_path).expect("read dijkstra.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile dijkstra.spy: {e}"));
}

#[test]
fn dijkstra_solves_classic_graphs() {
    let src_path = project_root().join("examples").join("dijkstra.spy");
    let src = fs::read_to_string(&src_path).expect("read dijkstra.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile dijkstra.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("dijkstra.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run dijkstra");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // No failures.
    assert!(
        !out.contains("FAIL"),
        "at least one Dijkstra test failed; stdout was:\n{out}"
    );

    // Each of the five tests should print its PASS line.
    for t in 1..=5i32 {
        let line = format!("PASS test={t}");
        assert!(
            out.contains(&line),
            "missing {line:?} in stdout:\n{out}"
        );
    }

    // Summary line.
    assert!(
        out.contains("OK: 5/5"),
        "summary line missing or wrong count; stdout was:\n{out}"
    );
}
