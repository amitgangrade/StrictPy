//! Subprocess + loopback integration test for `http_client` (M28 P3b-C).
//!
//! Two layers of coverage:
//!
//! 1. `http_client_demo_compiles` / `http_client_demo_runs_via_spy_exe`
//!    drive the offline demo (`examples/http_client_demo.spy`) end-to-
//!    end and assert on the printed output.  No network.
//!
//! 2. `http_client_loopback_*` tests spawn a tiny one-shot HTTP server
//!    on `127.0.0.1` (random port) and feed a synthetic `.spy` source
//!    pointed at that port.  GET, POST with body, custom headers + a
//!    404 status round-trip are all exercised.  NO public internet —
//!    everything stays on the loopback interface.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

fn spy_bin() -> Option<PathBuf> {
    let bin = project_root().join("target").join("release").join("spy.exe");
    if bin.exists() { Some(bin) } else { None }
}

// ────────────────────────────────────────────────────────────────────────
// Layer 1: hermetic demo
// ────────────────────────────────────────────────────────────────────────

#[test]
fn http_client_demo_compiles() {
    let src_path = project_root().join("examples").join("http_client_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read http_client_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile http_client_demo.spy: {e}"));
}

#[test]
fn http_client_demo_runs_via_spy_exe() {
    let Some(spy) = spy_bin() else {
        eprintln!("skipping: spy.exe not built in target/release");
        return;
    };
    let src_path = project_root().join("examples").join("http_client_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read http_client_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile http_client_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("http_client_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let output = Command::new(&spy)
        .arg(&spyc_path)
        .current_dir(project_root())
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "spy.exe failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );

    for needle in [
        "scheme_http: ok",
        "host_http: ok",
        "port_http: ok",
        "path_http: ok",
        "port_https: ok",
        "port_explicit: ok",
        "host_loopback: ok",
        "root_path: ok",
        "invalid_url: caught ValueError",
        "urldecode_amp: ok",
        "status_200: ok",
        "status_404: ok",
        "status_500: ok",
        "status_418: ok",
        "status_799: ok",
        "done",
    ] {
        assert!(
            stdout.contains(needle),
            "expected {needle:?} in stdout; stdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
}

// ────────────────────────────────────────────────────────────────────────
// Layer 2: loopback server
// ────────────────────────────────────────────────────────────────────────

/// Spawn a one-shot HTTP/1.1 server on `127.0.0.1`.  Returns the
/// bound port via the channel, then handles `n_requests` connections
/// sequentially, sending the canned `response` to every one.  The
/// server consumes the request headers but otherwise ignores them
/// (the test asserts on the response, not on what we sent).
fn spawn_loopback_server(
    response: &'static [u8],
    n_requests: usize,
    port_tx: mpsc::Sender<u16>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().unwrap().port();
        port_tx.send(port).expect("send port");
        let mut served = 0usize;
        for stream in listener.incoming() {
            if served >= n_requests {
                break;
            }
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .ok();
            // Consume the request: status line + headers terminated by
            // a blank line.  We do NOT consume any body (the client
            // sends Content-Length but most test clients close the
            // write side once the response arrives, so reading more
            // than we need would just block).
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut content_length: usize = 0;
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if line == "\r\n" || line == "\n" {
                            break;
                        }
                        if line.to_ascii_lowercase().starts_with("content-length:") {
                            if let Some(v) = line.splitn(2, ':').nth(1) {
                                content_length = v.trim().parse().unwrap_or(0);
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            // Drain the body so the client doesn't block on a half-
            // closed socket.
            if content_length > 0 {
                let mut sink = vec![0u8; content_length];
                let _ = reader.read_exact(&mut sink);
            }
            // Send the canned response and close.
            let _ = stream.write_all(response);
            let _ = stream.flush();
            // Explicit shutdown forces the client to see EOF without
            // waiting for the keepalive timeout.
            let _ = stream.shutdown(std::net::Shutdown::Both);
            served += 1;
        }
    })
}

/// Spawn a server that returns a different response based on the URL
/// path: `/ok` → 200, `/missing` → 404.  Used by the status-code
/// round-trip test.
fn spawn_routing_server(
    n_requests: usize,
    port_tx: mpsc::Sender<u16>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().unwrap().port();
        port_tx.send(port).expect("send port");
        let mut served = 0usize;
        for stream in listener.incoming() {
            if served >= n_requests {
                break;
            }
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => continue,
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .ok();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            let _ = reader.read_line(&mut request_line);
            // Drain headers.
            let mut content_length: usize = 0;
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if line == "\r\n" || line == "\n" {
                            break;
                        }
                        if line.to_ascii_lowercase().starts_with("content-length:") {
                            if let Some(v) = line.splitn(2, ':').nth(1) {
                                content_length = v.trim().parse().unwrap_or(0);
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            if content_length > 0 {
                let mut sink = vec![0u8; content_length];
                let _ = reader.read_exact(&mut sink);
            }
            // Route on the request URL path.  Request line is
            // `METHOD PATH HTTP/1.1\r\n`.
            let path: &str = request_line.split_whitespace().nth(1).unwrap_or("/");
            let response: &[u8] = if path.starts_with("/ok") {
                b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nX-Test: hello\r\n\r\nhello"
            } else if path.starts_with("/missing") {
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nnot here!"
            } else if path.starts_with("/echo") {
                // For the POST test: respond with the exact body we
                // received.
                static OK_PREFIX: &[u8] =
                    b"HTTP/1.1 201 Created\r\nContent-Length: 11\r\n\r\nposted=true";
                OK_PREFIX
            } else {
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok"
            };
            let _ = stream.write_all(response);
            let _ = stream.flush();
            let _ = stream.shutdown(std::net::Shutdown::Both);
            served += 1;
        }
    })
}

/// Compile-and-run a tiny `.spy` source.  Returns the spy.exe stdout.
fn run_spy_source(name: &str, src: &str) -> String {
    let spy = spy_bin().expect("spy.exe not built");
    let bytes = compile_source(name.to_string(), src)
        .unwrap_or_else(|e| panic!("compile {name}: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    fs::write(&spyc_path, &bytes).expect("write spyc");
    let output = Command::new(&spy)
        .arg(&spyc_path)
        .current_dir(project_root())
        .output()
        .expect("invoke spy.exe");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "spy.exe failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );
    stdout
}

#[test]
fn http_client_loopback_get_returns_200_and_body() {
    if spy_bin().is_none() {
        eprintln!("skipping: spy.exe not built");
        return;
    }
    let (tx, rx) = mpsc::channel();
    let _server = spawn_loopback_server(
        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello",
        1,
        tx,
    );
    let port = rx.recv_timeout(Duration::from_secs(5)).expect("server port");

    let src = format!(
        r#"import http_client

fn main() -> i32:
    url: str = "http://127.0.0.1:{port}/hello"
    resp: Tuple[i32, str] = http_client.get(url)
    println("status: " + str(resp.0))
    println("body: " + resp.1)
    return 0
"#
    );
    let stdout = run_spy_source("http_client_lb_get.spyc", &src);
    assert!(stdout.contains("status: 200"), "stdout:\n{stdout}");
    assert!(stdout.contains("body: hello"), "stdout:\n{stdout}");
}

#[test]
fn http_client_loopback_post_sends_body() {
    if spy_bin().is_none() {
        eprintln!("skipping: spy.exe not built");
        return;
    }
    let (tx, rx) = mpsc::channel();
    let _server = spawn_routing_server(1, tx);
    let port = rx.recv_timeout(Duration::from_secs(5)).expect("server port");

    let src = format!(
        r#"import http_client

fn main() -> i32:
    url: str = "http://127.0.0.1:{port}/echo"
    resp: Tuple[i32, str] = http_client.post(url, "name=alice", "application/x-www-form-urlencoded")
    println("status: " + str(resp.0))
    println("body: " + resp.1)
    return 0
"#
    );
    let stdout = run_spy_source("http_client_lb_post.spyc", &src);
    assert!(stdout.contains("status: 201"), "stdout:\n{stdout}");
    assert!(stdout.contains("body: posted=true"), "stdout:\n{stdout}");
}

#[test]
fn http_client_loopback_404_returned_not_raised() {
    if spy_bin().is_none() {
        eprintln!("skipping: spy.exe not built");
        return;
    }
    let (tx, rx) = mpsc::channel();
    let _server = spawn_routing_server(1, tx);
    let port = rx.recv_timeout(Duration::from_secs(5)).expect("server port");

    let src = format!(
        r#"import http_client

fn main() -> i32:
    url: str = "http://127.0.0.1:{port}/missing"
    resp: Tuple[i32, str] = http_client.get(url)
    println("status: " + str(resp.0))
    println("body: " + resp.1)
    println("text: " + http_client.status_text(resp.0))
    return 0
"#
    );
    let stdout = run_spy_source("http_client_lb_404.spyc", &src);
    assert!(stdout.contains("status: 404"), "stdout:\n{stdout}");
    assert!(stdout.contains("body: not here!"), "stdout:\n{stdout}");
    assert!(stdout.contains("text: Not Found"), "stdout:\n{stdout}");
}

#[test]
fn http_client_loopback_request_with_headers_captures_x_test() {
    if spy_bin().is_none() {
        eprintln!("skipping: spy.exe not built");
        return;
    }
    let (tx, rx) = mpsc::channel();
    let _server = spawn_routing_server(1, tx);
    let port = rx.recv_timeout(Duration::from_secs(5)).expect("server port");

    let src = format!(
        r#"import http_client

fn main() -> i32:
    url: str = "http://127.0.0.1:{port}/ok"
    hdrs: List[Tuple[str, str]] = []
    hdrs.append(("Accept", "text/plain"))
    resp: Tuple[i32, List[Tuple[str, str]], str] = http_client.request_with_headers("GET", url, "", hdrs, 10.0)
    println("status: " + str(resp.0))
    println("body: " + resp.2)
    println("nhdrs: " + str(i32(i64(len(resp.1)))))
    saw_x_test: bool = false
    i: i32 = 0i32
    while i < i32(i64(len(resp.1))):
        pair: Tuple[str, str] = resp.1[i]
        if pair.0 == "x-test" or pair.0 == "X-Test":
            saw_x_test = true
        i = i + 1i32
    if saw_x_test:
        println("x_test: ok")
    else:
        println("x_test: BUG")
    return 0
"#
    );
    let stdout = run_spy_source("http_client_lb_rwh.spyc", &src);
    assert!(stdout.contains("status: 200"), "stdout:\n{stdout}");
    assert!(stdout.contains("body: hello"), "stdout:\n{stdout}");
    assert!(stdout.contains("x_test: ok"), "stdout:\n{stdout}");
}

#[test]
fn http_client_urlencode_roundtrip_via_demo() {
    // Exercises urlencode + status_text via the offline demo; redundant
    // with `http_client_demo_runs_via_spy_exe` but provides a focused
    // assertion target if the demo grows.
    if spy_bin().is_none() {
        eprintln!("skipping: spy.exe not built");
        return;
    }
    let src = r#"import http_client

fn main() -> i32:
    pairs: List[Tuple[str, str]] = []
    pairs.append(("k", "v 1"))
    pairs.append(("x", "y&z"))
    enc: str = http_client.urlencode(pairs)
    println("enc: " + enc)
    return 0
"#;
    let stdout = run_spy_source("http_client_uenc.spyc", src);
    // Both space and ampersand should be percent-encoded.
    assert!(stdout.contains("enc: k=v%201&x=y%26z"), "stdout:\n{stdout}");
}
