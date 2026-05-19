//! M25 — unified `spy` CLI integration tests.
//!
//! Exercises the Python-analogous behaviour of the single `spy` binary:
//!
//! - `spy file.spy [args]`         → compile-if-stale + run; cache lands in
//!                                   `__spycache__/file.spyc` next to the source.
//! - `spy file.spyc [args]`        → run cached module directly.
//! - `spy -c "code" [args]`        → compile inline and run; never cached.
//! - `spy --compile-only file.spy` → compile only; default output is the
//!                                   `__spycache__/file.spyc` path.
//! - `spy --compile-only file.spy -o custom.spyc` → write custom output.
//! - Stale cache (source newer than cached) triggers a recompile.
//! - Fresh cache (source older than cached) is reused.
//!
//! Each test writes its inputs into `CARGO_TARGET_TMPDIR/m25_<name>/` so
//! parallel tests don't collide.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p
}

fn spy_bin() -> PathBuf {
    let exe = if cfg!(windows) { "spy.exe" } else { "spy" };
    project_root().join("target").join("release").join(exe)
}

fn case_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("m25_{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create m25 case dir");
    dir
}

const HELLO_SOURCE: &str = "fn main() -> i32:\n    println(\"hello from m25\")\n    return 0\n";

fn skip_if_no_binary() -> bool {
    if !spy_bin().exists() {
        eprintln!("skipping: {} not present", spy_bin().display());
        return true;
    }
    false
}

#[test]
fn m25_run_dot_spy_produces_cache_and_runs() {
    if skip_if_no_binary() {
        return;
    }
    let dir = case_dir("run_dot_spy");
    let src = dir.join("hello.spy");
    fs::write(&src, HELLO_SOURCE).unwrap();

    let cached = dir.join("__spycache__").join("hello.spyc");
    assert!(!cached.exists(), "cache must not exist before first run");

    let out = Command::new(spy_bin())
        .arg(&src)
        .output()
        .expect("invoke spy");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        out.status.success(),
        "spy failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status
    );
    assert!(stdout.contains("hello from m25"), "got: {stdout:?}");
    assert!(cached.exists(), "cache must be created on first .spy run");
}

#[test]
fn m25_run_dot_spyc_directly() {
    if skip_if_no_binary() {
        return;
    }
    let dir = case_dir("run_dot_spyc");
    let src = dir.join("hello.spy");
    fs::write(&src, HELLO_SOURCE).unwrap();

    // Compile first via --compile-only with an explicit -o path.
    let bc = dir.join("hello.spyc");
    let compile_out = Command::new(spy_bin())
        .arg("--compile-only")
        .arg(&src)
        .arg("-o")
        .arg(&bc)
        .output()
        .expect("invoke spy --compile-only");
    assert!(
        compile_out.status.success(),
        "--compile-only failed: {}",
        String::from_utf8_lossy(&compile_out.stderr)
    );
    assert!(bc.exists(), "explicit -o output not written");

    // Run the .spyc directly.
    let run_out = Command::new(spy_bin()).arg(&bc).output().expect("run .spyc");
    let stdout = String::from_utf8_lossy(&run_out.stdout).to_string();
    assert!(run_out.status.success(), "spy on .spyc failed");
    assert!(stdout.contains("hello from m25"));
}

#[test]
fn m25_dash_c_inline_executes() {
    if skip_if_no_binary() {
        return;
    }
    let out = Command::new(spy_bin())
        .arg("-c")
        .arg("fn main() -> i32:\n    println(\"inline ok\")\n    return 0\n")
        .output()
        .expect("invoke spy -c");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        out.status.success(),
        "spy -c failed; status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status
    );
    assert!(stdout.contains("inline ok"), "got: {stdout:?}");
}

