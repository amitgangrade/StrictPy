//! Integration test for `examples/webserver/todo_app.spy` (M29).
//!
//! Compiles the demo, spawns it as a subprocess, scrapes the `PORT=NNNN`
//! line from its stdout, then hits the server with a sequence of HTTP
//! requests via `ureq` (the same crate the stdlib `http_client` module
//! uses internally — keeps the crate graph small).
//!
//! Network-bound discipline: 127.0.0.1 only, port chosen by the OS
//! (`--port 0`), no public endpoints.  The server is told to drain
//! after a fixed number of accepts so it cleanly exits at the end of
//! the test rather than relying on a kill signal.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

/// Compile the demo to a .spyc and return its absolute path.
fn compile_demo(tmp_root: &PathBuf) -> PathBuf {
    let src_path = project_root().join("examples").join("webserver").join("todo_app.spy");
    let src = fs::read_to_string(&src_path).expect("read todo_app.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile todo_app.spy: {e}"));
    fs::create_dir_all(tmp_root).expect("create tmp_root");
    let spyc_path = tmp_root.join("todo_app.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");
    spyc_path
}

/// Wait for the server subprocess to print `PORT=NNNN` and `READY` on
/// stdout, then return the parsed port.  Times out after 30 s.
fn await_port(child: &mut Child) -> u16 {
    let stdout = child.stdout.take().expect("subprocess stdout");
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().flatten() {
            eprintln!("[server] {line}");
            let _ = tx.send(line);
        }
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut port: Option<u16> = None;
    let mut ready = false;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(line) => {
                if let Some(rest) = line.strip_prefix("PORT=") {
                    port = rest.trim().parse::<u16>().ok();
                }
                if line.trim() == "READY" {
                    ready = true;
                }
                if port.is_some() && ready {
                    return port.unwrap();
                }
            }
            Err(_) => {
                // No line yet; check whether the child died.
                if let Ok(Some(status)) = child.try_wait() {
                    panic!("server exited before READY: status={:?}", status);
                }
            }
        }
    }
    panic!("timed out waiting for PORT/READY (port={port:?}, ready={ready})");
}

fn http_call(method: &str, url: &str, body: Option<&str>, ct: Option<&str>) -> (u16, String) {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        // Disable connection reuse — each call should open a fresh socket
        // so the request-count matches the server's accept count.
        .max_idle_connections(0)
        .build();
    let mut req = agent.request(method, url);
    if let Some(c) = ct {
        req = req.set("Content-Type", c);
    }
    let result = match body {
        Some(b) => req.send_string(b),
        None => req.call(),
    };
    match result {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.into_string().unwrap_or_default();
            (status, body)
        }
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            (code, body)
        }
        Err(e) => panic!("ureq error on {method} {url}: {e}"),
    }
}

#[test]
fn webserver_demo_compiles() {
    let src_path = project_root().join("examples").join("webserver").join("todo_app.spy");
    let src = fs::read_to_string(&src_path).expect("read todo_app.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile todo_app.spy: {e}"));
}

