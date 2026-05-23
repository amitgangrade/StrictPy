//! M50a: tabular.serve HTTP transport tests.
//!
//! Every test boots the server in a child thread via `serve_with_timeout`
//! with a short deadline, then hits it from the test side via
//! `std::net::TcpStream`.  The server shuts itself down when the timeout
//! elapses, so the harness doesn't need to do explicit cleanup.
//!
//! The test port for each test is offset from a base to reduce the
//! chance of collisions when tests run in parallel (cargo test's
//! default).  We use `127.0.0.1:35NNN` (private port range).
//!
//! Wire-format note: each test makes a single GET or POST with
//! Connection: close, reads the entire response, and asserts on the
//! status line + a fragment of the body.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m50a_{test_name}.spyc"));
    std::fs::write(&out, &bytes).expect("write spyc");
    out
}

/// Make a raw HTTP GET request and return (status_code, headers_block, body).
fn http_get(port: u16, path: &str) -> (u16, String, String) {
    // Retry connecting for up to 2 seconds — the server may still be
    // binding when the test side races in.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    #[allow(unused_assignments)]
    let mut last_err: Option<std::io::Error> = None;
    loop {
        match TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(200),
        ) {
            Ok(mut s) => {
                s.set_read_timeout(Some(Duration::from_millis(1500))).unwrap();
                let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
                s.write_all(req.as_bytes()).expect("write");
                let mut buf = Vec::new();
                let _ = s.read_to_end(&mut buf);
                let txt = String::from_utf8_lossy(&buf).to_string();
                let (head, body) = match txt.find("\r\n\r\n") {
                    Some(idx) => (txt[..idx].to_string(), txt[idx + 4..].to_string()),
                    None => (txt.clone(), String::new()),
                };
                let status = head
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|c| c.parse::<u16>().ok())
                    .unwrap_or(0);
                return (status, head, body);
            }
            Err(e) => {
                last_err = Some(e);
                if std::time::Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    panic!("connect failed: {:?}", last_err);
}

/// Make a raw HTTP POST request with a JSON body.
fn http_post(port: u16, path: &str, body: &str) -> (u16, String, String) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    #[allow(unused_assignments)]
    let mut last_err: Option<std::io::Error> = None;
    loop {
        match TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(200),
        ) {
            Ok(mut s) => {
                s.set_read_timeout(Some(Duration::from_millis(1500))).unwrap();
                let req = format!(
                    "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body,
                );
                s.write_all(req.as_bytes()).expect("write");
                let mut buf = Vec::new();
                let _ = s.read_to_end(&mut buf);
                let txt = String::from_utf8_lossy(&buf).to_string();
                let (head, body_out) = match txt.find("\r\n\r\n") {
                    Some(idx) => (txt[..idx].to_string(), txt[idx + 4..].to_string()),
                    None => (txt.clone(), String::new()),
                };
                let status = head
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|c| c.parse::<u16>().ok())
                    .unwrap_or(0);
                return (status, head, body_out);
            }
            Err(e) => {
                last_err = Some(e);
                if std::time::Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    panic!("connect failed: {:?}", last_err);
}

/// Build a small DataFrame and call serve_with_timeout on the supplied
/// port for the supplied duration in milliseconds.  Returns the
/// compiled .spy file path.
fn small_df_serve_source(port: u16, timeout_ms: i64) -> String {
    format!(r#"
import tabular
from tabular import DataFrame, Column, ColumnI64, ColumnStr, ColumnF64, ColumnBool

fn main() -> i32:
    ns: List[str] = []
    ns.append("id")
    ns.append("name")
    ns.append("score")
    ns.append("active")
    vs1: List[i64] = []
    vs1.append(1i64)
    vs1.append(2i64)
    vs1.append(3i64)
    vs1.append(4i64)
    vs2: List[str] = []
    vs2.append("alice")
    vs2.append("bob")
    vs2.append("carol")
    vs2.append("dave")
    vs3: List[f64] = []
    vs3.append(10.5)
    vs3.append(20.0)
    vs3.append(15.25)
    vs3.append(7.5)
    vs4: List[bool] = []
    vs4.append(true)
    vs4.append(false)
    vs4.append(true)
    vs4.append(true)
    nulls: List[bool] = []
    nulls.append(false)
    nulls.append(false)
    nulls.append(false)
    nulls.append(false)
    c1: ColumnI64 = tabular.col_i64(vs1, nulls)
    c2: ColumnStr = tabular.col_str(vs2, nulls)
    c3: ColumnF64 = tabular.col_f64(vs3, nulls)
    c4: ColumnBool = tabular.col_bool(vs4, nulls)
    cols: List[Column] = []
    cols.append(c1)
    cols.append(c2)
    cols.append(c3)
    cols.append(c4)
    df: DataFrame = tabular.from_columns(ns, cols)
    rc: i32 = tabular.serve_with_timeout(df, {port}i32, {timeout_ms}i64)
    return rc
"#)
}

// ── Phase A: smoke ────────────────────────────────────────────────────

#[test]
fn server_boots_and_shuts_down_after_timeout() {
    let port = 35601u16;
    let src = small_df_serve_source(port, 400);
    let path = compile_snippet("smoke_boot", &src);
    let start = std::time::Instant::now();
    let (rc, _stdout) = run_file_capture(&path).expect("run");
    let elapsed = start.elapsed();
    assert_eq!(rc, 0, "expected clean shutdown");
    // Timeout was 400ms; should not take much longer.
    assert!(
        elapsed < Duration::from_millis(2000),
        "took {:?}, expected ~400ms",
        elapsed
    );
}

#[test]
fn server_responds_to_get_schema() {
    let port = 35602u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("smoke_schema", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_get(port, "/api/schema");
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("\"names\":"), "body: {body}");
    assert!(body.contains("\"nrows\":4"), "body: {body}");
    assert!(body.contains("\"id\""), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

// ── Phase B: read-only endpoints ──────────────────────────────────────

#[test]
fn schema_contains_all_dtypes() {
    let port = 35603u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("schema_dtypes", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_get(port, "/api/schema");
    assert_eq!(status, 200);
    assert!(body.contains("\"i64\""), "body: {body}");
    assert!(body.contains("\"str\""), "body: {body}");
    assert!(body.contains("\"f64\""), "body: {body}");
    assert!(body.contains("\"bool\""), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn rows_endpoint_returns_paginated_rows() {
    let port = 35604u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("rows_basic", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_get(port, "/api/rows?start=0&stop=2");
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("\"rows\":"), "body: {body}");
    // First row: [1, "alice", 10.5, true].
    assert!(body.contains("\"alice\""), "body: {body}");
    assert!(body.contains("\"bob\""), "body: {body}");
    // carol must NOT appear (we asked stop=2).
    assert!(!body.contains("\"carol\""), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn rows_endpoint_clamps_out_of_range() {
    let port = 35605u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("rows_clamp", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_get(port, "/api/rows?start=10&stop=100");
    assert_eq!(status, 200, "body: {body}");
    // Out-of-range range clamps to empty: rows array is [].
    assert!(body.contains("\"rows\":[]"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn cell_endpoint_returns_single_value() {
    let port = 35606u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("cell_basic", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_get(port, "/api/cell?row=1&col=1");
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("\"bob\""), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn cell_endpoint_400_on_out_of_range_row() {
    let port = 35607u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("cell_oob", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_get(port, "/api/cell?row=999&col=0");
    assert_eq!(status, 400, "body: {body}");
    assert!(body.contains("\"error\""), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn unknown_df_id_returns_404() {
    let port = 35608u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("unknown_df", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_get(port, "/api/schema?df=999");
    assert_eq!(status, 404, "body: {body}");
    assert!(body.contains("unknown df id"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn unknown_path_returns_404() {
    let port = 35609u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("unknown_path", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, _body) = http_get(port, "/nope");
    assert_eq!(status, 404);
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

// ── Phase C: filter + groupby endpoints ───────────────────────────────

#[test]
fn filter_i64_eq_returns_derived_df() {
    let port = 35610u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("filter_eq", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/filter",
        r#"{"df":0,"column":"id","op":"eq","value":2}"#,
    );
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("\"df\":1"), "body: {body}");
    assert!(body.contains("\"nrows\":1"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn filter_str_eq_returns_derived_df() {
    let port = 35611u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("filter_str", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/filter",
        r#"{"df":0,"column":"name","op":"eq","value":"carol"}"#,
    );
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("\"nrows\":1"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn filter_gt_returns_correct_count() {
    let port = 35612u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("filter_gt", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/filter",
        r#"{"df":0,"column":"id","op":"gt","value":2}"#,
    );
    assert_eq!(status, 200, "body: {body}");
    // id > 2 matches rows id=3 and id=4 → nrows=2.
    assert!(body.contains("\"nrows\":2"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn filter_invalid_op_returns_400() {
    let port = 35613u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("filter_bad_op", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/filter",
        r#"{"df":0,"column":"id","op":"contains","value":1}"#,
    );
    assert_eq!(status, 400, "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn filter_invalid_column_returns_400() {
    let port = 35614u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("filter_bad_col", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/filter",
        r#"{"df":0,"column":"nosuch","op":"eq","value":1}"#,
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(body.contains("not found"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn groupby_single_col_sum() {
    let port = 35615u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("gby_sum", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/groupby",
        r#"{"df":0,"by":["active"],"agg":{"id":"sum"}}"#,
    );
    assert_eq!(status, 200, "body: {body}");
    // active has two distinct values (true, false) → nrows=2.
    assert!(body.contains("\"nrows\":2"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

// ── Phase D: bundled HTML frontend ────────────────────────────────────

#[test]
fn root_returns_bundled_html() {
    let port = 35616u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("root_html", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, head, body) = http_get(port, "/");
    assert_eq!(status, 200, "body: {body}");
    assert!(head.to_lowercase().contains("text/html"), "head: {head}");
    assert!(body.contains("<title>tabular.serve</title>"), "body: {body}");
    assert!(body.contains("<script>"), "body: {body}");
    assert!(body.contains("/api/schema"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}
