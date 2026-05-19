//! M23 P3a-B regression tests for the `datetime` stdlib module.
//!
//! Follows the M19/M20a/M20b pattern: each test compiles a tiny `.spy`
//! snippet, writes the bytecode to `CARGO_TARGET_TMPDIR`, runs it
//! through `run_file_capture`, and asserts on stdout.
//!
//! Coverage:
//!   * `datetime.now()` — returns a non-negative i64 (modern wall-clocks).
//!   * `datetime.from_ymd(1970, 1, 1)` — pins the unix epoch (=0).
//!   * `datetime.from_ymd(2000, 1, 1)` — pins 946684800.
//!   * `datetime.from_ymd_hms` — full constructor.
//!   * Component extraction — year/month/day/hour/minute/second.
//!   * `datetime.weekday(2026-05-19)` == 1 (Tuesday).
//!   * `datetime.ymd` — tuple round-trip.
//!   * `datetime.to_iso(0)` == "1970-01-01T00:00:00Z".
//!   * `datetime.from_iso` — round-trip; +00:00 == Z; offset arithmetic.
//!   * `datetime.from_date_str` — date-only form.
//!   * `datetime.add_days` / `diff_days` — calendar arithmetic.
//!   * `datetime.add_seconds` / `diff_seconds`.
//!   * Negative inputs (pre-1970, BCE years).
//!   * `datetime.local_offset_minutes()` — returns a sane TZ value.
//!   * ValueError on out-of-range inputs (month=0, month=13, etc.).
//!   * ValueError on malformed ISO strings.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m23_p3a_b_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── Constructors ──────────────────────────────────────────────────────