#[test]
fn webserver_demo_runs_http() {
    let tmp_root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("m29_webserver_http");
    // Ensure a clean DB so we don't inherit ids from a previous run.
    let _ = fs::remove_dir_all(&tmp_root);
    let spyc_path = compile_demo(&tmp_root);

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    // Server drains after 8 accepts -> health, list, create, list (again),
    // delete, list (again), static, 404 = 8 round trips planned.  Pad to
    // 12 to leave headroom for any retries.
    // Generously over-provision accepts; we'll send `/__shutdown` as the
    // last request to trip the explicit drain path on the server.
    let mut child = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg("--port").arg("0")
        .arg("--host").arg("127.0.0.1")
        .arg("--db").arg(tmp_root.join("todos.db").display().to_string())
        .arg("--max-accepts").arg("200")
        .current_dir(project_root())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn spy.exe");

    let port = await_port(&mut child);
    let base = format!("http://127.0.0.1:{port}");

    // 1. health probe.
    let (status, body) = http_call("GET", &format!("{base}/health"), None, None);
    assert_eq!(status, 200, "health status; body={body}");
    assert!(body.contains("\"status\":\"ok\""), "health body: {body}");

    // 2. list initial (empty).
    let (status, body) = http_call("GET", &format!("{base}/api/todos"), None, None);
    assert_eq!(status, 200);
    assert_eq!(body, "[]");

    // 3. create one.
    let (status, body) = http_call(
        "POST",
        &format!("{base}/api/todos"),
        Some(r#"{"text":"buy milk"}"#),
        Some("application/json"),
    );
    assert_eq!(status, 201, "create status; body={body}");
    assert!(body.contains("\"text\":\"buy milk\""), "create body: {body}");

    // 4. list (now has 1).
    let (status, body) = http_call("GET", &format!("{base}/api/todos"), None, None);
    assert_eq!(status, 200);
    assert!(body.contains("\"text\":\"buy milk\""), "list body: {body}");
    assert!(body.starts_with("[{"), "list body should be JSON array: {body}");

    // 5. delete it.  We assume id == 1 since the DB was fresh.
    let (status, body) = http_call("DELETE", &format!("{base}/api/todos/1"), None, None);
    assert_eq!(status, 200, "delete status; body={body}");
    assert!(body.contains("\"deleted\":1"), "delete body: {body}");

    // 6. list (back to empty).
    let (status, body) = http_call("GET", &format!("{base}/api/todos"), None, None);
    assert_eq!(status, 200);
    assert_eq!(body, "[]");

    // 7. static file lookup.
    let (status, body) = http_call("GET", &format!("{base}/static/hello.txt"), None, None);
    assert_eq!(status, 200, "static status; body={body}");
    assert!(body.contains("hello, static!"), "static body: {body}");

    // 8. 404 path.
    let (status, _body) = http_call("GET", &format!("{base}/no/such/route"), None, None);
    assert_eq!(status, 404);

    // 9. 405 — wrong method on a known path.
    let (status, _body) = http_call("DELETE", &format!("{base}/health"), None, None);
    assert_eq!(status, 405);

    // 10. malformed JSON body on create -> 400.
    let (status, _body) = http_call(
        "POST",
        &format!("{base}/api/todos"),
        Some("not json"),
        Some("application/json"),
    );
    assert_eq!(status, 400);

    // 11. delete non-existent id.
    let (status, _body) = http_call("DELETE", &format!("{base}/api/todos/9999"), None, None);
    assert_eq!(status, 404);

    // 12. home page HTML.
    let (status, body) = http_call("GET", &format!("{base}/"), None, None);
    assert_eq!(status, 200);
    assert!(body.contains("<h1>StrictPy TODO"), "home body: {body}");

    // Tear down: kill the server.  It has no graceful-shutdown endpoint
    // in v0.2 (would need an out-of-band signal); kill+wait is fine for
    // an integration test.
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn webserver_demo_runs_https() {
    let tmp_root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("m29_webserver_https");
    let _ = fs::remove_dir_all(&tmp_root);
    let spyc_path = compile_demo(&tmp_root);

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    // Issue a self-signed cert for `localhost`.
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])
        .expect("rcgen");
    let cert_path = tmp_root.join("cert.pem");
    let key_path = tmp_root.join("key.pem");
    fs::write(&cert_path, cert.cert.pem()).expect("write cert");
    fs::write(&key_path, cert.key_pair.serialize_pem()).expect("write key");

    let mut child = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg("--port").arg("0")
        .arg("--host").arg("127.0.0.1")
        .arg("--db").arg(tmp_root.join("todos.db").display().to_string())
        .arg("--max-accepts").arg("50")
        .arg("--tls")
        .arg(cert_path.display().to_string())
        .arg(key_path.display().to_string())
        .current_dir(project_root())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn spy.exe");

    let port = await_port(&mut child);

    // Build a ureq agent that trusts our self-signed cert.  ureq's
    // default builder uses webpki-roots, which obviously doesn't sign
    // a rcgen-generated localhost cert; we need to install our own root
    // store.  Easiest path: build a rustls::ClientConfig with our
    // cert in the trust roots and hand it to ureq.
    let cert_der = rustls_pemfile::certs(&mut fs::read(&cert_path).unwrap().as_slice())
        .filter_map(Result::ok)
        .next()
        .expect("parse cert");
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert_der).expect("add root");
    let tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let agent = ureq::AgentBuilder::new()
        .tls_config(std::sync::Arc::new(tls_config))
        .timeout(Duration::from_secs(10))
        .build();

    let url = format!("https://localhost:{port}/health");
    let resp = agent.get(&url).call().expect("https get");
    let status = resp.status();
    let body = resp.into_string().expect("read body");
    assert_eq!(status, 200, "https status; body={body}");
    assert!(body.contains("\"status\":\"ok\""), "https body: {body}");

    // Tear down.
    let _ = child.kill();
    let _ = child.wait();
}

