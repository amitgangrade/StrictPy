//! Strings round 2: native `join`/`lower`/`upper`/`repeat`, the fixed
//! `char_at` wiring (REPORT_V2 bug #7), O(1) ASCII `s[i]` / O(slice_len)
//! ASCII `s.slice(...)` with correct non-ASCII fallback, and the widened
//! chained in-place append (`s = s + a + b`) — including alias safety.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn run(name: &str, src: &str) -> (i32, String) {
    let bytes = compile_source(format!("{name}.spy"), src)
        .unwrap_or_else(|e| panic!("{name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_strround2_{name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    run_file_capture(&out).expect("run")
}

// ── join ────────────────────────────────────────────────────────────────

#[test]
fn join_basic_empty_single_and_empty_sep() {
    let src = "\
fn main() -> i32:
    xs: List[str] = [\"a\", \"b\", \"c\"]
    println(\",\".join(xs))
    one: List[str] = [\"solo\"]
    println(\"-\".join(one))
    empty: List[str] = []
    println(\"[\" + \"-\".join(empty) + \"]\")
    println(\"\".join(xs))
    println(\" :: \".join(xs))
    return 0
";
    let (code, out) = run("join_basic", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "a,b,c\nsolo\n[]\nabc\na :: b :: c\n", "{out:?}");
}

#[test]
fn join_unicode_sep_and_elements() {
    let src = "\
fn main() -> i32:
    xs: List[str] = [\"α\", \"β\", \"γ\"]
    println(\"—\".join(xs))
    println(\",\".join(xs))
    return 0
";
    let (code, out) = run("join_unicode", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "α—β—γ\nα,β,γ\n", "{out:?}");
}

// ── lower / upper ───────────────────────────────────────────────────────

#[test]
fn lower_upper_ascii() {
    let src = "\
fn main() -> i32:
    println(\"HeLLo, WoRLD-42!\".lower())
    println(\"HeLLo, WoRLD-42!\".upper())
    println(\"[\" + \"\".lower() + \"]\")
    println(\"[\" + \"\".upper() + \"]\")
    return 0
";
    let (code, out) = run("case_ascii", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "hello, world-42!\nHELLO, WORLD-42!\n[]\n[]\n", "{out:?}");
}

#[test]
fn lower_upper_unicode() {
    let src = "\
fn main() -> i32:
    println(\"HÉLLO Ñ\".lower())
    println(\"héllo ñ\".upper())
    println(\"straße\".upper())
    return 0
";
    let (code, out) = run("case_unicode", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    // 'ß' uppercases to "SS" (one-to-many mapping, same as Python).
    assert_eq!(out, "héllo ñ\nHÉLLO Ñ\nSTRASSE\n", "{out:?}");
}

// ── repeat ──────────────────────────────────────────────────────────────

#[test]
fn repeat_basic_zero_negative_and_unicode() {
    let src = "\
fn main() -> i32:
    println(\"ab\".repeat(3i64))
    println(\"[\" + \"x\".repeat(0i64) + \"]\")
    println(\"[\" + \"x\".repeat(-2i64) + \"]\")
    println(\"é→\".repeat(2i64))
    println(str(len(\"ab\".repeat(1000i64))))
    return 0
";
    let (code, out) = run("repeat", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "ababab\n[]\n[]\né→é→\n2000\n", "{out:?}");
}

// ── char_at (REPORT_V2 bug #7: compiled but trapped "unknown native id") ─

#[test]
fn char_at_works_ascii_and_unicode() {
    let src = "\
fn b(x: bool) -> str:
    if x:
        return \"T\"
    return \"F\"

fn main() -> i32:
    c: char = \"abcdef\".char_at(2i64)
    println(str(c))
    println(b(\"abcdef\".char_at(0i64) == 'a'))
    u: char = \"héllo\".char_at(1i64)
    println(b(u == 'é'))
    println(str(\"héllo\".char_at(4i64)))
    return 0
";
    let (code, out) = run("char_at", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "c\nT\nT\no\n", "{out:?}");
}

// ── ASCII index/slice fast path + non-ASCII fallback correctness ────────

#[test]
fn index_scan_ascii_and_non_ascii() {
    // Sums codepoints with `s[i]` over both an ASCII and a non-ASCII string;
    // the ASCII string takes the O(1) byte path, the other the chars() walk.
    let src = "\
fn scan(s: str) -> i64:
    total: i64 = 0
    i: i64 = 0
    n: i64 = i64(len(s))
    while i < n:
        c: char = s[i]
        total = total + i64(i32(c))
        i = i + 1
    return total

fn main() -> i32:
    println(str(scan(\"abc\")))
    println(str(scan(\"héllo\")))
    return 0
";
    let (code, out) = run("index_scan", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    // "abc" = 97+98+99 = 294; "héllo" = 104+233+108+108+111 = 664.
    assert_eq!(out, "294\n664\n", "{out:?}");
}

#[test]
fn slice_ascii_and_non_ascii() {
    let src = "\
fn main() -> i32:
    s: str = \"hello world\"
    println(s.slice(6i64, 11i64))
    println(s.slice(0i64, 5i64))
    println(\"[\" + s.slice(3i64, 3i64) + \"]\")
    println(\"[\" + s.slice(9i64, 100i64) + \"]\")
    u: str = \"héllo wörld\"
    println(u.slice(1i64, 4i64))
    println(u.slice(6i64, 11i64))
    return 0
";
    let (code, out) = run("slice", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "world\nhello\n[]\n[ld]\néll\nwörld\n", "{out:?}");
}

#[test]
fn slice_window_scan_is_correct() {
    // Sliding-window slices over an ASCII string (the str_slice_scan shape):
    // count windows equal to a needle.
    let src = "\
fn count(s: str, w: str) -> i64:
    n: i64 = i64(len(s))
    k: i64 = i64(len(w))
    hits: i64 = 0
    i: i64 = 0
    while i + k <= n:
        if s.slice(i, i + k) == w:
            hits = hits + 1
        i = i + 1
    return hits

fn main() -> i32:
    println(str(count(\"abababab\", \"ab\")))
    println(str(count(\"aaaa\", \"aa\")))
    println(str(count(\"abc\", \"zz\")))
    return 0
";
    let (code, out) = run("slice_scan", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "4\n3\n0\n", "{out:?}");
}

// ── chained in-place append (s = s + a + b) ─────────────────────────────

#[test]
fn chained_append_small_exact() {
    // `s` is pure inside build() (init + chain appends + bare return), so
    // the widened in-place chain fires; exact output asserted in main.
    let src = "\
fn build() -> str:
    s: str = \"\"
    i: i64 = 0
    while i < 4:
        s = s + \"<\" + str(i) + \">\"
        i = i + 1
    return s

fn main() -> i32:
    println(build())
    return 0
";
    let (code, out) = run("chain_small", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "<0><1><2><3>\n", "{out:?}");
}

#[test]
fn chained_append_many_iterations() {
    // 20k iterations × 3 operands per step. With the chain reverting to
    // O(n²) StrConcat this is the documented performance cliff; correctness
    // here is asserted via length + endswith.
    let src = "\
fn b(x: bool) -> str:
    if x:
        return \"T\"
    return \"F\"

fn build(n: i64) -> str:
    s: str = \"\"
    i: i64 = 0
    while i < n:
        s = s + \"ab\" + str(i) + \";\"
        i = i + 1
    return s

fn main() -> i32:
    s: str = build(20000i64)
    println(str(len(s)))
    println(b(s.startswith(\"ab0;ab1;\")))
    println(b(s.endswith(\"ab19998;ab19999;\")))
    return 0
";
    let (code, out) = run("chain_many", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    // len = 20000*("ab"=2 + ";"=1) + digits(0..19999)
    //     = 60000 + (10*1 + 90*2 + 900*3 + 9000*4 + 10000*5) = 60000 + 88890.
    assert_eq!(out, "148890\nT\nT\n", "{out:?}");
}

#[test]
fn chained_append_unicode_operands() {
    // Pure accumulator in a helper fn → in-place chain fires with
    // multi-byte operands (the ascii flag must drop and stay correct).
    let src = "\
fn build() -> str:
    s: str = \"\"
    i: i64 = 0
    while i < 3:
        s = s + \"é\" + str(i) + \"→\"
        i = i + 1
    return s

fn main() -> i32:
    s: str = build()
    println(s)
    println(str(len(s)))
    return 0
";
    let (code, out) = run("chain_unicode", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "é0→é1→é2→\n9\n", "{out:?}");
}

#[test]
fn chained_append_alias_safety() {
    // `t = s` aliases the accumulator, so the in-place chain must NOT fire;
    // later chained appends to `s` must leave `t` at its old value.
    let src = "\
fn main() -> i32:
    s: str = \"\"
    s = s + \"x\" + \"y\" + \"z\"
    t: str = s
    s = s + \"A\" + \"B\"
    println(t)
    println(s)
    return 0
";
    let (code, out) = run("chain_alias", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "xyz\nxyzAB\n", "{out:?}");
}

#[test]
fn chained_append_alias_in_loop_safety() {
    // Alias captured mid-loop: every snapshot must keep its value while the
    // accumulator keeps growing through chained appends.
    let src = "\
fn main() -> i32:
    s: str = \"\"
    snaps: List[str] = []
    i: i64 = 0
    while i < 3:
        s = s + \"a\" + str(i)
        snaps.append(s)
        i = i + 1
    println(snaps[0])
    println(snaps[1])
    println(snaps[2])
    println(s)
    return 0
";
    let (code, out) = run("chain_alias_loop", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "a0\na0a1\na0a1a2\na0a1a2\n", "{out:?}");
}

#[test]
fn chained_append_operand_order_and_calls() {
    // Operands that are calls / nested concats evaluate in source order.
    let src = "\
fn tag(x: str) -> str:
    return \"[\" + x + \"]\"

fn main() -> i32:
    s: str = \"start:\"
    s = s + tag(\"a\") + \"-\" + tag(\"b\")
    println(s)
    return 0
";
    let (code, out) = run("chain_calls", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "start:[a]-[b]\n", "{out:?}");
}

// ── integration: join over split + case-normalised headers ──────────────

#[test]
fn join_split_roundtrip_and_header_normalise() {
    let src = "\
fn main() -> i32:
    csv: str = \"a,b,c,d\"
    cells: List[str] = csv.split(\",\")
    println(\"|\".join(cells))
    hdr: str = \"Content-Length\".lower()
    println(hdr)
    return 0
";
    let (code, out) = run("integration", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "a|b|c|d\ncontent-length\n", "{out:?}");
}
