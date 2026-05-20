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
use std::io::{BufRead, BufReader};
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