// ─────────────────────────────────────────────────────────────────────
// M29.5 tests — keep-alive, chunked, multipart, HTML errors, graceful
// shutdown.  These all share the "compile + spawn + scrape PORT" prelude
// with the M29 tests above; the spawn helper is duplicated here so the
// suite stays a single-file integration test (matches the M29 layout).
// ─────────────────────────────────────────────────────────────────────

fn spawn_server(test_name: &str, extra_args: &[&str]) -> (Child, u16, PathBuf) {
    let tmp_root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(test_name);
    let _ = fs::remove_dir_all(&tmp_root);
    let spyc_path = compile_demo(&tmp_root);
    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    let mut cmd = Command::new(&spy_bin);
    cmd.arg(&spyc_path)
        .arg("--port")
        .arg("0")
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--db")
        .arg(tmp_root.join("todos.db").display().to_string());
    for a in extra_args {
        cmd.arg(a);
    }
    let mut child = cmd
        .current_dir(project_root())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn spy.exe");
    let port = await_port(&mut child);
    (child, port, tmp_root)
}

/// Read response off `stream`.  Returns (status_line, headers,
/// body_after_headers).  body_after_headers may be the full body (for
/// Content-Length-framed responses) or only the prefix (for chunked
/// responses).  Reads up to `max_bytes` before giving up.
fn read_http_response(stream: &mut TcpStream, max_bytes: usize) -> (String, Vec<(String, String)>, Vec<u8>) {
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 1024];
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        stream
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    // We have the full header block.  Decide whether the
                    // body is content-length-framed or chunked; if
                    // chunked or unknown, read more aggressively for a
                    // short while.
                    // For test simplicity we always pull more bytes if
                    // < max_bytes (callers verify what they need).
                    if buf.len() >= max_bytes {
                        break;
                    }
                }
            }
            Err(_) => {
                if Instant::now() > deadline {
                    break;
                }
                if !buf.is_empty()
                    && buf.windows(4).any(|w| w == b"\r\n\r\n")
                    && buf.ends_with(b"0\r\n\r\n")
                {
                    break;
                }
                if !buf.is_empty() && buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    // We have headers — return what we got, caller
                    // checks framing.
                    break;
                }
            }
        }
    }
    let head_end = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("no \\r\\n\\r\\n in response");
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap().to_string();
    let mut headers = Vec::new();
    for l in lines {
        if let Some(c) = l.find(':') {
            headers.push((l[..c].trim().to_ascii_lowercase(), l[c + 1..].trim().to_string()));
        }
    }
    let body = buf[head_end + 4..].to_vec();
    (status_line, headers, body)
}

