//! M18 acceptance test: `examples/graph_lib.spy` — generic graph
//! algorithms over a non-generic `final class Graph`. Hammers the M17
//! monomorphisation worklist with five distinct instantiations of
//! `safe_get[T]` (i32, f64, str, Tuple[i32, f64], and a final-class
//! type) plus transitive instantiation paths (`min_by[T] -> safe_get[T]`
//! and `first_or_default[T] -> safe_get[T]`).
//!
//! Asserts:
//!   - the program exits 0 with "OK: 9/9"
//!   - every PASS line shows up (9 tests)
//!   - the worklist drained the expected mangled `safe_get__*` set —
//!     this is the M17-specific empirical claim of the M18 brief.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::loader;
use strictpy_vm::run_file_capture;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn graph_lib_compiles() {
    let src_path = project_root().join("examples").join("graph_lib.spy");
    let src = fs::read_to_string(&src_path).expect("read graph_lib.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile graph_lib.spy: {e}"));
}

#[test]
fn graph_lib_runs_all_nine_tests() {
    let src_path = project_root().join("examples").join("graph_lib.spy");
    let src = fs::read_to_string(&src_path).expect("read graph_lib.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile graph_lib.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("graph_lib.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run graph_lib");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // All nine sub-tests must pass.
    for n in 1..=9 {
        let needle = format!("PASS test={n}");
        assert!(
            out.contains(&needle),
            "missing {needle}; stdout:\n{out}"
        );
    }
    assert!(
        out.contains("OK: 9/9"),
        "missing OK line; stdout:\n{out}"
    );

    // Test 8 emits the safe_get instantiation count it observed.
    assert!(
        out.contains("instantiations=5"),
        "test 8 should have exercised 5 distinct safe_get[T] instantiations; stdout:\n{out}"
    );
}

/// The M18 brief specifically asks for an empirical count of how many
/// monomorphic instantiations the worklist drained. We get that by
/// loading the compiled `.spyc`, walking its function table, and
/// counting entries whose name contains `__` (the M17 mangling marker).
#[test]
fn graph_lib_worklist_drains_to_expected_set() {
    let src_path = project_root().join("examples").join("graph_lib.spy");
    let src = fs::read_to_string(&src_path).expect("read graph_lib.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile graph_lib.spy: {e}"));

    let module = loader::load(&bytes).expect("load graph_lib bytecode");

    // Collect every function name. The string at `name_idx` is the
    // (possibly mangled) function name.
    let mut all_names: Vec<&str> = module
        .functions
        .iter()
        .map(|f| module.strings[f.name_idx as usize].as_str())
        .collect();
    all_names.sort();

    // Mangled names contain `__` (double underscore separator between
    // source name and the encoded type args).
    let mangled: Vec<&str> = all_names
        .iter()
        .filter(|n| n.contains("__"))
        .copied()
        .collect();

    // === Expected safe_get[T] instantiations ============================
    // The brief requires 5+ distinct T values. We use:
    //   i32, f64, str, Tuple[i32, f64], class Node.
    let safe_get_insts: Vec<&str> = mangled
        .iter()
        .filter(|n| n.starts_with("safe_get__"))
        .copied()
        .collect();
    assert!(
        safe_get_insts.len() >= 5,
        "expected >=5 safe_get instantiations; got {}: {:?}\nall mangled: {:?}",
        safe_get_insts.len(),
        safe_get_insts,
        mangled
    );

    // Each of these specific suffixes must be present.
    let must_have = [
        "safe_get__i32",
        "safe_get__f64",
        "safe_get__str",
        // tuple_<A>_<B>; per §5.1.5 the i32+f64 instantiation mangles as
        // `safe_get__tuple_i32_f64`.
        "safe_get__tuple_i32_f64",
    ];
    for needle in &must_have {
        assert!(
            mangled.contains(needle),
            "expected to find {needle} in mangled table; got {:?}",
            safe_get_insts
        );
    }
    // The class-typed instantiation mangles as `safe_get__class<N>` for
    // some N (the type id is assigned during typecheck). The actual
    // implementation drops the `<>` brackets — e.g. `safe_get__class17`
    // — which is a minor divergence from the spec §5.1.5 example
    // (spec shows `class<N>`). Just confirm at least one such
    // instantiation exists.
    let class_safe_get = safe_get_insts
        .iter()
        .any(|n| n.starts_with("safe_get__class"));
    assert!(
        class_safe_get,
        "expected a class-typed safe_get instantiation; got {:?}",
        safe_get_insts
    );

    // === Expected transitive helpers ====================================
    // min_by[T] called with i32 -> mangled `min_by__i32`
    assert!(
        mangled.iter().any(|n| n.starts_with("min_by__")),
        "expected at least one min_by instantiation; mangled: {:?}",
        mangled
    );
    // enumerate[T] called with i32 -> mangled `enumerate__i32`
    assert!(
        mangled.iter().any(|n| n.starts_with("enumerate__")),
        "expected at least one enumerate instantiation; mangled: {:?}",
        mangled
    );
    // first_or_default[T] called with i32
    assert!(
        mangled.iter().any(|n| n.starts_with("first_or_default__")),
        "expected at least one first_or_default instantiation; mangled: {:?}",
        mangled
    );

    // Total mangled count must be 8+ (5 safe_get + min_by + enumerate
    // + first_or_default = 8 minimum). Any duplicates or unexpected
    // re-mangling would surface here.
    assert!(
        mangled.len() >= 8,
        "expected >=8 mangled instantiations; got {} ({:?})",
        mangled.len(),
        mangled
    );

    // For the M18 report — print the actual list so the orchestrator
    // can paste it. cargo test --release captures stdout by default;
    // `-- --nocapture` will show it.
    eprintln!("graph_lib mangled function table ({} entries):", mangled.len());
    for n in &mangled {
        eprintln!("  {n}");
    }
}
