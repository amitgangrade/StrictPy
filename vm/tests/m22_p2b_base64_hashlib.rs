//! M22 P2B in-process regression tests for the `base64` and `hashlib`
//! stdlib modules.  Follows the M19/M20a/M20b/M20c pattern: compile a
//! tiny `.spy` snippet, write the bytecode to `CARGO_TARGET_TMPDIR`,
//! run through `run_file_capture`, and assert on stdout.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m22_p2b_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── base64 module ──────────────────────────────────────────────────────

#[test]
fn base64_encode_decode_round_trip() {
    let src = "\
import base64
fn main() -> i32:
    plain: str = \"Hello, StrictPy!\"
    enc: str = base64.encode(plain)
    dec: str = base64.decode(enc)
    println(\"enc=\" + enc)
    if dec == plain:
        println(\"round-trip: ok\")
    return 0
";
    let p = compile_snippet("base64_round_trip", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(
        out.contains("enc=SGVsbG8sIFN0cmljdFB5IQ=="),
        "expected std-base64 encoding; got: {out:?}"
    );
    assert!(out.contains("round-trip: ok"), "got: {out:?}");
}

#[test]
fn base64_known_vector_man() {
    // RFC 4648 §10 test vector: "Man" -> "TWFu".
    let src = "\
import base64
fn main() -> i32:
    println(base64.encode(\"Man\"))
    println(base64.decode(\"TWFu\"))
    return 0
";
    let p = compile_snippet("base64_man", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("TWFu"), "encoding; got: {out:?}");
    assert!(out.contains("Man"), "decoding; got: {out:?}");
}

#[test]
fn base64_empty_string_round_trip() {
    let src = "\
import base64
fn main() -> i32:
    e: str = base64.encode(\"\")
    println(\"e=[\" + e + \"]\")
    d: str = base64.decode(\"\")
    println(\"d=[\" + d + \"]\")
    return 0
";
    let p = compile_snippet("base64_empty", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("e=[]"), "got: {out:?}");
    assert!(out.contains("d=[]"), "got: {out:?}");
}

#[test]
fn base64_url_safe_uses_alt_alphabet() {
    // Pick input whose standard encoding contains `+`/`/`/`=` so the
    // URL-safe form is observably different.  `?` is 0x3F, `>` is 0x3E
    // — running them through the encoder produces both `+` and `/`.
    let src = "\
import base64
fn main() -> i32:
    raw: str = \"??>>\"
    std_enc: str = base64.encode(raw)
    url_enc: str = base64.encode_url_safe(raw)
    println(\"std=\" + std_enc)
    println(\"url=\" + url_enc)
    if base64.decode_url_safe(url_enc) == raw:
        println(\"url round-trip: ok\")
    return 0
";
    let p = compile_snippet("base64_url_safe", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // Standard encoding of "??>>" = 0x3F 0x3F 0x3E 0x3E -> "Pz8+Pg=="
    assert!(out.contains("std=Pz8+Pg=="), "got: {out:?}");
    // URL-safe encoding replaces `+` with `-` and drops padding.
    assert!(out.contains("url=Pz8-Pg"), "got: {out:?}");
    assert!(!out.contains("url=Pz8+"), "url-safe must not contain '+'; got: {out:?}");
    assert!(out.contains("url round-trip: ok"), "got: {out:?}");
}

#[test]
fn base64_decode_raises_value_error_on_bad_input() {
    let src = "\
import base64
fn main() -> i32:
    try:
        bad: str = base64.decode(\"!!!\")
        println(\"UNREACHED \" + bad)
    except ValueError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("base64_bad", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

#[test]
fn base64_decode_url_safe_rejects_standard_alphabet() {
    // The URL-safe decoder must NOT accept '+' / '/' — those belong to
    // the standard alphabet.
    let src = "\
import base64
fn main() -> i32:
    try:
        bad: str = base64.decode_url_safe(\"a+b/\")
        println(\"UNREACHED \" + bad)
    except ValueError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("base64_url_bad", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught"), "got: {out:?}");
}

#[test]
fn base64_utf8_payload_round_trips() {
    // Multi-byte UTF-8 input (the euro sign is 3 bytes).
    let src = "\
import base64
fn main() -> i32:
    plain: str = \"5 \u{20AC}\"
    enc: str = base64.encode(plain)
    dec: str = base64.decode(enc)
    if dec == plain:
        println(\"utf8 ok\")
    else:
        println(\"utf8 FAIL\")
    return 0
";
    let p = compile_snippet("base64_utf8", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("utf8 ok"), "got: {out:?}");
}

// ── hashlib module ─────────────────────────────────────────────────────

#[test]
fn hashlib_md5_empty_vector() {
    let src = "\
import hashlib
fn main() -> i32:
    println(hashlib.md5(\"\"))
    return 0
";
    let p = compile_snippet("hashlib_md5_empty", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(
        out.contains("d41d8cd98f00b204e9800998ecf8427e"),
        "got: {out:?}"
    );
}

#[test]
fn hashlib_md5_pangram() {
    let src = "\
import hashlib
fn main() -> i32:
    s: str = \"The quick brown fox jumps over the lazy dog\"
    println(hashlib.md5(s))
    return 0
";
    let p = compile_snippet("hashlib_md5_pangram", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(
        out.contains("9e107d9d372bb6826bd81d3542a419d6"),
        "got: {out:?}"
    );
}

#[test]
fn hashlib_sha1_abc() {
    let src = "\
import hashlib
fn main() -> i32:
    println(hashlib.sha1(\"abc\"))
    return 0
";
    let p = compile_snippet("hashlib_sha1_abc", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(
        out.contains("a9993e364706816aba3e25717850c26c9cd0d89d"),
        "got: {out:?}"
    );
}

#[test]
fn hashlib_sha256_empty_and_abc() {
    let src = "\
import hashlib
fn main() -> i32:
    println(hashlib.sha256(\"\"))
    println(hashlib.sha256(\"abc\"))
    return 0
";
    let p = compile_snippet("hashlib_sha256", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(
        out.contains("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        "got: {out:?}"
    );
    assert!(
        out.contains("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        "got: {out:?}"
    );
}

#[test]
fn hashlib_sha512_abc() {
    let src = "\
import hashlib
fn main() -> i32:
    println(hashlib.sha512(\"abc\"))
    return 0
";
    let p = compile_snippet("hashlib_sha512_abc", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // NIST FIPS 180-4 §C.1.
    assert!(
        out.contains("ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"),
        "got: {out:?}"
    );
}

#[test]
fn hashlib_digest_lengths_are_canonical() {
    let src = "\
import hashlib
fn main() -> i32:
    println(\"md5=\" + str(len(hashlib.md5(\"x\"))))
    println(\"sha1=\" + str(len(hashlib.sha1(\"x\"))))
    println(\"sha256=\" + str(len(hashlib.sha256(\"x\"))))
    println(\"sha512=\" + str(len(hashlib.sha512(\"x\"))))
    return 0
";
    let p = compile_snippet("hashlib_lens", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("md5=32"), "got: {out:?}");
    assert!(out.contains("sha1=40"), "got: {out:?}");
    assert!(out.contains("sha256=64"), "got: {out:?}");
    assert!(out.contains("sha512=128"), "got: {out:?}");
}

#[test]
fn hashlib_hex_digest_is_lowercase() {
    // sha256("XYZ") begins with '0a92...' if lowercase; the uppercase
    // form would be '0A92...'.  Just make sure no uppercase hex appears.
    let src = "\
import hashlib
fn main() -> i32:
    h: str = hashlib.sha256(\"the quick brown fox jumps over the lazy dog\")
    println(h)
    return 0
";
    let p = compile_snippet("hashlib_lowercase", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // The known sha256 of that exact lowercase pangram.
    assert!(
        out.contains("05c6e08f1d9fdafa03147fcb8f82f124c76d2f70e3d989dc8aadb5e7d7450bec"),
        "got: {out:?}"
    );
}

#[test]
fn hashlib_hmac_sha256_known_vector() {
    // python3 -c "import hmac, hashlib;
    //   print(hmac.new(b'key',
    //   b'The quick brown fox jumps over the lazy dog',
    //   hashlib.sha256).hexdigest())"
    let src = "\
import hashlib
fn main() -> i32:
    out: str = hashlib.hmac_sha256(\"key\", \"The quick brown fox jumps over the lazy dog\")
    println(out)
    return 0
";
    let p = compile_snippet("hashlib_hmac", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(
        out.contains("f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"),
        "got: {out:?}"
    );
}

#[test]
fn hashlib_md5_is_deterministic() {
    // Sanity check: same input → same hash, different input → different.
    let src = "\
import hashlib
fn main() -> i32:
    a: str = hashlib.md5(\"hello\")
    b: str = hashlib.md5(\"hello\")
    c: str = hashlib.md5(\"world\")
    if a == b:
        println(\"same eq\")
    if a == c:
        println(\"BAD different eq\")
    else:
        println(\"diff neq\")
    return 0
";
    let p = compile_snippet("hashlib_det", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("same eq"));
    assert!(out.contains("diff neq"));
    assert!(!out.contains("BAD"));
}
