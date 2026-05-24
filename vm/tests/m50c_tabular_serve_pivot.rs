//! M50c: tabular.serve interactive pivot + sortable group-by + chart UI.
//!
//! Tests the M50c additions to the M50a/M50b HTTP transport:
//!   - POST /api/pivot          — interactive pivot UI backend
//!   - POST /api/groupby with `"sort":true` — sort agg by first key col
//!   - Bundled HTML now includes Pivot panel + Chart panel
//!
//! The chart rendering is pure-JS canvas — there's no server endpoint
//! for it to drive — so the chart tests just verify the bundled HTML
//! wires up the right elements and references.
//!
//! Test setup mirrors `m50a_tabular_serve.rs`.

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
    out.push(format!("strictpy_m50c_{test_name}.spyc"));
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
                let status = head.lines().next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|c| c.parse::<u16>().ok()).unwrap_or(0);
                return (status, head, body);
            }
            Err(e) => {
                last_err = Some(e);
                if std::time::Instant::now() >= deadline { break; }
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
                    body.len(), body,
                );
                s.write_all(req.as_bytes()).expect("write");
                let mut buf = Vec::new();
                let _ = s.read_to_end(&mut buf);
                let txt = String::from_utf8_lossy(&buf).to_string();
                let (head, body_out) = match txt.find("\r\n\r\n") {
                    Some(idx) => (txt[..idx].to_string(), txt[idx + 4..].to_string()),
                    None => (txt.clone(), String::new()),
                };
                let status = head.lines().next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|c| c.parse::<u16>().ok()).unwrap_or(0);
                return (status, head, body_out);
            }
            Err(e) => {
                last_err = Some(e);
                if std::time::Instant::now() >= deadline { break; }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    panic!("connect failed: {:?}", last_err);
}

