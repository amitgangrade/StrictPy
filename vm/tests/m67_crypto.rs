//! M67 (wave-3 Lane C) regression tests for the `crypto` module.
//!
//! Follows the M22/M34/M35 harness: compile a tiny `.spy` snippet, write
//! the bytecode to `CARGO_TARGET_TMPDIR`, run through `run_file_capture`,
//! and assert on stdout / exit code.
//!
//! Binary values ride the str-as-byte-buffer convention (spec §9.40): a
//! `\xNN` string escape produces codepoint 0..=255, i.e. one packed byte.
//! `esc()` turns a published hex vector into such a literal so we can feed
//! and compare byte buffers with plain string `==`.
//!
//! Published vectors exercised:
//!   * AES-256-GCM  — McGrew/Viega (NIST CAVS) Test Case 14: encrypt +
//!     decrypt + tampered-tag ValueError.
//!   * PBKDF2-HMAC-SHA256 — RFC 7914 §11 (P="passwd", S="salt", c=1, 64B).
//!   * HKDF-SHA256  — RFC 5869 Test Case 1.
//!   * Ed25519      — RFC 8032 §7.1 Test 1 (known-answer verify + wrong
//!     key false), plus a keygen/sign/verify round-trip.
//!   * constant_time_eq truth table.
//!   * JWT HS256 round-trip + wrong-key + alg-mismatch + expired-token
//!     ValueError; EdDSA round-trip.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m67_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

fn run(name: &str, src: &str) -> (i32, String) {
    let p = compile_snippet(name, src);
    run_file_capture(&p).expect("run")
}

/// Turn a hex string into a StrictPy string literal body of `\xNN`
/// escapes (one packed byte per escape).
fn esc(hex: &str) -> String {
    let bytes = hex_bytes(hex);
    let mut s = String::with_capacity(bytes.len() * 4);
    for b in bytes {
        s.push_str(&format!("\\x{b:02x}"));
    }
    s
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    let clean: String = hex.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(clean.len() % 2 == 0, "odd hex length");
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).expect("hex"))
        .collect()
}

// ── AES-256-GCM — McGrew/Viega / NIST CAVS Test Case 14 ─────────────────

const AES_KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const AES_IV: &str = "000000000000000000000000";
const AES_PT: &str = "00000000000000000000000000000000";
// ciphertext (16) || GCM tag (16)
const AES_CT_TAG: &str =
    "cea7403d4d606b6e074ec5d3baf39d18d0d1c8a799996bf0265b98b5d48ab919";

