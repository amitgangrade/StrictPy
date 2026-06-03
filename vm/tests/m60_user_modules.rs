//! M60 regression tests: user-defined sibling-module imports.
//!
//! Before M60, `import foo` / `from foo import bar` against a sibling
//! `.spy` file errored at resolve time with
//! `E4001 (user-defined modules are v0.3)`. M60 resolves the import graph
//! at compile time and merges every imported module into one namespaced
//! compilation unit, so cross-module calls, classes, and globals work in
//! both the interpreter and the JIT.
//!
//! Coverage:
//!   1. Basic `import foo` + qualified access `foo.fn()` / `foo.CONST`.
//!   2. `from foo import name` named import.
//!   3. Dotted subpackage `import pkg.mod` → `pkg/mod.spy`.
//!   4. Cross-module class instantiation + method call (qualified type).
//!   5. Circular import raises a clear compile error (does not hang).

use std::fs;
use std::path::{Path, PathBuf};

use strictpy_compiler::compile_file;
use strictpy_vm::run_file_capture;

/// Create a fresh, unique scratch directory under the cargo target tmp dir.
fn scratch(tag: &str) -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    dir.push(format!("m60_{tag}"));
    // Start clean so stale fixtures from a previous run never leak in.
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn write(dir: &Path, rel: &str, contents: &str) -> PathBuf {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(&path, contents).expect("write fixture");
    path
}

/// Compile `main` (resolving its sibling imports relative to its dir) and
/// run it, returning (exit_code, stdout).
fn compile_and_run(dir: &Path, main: &Path) -> (i32, String) {
    let out = dir.join("__spycache__").join("main.spyc");
    compile_file(main, &out).unwrap_or_else(|e| panic!("compile error: {e}"));
    run_file_capture(&out).expect("run")
}

// ── (1) basic import + qualified function call + constant ────────────────

#[test]
fn basic_import_qualified_access() {
    let dir = scratch("basic");
    write(
        &dir,
        "mathutil.spy",
        "\
ANSWER: i64 = 42

fn add(a: i64, b: i64) -> i64:
    return a + b
",
    );
    let main = write(
        &dir,
        "main.spy",
        "\
import mathutil

fn main() -> i32:
    println(str(mathutil.add(2, 3)))
    println(str(mathutil.ANSWER))
    return 0
",
    );
    let (code, out) = compile_and_run(&dir, &main);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("5\n"), "qualified call: {out:?}");
    assert!(out.contains("42\n"), "qualified const: {out:?}");
}

// ── (2) from-import named binding ────────────────────────────────────────

#[test]
fn from_import_named() {
    let dir = scratch("from_import");
    write(
        &dir,
        "strutil.spy",
        "\
fn shout(s: str) -> str:
    return s + \"!\"
",
    );
    let main = write(
        &dir,
        "main.spy",
        "\
from strutil import shout

fn main() -> i32:
    println(shout(\"hi\"))
    return 0
",
    );
    let (code, out) = compile_and_run(&dir, &main);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("hi!\n"), "from-import: {out:?}");
}

// ── (3) dotted subpackage import ─────────────────────────────────────────

#[test]
fn dotted_subpackage_import() {
    let dir = scratch("dotted");
    // pkg/ is a package purely by containing a .spy file (no __init__).
    write(
        &dir,
        "pkg/geo.spy",
        "\
fn area(w: i64, h: i64) -> i64:
    return w * h
",
    );
    let main = write(
        &dir,
        "main.spy",
        "\
import pkg.geo

fn main() -> i32:
    println(str(pkg.geo.area(4, 5)))
    return 0
",
    );
    let (code, out) = compile_and_run(&dir, &main);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("20\n"), "dotted import: {out:?}");
}

// ── (4) cross-module class: construct + method + qualified type annot ────

#[test]
fn cross_module_class_instantiation() {
    let dir = scratch("class");
    write(
        &dir,
        "counters.spy",
        "\
final class Counter:
    value: i64
    fn __init__(self, start: i64) -> None:
        self.value = start
    fn bump(self, by: i64) -> i64:
        self.value = self.value + by
        return self.value
    fn get(self) -> i64:
        return self.value
",
    );
    let main = write(
        &dir,
        "main.spy",
        "\
import counters
from counters import Counter

fn main() -> i32:
    # qualified-type annotation + qualified constructor
    a: counters.Counter = counters.Counter(10)
    a.bump(5)
    println(str(a.get()))
    # from-imported class name as a bare type + constructor
    b: Counter = Counter(100)
    b.bump(1)
    println(str(b.get()))
    return 0
",
    );
    let (code, out) = compile_and_run(&dir, &main);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("15\n"), "qualified class: {out:?}");
    assert!(out.contains("101\n"), "from-imported class: {out:?}");
}

// ── (5) circular import → clear error, no hang ───────────────────────────

#[test]
fn circular_import_is_rejected() {
    let dir = scratch("circular");
    write(
        &dir,
        "a.spy",
        "\
from b import beta

fn alpha() -> i64:
    return beta() + 1
",
    );
    write(
        &dir,
        "b.spy",
        "\
from a import alpha

fn beta() -> i64:
    return alpha() + 1
",
    );
    let main = write(
        &dir,
        "main.spy",
        "\
import a

fn main() -> i32:
    println(str(a.alpha()))
    return 0
",
    );
    let out = dir.join("__spycache__").join("main.spyc");
    let err = compile_file(&main, &out).expect_err("circular import must error");
    let msg = format!("{err}");
    assert!(
        msg.contains("circular import") || msg.contains("E4003"),
        "expected a circular-import diagnostic, got: {msg}"
    );
}

// ── (6) internal cross-module call chain stays internal after merge ──────

#[test]
fn internal_call_chain_preserved() {
    let dir = scratch("chain");
    write(
        &dir,
        "m.spy",
        "\
fn square(x: i64) -> i64:
    return mul(x, x)

fn mul(a: i64, b: i64) -> i64:
    return a * b
",
    );
    let main = write(
        &dir,
        "main.spy",
        "\
import m

fn main() -> i32:
    println(str(m.square(6)))
    return 0
",
    );
    let (code, out) = compile_and_run(&dir, &main);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("36\n"), "internal chain: {out:?}");
}