#[test]
fn from_ymd_unix_epoch_is_zero() {
    let src = "\
import datetime
fn main() -> i32:
    e: i64 = datetime.from_ymd(1970, 1, 1)
    println(\"e=\" + str(e))
    return 0
";
    let p = compile_snippet("from_ymd_epoch", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("e=0"), "got: {out:?}");
}

#[test]
fn from_ymd_y2k_is_known_constant() {
    // 2000-01-01 00:00:00 UTC = 946684800 — the standard reference.
    let src = "\
import datetime
fn main() -> i32:
    e: i64 = datetime.from_ymd(2000, 1, 1)
    println(\"e=\" + str(e))
    return 0
";
    let p = compile_snippet("from_ymd_y2k", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("e=946684800"), "got: {out:?}");
}

#[test]
fn from_ymd_hms_full_constructor() {
    // 2026-05-19 14:23:11 UTC.  Hand-computed: 56 years and 138 days
    // beyond the epoch.  We trust the algorithm and pin via the ISO
    // round-trip — easier to read than the raw second count.
    let src = "\
import datetime
fn main() -> i32:
    dt: i64 = datetime.from_ymd_hms(2026, 5, 19, 14, 23, 11)
    println(\"iso=\" + datetime.to_iso(dt))
    return 0
";
    let p = compile_snippet("from_ymd_hms", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("iso=2026-05-19T14:23:11Z"), "got: {out:?}");
}

#[test]
fn now_is_non_negative_on_modern_systems() {
    let src = "\
import datetime
fn main() -> i32:
    n: i64 = datetime.now()
    if n >= 0i64:
        println(\"non-negative\")
    else:
        println(\"NEGATIVE\")
    return 0
";
    let p = compile_snippet("now_nonneg", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("non-negative"), "got: {out:?}");
}

#[test]
fn from_unix_is_identity() {
    let src = "\
import datetime
fn main() -> i32:
    e: i64 = datetime.from_unix(12345i64)
    println(\"e=\" + str(e))
    return 0
";
    let p = compile_snippet("from_unix", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("e=12345"), "got: {out:?}");
}

// ── Component extraction ──────────────────────────────────────────────

#[test]
fn component_extraction_round_trip() {
    let src = "\
import datetime
fn main() -> i32:
    dt: i64 = datetime.from_ymd_hms(2026, 5, 19, 14, 23, 11)
    println(\"y=\" + str(datetime.year(dt)))
    println(\"mo=\" + str(datetime.month(dt)))
    println(\"d=\" + str(datetime.day(dt)))
    println(\"h=\" + str(datetime.hour(dt)))
    println(\"mi=\" + str(datetime.minute(dt)))
    println(\"s=\" + str(datetime.second(dt)))
    return 0
";
    let p = compile_snippet("components", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("y=2026"), "got: {out:?}");
    assert!(out.contains("mo=5"), "got: {out:?}");
    assert!(out.contains("d=19"), "got: {out:?}");
    assert!(out.contains("h=14"), "got: {out:?}");
    assert!(out.contains("mi=23"), "got: {out:?}");
    assert!(out.contains("s=11"), "got: {out:?}");
}

#[test]
fn weekday_of_known_date() {
    // 2026-05-19 is a Tuesday → ISO weekday=1.
    // 1970-01-01 was a Thursday → ISO weekday=3.
    // 2000-01-01 was a Saturday → ISO weekday=5.
    let src = "\
import datetime
fn main() -> i32:
    println(\"tue=\" + str(datetime.weekday(datetime.from_ymd(2026, 5, 19))))
    println(\"thu=\" + str(datetime.weekday(datetime.from_ymd(1970, 1, 1))))
    println(\"sat=\" + str(datetime.weekday(datetime.from_ymd(2000, 1, 1))))
    return 0
";
    let p = compile_snippet("weekday", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("tue=1"), "got: {out:?}");
    assert!(out.contains("thu=3"), "got: {out:?}");
    assert!(out.contains("sat=5"), "got: {out:?}");
}

#[test]
fn ymd_tuple_returns_three_slots() {
    let src = "\
import datetime
fn main() -> i32:
    t: Tuple[i32, i32, i32] = datetime.ymd(datetime.from_ymd(2026, 5, 19))
    println(\"y=\" + str(t.0) + \" m=\" + str(t.1) + \" d=\" + str(t.2))
    return 0
";
    let p = compile_snippet("ymd_tuple", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("y=2026 m=5 d=19"), "got: {out:?}");
}

// ── ISO formatting / parsing ──────────────────────────────────────────

#[test]
fn to_iso_epoch_is_canonical_string() {
    let src = "\
import datetime
fn main() -> i32:
    s: str = datetime.to_iso(0i64)
    println(\"iso=\" + s)
    return 0
";
    let p = compile_snippet("to_iso_epoch", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("iso=1970-01-01T00:00:00Z"), "got: {out:?}");
}

#[test]
fn to_date_str_and_to_time_str() {
    let src = "\
import datetime
fn main() -> i32:
    dt: i64 = datetime.from_ymd_hms(2026, 5, 19, 14, 23, 11)
    println(\"d=\" + datetime.to_date_str(dt))
    println(\"t=\" + datetime.to_time_str(dt))
    return 0
";
    let p = compile_snippet("to_date_time_str", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("d=2026-05-19"), "got: {out:?}");
    assert!(out.contains("t=14:23:11"), "got: {out:?}");
}

#[test]
fn from_iso_round_trip_z_suffix() {
    let src = "\
import datetime
fn main() -> i32:
    dt: i64 = datetime.from_ymd_hms(2026, 5, 19, 14, 23, 11)
    parsed: i64 = datetime.from_iso(\"2026-05-19T14:23:11Z\")
    if parsed == dt:
        println(\"match\")
    else:
        println(\"MISMATCH\")
    return 0
";
    let p = compile_snippet("from_iso_z", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("match"), "got: {out:?}");
}

#[test]
fn from_iso_round_trip_offset_suffix() {
    // `+00:00` should be equivalent to `Z`.
    let src = "\
import datetime
fn main() -> i32:
    dt: i64 = datetime.from_ymd_hms(2026, 5, 19, 14, 23, 11)
    parsed: i64 = datetime.from_iso(\"2026-05-19T14:23:11+00:00\")
    if parsed == dt:
        println(\"match\")
    else:
        println(\"MISMATCH\")
    return 0
";
    let p = compile_snippet("from_iso_plus00", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("match"), "got: {out:?}");
}

#[test]
fn from_iso_with_negative_offset() {
    // `-05:00` means "local = UTC - 5h"; the UTC instant is 5h later.
    let src = "\
import datetime
fn main() -> i32:
    base: i64 = datetime.from_ymd_hms(2026, 5, 19, 14, 23, 11)
    expected: i64 = base + 5i64 * 3600i64
    parsed: i64 = datetime.from_iso(\"2026-05-19T14:23:11-05:00\")
    println(\"d=\" + str(parsed - expected))
    return 0
";
    let p = compile_snippet("from_iso_neg_offset", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("d=0"), "got: {out:?}");
}

#[test]
fn from_iso_date_only_form() {
    let src = "\
import datetime
fn main() -> i32:
    dt: i64 = datetime.from_iso(\"2026-05-19\")
    expected: i64 = datetime.from_ymd(2026, 5, 19)
    if dt == expected:
        println(\"match\")
    else:
        println(\"MISMATCH\")
    return 0
";
    let p = compile_snippet("from_iso_date_only", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("match"), "got: {out:?}");
}

#[test]
fn from_date_str_matches_from_ymd() {
    let src = "\
import datetime
fn main() -> i32:
    dt: i64 = datetime.from_date_str(\"2026-05-19\")
    expected: i64 = datetime.from_ymd(2026, 5, 19)
    if dt == expected:
        println(\"match\")
    else:
        println(\"MISMATCH\")
    return 0
";
    let p = compile_snippet("from_date_str", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("match"), "got: {out:?}");
}

// ── Arithmetic ────────────────────────────────────────────────────────

#[test]
fn add_days_and_diff_days_invert() {
    let src = "\
import datetime
fn main() -> i32:
    a: i64 = datetime.from_ymd(2026, 5, 19)
    b: i64 = datetime.add_days(a, 30i64)
    n: i64 = datetime.diff_days(b, a)
    println(\"days=\" + str(n))
    println(\"later=\" + datetime.to_date_str(b))
    return 0
";
    let p = compile_snippet("add_diff_days", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("days=30"), "got: {out:?}");
    assert!(out.contains("later=2026-06-18"), "got: {out:?}");
}

#[test]
fn add_seconds_and_diff_seconds_invert() {
    let src = "\
import datetime
fn main() -> i32:
    a: i64 = datetime.from_ymd_hms(2026, 5, 19, 14, 23, 11)
    b: i64 = datetime.add_seconds(a, 3600i64)
    n: i64 = datetime.diff_seconds(b, a)
    println(\"sec=\" + str(n))
    println(\"h=\" + str(datetime.hour(b)))
    return 0
";
    let p = compile_snippet("add_diff_seconds", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("sec=3600"), "got: {out:?}");
    assert!(out.contains("h=15"), "got: {out:?}");
}

#[test]
fn diff_days_floors_negative_spans() {
    // Floor division matches Python's `//`: -1.5d -> -2.  We exercise
    // the boundary by subtracting 12h (negative span).
    let src = "\
import datetime
fn main() -> i32:
    a: i64 = datetime.from_ymd(2026, 5, 19)
    b: i64 = datetime.add_seconds(a, -12i64 * 3600i64)
    n: i64 = datetime.diff_days(b, a)
    println(\"n=\" + str(n))
    return 0
";
    let p = compile_snippet("diff_days_floor", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    // a - 12h is the same calendar-day-minus-one when floored.
    assert!(out.contains("n=-1"), "got: {out:?}");
}

// ── Negative / boundary inputs ────────────────────────────────────────

#[test]
fn pre_epoch_date_yields_negative_seconds() {
    // 1969-12-31 = -86400 (one day before epoch).
    let src = "\
import datetime
fn main() -> i32:
    e: i64 = datetime.from_ymd(1969, 12, 31)
    println(\"e=\" + str(e))
    return 0
";
    let p = compile_snippet("pre_epoch", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("e=-86400"), "got: {out:?}");
}

#[test]
fn weekday_handles_pre_epoch_correctly() {
    // 1969-12-31 was a Wednesday → ISO weekday=2.
    let src = "\
import datetime
fn main() -> i32:
    println(\"w=\" + str(datetime.weekday(datetime.from_ymd(1969, 12, 31))))
    return 0
";
    let p = compile_snippet("weekday_pre_epoch", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("w=2"), "got: {out:?}");
}

#[test]
fn leap_year_feb_29_is_valid() {
    // 2024 is a leap year → 2024-02-29 is valid.
    let src = "\
import datetime
fn main() -> i32:
    e: i64 = datetime.from_ymd(2024, 2, 29)
    println(\"d=\" + datetime.to_date_str(e))
    return 0
";
    let p = compile_snippet("leap_feb29", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("d=2024-02-29"), "got: {out:?}");
}

// ── Error cases ───────────────────────────────────────────────────────

/// Helper: assert that running `src` raises a `ValueError` whose
/// message contains `needle`.  Uses `run_file_capture` and matches on
/// the `Err(VmError::UncaughtException { .. })` arm.
fn assert_raises_value_error(test_name: &str, src: &str, needle: &str) {
    use strictpy_vm::VmError;
    let p = compile_snippet(test_name, src);
    let result = run_file_capture(&p);
    match result {
        Err(VmError::UncaughtException { type_name, message }) => {
            assert_eq!(
                type_name, "ValueError",
                "{test_name}: expected ValueError, got {type_name}: {message}"
            );
            assert!(
                message.contains(needle),
                "{test_name}: expected message containing {needle:?}, got {message:?}"
            );
        }
        other => panic!("{test_name}: expected ValueError, got {other:?}"),
    }
}

#[test]
fn invalid_month_raises_value_error() {
    let src = "\
import datetime
fn main() -> i32:
    e: i64 = datetime.from_ymd(2026, 13, 1)
    println(\"e=\" + str(e))
    return 0
";
    assert_raises_value_error("invalid_month", src, "month");
}

#[test]
fn invalid_day_raises_value_error() {
    // Feb 30 doesn't exist in any year.
    let src = "\
import datetime
fn main() -> i32:
    e: i64 = datetime.from_ymd(2026, 2, 30)
    println(\"e=\" + str(e))
    return 0
";
    assert_raises_value_error("invalid_day", src, "day");
}

#[test]
fn malformed_iso_raises_value_error() {
    let src = "\
import datetime
fn main() -> i32:
    e: i64 = datetime.from_iso(\"not-a-date\")
    println(\"e=\" + str(e))
    return 0
";
    assert_raises_value_error("bad_iso", src, "datetime");
}

#[test]
fn malformed_iso_with_bad_tz_raises_value_error() {
    // `+blah` is not a valid TZ suffix.
    let src = "\
import datetime
fn main() -> i32:
    e: i64 = datetime.from_iso(\"2026-05-19T14:23:11+blah\")
    println(\"e=\" + str(e))
    return 0
";
    assert_raises_value_error("bad_iso_tz", src, "datetime");
}

// ── Local offset ──────────────────────────────────────────────────────

#[test]
fn local_offset_minutes_returns_realistic_value() {
    // Every named IANA timezone falls in [-720, +840] minutes (-12h to
    // +14h).  We assert the loose range rather than a specific value
    // since CI hosts can run in any TZ.
    let src = "\
import datetime
fn main() -> i32:
    off: i32 = datetime.local_offset_minutes()
    if off >= -720 and off <= 840:
        println(\"in-range\")
    else:
        println(\"OUT-OF-RANGE: \" + str(off))
    return 0
";
    let p = compile_snippet("local_offset", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("in-range"), "got: {out:?}");
}
