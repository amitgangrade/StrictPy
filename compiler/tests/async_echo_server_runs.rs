//! Subprocess integration test for `examples/async_echo_server.spy`
//! (M32 v0.3 asyncio demo).
//!
//! Spawns the demo as a subprocess (binding a loopback port that the
//! demo prints on its first line of stdout), then connects N clients
//! concurrently — each sending a small payload that the server should
//! echo back via `socket.async_recv` + `socket.async_send`.
//!
//! This is the v0.3 acceptance criterion for M32: "the demo composes
//! multiple async primitives in one program" (brief).  The server's
//! per-client work runs on a `asyncio.spawn_i32` task; the accept
//! itself runs via `socket.async_accept`; the recv/send pair runs via
//! `socket.async_recv` + `socket.async_send`.  All compose against
//! the same `Future[T]` shape.
//!
//! Loopback-only (127.0.0.1) — no public-internet I/O.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn async_echo_server_compiles() {
    let src_path = project_root()
        .join("examples")
        .join("async_echo_server.spy");
    let src = fs::read_to_string(&src_path).expect("read async_echo_server.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile async_echo_server.spy: {e}"));
}

/// Read lines from the subprocess stdout until we see the
/// `bound-port=<N>` line, then return `N`.  Returns `None` if the
/// process exits without printing one.  Capped at a few seconds via
/// `thread::sleep` between non-blocking reads.
fn wait_for_bound_port(buf: &Arc<Mutex<Vec<u8>>>, deadline_ms: u64) -> Option<i32> {
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_millis(deadline_ms) {
        {
            let bytes = buf.lock().unwrap();
            // Look line-by-line.
            let s = String::from_utf8_lossy(&bytes);
            for line in s.lines() {
                if let Some(rest) = line.strip_prefix("bound-port=") {
                    if let Ok(port) = rest.trim().parse::<i32>() {
                        return Some(port);
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    None
}

#[test]
fn async_echo_server_runs_with_three_concurrent_clients() {
    let src_path = project_root()
        .join("examples")
        .join("async_echo_server.spy");
    let src = fs::read_to_string(&src_path).expect("read async_echo_server.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile async_echo_server.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("async_echo_server.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    // Launch the server subprocess.
    let mut child = Command::new(&spy_bin)
        .arg(&spyc_path)
        .current_dir(project_root())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("invoke spy.exe");
    let mut stdout = child.stdout.take().expect("child stdout");

    // Background reader: stream stdout into a shared buffer.
    let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let buf_reader = buf.clone();
    let reader_handle = thread::spawn(move || {
        let mut local = [0u8; 1024];
        loop {
            match stdout.read(&mut local) {
                Ok(0) => break,
                Ok(n) => {
                    buf_reader.lock().unwrap().extend_from_slice(&local[..n]);
                }
                Err(_) => break,
            }
        }
    });

    // Wait until the server announces its bound port.
    let port = match wait_for_bound_port(&buf, 5_000) {
        Some(p) => p,
        None => {
            // Tear down before panicking so the subprocess doesn't linger.
            let _ = child.kill();
            let _ = reader_handle.join();
            panic!(
                "server didn't print bound-port within 5s; stdout so far:\n{}",
                String::from_utf8_lossy(&buf.lock().unwrap())
            );
        }
    };

    // Connect 3 clients concurrently and exchange a payload with each.
    let payloads = ["alpha", "bravo", "charlie"];
    let connect_threads: Vec<_> = payloads
        .iter()
        .map(|&pl| {
            thread::spawn(move || -> String {
                // Small backoff so all three race start the connect at
                // roughly the same time — gives the async_accept
                // multi-future path something to chew on.
                thread::sleep(Duration::from_millis(20));
                let mut sock = TcpStream::connect(("127.0.0.1", port as u16))
                    .unwrap_or_else(|e| panic!("client connect: {e}"));
                sock.set_read_timeout(Some(Duration::from_secs(5))).ok();
                sock.write_all(pl.as_bytes()).expect("client write");
                let mut reply = vec![0u8; pl.len()];
                sock.read_exact(&mut reply).expect("client read");
                String::from_utf8(reply).expect("utf8 reply")
            })
        })
        .collect();

    // Each client should see its own payload echoed back.
    for (handle, expected) in connect_threads.into_iter().zip(payloads.iter()) {
        let got = handle.join().expect("client thread");
        assert_eq!(
            got, *expected,
            "client expected echo {:?}, got {:?}",
            expected, got
        );
    }

    // Wait for the server to finish.  Hard 15s cap so a hung server
    // doesn't time out CI as a "test took forever" failure.
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() < Duration::from_secs(15) => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                break None;
            }
            Err(_) => {
                let _ = child.kill();
                break None;
            }
        }
    };
    let _ = reader_handle.join();

    let stdout = String::from_utf8_lossy(&buf.lock().unwrap()).to_string();
    let status = status.expect("server didn't exit within 15s");
    assert!(
        status.success(),
        "server exit status non-zero; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("clients-served=3"),
        "missing clients-served=3; stdout:\n{stdout}"
    );
    assert!(stdout.contains("done"), "missing done; stdout:\n{stdout}");
}
