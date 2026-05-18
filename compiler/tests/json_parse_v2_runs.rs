//! Integration test for `examples/json_parse_v2.spy` — the M18 rewrite of
//! `examples/json_parse.spy` against the M13-M17 surface.
//!
//! Where the M10 baseline had to run out-of-process (because the in-process
//! VM `Drop` path was crashing with `STATUS_HEAP_CORRUPTION` after the
//! program had otherwise completed), v2 should be heap-clean — the
//! M11 BUG-016 fix + the M12 torture-test verification removed the
//! corruption family entirely. So we use `run_file_capture` here directly.
//!
//! Acceptance:
//!   * compile succeeds
//!   * program exits 0
//!   * stdout contains the expected round-tripped forms for each input
//!   * stdout ends with `OK: N/N` where N is the total number of cases
//!     enumerated in main().

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
fn json_parse_v2_compiles() {
    let src_path = project_root().join("examples").join("json_parse_v2.spy");
    let src = fs::read_to_string(&src_path).expect("read json_parse_v2.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile json_parse_v2.spy: {e}"));
}

#[test]
fn json_parse_v2_runs_clean() {
    let src_path = project_root().join("examples").join("json_parse_v2.spy");
    let src = fs::read_to_string(&src_path).expect("read json_parse_v2.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("json_parse_v2.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc_path).expect("run json_parse_v2");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    // 14 cases enumerated in main(). All should pass.
    assert!(out.contains("OK: 14/14"), "summary; stdout:\n{out}");

    // Each round-trip prints `<label>: <rendered>`. We sample the more
    // interesting cases.

    // 01 atom_null
    assert!(out.contains("01 atom_null: null"), "01; stdout:\n{out}");
    // 02 atom_true
    assert!(out.contains("02 atom_true: true"), "02; stdout:\n{out}");
    // 03 atom_num — render_number trims the .0 off integer-valued f64.
    assert!(out.contains("03 atom_num: 42"), "03; stdout:\n{out}");
    // 04 atom_str
    assert!(out.contains("04 atom_str: \"alice\""), "04; stdout:\n{out}");
    // 05 array — exact round-trip including types.
    assert!(out.contains("05 array: [1, 2, 3, true, null, \"x\"]"),
            "05; stdout:\n{out}");
    // 06 flat_obj — preserves insertion order via the List[Tuple] storage.
    assert!(out.contains("06 flat_obj: {\"name\": \"alice\", \"age\": 30, \"active\": true, \"score\": null}"),
            "06; stdout:\n{out}");
    // 07 nested_obj — depth-2 object-inside-object, plus an array of strings.
    assert!(out.contains("07 nested_obj: {\"name\": \"alice\", \"tags\": [\"admin\", \"user\"], \"meta\": {\"active\": true, \"age\": 30}}"),
            "07; stdout:\n{out}");
    // 08 deep_nest — 6+ levels deep, exercises recursive method calls.
    assert!(out.contains("08 deep_nest: {\"a\": [1, [2, [3, [4, [5, [6, {\"deep\": [7, 8, 9]}]]]]]]}"),
            "08; stdout:\n{out}");
    // 09 malformed_obj — should print PARSE-ERROR (caught by try/except).
    assert!(out.contains("09 malformed_obj: PARSE-ERROR"),
            "09; stdout:\n{out}");
    // 10 malformed_atom
    assert!(out.contains("10 malformed_atom: PARSE-ERROR"),
            "10; stdout:\n{out}");
    // 11 trailing garbage
    assert!(out.contains("11 trailing: PARSE-ERROR"),
            "11; stdout:\n{out}");
    // 12 try_parse recovered to none.
    assert!(out.contains("12 try_parse: recovered to none"),
            "12; stdout:\n{out}");
    // 13 isinstance helper.
    assert!(out.contains("13 isinstance: parsed is JsonObject"),
            "13; stdout:\n{out}");
    // 14 finally counter increments on both paths.
    assert!(out.contains("14 finally: cleaned=12"),
            "14; stdout:\n{out}");
}