#[test]
fn m25_compile_only_default_lands_in_spycache() {
    if skip_if_no_binary() {
        return;
    }
    let dir = case_dir("compile_only_default");
    let src = dir.join("hello.spy");
    fs::write(&src, HELLO_SOURCE).unwrap();

    let cached = dir.join("__spycache__").join("hello.spyc");
    let out = Command::new(spy_bin())
        .arg("--compile-only")
        .arg(&src)
        .output()
        .expect("invoke spy --compile-only");
    assert!(
        out.status.success(),
        "--compile-only failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(cached.exists(), "default cache path not produced");
}

#[test]
fn m25_stale_cache_recompiles() {
    if skip_if_no_binary() {
        return;
    }
    let dir = case_dir("stale_cache");
    let src = dir.join("greet.spy");
    fs::write(&src, "fn main() -> i32:\n    println(\"v1\")\n    return 0\n").unwrap();

    // First run produces v1 in cache.
    let r1 = Command::new(spy_bin()).arg(&src).output().expect("run 1");
    assert!(r1.status.success());
    assert!(String::from_utf8_lossy(&r1.stdout).contains("v1"));

    let cached = dir.join("__spycache__").join("greet.spyc");
    assert!(cached.exists());

    // Sleep past NTFS/ext4 mtime granularity (~10ms on Windows,
    // 1s on some ext4 mounts) so the rewrite's mtime is strictly
    // greater than the cache's.
    thread::sleep(Duration::from_millis(1500));
    fs::write(&src, "fn main() -> i32:\n    println(\"v2\")\n    return 0\n").unwrap();

    let r2 = Command::new(spy_bin()).arg(&src).output().expect("run 2");
    assert!(r2.status.success());
    let s2 = String::from_utf8_lossy(&r2.stdout).to_string();
    assert!(s2.contains("v2"), "stale cache not recompiled; got {s2:?}");
    assert!(
        !s2.contains("v1"),
        "old cached bytecode still being used; got {s2:?}"
    );
}

#[test]
fn m25_fresh_cache_is_reused() {
    if skip_if_no_binary() {
        return;
    }
    let dir = case_dir("fresh_cache");
    let src = dir.join("once.spy");
    fs::write(&src, HELLO_SOURCE).unwrap();

    // First run creates cache.
    let r1 = Command::new(spy_bin()).arg(&src).output().expect("run 1");
    assert!(r1.status.success());

    let cached = dir.join("__spycache__").join("once.spyc");
    let mt_after_first = fs::metadata(&cached).unwrap().modified().unwrap();

    // Second run with no source changes: cache mtime must NOT advance.
    let r2 = Command::new(spy_bin()).arg(&src).output().expect("run 2");
    assert!(r2.status.success());
    let mt_after_second = fs::metadata(&cached).unwrap().modified().unwrap();

    assert_eq!(
        mt_after_first, mt_after_second,
        "fresh cache was rewritten on second run (mtime advanced)"
    );
}

#[test]
fn m25_argv_passes_trailing_args() {
    if skip_if_no_binary() {
        return;
    }
    let dir = case_dir("argv");
    let src = dir.join("argv_echo.spy");
    let body = r#"import sys

fn main() -> i32:
    n: i64 = i64(len(sys.argv))
    println("argc=" + str(n))
    i: i64 = 1
    while i < n:
        println("arg=" + sys.argv[i])
        i = i + 1
    return 0
"#;
    fs::write(&src, body).unwrap();

    let out = Command::new(spy_bin())
        .arg(&src)
        .arg("first")
        .arg("second")
        .arg("third")
        .output()
        .expect("invoke spy with trailing args");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        out.status.success(),
        "spy failed: status={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status
    );
    // argv[0] is the script path itself; trailing args are 1..4.
    assert!(stdout.contains("argc=4"), "got:\n{stdout}");
    assert!(stdout.contains("arg=first"), "got:\n{stdout}");
    assert!(stdout.contains("arg=second"), "got:\n{stdout}");
    assert!(stdout.contains("arg=third"), "got:\n{stdout}");
}

#[test]
fn m25_unknown_extension_errors() {
    if skip_if_no_binary() {
        return;
    }
    let dir = case_dir("unknown_ext");
    let bogus = dir.join("foo.txt");
    fs::write(&bogus, HELLO_SOURCE).unwrap();

    let out = Command::new(spy_bin())
        .arg(&bogus)
        .output()
        .expect("invoke spy with non-spy/spyc file");
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(!out.status.success(), "spy must reject non-spy/spyc");
    assert!(
        stderr.contains("unrecognised file extension"),
        "expected error message about extension; got:\n{stderr}"
    );
}
