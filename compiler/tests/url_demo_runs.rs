//! Subprocess integration test for `examples/url_demo.spy` (M22 P2D).
//!
//! Compiles + runs the URL demo through `spy.exe` and asserts the
//! printed output covers quote/unquote (both ascii + unicode +
//! plus-form), urlencode + parse_query round-trip, and the bad-escape
//! ValueError path.

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
fn url_demo_compiles() {
    let src_path = project_root().join("examples").join("url_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read url_demo.spy");
    let _bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile url_demo.spy: {e}"));
}

#[test]
fn url_demo_runs_via_spy_exe() {
    let src_path = project_root().join("examples").join("url_demo.spy");
    let src = fs::read_to_string(&src_path).expect("read url_demo.spy");
    let bytes = compile_source(src_path.display().to_string(), &src)
        .unwrap_or_else(|e| panic!("compile url_demo.spy: {e}"));
    let spyc_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("url_demo.spyc");
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

    // quote / quote_plus.
    assert!(stdout.contains("quote ascii: a%20b%2Fc%3Dd"), "stdout:\n{stdout}");
    assert!(stdout.contains("quote_plus: hello+world"), "stdout:\n{stdout}");
    // Unreserved set passes through.
    assert!(
        stdout.contains("unreserved: foo.bar_baz-qux~zot"),
        "stdout:\n{stdout}"
    );
    // Unicode (☃ → %E2%98%83) round-trip.
    assert!(
        stdout.contains("quote unicode: snow%3D%E2%98%83"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("unicode round-trip: ok"), "stdout:\n{stdout}");
    // unquote / unquote_plus.
    assert!(stdout.contains("unquote ascii: a b/c=d"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("unquote_plus: hello world foo"),
        "stdout:\n{stdout}"
    );
    // urlencode + parse_query round-trip.
    assert!(
        stdout.contains("urlencode: k=v&a=b+c&greet=hi+there%21"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("parse_query count: 3"), "stdout:\n{stdout}");
    assert!(stdout.contains("  k = v"), "stdout:\n{stdout}");
    assert!(stdout.contains("  a = b c"), "stdout:\n{stdout}");
    assert!(stdout.contains("  greet = hi there!"), "stdout:\n{stdout}");
    // Edge cases.
    assert!(stdout.contains("empty query len: 0"), "stdout:\n{stdout}");
    assert!(stdout.contains("nokey count: 2"), "stdout:\n{stdout}");
    assert!(stdout.contains("nokey[0]: flag=[]"), "stdout:\n{stdout}");
    // Bad escape raises ValueError.
    assert!(stdout.contains("caught bad escape"), "stdout:\n{stdout}");
    assert!(stdout.contains("done"), "stdout:\n{stdout}");
}