/// 6-row sales-style frame: (region, product, units) — exercises pivot
/// with categorical-ish index/columns and a numeric values column.
fn pivot_df_serve_source(port: u16, timeout_ms: i64) -> String {
    format!(r#"
import tabular
from tabular import DataFrame, Column, ColumnI64, ColumnStr

fn main() -> i32:
    ns: List[str] = []
    ns.append("region")
    ns.append("product")
    ns.append("units")
    regions: List[str] = []
    regions.append("east")
    regions.append("east")
    regions.append("west")
    regions.append("west")
    regions.append("east")
    regions.append("west")
    products: List[str] = []
    products.append("a")
    products.append("b")
    products.append("a")
    products.append("b")
    products.append("a")
    products.append("a")
    units: List[i64] = []
    units.append(10i64)
    units.append(5i64)
    units.append(7i64)
    units.append(3i64)
    units.append(2i64)
    units.append(8i64)
    nulls: List[bool] = []
    nulls.append(false)
    nulls.append(false)
    nulls.append(false)
    nulls.append(false)
    nulls.append(false)
    nulls.append(false)
    c1: ColumnStr = tabular.col_str(regions, nulls)
    c2: ColumnStr = tabular.col_str(products, nulls)
    c3: ColumnI64 = tabular.col_i64(units, nulls)
    cols: List[Column] = []
    cols.append(c1)
    cols.append(c2)
    cols.append(c3)
    df: DataFrame = tabular.from_columns(ns, cols)
    rc: i32 = tabular.serve_with_timeout(df, {port}i32, {timeout_ms}i64)
    return rc
"#)
}

// ── /api/pivot ────────────────────────────────────────────────────────

#[test]
fn pivot_sum_happy_path() {
    let port = 35801u16;
    let src = pivot_df_serve_source(port, 1500);
    let path = compile_snippet("pivot_sum", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/pivot",
        r#"{"df":0,"index":"region","columns":"product","values":"units","aggfunc":"sum"}"#,
    );
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("\"df\":1"), "body: {body}");
    // 2 distinct regions → 2 rows in the pivot table.
    assert!(body.contains("\"nrows\":2"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn pivot_count_returns_derived_df() {
    let port = 35802u16;
    let src = pivot_df_serve_source(port, 1500);
    let path = compile_snippet("pivot_count", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/pivot",
        r#"{"df":0,"index":"region","columns":"product","values":"units","aggfunc":"count"}"#,
    );
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("\"nrows\":2"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn pivot_unknown_index_column_returns_400() {
    let port = 35803u16;
    let src = pivot_df_serve_source(port, 1500);
    let path = compile_snippet("pivot_bad_idx", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/pivot",
        r#"{"df":0,"index":"nope","columns":"product","values":"units","aggfunc":"sum"}"#,
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(body.contains("index column"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn pivot_unknown_values_column_returns_400() {
    let port = 35804u16;
    let src = pivot_df_serve_source(port, 1500);
    let path = compile_snippet("pivot_bad_vals", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/pivot",
        r#"{"df":0,"index":"region","columns":"product","values":"nosuch","aggfunc":"sum"}"#,
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(body.contains("values column"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn pivot_invalid_aggfunc_returns_400() {
    let port = 35805u16;
    let src = pivot_df_serve_source(port, 1500);
    let path = compile_snippet("pivot_bad_agg", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/pivot",
        r#"{"df":0,"index":"region","columns":"product","values":"units","aggfunc":"median"}"#,
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(body.contains("aggfunc"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn pivot_unknown_df_returns_404() {
    let port = 35806u16;
    let src = pivot_df_serve_source(port, 1500);
    let path = compile_snippet("pivot_bad_df", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/pivot",
        r#"{"df":999,"index":"region","columns":"product","values":"units","aggfunc":"sum"}"#,
    );
    assert_eq!(status, 404, "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn pivot_missing_field_returns_400() {
    let port = 35807u16;
    let src = pivot_df_serve_source(port, 1500);
    let path = compile_snippet("pivot_missing", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/pivot",
        r#"{"df":0,"index":"region","columns":"product","aggfunc":"sum"}"#,
    );
    assert_eq!(status, 400, "body: {body}");
    assert!(body.contains("missing values"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

// ── /api/groupby with sort=true (M50c) ────────────────────────────────

#[test]
fn groupby_with_sort_true_returns_ordered_rows() {
    // Multi-key group_by retains keys as regular columns (per M44).
    // With sort=true, the result is sorted by the first key column
    // ascending, so the FIRST row should have the alphabetically-first
    // region ("east").
    let port = 35808u16;
    let src = pivot_df_serve_source(port, 1500);
    let path = compile_snippet("gby_sorted", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/groupby",
        r#"{"df":0,"by":["region","product"],"agg":{"units":"sum"},"sort":true}"#,
    );
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("\"df\":1"), "body: {body}");

    // 4 distinct (region,product) tuples → 4 rows.
    assert!(body.contains("\"nrows\":4"), "body: {body}");

    // First row of derived df: the region key should be "east" — but
    // because group_by promoted both keys to a MultiIndex (M44), it
    // lives in the rows endpoint's `index` array (M50c addition), not
    // in `rows`.
    let (_, _, rows_body) = http_get(port, "/api/rows?df=1&start=0&stop=1");
    assert!(rows_body.contains("\"index\":[[\"east\""),
            "first row's index should start with east: {rows_body}");

    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn groupby_without_sort_still_works() {
    // Sanity check — omitting `sort` returns the same nrows as the
    // M50a behavior (just unsorted).
    let port = 35809u16;
    let src = pivot_df_serve_source(port, 1500);
    let path = compile_snippet("gby_unsorted", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/groupby",
        r#"{"df":0,"by":["region","product"],"agg":{"units":"sum"}}"#,
    );
    assert_eq!(status, 200, "body: {body}");
    assert!(body.contains("\"nrows\":4"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn groupby_with_sort_single_col_falls_back_gracefully() {
    // Single-col group_by promotes the key to the index (per M43), so
    // df.sort_by(region) would error.  The handler catches that and
    // returns the unsorted aggregate rather than 400'ing the request.
    let port = 35810u16;
    let src = pivot_df_serve_source(port, 1500);
    let path = compile_snippet("gby_sort_single", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_post(
        port,
        "/api/groupby",
        r#"{"df":0,"by":["region"],"agg":{"units":"sum"},"sort":true}"#,
    );
    assert_eq!(status, 200, "body: {body}");
    // 2 distinct regions.
    assert!(body.contains("\"nrows\":2"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

// ── Bundled HTML — M50c additions ─────────────────────────────────────

#[test]
fn html_includes_m50c_pivot_panel() {
    let port = 35811u16;
    let src = pivot_df_serve_source(port, 1500);
    let path = compile_snippet("html_pivot", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_get(port, "/");
    assert_eq!(status, 200);
    // Pivot panel structural elements.
    assert!(body.contains("id=\"pivot-panel\""), "missing pivot panel id: body len={}", body.len());
    assert!(body.contains("id=\"pivot-bar\""), "missing pivot bar id");
    assert!(body.contains("/api/pivot"), "missing /api/pivot wiring");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn html_includes_m50c_chart_panel() {
    let port = 35812u16;
    let src = pivot_df_serve_source(port, 1500);
    let path = compile_snippet("html_chart", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_get(port, "/");
    assert_eq!(status, 200);
    assert!(body.contains("id=\"chart-canvas\""), "missing chart canvas");
    assert!(body.contains("drawHistogram"), "missing histogram chart fn");
    assert!(body.contains("drawBar"), "missing bar chart fn");
    assert!(body.contains("drawLine"), "missing line chart fn");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

// ── /api/rows + /api/schema: M50c index data ─────────────────────────

#[test]
fn schema_includes_index_names_and_dtypes_after_groupby() {
    let port = 35814u16;
    let src = pivot_df_serve_source(port, 1500);
    let path = compile_snippet("schema_idx", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    // Multi-key group_by promotes both keys to a 2-level MultiIndex.
    let (_, _, gb_body) = http_post(
        port, "/api/groupby",
        r#"{"df":0,"by":["region","product"],"agg":{"units":"sum"}}"#,
    );
    assert!(gb_body.contains("\"df\":1"));
    let (status, _head, schema_body) = http_get(port, "/api/schema?df=1");
    assert_eq!(status, 200, "body: {schema_body}");
    assert!(schema_body.contains("\"index_nlevels\":2"), "body: {schema_body}");
    assert!(schema_body.contains("\"index_names\":[\"region\",\"product\"]"),
            "body: {schema_body}");
    assert!(schema_body.contains("\"index_dtypes\":[\"str\",\"str\"]"),
            "body: {schema_body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn rows_endpoint_includes_index_array_when_present() {
    let port = 35815u16;
    let src = pivot_df_serve_source(port, 1500);
    let path = compile_snippet("rows_idx", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (_, _, _) = http_post(
        port, "/api/groupby",
        r#"{"df":0,"by":["region"],"agg":{"units":"sum"}}"#,
    );
    let (status, _head, body) = http_get(port, "/api/rows?df=1&start=0&stop=2");
    assert_eq!(status, 200, "body: {body}");
    // Each row entry has a parallel index entry.
    assert!(body.contains("\"index\":["), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn rows_endpoint_index_is_empty_arrays_for_rangeindex() {
    let port = 35816u16;
    let src = pivot_df_serve_source(port, 1500);
    let path = compile_snippet("rows_no_idx", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    // Primary frame has no index (RangeIndex).
    let (status, _head, body) = http_get(port, "/api/rows?df=0&start=0&stop=2");
    assert_eq!(status, 200);
    // Each row gets an empty `[]` in the index array.
    assert!(body.contains("\"index\":[[],[]]"), "body: {body}");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}

#[test]
fn html_includes_m50c_groupby_sort_checkbox() {
    let port = 35813u16;
    let src = pivot_df_serve_source(port, 1500);
    let path = compile_snippet("html_gby_sort", &src);
    let server = std::thread::spawn(move || run_file_capture(&path).expect("run"));
    let (status, _head, body) = http_get(port, "/");
    assert_eq!(status, 200);
    assert!(body.contains("gby-sort"), "missing gby-sort checkbox id");
    assert!(body.contains("sort by key"), "missing sort label");
    let (rc, _) = server.join().expect("join");
    assert_eq!(rc, 0);
}
