//! Integration test for `examples/ssl_demo.spy` (M28 P3b-B).
//!
//! Spawns a loopback TLS echo server inside this Rust test process,
//! generates a fresh self-signed cert with `rcgen` at test-time, then
//! invokes `spy.exe` against the compiled `.spyc` with the server's
//! port on argv.  The demo (StrictPy side) is the *client*; this Rust
//! harness is the *server*.
//!
//! Network-bound test discipline: nothing here ever leaves loopback
//! (127.0.0.1).  We bind to port 0 so the OS picks a free port; this
//! avoids collisions with anything the dev machine happens to be
//! running on a hard-coded port.  No public TLS endpoints, no
//! crates.io / pypi.org / google.com checks.  The whole test is
//! self-contained.

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn ssl_demo_compiles() {
    let src_path = project_root().join("examples").join("ssl_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read ssl_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile ssl_demo.spy: {e}"));
}

#[test]
fn ssl_demo_runs_via_spy_exe() {
    // 1. Compile the demo.
    let src_path = project_root().join("examples").join("ssl_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read ssl_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile ssl_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("ssl_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    // 2. Generate a self-signed cert for CN=localhost with the rcgen
    //    one-liner helper.  Algorithm defaults to ECDSA P-256 which
    //    rustls + ring both support out of the box.
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])
        .expect("rcgen self-signed cert");
    let cert_der = rustls_pki_types::CertificateDer::from(cert.cert.der().to_vec());
    let key_der = rustls_pki_types::PrivateKeyDer::Pkcs8(
        rustls_pki_types::PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()),
    );

    // 3. Build the server-side rustls config.
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let server_cfg = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("server protocol versions")
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .expect("server cert install");
    let server_cfg = Arc::new(server_cfg);

    // 4. Spin up the loopback echo server on a free port chosen by
    //    the OS (port 0 -> ephemeral).  Background thread accepts one
    //    client, echoes everything until EOF, then exits.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local_addr").port();

    let server_cfg_for_thread = server_cfg.clone();
    let server_thread = std::thread::spawn(move || {
        // Accept exactly one connection (the demo only opens one).
        let (mut tcp, _peer) = match listener.accept() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("server accept failed: {e}");
                return;
            }
        };
        // Sane per-side deadlines so a misbehaving client doesn't
        // wedge the test forever.
        tcp.set_read_timeout(Some(std::time::Duration::from_secs(20))).ok();
        tcp.set_write_timeout(Some(std::time::Duration::from_secs(20))).ok();

        let mut conn = match rustls::ServerConnection::new(server_cfg_for_thread) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("server connection: {e}");
                return;
            }
        };
        let mut stream = rustls::Stream::new(&mut conn, &mut tcp);

        // Echo loop: read any plaintext that decodes, write it back,
        // until the client closes the connection.
        let mut buf = [0u8; 4096];
        loop {
            match stream.read(&mut buf) {
                Ok(0) => return,
                Ok(n) => {
                    if let Err(e) = stream.write_all(&buf[..n]) {
                        eprintln!("server write: {e}");
                        return;
                    }
                    let _ = stream.flush();
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return,
                Err(e) => {
                    eprintln!("server read: {e}");
                    return;
                }
            }
        }
    });

    // 5. Run the StrictPy demo against the loopback server.
    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg("127.0.0.1")
        .arg(port.to_string())
        .current_dir(project_root())
        .output()
        .expect("invoke spy.exe");

    // Best-effort join; the server thread exits on EOF after the
    // demo closes its socket.
    let _ = server_thread.join();

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "spy.exe failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );

    // 6. Assert each demo waypoint printed its `ok` (or known-good)
    //    line.
    for needle in [
        "verify: off",
        "connected: true",
        "peer_addr: nonempty",
        "rt_a: ok",
        "rt_b: ok",
        "recv_bound: ok",
        "closed",
        "post_close: caught ValueError",
        "done",
    ] {
        assert!(
            stdout.contains(needle),
            "missing {:?}; stdout:\n{stdout}\nstderr:\n{stderr}",
            needle
        );
    }
}
