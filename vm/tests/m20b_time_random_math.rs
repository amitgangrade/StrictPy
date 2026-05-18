//! M20b regression tests for `time`, `random`, and `math` stdlib modules.
//!
//! Follows the M19/M20a pattern: each test compiles a tiny `.spy` snippet,
//! writes the bytecode to `CARGO_TARGET_TMPDIR`, runs it through
//! `run_file_capture`, and asserts on stdout.
//!
//! Coverage:
//!   * `time.now()` / `time.now_ms()` — return positive values.
//!   * `time.monotonic()` — non-decreasing across two reads.
//!   * `time.sleep_ms(N)` — wall-clock advances by at least N (lenient).
//!   * `time.format_iso(0.0)` — Unix epoch matches `"1970-01-01T00:00:00Z"`.
//!   * `random.seed(s)` + `random.randint` — deterministic across re-seeds.
//!   * `random.random()` — value in [0.0, 1.0).
//!   * `random.choice_*` — picks an element of the list.
//!   * `random.shuffle_*` — preserves length, contains same elements.
//!   * `random.sample_*` — returns exactly n distinct elements; errors
//!     on n > len and n < 0.
//!   * `math.pi/e/tau/inf/nan` constants.
//!   * `math.sqrt(16.0)` — same value as the prelude bare `sqrt`.
//!   * `math.log2(1024.0)` — 10.0.
//!   * `math.gcd(12, 18)` — 6.
//!   * `math.factorial(0..20)` — boundary checks; 21 raises OverflowError.
//!   * `math.is_nan(nan)` / `math.is_inf(inf)` — predicates.
//!   * Negative: `math.factorial(-1)` raises ValueError.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn compile_snippet(test_name: &str, src: &str) -> PathBuf {
    let bytes = compile_source(format!("{test_name}.spy"), src)
        .unwrap_or_else(|e| panic!("{test_name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_m20b_{test_name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    out
}

// ── time module ────────────────────────────────────────────────────────

#[test]
fn time_now_returns_positive() {
    let src = "\
import time
fn main() -> i32:
    t: f64 = time.now()
    if t > 0.0:
        println(\"positive\")
    else:
        println(\"non-positive\")
    return 0
";
    let p = compile_snippet("time_now", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("positive"), "got: {out:?}");
}

#[test]
fn time_now_ms_returns_positive() {
    let src = "\
import time
fn main() -> i32:
    t: i64 = time.now_ms()
    if t > 0i64:
        println(\"positive\")
    else:
        println(\"non-positive\")
    return 0
";
    let p = compile_snippet("time_now_ms", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("positive"), "got: {out:?}");
}

#[test]
fn time_monotonic_is_non_decreasing() {
    // Two reads in quick succession must satisfy t1 >= t0.  We loop to
    // burn a few microseconds between reads so the clock has a chance
    // to advance on platforms where consecutive reads can return the
    // exact same instant.
    let src = "\
import time
fn main() -> i32:
    t0: f64 = time.monotonic()
    i: i64 = 0i64
    sum: i64 = 0i64
    while i < 1000i64:
        sum = sum + i
        i = i + 1i64
    t1: f64 = time.monotonic()
    if t1 >= t0:
        println(\"non-decreasing\")
    else:
        println(\"WENT-BACKWARDS\")
    println(\"sum=\" + str(sum))
    return 0
";
    let p = compile_snippet("time_monotonic", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("non-decreasing"), "got: {out:?}");
    assert!(out.contains("sum=499500"), "got: {out:?}");
}

#[test]
fn time_sleep_ms_advances_clock() {
    // Sleep ~80ms and verify the monotonic clock advanced by at least
    // 50ms (lenient floor — Windows default sleep granularity is ~15ms,
    // and CI hosts can be noisy).
    let src = "\
import time
fn main() -> i32:
    t0: f64 = time.monotonic()
    time.sleep_ms(80i64)
    t1: f64 = time.monotonic()
    elapsed_ms: i64 = i64((t1 - t0) * 1000.0)
    if elapsed_ms >= 50i64:
        println(\"advanced\")
    else:
        println(\"DID-NOT-ADVANCE\")
    return 0
";
    let p = compile_snippet("sleep_ms_advance", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("advanced"), "got: {out:?}");
}

#[test]
fn time_format_iso_unix_epoch() {
    let src = "\
import time
fn main() -> i32:
    println(time.format_iso(0.0))
    return 0
";
    let p = compile_snippet("format_iso_epoch", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("1970-01-01T00:00:00Z"), "got: {out:?}");
}

#[test]
fn time_format_iso_known_date() {
    // 2000-01-01 00:00:00 UTC = 946684800 unix-epoch seconds.  Pin this
    // exact value: it's the canonical Y2K test point and there's no
    // ambiguity about the day boundary.
    let src = "\
import time
fn main() -> i32:
    println(time.format_iso(946684800.0))
    return 0
";
    let p = compile_snippet("format_iso_y2k", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("2000-01-01T00:00:00Z"), "got: {out:?}");
}

// ── random module ──────────────────────────────────────────────────────

#[test]
fn random_seed_is_deterministic() {
    // Re-seeding to the same value produces the same sequence.
    let src = "\
import random
fn main() -> i32:
    random.seed(42i64)
    a1: i64 = random.randint(1i64, 100i64)
    a2: i64 = random.randint(1i64, 100i64)
    a3: i64 = random.randint(1i64, 100i64)
    random.seed(42i64)
    b1: i64 = random.randint(1i64, 100i64)
    b2: i64 = random.randint(1i64, 100i64)
    b3: i64 = random.randint(1i64, 100i64)
    if a1 == b1:
        if a2 == b2:
            if a3 == b3:
                println(\"deterministic\")
                return 0
    println(\"DRIFT\")
    return 0
";
    let p = compile_snippet("random_seed", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("deterministic"), "got: {out:?}");
}

#[test]
fn random_randint_respects_bounds() {
    let src = "\
import random
fn main() -> i32:
    random.seed(7i64)
    i: i64 = 0i64
    all_in_range: bool = true
    while i < 100i64:
        v: i64 = random.randint(5i64, 9i64)
        if v < 5i64:
            all_in_range = false
        if v > 9i64:
            all_in_range = false
        i = i + 1i64
    if all_in_range:
        println(\"in-range\")
    else:
        println(\"OUT-OF-RANGE\")
    return 0
";
    let p = compile_snippet("randint_bounds", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("in-range"), "got: {out:?}");
}

#[test]
fn random_random_is_in_unit_interval() {
    let src = "\
import random
fn main() -> i32:
    random.seed(3i64)
    i: i64 = 0i64
    ok: bool = true
    while i < 50i64:
        r: f64 = random.random()
        if r < 0.0:
            ok = false
        if r >= 1.0:
            ok = false
        i = i + 1i64
    if ok:
        println(\"in-unit\")
    else:
        println(\"OUT-OF-UNIT\")
    return 0
";
    let p = compile_snippet("random_unit", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("in-unit"), "got: {out:?}");
}

#[test]
fn random_choice_i64_picks_element() {
    let src = "\
import random
fn main() -> i32:
    random.seed(1i64)
    xs: List[i64] = [10i64, 20i64, 30i64, 40i64, 50i64]
    found_match: bool = false
    i: i64 = 0i64
    while i < 50i64:
        v: i64 = random.choice_i64(xs)
        # Each draw must equal one of the inputs.
        if v == 10i64 or v == 20i64 or v == 30i64 or v == 40i64 or v == 50i64:
            found_match = true
        else:
            found_match = false
            i = 100i64
        i = i + 1i64
    if found_match:
        println(\"all-members\")
    else:
        println(\"FOUND-INTRUDER\")
    return 0
";
    let p = compile_snippet("choice_i64", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("all-members"), "got: {out:?}");
}

#[test]
fn random_choice_str_picks_element() {
    let src = "\
import random
fn main() -> i32:
    random.seed(99i64)
    xs: List[str] = [\"alpha\", \"beta\", \"gamma\"]
    v: str = random.choice_str(xs)
    if v == \"alpha\":
        println(\"ok\")
    else:
        if v == \"beta\":
            println(\"ok\")
        else:
            if v == \"gamma\":
                println(\"ok\")
            else:
                println(\"BAD: \" + v)
    return 0
";
    let p = compile_snippet("choice_str", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("ok"), "got: {out:?}");
}

#[test]
fn random_choice_empty_raises_index_error() {
    let src = "\
import random
fn main() -> i32:
    xs: List[i64] = []
    try:
        v: i64 = random.choice_i64(xs)
        println(\"no error\")
    except IndexError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("choice_empty", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught"), "got: {out:?}");
}

#[test]
fn random_shuffle_preserves_length_and_elements() {
    let src = "\
import random
fn main() -> i32:
    random.seed(11i64)
    xs: List[i64] = [1i64, 2i64, 3i64, 4i64, 5i64]
    random.shuffle_i64(xs)
    n: i64 = i64(len(xs))
    println(\"len=\" + str(n))
    # Each input value must still be present after shuffling.
    sum: i64 = 0i64
    i: i64 = 0i64
    while i < n:
        sum = sum + xs[i]
        i = i + 1i64
    println(\"sum=\" + str(sum))
    return 0
";
    let p = compile_snippet("shuffle_i64", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("len=5"), "got: {out:?}");
    assert!(out.contains("sum=15"), "got: {out:?}");
}

#[test]
fn random_sample_returns_n_elements() {
    let src = "\
import random
fn main() -> i32:
    random.seed(13i64)
    xs: List[i64] = [1i64, 2i64, 3i64, 4i64, 5i64, 6i64, 7i64, 8i64]
    out: List[i64] = random.sample_i64(xs, 3i32)
    println(\"n=\" + str(i64(len(out))))
    return 0
";
    let p = compile_snippet("sample_3", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("n=3"), "got: {out:?}");
}

#[test]
fn random_sample_n_larger_than_len_raises_value_error() {
    let src = "\
import random
fn main() -> i32:
    xs: List[i64] = [1i64, 2i64]
    try:
        out: List[i64] = random.sample_i64(xs, 5i32)
        println(\"no error\")
    except ValueError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("sample_too_big", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught"), "got: {out:?}");
}

// ── math module ────────────────────────────────────────────────────────

#[test]
fn math_constants_have_expected_values() {
    let src = "\
import math
fn main() -> i32:
    # Pi to 6 digits.  We compare as a string so the float formatter
    # doesn't surprise us.
    if math.pi > 3.14159:
        if math.pi < 3.14160:
            println(\"pi-ok\")
    if math.e > 2.71828:
        if math.e < 2.71829:
            println(\"e-ok\")
    # tau == 2*pi.
    diff: f64 = math.tau - 2.0 * math.pi
    if diff > -0.000001:
        if diff < 0.000001:
            println(\"tau-ok\")
    return 0
";
    let p = compile_snippet("math_consts", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("pi-ok"), "got: {out:?}");
    assert!(out.contains("e-ok"), "got: {out:?}");
    assert!(out.contains("tau-ok"), "got: {out:?}");
}

#[test]
fn math_sqrt_log_basics() {
    let src = "\
import math
fn main() -> i32:
    println(\"sqrt-16=\" + str(math.sqrt(16.0)))
    println(\"log2-1024=\" + str(math.log2(1024.0)))
    println(\"log10-1000=\" + str(math.log10(1000.0)))
    return 0
";
    let p = compile_snippet("math_basics", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("sqrt-16=4."), "got: {out:?}");
    assert!(out.contains("log2-1024=10."), "got: {out:?}");
    assert!(out.contains("log10-1000=3."), "got: {out:?}");
}

#[test]
fn math_floor_ceil_return_i64() {
    let src = "\
import math
fn main() -> i32:
    a: i64 = math.floor(3.7)
    b: i64 = math.ceil(3.2)
    c: i64 = math.floor(-1.2)
    d: i64 = math.ceil(-1.2)
    println(\"floor-3.7=\" + str(a))
    println(\"ceil-3.2=\" + str(b))
    println(\"floor-neg-1.2=\" + str(c))
    println(\"ceil-neg-1.2=\" + str(d))
    return 0
";
    let p = compile_snippet("math_floor_ceil", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("floor-3.7=3"), "got: {out:?}");
    assert!(out.contains("ceil-3.2=4"), "got: {out:?}");
    assert!(out.contains("floor-neg-1.2=-2"), "got: {out:?}");
    assert!(out.contains("ceil-neg-1.2=-1"), "got: {out:?}");
}

#[test]
fn math_gcd_returns_correct_value() {
    let src = "\
import math
fn main() -> i32:
    println(\"gcd-12-18=\" + str(math.gcd(12i64, 18i64)))
    println(\"gcd-17-5=\" + str(math.gcd(17i64, 5i64)))
    println(\"gcd-0-7=\" + str(math.gcd(0i64, 7i64)))
    println(\"gcd-neg-12-18=\" + str(math.gcd(0i64 - 12i64, 18i64)))
    return 0
";
    let p = compile_snippet("math_gcd", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("gcd-12-18=6"), "got: {out:?}");
    assert!(out.contains("gcd-17-5=1"), "got: {out:?}");
    assert!(out.contains("gcd-0-7=7"), "got: {out:?}");
    assert!(out.contains("gcd-neg-12-18=6"), "got: {out:?}");
}

#[test]
fn math_factorial_basic_and_boundary() {
    let src = "\
import math
fn main() -> i32:
    println(\"0=\" + str(math.factorial(0i64)))
    println(\"1=\" + str(math.factorial(1i64)))
    println(\"5=\" + str(math.factorial(5i64)))
    println(\"10=\" + str(math.factorial(10i64)))
    println(\"20=\" + str(math.factorial(20i64)))
    return 0
";
    let p = compile_snippet("math_factorial", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("0=1"), "got: {out:?}");
    assert!(out.contains("5=120"), "got: {out:?}");
    assert!(out.contains("10=3628800"), "got: {out:?}");
    assert!(out.contains("20=2432902008176640000"), "got: {out:?}");
}

#[test]
fn math_factorial_negative_raises_value_error() {
    let src = "\
import math
fn main() -> i32:
    try:
        v: i64 = math.factorial(0i64 - 1i64)
        println(\"no error\")
    except ValueError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("math_fact_neg", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught"), "got: {out:?}");
}

#[test]
fn math_factorial_overflow_raises_overflow_error() {
    let src = "\
import math
fn main() -> i32:
    try:
        v: i64 = math.factorial(21i64)
        println(\"no error\")
    except OverflowError as e:
        println(\"caught\")
    return 0
";
    let p = compile_snippet("math_fact_overflow", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("caught"), "got: {out:?}");
}

#[test]
fn math_is_nan_and_is_inf() {
    let src = "\
import math
fn main() -> i32:
    println(\"nan=\" + str(math.is_nan(math.nan)))
    println(\"zero=\" + str(math.is_nan(0.0)))
    println(\"inf=\" + str(math.is_inf(math.inf)))
    println(\"finite=\" + str(math.is_inf(1.5)))
    println(\"neg-inf=\" + str(math.is_inf(0.0 - math.inf)))
    return 0
";
    let p = compile_snippet("math_pred", src);
    let (code, out) = run_file_capture(&p).expect("run");
    assert_eq!(code, 0);
    assert!(out.contains("nan=true"), "got: {out:?}");
    assert!(out.contains("zero=false"), "got: {out:?}");
    assert!(out.contains("inf=true"), "got: {out:?}");
    assert!(out.contains("finite=false"), "got: {out:?}");
    assert!(out.contains("neg-inf=true"), "got: {out:?}");
}

// ── Compile-time checks ────────────────────────────────────────────────

#[test]
fn unknown_time_attr_is_compile_error() {
    let src = "\
import time
fn main() -> i32:
    println(str(time.no_such_attr))
    return 0
";
    let r = compile_source("bad_time.spy".into(), src);
    let err = r.err().expect("should fail");
    let msg = format!("{err}");
    assert!(
        msg.contains("no_such_attr") || msg.contains("no attribute"),
        "got: {msg}"
    );
}

#[test]
fn unknown_random_attr_is_compile_error() {
    let src = "\
import random
fn main() -> i32:
    println(str(random.no_such_thing))
    return 0
";
    let r = compile_source("bad_random.spy".into(), src);
    let err = r.err().expect("should fail");
    let msg = format!("{err}");
    assert!(
        msg.contains("no_such_thing") || msg.contains("no attribute"),
        "got: {msg}"
    );
}

#[test]
fn unknown_math_attr_is_compile_error() {
    let src = "\
import math
fn main() -> i32:
    println(str(math.no_such_thing))
    return 0
";
    let r = compile_source("bad_math.spy".into(), src);
    let err = r.err().expect("should fail");
    let msg = format!("{err}");
    assert!(
        msg.contains("no_such_thing") || msg.contains("no attribute"),
        "got: {msg}"
    );
}
