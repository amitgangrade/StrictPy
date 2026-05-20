//! Subprocess integration tests for `examples/socket_demo.spy` and
//! `examples/socket_udp_demo.spy` (M28 P3b-A).
//!
//! Both demos are pure loopback (127.0.0.1) so the suite never
//! touches the public network — running under `cargo test --release`
//! on an offline build host should still pass.  The TCP demo spawns
//! one worker thread plus an accept loop; the UDP demo runs purely
//! single-threaded with two sockets.

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
fn socket_demo_compiles() {
    let src_path = project_root().join("examples").join("socket_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read socket_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile socket_demo.spy: {e}"));
}

#[test]
fn socket_udp_demo_compiles() {
    let src_path = project_root().join("examples").join("socket_udp_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read socket_udp_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile socket_udp_demo.spy: {e}"));
}

#[test]
fn socket_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("socket_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read socket_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile socket_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("socket_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    let output = Command::new(&spy_bin)
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

    // Resolver probes (StrictPy prints bool as lowercase true/false).
    assert!(
        stdout.contains("resolved-len-nonzero=true"),
        "gethostbyname; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("resolve-count-nonzero=true"),
        "resolve; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("hostname-nonempty=true"),
        "gethostname; stdout:\n{stdout}"
    );

    // Bound + handshake
    assert!(
        stdout.contains("bound-loopback=true"),
        "bound-loopback; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("peer-starts-with-127=true"),
        "peer-starts-with-127; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("local-addr-nonempty=true"),
        "local-addr; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("got-request=ping"),
        "got-request; stdout:\n{stdout}"
    );
    assert!(stdout.contains("reply=pong"), "reply; stdout:\n{stdout}");
    assert!(stdout.contains("done"), "done; stdout:\n{stdout}");
}

#[test]
fn socket_udp_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("socket_udp_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read socket_udp_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile socket_udp_demo.spy: {e}"));
    let spyc_path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("socket_udp_demo.spyc");
    fs::write(&spyc_path, &bytes).expect("write spyc");

    let spy_bin = project_root().join("target").join("release").join("spy.exe");
    if !spy_bin.exists() {
        eprintln!("skipping: {} not present", spy_bin.display());
        return;
    }

    let output = Command::new(&spy_bin)
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

    assert!(
        stdout.contains("udp-bound=true"),
        "udp-bound; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("client-sent=5"),
        "client-sent; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("server-recv=hello"),
        "server-recv; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("src-port-nonzero=true"),
        "src-port-nonzero; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("server-echoed=5"),
        "server-echoed; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("client-echo=hello"),
        "client-echo; stdout:\n{stdout}"
    );
    assert!(stdout.contains("done"), "done; stdout:\n{stdout}");
}
