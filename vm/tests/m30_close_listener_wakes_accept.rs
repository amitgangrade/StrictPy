//! M30 BUG-040 regression: `socket.close_listener` must unblock an
//! in-flight `socket.accept` on another thread.
//!
//! Pre-fix repro (from `docs/thesis/bugs/catalog.md`):
//!
//! 1. Thread A binds a listener.
//! 2. Thread B blocks inside `socket.accept(listener)`.
//! 3. Thread A calls `socket.close_listener(listener)`.
//! 4. Thread B's accept stayed blocked indefinitely, because the
//!    `Arc::clone` in `SocketAccept` kept the underlying `TcpListener`
//!    alive even after `close_listener` `take()`'d the slot.
//!
//! Post-fix: `close_listener` calls `shutdown(fd, SHUT_RDWR)` on the
//! listener's socket before dropping its Arc; the blocked accept then
//! returns with an OS error which the VM surfaces as IOError, which the
//! .spy program catches with `except IOError`.
//!
//! Acceptance: the program below must complete in well under 1s.  Pre-
//! fix it hangs forever (which the test runner's per-test timeout would
//! eventually surface, but we add an explicit wall-clock budget via a
//! watchdog thread so the failure mode is "panic after 5s" instead of
//! "hung CI run").

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{name}.spy"), src)
        .unwrap_or_else(|e| panic!("{name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m30_{name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

/// Run `run_file_capture` on a background thread so the main test
/// thread can enforce a wall-clock budget.  Pre-fix this is critical:
/// the bug manifests as `socket.accept` blocking forever, which without
/// a watchdog would simply hang `cargo test` instead of failing.
fn run_with_watchdog(path: PathBuf, budget: Duration) -> (i32, String) {
    let (tx, rx) = mpsc::channel::<Result<(i32, String), String>>();
    let path_clone = path.clone();
    let handle = thread::spawn(move || {
        let r = run_file_capture(&path_clone)
            .map(|(c, s)| (c, s))
            .map_err(|e| format!("{e:?}"));
        let _ = tx.send(r);
    });
    match rx.recv_timeout(budget) {
        Ok(Ok(pair)) => {
            // The runner thread already sent; join it so we don't leak.
            let _ = handle.join();
            pair
        }
        Ok(Err(e)) => {
            let _ = handle.join();
            panic!("VM run errored: {e}");
        }
        Err(_) => panic!(
            "BUG-040 regression: VM run exceeded {:?} watchdog — \
             `socket.close_listener` did NOT unblock the in-flight \
             `socket.accept` (the canonical pre-fix symptom).",
            budget
        ),
    }
}

#[test]
fn close_listener_unblocks_in_flight_accept() {
    // The .spy program:
    //   * binds a loopback listener on an OS-assigned port,
    //   * spawns a worker thread that blocks inside `socket.accept`,
    //   * sleeps 200ms on the main thread (gives the worker time to
    //     reach the accept syscall),
    //   * calls `socket.close_listener`,
    //   * joins the worker, which should have caught IOError.
    //
    // Expected stdout markers:
    //   "listener-up"        – main thread bound the port
    //   "accept-interrupted" – worker caught IOError as expected
    //   "main-done"          – main thread joined the worker
    //
    // Pre-fix: the worker's `socket.accept` never returns, the `t.join()`
    // on main blocks forever, and the watchdog above fires.
    let src = "\
from threading import Thread
import socket
import time

fn worker(lh: i64, status_cell: List[str]) -> None:
    # Post-fix behaviour: `close_listener` wakes this blocked accept.
    # The wake-up arrives in one of two shapes, both acceptable:
    #   * `accept` returns `Err -> IOError` (POSIX path; the
    #     `shutdown(SHUT_RDWR)` syscall in `close_listener` flips the
    #     listening socket into a no-more-connections state).
    #   * `accept` returns a successful connection from the stdlib's
    #     internal self-connect (Windows path; `shutdown` on Winsock
    #     does NOT wake `accept`, so `close_listener` dials the bound
    #     address to deliver a throwaway connection that satisfies the
    #     pending accept).  User code should drop the connection.
    # The critical property — and what BUG-040 was about — is that
    # `accept` no longer blocks indefinitely.
    try:
        pair: Tuple[i64, str] = socket.accept(lh)
        # Windows path: drop the throwaway connection.
        socket.close(pair.0)
        status_cell[0] = \"accept-woken-via-stub-conn\"
    except IOError as e:
        status_cell[0] = \"accept-interrupted\"
    except Exception as e:
        status_cell[0] = \"UNEXPECTED-EXC=\" + e.message

fn main() -> i32:
    lh: i64 = socket.listen_tcp(\"127.0.0.1\", 0i32, 8i32)
    println(\"listener-up\")

    status: List[str] = [\"<unset>\"]
    t: Thread = Thread(fn() -> None: worker(lh, status))
    t.start()

    # Let the worker thread reach the accept() syscall.  200ms is
    # several orders of magnitude longer than the OS needs to schedule
    # a thread + enter a blocking syscall on loopback.
    time.sleep_ms(200i64)

    socket.close_listener(lh)

    # Pre-fix this join hangs forever; post-fix it returns within ms.
    t.join()

    println(status[0])
    println(\"main-done\")
    return 0
";
    let path = compile_snippet("close_listener_wakes_accept", src);
    // 5-second wall-clock budget — generous enough that a slow CI host
    // never trips it under the post-fix code (the operation completes
    // in low double-digit milliseconds locally), but short enough that
    // a regression surfaces as a test failure rather than a hung run.
    let (code, out) = run_with_watchdog(path, Duration::from_secs(5));
    assert_eq!(code, 0, "VM exit non-zero. stdout:\n{out}");
    assert!(
        out.contains("listener-up\n"),
        "missing 'listener-up' marker. stdout:\n{out}"
    );
    // Either wake mechanism is acceptable — see the .spy worker
    // function's docstring for why both shapes are valid.  What we are
    // testing is that BUG-040 (accept hangs forever) is fixed; the
    // exact wake-up path is platform-dependent.
    let accept_woke = out.contains("accept-interrupted\n")
        || out.contains("accept-woken-via-stub-conn\n");
    assert!(
        accept_woke,
        "BUG-040 regression: accept did not wake after close_listener. \
         stdout:\n{out}"
    );
    assert!(
        out.contains("main-done\n"),
        "missing 'main-done' marker (program did not finish cleanly). \
         stdout:\n{out}"
    );
    assert!(
        !out.contains("UNEXPECTED"),
        "unexpected control-flow in worker: stdout:\n{out}"
    );
}
