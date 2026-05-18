//! Integration test for examples/dijkstra_with_tuples.spy.
//!
//! Mirrors `dijkstra_runs.rs` but for the M14 rewrite that uses the new
//! `Tuple[i32, f64]` return shape on `MinHeap.pop_min`. The end-to-end
//! contract is the same — four canonical Dijkstra cases compute the
//! expected shortest-path distances — but the path through the compiler
//! is different:
//!
//!   - `MinHeap.pop_min(self) -> Tuple[i32, f64]` exercises tuple
//!     return-value lowering (M14 IR Pass 2.5 must register the
//!     `(i32, f64)` shape and emit a synthetic type-table entry).
//!   - The dijkstra(...) main loop destructures via
//!         `u: i32, p: f64 = pq.pop_min()`
//!     which exercises the new `Stmt::LetDestructure` lowering path.
//!   - `return (root_k, root_p)` exercises `Expr::Tuple` lowering at
//!     return position.
//!
//! If any of those paths regresses, this test surfaces it either as a
//! compile error (parser / typechecker rejected the new shape) or as a
//! "FAIL test=N" line (the IR allocated the tuple correctly but the
//! caller read garbage from `.0` / `.1`).

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
fn dijkstra_with_tuples_compiles() {
    let src_path = project_root().join("examples").join("dijkstra_with_tuples.spy");
    let src = fs::read_to_string(&src_path).expect("read dijkstra_with_tuples.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile dijkstra_with_tuples.spy: {e}"));
}

#[test]
fn dijkstra_with_tuples_solves_classic_graphs() {
    let src_path = project_root().join("examples").join("dijkstra_with_tuples.spy");
    let src = fs::read_to_string(&src_path).expect("read dijkstra_with_tuples.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile dijkstra_with_tuples.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("dijkstra_with_tuples.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run dijkstra_with_tuples");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // No failures.
    assert!(
        !out.contains("FAIL"),
        "at least one Dijkstra-with-tuples test failed; stdout was:\n{out}"
    );

    // Each of the four tests should print its PASS line. The M14 program
    // drops the "for v in g.adj_node[u]" probe (test #5 in the original)
    // because the win we're showcasing is on the heap side, not loop
    // desugaring — keeping just the canonical 4 graphs.
    for t in 1..=4i32 {
        let line = format!("PASS test={t}");
        assert!(
            out.contains(&line),
            "missing {line:?} in stdout:\n{out}"
        );
    }

    // Summary line.
    assert!(
        out.contains("OK: 4/4"),
        "summary line missing or wrong count; stdout was:\n{out}"
    );
}
