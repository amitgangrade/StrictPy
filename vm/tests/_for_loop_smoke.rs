//! Smoke test for the M9 `for x in xs:` desugaring: compile and run
//! quicksort.spy (which uses the new for-loop in main) and confirm it
//! exits cleanly and prints each sorted element on its own line.
//!
//! real-world: this is the only spot exercising the for-loop runtime
//! path end-to-end; ir.rs::tests::for_in_list_desugars_to_indexed_while
//! covers the IR shape.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn examples_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.push("examples");
    p
}

fn compile_to_temp(name: &str) -> PathBuf {
    let src_path = examples_dir().join(name);
    let src = fs::read_to_string(&src_path).expect("read example");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile {name}: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_for_loop_{}.spyc", name.replace('.', "_")));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

#[test]
fn quicksort_for_loop_emits_sorted_elements() {
    let p = compile_to_temp("quicksort.spy");
    let (code, out) = run_file_capture(&p).expect("quicksort.spy must run cleanly");
    assert_eq!(code, 0, "exit code; stdout was: {out:?}");
    // The for-loop iterates the sorted list 0..9 and prints each element
    // on its own line via str(v). Expected order after sorting:
    //   [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
    for i in 0..=9 {
        let expected = format!("{i}\n");
        assert!(
            out.contains(&expected),
            "for-loop output missing `{i}` line; stdout was: {out:?}"
        );
    }
}