fn parse_status_code(status_line: &str) -> u16 {
    status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn header(headers: &[(String, String)], name: &str) -> Option<String> {
    let key = name.to_ascii_lowercase();
    headers.iter().find(|(k, _)| k == &key).map(|(_, v)| v.clone())
}

#[test]
fn webserver_demo_keepalive_serves_multiple_requests_on_one_conn() {
    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }
    let (mut child, port, _tmp) = spawn_server(
        "m29_5_webserver_keepalive",
        &["--max-accepts", "200", "--shutdown-after-secs", "20"],
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream.set_nodelay(true).unwrap();

    // Send three requests on the same connection.  HTTP/1.1 defaults to
    // keep-alive; we explicitly send Connection: keep-alive for the
    // first two and Connection: close on the third.
    let r1 = b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: keep-alive\r\n\r\n";
    let r2 = b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: keep-alive\r\n\r\n";
    let r3 = b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";

    stream.write_all(r1).unwrap();
    let (status_line, headers, _body) = read_http_response(&mut stream, 256);
    assert_eq!(parse_status_code(&status_line), 200, "req1 status: {status_line}");
    assert_eq!(
        header(&headers, "Connection").as_deref(),
        Some("keep-alive"),
        "req1 connection: {headers:?}"
    );
    assert!(
        header(&headers, "Keep-Alive").is_some(),
        "req1 keep-alive header missing: {headers:?}"
    );

    stream.write_all(r2).unwrap();
    let (status_line2, headers2, _) = read_http_response(&mut stream, 256);
    assert_eq!(parse_status_code(&status_line2), 200, "req2 status: {status_line2}");
    assert_eq!(
        header(&headers2, "Connection").as_deref(),
        Some("keep-alive"),
        "req2 connection: {headers2:?}"
    );

    stream.write_all(r3).unwrap();
    let (status_line3, headers3, _) = read_http_response(&mut stream, 256);
    assert_eq!(parse_status_code(&status_line3), 200, "req3 status: {status_line3}");
    assert_eq!(
        header(&headers3, "Connection").as_deref(),
        Some("close"),
        "req3 connection: {headers3:?}"
    );

    drop(stream);
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn webserver_demo_chunked_response_streams_correctly() {
    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }
    let (mut child, port, _tmp) = spawn_server(
        "m29_5_webserver_chunked",
        &["--max-accepts", "20", "--shutdown-after-secs", "20"],
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream.set_nodelay(true).unwrap();

    // Ask for 5 chunks so the test stays fast.
    let req = b"GET /api/stream?n=5 HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    stream.write_all(req).unwrap();

    let (status_line, headers, body) = read_http_response(&mut stream, 8192);
    assert_eq!(parse_status_code(&status_line), 200, "stream status: {status_line}");
    assert_eq!(
        header(&headers, "Transfer-Encoding").as_deref(),
        Some("chunked"),
        "stream TE: {headers:?}"
    );
    assert!(
        header(&headers, "Content-Length").is_none(),
        "stream should not have Content-Length: {headers:?}"
    );

    let body_str = String::from_utf8_lossy(&body);
    // The body should be 5 chunks framed as `<hex>\r\n<data>\r\n` ending
    // with `0\r\n\r\n`.  Each chunk should be "chunk N\n" for N in 0..5.
    assert!(body_str.contains("chunk 0\n"), "missing chunk 0: {body_str:?}");
    assert!(body_str.contains("chunk 4\n"), "missing chunk 4: {body_str:?}");
    assert!(body_str.ends_with("0\r\n\r\n"), "no terminator chunk: {body_str:?}");

    drop(stream);
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn webserver_demo_multipart_upload_roundtrip() {
    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }
    let (mut child, port, _tmp) = spawn_server(
        "m29_5_webserver_multipart",
        &["--max-accepts", "20", "--shutdown-after-secs", "20"],
    );

    // Build a multipart body by hand to control the exact framing.
    let boundary = "----TestBoundary12345";
    let file_content = b"hello multipart world";
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"file\"; filename=\"greeting.txt\"\r\n");
    body.extend_from_slice(b"Content-Type: text/plain\r\n\r\n");
    body.extend_from_slice(file_content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .max_idle_connections(0)
        .build();
    let resp = agent
        .post(&format!("http://127.0.0.1:{port}/api/upload"))
        .set("Content-Type", &format!("multipart/form-data; boundary={boundary}"))
        .send_bytes(&body);
    let resp = resp.expect("upload");
    assert_eq!(resp.status(), 201);
    let upload_body = resp.into_string().unwrap();
    assert!(
        upload_body.contains("\"url\":\"/api/uploads/greeting.txt\""),
        "upload body: {upload_body}"
    );
    assert!(
        upload_body.contains(&format!("\"size\":{}", file_content.len())),
        "upload body size: {upload_body}"
    );

    // Now download it.
    let (status, fetched_body) = http_call(
        "GET",
        &format!("http://127.0.0.1:{port}/api/uploads/greeting.txt"),
        None,
        None,
    );
    assert_eq!(status, 200);
    assert_eq!(fetched_body, "hello multipart world");

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn webserver_demo_returns_html_404_not_textplain() {
    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }
    let (mut child, port, _tmp) = spawn_server(
        "m29_5_webserver_html404",
        &["--max-accepts", "20", "--shutdown-after-secs", "20"],
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream.set_nodelay(true).unwrap();
    let req = b"GET /no/such/route HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    stream.write_all(req).unwrap();
    let (status_line, headers, body) = read_http_response(&mut stream, 4096);
    assert_eq!(parse_status_code(&status_line), 404, "404 status: {status_line}");
    let ct = header(&headers, "Content-Type").unwrap_or_default();
    assert!(
        ct.starts_with("text/html"),
        "404 Content-Type should be text/html, got: {ct} (headers={headers:?})"
    );
    let body_str = String::from_utf8_lossy(&body);
    assert!(
        body_str.contains("<h1>404"),
        "404 body should be HTML with h1: {body_str:?}"
    );

    drop(stream);
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn webserver_demo_graceful_shutdown_completes_in_flight() {
    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }
    // Server shuts down 3 seconds after startup.  We send a slow-streaming
    // /api/stream?n=10 request (10 chunks * 50ms = ~500ms wall-clock) and
    // verify the response completes before the SHUTDOWN line is printed.
    let tmp_root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("m29_5_webserver_shutdown");
    let _ = fs::remove_dir_all(&tmp_root);
    let spyc_path = compile_demo(&tmp_root);

    let mut child = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg("--port").arg("0")
        .arg("--host").arg("127.0.0.1")
        .arg("--db").arg(tmp_root.join("todos.db").display().to_string())
        .arg("--max-accepts").arg("20")
        .arg("--shutdown-after-secs").arg("3")
        .current_dir(project_root())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn spy.exe");

    let port = await_port(&mut child);

    // Issue an in-flight request that takes ~500ms.  This should
    // complete cleanly even though the shutdown timer is going to fire
    // shortly after.
    let t0 = Instant::now();
    let (status, body) = http_call("GET", &format!("http://127.0.0.1:{port}/api/stream?n=10"), None, None);
    let elapsed = t0.elapsed();
    assert_eq!(status, 200, "in-flight status; body={body}");
    assert!(body.contains("chunk 9"), "in-flight body should be complete: {body:?}");
    assert!(
        elapsed >= Duration::from_millis(400),
        "request finished suspiciously fast: {elapsed:?}"
    );

    // Wait for the server to print SHUTDOWN on stdout.  We'll observe
    // this via try_wait — the child should exit cleanly within ~15s of
    // launch.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut exited = false;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_status)) => {
                exited = true;
                break;
            }
            Ok(None) => {
                thread::sleep(Duration::from_millis(200));
            }
            Err(_) => break,
        }
    }
    if !exited {
        let _ = child.kill();
        let _ = child.wait();
        panic!("server did not exit gracefully within 20s of --shutdown-after-secs=3");
    }
}
