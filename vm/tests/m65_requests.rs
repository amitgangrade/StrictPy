//! M65 (wave-3 Lane A) integration tests for the `requests` module.
//!
//! Every test boots a throwaway HTTP/1.1 server on an ephemeral
//! 127.0.0.1 port inside the test process (a background thread serving
//! canned responses), then compiles + runs a `.spy` snippet that drives
//! the `requests` client against it via `run_file_capture`.  No test
//! ever touches the real internet.
//!
//! Coverage: get + status/ok/text/headers/header, post/post_json/
//! post_form body round-trips, session default headers + cookie replay,
//! redirect following + `set_max_redirects(0)`, `raise_for_status` on
//! 404 (IOError), `download` writing exact bytes, and session close →
//! reuse raising ValueError.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

// ── Test HTTP server ──────────────────────────────────────────────────

struct Req {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Req {
    fn header(&self, name: &str) -> Option<String> {
        let n = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| k.to_ascii_lowercase() == n)
            .map(|(_, v)| v.clone())
    }
}

fn find_subseq(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
}

fn read_request(stream: &mut TcpStream) -> Option<Req> {
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 2048];
    loop {
        if let Some(pos) = find_subseq(&buf, b"\r\n\r\n") {
            let header_str = String::from_utf8_lossy(&buf[..pos]).to_string();
            let mut lines = header_str.split("\r\n");
            let reqline = lines.next().unwrap_or("");
            let mut parts = reqline.split_whitespace();
            let method = parts.next().unwrap_or("").to_string();
            let path = parts.next().unwrap_or("").to_string();
            let mut headers = Vec::new();
            for l in lines {
                if let Some(i) = l.find(':') {
                    headers.push((l[..i].trim().to_string(), l[i + 1..].trim().to_string()));
                }
            }
            let clen: usize = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, v)| v.parse().ok())
                .unwrap_or(0);
            let mut body = buf[pos + 4..].to_vec();
            while body.len() < clen {
                let n = stream.read(&mut tmp).ok()?;
                if n == 0 {
                    break;
                }
                body.extend_from_slice(&tmp[..n]);
            }
            body.truncate(clen);
            return Some(Req { method, path, headers, body });
        }
        let n = stream.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}

/// Write a response with an explicit status line + headers.  Always adds
/// `Content-Length` and `Connection: close` so ureq opens a fresh TCP
/// connection per request (keeps the server single-request-per-socket).
fn write_response(stream: &mut TcpStream, status: &str, headers: &[(&str, &str)], body: &[u8]) {
    let mut resp = format!("HTTP/1.1 {}\r\n", status);
    for (k, v) in headers {
        resp.push_str(&format!("{}: {}\r\n", k, v));
    }
    resp.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ));
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

/// Boot a server whose handler is called once per accepted connection
/// with the (0-based) request index.  Returns the bound port.  The
/// server thread runs for the lifetime of the test process.
fn start_server<F>(handler: F) -> u16
where
    F: Fn(usize, &Req, &mut TcpStream) + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let handler = Arc::new(handler);
    thread::spawn(move || {
        let counter = AtomicUsize::new(0);
        for stream in listener.incoming() {
            if let Ok(mut s) = stream {
                let i = counter.fetch_add(1, Ordering::SeqCst);
                if let Some(req) = read_request(&mut s) {
                    handler(i, &req, &mut s);
                }
            }
        }
    });
    port
}

// ── Snippet compile/run harness ───────────────────────────────────────

