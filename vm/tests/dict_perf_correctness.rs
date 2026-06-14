//! Correctness tests for the dict hash-cache + FxHash change (strdict.rs).
//!
//! The perf design caches each key string's FxHash in its object header
//! (`gc_meta` bits 32..64, validity bit 1 — see `object::cached_str_hash`) and
//! the dict stores `(hash, copied key bytes, value)` in a hashbrown table
//! keyed by precomputed hash. These tests pin the correctness obligations
//! that design creates:
//!   1. The cache must be keyed per string OBJECT but agree across different
//!      objects with equal bytes (same entry hit either way).
//!   2. `StrAppendInPlace` MUTATES a string's bytes in place — it must
//!      invalidate the cached hash, so an accumulator used as a key, then
//!      extended, then used again behaves like a fresh string.
//!   3. Unicode keys hash/compare by full byte content.
//!   4. Large mixed insert/lookup/has/remove workloads stay consistent
//!      (FxHash collisions are resolved by byte equality, and key bytes are
//!      copied at insert so later mutation of the source string can't corrupt
//!      stored keys).

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_vm::run_file_capture;

fn run(name: &str, src: &str) -> (i32, String) {
    let bytes = compile_source(format!("{name}.spy"), src)
        .unwrap_or_else(|e| panic!("{name}: compile error: {e}"));
    let mut out = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out.push(format!("strictpy_dictperf_{name}.spyc"));
    fs::write(&out, &bytes).expect("write spyc");
    run_file_capture(&out).expect("run")
}

#[test]
fn same_key_via_different_string_instances() {
    // The key reaches the dict as different StringRepr objects (literal,
    // concat-built, split-derived) — all must hit the same entry.
    let src = "\
fn main() -> i32:
    d: Dict[str, i64] = {}
    d[\"alpha\"] = 1
    built: str = \"al\" + \"pha\"
    println(str(d[built]))                  # 1
    parts: List[str] = \"alpha,beta\".split(\",\")
    println(str(d.has(parts[0])))           # true
    d[built] = 2
    println(str(d[\"alpha\"]))               # 2 (overwrite via other instance)
    println(str(len(d)))                     # 1 (still one entry)
    return 0
";
    let (code, out) = run("instances", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "1\ntrue\n2\n1\n", "{out:?}");
}

#[test]
fn accumulator_key_extended_and_reused() {
    // An accumulator string is used as a key, then extended, then used again.
    // Whether the compiler takes the in-place-append path or the StrConcat
    // fallback, lookups must be by current byte content (cache invalidated on
    // mutation).
    let src = "\
fn main() -> i32:
    d: Dict[str, i64] = {}
    s: str = \"\"
    s = s + \"ab\"
    d[s] = 10
    s = s + \"cd\"
    d[s] = 20
    println(str(d[\"ab\"]))                  # 10 (stored key copied, unchanged)
    println(str(d[\"abcd\"]))                # 20
    println(str(d.has(s)))                   # true
    println(str(len(d)))                     # 2
    return 0
";
    let (code, out) = run("accum_key", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "10\n20\ntrue\n2\n", "{out:?}");
}

#[test]
fn unicode_keys() {
    let src = "\
fn main() -> i32:
    d: Dict[str, i64] = {}
    d[\"héllo\"] = 1
    d[\"héllô\"] = 2
    d[\"日本語\"] = 3
    println(str(d[\"héllo\"]))               # 1
    println(str(d[\"héllô\"]))               # 2
    println(str(d[\"日本語\"]))               # 3
    println(str(d.has(\"hello\")))           # false (ascii lookalike misses)
    println(str(len(d)))                      # 3
    return 0
";
    let (code, out) = run("unicode", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "1\n2\n3\nfalse\n3\n", "{out:?}");
}

#[test]
fn large_mixed_workload_with_removal() {
    // 10k inserts via stringified ints, full lookup sweep, remove half
    // (alternating del / remove), verify membership and a value checksum.
    let src = "\
fn main() -> i32:
    d: Dict[str, i64] = {}
    i: i64 = 0
    while i < 10000:
        d[str(i)] = i * 3i64
        i = i + 1
    println(str(len(d)))                     # 10000
    total: i64 = 0
    i = 0
    while i < 10000:
        total = total + d[str(i)]
        i = i + 1
    println(str(total))                       # 3 * 10000*9999/2 = 149985000
    i = 0
    while i < 10000:
        if i % 2i64 == 0i64:
            del d[str(i)]
        i = i + 2
    println(str(len(d)))                     # 5000 evens removed -> 5000 left
    println(str(d.has(\"0\")))               # false
    println(str(d.has(\"1\")))               # true
    rem: i64 = 0
    i = 1
    while i < 10000:
        rem = rem + d[str(i)]
        i = i + 2
    println(str(rem))                         # odd i: 3 * 5000^2 = 75000000
    return 0
";
    let (code, out) = run("large", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(
        out, "10000\n149985000\n5000\nfalse\ntrue\n75000000\n",
        "{out:?}"
    );
}

#[test]
fn overwrite_and_reinsert_after_remove() {
    let src = "\
fn main() -> i32:
    d: Dict[str, i64] = {}
    d[\"k\"] = 1
    d[\"k\"] = 2
    println(str(d[\"k\"]))                   # 2
    d.remove(\"k\")
    println(str(d.has(\"k\")))               # false
    d[\"k\"] = 3
    println(str(d[\"k\"]))                   # 3
    println(str(len(d)))                      # 1
    return 0
";
    let (code, out) = run("overwrite", src);
    assert_eq!(code, 0, "stdout: {out:?}");
    assert_eq!(out, "2\nfalse\n3\n1\n", "{out:?}");
}
