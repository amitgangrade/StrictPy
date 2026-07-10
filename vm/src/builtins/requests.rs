//! M65 (wave-3 Lane A): `requests` module engine.
//!
//! A requests-shaped ergonomic HTTP layer over the same `ureq` engine
//! that powers `http_client` (§9.42).  This module holds the pure engine
//! side — the slot payload structs and the transport helpers.  The
//! NativeFn dispatch arms (in `builtins.rs`) own the VM-object plumbing:
//! reading the `handle` field off a `Response`/`Session` heap object,
//! inserting freshly-built slots, and marshalling strings/dicts.
//!
//! Frozen contract: STRICTPY_SPEC.md §9.51.

use std::io::Read;

use crate::error::VmError;

/// Response bodies are read fully into the slot at request time, capped
/// at 10 MiB (matches ureq's `into_string` default); larger bodies raise
/// `IOError`.  `download` streams to disk instead and has no cap.
pub const REQ_BODY_CAP: u64 = 10 * 1024 * 1024;

/// Persistent per-`Session` state.  Holds a `ureq::Agent` (connection
/// pool + cookie jar via the `cookies` feature) plus the mutable config
/// the setter methods adjust.  Changing a config knob rebuilds the agent
/// (setters run before requests in the typical flow, so the cookie jar —
/// empty until a response arrives — is not lost in practice).
pub struct ReqSessionSlot {
    pub agent: ureq::Agent,
    /// Default headers layered onto every request (lowest precedence).
    pub default_headers: Vec<(String, String)>,
    /// Connect+read timeout in ms; default 30000.
    pub timeout_ms: i64,
    /// Redirect follow limit; default 10, 0 = no follow.
    pub max_redirects: i64,
}

impl ReqSessionSlot {
    pub fn new() -> Self {
        ReqSessionSlot {
            agent: build_agent(30_000, 10),
            default_headers: Vec::new(),
            timeout_ms: 30_000,
            max_redirects: 10,
        }
    }

    /// Rebuild the underlying agent to reflect the current timeout /
    /// redirect config.
    pub fn rebuild_agent(&mut self) {
        self.agent = build_agent(self.timeout_ms, self.max_redirects);
    }
}

impl Default for ReqSessionSlot {
    fn default() -> Self {
        Self::new()
    }
}

/// Materialised response state.  The body rides the str-as-byte-buffer
/// convention (§9.40) so binary downloads round-trip exactly; `text()`
/// re-reads it, `json()` re-parses it.  `headers` keys are lowercased.
pub struct ReqResponseSlot {
    pub status: i64,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl ReqResponseSlot {
    /// Case-insensitive single-header lookup; `""` when absent.
    pub fn header(&self, name: &str) -> String {
        let want = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| *k == want)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }
}

/// Build a fresh agent with the given timeout / redirect config.  A
/// non-positive timeout falls back to the 30 s default.
pub fn build_agent(timeout_ms: i64, max_redirects: i64) -> ureq::Agent {
    let t = if timeout_ms <= 0 { 30_000 } else { timeout_ms };
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_millis(t as u64))
        .redirects(max_redirects.max(0) as u32)
        .user_agent("StrictPy/0.3 requests")
        .build()
}

fn collect_headers(resp: &ureq::Response) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for name in resp.headers_names() {
        if let Some(v) = resp.header(&name) {
            out.push((name.to_ascii_lowercase(), v.to_string()));
        }
    }
    out
}

fn read_body_capped(resp: ureq::Response, who: &str) -> Result<Vec<u8>, VmError> {
    // Read one byte past the cap so an over-cap body is detectable.
    let mut reader = resp.into_reader().take(REQ_BODY_CAP + 1);
    let mut buf: Vec<u8> = Vec::new();
    reader
        .read_to_end(&mut buf)
        .map_err(|e| VmError::UncaughtException {
            type_name: "IOError".into(),
            message: format!("requests.{}: read body: {}", who, e),
        })?;
    if buf.len() as u64 > REQ_BODY_CAP {
        return Err(VmError::UncaughtException {
            type_name: "IOError".into(),
            message: format!("requests.{}: response body exceeds 10 MiB cap", who),
        });
    }
    Ok(buf)
}

