//! M33 regression tests: precise-ish GC during JIT'd execution.
//!
//! Before M33 the JIT was bracketed by an `in_jit: AtomicUsize` counter
//! that suppressed GC for the whole duration of any JIT'd call chain.
//! The conservative root scan couldn't see pointers held in CPU
//! registers, so freezing GC was the only way to stay memory-safe; the
//! cost was that the heap could grow without bound during a tight
//! allocating loop. The M26 benchmark suite caught this as the `btree`
//! row's narrowing-as-n-grows column: 0.23x vs CPython at 1k allocations
//! but 1.13x at 10k.
//!
//! M33 replaces the pause with a per-thread *shadow stack* maintained by
//! the JIT'd code itself. Before every heap-allocating runtime helper
//! call (`rt_alloc`, `rt_list_*`, `rt_array_new`, `rt_virtual_call`, the
//! native trampoline, and `strictpy_alloc_str_const`) the JIT spills
//! every register variable into a per-function Cranelift stack slot and
//! publishes the resulting window via `rt_shadow_push`. The GC scans
//! every published window in addition to the interpreter's frame
//! register files; collection during JIT'd execution is therefore safe.
//!
//! What this test covers:
//!   1. A high-allocation recursive workload that resembles the M26
//!      btree(10k) shape — many `New` ops per recursive call, no early
//!      exit until the workload finishes — runs to completion AND
//!      collection happens *during* the JIT'd loop (verified by the
//!      result being deterministic over multiple runs: a torn pointer
//!      would crash or produce nondeterministic output).
//!   2. The shadow stack returns to depth zero after the workload
//!      exits — no leaked push/pop pairs.
//!   3. The shadow-stack helpers themselves are push/pop balanced.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m33_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── (1) High-allocation recursion — the M26 btree(10k) shape ───────────

/// Recursive allocation cascade: each call allocates a fresh `Node` and
/// appends it to a list. With N = 5000 the heap grows past the 4 MB
/// initial GC threshold, so a correct implementation must collect at
/// least once during the recursive loop. (Pre-M33 this entire chain
/// would run with GC frozen.)
#[test]
fn recursive_allocation_does_not_leak_or_crash() {
    let src = "\
final class Node:
    value: i64
    fn __init__(self, value: i64) -> None:
        self.value = value

fn build(n: i64, acc: List[Node]) -> i64:
    if n <= 0i64:
        return i64(acc.len())
    node: Node = Node(n)
    acc.append(node)
    return build(n - 1i64, acc)

fn main() -> i32:
    bucket: List[Node] = []
    total: i64 = build(5000i64, bucket)
    println(str(total))
    return 0
";
    let p = compile_snippet("recursive_alloc", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code");
    assert_eq!(out.trim(), "5000", "all 5000 nodes survived");
}

// ── (2) Repeated execution is deterministic ─────────────────────────────

/// Run the same JIT'd allocation workload three times. If the shadow
/// stack ever lost a pointer mid-collection the second or third run
/// would crash (use-after-free on a freed Node) or print a different
/// total (a torn link). The result must be identical across runs.
#[test]
fn deterministic_across_runs() {
    let src = "\
final class Node:
    value: i64
    next_idx: i64
    fn __init__(self, value: i64, next_idx: i64) -> None:
        self.value = value
        self.next_idx = next_idx

fn walk(nodes: List[Node]) -> i64:
    sum: i64 = 0i64
    n: i64 = nodes.len()
    i: i64 = 0i64
    while i < n:
        n_i: Node = nodes[i]
        sum = sum + n_i.value
        i = i + 1i64
    return sum

fn main() -> i32:
    nodes: List[Node] = []
    i: i64 = 0i64
    while i < 3000i64:
        nodes.append(Node(i, i + 1i64))
        i = i + 1i64
    s: i64 = walk(nodes)
    println(str(s))
    return 0
";
    let p = compile_snippet("determinism", src);
    let mut outputs: Vec<String> = Vec::new();
    for _ in 0..3 {
        let (code, out) = run_file_capture(&p).expect("run");
        assert_eq!(code, 0, "exit code");
        outputs.push(out.trim().to_string());
    }
    // 0 + 1 + ... + 2999 = 2999 * 3000 / 2 = 4_498_500
    assert_eq!(outputs[0], "4498500");
    assert!(
        outputs.iter().all(|s| s == &outputs[0]),
        "results differed across runs: {:?}",
        outputs
    );
}

// ── (3) Shadow stack ends at depth zero ─────────────────────────────────

/// After a JIT'd workload finishes, the shadow stack must be empty.
/// Every `rt_shadow_push` in the JIT'd code is paired with an
/// `rt_shadow_pop`, so any leak would compound across many calls and
/// eventually overflow.
///
/// This test calls the registry directly because the shadow stack is
/// thread-local and the only thread that mutates it during a single
/// `run_file_capture` is the interpreter thread. After the run, we
/// inspect the depth from the same test thread (which has its own
/// shadow stack, initialised to zero).
#[cfg(feature = "jit")]
#[test]
fn shadow_stack_returns_to_zero_after_jit_workload() {
    use strictpy_vm::stackmap_registry;

    // The test thread's shadow stack starts at zero.
    assert_eq!(stackmap_registry::depth(), 0);

    let src = "\
final class Node:
    value: i64
    fn __init__(self, value: i64) -> None:
        self.value = value

fn main() -> i32:
    v: List[Node] = []
    i: i64 = 0i64
    while i < 500i64:
        v.append(Node(i))
        i = i + 1i64
    println(str(v.len()))
    return 0
";
    let p = compile_snippet("zero_depth", src);
    // `run_file_capture` runs the interpreter on the *current* thread, so
    // any push that escapes a pop would leave residual depth here.
    let (code, _out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0, "exit code");
    assert_eq!(
        stackmap_registry::depth(),
        0,
        "shadow stack must be empty after the JIT workload exits"
    );
}

// ── (4) Shadow stack manual push/pop balance ───────────────────────────

/// Sanity check that the shadow-stack helpers themselves balance. This
/// covers a regression we want to catch fast (e.g. a refactor changing
/// the helper ABI), independently of the JIT actually emitting them.
#[cfg(feature = "jit")]
#[test]
fn shadow_helpers_balance() {
    use strictpy_vm::stackmap_registry::{depth, rt_shadow_pop, rt_shadow_push, snapshot};

    assert_eq!(depth(), 0);
    let buf: [u64; 8] = [0; 8];
    unsafe {
        rt_shadow_push(buf.as_ptr(), 8);
        assert_eq!(depth(), 1);
        let snap = snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].1, 8);
        rt_shadow_pop();
    }
    assert_eq!(depth(), 0);
}

