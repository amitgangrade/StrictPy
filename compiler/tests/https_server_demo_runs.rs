//! Integration test for `examples/https_server_demo.spy` (M28.5 P3b-D).
//!
//! Generates a self-signed cert with `rcgen` at test time, writes the
//! cert + key as PEM files to a tempdir, and invokes `spy.exe` against
//! the compiled `.spyc` with the two PEM paths on argv.  The demo
//! itself spawns BOTH the server (using ssl.accept_tls) and the
//! client (using ssl.connect) inside StrictPy — this Rust harness is
//! purely the cert-issuer + test-driver.
//!
//! Network-bound discipline: same as M28 P3b-B's ssl_demo_runs.rs.
//! 127.0.0.1 only, port chosen by walking a fixed range to dodge
//! "address in use" collisions, no public TLS endpoints.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use strictpy_compiler::compile_source;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

#[test]
fn https_server_demo_compiles() {
    let src_path = project_root().join("examples").join("https_server_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read https_server_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile https_server_demo.spy: {e}"));
}

#[test]
fn https_server_demo_runs_via_spy_exe() {
    // 1. Compile the demo.
    let src_path = project_root().join("examples").join("https_server_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read https_server_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile https_server_demo.spy: {e}"));
    let tmp_root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("https_server_demo");
    fs::create_dir_all(&tmp_root).expect("create tmp_root");
    let spyc_path = tmp_root.join("https_server_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    // 2. Generate a fresh self-signed cert for CN=localhost.  rcgen's
    //    `generate_simple_self_signed` returns a `CertifiedKey` whose
    //    `cert` exposes a PEM serializer and whose `key_pair` exposes
    //    PKCS#8 PEM serialization out of the box.  Both files go in a
    //    tempdir alongside the .spyc.
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])
        .expect("rcgen self-signed cert");
    let cert_pem_path = tmp_root.join("server.crt");
    let key_pem_path = tmp_root.join("server.key");
    fs::write(&cert_pem_path, cert.cert.pem()).expect("write cert pem");
    fs::write(&key_pem_path, cert.key_pair.serialize_pem()).expect("write key pem");

    // 3. Run the StrictPy demo with the PEM paths on argv.
    let output = Command::new(&spy_bin)
        .arg(&spyc_path)
        .arg(cert_pem_path.display().to_string())
        .arg(key_pem_path.display().to_string())
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

    // 4. Assert each waypoint of the demo printed its expected line.
    for needle in [
        "server_cfg: loaded",
        "bound: true",
        "accepted: true",
        "server.peer_addr: nonempty",
        "server.recv_exact: ok",
        "server.send: ok",
        "server.close: ok",
        "client.status: ok",
        "round_trip: ok",
        "done",
    ] {
        assert!(
            stdout.contains(needle),
            "missing {:?}; stdout:\n{stdout}\nstderr:\n{stderr}",
            needle
        );
    }
}