#[test]
fn aes256gcm_nist_tc14_encrypt() {
    let src = format!(
        "import crypto
fn main() -> i32:
    key: str = \"{key}\"
    nonce: str = \"{nonce}\"
    pt: str = \"{pt}\"
    ct: str = crypto.aes_gcm_encrypt(key, nonce, pt, \"\")
    if ct == \"{ct}\":
        println(\"PASS\")
    else:
        println(\"FAIL\")
    return 0\n",
        key = esc(AES_KEY),
        nonce = esc(AES_IV),
        pt = esc(AES_PT),
        ct = esc(AES_CT_TAG),
    );
    let (code, out) = run("aes_encrypt", &src);
    assert_eq!(code, 0, "out: {out:?}");
    assert!(out.contains("PASS"), "got: {out:?}");
    assert!(!out.contains("FAIL"), "got: {out:?}");
}

#[test]
fn aes256gcm_nist_tc14_decrypt() {
    let src = format!(
        "import crypto
fn main() -> i32:
    key: str = \"{key}\"
    nonce: str = \"{nonce}\"
    ct: str = \"{ct}\"
    pt: str = crypto.aes_gcm_decrypt(key, nonce, ct, \"\")
    if pt == \"{pt}\":
        println(\"PASS\")
    else:
        println(\"FAIL\")
    return 0\n",
        key = esc(AES_KEY),
        nonce = esc(AES_IV),
        ct = esc(AES_CT_TAG),
        pt = esc(AES_PT),
    );
    let (code, out) = run("aes_decrypt", &src);
    assert_eq!(code, 0, "out: {out:?}");
    assert!(out.contains("PASS"), "got: {out:?}");
    assert!(!out.contains("FAIL"), "got: {out:?}");
}

#[test]
fn aes256gcm_tampered_tag_raises_value_error() {
    // Flip the final tag byte 0x19 -> 0x18.
    let tampered =
        "cea7403d4d606b6e074ec5d3baf39d18d0d1c8a799996bf0265b98b5d48ab918";
    let src = format!(
        "import crypto
fn main() -> i32:
    key: str = \"{key}\"
    nonce: str = \"{nonce}\"
    ct: str = \"{ct}\"
    try:
        pt: str = crypto.aes_gcm_decrypt(key, nonce, ct, \"\")
        println(\"NOCATCH\")
    except ValueError as e:
        println(\"caught\")
    return 0\n",
        key = esc(AES_KEY),
        nonce = esc(AES_IV),
        ct = esc(tampered),
    );
    let (code, out) = run("aes_tampered", &src);
    assert_eq!(code, 0, "out: {out:?}");
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("NOCATCH"), "got: {out:?}");
}

#[test]
fn aes256gcm_bad_key_length_raises_value_error() {
    let src = "import crypto
fn main() -> i32:
    try:
        ct: str = crypto.aes_gcm_encrypt(\"short\", \"000000000000\", \"x\", \"\")
        println(\"NOCATCH\")
    except ValueError as e:
        println(\"caught\")
    return 0\n";
    let (code, out) = run("aes_bad_key", src);
    assert_eq!(code, 0, "out: {out:?}");
    assert!(out.contains("caught"), "got: {out:?}");
}

// ── PBKDF2-HMAC-SHA256 — RFC 7914 §11 ───────────────────────────────────

#[test]
fn pbkdf2_sha256_rfc7914_c1() {
    let expected = "55ac046e56e3089fec1691c22544b605\
f94185216dde0465e68b9d57c20dacbc\
49ca9cccf179b645991664b39d77ef31\
7c71b845b1e30bd50911204 1d3a19783"
        .replace(' ', "");
    let src = format!(
        "import crypto
fn main() -> i32:
    dk: str = crypto.pbkdf2_sha256(\"passwd\", \"salt\", 1, 64)
    if dk == \"{exp}\":
        println(\"PASS\")
    else:
        println(\"FAIL\")
    return 0\n",
        exp = esc(&expected),
    );
    let (code, out) = run("pbkdf2", &src);
    assert_eq!(code, 0, "out: {out:?}");
    assert!(out.contains("PASS"), "got: {out:?}");
    assert!(!out.contains("FAIL"), "got: {out:?}");
}

#[test]
fn pbkdf2_sha256_out_of_range_raises() {
    let src = "import crypto
fn main() -> i32:
    try:
        dk: str = crypto.pbkdf2_sha256(\"p\", \"s\", 0, 32)
        println(\"NOCATCH\")
    except ValueError as e:
        println(\"caught\")
    return 0\n";
    let (code, out) = run("pbkdf2_range", src);
    assert_eq!(code, 0, "out: {out:?}");
    assert!(out.contains("caught"), "got: {out:?}");
}

// ── HKDF-SHA256 — RFC 5869 Test Case 1 ──────────────────────────────────

#[test]
fn hkdf_sha256_rfc5869_tc1() {
    let ikm = "0b".repeat(22);
    let salt = "000102030405060708090a0b0c";
    let info = "f0f1f2f3f4f5f6f7f8f9";
    let okm = "3cb25f25faacd57a90434f64d0362f2a\
2d2d0a90cf1a5a4c5db02d56ecc4c5bf\
34007208d5b887185865";
    let src = format!(
        "import crypto
fn main() -> i32:
    ikm: str = \"{ikm}\"
    salt: str = \"{salt}\"
    info: str = \"{info}\"
    okm: str = crypto.hkdf_sha256(ikm, salt, info, 42)
    if okm == \"{okm}\":
        println(\"PASS\")
    else:
        println(\"FAIL\")
    return 0\n",
        ikm = esc(&ikm),
        salt = esc(salt),
        info = esc(info),
        okm = esc(okm),
    );
    let (code, out) = run("hkdf", &src);
    assert_eq!(code, 0, "out: {out:?}");
    assert!(out.contains("PASS"), "got: {out:?}");
    assert!(!out.contains("FAIL"), "got: {out:?}");
}

// ── Ed25519 — RFC 8032 §7.1 Test 1 + round-trip ─────────────────────────

// RFC 8032 Test 1: empty message.
const ED_PK: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
const ED_SIG: &str = "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155\
5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b";

#[test]
fn ed25519_rfc8032_test1_known_answer_verify() {
    let src = format!(
        "import crypto
fn main() -> i32:
    pk: str = \"{pk}\"
    sig: str = \"{sig}\"
    ok: bool = crypto.ed25519_verify(pk, \"\", sig)
    if ok:
        println(\"VERIFY-OK\")
    else:
        println(\"VERIFY-FAIL\")
    tampered: bool = crypto.ed25519_verify(pk, \"\\x00\", sig)
    if tampered:
        println(\"TAMPER-BUG\")
    else:
        println(\"TAMPER-REJECTED\")
    return 0\n",
        pk = esc(ED_PK),
        sig = esc(ED_SIG),
    );
    let (code, out) = run("ed25519_ka", &src);
    assert_eq!(code, 0, "out: {out:?}");
    assert!(out.contains("VERIFY-OK"), "got: {out:?}");
    assert!(out.contains("TAMPER-REJECTED"), "got: {out:?}");
    assert!(!out.contains("VERIFY-FAIL"), "got: {out:?}");
    assert!(!out.contains("TAMPER-BUG"), "got: {out:?}");
}

#[test]
fn ed25519_wrong_key_returns_false_and_roundtrip() {
    // Generate an independent keypair; use its pk (a valid curve point) to
    // reject the RFC-8032 signature, and its own keypair to sign/verify.
    let src = format!(
        "import crypto
fn main() -> i32:
    kp: Tuple[str, str] = crypto.ed25519_keygen()
    sk: str = kp.0
    pk: str = kp.1
    pk2: str = crypto.ed25519_public_key(sk)
    if pk == pk2:
        println(\"PUBKEY-OK\")
    else:
        println(\"PUBKEY-FAIL\")
    wrong: bool = crypto.ed25519_verify(pk, \"\", \"{sig}\")
    if wrong:
        println(\"WRONGKEY-BUG\")
    else:
        println(\"WRONGKEY-FALSE\")
    sig: str = crypto.ed25519_sign(sk, \"hello world\")
    good: bool = crypto.ed25519_verify(pk, \"hello world\", sig)
    bad: bool = crypto.ed25519_verify(pk, \"hello worlx\", sig)
    if good:
        println(\"SIGN-OK\")
    if bad:
        println(\"SIGN-BUG\")
    else:
        println(\"SIGN-REJECTED\")
    return 0\n",
        sig = esc(ED_SIG),
    );
    let (code, out) = run("ed25519_rt", &src);
    assert_eq!(code, 0, "out: {out:?}");
    assert!(out.contains("PUBKEY-OK"), "got: {out:?}");
    assert!(out.contains("WRONGKEY-FALSE"), "got: {out:?}");
    assert!(out.contains("SIGN-OK"), "got: {out:?}");
    assert!(out.contains("SIGN-REJECTED"), "got: {out:?}");
    assert!(!out.contains("BUG"), "got: {out:?}");
    assert!(!out.contains("FAIL"), "got: {out:?}");
}

// ── constant_time_eq truth table ────────────────────────────────────────

#[test]
fn constant_time_eq_truth_table() {
    let src = "import crypto
fn main() -> i32:
    if crypto.constant_time_eq(\"abc\", \"abc\"):
        println(\"eq-true\")
    if crypto.constant_time_eq(\"abc\", \"abd\"):
        println(\"diff-BUG\")
    else:
        println(\"diff-false\")
    if crypto.constant_time_eq(\"abc\", \"ab\"):
        println(\"len-BUG\")
    else:
        println(\"len-false\")
    if crypto.constant_time_eq(\"\", \"\"):
        println(\"empty-true\")
    return 0\n";
    let (code, out) = run("ct_eq", src);
    assert_eq!(code, 0, "out: {out:?}");
    assert!(out.contains("eq-true"), "got: {out:?}");
    assert!(out.contains("diff-false"), "got: {out:?}");
    assert!(out.contains("len-false"), "got: {out:?}");
    assert!(out.contains("empty-true"), "got: {out:?}");
    assert!(!out.contains("BUG"), "got: {out:?}");
}

// ── random_bytes ────────────────────────────────────────────────────────

#[test]
fn random_bytes_length_and_bounds() {
    let src = "import crypto
fn main() -> i32:
    r: str = crypto.random_bytes(16)
    println(\"len=\" + str(len(r)))
    r2: str = crypto.random_bytes(16)
    if r == r2:
        println(\"NOT-RANDOM\")
    else:
        println(\"distinct\")
    try:
        z: str = crypto.random_bytes(0)
        println(\"NOCATCH0\")
    except ValueError as e:
        println(\"caught0\")
    try:
        big: str = crypto.random_bytes(2000000)
        println(\"NOCATCHBIG\")
    except ValueError as e:
        println(\"caughtbig\")
    return 0\n";
    let (code, out) = run("random_bytes", src);
    assert_eq!(code, 0, "out: {out:?}");
    assert!(out.contains("len=16"), "got: {out:?}");
    assert!(out.contains("distinct"), "got: {out:?}");
    assert!(out.contains("caught0"), "got: {out:?}");
    assert!(out.contains("caughtbig"), "got: {out:?}");
    assert!(!out.contains("NOT-RANDOM"), "got: {out:?}");
}

// ── JWT HS256 ───────────────────────────────────────────────────────────

#[test]
fn jwt_hs256_round_trip() {
    let src = "import crypto
import json
fn main() -> i32:
    claims: JsonValue = json.parse(\"{\\\"admin\\\": true, \\\"name\\\": \\\"John Doe\\\", \\\"sub\\\": \\\"1234567890\\\"}\")
    tok: str = crypto.jwt_encode(claims, \"my-secret-key\", \"HS256\")
    dec: JsonValue = crypto.jwt_decode(tok, \"my-secret-key\", \"HS256\")
    expect: JsonValue = json.parse(\"{\\\"admin\\\": true, \\\"name\\\": \\\"John Doe\\\", \\\"sub\\\": \\\"1234567890\\\"}\")
    if json.stringify(dec) == json.stringify(expect):
        println(\"ROUNDTRIP-OK\")
    else:
        println(\"ROUNDTRIP-FAIL: \" + json.stringify(dec))
    return 0\n";
    let (code, out) = run("jwt_hs256", src);
    assert_eq!(code, 0, "out: {out:?}");
    assert!(out.contains("ROUNDTRIP-OK"), "got: {out:?}");
}

#[test]
fn jwt_hs256_wrong_key_raises() {
    let src = "import crypto
import json
fn main() -> i32:
    claims: JsonValue = json.parse(\"{\\\"sub\\\": \\\"x\\\"}\")
    tok: str = crypto.jwt_encode(claims, \"my-secret-key\", \"HS256\")
    try:
        dec: JsonValue = crypto.jwt_decode(tok, \"wrong-key\", \"HS256\")
        println(\"NOCATCH\")
    except ValueError as e:
        println(\"caught\")
    return 0\n";
    let (code, out) = run("jwt_wrongkey", src);
    assert_eq!(code, 0, "out: {out:?}");
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("NOCATCH"), "got: {out:?}");
}

#[test]
fn jwt_alg_mismatch_raises() {
    let src = "import crypto
import json
fn main() -> i32:
    claims: JsonValue = json.parse(\"{\\\"sub\\\": \\\"x\\\"}\")
    tok: str = crypto.jwt_encode(claims, \"my-secret-key\", \"HS256\")
    try:
        dec: JsonValue = crypto.jwt_decode(tok, \"my-secret-key\", \"EdDSA\")
        println(\"NOCATCH\")
    except ValueError as e:
        println(\"caught\")
    return 0\n";
    let (code, out) = run("jwt_algmismatch", src);
    assert_eq!(code, 0, "out: {out:?}");
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("NOCATCH"), "got: {out:?}");
}

#[test]
fn jwt_expired_token_raises() {
    // exp = 100 (1970) is far in the past; even with +60s leeway, expired.
    let src = "import crypto
import json
fn main() -> i32:
    claims: JsonValue = json.parse(\"{\\\"exp\\\": 100, \\\"sub\\\": \\\"x\\\"}\")
    tok: str = crypto.jwt_encode(claims, \"my-secret-key\", \"HS256\")
    try:
        dec: JsonValue = crypto.jwt_decode(tok, \"my-secret-key\", \"HS256\")
        println(\"NOCATCH\")
    except ValueError as e:
        println(\"caught\")
    return 0\n";
    let (code, out) = run("jwt_expired", src);
    assert_eq!(code, 0, "out: {out:?}");
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("NOCATCH"), "got: {out:?}");
}

#[test]
fn jwt_eddsa_round_trip() {
    let src = "import crypto
import json
fn main() -> i32:
    kp: Tuple[str, str] = crypto.ed25519_keygen()
    sk: str = kp.0
    pk: str = kp.1
    claims: JsonValue = json.parse(\"{\\\"role\\\": \\\"admin\\\", \\\"sub\\\": \\\"abc\\\"}\")
    tok: str = crypto.jwt_encode(claims, sk, \"EdDSA\")
    dec: JsonValue = crypto.jwt_decode(tok, pk, \"EdDSA\")
    expect: JsonValue = json.parse(\"{\\\"role\\\": \\\"admin\\\", \\\"sub\\\": \\\"abc\\\"}\")
    if json.stringify(dec) == json.stringify(expect):
        println(\"EDDSA-OK\")
    else:
        println(\"EDDSA-FAIL: \" + json.stringify(dec))
    return 0\n";
    let (code, out) = run("jwt_eddsa", src);
    assert_eq!(code, 0, "out: {out:?}");
    assert!(out.contains("EDDSA-OK"), "got: {out:?}");
}
