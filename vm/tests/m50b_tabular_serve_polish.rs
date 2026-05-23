//! M50b: tabular.serve frontend polish + new endpoints.
//!
//! Tests the M50b additions to the M50a HTTP transport:
//!   - GET  /api/csv?df=N            — CSV download
//!   - POST /api/forget {"df":N}     — drop a derived frame from registry
//!   - POST /api/sort                — sortable column headers (server-side)
//!   - POST /api/filter_multi        — composite AND/OR filter
//!   - ColumnCategorical cells now serialize as their resolved string
//!     (M50a punted to JSON null; M50b resolves through codes->cats)
//!
//! Test setup mirrors `m50a_tabular_serve.rs` — boot the server with
//! `serve_with_timeout`, hit it from a TcpStream, assert on status +
//! body fragments.

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
    out.push(format!("strictpy_m50b_{test_name}.spyc"));
    std::fs::write(&out, &bytes).expect("write spyc");
    out
}

fn http_get(port: u16, path: &str) -> (u16, String, String) {
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

/// Standard 4-row mixed-dtype frame (id/name/score/active).  Same shape
/// the M50a tests use, so the M50b assertions stay comparable.
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

/// Frame including a ColumnCategorical so we can assert the M50b
/// categorical serialization fix.
fn categorical_df_serve_source(port: u16, timeout_ms: i64) -> String {
    format!(r#"
import tabular
from tabular import DataFrame, Column, ColumnI64, ColumnStr, ColumnCategorical

fn main() -> i32:
    ns: List[str] = []
    ns.append("id")
    ns.append("color")
    vs1: List[i64] = []
    vs1.append(1i64)
    vs1.append(2i64)
    vs1.append(3i64)
    vs1.append(4i64)
    nulls: List[bool] = []
    nulls.append(false)
    nulls.append(false)
    nulls.append(false)
    nulls.append(false)
    cats: List[str] = []
    cats.append("red")
    cats.append("green")
    cats.append("blue")
    cats.append("red")
    c1: ColumnI64 = tabular.col_i64(vs1, nulls)
    c2: ColumnCategorical = tabular.col_categorical(cats)
    cols: List[Column] = []
    cols.append(c1)
    cols.append(c2)
    df: DataFrame = tabular.from_columns(ns, cols)
    rc: i32 = tabular.serve_with_timeout(df, {port}i32, {timeout_ms}i64)
    return rc
"#)
}

// ── ColumnCategorical serialization ──────────────────────────────────

#[test]
fn rows_endpoint_renders_categorical_as_string() {
    let port = 35701u16;
    let src = categorical_df_serve_source(port, 1500);
    let path = compile_snippet("cat_rows", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_get(port, "/api/rows?start=0&stop=4");
    assert_eq!(status, 200, "body: {body}");
    // Categorical cells should now show as their resolved strings — not
    // null (M50a's punted behavior).
    assert!(body.contains("\"red\""), "expected 'red' in body: {body}");
    assert!(body.contains("\"green\""), "expected 'green' in body: {body}");
    assert!(body.contains("\"blue\""), "expected 'blue' in body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn schema_endpoint_lists_categorical_dtype() {
    let port = 35702u16;
    let src = categorical_df_serve_source(port, 1500);
    let path = compile_snippet("cat_schema", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_get(port, "/api/schema");
    assert_eq!(status, 200);
    assert!(body.contains("\"categorical\""), "expected categorical dtype: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

// ── CSV download endpoint ────────────────────────────────────────────

#[test]
fn csv_endpoint_returns_text_csv_with_header() {
    let port = 35703u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("csv_basic", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, head, body) = http_get(port, "/api/csv?df=0");
    assert_eq!(status, 200, "body: {body}");
    assert!(head.to_lowercase().contains("text/csv"), "head: {head}");
    // Header line.
    assert!(body.starts_with("id,name,score,active\n"), "body: {body}");
    // One of the rows.
    assert!(body.contains("1,alice,10.5,true"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn csv_endpoint_renders_categorical_cells() {
    let port = 35704u16;
    let src = categorical_df_serve_source(port, 1500);
    let path = compile_snippet("csv_cat", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_get(port, "/api/csv?df=0");
    assert_eq!(status, 200);
    assert!(body.starts_with("id,color\n"), "body: {body}");
    assert!(body.contains("1,red"), "body: {body}");
    assert!(body.contains("3,blue"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn csv_endpoint_unknown_df_returns_404() {
    let port = 35705u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("csv_unknown", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_get(port, "/api/csv?df=999");
    assert_eq!(status, 404, "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

// ── /api/sort ────────────────────────────────────────────────────────

#[test]
fn sort_ascending_returns_derived_df() {
    let port = 35706u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("sort_asc", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/sort",
        r#"{"df":0,"column":"score","ascending":true}"#,
    );
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("\"df\":1"), "body: {body}");
    assert!(body.contains("\"nrows\":4"), "body: {body}");

    // Read the first row of the derived frame and assert ascending order
    // (lowest score = 7.5 for dave).
    let (s2, _, b2) = http_get(port, "/api/rows?df=1&start=0&stop=1");
    assert_eq!(s2, 200);
    assert!(b2.contains("\"dave\""), "first row should be dave (score 7.5): {b2}");

    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn sort_descending_returns_derived_df() {
    let port = 35707u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("sort_desc", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/sort",
        r#"{"df":0,"column":"score","ascending":false}"#,
    );
    assert_eq!(status, 200, "body: {body}");
    // First row of descending sort = highest score = bob (20.0).
    let (_, _, b2) = http_get(port, "/api/rows?df=1&start=0&stop=1");
    assert!(b2.contains("\"bob\""), "first row should be bob (score 20.0): {b2}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn sort_unknown_column_returns_400() {
    let port = 35708u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("sort_bad_col", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/sort",
        r#"{"df":0,"column":"nope","ascending":true}"#,
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(body.contains("not found"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn sort_unknown_df_returns_404() {
    let port = 35709u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("sort_bad_df", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/sort",
        r#"{"df":999,"column":"score","ascending":true}"#,
    );
    assert_eq!(status, 404, "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

// ── /api/filter_multi (composite AND/OR) ──────────────────────────────

#[test]
fn filter_multi_and_matches_intersection() {
    let port = 35710u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("fm_and", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    // id > 1 AND active == true → rows id=3, id=4 (id=2 has active=false)
    let (status, _head, body) = http_post(
        port,
        "/api/filter_multi",
        r#"{"df":0,"logic":"and","clauses":[{"column":"id","op":"gt","value":1},{"column":"active","op":"eq","value":true}]}"#,
    );
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("\"nrows\":2"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn filter_multi_or_matches_union() {
    let port = 35711u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("fm_or", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    // id == 1 OR id == 4 → rows id=1, id=4 → 2 rows
    let (status, _head, body) = http_post(
        port,
        "/api/filter_multi",
        r#"{"df":0,"logic":"or","clauses":[{"column":"id","op":"eq","value":1},{"column":"id","op":"eq","value":4}]}"#,
    );
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("\"nrows\":2"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn filter_multi_single_clause_is_equivalent_to_filter() {
    let port = 35712u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("fm_single", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/filter_multi",
        r#"{"df":0,"logic":"and","clauses":[{"column":"name","op":"eq","value":"alice"}]}"#,
    );
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("\"nrows\":1"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn filter_multi_empty_clauses_returns_400() {
    let port = 35713u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("fm_empty", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/filter_multi",
        r#"{"df":0,"logic":"and","clauses":[]}"#,
    );
    assert_eq!(status, 400, "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn filter_multi_invalid_logic_returns_400() {
    let port = 35714u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("fm_bad_logic", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/filter_multi",
        r#"{"df":0,"logic":"xor","clauses":[{"column":"id","op":"eq","value":1}]}"#,
    );
    assert_eq!(status, 400, "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

// ── /api/forget ──────────────────────────────────────────────────────

#[test]
fn forget_derived_df_returns_ok_true() {
    let port = 35715u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("forget_ok", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    // First create a derived df via filter.
    let (s1, _, b1) = http_post(
        port,
        "/api/filter",
        r#"{"df":0,"column":"id","op":"eq","value":1}"#,
    );
    assert_eq!(s1, 200, "body: {b1}");
    assert!(b1.contains("\"df\":1"), "body: {b1}");

    // Now forget it.
    let (s2, _, b2) = http_post(port, "/api/forget", r#"{"df":1}"#);
    assert_eq!(s2, 200, "body: {b2}");
    assert!(b2.contains("\"ok\":true"), "body: {b2}");

    // Subsequent schema lookup for df=1 should 404.
    let (s3, _, b3) = http_get(port, "/api/schema?df=1");
    assert_eq!(s3, 404, "body: {b3}");

    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn forget_primary_df_is_refused() {
    let port = 35716u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("forget_primary", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(port, "/api/forget", r#"{"df":0}"#);
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("\"ok\":false"), "body: {body}");

    // Primary still reachable.
    let (s2, _, _) = http_get(port, "/api/schema?df=0");
    assert_eq!(s2, 200);

    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn forget_unknown_df_returns_ok_false() {
    let port = 35717u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("forget_unknown", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(port, "/api/forget", r#"{"df":999}"#);
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("\"ok\":false"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

// ── Bundled HTML — M50b additions ────────────────────────────────────

#[test]
fn html_includes_m50b_ui_affordances() {
    let port = 35718u16;
    let src = small_df_serve_source(port, 1500);
    let path = compile_snippet("html_polish", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_get(port, "/");
    assert_eq!(status, 200);
    // CSV download + forget buttons.
    assert!(body.contains("CSV download"), "missing CSV button: {body:.200}");
    assert!(body.contains("Reset to primary"), "missing forget button: {body:.200}");
    // Composite-filter logic toggle.
    assert!(body.contains("logic-sel"), "missing logic toggle: {body:.200}");
    // Sort endpoint reference.
    assert!(body.contains("/api/sort"), "missing /api/sort wiring: {body:.200}");
    assert!(body.contains("/api/filter_multi"), "missing /api/filter_multi wiring: {body:.200}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}