/// Execute a request against `agent`, returning a filled response slot.
///
/// Transport-level failures (DNS, connect, TLS, timeout) raise `IOError`;
/// HTTP error *statuses* (4xx/5xx) do NOT raise — they come back as a
/// normal slot (Python-`requests` parity).  `body` carries the raw
/// request bytes for methods that send one.
pub fn execute(
    agent: &ureq::Agent,
    method: &str,
    url: &str,
    default_headers: &[(String, String)],
    per_req_headers: &[(String, String)],
    body: Option<&[u8]>,
    who: &str,
) -> Result<ReqResponseSlot, VmError> {
    let mut req = agent.request(method, url);
    for (k, v) in default_headers {
        req = req.set(k, v);
    }
    for (k, v) in per_req_headers {
        req = req.set(k, v);
    }
    let result = match body {
        Some(b) => req.send_bytes(b),
        None => req.call(),
    };
    match result {
        Ok(resp) => {
            let status = resp.status() as i64;
            let final_url = resp.get_url().to_string();
            let headers = collect_headers(&resp);
            let bytes = read_body_capped(resp, who)?;
            Ok(ReqResponseSlot { status, url: final_url, headers, body: bytes })
        }
        // 4xx / 5xx — surfaced as a normal response, not an error.
        Err(ureq::Error::Status(code, resp)) => {
            let status = code as i64;
            let final_url = resp.get_url().to_string();
            let headers = collect_headers(&resp);
            let bytes = read_body_capped(resp, who).unwrap_or_default();
            Ok(ReqResponseSlot { status, url: final_url, headers, body: bytes })
        }
        Err(e) => Err(VmError::UncaughtException {
            type_name: "IOError".into(),
            message: format!("requests.{}({:?}): {}", who, url, e),
        }),
    }
}

/// Stream a GET body to `path`, no size cap.  Returns bytes written.
pub fn download(
    agent: &ureq::Agent,
    url: &str,
    default_headers: &[(String, String)],
    path: &str,
    who: &str,
) -> Result<i64, VmError> {
    let mut req = agent.request("GET", url);
    for (k, v) in default_headers {
        req = req.set(k, v);
    }
    let resp = match req.call() {
        Ok(r) => r,
        // A 4xx/5xx still has a body worth writing (Python `requests`
        // does not raise on status either); write whatever came back.
        Err(ureq::Error::Status(_code, r)) => r,
        Err(e) => {
            return Err(VmError::UncaughtException {
                type_name: "IOError".into(),
                message: format!("requests.{}({:?}): {}", who, url, e),
            });
        }
    };
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(path).map_err(|e| VmError::UncaughtException {
        type_name: "IOError".into(),
        message: format!("requests.{}: create {:?}: {}", who, path, e),
    })?;
    let n = std::io::copy(&mut reader, &mut file).map_err(|e| VmError::UncaughtException {
        type_name: "IOError".into(),
        message: format!("requests.{}: write {:?}: {}", who, path, e),
    })?;
    Ok(n as i64)
}

/// Percent-encode one component per `application/x-www-form-urlencoded`
/// (spaces become `+`; unreserved chars pass through).
fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// urlencode a set of key/value pairs into `k=v&k2=v2` form.
pub fn urlencode_pairs(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", pct_encode(k), pct_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Append urlencoded `params` to `url`'s query string, choosing `?` or
/// `&` based on whether a query is already present.  Empty params return
/// the URL unchanged.
pub fn merge_query(url: &str, params: &[(String, String)]) -> String {
    if params.is_empty() {
        return url.to_string();
    }
    let encoded = urlencode_pairs(params);
    let sep = if url.contains('?') { '&' } else { '?' };
    format!("{}{}{}", url, sep, encoded)
}
