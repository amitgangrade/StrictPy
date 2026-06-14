//! In-process regression tests for the `struct` module's batch
//! writer/reader surface (handle-based binary serialization — the fast
//! path over per-field `pack_*` / `unpack_*`).  Pattern matches
//! m22_p2d_struct_url.rs: compile a small `.spy` snippet, write the
//! bytecode to `CARGO_TARGET_TMPDIR`, run through `run_file_capture`,
//! assert on stdout.
//!
//! Covered:
//!   1. writer output is byte-identical to the equivalent old-API
//!      concatenation `pack_u32_be(a) + pack_f64_be(b) + pack_u64_be(c)`,
//!   2. full write→finish→reader→sequential-read roundtrip including
//!      edge values (0, max u32, -1-as-u64 bit pattern, f64 specials
//!      via bit-pattern crosschecks),
//!   3. reading past the end raises ValueError ("struct reader overrun"),
//!   4. several interleaved handles stay independent,
//!   5. using a finished/freed handle raises a clear ValueError.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_struct_builder_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── 1. byte-identical to the old API ───────────────────────────────────

#[test]
fn writer_matches_old_api_concatenation() {
    let src = "\
import struct
fn main() -> i32:
    a: i64 = 0x01020304
    b: f64 = 3.141592653589793
    c: i64 = 9223372036854775807
    old: str = struct.pack_u32_be(a) + struct.pack_f64_be(b) + struct.pack_u64_be(c)
    w: i64 = struct.writer()
    struct.w_u32_be(w, a)
    struct.w_f64_be(w, b)
    struct.w_u64_be(w, c)
    new: str = struct.finish(w)
    println(\"len old=\" + str(len(old)) + \" new=\" + str(len(new)))
    if old == new:
        println(\"identical\")
    else:
        println(\"MISMATCH\")
    return 0
";
    let p = compile_snippet("matches_old_api", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("len old=20 new=20"), "lengths; got: {out:?}");
    assert!(out.contains("identical"), "byte-identity; got: {out:?}");
    assert!(!out.contains("MISMATCH"), "got: {out:?}");
}

#[test]
fn writer_matches_old_api_little_endian() {
    let src = "\
import struct
fn main() -> i32:
    a: i64 = 0xAABBCCDD
    b: i64 = 0x0011223344556677
    old: str = struct.pack_u32_le(a) + struct.pack_u64_le(b) + struct.pack_f64_le(2.5)
    w: i64 = struct.writer()
    struct.w_u32_le(w, a)
    struct.w_u64_le(w, b)
    struct.w_f64_le(w, 2.5)
    new: str = struct.finish(w)
    if old == new:
        println(\"identical le\")
    else:
        println(\"MISMATCH\")
    return 0
";
    let p = compile_snippet("matches_old_api_le", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("identical le"), "got: {out:?}");
    assert!(!out.contains("MISMATCH"), "got: {out:?}");
}

// ── 2. full roundtrip incl. edge values ────────────────────────────────

#[test]
fn full_roundtrip_recovers_all_values() {
    // Edge values: 0, max u32, -1 (all-ones u64 bit pattern, comes back
    // as i64 -1 from r_u64), i64::MAX, pi and -0.0 for f64.
    let src = "\
import struct
fn main() -> i32:
    zero: i64 = 0
    umax: i64 = 4294967295
    neg1: i64 = -1
    imax: i64 = 9223372036854775807
    w: i64 = struct.writer()
    struct.w_u32_be(w, zero)
    struct.w_u32_le(w, umax)
    struct.w_u64_be(w, neg1)
    struct.w_u64_le(w, imax)
    struct.w_f64_be(w, 3.141592653589793)
    struct.w_f64_le(w, -0.0)
    buf: str = struct.finish(w)
    println(\"buf len: \" + str(len(buf)))
    r: i64 = struct.reader(buf)
    println(\"v1: \" + str(struct.r_u32_be(r)))
    println(\"v2: \" + str(struct.r_u32_le(r)))
    println(\"v3: \" + str(struct.r_u64_be(r)))
    println(\"v4: \" + str(struct.r_u64_le(r)))
    pi: f64 = struct.r_f64_be(r)
    if pi == 3.141592653589793:
        println(\"pi ok\")
    nz: f64 = struct.r_f64_le(r)
    if nz == 0.0:
        println(\"negzero compares equal to zero\")
    struct.reader_done(r)
    println(\"done\")
    return 0
";
    let p = compile_snippet("full_roundtrip", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // 4 + 4 + 8 + 8 + 8 + 8 = 40 bytes.
    assert!(out.contains("buf len: 40"), "len; got: {out:?}");
    assert!(out.contains("v1: 0"), "zero u32; got: {out:?}");
    assert!(out.contains("v2: 4294967295"), "max u32; got: {out:?}");
    assert!(out.contains("v3: -1"), "all-ones u64; got: {out:?}");
    assert!(out.contains("v4: 9223372036854775807"), "i64::MAX; got: {out:?}");
    assert!(out.contains("pi ok"), "f64 be; got: {out:?}");
    assert!(out.contains("negzero compares equal to zero"), "-0.0; got: {out:?}");
    assert!(out.contains("done"), "reader_done; got: {out:?}");
}

#[test]
fn f64_specials_roundtrip_by_bit_pattern() {
    // Infinity / NaN can't be written as literals, so cross-check by bit
    // pattern: write the IEEE 754 bits with w_u64_be, read them back as
    // f64 with r_f64_be (same 8 bytes), then re-serialise that f64 and
    // recover the exact bit pattern with r_u64_be.  This proves the f64
    // arms preserve arbitrary bit patterns (incl. NaN payloads) without
    // relying on NaN == NaN.
    let src = "\
import struct
fn check(bits: i64, label: str) -> None:
    w1: i64 = struct.writer()
    struct.w_u64_be(w1, bits)
    r1: i64 = struct.reader(struct.finish(w1))
    x: f64 = struct.r_f64_be(r1)
    struct.reader_done(r1)
    w2: i64 = struct.writer()
    struct.w_f64_be(w2, x)
    r2: i64 = struct.reader(struct.finish(w2))
    back: i64 = struct.r_u64_be(r2)
    struct.reader_done(r2)
    if back == bits:
        println(label + \": ok\")
    else:
        println(label + \": FAIL\")

fn main() -> i32:
    pos_inf: i64 = 0x7FF0000000000000
    # 0xFFF0000000000000 reinterpreted as i64 (sign bit set).
    neg_inf: i64 = -4503599627370496
    quiet_nan: i64 = 0x7FF8000000000000
    min_subnormal: i64 = 1
    check(pos_inf, \"+inf\")
    check(neg_inf, \"-inf\")
    check(quiet_nan, \"nan\")
    check(min_subnormal, \"min subnormal\")
    return 0
";
    let p = compile_snippet("f64_specials", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    for label in ["+inf", "-inf", "nan", "min subnormal"] {
        assert!(out.contains(&format!("{label}: ok")), "{label}; got: {out:?}");
    }
    assert!(!out.contains("FAIL"), "got: {out:?}");
}

#[test]
fn reader_interops_with_old_pack_api() {
    // A buffer produced by the *old* pack_* concatenation must be
    // readable by the new sequential reader (same byte<->str convention).
    let src = "\
import struct
fn main() -> i32:
    cafe: i64 = 0xDEADBEEF
    buf: str = struct.pack_u32_be(cafe) + struct.pack_u64_le(424242)
    r: i64 = struct.reader(buf)
    println(str(struct.r_u32_be(r)))
    println(str(struct.r_u64_le(r)))
    struct.reader_done(r)
    return 0
";
    let p = compile_snippet("interop_old_pack", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // 0xDEADBEEF == 3735928559.
    assert!(out.contains("3735928559"), "got: {out:?}");
    assert!(out.contains("424242"), "got: {out:?}");
}

// ── 3. overrun raises ValueError ───────────────────────────────────────

#[test]
fn reader_overrun_raises_value_error() {
    let src = "\
import struct
fn main() -> i32:
    w: i64 = struct.writer()
    struct.w_u32_be(w, 7)
    r: i64 = struct.reader(struct.finish(w))
    first: i64 = struct.r_u32_be(r)
    println(\"first: \" + str(first))
    try:
        second: i64 = struct.r_u32_be(r)
        println(\"UNREACHED \" + str(second))
    except ValueError as e:
        println(\"caught overrun\")
    # A 4-byte buffer can never satisfy an 8-byte read either.
    r2: i64 = struct.reader(struct.pack_u32_be(1))
    try:
        big: i64 = struct.r_u64_be(r2)
        println(\"UNREACHED \" + str(big))
    except ValueError as e:
        println(\"caught wide overrun\")
    struct.reader_done(r)
    struct.reader_done(r2)
    return 0
";
    let p = compile_snippet("overrun", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("first: 7"), "got: {out:?}");
    assert!(out.contains("caught overrun"), "got: {out:?}");
    assert!(out.contains("caught wide overrun"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

// ── 4. interleaved handles stay independent ────────────────────────────

#[test]
fn interleaved_handles_are_independent() {
    let src = "\
import struct
fn main() -> i32:
    w1: i64 = struct.writer()
    w2: i64 = struct.writer()
    w3: i64 = struct.writer()
    struct.w_u32_be(w1, 1)
    struct.w_u32_be(w2, 2)
    struct.w_u32_be(w3, 3)
    struct.w_u32_be(w1, 10)
    struct.w_u32_be(w3, 30)
    struct.w_u32_be(w2, 20)
    b2: str = struct.finish(w2)
    b1: str = struct.finish(w1)
    b3: str = struct.finish(w3)
    # Interleave the readers too.
    r1: i64 = struct.reader(b1)
    r2: i64 = struct.reader(b2)
    r3: i64 = struct.reader(b3)
    println(\"a: \" + str(struct.r_u32_be(r2)))
    println(\"b: \" + str(struct.r_u32_be(r1)))
    println(\"c: \" + str(struct.r_u32_be(r3)))
    println(\"d: \" + str(struct.r_u32_be(r1)))
    println(\"e: \" + str(struct.r_u32_be(r3)))
    println(\"f: \" + str(struct.r_u32_be(r2)))
    struct.reader_done(r2)
    struct.reader_done(r1)
    struct.reader_done(r3)
    return 0
";
    let p = compile_snippet("interleaved", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("a: 2"), "got: {out:?}");
    assert!(out.contains("b: 1"), "got: {out:?}");
    assert!(out.contains("c: 3"), "got: {out:?}");
    assert!(out.contains("d: 10"), "got: {out:?}");
    assert!(out.contains("e: 30"), "got: {out:?}");
    assert!(out.contains("f: 20"), "got: {out:?}");
}

// ── 5. finished / freed handle raises a clear error ────────────────────

#[test]
fn finished_writer_handle_raises() {
    let src = "\
import struct
fn main() -> i32:
    w: i64 = struct.writer()
    struct.w_u32_be(w, 1)
    buf: str = struct.finish(w)
    println(\"len: \" + str(len(buf)))
    try:
        struct.w_u32_be(w, 2)
        println(\"UNREACHED write\")
    except ValueError as e:
        println(\"caught stale write\")
    try:
        again: str = struct.finish(w)
        println(\"UNREACHED finish \" + again)
    except ValueError as e:
        println(\"caught double finish\")
    return 0
";
    let p = compile_snippet("stale_writer", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("len: 4"), "got: {out:?}");
    assert!(out.contains("caught stale write"), "got: {out:?}");
    assert!(out.contains("caught double finish"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

#[test]
fn freed_reader_handle_raises() {
    let src = "\
import struct
fn main() -> i32:
    r: i64 = struct.reader(struct.pack_u32_be(5))
    println(\"v: \" + str(struct.r_u32_be(r)))
    struct.reader_done(r)
    try:
        late: i64 = struct.r_u32_be(r)
        println(\"UNREACHED read \" + str(late))
    except ValueError as e:
        println(\"caught stale read\")
    try:
        struct.reader_done(r)
        println(\"UNREACHED done\")
    except ValueError as e:
        println(\"caught double done\")
    return 0
";
    let p = compile_snippet("stale_reader", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("v: 5"), "got: {out:?}");
    assert!(out.contains("caught stale read"), "got: {out:?}");
    assert!(out.contains("caught double done"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

#[test]
fn mixed_kind_handle_use_raises() {
    // Writer fns on a reader handle (and vice versa) are ValueError, and
    // the wrongly-targeted slot survives the failed call.
    let src = "\
import struct
fn main() -> i32:
    w: i64 = struct.writer()
    struct.w_u32_be(w, 9)
    r: i64 = struct.reader(struct.pack_u32_be(8))
    try:
        struct.w_u32_be(r, 1)
        println(\"UNREACHED w-on-r\")
    except ValueError as e:
        println(\"caught w-on-r\")
    try:
        x: i64 = struct.r_u32_be(w)
        println(\"UNREACHED r-on-w \" + str(x))
    except ValueError as e:
        println(\"caught r-on-w\")
    try:
        bad: str = struct.finish(r)
        println(\"UNREACHED finish-on-r \" + bad)
    except ValueError as e:
        println(\"caught finish-on-r\")
    try:
        struct.reader_done(w)
        println(\"UNREACHED done-on-w\")
    except ValueError as e:
        println(\"caught done-on-w\")
    # Both slots must still be intact after the failed calls.
    println(\"r still reads: \" + str(struct.r_u32_be(r)))
    println(\"w still finishes: \" + str(len(struct.finish(w))))
    struct.reader_done(r)
    return 0
";
    let p = compile_snippet("mixed_kind", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught w-on-r"), "got: {out:?}");
    assert!(out.contains("caught r-on-w"), "got: {out:?}");
    assert!(out.contains("caught finish-on-r"), "got: {out:?}");
    assert!(out.contains("caught done-on-w"), "got: {out:?}");
    assert!(out.contains("r still reads: 8"), "got: {out:?}");
    assert!(out.contains("w still finishes: 4"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}

#[test]
fn empty_writer_finishes_to_empty_str() {
    let src = "\
import struct
fn main() -> i32:
    w: i64 = struct.writer()
    buf: str = struct.finish(w)
    println(\"empty len: \" + str(len(buf)))
    r: i64 = struct.reader(buf)
    try:
        v: i64 = struct.r_u32_be(r)
        println(\"UNREACHED \" + str(v))
    except ValueError as e:
        println(\"empty reader overruns immediately\")
    struct.reader_done(r)
    return 0
";
    let p = compile_snippet("empty_writer", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("empty len: 0"), "got: {out:?}");
    assert!(out.contains("empty reader overruns immediately"), "got: {out:?}");
    assert!(!out.contains("UNREACHED"), "got: {out:?}");
}
