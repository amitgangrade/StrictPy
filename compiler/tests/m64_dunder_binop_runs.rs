//! WAVE-2 LANE-B acceptance test: binary-operator and comparison dunder
//! dispatch on user-defined classes.
//!
//! Covers:
//!   * `examples/dunder_binop_demo.spy` compiles and runs with the expected
//!     deterministic stdout — `a + b`, `a - b`, `a * k`, `a == b`, `a != b`,
//!     `a < b` all route to the class's dunders, and `is` stays
//!     pointer-identity.
//!   * A class operand missing the needed dunder is a clean compile-time type
//!     error (NOT a silent pointer operation).
//!   * `is` / `is not` never call `__eq__` — verified both by output above and
//!     by a focused program here.
//!   * REGRESSION: Lane-A numeric coercion (`1 + 2.0`, `i32 + i64`,
//!     `/`-as-float) still produces the right runtime values — the class
//!     branch must not perturb primitive arithmetic.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

/// Compile `src` and run it, returning `(exit_code, stdout)`.
fn compile_and_run(name: &str, src: &str) -> (i32, String) {
    let bytes = compile_source(format!("{name}.spy"), src)
        .unwrap_or_else(|e| panic!("compile {name}: {e}"));
    let spyc = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.spyc"));
    fs::write(&spyc, &bytes).expect("write spyc");
    run_file_capture(&spyc).unwrap_or_else(|e| panic!("run {name}: {e}"))
}

#[test]
fn dunder_binop_demo_compiles() {
    let src_path = project_root().join("examples").join("dunder_binop_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read dunder_binop_demo.spy");
    let _ = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile dunder_binop_demo.spy: {e}"));
}

#[test]
fn dunder_binop_demo_runs_with_expected_output() {
    let src_path = project_root().join("examples").join("dunder_binop_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read dunder_binop_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile dunder_binop_demo.spy: {e}"));
    let spyc = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("dunder_binop_demo.spyc");
    fs::write(&spyc, &bytes).expect("write spyc");

    let (code, out) = run_file_capture(&spyc).expect("run dunder_binop_demo");
    assert_eq!(code, 0, "exit code; stdout was:\n{out}");

    let expected = "\
4
6
2
2
3
6
eq
ne
lt
same
distinct
";
    assert_eq!(out, expected, "stdout mismatch:\n{out}");
}

#[test]
fn add_eq_lt_dispatch_to_dunders() {
    // Minimal end-to-end: `+`, `==`, `<` on a class with the dunders compute
    // the dunder result. The program returns 0 only if every operator did the
    // right thing, so a green run proves dispatch.
    let src = "\
final class N:
    v: i64
    fn __init__(self, v: i64) -> None:
        self.v = v
    fn __add__(self, other: N) -> N:
        return N(self.v + other.v)
    fn __eq__(self, other: N) -> bool:
        return self.v == other.v
    fn __lt__(self, other: N) -> bool:
        return self.v < other.v

fn main() -> i32:
    a: N = N(2)
    b: N = N(5)
    c: N = a + b           # __add__  -> 7
    ok: bool = c == N(7)   # __eq__   -> true
    lt: bool = a < b       # __lt__   -> true
    ne: bool = a != b      # synthesised not __eq__ -> true
    if ok and lt and ne and c.v == 7:
        return 0
    return 1
";
    let (code, out) = compile_and_run("dunder_add_eq_lt", src);
    assert_eq!(code, 0, "all dunder dispatches must hold; stdout:\n{out}");
}

#[test]
fn missing_dunder_is_type_error_not_pointer_op() {
    // `Empty` defines no `__add__`. Adding two instances must be REJECTED at
    // compile time (a clean type error), never silently lowered to a pointer
    // arithmetic / IAdd on the two object handles.
    let src = "\
final class Empty:
    v: i64
    fn __init__(self, v: i64) -> None:
        self.v = v

fn main() -> i32:
    a: Empty = Empty(1)
    b: Empty = Empty(2)
    c: Empty = a + b
    return 0
";
    let err = compile_source("missing_add.spy".into(), src)
        .expect_err("a + b on a class with no __add__ must be a type error");
    let msg = format!("{err}");
    assert!(
        msg.contains("__add__") || msg.to_lowercase().contains("add"),
        "error should mention the missing __add__ dunder, got: {msg}"
    );
}

#[test]
fn missing_ordering_dunder_is_type_error() {
    // `__lt__` is present but `__gt__` is NOT synthesised from it — ordering
    // operators each require their own dunder.
    let src = "\
final class M:
    v: i64
    fn __init__(self, v: i64) -> None:
        self.v = v
    fn __lt__(self, other: M) -> bool:
        return self.v < other.v

fn main() -> i32:
    a: M = M(1)
    b: M = M(2)
    if a > b:
        return 1
    return 0
";
    let err = compile_source("missing_gt.spy".into(), src)
        .expect_err("a > b with only __lt__ defined must be a type error");
    let msg = format!("{err}");
    assert!(
        msg.contains("__gt__"),
        "error should mention the missing __gt__ dunder, got: {msg}"
    );
}

#[test]
fn is_stays_pointer_identity_not_eq() {
    // `__eq__` makes two equal-valued instances compare equal, but `is` must
    // remain pointer identity: distinct allocations are `is`-distinct even
    // when `==`-equal.
    let src = "\
final class P:
    v: i64
    fn __init__(self, v: i64) -> None:
        self.v = v
    fn __eq__(self, other: P) -> bool:
        return self.v == other.v

fn main() -> i32:
    a: P = P(1)
    b: P = P(1)
    eq: bool = a == b      # __eq__ -> true (same value)
    same: bool = a is b    # pointer identity -> false (distinct objects)
    notsame: bool = a is not b   # -> true
    selfsame: bool = a is a      # -> true
    if eq and (not same) and notsame and selfsame:
        return 0
    return 1
";
    let (code, out) = compile_and_run("is_identity", src);
    assert_eq!(code, 0, "`is` must stay pointer-identity; stdout:\n{out}");
}

#[test]
fn numeric_coercion_regression_runs() {
    // REGRESSION: the Lane-B class branch must sit strictly BEFORE the numeric
    // coercion path, so mixed-numeric arithmetic is unchanged. We verify the
    // actual runtime values, not just that it compiles. (Operands are typed
    // locals — the canonical Lane-A form — so `i64 + f64`/`i32 + i64`/`/` go
    // through the full widening + true-division lowering.)
    //   * `i64 + f64`   -> f64  (int operand widens to f64)
    //   * `i32 + i64`   -> widens to i64
    //   * `7 / 2`       -> 3.5  (Python-3 true division is always f64)
    //   * `7 // 2`      -> 3    (floor division stays integer)
    let src = "\
fn main() -> i32:
    one: i64 = 1
    two: f64 = 2.0
    a: f64 = one + two
    if a != 3.0:
        return 1
    x: i32 = 1
    y: i64 = 2
    z: i64 = x + y
    if z != 3:
        return 2
    p: i64 = 7
    d: i64 = 2
    q: f64 = p / d
    if q != 3.5:
        return 3
    f: i64 = p // d
    if f != 3:
        return 4
    return 0
";
    let (code, out) = compile_and_run("numeric_regression", src);
    assert_eq!(
        code, 0,
        "numeric coercion must be preserved (exit {code} marks which check failed); stdout:\n{out}"
    );
}
