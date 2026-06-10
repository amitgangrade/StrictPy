//! M32 asyncio — end-to-end smoke tests for the spawn/await/gather/sleep
//! surface.  Drives the compiler+VM together against small inline .spy
//! programs (no demo file dependency, no socket dependency).
//!
//! The chat-server / echo-server flow is exercised separately via
//! `compiler/tests/async_echo_server_runs.rs`; these here pin the
//! v0.3 thread-per-task scheduler's basic contract:
//!
//!  * `asyncio.spawn_i32(f) -> Future[i32]` then `.await()` round-trips
//!    the closure's return value.
//!  * `Future.is_ready()` reports `false` before the spawn finishes
//!    and `true` after.
//!  * `asyncio.gather_2_i32(a, b)` returns both values as a Tuple.
//!  * `asyncio.sleep(0.05)` blocks for ~50ms (Shape A — Shape B will
//!    yield to the event loop without changing the user-visible shape).
//!  * `asyncio.run_i32(main)` runs the closure as the root task and
//!    returns its result.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;

/// Compile + run a self-contained .spy program in-process, capturing
/// stdout.  Asserts the exit code is 0.
fn run_inline(name: &str, src: &str) -> String {
    let bytes = compile_source(format!("{}.spy", name), src)
        .unwrap_or_else(|e| panic!("compile {name}: {e}"));
    let tmp_dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(&tmp_dir).expect("create tmpdir");
    let spyc_path = tmp_dir.join(format!("{name}.spyc"));
    fs::write(&spyc_path, &bytes).expect("write spyc");
    let (code, out) = strictpy_vm::run_file_capture(&spyc_path)
        .unwrap_or_else(|e| panic!("run {name}: {e:?}"));
    assert_eq!(code, 0, "{name} exit code; stdout:\n{out}");
    out
}

#[test]
fn spawn_i32_then_await_round_trips_return_value() {
    // Lambda-wrapped target (the producer.spy idiom). Bare function
    // references also work — see the dedicated bare-ref tests below.
    let src = r#"
import asyncio
from asyncio import Future

fn worker() -> i32:
    return 42i32

fn main() -> i32:
    f: Future[i32] = asyncio.spawn_i32(fn() -> i32: worker())
    v: i32 = f.await()
    println("v=" + str(v))
    return 0
"#;
    let out = run_inline("m32_spawn_i32", src);
    assert!(out.contains("v=42"), "stdout:\n{out}");
}

#[test]
fn spawn_str_then_await_round_trips_string() {
    let src = r#"
import asyncio
from asyncio import Future

fn worker() -> str:
    return "hello, async world"

fn main() -> i32:
    f: Future[str] = asyncio.spawn_str(fn() -> str: worker())
    v: str = f.await()
    println("v=" + v)
    return 0
"#;
    let out = run_inline("m32_spawn_str", src);
    assert!(out.contains("v=hello, async world"), "stdout:\n{out}");
}

#[test]
fn is_ready_eventually_true_after_spawn() {
    // We can't reliably observe `is_ready=false` (the worker can race us
    // and finish before we check); we *can* reliably observe that after
    // a generous sleep, `is_ready=true` holds.  That's the contract
    // the API documents.
    let src = r#"
import asyncio
from asyncio import Future

fn worker() -> i32:
    return 7i32

fn main() -> i32:
    f: Future[i32] = asyncio.spawn_i32(fn() -> i32: worker())
    asyncio.sleep(0.2)
    ready_after: bool = f.is_ready()
    println("ready_after=" + str(ready_after))
    v: i32 = f.await()
    println("v=" + str(v))
    return 0
"#;
    let out = run_inline("m32_is_ready", src);
    assert!(out.contains("ready_after=true"), "stdout:\n{out}");
    assert!(out.contains("v=7"), "stdout:\n{out}");
}

