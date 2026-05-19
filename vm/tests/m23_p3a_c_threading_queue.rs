//! M23 P3a-C in-process regression tests for the `threading` (Lock +
//! Semaphore) and `queue` (PriorityQueue) stdlib modules.  Follows the
//! M22 P2x convention: compile a snippet, run via `run_file_capture`,
//! assert on stdout.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m23_p3a_c_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── threading.Lock ─────────────────────────────────────────────────────

#[test]
fn lock_new_and_try_acquire_initially_unheld() {
    let src = "\
import threading
fn main() -> i32:
    h: i64 = threading.lock_new()
    got: bool = threading.lock_try_acquire(h)
    println(\"first=\" + str(got))
    # Already held; a second try_acquire should fail.
    again: bool = threading.lock_try_acquire(h)
    println(\"second=\" + str(again))
    threading.lock_release(h)
    return 0
";
    let p = compile_snippet("lock_try_acquire", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("first=true"), "got: {out:?}");
    assert!(out.contains("second=false"), "got: {out:?}");
}

#[test]
fn lock_acquire_release_blocks_correctly_single_thread() {
    let src = "\
import threading
fn main() -> i32:
    h: i64 = threading.lock_new()
    threading.lock_acquire(h)
    threading.lock_release(h)
    threading.lock_acquire(h)
    threading.lock_release(h)
    println(\"ok\")
    return 0
";
    let p = compile_snippet("lock_acquire_release", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("ok"), "got: {out:?}");
}

#[test]
fn lock_release_unheld_raises_runtime_error() {
    let src = "\
import threading
fn main() -> i32:
    h: i64 = threading.lock_new()
    try:
        threading.lock_release(h)
        println(\"no-throw\")
    except RuntimeError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("lock_release_unheld", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught"), "got: {out:?}");
}

#[test]
fn lock_protects_shared_counter_across_threads() {
    // 4 workers × 100 increments under a shared lock → counter == 400.
    let src = "\
from threading import Thread
import threading

fn worker(counter: List[i32], lh: i64) -> None:
    i: i32 = 0
    while i < 100:
        threading.lock_acquire(lh)
        counter[0] = counter[0] + 1
        threading.lock_release(lh)
        i = i + 1

fn main() -> i32:
    counter: List[i32] = [0]
    lh: i64 = threading.lock_new()
    t1: Thread = Thread(fn() -> None: worker(counter, lh))
    t2: Thread = Thread(fn() -> None: worker(counter, lh))
    t3: Thread = Thread(fn() -> None: worker(counter, lh))
    t4: Thread = Thread(fn() -> None: worker(counter, lh))
    t1.start()
    t2.start()
    t3.start()
    t4.start()
    t1.join()
    t2.join()
    t3.join()
    t4.join()
    println(\"counter=\" + str(counter[0]))
    return 0
";
    let p = compile_snippet("lock_counter", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(
        out.contains("counter=400"),
        "lock correctness failed; got: {out:?}"
    );
}

// ── threading.Semaphore ────────────────────────────────────────────────

#[test]
fn semaphore_initial_permits_drain_then_block() {
    let src = "\
import threading
fn main() -> i32:
    s: i64 = threading.semaphore_new(2)
    a: bool = threading.semaphore_try_acquire(s)
    b: bool = threading.semaphore_try_acquire(s)
    # Third try should fail (only 2 permits).
    c: bool = threading.semaphore_try_acquire(s)
    println(\"a=\" + str(a))
    println(\"b=\" + str(b))
    println(\"c=\" + str(c))
    threading.semaphore_release(s)
    d: bool = threading.semaphore_try_acquire(s)
    println(\"d=\" + str(d))
    return 0
";
    let p = compile_snippet("semaphore_drain", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("a=true"), "got: {out:?}");
    assert!(out.contains("b=true"), "got: {out:?}");
    assert!(out.contains("c=false"), "got: {out:?}");
    assert!(out.contains("d=true"), "got: {out:?}");
}

#[test]
fn semaphore_binary_mutex_round_trip() {
    let src = "\
import threading
fn main() -> i32:
    s: i64 = threading.semaphore_new(1)
    threading.semaphore_acquire(s)
    threading.semaphore_release(s)
    threading.semaphore_acquire(s)
    threading.semaphore_release(s)
    println(\"ok\")
    return 0
";
    let p = compile_snippet("semaphore_binary", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("ok"), "got: {out:?}");
}

#[test]
fn semaphore_zero_initial_try_acquire_fails() {
    let src = "\
import threading
fn main() -> i32:
    s: i64 = threading.semaphore_new(0)
    a: bool = threading.semaphore_try_acquire(s)
    println(\"a=\" + str(a))
    threading.semaphore_release(s)
    b: bool = threading.semaphore_try_acquire(s)
    println(\"b=\" + str(b))
    return 0
";
    let p = compile_snippet("semaphore_zero", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("a=false"), "got: {out:?}");
    assert!(out.contains("b=true"), "got: {out:?}");
}

// ── queue.PriorityQueue (i64) ──────────────────────────────────────────

#[test]
fn pq_i64_pushes_pop_in_ascending_priority() {
    let src = "\
import queue
fn main() -> i32:
    pq: i64 = queue.pq_new_i64()
    queue.pq_push_i64(pq, 3.0, 30i64)
    queue.pq_push_i64(pq, 1.0, 10i64)
    queue.pq_push_i64(pq, 2.0, 20i64)
    println(\"len=\" + str(queue.pq_len(pq)))
    a: Tuple[f64, i64] = queue.pq_pop_min_i64(pq)
    b: Tuple[f64, i64] = queue.pq_pop_min_i64(pq)
    c: Tuple[f64, i64] = queue.pq_pop_min_i64(pq)
    println(\"a=\" + str(a.1))
    println(\"b=\" + str(b.1))
    println(\"c=\" + str(c.1))
    println(\"empty=\" + str(queue.pq_is_empty(pq)))
    return 0
";
    let p = compile_snippet("pq_i64_basic", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("len=3"), "got: {out:?}");
    assert!(out.contains("a=10"), "got: {out:?}");
    assert!(out.contains("b=20"), "got: {out:?}");
    assert!(out.contains("c=30"), "got: {out:?}");
    assert!(out.contains("empty=true"), "got: {out:?}");
}

#[test]
fn pq_i64_peek_does_not_pop() {
    let src = "\
import queue
fn main() -> i32:
    pq: i64 = queue.pq_new_i64()
    queue.pq_push_i64(pq, 5.0, 50i64)
    queue.pq_push_i64(pq, 1.0, 10i64)
    p1: Tuple[f64, i64] = queue.pq_peek_min_i64(pq)
    p2: Tuple[f64, i64] = queue.pq_peek_min_i64(pq)
    println(\"p1=\" + str(p1.1))
    println(\"p2=\" + str(p2.1))
    println(\"len-after-peek=\" + str(queue.pq_len(pq)))
    return 0
";
    let p = compile_snippet("pq_peek", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("p1=10"), "got: {out:?}");
    assert!(out.contains("p2=10"), "got: {out:?}");
    assert!(out.contains("len-after-peek=2"), "got: {out:?}");
}

#[test]
fn pq_pop_empty_raises_index_error() {
    let src = "\
import queue
fn main() -> i32:
    pq: i64 = queue.pq_new_i64()
    try:
        x: Tuple[f64, i64] = queue.pq_pop_min_i64(pq)
        println(\"no-throw\")
    except IndexError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("pq_pop_empty", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught"), "got: {out:?}");
}

#[test]
fn pq_peek_empty_raises_index_error() {
    let src = "\
import queue
fn main() -> i32:
    pq: i64 = queue.pq_new_i64()
    try:
        x: Tuple[f64, i64] = queue.pq_peek_min_i64(pq)
        println(\"no-throw\")
    except IndexError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("pq_peek_empty", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught"), "got: {out:?}");
}

#[test]
fn pq_same_priority_ties_break_fifo() {
    // Insertion-order tie-break: two items with priority 5.0 → the
    // first inserted pops first.
    let src = "\
import queue
fn main() -> i32:
    pq: i64 = queue.pq_new_i64()
    queue.pq_push_i64(pq, 5.0, 100i64)
    queue.pq_push_i64(pq, 5.0, 200i64)
    queue.pq_push_i64(pq, 5.0, 300i64)
    a: Tuple[f64, i64] = queue.pq_pop_min_i64(pq)
    b: Tuple[f64, i64] = queue.pq_pop_min_i64(pq)
    c: Tuple[f64, i64] = queue.pq_pop_min_i64(pq)
    println(\"a=\" + str(a.1))
    println(\"b=\" + str(b.1))
    println(\"c=\" + str(c.1))
    return 0
";
    let p = compile_snippet("pq_fifo_ties", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("a=100"), "got: {out:?}");
    assert!(out.contains("b=200"), "got: {out:?}");
    assert!(out.contains("c=300"), "got: {out:?}");
}

#[test]
fn pq_priority_is_returned_in_tuple() {
    let src = "\
import queue
fn main() -> i32:
    pq: i64 = queue.pq_new_i64()
    queue.pq_push_i64(pq, 2.5, 42i64)
    t: Tuple[f64, i64] = queue.pq_pop_min_i64(pq)
    println(\"prio=\" + str(t.0))
    println(\"item=\" + str(t.1))
    return 0
";
    let p = compile_snippet("pq_tuple_prio", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("prio=2.5"), "got: {out:?}");
    assert!(out.contains("item=42"), "got: {out:?}");
}

// ── queue.PriorityQueue (str) ──────────────────────────────────────────

#[test]
fn pq_str_drains_in_priority_order() {
    let src = "\
import queue
fn main() -> i32:
    pq: i64 = queue.pq_new_str()
    queue.pq_push_str(pq, 3.0, \"c\")
    queue.pq_push_str(pq, 1.0, \"a\")
    queue.pq_push_str(pq, 2.0, \"b\")
    a: Tuple[f64, str] = queue.pq_pop_min_str(pq)
    b: Tuple[f64, str] = queue.pq_pop_min_str(pq)
    c: Tuple[f64, str] = queue.pq_pop_min_str(pq)
    println(\"a=\" + a.1)
    println(\"b=\" + b.1)
    println(\"c=\" + c.1)
    return 0
";
    let p = compile_snippet("pq_str_basic", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("a=a"), "got: {out:?}");
    assert!(out.contains("b=b"), "got: {out:?}");
    assert!(out.contains("c=c"), "got: {out:?}");
}

#[test]
fn pq_str_peek_returns_priority_and_item() {
    let src = "\
import queue
fn main() -> i32:
    pq: i64 = queue.pq_new_str()
    queue.pq_push_str(pq, 7.0, \"low\")
    queue.pq_push_str(pq, 0.5, \"top\")
    t: Tuple[f64, str] = queue.pq_peek_min_str(pq)
    println(\"prio=\" + str(t.0))
    println(\"item=\" + t.1)
    println(\"len-after-peek=\" + str(queue.pq_len(pq)))
    return 0
";
    let p = compile_snippet("pq_str_peek", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("prio=0.5"), "got: {out:?}");
    assert!(out.contains("item=top"), "got: {out:?}");
    assert!(out.contains("len-after-peek=2"), "got: {out:?}");
}

#[test]
fn pq_len_and_is_empty_track_size() {
    let src = "\
import queue
fn main() -> i32:
    pq: i64 = queue.pq_new_i64()
    println(\"empty1=\" + str(queue.pq_is_empty(pq)))
    println(\"len1=\" + str(queue.pq_len(pq)))
    queue.pq_push_i64(pq, 1.0, 1i64)
    queue.pq_push_i64(pq, 2.0, 2i64)
    println(\"len2=\" + str(queue.pq_len(pq)))
    println(\"empty2=\" + str(queue.pq_is_empty(pq)))
    return 0
";
    let p = compile_snippet("pq_len", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("empty1=true"), "got: {out:?}");
    assert!(out.contains("len1=0"), "got: {out:?}");
    assert!(out.contains("len2=2"), "got: {out:?}");
    assert!(out.contains("empty2=false"), "got: {out:?}");
}