fn run_spy(test_name: &str, src_template: &str, port: u16) -> (i32, String) {
    let src = src_template.replace("PORT", &port.to_string());
    let bytes = compile_source(format!("{test_name}.spy"), &src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m65_{test_name}.spyc"));
    std::fs::write(&out, &bytes).expect("write spyc");
    run_file_capture(&out).expect("run")
}

// ── Tests ─────────────────────────────────────────────────────────────

#[test]
fn get_status_ok_text_headers_header() {
    let port = start_server(|_i, _req, s| {
        write_response(
            s,
            "200 OK",
            &[("Content-Type", "text/plain"), ("X-Greeting", "hi there")],
            b"hello world",
        );
    });
    let src = r#"
import requests
from requests import Response

fn main() -> i32:
    r: Response = requests.get("http://127.0.0.1:PORT/hello")
    println("status=" + str(r.status()))
    if r.ok():
        println("ok=true")
    else:
        println("ok=false")
    println("text=" + r.text())
    println("ct=" + r.header("content-type"))
    println("greet=" + r.header("X-GREETING"))
    println("absent=[" + r.header("nope") + "]")
    hs: Dict[str, str] = r.headers()
    println("hdr_ct=" + hs["content-type"])
    return 0
"#;
    let (code, out) = run_spy("get_basic", src, port);
    assert_eq!(code, 0, "out={out}");
    assert!(out.contains("status=200"), "got: {out:?}");
    assert!(out.contains("ok=true"), "got: {out:?}");
    assert!(out.contains("text=hello world"), "got: {out:?}");
    assert!(out.contains("ct=text/plain"), "got: {out:?}");
    assert!(out.contains("greet=hi there"), "got: {out:?}");
    assert!(out.contains("absent=[]"), "got: {out:?}");
    assert!(out.contains("hdr_ct=text/plain"), "got: {out:?}");
}

#[test]
fn post_body_and_content_type_arrive() {
    let port = start_server(|_i, req, s| {
        let ct = req.header("content-type").unwrap_or_default();
        let body = String::from_utf8_lossy(&req.body).to_string();
        let echo = format!("m={} ct={} body={}", req.method, ct, body);
        write_response(s, "200 OK", &[], echo.as_bytes());
    });
    let src = r#"
import requests
from requests import Response

fn main() -> i32:
    r: Response = requests.post("http://127.0.0.1:PORT/p", "the-body", "text/plain")
    println(r.text())
    return 0
"#;
    let (code, out) = run_spy("post_basic", src, port);
    assert_eq!(code, 0, "out={out}");
    assert!(out.contains("m=POST"), "got: {out:?}");
    assert!(out.contains("ct=text/plain"), "got: {out:?}");
    assert!(out.contains("body=the-body"), "got: {out:?}");
}

#[test]
fn post_json_serialises_and_sets_content_type() {
    let port = start_server(|_i, req, s| {
        let ct = req.header("content-type").unwrap_or_default();
        let body = String::from_utf8_lossy(&req.body).to_string();
        let echo = format!("ct={} body={}", ct, body);
        write_response(s, "200 OK", &[], echo.as_bytes());
    });
    let src = r#"
import requests
from requests import Response
import json

fn main() -> i32:
    payload: JsonValue = json.parse("{\"a\": 1, \"b\": \"x\"}")
    r: Response = requests.post_json("http://127.0.0.1:PORT/j", payload)
    println(r.text())
    return 0
"#;
    let (code, out) = run_spy("post_json", src, port);
    assert_eq!(code, 0, "out={out}");
    assert!(out.contains("ct=application/json"), "got: {out:?}");
    assert!(out.contains("\"a\""), "got: {out:?}");
    assert!(out.contains("\"b\""), "got: {out:?}");
}

#[test]
fn post_form_urlencodes_body() {
    let port = start_server(|_i, req, s| {
        let ct = req.header("content-type").unwrap_or_default();
        let body = String::from_utf8_lossy(&req.body).to_string();
        let echo = format!("ct={} body={}", ct, body);
        write_response(s, "200 OK", &[], echo.as_bytes());
    });
    let src = r#"
import requests
from requests import Response

fn main() -> i32:
    form: Dict[str, str] = {}
    form["name"] = "ada lovelace"
    r: Response = requests.post_form("http://127.0.0.1:PORT/f", form)
    println(r.text())
    return 0
"#;
    let (code, out) = run_spy("post_form", src, port);
    assert_eq!(code, 0, "out={out}");
    assert!(
        out.contains("ct=application/x-www-form-urlencoded"),
        "got: {out:?}"
    );
    assert!(out.contains("name=ada+lovelace"), "got: {out:?}");
}

#[test]
fn get_with_merges_query_params() {
    let port = start_server(|_i, req, s| {
        write_response(s, "200 OK", &[], req.path.as_bytes());
    });
    let src = r#"
import requests
from requests import Response

fn main() -> i32:
    params: Dict[str, str] = {}
    params["q"] = "rust lang"
    headers: Dict[str, str] = {}
    headers["X-Trace"] = "1"
    r: Response = requests.get_with("http://127.0.0.1:PORT/search", params, headers)
    println("path=" + r.text())
    return 0
"#;
    let (code, out) = run_spy("get_with", src, port);
    assert_eq!(code, 0, "out={out}");
    assert!(out.contains("path=/search?q=rust+lang"), "got: {out:?}");
}

#[test]
fn session_default_headers_and_cookie_replay() {
    let port = start_server(|_i, req, s| {
        let xapp = req.header("x-app").unwrap_or_default();
        if req.path == "/set" {
            let body = format!("set xapp={}", xapp);
            write_response(
                s,
                "200 OK",
                &[("Set-Cookie", "sid=abc123; Path=/")],
                body.as_bytes(),
            );
        } else {
            let cookie = req.header("cookie").unwrap_or_default();
            let has = cookie.contains("sid=abc123");
            let body = format!("check cookie={} xapp={}", has, xapp);
            write_response(s, "200 OK", &[], body.as_bytes());
        }
    });
    let src = r#"
import requests
from requests import Response, Session

fn main() -> i32:
    s: Session = requests.session()
    s.set_header("X-App", "strict")
    r1: Response = s.get("http://127.0.0.1:PORT/set")
    println("r1=" + r1.text())
    r2: Response = s.get("http://127.0.0.1:PORT/check")
    println("r2=" + r2.text())
    return 0
"#;
    let (code, out) = run_spy("session_cookie", src, port);
    assert_eq!(code, 0, "out={out}");
    assert!(out.contains("r1=set xapp=strict"), "got: {out:?}");
    assert!(out.contains("r2=check cookie=true xapp=strict"), "got: {out:?}");
}

#[test]
fn redirect_followed_by_default() {
    let port = start_server(|_i, req, s| {
        if req.path == "/redir" {
            write_response(s, "302 Found", &[("Location", "/final")], b"");
        } else {
            write_response(s, "200 OK", &[], b"arrived");
        }
    });
    let src = r#"
import requests
from requests import Response

fn main() -> i32:
    r: Response = requests.get("http://127.0.0.1:PORT/redir")
    println("status=" + str(r.status()))
    println("text=" + r.text())
    return 0
"#;
    let (code, out) = run_spy("redirect_follow", src, port);
    assert_eq!(code, 0, "out={out}");
    assert!(out.contains("status=200"), "got: {out:?}");
    assert!(out.contains("text=arrived"), "got: {out:?}");
}

#[test]
fn max_redirects_zero_does_not_follow() {
    let port = start_server(|_i, req, s| {
        if req.path == "/redir" {
            write_response(s, "302 Found", &[("Location", "/final")], b"");
        } else {
            write_response(s, "200 OK", &[], b"arrived");
        }
    });
    let src = r#"
import requests
from requests import Response, Session

fn main() -> i32:
    s: Session = requests.session()
    s.set_max_redirects(0i64)
    r: Response = s.get("http://127.0.0.1:PORT/redir")
    println("status=" + str(r.status()))
    return 0
"#;
    let (code, out) = run_spy("max_redirects_zero", src, port);
    assert_eq!(code, 0, "out={out}");
    assert!(out.contains("status=302"), "got: {out:?}");
}

#[test]
fn raise_for_status_on_404() {
    let port = start_server(|_i, _req, s| {
        write_response(s, "404 Not Found", &[], b"nope");
    });
    let src = r#"
import requests
from requests import Response

fn main() -> i32:
    r: Response = requests.get("http://127.0.0.1:PORT/missing")
    println("status=" + str(r.status()))
    try:
        r.raise_for_status()
        println("no-raise")
    except IOError as e:
        println("caught-" + e.type_name)
    return 0
"#;
    let (code, out) = run_spy("raise_for_status", src, port);
    assert_eq!(code, 0, "out={out}");
    assert!(out.contains("status=404"), "got: {out:?}");
    assert!(out.contains("caught-IOError"), "got: {out:?}");
    assert!(!out.contains("no-raise"), "got: {out:?}");
}

#[test]
fn download_writes_exact_bytes() {
    // Serve a body containing a non-ASCII byte to prove exactness.
    let port = start_server(|_i, _req, s| {
        write_response(s, "200 OK", &[], &[0x48, 0x69, 0x00, 0xFF, 0x42]);
    });
    let mut out_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out_path.push("m65_download_out.bin");
    let _ = std::fs::remove_file(&out_path);
    let spy_path = out_path.to_string_lossy().replace('\\', "/");
    let src = format!(
        r#"
import requests

fn main() -> i32:
    n: i64 = requests.download("http://127.0.0.1:PORT/blob", "{spy_path}")
    println("wrote=" + str(n))
    return 0
"#
    );
    let (code, out) = run_spy("download_bytes", &src, port);
    assert_eq!(code, 0, "out={out}");
    assert!(out.contains("wrote=5"), "got: {out:?}");
    let written = std::fs::read(&out_path).expect("read downloaded file");
    assert_eq!(written, vec![0x48, 0x69, 0x00, 0xFF, 0x42], "bytes mismatch");
}

#[test]
fn session_close_then_reuse_raises_value_error() {
    let port = start_server(|_i, _req, s| {
        write_response(s, "200 OK", &[], b"ok");
    });
    let src = r#"
import requests
from requests import Response, Session

fn main() -> i32:
    s: Session = requests.session()
    s.close()
    try:
        r: Response = s.get("http://127.0.0.1:PORT/x")
        println("reused=" + str(r.status()))
    except ValueError as e:
        println("caught-" + e.type_name)
    return 0
"#;
    let (code, out) = run_spy("session_close_reuse", src, port);
    assert_eq!(code, 0, "out={out}");
    assert!(out.contains("caught-ValueError"), "got: {out:?}");
    assert!(!out.contains("reused="), "got: {out:?}");
}
