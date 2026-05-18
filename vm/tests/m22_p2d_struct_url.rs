//! M22 P2D in-process regression tests for the `struct` and
//! `urllib_parse` stdlib modules.  Pattern matches m20c_json_re.rs:
//! compile a small `.spy` snippet, write the bytecode to
//! `CARGO_TARGET_TMPDIR`, run through `run_file_capture`, assert on
//! stdout.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m22_p2d_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── struct module ──────────────────────────────────────────────────────

#[test]
fn struct_pack_u32_be_round_trips() {
    let src = "\
import struct
fn main() -> i32:
    n: i64 = 0x01020304
    buf: str = struct.pack_u32_be(n)
    println(str(len(buf)))
    got: i64 = struct.unpack_u32_be(buf, 0)
    println(str(got))
    return 0
";
    let p = compile_snippet("struct_u32_be", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // u32_be of 0x01020304 is exactly 4 chars; unpack returns the
    // original 16909060 (= 0x01020304).
    assert!(out.contains("4"), "len; got: {out:?}");
    assert!(out.contains("16909060"), "round-trip; got: {out:?}");
}

#[test]
fn struct_pack_u32_le_differs_from_be() {
    let src = "\
import struct
fn main() -> i32:
    be: str = struct.pack_u32_be(0x12345678)
    le: str = struct.pack_u32_le(0x12345678)
    if be == le:
        println(\"same\")
    else:
        println(\"differ\")
    # Decode each in its own endianness to confirm round-trip.
    println(str(struct.unpack_u32_be(be, 0)))
    println(str(struct.unpack_u32_le(le, 0)))
    return 0
";
    let p = compile_snippet("struct_u32_be_vs_le", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("differ"), "endianness; got: {out:?}");
    // 0x12345678 == 305419896.
    assert!(out.contains("305419896"), "round-trip; got: {out:?}");
}

#[test]
fn struct_u32_max_round_trips() {
    let src = "\
import struct
fn main() -> i32:
    nmax: i64 = 4294967295
    buf: str = struct.pack_u32_be(nmax)
    println(str(struct.unpack_u32_be(buf, 0)))
    return 0
";
    let p = compile_snippet("struct_u32_max", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("4294967295"), "got: {out:?}");
}

#[test]
fn struct_pack_u64_be_round_trips() {
    let src = "\
import struct
fn main() -> i32:
    n: i64 = 9223372036854775807
    buf: str = struct.pack_u64_be(n)
    println(str(len(buf)))
    println(str(struct.unpack_u64_be(buf, 0)))
    return 0
";
    let p = compile_snippet("struct_u64_be", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("8"), "len; got: {out:?}");
    assert!(out.contains("9223372036854775807"), "got: {out:?}");
}

#[test]
fn struct_pack_u64_le_round_trips() {
    let src = "\
import struct
fn main() -> i32:
    n: i64 = 0x0011223344556677
    buf: str = struct.pack_u64_le(n)
    println(str(struct.unpack_u64_le(buf, 0)))
    return 0
";
    let p = compile_snippet("struct_u64_le", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // 0x0011223344556677 == 4822678189205111.
    assert!(out.contains("4822678189205111"), "got: {out:?}");
}

#[test]
fn struct_pack_f64_round_trip_be_and_le() {
    let src = "\
import struct
fn main() -> i32:
    pi: f64 = 3.141592653589793
    bbe: str = struct.pack_f64_be(pi)
    ble: str = struct.pack_f64_le(pi)
    println(str(len(bbe)))
    println(str(len(ble)))
    rbe: f64 = struct.unpack_f64_be(bbe, 0)
    rle: f64 = struct.unpack_f64_le(ble, 0)
    if rbe == pi:
        println(\"be ok\")
    if rle == pi:
        println(\"le ok\")
    return 0
";
    let p = compile_snippet("struct_f64", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("be ok"), "be; got: {out:?}");
    assert!(out.contains("le ok"), "le; got: {out:?}");
}

#[test]
fn struct_unpack_at_offset() {
    // Concatenate two u32_be buffers and unpack from offset 4 to pull
    // the second value back out.
    let src = "\
import struct
fn main() -> i32:
    a: i64 = 0xAABBCCDD
    b: i64 = 0x11223344
    cat: str = struct.pack_u32_be(a) + struct.pack_u32_be(b)
    println(str(len(cat)))
    println(str(struct.unpack_u32_be(cat, 4)))
    return 0
";
    let p = compile_snippet("struct_offset", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("8"), "len; got: {out:?}");
    // 0x11223344 == 287454020.
    assert!(out.contains("287454020"), "got: {out:?}");
}

#[test]
fn struct_unpack_short_buffer_raises_value_error() {
    let src = "\
import struct
fn main() -> i32:
    buf: str = struct.pack_u32_be(42)
    try:
        # 4-byte buffer, offset 100 — guaranteed out-of-bounds.
        oob: i64 = struct.unpack_u32_be(buf, 100)
        println(\"UNREACHED \" + str(oob))
    except ValueError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("struct_oob", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

// ── urllib_parse module ────────────────────────────────────────────────

#[test]
fn url_quote_basic_ascii() {
    let src = "\
import urllib_parse
fn main() -> i32:
    println(urllib_parse.quote(\"a b/c=d\"))
    return 0
";
    let p = compile_snippet("url_quote_ascii", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("a%20b%2Fc%3Dd"), "got: {out:?}");
}

#[test]
fn url_quote_plus_space_becomes_plus() {
    let src = "\
import urllib_parse
fn main() -> i32:
    println(urllib_parse.quote_plus(\"hello world\"))
    return 0
";
    let p = compile_snippet("url_quote_plus", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("hello+world"), "got: {out:?}");
}

#[test]
fn url_unreserved_characters_pass_through() {
    let src = "\
import urllib_parse
fn main() -> i32:
    println(urllib_parse.quote(\"foo.bar_baz-qux~zot\"))
    return 0
";
    let p = compile_snippet("url_unreserved", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("foo.bar_baz-qux~zot"), "got: {out:?}");
}

#[test]
fn url_unicode_round_trip() {
    let src = "\
import urllib_parse
fn main() -> i32:
    orig: str = \"snow=☃\"
    enc: str = urllib_parse.quote(orig)
    println(enc)
    back: str = urllib_parse.unquote(enc)
    if back == orig:
        println(\"ok\")
    return 0
";
    let p = compile_snippet("url_unicode", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // U+2603 (☃) is 3 UTF-8 bytes: 0xE2 0x98 0x83 → %E2%98%83.
    assert!(out.contains("snow%3D%E2%98%83"), "encoded; got: {out:?}");
    assert!(out.contains("ok"), "round-trip; got: {out:?}");
}

#[test]
fn url_unquote_plus_decodes_plus_as_space() {
    let src = "\
import urllib_parse
fn main() -> i32:
    println(urllib_parse.unquote_plus(\"hello+world+foo\"))
    return 0
";
    let p = compile_snippet("url_unquote_plus", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("hello world foo"), "got: {out:?}");
}

#[test]
fn url_unquote_bad_escape_raises_value_error() {
    let src = "\
import urllib_parse
fn main() -> i32:
    try:
        bad: str = urllib_parse.unquote(\"a%ZZb\")
        println(\"UNREACHED: \" + bad)
    except ValueError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("url_unquote_bad", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

#[test]
fn urlencode_round_trip_with_parse_query() {
    let src = "\
import urllib_parse
fn main() -> i32:
    pairs: List[Tuple[str, str]] = []
    pairs.append((\"k\", \"v\"))
    pairs.append((\"a\", \"b c\"))
    pairs.append((\"hi\", \"there!\"))
    qs: str = urllib_parse.urlencode(pairs)
    println(qs)
    parsed: List[Tuple[str, str]] = urllib_parse.parse_query(qs)
    println(str(len(parsed)))
    i: i64 = 0
    while i < i64(len(parsed)):
        p: Tuple[str, str] = parsed[i]
        println(p.0 + \"=\" + p.1)
        i = i + 1
    return 0
";
    let p = compile_snippet("url_round_trip", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("k=v&a=b+c&hi=there%21"), "encode; got: {out:?}");
    assert!(out.contains("3"), "count; got: {out:?}");
    assert!(out.contains("k=v"), "first pair; got: {out:?}");
    assert!(out.contains("a=b c"), "second pair; got: {out:?}");
    assert!(out.contains("hi=there!"), "third pair; got: {out:?}");
}

#[test]
fn parse_query_handles_missing_equals() {
    let src = "\
import urllib_parse
fn main() -> i32:
    parsed: List[Tuple[str, str]] = urllib_parse.parse_query(\"flag&k=v\")
    println(str(len(parsed)))
    p0: Tuple[str, str] = parsed[0]
    println(p0.0 + \"=[\" + p0.1 + \"]\")
    p1: Tuple[str, str] = parsed[1]
    println(p1.0 + \"=[\" + p1.1 + \"]\")
    return 0
";
    let p = compile_snippet("url_parse_no_eq", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("2"), "count; got: {out:?}");
    assert!(out.contains("flag=[]"), "first; got: {out:?}");
    assert!(out.contains("k=[v]"), "second; got: {out:?}");
}

#[test]
fn parse_query_empty_string_returns_empty_list() {
    let src = "\
import urllib_parse
fn main() -> i32:
    parsed: List[Tuple[str, str]] = urllib_parse.parse_query(\"\")
    println(str(len(parsed)))
    return 0
";
    let p = compile_snippet("url_parse_empty", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("0"), "got: {out:?}");
}