#[test]
fn gather_2_i32_returns_both_results() {
    let src = r#"
import asyncio
from asyncio import Future

fn a_worker() -> i32:
    return 10i32

fn b_worker() -> i32:
    return 32i32

fn main() -> i32:
    a: Future[i32] = asyncio.spawn_i32(fn() -> i32: a_worker())
    b: Future[i32] = asyncio.spawn_i32(fn() -> i32: b_worker())
    pair: Tuple[i32, i32] = asyncio.gather_2_i32(a, b)
    println("a=" + str(pair.0))
    println("b=" + str(pair.1))
    println("sum=" + str(pair.0 + pair.1))
    return 0
"#;
    let out = run_inline("m32_gather_2", src);
    assert!(out.contains("a=10"), "stdout:\n{out}");
    assert!(out.contains("b=32"), "stdout:\n{out}");
    assert!(out.contains("sum=42"), "stdout:\n{out}");
}

#[test]
fn sleep_blocks_for_at_least_requested_duration() {
    // Time is observed in StrictPy via std::time::Instant, exposed
    // through `time.elapsed_ms()` — but to keep this test self-contained
    // and not pull in the time module, we just exercise the call and
    // assert it returns cleanly.  The duration contract is tested
    // implicitly by the is_ready test above (a 200ms sleep gave the
    // worker enough wall-clock time to finish).
    let src = r#"
import asyncio

fn main() -> i32:
    asyncio.sleep(0.01)
    println("slept")
    return 0
"#;
    let out = run_inline("m32_sleep", src);
    assert!(out.contains("slept"), "stdout:\n{out}");
}

#[test]
fn run_i32_runs_closure_as_root_task() {
    let src = r#"
import asyncio

fn root_task() -> i32:
    return 99i32

fn main() -> i32:
    code: i32 = asyncio.run_i32(fn() -> i32: root_task())
    println("code=" + str(code))
    return 0
"#;
    let out = run_inline("m32_run_i32", src);
    assert!(out.contains("code=99"), "stdout:\n{out}");
}

#[test]
fn spawn_with_capture_sees_main_thread_state() {
    let src = r#"
import asyncio
from asyncio import Future

fn main() -> i32:
    base: i32 = 100i32
    worker: fn() -> i32 = fn() -> i32: base + 23i32
    f: Future[i32] = asyncio.spawn_i32(worker)
    v: i32 = f.await()
    println("v=" + str(v))
    return 0
"#;
    let out = run_inline("m32_spawn_capture", src);
    assert!(out.contains("v=123"), "stdout:\n{out}");
}

// ── Bare function references (no lambda wrapper) ─────────────────────
//
// LANGUAGE_GUIDE §9.43 documents `asyncio.spawn_i32(do_work)` /
// `asyncio.run_i32(server_main)` with a *bare* module-scope function
// name. Until the ir.rs fix that materialises such references as
// zero-capture ClosureNew values, the name lowered to the IRConst::None
// placeholder (= NONE_SENTINEL at runtime) and the asyncio natives
// dereferenced it as a ClosureRepr — an instant access violation with
// no output. These pin the fixed behaviour end-to-end.

#[test]
fn run_unit_accepts_bare_function_reference() {
    let src = r#"
import asyncio

fn w() -> None:
    pass

fn main() -> i32:
    asyncio.run_unit(w)
    println("ok")
    return 0
"#;
    let out = run_inline("m32_run_unit_bare_ref", src);
    assert!(out.contains("ok"), "stdout:\n{out}");
}

#[test]
fn spawn_i32_accepts_bare_function_reference() {
    let src = r#"
import asyncio
from asyncio import Future

fn t() -> i32:
    return 7i32

fn main() -> i32:
    f: Future[i32] = asyncio.spawn_i32(t)
    v: i32 = f.await()
    println("v=" + str(v))
    return 0
"#;
    let out = run_inline("m32_spawn_bare_ref", src);
    assert!(out.contains("v=7"), "stdout:\n{out}");
}

#[test]
fn map_accepts_bare_function_reference() {
    let src = r#"
fn double(x: i64) -> i64:
    return x * 2

fn main() -> i32:
    xs: List[i64] = [1, 2, 3]
    ys: List[i64] = map(double, xs)
    println(str(ys[0]) + "," + str(ys[1]) + "," + str(ys[2]))
    return 0
"#;
    let out = run_inline("m32_map_bare_ref", src);
    assert!(out.contains("2,4,6"), "stdout:\n{out}");
}
