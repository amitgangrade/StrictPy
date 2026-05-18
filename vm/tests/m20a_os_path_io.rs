//! M20a regression tests for `os`, `path`, and `io` stdlib modules.
//!
//! Follows the M19 pattern: each test compiles a tiny `.spy` snippet,
//! writes the bytecode to `CARGO_TARGET_TMPDIR`, runs it through
//! `run_file_capture[_with_args]`, and asserts on stdout.
//!
//! Filesystem-mutating tests (mkdir/remove/write_file) build their
//! tempdirs under `CARGO_TARGET_TMPDIR` and clean up on success — no
//! shared global state across tests.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::{run_file_capture, run_file_capture_with_args};

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m20a_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

fn tmpdir(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    p.push(format!("m20a_{name}"));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).expect("create tmpdir");
    p
}

// ── os module ──────────────────────────────────────────────────────────

#[test]
fn os_env_returns_set_value() {
    // We set the var from the test side, then read it from inside the VM
    // to prove the os.env native sees the same process env.  We use an
    // explicit `is none` check rather than `??` because the M10 `??`
    // operator is still incomplete (lowers to `Copy(rhs)` and unconditionally
    // picks the fallback — see ir.rs::Expr::NullCoalesce).
    std::env::set_var("STRICTPY_M20A_TEST_VAR", "hello_from_test");
    let src = "\
import os
fn main() -> i32:
    v: str? = os.env(\"STRICTPY_M20A_TEST_VAR\")
    if v is none:
        println(\"missing\")
    else:
        println(v)
    return 0
";
    let p = compile_snippet("env_set", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("hello_from_test"), "got: {out:?}");
    std::env::remove_var("STRICTPY_M20A_TEST_VAR");
}

#[test]
fn os_env_returns_none_for_unset_var() {
    std::env::remove_var("STRICTPY_M20A_DEFINITELY_UNSET");
    let src = "\
import os
fn main() -> i32:
    v: str? = os.env(\"STRICTPY_M20A_DEFINITELY_UNSET\")
    if v is none:
        println(\"<unset>\")
    else:
        println(\"present\")
    return 0
";
    let p = compile_snippet("env_unset", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("<unset>"), "got: {out:?}");
}

#[test]
fn os_set_env_round_trip() {
    let src = "\
import os
fn main() -> i32:
    os.set_env(\"STRICTPY_M20A_ROUNDTRIP\", \"abc123\")
    v: str? = os.env(\"STRICTPY_M20A_ROUNDTRIP\")
    if v is none:
        println(\"missing\")
    else:
        println(v)
    return 0
";
    let p = compile_snippet("env_roundtrip", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("abc123"), "got: {out:?}");
    std::env::remove_var("STRICTPY_M20A_ROUNDTRIP");
}

#[test]
fn os_getcwd_returns_non_empty_string() {
    let src = "\
import os
fn main() -> i32:
    cwd: str = os.getcwd()
    println(cwd)
    return 0
";
    let p = compile_snippet("getcwd", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    let line = out.lines().next().unwrap_or("").trim();
    assert!(!line.is_empty(), "cwd should be non-empty; got: {out:?}");
}

#[test]
fn os_listdir_lists_known_dir() {
    let dir = tmpdir("listdir_basic");
    fs::write(dir.join("alpha.txt"), b"a").unwrap();
    fs::write(dir.join("beta.txt"), b"b").unwrap();
    let dir_lit = dir.to_string_lossy().replace('\\', "\\\\");
    let src = format!("\
import os
fn main() -> i32:
    entries: List[str] = os.listdir(\"{dir_lit}\")
    n: i64 = i64(len(entries))
    i: i64 = 0
    while i < n:
        println(entries[i])
        i = i + 1
    return 0
");
    let p = compile_snippet("listdir_basic", &src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("alpha.txt"), "missing alpha.txt; got: {out:?}");
    assert!(out.contains("beta.txt"), "missing beta.txt; got: {out:?}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn os_listdir_missing_dir_raises_io_error() {
    let src = "\
import os
fn main() -> i32:
    try:
        entries: List[str] = os.listdir(\"this/path/does/not/exist/anywhere\")
        println(\"no error\")
    except IOError as e:
        println(\"io error: caught\")
    return 0
";
    let p = compile_snippet("listdir_missing", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("io error: caught"), "got: {out:?}");
}

#[test]
fn os_mkdir_and_remove_round_trip() {
    let parent = tmpdir("mkdir_root");
    let target = parent.join("new_subdir");
    let target_lit = target.to_string_lossy().replace('\\', "\\\\");
    let src = format!("\
import os
fn main() -> i32:
    os.mkdir(\"{target_lit}\")
    println(\"mkdir-ok\")
    if os.is_dir(\"{target_lit}\"):
        println(\"is-dir-ok\")
    return 0
");
    let p = compile_snippet("mkdir_basic", &src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("mkdir-ok"), "got: {out:?}");
    assert!(out.contains("is-dir-ok"), "got: {out:?}");
    assert!(target.is_dir(), "tmp subdir missing");
    let _ = fs::remove_dir_all(&parent);
}

#[test]
fn os_remove_deletes_a_file() {
    let dir = tmpdir("remove_file");
    let f = dir.join("doomed.txt");
    fs::write(&f, b"bye").unwrap();
    let f_lit = f.to_string_lossy().replace('\\', "\\\\");
    let src = format!("\
import os
fn main() -> i32:
    os.remove(\"{f_lit}\")
    if os.exists(\"{f_lit}\"):
        println(\"still here\")
    else:
        println(\"gone\")
    return 0
");
    let p = compile_snippet("remove_file", &src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("gone"), "got: {out:?}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn os_exists_is_file_is_dir() {
    let dir = tmpdir("exists_check");
    let f = dir.join("a.txt");
    fs::write(&f, b"x").unwrap();
    let f_lit = f.to_string_lossy().replace('\\', "\\\\");
    let dir_lit = dir.to_string_lossy().replace('\\', "\\\\");
    let src = format!("\
import os
fn main() -> i32:
    println(\"file-exists=\" + str(os.exists(\"{f_lit}\")))
    println(\"file-is-file=\" + str(os.is_file(\"{f_lit}\")))
    println(\"file-is-dir=\" + str(os.is_dir(\"{f_lit}\")))
    println(\"dir-exists=\" + str(os.exists(\"{dir_lit}\")))
    println(\"dir-is-file=\" + str(os.is_file(\"{dir_lit}\")))
    println(\"dir-is-dir=\" + str(os.is_dir(\"{dir_lit}\")))
    return 0
");
    let p = compile_snippet("exists_check", &src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("file-exists=true"), "got: {out:?}");
    assert!(out.contains("file-is-file=true"), "got: {out:?}");
    assert!(out.contains("file-is-dir=false"), "got: {out:?}");
    assert!(out.contains("dir-exists=true"), "got: {out:?}");
    assert!(out.contains("dir-is-file=false"), "got: {out:?}");
    assert!(out.contains("dir-is-dir=true"), "got: {out:?}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn os_read_file_and_write_file_round_trip() {
    let dir = tmpdir("readwrite");
    let target = dir.join("data.txt");
    let target_lit = target.to_string_lossy().replace('\\', "\\\\");
    let src = format!("\
import os
fn main() -> i32:
    os.write_file(\"{target_lit}\", \"hello\\nworld\\n\")
    s: str = os.read_file(\"{target_lit}\")
    println(s)
    return 0
");
    let p = compile_snippet("readwrite", &src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("hello"), "got: {out:?}");
    assert!(out.contains("world"), "got: {out:?}");
    let _ = fs::remove_dir_all(&dir);
}

// ── path module ────────────────────────────────────────────────────────

#[test]
fn path_join_concatenates_components() {
    // The exact separator is OS-dependent; assert on a substring that
    // appears regardless: both parts present, in order.
    let src = "\
import path
fn main() -> i32:
    println(path.join(\"foo\", \"bar\"))
    println(path.join3(\"a\", \"b\", \"c\"))
    return 0
";
    let p = compile_snippet("path_join", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].contains("foo"), "got: {out:?}");
    assert!(lines[0].contains("bar"), "got: {out:?}");
    assert!(lines[1].contains("a"), "got: {out:?}");
    assert!(lines[1].contains("b"), "got: {out:?}");
    assert!(lines[1].contains("c"), "got: {out:?}");
}

#[test]
fn path_dirname_and_basename_work() {
    // Use forward slashes; std::path::Path::parent on Windows handles
    // both / and \ uniformly for parsing.
    let src = "\
import path
fn main() -> i32:
    println(\"dn=\" + path.dirname(\"/a/b/c.txt\"))
    println(\"bn=\" + path.basename(\"/a/b/c.txt\"))
    println(\"bn-bare=\" + path.basename(\"only\"))
    return 0
";
    let p = compile_snippet("dirname_basename", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("bn=c.txt"), "got: {out:?}");
    assert!(out.contains("bn-bare=only"), "got: {out:?}");
    // dirname output uses OS separator; just check it contains "a"
    assert!(out.contains("dn=") && out.lines().any(|l| l.starts_with("dn=") && l.contains('a') && l.contains('b')),
        "got: {out:?}");
}

#[test]
fn path_splitext_handles_ext_and_no_ext() {
    let src = "\
import path
fn main() -> i32:
    p1: Tuple[str, str] = path.splitext(\"a.txt\")
    println(\"1: (\" + p1.0 + \", \" + p1.1 + \")\")
    p2: Tuple[str, str] = path.splitext(\"noext\")
    println(\"2: (\" + p2.0 + \", \" + p2.1 + \")\")
    p3: Tuple[str, str] = path.splitext(\".bashrc\")
    println(\"3: (\" + p3.0 + \", \" + p3.1 + \")\")
    p4: Tuple[str, str] = path.splitext(\"path/to/file.md\")
    println(\"4: (\" + p4.0 + \", \" + p4.1 + \")\")
    return 0
";
    let p = compile_snippet("splitext_cases", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out}");
    assert!(out.contains("1: (a, .txt)"), "got: {out:?}");
    assert!(out.contains("2: (noext, )"), "got: {out:?}");
    assert!(out.contains("3: (.bashrc, )"), "got: {out:?}");
    assert!(out.contains("4: (path/to/file, .md)"), "got: {out:?}");
}

#[test]
fn path_sep_is_legal_separator() {
    let src = "\
import path
fn main() -> i32:
    println(path.sep)
    return 0
";
    let p = compile_snippet("path_sep", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    let line = out.lines().next().unwrap_or("");
    assert!(line == "/" || line == "\\", "got: {out:?}");
}

// ── io module ──────────────────────────────────────────────────────────

#[test]
fn io_write_stdout_does_not_append_newline() {
    let src = "\
import io
fn main() -> i32:
    io.write_stdout(\"abc\")
    io.write_stdout(\"def\")
    println(\"\")
    return 0
";
    let p = compile_snippet("write_stdout", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // The capture buffer should see "abcdef\n" — i.e. abc and def appear
    // on the same line.
    assert!(out.contains("abcdef"), "got: {out:?}");
}

#[test]
fn io_flush_stdout_does_not_crash() {
    // Capturing stdout means the flush is a no-op for the sink, but the
    // native must still succeed.  This is mostly a sanity test.
    let src = "\
import io
fn main() -> i32:
    io.write_stdout(\"before-flush \")
    io.flush_stdout()
    io.write_stdout(\"after-flush\")
    println(\"\")
    return 0
";
    let p = compile_snippet("flush_stdout", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("before-flush after-flush"), "got: {out:?}");
}

#[test]
fn io_write_stderr_does_not_affect_stdout_capture() {
    // We can't easily capture stderr in-process; just verify the native
    // doesn't crash and doesn't leak into stdout.
    let src = "\
import io
fn main() -> i32:
    io.write_stderr(\"diagnostic\\n\")
    io.write_stdout(\"stdout-line\")
    println(\"\")
    return 0
";
    let p = compile_snippet("write_stderr", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("stdout-line"), "got: {out:?}");
    assert!(!out.contains("diagnostic"), "stderr leaked to stdout: {out:?}");
}

// ── compile-time checks ────────────────────────────────────────────────

#[test]
fn unknown_os_attr_is_compile_error() {
    let src = "\
import os
fn main() -> i32:
    println(str(os.no_such_attr_xyz))
    return 0
";
    let r = compile_source("bad_os.spy".into(), src);
    let err = r.err().expect("should fail to compile");
    let msg = format!("{err}");
    assert!(
        msg.contains("no_such_attr_xyz") || msg.contains("no attribute"),
        "got: {msg}"
    );
}

#[test]
fn unknown_path_attr_is_compile_error() {
    let src = "\
import path
fn main() -> i32:
    println(path.no_such_thing)
    return 0
";
    let r = compile_source("bad_path.spy".into(), src);
    let err = r.err().expect("should fail to compile");
    let msg = format!("{err}");
    assert!(
        msg.contains("no_such_thing") || msg.contains("no attribute"),
        "got: {msg}"
    );
}

// ── Combined example sanity (run_file_capture_with_args) ───────────────

#[test]
fn os_path_io_compose_via_sys_argv() {
    // Tiny end-to-end demo: read a path from sys.argv, ask os whether it
    // exists, print the dirname.  Uses three modules in one program.
    let src = "\
import sys
import os
import path
fn main() -> i32:
    if i64(len(sys.argv)) < 2i64:
        println(\"no path\")
        return 0
    p: str = sys.argv[1]
    println(\"e=\" + str(os.exists(p)))
    println(\"d=\" + path.basename(p))
    return 0
";
    let prog = compile_snippet("compose", src);
    let (code, out) = run_file_capture_with_args(
        &prog,
        vec!["myfile.txt".into()],
    ).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("e=false"), "got: {out:?}");
    assert!(out.contains("d=myfile.txt"), "got: {out:?}");
}
