//! M23 P3a-A in-process regression tests for the `subprocess` and
//! `pathlib` stdlib modules.  Pattern matches m22_p2d_struct_url.rs:
//! compile a small `.spy` snippet, write the bytecode to
//! `CARGO_TARGET_TMPDIR`, run through `run_file_capture`, assert on
//! stdout.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m23_p3a_a_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── pathlib module (pure functions, fully portable) ────────────────────

#[test]
fn pathlib_stem_strips_last_extension_only() {
    let src = "\
import pathlib
fn main() -> i32:
    println(pathlib.stem(\"archive.tar.gz\"))
    println(pathlib.stem(\"README\"))
    println(pathlib.stem(\".bashrc\"))
    return 0
";
    let p = compile_snippet("pathlib_stem", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // Python pathlib: stem of "a.tar.gz" is "a.tar".  Stem of a leading-
    // dot name is the whole name (no extension).
    assert!(out.contains("archive.tar"), "got: {out:?}");
    assert!(out.contains("README"), "got: {out:?}");
    assert!(out.contains(".bashrc"), "got: {out:?}");
}

#[test]
fn pathlib_suffix_returns_dot_extension() {
    let src = "\
import pathlib
fn main() -> i32:
    println(\"[\" + pathlib.suffix(\"a.txt\") + \"]\")
    println(\"[\" + pathlib.suffix(\"README\") + \"]\")
    println(\"[\" + pathlib.suffix(\"archive.tar.gz\") + \"]\")
    return 0
";
    let p = compile_snippet("pathlib_suffix", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("[.txt]"), "got: {out:?}");
    assert!(out.contains("[]"), "got: {out:?}");
    assert!(out.contains("[.gz]"), "got: {out:?}");
}

#[test]
fn pathlib_with_suffix_replaces_extension() {
    let src = "\
import pathlib
fn main() -> i32:
    println(pathlib.with_suffix(\"report.txt\", \".csv\"))
    println(pathlib.with_suffix(\"README\", \".md\"))
    return 0
";
    let p = compile_snippet("pathlib_with_suffix", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("report.csv"), "got: {out:?}");
    assert!(out.contains("README.md"), "got: {out:?}");
}

#[test]
fn pathlib_parts_splits_path() {
    let src = "\
import pathlib
fn main() -> i32:
    parts: List[str] = pathlib.parts(\"a/b/c\")
    println(str(len(parts)))
    return 0
";
    let p = compile_snippet("pathlib_parts", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("3"), "got: {out:?}");
}

#[test]
fn pathlib_is_absolute_distinguishes_rel_abs() {
    let src = "\
import pathlib
fn main() -> i32:
    println(str(pathlib.is_absolute(\"a/b\")))
    abs: str = pathlib.absolute(\"local.txt\")
    println(str(pathlib.is_absolute(abs)))
    return 0
";
    let p = compile_snippet("pathlib_is_absolute", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // First print: rel → false.  Second print: absolute(p) → true.
    assert!(out.contains("false"), "got: {out:?}");
    assert!(out.contains("true"), "got: {out:?}");
}

#[test]
fn pathlib_read_write_text_round_trips() {
    let src = "\
import pathlib
import os
fn main() -> i32:
    tmp: str = \"_pathlib_test_tmp_rw.txt\"
    pathlib.write_text(tmp, \"hello-pathlib\")
    got: str = pathlib.read_text(tmp)
    println(got)
    os.remove(tmp)
    return 0
";
    let p = compile_snippet("pathlib_read_write_text", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("hello-pathlib"), "got: {out:?}");
}

#[test]
fn pathlib_read_lines_strips_trailing_newline() {
    let src = "\
import pathlib
import os
fn main() -> i32:
    tmp: str = \"_pathlib_test_tmp_rl.txt\"
    pathlib.write_text(tmp, \"a\\nb\\nc\\n\")
    lines: List[str] = pathlib.read_lines(tmp)
    println(str(len(lines)))
    if len(lines) >= 3:
        println(lines[0])
        println(lines[2])
    os.remove(tmp)
    return 0
";
    let p = compile_snippet("pathlib_read_lines", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // 3 lines, not 4 (trailing newline stripped).
    assert!(out.contains("3"), "got: {out:?}");
    assert!(out.contains("a"), "got: {out:?}");
    assert!(out.contains("c"), "got: {out:?}");
}

#[test]
fn pathlib_relative_to_recovers_subpath() {
    let src = "\
import pathlib
fn main() -> i32:
    rel: str = pathlib.relative_to(\"a/b/c\", \"a\")
    println(pathlib.name(rel))
    println(str(pathlib.is_absolute(rel)))
    return 0
";
    let p = compile_snippet("pathlib_relative_to_ok", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // The relative path's basename is "c".
    assert!(out.contains("c"), "got: {out:?}");
    // Relative result is not absolute.
    assert!(out.contains("false"), "got: {out:?}");
}

#[test]
fn pathlib_relative_to_not_subpath_raises_value_error() {
    let src = "\
import pathlib
fn main() -> i32:
    try:
        bad: str = pathlib.relative_to(\"x/y\", \"a/b\")
        println(\"UNREACHED\")
    except ValueError as e:
        println(\"ok\")
    return 0
";
    let p = compile_snippet("pathlib_relative_to_err", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("ok"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

#[test]
fn pathlib_read_text_missing_file_raises_io_error() {
    let src = "\
import pathlib
fn main() -> i32:
    try:
        s: str = pathlib.read_text(\"definitely_not_a_real_file_42424242.txt\")
        println(\"UNREACHED\")
    except IOError as e:
        println(\"ok\")
    return 0
";
    let p = compile_snippet("pathlib_read_text_miss", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("ok"), "got: {out:?}");
}

#[test]
fn pathlib_parent_and_name_split_path() {
    let src = "\
import pathlib
fn main() -> i32:
    println(pathlib.name(\"a/b/c.txt\"))
    p: str = pathlib.parent(\"a/b/c.txt\")
    println(str(len(p)))
    return 0
";
    let p = compile_snippet("pathlib_parent_name", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("c.txt"), "got: {out:?}");
    // parent is "a/b" or "a\\b" → len == 3.
    assert!(out.contains("3"), "got: {out:?}");
}

#[test]
fn pathlib_with_name_replaces_basename() {
    let src = "\
import pathlib
fn main() -> i32:
    out: str = pathlib.with_name(\"a/b/c.txt\", \"new.csv\")
    println(pathlib.name(out))
    return 0
";
    let p = compile_snippet("pathlib_with_name", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("new.csv"), "got: {out:?}");
}

// ── subprocess module (cross-platform shell-based smoke) ───────────────
//
// We use the shell directly (cmd.exe on Windows, sh on Unix) so we
// don't depend on PATH-resolution for raw executable lookups (which
// would need additional special-casing per host).

#[cfg(target_family = "windows")]
fn shell_invoke() -> (&'static str, &'static str) {
    ("cmd.exe", "/c")
}

#[cfg(target_family = "unix")]
fn shell_invoke() -> (&'static str, &'static str) {
    ("sh", "-c")
}

#[test]
fn subprocess_run_captures_stdout() {
    let (shell, flag) = shell_invoke();
    let src = format!(
        "\
import subprocess
fn main() -> i32:
    args: List[str] = [\"{flag}\", \"echo hello-subprocess\"]
    r: Tuple[i32, str, str] = subprocess.run(\"{shell}\", args)
    println(\"exit=\" + str(r.0))
    println(r.1)
    return 0
"
    );
    let p = compile_snippet("subprocess_run_stdout", &src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("exit=0"), "got: {out:?}");
    assert!(out.contains("hello-subprocess"), "got: {out:?}");
}

#[test]
fn subprocess_run_propagates_nonzero_exit() {
    let (shell, flag) = shell_invoke();
    let src = format!(
        "\
import subprocess
fn main() -> i32:
    args: List[str] = [\"{flag}\", \"exit 7\"]
    r: Tuple[i32, str, str] = subprocess.run(\"{shell}\", args)
    println(\"exit=\" + str(r.0))
    return 0
"
    );
    let p = compile_snippet("subprocess_run_nonzero", &src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("exit=7"), "got: {out:?}");
}

#[test]
fn subprocess_spawn_wait_round_trip() {
    let (shell, flag) = shell_invoke();
    let src = format!(
        "\
import subprocess
fn main() -> i32:
    args: List[str] = [\"{flag}\", \"exit 0\"]
    h: i64 = subprocess.spawn(\"{shell}\", args)
    println(\"spawned\")
    code: i32 = subprocess.wait(h)
    println(\"code=\" + str(code))
    return 0
"
    );
    let p = compile_snippet("subprocess_spawn_wait", &src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("spawned"), "got: {out:?}");
    assert!(out.contains("code=0"), "got: {out:?}");
}

#[test]
fn subprocess_spawn_missing_program_raises_io_error() {
    let src = "\
import subprocess
fn main() -> i32:
    try:
        h: i64 = subprocess.spawn(\"definitely_not_a_real_program_42424242\", [])
        println(\"UNREACHED\")
    except IOError as e:
        println(\"ok\")
    return 0
";
    let p = compile_snippet("subprocess_spawn_miss", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("ok"), "got: {out:?}");
}

#[test]
fn subprocess_wait_invalid_handle_raises_io_error() {
    let src = "\
import subprocess
fn main() -> i32:
    try:
        code: i32 = subprocess.wait(99999999i64)
        println(\"UNREACHED\")
    except IOError as e:
        println(\"ok\")
    return 0
";
    let p = compile_snippet("subprocess_wait_bad", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("ok"), "got: {out:?}");
}

#[test]
fn subprocess_run_with_stdin_round_trips_data() {
    // Use a shell command that emits its stdin verbatim:
    //   Windows: `findstr x*` matches any line (acts like cat).
    //   Unix:    `cat` is the obvious choice.
    #[cfg(target_family = "windows")]
    let (shell, flag, pipe_cmd) = ("cmd.exe", "/c", "findstr x*");
    #[cfg(target_family = "unix")]
    let (shell, flag, pipe_cmd) = ("sh", "-c", "cat");
    let src = format!(
        "\
import subprocess
fn main() -> i32:
    args: List[str] = [\"{flag}\", \"{pipe_cmd}\"]
    r: Tuple[i32, str, str] = subprocess.run_with_stdin(\"{shell}\", args, \"piped-line\\n\")
    println(\"exit=\" + str(r.0))
    println(r.1)
    return 0
"
    );
    let p = compile_snippet("subprocess_run_stdin", &src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("exit=0"), "got: {out:?}");
    assert!(out.contains("piped-line"), "got: {out:?}");
}
