//! M35 P4-C integration tests for the streaming `Hasher` class.
//!
//! Mirrors the M34 JsonValue test structure: each test compiles a
//! small `.spy` snippet, runs it through `run_file_capture`, and
//! asserts on stdout.
//!
//! The Hasher class is registered in the resolver prelude alongside
//! io.File / Channel / Thread; the actual hash state lives in
//! `SharedVm.hashers` (a `HashMap<i64, HasherSlot>`) and the heap-side
//! `HasherRepr` carries an i64 handle into that table.  These tests
//! exercise the full path: `hashlib.new(algo)` -> alloc + insert,
//! `h.update(chunk)` -> table-lookup + dispatch by HasherState variant,
//! `h.hexdigest()` -> clone-then-finalize so the original Hasher
//! remains usable for further updates.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m35_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

fn run_capture(path: &PathBuf) -> String {
    let (_code, out) = run_file_capture(path)
        .unwrap_or_else(|e| panic!("run error: {e:?}"));
    out
}

#[test]
fn sha256_incremental_matches_one_shot() {
    // Build the hash by feeding three chunks, then verify the digest
    // equals `hashlib.sha256("hello, world")` of the concatenated
    // input — this is the canonical M35 use case.
    let src = r#"
import hashlib

fn main() -> i32:
    h: Hasher = hashlib.new("sha256")
    h.update("hello")
    h.update(", ")
    h.update("world")
    streamed: str = h.hexdigest()
    one_shot: str = hashlib.sha256("hello, world")
    println(streamed)
    println(one_shot)
    if streamed == one_shot:
        println("match")
    else:
        println("MISMATCH")
    return 0
"#;
    let p = compile_snippet("sha256_inc", src);
    let out = run_capture(&p);
    let lines: Vec<&str> = out.lines().collect();
    // Canonical sha256("hello, world") — known constant.
    let expected = "09ca7e4eaa6e8ae9c7d261167129184883644d07dfba7cbfbc4c8a2e08360d5b";
    assert_eq!(lines[0], expected, "streaming digest: {}", out);
    assert_eq!(lines[1], expected, "one-shot digest:  {}", out);
    assert_eq!(lines[2], "match", "digests should agree");
}

#[test]
fn sha1_incremental_matches_one_shot() {
    let src = r#"
import hashlib

fn main() -> i32:
    h: Hasher = hashlib.new("sha1")
    h.update("abc")
    h.update("defg")
    streamed: str = h.hexdigest()
    one_shot: str = hashlib.sha1("abcdefg")
    println(streamed)
    if streamed == one_shot:
        println("ok")
    else:
        println("bad")
    return 0
"#;
    let p = compile_snippet("sha1_inc", src);
    let out = run_capture(&p);
    let lines: Vec<&str> = out.lines().collect();
    // sha1("abcdefg")
    assert_eq!(lines[0], "2fb5e13419fc89246865e7a324f476ec624e8740", "out: {}", out);
    assert_eq!(lines[1], "ok");
}

#[test]
fn sha512_incremental_matches_one_shot() {
    let src = r#"
import hashlib

fn main() -> i32:
    h: Hasher = hashlib.new("sha512")
    h.update("The quick brown fox ")
    h.update("jumps over the lazy dog")
    streamed: str = h.hexdigest()
    one_shot: str = hashlib.sha512("The quick brown fox jumps over the lazy dog")
    if streamed == one_shot:
        println("ok")
    else:
        println(streamed)
        println(one_shot)
    return 0
"#;
    let p = compile_snippet("sha512_inc", src);
    let out = run_capture(&p);
    assert_eq!(out.trim(), "ok", "out: {}", out);
}

#[test]
fn md5_incremental_matches_one_shot() {
    let src = r#"
import hashlib

fn main() -> i32:
    h: Hasher = hashlib.new("md5")
    h.update("foo")
    h.update("bar")
    h.update("baz")
    streamed: str = h.hexdigest()
    one_shot: str = hashlib.md5("foobarbaz")
    println(streamed)
    if streamed == one_shot:
        println("ok")
    else:
        println("bad")
    return 0
"#;
    let p = compile_snippet("md5_inc", src);
    let out = run_capture(&p);
    let lines: Vec<&str> = out.lines().collect();
    // md5("foobarbaz")
    assert_eq!(lines[0], "6df23dc03f9b54cc38a0fc1483df6e21", "out: {}", out);
    assert_eq!(lines[1], "ok");
}

#[test]
fn bad_algorithm_name_raises_value_error() {
    let src = r#"
import hashlib

fn main() -> i32:
    try:
        h: Hasher = hashlib.new("blake2b")
        println("no_raise")
    except ValueError as e:
        println("caught:" + e.message)
    return 0
"#;
    let p = compile_snippet("bad_algo", src);
    let out = run_capture(&p);
    assert!(out.starts_with("caught:"), "expected ValueError, got: {}", out);
    // Message should mention the algo we passed.
    assert!(out.contains("blake2b"), "out: {}", out);
}

#[test]
fn empty_input_matches_empty_string_digest() {
    // sha256("") is the canonical empty-string digest.  The streaming
    // path should produce the same byte-for-byte.
    let src = r#"
import hashlib

fn main() -> i32:
    h: Hasher = hashlib.new("sha256")
    streamed: str = h.hexdigest()
    one_shot: str = hashlib.sha256("")
    println(streamed)
    if streamed == one_shot:
        println("ok")
    else:
        println("bad")
    return 0
"#;
    let p = compile_snippet("empty_input", src);
    let out = run_capture(&p);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines[0], "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "out: {}", out
    );
    assert_eq!(lines[1], "ok");
}

#[test]
fn hexdigest_is_idempotent_under_further_updates() {
    // Per spec §9.X: hexdigest() clones-not-consumes, so the user can
    // call it for intermediate digests and keep updating afterwards.
    let src = r#"
import hashlib

fn main() -> i32:
    h: Hasher = hashlib.new("sha256")
    h.update("hello")
    d1: str = h.hexdigest()
    d1b: str = h.hexdigest()
    h.update(", world")
    d2: str = h.hexdigest()
    hello: str = hashlib.sha256("hello")
    helloworld: str = hashlib.sha256("hello, world")
    if d1 == d1b:
        println("idempotent")
    else:
        println("BAD_IDEM")
    if d1 == hello:
        println("d1_ok")
    else:
        println("BAD_D1")
    if d2 == helloworld:
        println("d2_ok")
    else:
        println("BAD_D2")
    return 0
"#;
    let p = compile_snippet("hexdigest_idem", src);
    let out = run_capture(&p);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "idempotent", "out: {}", out);
    assert_eq!(lines[1], "d1_ok", "out: {}", out);
    assert_eq!(lines[2], "d2_ok", "out: {}", out);
}

#[test]
fn algorithm_method_returns_canonical_name() {
    // Hasher.algorithm() returns the canonical name that was passed
    // to hashlib.new.
    let src = r#"
import hashlib

fn main() -> i32:
    h1: Hasher = hashlib.new("sha256")
    h2: Hasher = hashlib.new("md5")
    println(h1.algorithm())
    println(h2.algorithm())
    return 0
"#;
    let p = compile_snippet("algorithm_method", src);
    let out = run_capture(&p);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "sha256");
    assert_eq!(lines[1], "md5");
}
