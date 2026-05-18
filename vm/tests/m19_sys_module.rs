//! M19 regression tests for `import` + the `sys` module.
//!
//! The orchestration story:
//!   * The parser already accepted `import` / `from ... import` since
//!     M0 (the AST has carried `Module::imports` since day 1), but
//!     pre-M19 the resolver only recognised legacy prelude flat names
//!     (`from threading import Channel`). Truly new modules dropped to
//!     a "no stdlib module named …" error.
//!   * M19 wires resolution, typecheck (`module.attr`), IR lowering
//!     (`CallNative { native_id }`), and four native handlers
//!     (`SysArgv`, `SysExit`, `SysPlatform`, `SysVersion`) for the
//!     v0.2 `sys` surface.
//!   * `VmError::Exit(code)` is a new non-catchable variant — it
//!     walks past any active try/except (`propagate_exception` only
//!     fires on `UncaughtException`) and lands in the CLI driver,
//!     which translates it to a process exit code.
//!
//! Coverage:
//!   (1) `sys.exit(0)` / `sys.exit(42)` round-trip through the CLI as
//!       exit codes 0 / 42.
//!   (2) `sys.platform` prints one of the four legal strings.
//!   (3) `sys.version` prints the pinned v0.2 banner.
//!   (4) `from sys import exit; exit(N)` — bare-name form.
//!   (5) `from sys import argv` — argv visible after the program-path
//!       prefix (argv[0]).
//!   (6) `import sys as s; s.platform` — alias form.
//!   (7) `sys.exit` inside `try ... except Exception:` is NOT caught
//!       (the whole point of the non-catchable design).
//!   (8) `import undefined_module` is a compile-time error.
//!   (9) `sys.no_such_attr` is a compile-time error.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::{run_file_capture, run_file_capture_with_args};

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m19_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── (1) sys.exit codes round-trip ───────────────────────────────────────

#[test]
fn sys_exit_zero_returns_zero() {
    let src = "\
import sys
fn main() -> i32:
    sys.exit(0i32)
    return 0
";
    let p = compile_snippet("exit_zero", src);
    let (code, _out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
}

#[test]
fn sys_exit_forty_two_returns_forty_two() {
    let src = "\
import sys
fn main() -> i32:
    sys.exit(42i32)
    return 0
";
    let p = compile_snippet("exit_42", src);
    let (code, _out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 42);
}

// ── (2) / (3) sys.platform + sys.version ───────────────────────────────

#[test]
fn sys_platform_is_legal_value() {
    let src = "\
import sys
fn main() -> i32:
    println(sys.platform)
    return 0
";
    let p = compile_snippet("platform", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    let line = out.lines().next().unwrap_or("").trim();
    assert!(
        matches!(line, "windows" | "linux" | "macos" | "unknown"),
        "platform line was {line:?}; full output: {out:?}"
    );
}

#[test]
fn sys_version_is_the_pinned_banner() {
    let src = "\
import sys
fn main() -> i32:
    println(sys.version)
    return 0
";
    let p = compile_snippet("version", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(
        out.contains("StrictPy v0.2"),
        "version banner missing; got: {out:?}"
    );
}

// ── (4) bare-name exit via `from sys import exit` ──────────────────────

#[test]
fn from_sys_import_exit_works() {
    let src = "\
from sys import exit
fn main() -> i32:
    exit(7i32)
    return 0
";
    let p = compile_snippet("from_exit", src);
    let (code, _out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 7);
}

// ── (5) `from sys import argv` — bare-name form ────────────────────────

#[test]
fn from_sys_import_argv_works() {
    let src = "\
from sys import argv
fn main() -> i32:
    println(\"argc=\" + str(i64(len(argv))))
    return 0
";
    let p = compile_snippet("from_argv", src);
    let (code, out) = run_file_capture_with_args(
        &p,
        vec!["one".into(), "two".into()],
    ).expect("run");
    assert_eq!(code, 0);
    // argv = [program-path, "one", "two"] → argc=3.
    assert!(out.contains("argc=3"), "expected argc=3; got: {out:?}");
}

// ── (6) `import sys as s` — alias ──────────────────────────────────────

#[test]
fn import_alias_routes_to_same_module() {
    let src = "\
import sys as s
fn main() -> i32:
    println(s.platform)
    return 0
";
    let p = compile_snippet("alias", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    let line = out.lines().next().unwrap_or("").trim();
    assert!(matches!(line, "windows" | "linux" | "macos" | "unknown"));
}

// ── (7) sys.exit inside try/except STILL escapes ───────────────────────

#[test]
fn sys_exit_is_not_caught_by_except_exception() {
    let src = "\
import sys
fn main() -> i32:
    try:
        sys.exit(5i32)
    except Exception as e:
        println(\"caught: \" + e.message)
    println(\"after-try\")
    return 0
";
    let p = compile_snippet("uncatchable_exit", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 5, "Exit must walk past the handler; stdout: {out:?}");
    assert!(!out.contains("caught:"), "handler ran! stdout: {out:?}");
    assert!(!out.contains("after-try"), "post-try ran! stdout: {out:?}");
}

// ── (8) `import undefined_module` is a compile-time error ──────────────

#[test]
fn import_unknown_module_is_compile_error() {
    let src = "\
import this_module_does_not_exist
fn main() -> i32:
    return 0
";
    let r = compile_source("bad_import.spy".into(), src);
    let err = r.err().expect("should fail to compile");
    let msg = format!("{err}");
    assert!(
        msg.contains("no stdlib module") || msg.contains("this_module_does_not_exist"),
        "error message should mention the missing module; got: {msg}"
    );
}

// ── (9) `sys.no_such_attr` is a compile-time error ─────────────────────

#[test]
fn unknown_sys_attribute_is_compile_error() {
    let src = "\
import sys
fn main() -> i32:
    println(sys.no_such_attr_at_all)
    return 0
";
    let r = compile_source("bad_attr.spy".into(), src);
    let err = r.err().expect("should fail to compile");
    let msg = format!("{err}");
    assert!(
        msg.contains("no_such_attr_at_all") || msg.contains("no attribute"),
        "error should mention the bad attribute; got: {msg}"
    );
}
