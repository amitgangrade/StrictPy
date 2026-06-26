//! M17 regression tests: user-code generic free functions.
//!
//! Surface added: `fn id[T](x: T) -> T: return x`, with call-site type-arg
//! inference and per-instantiation monomorphisation. Before M17 the AST
//! already carried `GenericParam` and the parser accepted `[T]`, but type
//! parameters resolved to `Ty::Never` and call sites either rejected the
//! call or silently treated `T` as `Any`.
//!
//! Coverage:
//!   1. Identity `fn id[T](x: T) -> T` over five primitive widths.
//!   2. Two type parameters: `fn first[K, V](p: Tuple[K, V]) -> K`.
//!   3. `List[T]` argument inference: `fn first_of[T](xs: List[T]) -> T`.
//!   4. Transitive monomorphisation: `outer[T](x)` calls `id(x)`.
//!   5. Inference-failure call site emits a typecheck error.
//!   6. `double[T](x: T) -> T` with `+`: works for i32 and str (concat).
//!   7. Tuple type-args: `pair_swap[A, B]` swaps and types correctly.
//!   8. Class type-args: `box[T]` round-trips a user-defined class value.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m17_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── (1) identity across primitive widths ────────────────────────────────

#[test]
fn identity_over_primitives() {
    // The `i64`-typed local-then-pass form is intentional: inside a generic
    // call the typechecker uses `synth_expr` on argument exprs (the
    // expected param type is the unresolved `Ty::Var(0)` until we've solved
    // for T), so a bare int literal defaults to i64. `7i32` explicitly pins
    // T = i32; the `big` i64 local pins T = i64.
    let src = "\
fn id[T](x: T) -> T:
    return x

fn main() -> i32:
    a: i32 = id(7i32)
    println(str(a))
    big: i64 = 900000000000
    b: i64 = id(big)
    println(str(b))
    c: f64 = id(3.14)
    println(str(c))
    d: str = id(\"hi\")
    println(d)
    e: bool = id(true)
    if e:
        println(\"true\")
    else:
        println(\"false\")
    return 0
";
    let p = compile_snippet("identity_primitives", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("7\n"), "i32: {out:?}");
    assert!(out.contains("900000000000\n"), "i64: {out:?}");
    assert!(out.contains("3.14\n"), "f64: {out:?}");
    assert!(out.contains("hi\n"), "str: {out:?}");
    assert!(out.contains("true\n"), "bool: {out:?}");
}

// ── (2) two type parameters: tuple projection ───────────────────────────

#[test]
fn first_of_two_param_tuple() {
    let src = "\
fn first[K, V](p: Tuple[K, V]) -> K:
    return p.0

fn main() -> i32:
    pair: Tuple[i32, str] = (42, \"two\")
    a: i32 = first(pair)
    println(str(a))
    return 0
";
    let p = compile_snippet("first_kv", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("42\n"), "got: {out:?}");
}

// ── (3) List[T] argument ────────────────────────────────────────────────

#[test]
fn first_of_list_t() {
    let src = "\
fn first_of[T](xs: List[T]) -> T:
    return xs[0]

fn main() -> i32:
    ai: List[i32] = [10, 20, 30]
    println(str(first_of(ai)))
    bs: List[str] = [\"x\", \"y\", \"z\"]
    println(first_of(bs))
    return 0
";
    let p = compile_snippet("first_of_list", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("10\n"), "int branch: {out:?}");
    assert!(out.contains("x\n"), "str branch: {out:?}");
}

// ── (4) transitive monomorphisation ─────────────────────────────────────

#[test]
fn transitive_outer_inner() {
    let src = "\
fn id[T](x: T) -> T:
    return x

fn outer[U](v: U) -> U:
    return id(v)

fn main() -> i32:
    a: i32 = outer(7i32)
    println(str(a))
    b: str = outer(\"hello\")
    println(b)
    return 0
";
    let p = compile_snippet("transitive", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("7\n"), "outer i32: {out:?}");
    assert!(out.contains("hello\n"), "outer str: {out:?}");
}

// ── (5) inference failure: same T solved to conflicting concrete types ──

#[test]
fn ambiguous_inference_is_typecheck_error() {
    let src = "\
fn pair[T](a: T, b: T) -> i32:
    return 0

fn main() -> i32:
    return pair(1, \"two\")
";
    let r = compile_source("ambiguous.spy".to_string(), src);
    let err = r.expect_err("expected a typecheck error");
    let msg = format!("{}", err);
    assert!(
        msg.contains("generic") || msg.contains("?T") || msg.contains("doesn't match"),
        "expected generic-call inference error, got: {msg}"
    );
}

// ── (6) instantiation-specific body: T+T over i32 and str ───────────────

#[test]
fn double_specialises_over_i32_and_str() {
    let src = "\
fn double[T](x: T) -> T:
    return x + x

fn main() -> i32:
    a: i32 = double(21i32)
    println(str(a))
    b: str = double(\"ab\")
    println(b)
    return 0
";
    let p = compile_snippet("double_specialised", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("42\n"), "i32: {out:?}");
    assert!(out.contains("abab\n"), "str: {out:?}");
}

// ── (7) generic + tuple: A/B swap ───────────────────────────────────────

#[test]
fn pair_swap_two_type_params() {
    let src = "\
fn pair_swap[A, B](p: Tuple[A, B]) -> Tuple[B, A]:
    return (p.1, p.0)

fn main() -> i32:
    p: Tuple[i32, str] = (7, \"x\")
    q: Tuple[str, i32] = pair_swap(p)
    println(q.0)
    println(str(q.1))
    return 0
";
    let p = compile_snippet("pair_swap", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("x\n"), "first elem: {out:?}");
    assert!(out.contains("7\n"), "second elem: {out:?}");
}

// ── (8) generic over a user class ───────────────────────────────────────

#[test]
fn generic_over_user_class() {
    let src = "\
final class Box:
    v: i32
    fn __init__(self, v: i32) -> None:
        self.v = v

fn box_id[T](x: T) -> T:
    return x

fn main() -> i32:
    b: Box = Box(99)
    c: Box = box_id(b)
    println(str(c.v))
    return 0
";
    let p = compile_snippet("box_id", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "stdout: {out:?}");
    assert!(out.contains("99\n"), "got: {out:?}");
}