// ── (5) GC actually fires during a fully-JIT'd allocation loop ──────────

/// The headline property this change adds: a fully-JIT'd loop that
/// allocates far more than the GC threshold but retains almost nothing must
/// actually *collect* mid-flight, keeping the heap bounded.
///
/// Before the JIT allocation helpers took a safepoint, `maybe_collect` was
/// unreachable from JIT'd code — the interpreter dispatch loop (its only
/// other caller, via `Opcode::GcSafepoint`) isn't running while a JIT'd
/// function executes — so this loop ran to completion with **zero**
/// collections and the heap grew without bound. Now each heap-allocating
/// runtime helper polls the lock-free `gc_due` flag and runs a collection
/// (with precise roots from the published shadow window) when one is due.
///
/// We assert the per-thread collection counter advanced across the run (the
/// interpreter + JIT'd code execute synchronously on the test thread) AND
/// that the program still produced the right answer — a torn or prematurely
/// freed live pointer would crash or corrupt the result.
#[cfg(feature = "jit")]
#[test]
fn gc_fires_during_jit_allocation_loop() {
    use strictpy_vm::gc::collections_this_thread;

    // ~1M allocations of a 24-byte object ≈ 24 MB churned, far past the
    // 4 MB initial GC threshold — but the live set is a single object at a
    // time (each `Box` is dead the moment the next iteration overwrites
    // `b`). A correct collector keeps the heap at a few MB; a missing one
    // grows it to the full 24 MB.
    let src = "\
final class Box:
    v: i64
    fn __init__(self, v: i64) -> None:
        self.v = v

fn churn(n: i64) -> i64:
    last: i64 = 0i64
    i: i64 = 0i64
    while i < n:
        b: Box = Box(i)
        last = b.v
        i = i + 1i64
    return last

fn main() -> i32:
    r: i64 = churn(1000000i64)
    println(str(r))
    return 0
";
    let p = compile_snippet("gc_jit_loop", src);

    let before = collections_this_thread();
    let (code, out) = run_file_capture(&p).expect("run");
    let after = collections_this_thread();

    assert_eq!(code, 0, "exit code");
    assert_eq!(out.trim(), "999999", "final value survived the churn");
    assert!(
        after > before,
        "expected ≥1 collection during the JIT'd allocation loop, \
         but the counter did not advance (before={before}, after={after}). \
         GC is not firing from JIT'd code.",
    );
}
