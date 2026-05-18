//! M22 P2A regression tests for `argparse`, `collections`, and `csv`
//! stdlib modules.  Follows the M19/M20a/M20b/M20c pattern: compile a
//! tiny `.spy` snippet, write the bytecode to `CARGO_TARGET_TMPDIR`,
//! run through `run_file_capture[_with_args]`, and assert on stdout.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::{run_file_capture, run_file_capture_with_args};

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m22_p2a_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

fn tmpdir(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    p.push(format!("m22_p2a_{name}"));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).expect("create tmpdir");
    p
}

// ── argparse module ────────────────────────────────────────────────────

#[test]
fn argparse_parses_positional_and_flag_and_opt() {
    let src = "\
import argparse
fn main() -> i32:
    p: Dict[str, str] = argparse.new(\"demo\")
    argparse.add_arg(p, \"input\")
    argparse.add_flag(p, \"--verbose\", false)
    argparse.add_opt(p, \"--output\", \"out.txt\")
    a: Dict[str, str] = argparse.parse(p, sys.argv)
    println(\"input=\" + argparse.get_arg(a, \"input\"))
    println(\"output=\" + argparse.get_opt(a, \"--output\"))
    if argparse.get_flag(a, \"--verbose\"):
        println(\"verbose=true\")
    else:
        println(\"verbose=false\")
    return 0
import sys
";
    let p = compile_snippet("argparse_basic", src);
    let (code, out) = run_file_capture_with_args(
        &p,
        vec![
            "hello.in".to_string(),
            "--verbose".to_string(),
            "--output".to_string(),
            "result.bin".to_string(),
        ],
    )
    .expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("input=hello.in"), "got: {out:?}");
    assert!(out.contains("output=result.bin"), "got: {out:?}");
    assert!(out.contains("verbose=true"), "got: {out:?}");
}

#[test]
fn argparse_missing_positional_raises_value_error() {
    let src = "\
import argparse
import sys
fn main() -> i32:
    p: Dict[str, str] = argparse.new(\"demo\")
    argparse.add_arg(p, \"required_input\")
    try:
        a: Dict[str, str] = argparse.parse(p, sys.argv)
        println(\"unexpectedly succeeded\")
    except ValueError as e:
        println(\"caught: \" + e.message)
    return 0
";
    let p = compile_snippet("argparse_missing_arg", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught:"), "got: {out:?}");
    assert!(out.contains("required_input"), "got: {out:?}");
}

#[test]
fn argparse_default_opt_value_visible_when_unset() {
    let src = "\
import argparse
import sys
fn main() -> i32:
    p: Dict[str, str] = argparse.new(\"demo\")
    argparse.add_opt(p, \"--mode\", \"fast\")
    a: Dict[str, str] = argparse.parse(p, sys.argv)
    println(\"mode=\" + argparse.get_opt(a, \"--mode\"))
    return 0
";
    let p = compile_snippet("argparse_default", src);
    // No `--mode` in argv — should fall back to the registered default.
    let (code, out) = run_file_capture_with_args(&p, Vec::new()).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("mode=fast"), "got: {out:?}");
}

#[test]
fn argparse_unknown_flag_raises_value_error() {
    let src = "\
import argparse
import sys
fn main() -> i32:
    p: Dict[str, str] = argparse.new(\"demo\")
    try:
        a: Dict[str, str] = argparse.parse(p, sys.argv)
        println(\"no error\")
    except ValueError as e:
        println(\"caught: \" + e.message)
    return 0
";
    let p = compile_snippet("argparse_unknown", src);
    let (code, out) = run_file_capture_with_args(
        &p,
        vec!["--mystery".to_string()],
    )
    .expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught:"), "got: {out:?}");
    assert!(out.contains("--mystery"), "got: {out:?}");
}

#[test]
fn argparse_help_requested_detects_flag() {
    let src = "\
import argparse
import sys
fn main() -> i32:
    if argparse.help_requested(sys.argv):
        println(\"help yes\")
    else:
        println(\"help no\")
    return 0
";
    let p = compile_snippet("argparse_help_req", src);
    let (code, out) =
        run_file_capture_with_args(&p, vec!["--help".to_string()]).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("help yes"), "got: {out:?}");

    let (code2, out2) =
        run_file_capture_with_args(&p, vec!["something".to_string()]).expect("run");
    assert_eq!(code2, 0);
    assert!(out2.contains("help no"), "got: {out2:?}");
}

#[test]
fn argparse_help_text_includes_prog_and_options() {
    let src = "\
import argparse
fn main() -> i32:
    p: Dict[str, str] = argparse.new(\"mytool\")
    argparse.add_arg(p, \"input\")
    argparse.add_flag(p, \"--verbose\", false)
    argparse.add_opt(p, \"--output\", \"out.txt\")
    println(argparse.help_text(p))
    return 0
";
    let p = compile_snippet("argparse_help_text", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("mytool"), "got: {out:?}");
    assert!(out.contains("<input>"), "got: {out:?}");
    assert!(out.contains("--verbose"), "got: {out:?}");
    assert!(out.contains("--output"), "got: {out:?}");
    assert!(out.contains("--help"), "got: {out:?}");
}

#[test]
fn argparse_eq_form_for_opts() {
    // `--output=file.txt` should be equivalent to `--output file.txt`.
    let src = "\
import argparse
import sys
fn main() -> i32:
    p: Dict[str, str] = argparse.new(\"demo\")
    argparse.add_opt(p, \"--output\", \"default\")
    a: Dict[str, str] = argparse.parse(p, sys.argv)
    println(\"output=\" + argparse.get_opt(a, \"--output\"))
    return 0
";
    let p = compile_snippet("argparse_eq_form", src);
    let (code, out) =
        run_file_capture_with_args(&p, vec!["--output=eqval".to_string()]).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("output=eqval"), "got: {out:?}");
}

// ── collections module ─────────────────────────────────────────────────

#[test]
fn counter_basic_increment_and_get() {
    let src = "\
import collections
fn main() -> i32:
    c: Dict[str, i64] = collections.counter_new()
    collections.counter_increment(c, \"apple\")
    collections.counter_increment(c, \"apple\")
    collections.counter_increment(c, \"banana\")
    collections.counter_add(c, \"cherry\", 5i64)
    println(\"apple=\" + str(collections.counter_get(c, \"apple\")))
    println(\"banana=\" + str(collections.counter_get(c, \"banana\")))
    println(\"cherry=\" + str(collections.counter_get(c, \"cherry\")))
    println(\"missing=\" + str(collections.counter_get(c, \"missing\")))
    return 0
";
    let p = compile_snippet("counter_basic", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("apple=2"), "got: {out:?}");
    assert!(out.contains("banana=1"), "got: {out:?}");
    assert!(out.contains("cherry=5"), "got: {out:?}");
    // Counters return 0 for missing keys (Python parity).
    assert!(out.contains("missing=0"), "got: {out:?}");
}

#[test]
fn counter_top_keys_ranks_correctly() {
    let src = "\
import collections
fn main() -> i32:
    c: Dict[str, i64] = collections.counter_new()
    collections.counter_add(c, \"the\", 5i64)
    collections.counter_add(c, \"and\", 3i64)
    collections.counter_add(c, \"cat\", 7i64)
    collections.counter_add(c, \"dog\", 7i64)
    top: List[str] = collections.counter_top_keys(c, 3i32)
    n: i32 = i32(i64(len(top)))
    i: i32 = 0i32
    while i < n:
        println(top[i])
        i = i + 1i32
    return 0
";
    let p = compile_snippet("counter_top_keys", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    // Top 3: cat & dog tied at 7 (alphabetical breaks tie → cat first),
    // then `the` at 5.
    assert_eq!(lines[0], "cat", "got: {out:?}");
    assert_eq!(lines[1], "dog", "got: {out:?}");
    assert_eq!(lines[2], "the", "got: {out:?}");
}

#[test]
fn deque_fifo_order() {
    let src = "\
import collections
fn main() -> i32:
    d: List[i64] = collections.deque_new()
    if collections.deque_is_empty(d):
        println(\"empty start\")
    collections.deque_push_back(d, 10i64)
    collections.deque_push_back(d, 20i64)
    collections.deque_push_back(d, 30i64)
    println(\"len=\" + str(collections.deque_len(d)))
    while not collections.deque_is_empty(d):
        v: i64 = collections.deque_pop_front(d)
        println(\"popped=\" + str(v))
    println(\"done\")
    return 0
";
    let p = compile_snippet("deque_fifo", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("empty start"), "got: {out:?}");
    assert!(out.contains("len=3"), "got: {out:?}");
    // FIFO order.
    let lines: Vec<&str> = out.lines().collect();
    let popped: Vec<&&str> = lines.iter().filter(|l| l.starts_with("popped=")).collect();
    assert_eq!(*popped[0], "popped=10", "got: {out:?}");
    assert_eq!(*popped[1], "popped=20", "got: {out:?}");
    assert_eq!(*popped[2], "popped=30", "got: {out:?}");
    assert!(out.contains("done"), "got: {out:?}");
}

#[test]
fn deque_pop_empty_raises_index_error() {
    let src = "\
import collections
fn main() -> i32:
    d: List[i64] = collections.deque_new()
    try:
        v: i64 = collections.deque_pop_front(d)
        println(\"unexpected: \" + str(v))
    except IndexError as e:
        println(\"caught: \" + e.message)
    return 0
";
    let p = compile_snippet("deque_pop_empty", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught:"), "got: {out:?}");
}

// ── csv module ─────────────────────────────────────────────────────────

#[test]
fn csv_parse_line_handles_quoted_comma() {
    let src = "\
import csv
fn main() -> i32:
    pieces: List[str] = csv.parse_line(\"a,\\\"hello, world\\\",c\")
    n: i32 = i32(i64(len(pieces)))
    i: i32 = 0i32
    while i < n:
        println(pieces[i])
        i = i + 1i32
    return 0
";
    let p = compile_snippet("csv_parse_line", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 3, "got: {out:?}");
    assert_eq!(lines[0], "a");
    assert_eq!(lines[1], "hello, world");
    assert_eq!(lines[2], "c");
}

#[test]
fn csv_parse_line_handles_double_quote_escape() {
    let src = "\
import csv
fn main() -> i32:
    pieces: List[str] = csv.parse_line(\"\\\"a\\\"\\\"b\\\",c\")
    println(\"p0=\" + pieces[0])
    println(\"p1=\" + pieces[1])
    return 0
";
    let p = compile_snippet("csv_dq_escape", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    // The double-quote-escape pattern: `"a""b"` → `a"b`.
    assert!(out.contains("p0=a\"b"), "got: {out:?}");
    assert!(out.contains("p1=c"), "got: {out:?}");
}

#[test]
fn csv_parse_multiline_with_embedded_newline() {
    let src = "\
import csv
fn main() -> i32:
    text: str = \"name,value\\nalice,\\\"line1\\nline2\\\"\\nbob,42\"
    rows: List[List[str]] = csv.parse(text)
    println(\"rows=\" + str(len(rows)))
    return 0
";
    let p = compile_snippet("csv_multiline", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    // 3 rows: header + alice (with embedded \n preserved) + bob.
    assert!(out.contains("rows=3"), "got: {out:?}");
}

#[test]
fn csv_escape_quotes_when_needed() {
    let src = "\
import csv
fn main() -> i32:
    println(csv.escape(\"plain\"))
    println(csv.escape(\"has,comma\"))
    println(csv.escape(\"has\\\"quote\"))
    return 0
";
    let p = compile_snippet("csv_escape", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "plain", "got: {out:?}");
    assert_eq!(lines[1], "\"has,comma\"", "got: {out:?}");
    assert_eq!(lines[2], "\"has\"\"quote\"", "got: {out:?}");
}

#[test]
fn csv_format_row_round_trips() {
    let src = "\
import csv
fn main() -> i32:
    row: List[str] = []
    row.append(\"alice\")
    row.append(\"a,b\")
    row.append(\"plain\")
    println(csv.format_row(row))
    return 0
";
    let p = compile_snippet("csv_format_row", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("alice,\"a,b\",plain"), "got: {out:?}");
}

#[test]
fn csv_write_then_read_round_trips() {
    let dir = tmpdir("rw_round_trip");
    let csv_path = dir.join("data.csv");
    let csv_path_str = csv_path.display().to_string().replace('\\', "\\\\");
    let src = format!("\
import csv
fn main() -> i32:
    rows: List[List[str]] = []
    h: List[str] = []
    h.append(\"k\")
    h.append(\"v\")
    rows.append(h)
    r: List[str] = []
    r.append(\"alpha\")
    r.append(\"a, b\")
    rows.append(r)
    csv.write_file(\"{csv_path_str}\", rows)
    again: List[List[str]] = csv.read_file(\"{csv_path_str}\")
    println(\"rows=\" + str(len(again)))
    println(\"cell=\" + again[1i32][1i32])
    return 0
");
    let p = compile_snippet("csv_write_read", &src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("rows=2"), "got: {out:?}");
    assert!(out.contains("cell=a, b"), "got: {out:?}");
}

#[test]
fn csv_read_file_missing_path_raises_ioerror() {
    let src = "\
import csv
fn main() -> i32:
    try:
        rows: List[List[str]] = csv.read_file(\"/path/that/does/not/exist.csv\")
        println(\"unexpectedly succeeded\")
    except IOError as e:
        println(\"caught: \" + e.message)
    return 0
";
    let p = compile_snippet("csv_missing", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught:"), "got: {out:?}");
}
