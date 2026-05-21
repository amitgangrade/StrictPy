//! Stable native-function identifiers shared between the compiler and VM.
//!
//! Every prelude/builtin and stdlib symbol that the compiler lowers to a
//! `CALL_NATIVE` opcode is assigned a `u32` here. Numbering is in stable
//! blocks (1–29 core, 30–49 io, 50–59 channels, 60–69 threads, 70–89 math,
//! 90–119 list/dict/string ops, 120+ misc) so future additions don't
//! perturb existing IDs. Zero is reserved as "invalid".
//!
//! The VM (M4) dispatches `CALL_NATIVE` by switching on these ids.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum NativeFn {
    // ── 1–29: core builtins ─────────────────────────────────────────────
    Println       = 1,
    Print         = 2,
    Len           = 3,
    Range         = 4,
    Assert        = 5,
    StrFromI32    = 6,
    StrFromI64    = 7,
    StrFromF64    = 8,
    StrFromBool   = 9,
    StrFromChar   = 10,
    StrConcat     = 11,
    Abs           = 12,
    Min           = 13,
    Max           = 14,
    I32FromI64    = 15,
    I64FromI32    = 16,
    F64FromI32    = 17,
    F64FromI64    = 18,
    I32FromF64    = 19,
    StrFromBytes  = 20,
    StrFromAny    = 21, // generic `str(x)` fallback
    BoolFromAny   = 22,
    CharFromI32   = 23,
    StrSlice      = 24,
    StrAppendChar = 25,
    // real-world: csv_aggregate — parse a decimal string into f64 / i64.
    // Spec §9 lists the numeric primitive constructors (`f64(x)`) but those
    // convert *between* numeric types, not from str; csv_aggregate.spy is the
    // first program that needs str→number parsing.
    F64FromStr    = 26,
    I64FromStr    = 27,
    /// M11 fix: `i64(f: f64)` — truncate toward zero. The IR lowerer
    /// previously had no entry for this and routed every `i64(x)` through
    /// `I64FromI32`, which read the f64 bit pattern as an integer.
    I64FromF64    = 29,
    // real-world: csv_aggregate / wordcount / markov — every stress test
    // that touches text rolled its own splitter. `s.split(sep) -> List[str]`
    // lifts that boilerplate. Returns the empty list for an empty `s`
    // (matches Rust's `str::split`'s behaviour minus the trailing-empty
    // edge case, which we suppress for ergonomics — see vm/src/builtins.rs).
    StrSplit      = 28,

    // ── 30–49: io ───────────────────────────────────────────────────────
    IoOpen     = 30,
    FileRead   = 31,
    FileWrite  = 32,
    FileClose  = 33,
    FileEnter  = 34, // for `with` desugaring (__enter__)
    FileExit   = 35,

    // ── 50–59: channels ─────────────────────────────────────────────────
    ChannelNew     = 50,
    ChannelSend    = 51,
    ChannelRecv    = 52,
    ChannelTryRecv = 53,
    ChannelClose   = 54,

    // ── 60–69: threads ──────────────────────────────────────────────────
    ThreadNew   = 60,
    ThreadStart = 61,
    ThreadJoin  = 62,

    // ── 70–89: math ─────────────────────────────────────────────────────
    MathSqrt = 70,
    MathSin  = 71,
    MathCos  = 72,
    MathTan  = 73,
    MathLog  = 74,
    MathExp  = 75,
    MathPow  = 76,
    MathFloor = 77,
    MathCeil = 78,
    MathAbsF = 79,

    // ── 90–119: list / dict / set ops not covered by dedicated opcodes ──
    ListAppend = 90,
    ListGet    = 91,
    ListSet    = 92,
    ListLen    = 93,
    DictGet    = 94,
    DictSet    = 95,
    DictHas    = 96,
    DictKeys   = 97,
    DictValues = 98,
    DictLen    = 99,
    SetAdd     = 100,
    SetHas     = 101,
    SetLen     = 102,
    /// M7: allocate a fresh empty dict. Used to lower `{}` dict literals
    /// so they materialise an actual DictRepr handle instead of a null
    /// pointer. See vm/src/interp.rs::alloc_dict.
    DictNew    = 103,
    /// M7: read `s[i]` for strings — returns the i-th char as u32. Mirrors
    /// the existing STR_CHAR_AT opcode but is reachable from the
    /// NativeCall path used by the Index-expr lowering.
    StrCharAt  = 104,
    // real-world: every stress test that produced ranked output
    // (csv_aggregate top-N, wordcount frequency table, markov chain
    // training) had to hand-roll a sort. Both forms take a trailing
    // u32 type-tag operand (TypeTag::I64/F64/Ref) that the VM uses to
    // pick the comparator — passing the tag explicitly is simpler than
    // teaching the native to walk the list's RuntimeType vtable.
    /// In-place: `xs.sort()` — args: `[list_ptr, type_tag_u32]`.
    ListSort   = 105,
    /// Returns a copy: `sorted(xs)` — args: `[list_ptr, type_tag_u32]`.
    ListSorted = 106,
    // real-world: fix — JSON / BF / KV-store stress tests all wanted
    // `xs.pop()`. The opcode `ListPop = 0xF2` exists in the VM but
    // wasn't reachable via the source-level method call (no native id,
    // no typecheck synth entry, no IR dispatch). Remove and return the
    // last element; trap with IndexError on empty.
    ListPop    = 107,

    // ── 130–149: `sys` module (M19) ─────────────────────────────────────
    // Foundation milestone for a real stdlib: the import-resolver and
    // module-attribute typecheck were both new in M19 (no built-in
    // module previously exposed attributes/functions to user code; the
    // pre-M19 prelude flattened every name into module-scope). All four
    // natives below dispatch through the standard `CallNative` path.
    /// `sys.argv` — lazy List[str] of program args. The VM caches the
    /// materialised list pointer in `Interpreter::sys_argv_cache` so
    /// repeated reads return the same heap object.
    SysArgv     = 130,
    /// `sys.exit(code: i32) -> Never` — raises `VmError::Exit(code)`.
    /// Deliberately not catchable: `propagate_exception` only matches
    /// `VmError::UncaughtException`, so `Exit` walks straight up to the
    /// CLI's top-level handler. Mirrors Python's `SystemExit` (a
    /// BaseException, not an Exception).
    SysExit     = 131,
    /// `sys.platform` — one of `"windows" | "linux" | "macos" | "unknown"`.
    /// Allocated once per call (str interning is a v0.3 nice-to-have).
    SysPlatform = 132,
    /// `sys.version` — version banner string. Constant per build.
    SysVersion  = 133,

    // ── 140–159: `os` module (M20a) ─────────────────────────────────────
    // Environment + filesystem syscalls.  Each variant maps to a Rust
    // `std::env` or `std::fs` call.  All failures raise `IOError` via the
    // M15 `VmError::UncaughtException` machinery (mirrors `IoOpen`).
    /// `os.env(key: str) -> str?` — reads a process env var.  `none` if unset.
    OsEnv      = 140,
    /// `os.set_env(key: str, value: str) -> None` — process-local set.
    OsSetEnv   = 141,
    /// `os.getcwd() -> str` — `std::env::current_dir`.
    OsGetCwd   = 142,
    /// `os.chdir(path: str) -> None` — `std::env::set_current_dir`.
    OsChdir    = 143,
    /// `os.listdir(path: str) -> List[str]` — entry names only.
    OsListDir  = 144,
    /// `os.remove(path: str) -> None` — `std::fs::remove_file`.
    OsRemove   = 145,
    /// `os.mkdir(path: str) -> None` — non-recursive `std::fs::create_dir`.
    OsMkdir    = 146,
    /// `os.exists(path: str) -> bool` — true for file *or* dir.
    OsExists   = 147,
    /// `os.is_file(path: str) -> bool`.
    OsIsFile   = 148,
    /// `os.is_dir(path: str) -> bool`.
    OsIsDir    = 149,
    /// `os.read_file(path: str) -> str` — convenience wrapper.
    OsReadFile  = 150,
    /// `os.write_file(path: str, content: str) -> None` — convenience.
    OsWriteFile = 151,

    // ── 160–169: `path` module (M20a) ────────────────────────────────────
    // Pure path-manipulation helpers.  Use Rust's `std::path::Path` so the
    // OS separator is picked up correctly (`/` on Unix, `\` on Windows).
    /// `path.join(a, b)` — 2-arg path concat.
    PathJoin    = 160,
    /// `path.join3(a, b, c)` — 3-arg path concat (no varargs in v0.2).
    PathJoin3   = 161,
    /// `path.dirname(p)` — parent dir (empty for bare name).
    PathDirname = 162,
    /// `path.basename(p)` — last component.
    PathBasename = 163,
    /// `path.splitext(p) -> (without_ext, ext_with_dot)`.  Returns a
    /// heap-allocated `(str, str)` tuple (16-byte payload, two str-ptr slots).
    PathSplitext = 164,
    /// `path.sep` — the OS separator string (`"/"` or `"\\"`).
    PathSep     = 165,

    // ── 170–179: `io` module (M20a) ─────────────────────────────────────
    // Line-based stdin/stdout/stderr.  Sister to M5's `io.File` (read/write
    // on opened files); these natives operate on the process's standard
    // streams via `interp.stdout_write` + a fresh `io::stdin().read_line`.
    /// `io.input() -> str` — one line from stdin, no trailing newline.
    IoInput     = 170,
    /// `io.input_with_prompt(prompt) -> str` — print + flush + read line.
    IoInputPrompt = 171,
    /// `io.write_stdout(s)` — like `print` but reachable as a stdlib symbol.
    IoWriteStdout = 172,
    /// `io.write_stderr(s)` — diagnostics to the process's stderr.
    IoWriteStderr = 173,
    /// `io.flush_stdout()` — flush so prompts appear before the next read.
    IoFlushStdout = 174,

    // ── 175–184: `time` module (M20b) ───────────────────────────────────
    // Wall-clock + monotonic clock + sleep.  All times use Rust's `std::time`
    // primitives; `monotonic()` is anchored to a per-process `Instant` set
    // up at interpreter construction.
    /// `time.now() -> f64` — Unix-epoch seconds (fractional precision).
    TimeNow       = 175,
    /// `time.now_ms() -> i64` — Unix-epoch milliseconds.
    TimeNowMs     = 176,
    /// `time.monotonic() -> f64` — seconds since interpreter init.
    TimeMonotonic = 177,
    /// `time.sleep_s(seconds: f64) -> None`.
    TimeSleepS    = 178,
    /// `time.sleep_ms(millis: i64) -> None`.
    TimeSleepMs   = 179,
    /// `time.format_iso(epoch_s: f64) -> str` — hand-formatted ISO 8601 UTC.
    TimeFormatIso = 180,

    // ── 185–199: `random` module (M20b) ─────────────────────────────────
    // Seeded LCG (Numerical Recipes constants).  State lives on the
    // interpreter so it's per-program; `random.seed(s)` resets it.
    //
    // Generics: stdlib functions can't be generic in v0.2 (the M17
    // generic-fn worklist only sees user-defined .spy fns), so we ship
    // monomorphic `_i64` / `_f64` / `_str` variants for `choice` /
    // `shuffle` / `sample`.  The underlying VM implementation is the same
    // raw-u64-slot logic; the variant just pins the typecheck signature.
    /// `random.seed(s: i64) -> None` — set the LCG state.
    RandomSeed        = 185,
    /// `random.randint(lo: i64, hi: i64) -> i64` — inclusive on both ends.
    RandomRandint     = 186,
    /// `random.random() -> f64` — uniform in `[0.0, 1.0)`.
    RandomRandom      = 187,
    /// `random.choice_i64(xs: List[i64]) -> i64`.  Raises IndexError on `[]`.
    RandomChoiceI64   = 188,
    /// `random.choice_f64(xs: List[f64]) -> f64`.
    RandomChoiceF64   = 189,
    /// `random.choice_str(xs: List[str]) -> str`.
    RandomChoiceStr   = 190,
    /// `random.shuffle_i64(xs: List[i64]) -> None` — in-place Fisher-Yates.
    RandomShuffleI64  = 191,
    /// `random.shuffle_f64(xs: List[f64]) -> None`.
    RandomShuffleF64  = 192,
    /// `random.shuffle_str(xs: List[str]) -> None`.
    RandomShuffleStr  = 193,
    /// `random.sample_i64(xs: List[i64], n: i32) -> List[i64]`.
    /// Raises ValueError when `n < 0` or `n > len(xs)`.
    RandomSampleI64   = 194,
    /// `random.sample_f64(xs: List[f64], n: i32) -> List[f64]`.
    RandomSampleF64   = 195,
    /// `random.sample_str(xs: List[str], n: i32) -> List[str]`.
    RandomSampleStr   = 196,

    // ── 200–229: `math` module (M20b) ───────────────────────────────────
    // The flat prelude `sqrt` / `sin` / `cos` / etc. natives (ids 70–79)
    // remain unchanged for backward compatibility.  `math.sqrt(x)` and
    // friends route to the *same* underlying NativeFn ids, so the diff
    // is just a registration in `seed_stdlib_modules`.  Genuinely new
    // entries are `log2`, `log10`, `gcd`, `factorial`, `is_nan`, `is_inf`,
    // and `floor`/`ceil`-to-i64 (Python returns int, not float).
    /// `math.log2(x: f64) -> f64`.
    MathLog2      = 200,
    /// `math.log10(x: f64) -> f64`.
    MathLog10     = 201,
    /// `math.floor(x: f64) -> i64` — truncates toward `-inf`, returns int.
    MathFloorI    = 202,
    /// `math.ceil(x: f64) -> i64` — toward `+inf`, returns int.
    MathCeilI     = 203,
    /// `math.gcd(a: i64, b: i64) -> i64` — Euclidean; result is non-negative.
    MathGcd       = 204,
    /// `math.factorial(n: i64) -> i64`.  Range `0 ≤ n ≤ 20`; outside that
    /// the function raises `ValueError` (negative) or `OverflowError`
    /// (would exceed i64::MAX).
    MathFactorial = 205,
    /// `math.is_nan(x: f64) -> bool`.
    MathIsNan     = 206,
    /// `math.is_inf(x: f64) -> bool` — true for both `+inf` and `-inf`.
    MathIsInf     = 207,
    /// `math.pi` / `math.e` / `math.tau` / `math.inf` / `math.nan` —
    /// f64 constants exposed as zero-arg natives.  Each handler ignores
    /// its args and returns the constant's bit pattern.
    MathConstPi   = 208,
    MathConstE    = 209,
    MathConstTau  = 210,
    MathConstInf  = 211,
    MathConstNan  = 212,

    // ── 213–219: `json` module (M20c) ───────────────────────────────────
    // M20c ships json without exposing a typed JsonValue tree to user
    // code — the typed-class path needs stdlib-class registration
    // infrastructure that doesn't yet exist (deferred to v0.3).  The
    // M18 example `examples/json_parse_v2.spy` remains the canonical
    // typed-parser demo; this module is the ergonomic
    // validate-and-reserialize surface for everyday programs.
    /// `json.parse_to_string(s: str) -> str` — parse, then re-serialize
    /// as canonical compact JSON.  Raises `ValueError` on malformed
    /// input.  Useful for "is this valid + normalize it".
    JsonParseToString = 213,
    /// `json.is_valid(s: str) -> bool` — true iff `s` parses as JSON.
    JsonIsValid       = 214,
    /// `json.pretty(s: str, indent: i32) -> str` — parse + pretty-print
    /// with N-space indent.  Raises `ValueError` on malformed input.
    JsonPretty        = 215,
    /// `json.escape(s: str) -> str` — render `s` as a JSON string
    /// literal (surrounding quotes included, control chars escaped).
    JsonEscape        = 216,
    /// `json.minify(s: str) -> str` — alias of `parse_to_string`.
    JsonMinify        = 217,

    // ── 220–229: `re` module (M20c) ─────────────────────────────────────
    // Backed by the `regex` crate.  Patterns are recompiled on each
    // call for v0.2 — a Pattern handle for cached compilation is v0.3.
    // Bad patterns raise `ValueError`.  Find/find_all/replace/split
    // follow the `regex::Regex` semantics, which are close enough to
    // Python's `re` for v0.2 purposes (the divergences are documented
    // in spec §9.14).
    /// `re.match(pattern: str, s: str) -> bool` — Python `fullmatch`.
    ReMatch    = 220,
    /// `re.search(pattern: str, s: str) -> bool`.
    ReSearch   = 221,
    /// `re.find(pattern: str, s: str) -> (i32, i32)` — first match
    /// (start, end), or `(-1, -1)` if no match.
    ReFind     = 222,
    /// `re.find_all(pattern: str, s: str) -> List[str]`.
    ReFindAll  = 223,
    /// `re.replace(pattern: str, s: str, repl: str) -> str`.
    ReReplace  = 224,
    /// `re.split(pattern: str, s: str) -> List[str]`.
    ReSplit    = 225,
    /// `re.is_valid(pattern: str) -> bool` — true iff the pattern
    /// compiles.  Doesn't raise.
    ReIsValid  = 226,

    // ── 310–321: `itertools` module (M22 P2C) ───────────────────────────
    // Iteration helpers. Stdlib functions aren't generic in v0.2 (the M17
    // generic-fn worklist only sees user-defined .spy fns), so we ship
    // monomorphic per-element-type variants the same way M20b's
    // `random.choice_i64/_f64/_str` does. Functions whose element type
    // doesn't influence the IR layout (e.g. `range_step` always returns
    // `List[i64]`) ship as a single non-generic variant.
    //
    // The runtime data layout for List/Tuple is type-erased u64 slots, so
    // many of these handlers are physically identical — the variants exist
    // only to pin static typecheck signatures and to allow the IR to emit
    // `Load(offset)` against the right shape.
    /// `itertools.range_step(start, stop, step) -> List[i64]` — like
    /// Python's three-arg `range()`.  Raises `ValueError` on `step == 0`.
    ItertoolsRangeStep      = 310,
    /// `itertools.enumerate_str(xs: List[str]) -> List[Tuple[i32, str]]`.
    /// Returns `[(0, xs[0]), (1, xs[1]), ...]`.
    ItertoolsEnumerateStr   = 311,
    /// `itertools.enumerate_i64(xs: List[i64]) -> List[Tuple[i32, i64]]`.
    ItertoolsEnumerateI64   = 312,
    /// `itertools.zip_str_str(xs: List[str], ys: List[str]) -> List[Tuple[str, str]]`.
    /// Truncates to the shorter input — like Python's `zip()`.
    ItertoolsZipStrStr      = 313,
    /// `itertools.zip_i64_i64(xs: List[i64], ys: List[i64]) -> List[Tuple[i64, i64]]`.
    ItertoolsZipI64I64      = 314,
    /// `itertools.chain_str(xs: List[str], ys: List[str]) -> List[str]`.
    ItertoolsChainStr       = 315,
    /// `itertools.chain_i64(xs: List[i64], ys: List[i64]) -> List[i64]`.
    ItertoolsChainI64       = 316,
    /// `itertools.take_str(xs: List[str], n: i32) -> List[str]` — first N
    /// elements (clamped to length).  Two variants for str/i64 because the
    /// typecheck signature differs; runtime is identical.
    ItertoolsTakeStr        = 317,
    /// `itertools.drop_str(xs: List[str], n: i32) -> List[str]` — skip
    /// first N elements.
    ItertoolsDropStr        = 318,
    /// `itertools.pairwise_str(xs: List[str]) -> List[Tuple[str, str]]` —
    /// adjacent pairs.  `[a, b, c]` → `[(a, b), (b, c)]`.
    ItertoolsPairwiseStr    = 319,
    /// `itertools.accumulate_i64(xs: List[i64]) -> List[i64]` — running
    /// prefix sum. `[1, 2, 3]` → `[1, 3, 6]`.  Empty input → empty output.
    ItertoolsAccumulateI64  = 320,
    /// `itertools.flatten_str(xs: List[List[str]]) -> List[str]` — list
    /// concatenation.  v0.2 only ships the str shape; i64/f64 variants
    /// are v0.3 work.
    ItertoolsFlattenStr     = 321,

    // ── 322–329: `statistics` module (M22 P2C) ──────────────────────────
    // Descriptive statistics over `List[f64]`.  All math is plain Rust
    // f64 arithmetic — no external crate.  Empty/short input raises
    // `ValueError` via the M15 UncaughtException path.
    /// `statistics.mean(xs: List[f64]) -> f64` — arithmetic mean.
    /// Raises `ValueError` on empty input.
    StatsMean      = 322,
    /// `statistics.median(xs: List[f64]) -> f64` — middle value of a
    /// sorted copy; for even-length input, average of the two centre
    /// values.  Raises `ValueError` on empty input.
    StatsMedian    = 323,
    /// `statistics.stdev(xs: List[f64]) -> f64` — sample standard
    /// deviation (n-1 denominator, Bessel-corrected).  Raises
    /// `ValueError` when `len(xs) < 2`.
    StatsStdev     = 324,
    /// `statistics.variance(xs: List[f64]) -> f64` — sample variance
    /// (n-1 denominator).  Raises `ValueError` when `len(xs) < 2`.
    StatsVariance  = 325,
    /// `statistics.min_max(xs: List[f64]) -> Tuple[f64, f64]` — single
    /// pass.  Raises `ValueError` on empty input.
    StatsMinMax    = 326,
    /// `statistics.sum(xs: List[f64]) -> f64` — total.  Empty input
    /// returns 0.0 (matches Python's `sum`).
    StatsSum       = 327,
    /// `statistics.quantile(xs: List[f64], q: f64) -> f64` — linear-
    /// interpolation quantile.  `q` is clamped to `[0.0, 1.0]`; values
    /// outside that range raise `ValueError`.  `q == 0.5` is the median.
    StatsQuantile  = 328,
    /// `statistics.mode_str(xs: List[str]) -> str` — most frequent
    /// element.  Ties broken by first-seen order.  Raises `ValueError`
    /// on empty input.
    StatsModeStr   = 329,

    // ── 330-349: M22 P2D — `struct` + `urllib_parse` modules ────────────
    // The `struct` module packs primitive integer / float values into
    // fixed-width binary buffers.  StrictPy's `str` is logically a
    // sequence of Unicode chars; we encode each "byte" as a Unicode
    // codepoint 0–255 (so byte 0xFF becomes char U+00FF, which encodes
    // to two UTF-8 bytes on disk but reads back as one char).  The
    // resulting `str` is therefore NOT a valid binary buffer on the
    // wire — for that, v0.3 will add a real `bytes` runtime type — but
    // it round-trips losslessly through `pack`/`unpack` and is a usable
    // shape for in-program protocol fiddling.  See spec §9.15.
    //
    // v0.3 / stretch goals not shipped:
    //   - pack_u16_be / le + unpack_u16_be / le (two more ID pairs)
    //   - real `bytes` type → would let `pack` return raw bytes, not
    //     a "wide-char" str
    //
    /// `struct.pack_u32_be(value: i64) -> str` — 4 chars/bytes, big-endian.
    StructPackU32Be   = 330,
    /// `struct.pack_u32_le(value: i64) -> str` — 4 chars/bytes, little-endian.
    StructPackU32Le   = 331,
    /// `struct.pack_u64_be(value: i64) -> str` — 8 chars/bytes, big-endian.
    StructPackU64Be   = 332,
    /// `struct.pack_u64_le(value: i64) -> str` — 8 chars/bytes, little-endian.
    StructPackU64Le   = 333,
    /// `struct.pack_f64_be(value: f64) -> str` — 8 chars/bytes, IEEE 754 big-endian.
    StructPackF64Be   = 334,
    /// `struct.pack_f64_le(value: f64) -> str` — 8 chars/bytes, IEEE 754 little-endian.
    StructPackF64Le   = 335,
    /// `struct.unpack_u32_be(bytes: str, offset: i32) -> i64`.
    StructUnpackU32Be = 336,
    /// `struct.unpack_u32_le(bytes: str, offset: i32) -> i64`.
    StructUnpackU32Le = 337,
    /// `struct.unpack_u64_be(bytes: str, offset: i32) -> i64`.
    StructUnpackU64Be = 338,
    /// `struct.unpack_u64_le(bytes: str, offset: i32) -> i64`.
    StructUnpackU64Le = 339,
    /// `struct.unpack_f64_be(bytes: str, offset: i32) -> f64`.
    StructUnpackF64Be = 340,
    /// `struct.unpack_f64_le(bytes: str, offset: i32) -> f64`.
    StructUnpackF64Le = 341,

    // ── `urllib_parse` module (342-347) ─────────────────────────────────
    // Hand-rolled URL helpers.  Module name uses underscore — submodules
    // (e.g. `urllib.parse`) are v0.3.  `parse_url` / `join_url` deferred.
    /// `urllib_parse.quote(s: str) -> str` — percent-encode unsafe chars.
    UrlQuote      = 342,
    /// `urllib_parse.quote_plus(s: str) -> str` — quote with `+` for spaces.
    UrlQuotePlus  = 343,
    /// `urllib_parse.unquote(s: str) -> str` — inverse of `quote`.
    UrlUnquote    = 344,
    /// `urllib_parse.unquote_plus(s: str) -> str` — inverse of `quote_plus`.
    UrlUnquotePlus = 345,
    /// `urllib_parse.urlencode(pairs: List[Tuple[str, str]]) -> str`.
    UrlEncode     = 346,
    /// `urllib_parse.parse_query(qs: str) -> List[Tuple[str, str]]`.
    UrlParseQuery = 347,

    // ── 350–369: `subprocess` module (M23 P3a-A) ────────────────────────
    // Cross-platform process spawn + wait + IO capture, backed by
    // Rust's `std::process::Command`.  Running processes are tracked
    // in a global `Mutex<HashMap<i64, Child>>` (see `vm/src/builtins.rs::
    // SUBPROCESS_TABLE`) and exposed to user code as opaque `i64` handles
    // — the same pattern M5 used for `io.File` (resource table + handle)
    // because stdlib classes are still v0.3 work.
    //
    // The blocking convenience wrappers `run` and `run_with_stdin` cover
    // the common case (`subprocess.run("ls", ["-l"])`).  `spawn` / `wait` /
    // `try_wait` / `kill` provide the non-blocking primitives for daemons
    // and supervision trees.
    //
    // Streaming stdin/stdout/stderr — i.e. `Popen.stdout.read(...)` — is
    // intentionally NOT shipped; it would need readable byte handles
    // which are also v0.3 territory.  v0.2 programs that need streaming
    // can spawn a child, pipe via `run_with_stdin`, and parse the
    // collected stdout/stderr after the child exits.
    //
    /// `subprocess.run(prog: str, args: List[str]) -> Tuple[i32, str, str]`.
    /// Spawn-and-wait.  Captures both stdout and stderr.  Returns
    /// `(exit_code, stdout, stderr)`.  Raises `IOError` if the spawn
    /// fails (`prog` not found, permission denied, etc.).
    SubprocessRun           = 350,
    /// `subprocess.run_with_stdin(prog: str, args: List[str], stdin_data: str)
    ///     -> Tuple[i32, str, str]`.
    /// Same as `run` but pipes `stdin_data` to the child's stdin before
    /// waiting.  Useful for filters (`sort`, `grep`, `wc`) that read
    /// stdin.  Raises `IOError` on spawn or pipe failure.
    SubprocessRunWithStdin  = 351,
    /// `subprocess.spawn(prog: str, args: List[str]) -> i64` — start a
    /// process without waiting; return an opaque handle for `wait` /
    /// `try_wait` / `kill`.  Stdin/stdout/stderr are inherited from the
    /// parent — no piping.  Raises `IOError` on spawn failure.
    SubprocessSpawn         = 352,
    /// `subprocess.wait(handle: i64) -> i32` — block until the child
    /// identified by `handle` exits; return its exit code.  Raises
    /// `IOError` if the handle is invalid or already waited.
    SubprocessWait          = 353,
    /// `subprocess.try_wait(handle: i64) -> i32?` — non-blocking check.
    /// Returns the exit code if the child has exited, `none` if it's
    /// still running.  Raises `IOError` on invalid handle.
    SubprocessTryWait       = 354,
    /// `subprocess.kill(handle: i64) -> None` — force-terminate the
    /// child (SIGKILL on Unix, TerminateProcess on Windows).  Silently
    /// succeeds if the child has already exited.  Raises `IOError` on
    /// invalid handle.
    SubprocessKill          = 355,

    // ── 370–389: `pathlib` module (M23 P3a-A) ───────────────────────────
    // Object-oriented path API as a *flat function* surface — the Pythonic
    // `Path("foo") / "bar"` chaining isn't expressible in v0.2 (stdlib
    // classes are v0.3, same blocker that punted typed JsonValue in M20c
    // and ArgParser in M22 P2A).  Functions operate on `str`-typed paths
    // and round-trip cleanly with the M20a `path` and `os` modules.
    //
    // Functions duplicated from M20a's `path` (e.g. `join`, `parent` ==
    // `dirname`, `name` == `basename`) are kept for namespace coherence
    // — programs that `import pathlib` shouldn't also need `import path`
    // for the basics.
    //
    /// `pathlib.join(a: str, b: str) -> str` — concat two path components
    /// using the OS-native separator.  Alias of `path.join`.
    PathlibJoin       = 370,
    /// `pathlib.with_suffix(p: str, new_suffix: str) -> str` — replace the
    /// extension.  `with_suffix("a.txt", ".csv")` → `"a.csv"`.  If `p` has
    /// no extension, the suffix is appended.  `new_suffix` should include
    /// the leading dot (matches Python's pathlib).
    PathlibWithSuffix = 371,
    /// `pathlib.with_name(p: str, new_name: str) -> str` — replace the
    /// final path component (basename).  `with_name("a/b.txt", "c.csv")`
    /// → `"a/c.csv"`.
    PathlibWithName   = 372,
    /// `pathlib.parent(p: str) -> str` — parent directory (alias for
    /// ergonomics over M20a `path.dirname`).
    PathlibParent     = 373,
    /// `pathlib.name(p: str) -> str` — final path component (alias over
    /// `path.basename`).
    PathlibName       = 374,
    /// `pathlib.stem(p: str) -> str` — basename minus the last extension.
    /// `"a.txt"` → `"a"`; `"archive.tar.gz"` → `"archive.tar"` (Python's
    /// stem only strips the last suffix).
    PathlibStem       = 375,
    /// `pathlib.suffix(p: str) -> str` — the last extension including the
    /// leading dot.  `"a.txt"` → `".txt"`; `"README"` → `""`.
    PathlibSuffix     = 376,
    /// `pathlib.parts(p: str) -> List[str]` — split the path into
    /// components.  `"a/b/c"` → `["a", "b", "c"]`.  Drive letters and
    /// the root separator are included verbatim on each platform.
    PathlibParts      = 377,
    /// `pathlib.is_absolute(p: str) -> bool` — cross-platform absolute-
    /// path check using `std::path::Path::is_absolute`.
    PathlibIsAbsolute = 378,
    /// `pathlib.absolute(p: str) -> str` — make `p` absolute relative to
    /// the current working directory.  Does NOT resolve symlinks (that's
    /// `os.realpath` territory; v0.3).  Raises `IOError` if the current
    /// directory can't be queried.
    PathlibAbsolute   = 379,
    /// `pathlib.relative_to(p: str, base: str) -> str` — make `p` relative
    /// to `base`.  Raises `ValueError` if `p` is not a sub-path of `base`.
    PathlibRelativeTo = 380,
    /// `pathlib.read_text(p: str) -> str` — read the entire file as UTF-8.
    /// Convenience wrapper over `std::fs::read_to_string`.  Raises
    /// `IOError`.
    PathlibReadText   = 381,
    /// `pathlib.write_text(p: str, content: str) -> None` — atomic-ish
    /// write via `std::fs::write`.  Raises `IOError`.
    PathlibWriteText  = 382,
    /// `pathlib.read_lines(p: str) -> List[str]` — read the file then
    /// split on `\n`.  A trailing newline is stripped (so a file
    /// `"a\nb\n"` reads as `["a", "b"]`, matching Python's
    /// `Path.read_text().splitlines()` minus the dialect quirks).
    /// Raises `IOError`.
    PathlibReadLines  = 383,

    // ── 290–299: `base64` module (M22 P2B) ──────────────────────────────
    // Backed by the `base64` crate (engine API).  Strings round-trip
    // through UTF-8.  `decode` raises `ValueError` on malformed input;
    // `encode` cannot fail.  Two variants: standard (`+`/`/`, padded)
    // and URL-safe (`-`/`_`, no padding).
    /// `base64.encode(data: str) -> str` — encode UTF-8 input as base64.
    Base64Encode         = 290,
    /// `base64.decode(b64: str) -> str` — decode; UTF-8 check.
    Base64Decode         = 291,
    /// `base64.encode_url_safe(data: str) -> str`.
    Base64EncodeUrlSafe  = 292,
    /// `base64.decode_url_safe(b64: str) -> str`.
    Base64DecodeUrlSafe  = 293,

    // ── 300–309: `hashlib` module (M22 P2B) ─────────────────────────────
    // Backed by the `md-5` and `sha2` crates.  All entry points take a
    // single `str` argument and return the lowercase hex digest as a
    // `str`.  No streaming API in v0.2 — programs that need it can
    // concatenate first, or wait for v0.3.
    /// `hashlib.md5(data: str) -> str` — 32-char hex digest of MD5.
    HashlibMd5     = 300,
    /// `hashlib.sha1(data: str) -> str` — 40-char hex digest.
    HashlibSha1    = 301,
    /// `hashlib.sha256(data: str) -> str` — 64-char hex digest.
    HashlibSha256  = 302,
    /// `hashlib.sha512(data: str) -> str` — 128-char hex digest.
    HashlibSha512  = 303,
    /// `hashlib.hmac_sha256(key: str, data: str) -> str` — 64-char hex
    /// digest of HMAC-SHA256(key, data).  Backed by the `hmac` crate.
    HashlibHmacSha256 = 304,

    // ── 820–829: streaming `Hasher` (M35 P4-C) ─────────────────────────
    // Incremental hashing API.  The Hasher class is a `final` handle-
    // backed prelude class (seed_prelude registers it alongside
    // io.File / Thread / Channel); user code obtains an instance via
    // `hashlib.new(algorithm)`.  Internally the heap object carries an
    // i64 handle into `SharedVm.hashers`, a HashMap<i64, HasherState>
    // (one of Sha256/Sha512/Sha1/Md5 + the algorithm name string).
    //
    // hexdigest is *idempotent* — it clones the in-progress state and
    // finalises the clone, leaving the original free to accept further
    // `update` calls.  This is friendlier than CPython's "you can call
    // hexdigest multiple times, but anything after is on the same
    // state" semantics; documented in spec §9.X.
    //
    // 825-829 reserved for v0.4 (Hasher.copy, Hasher.digest_size,
    // SHAKE variants, etc.).
    /// Reserved for direct `Hasher(...)` construction — not currently
    /// reachable from the surface (users must call `hashlib.new`).
    HasherCtor       = 820,
    /// `hashlib.new(algorithm: str) -> Hasher` — fresh streaming hasher.
    /// Raises ValueError for unknown algorithm names.
    HashlibNew       = 821,
    /// `Hasher.update(data: str) -> None`.  Treats `data` as a byte
    /// buffer (each codepoint 0..=255 contributes one byte, matching
    /// the M22 struct / M27 gzip convention).
    HasherUpdate     = 822,
    /// `Hasher.hexdigest() -> str`.  Finalises a *clone* of the in-
    /// progress state so the original Hasher remains usable.
    HasherHexdigest  = 823,
    /// `Hasher.algorithm() -> str`.  Returns the canonical name passed
    /// to `hashlib.new`: "sha256" / "sha512" / "sha1" / "md5".
    HasherAlgorithm  = 824,

    // ── 830-879: M37 — `tabular` (DataFrame + sealed Column hierarchy) ──
    //
    // First Pandas-shaped data package for v0.3.  Native Rust impl (real
    // pandas can't import — see LANGUAGE_GUIDE.md §11.11).  Uses the
    // post-M36 `StdlibItemKind::Class` path — classes are registered
    // module-scoped on the `tabular` stdlib module (NOT in seed_prelude).
    //
    // Per-class layout (registered in resolver::seed_stdlib_modules):
    //   ColumnI64:      { values: List[i64],  nulls: List[bool], length: i64 }
    //   ColumnF64:      { values: List[f64],  nulls: List[bool], length: i64 }
    //   ColumnStr:      { values: List[str],  nulls: List[bool], length: i64 }
    //   ColumnBool:     { values: List[bool], nulls: List[bool], length: i64 }
    //   ColumnDateTime: { values: List[i64],  nulls: List[bool], length: i64 }
    //                   (values are epoch ms)
    //   DataFrame:      { names: List[str], columns: List[Column], nrows: i64 }
    //
    // NA semantics: per-column null mask — `nulls[i] == true` means the
    // i-th cell is NA.  No NaN sentinel, no `T?` per cell.  Comparison
    // ops propagate null cells by OR-ing the input null masks.
    //
    // ── Column construction helpers (module-level) ──
    /// `tabular.col_i64(values: List[i64], nulls: List[bool]) -> ColumnI64`.
    M37TabColI64           = 830,
    /// `tabular.col_i64_simple(values: List[i64]) -> ColumnI64`.
    M37TabColI64Simple     = 831,
    /// `tabular.col_f64(values: List[f64], nulls: List[bool]) -> ColumnF64`.
    M37TabColF64           = 832,
    /// `tabular.col_f64_simple(values: List[f64]) -> ColumnF64`.
    M37TabColF64Simple     = 833,
    /// `tabular.col_str(values: List[str], nulls: List[bool]) -> ColumnStr`.
    M37TabColStr           = 834,
    /// `tabular.col_str_simple(values: List[str]) -> ColumnStr`.
    M37TabColStrSimple     = 835,
    /// `tabular.col_bool(values: List[bool], nulls: List[bool]) -> ColumnBool`.
    M37TabColBool          = 836,
    /// `tabular.col_bool_simple(values: List[bool]) -> ColumnBool`.
    M37TabColBoolSimple    = 837,
    /// `tabular.col_datetime(values: List[i64], nulls: List[bool]) -> ColumnDateTime`.
    M37TabColDateTime      = 838,
    /// `tabular.from_columns(names: List[str], cols: List[Column]) -> DataFrame`.
    M37TabFromColumns      = 839,
    // ── Column shared methods (one slot per (column type, method)) ──
    //  Per-column inspection (length / dtype / is_null / null_count).
    /// `Column*.length(self) -> i64` — shared by all column types.
    M37TabColLength        = 840,
    /// `Column*.dtype(self) -> str` — "i64"/"f64"/"str"/"bool"/"datetime".
    M37TabColDtype         = 841,
    /// `Column*.is_null(self, i: i64) -> bool` — bounds-checked.
    M37TabColIsNull        = 842,
    /// `Column*.null_count(self) -> i64` — count of true entries in nulls.
    M37TabColNullCount     = 843,
    /// `ColumnI64.get(self, i: i64) -> i64?` — none if null.
    M37TabColI64Get        = 844,
    /// `ColumnF64.get(self, i: i64) -> f64?`.
    M37TabColF64Get        = 845,
    /// `ColumnStr.get(self, i: i64) -> str?`.
    M37TabColStrGet        = 846,
    /// `ColumnBool.get(self, i: i64) -> bool?`.
    M37TabColBoolGet       = 847,
    /// `ColumnDateTime.get_ms(self, i: i64) -> i64?`.
    M37TabColDateTimeGetMs = 848,
    // ── DataFrame inspection ──
    /// `DataFrame.length(self) -> i64` — nrows.
    M37TabDfLength         = 849,
    /// `DataFrame.ncols(self) -> i64`.
    M37TabDfNcols          = 850,
    /// `DataFrame.columns(self) -> List[str]`.
    M37TabDfColumns        = 851,
    /// `DataFrame.dtypes(self) -> List[str]`.
    M37TabDfDtypes         = 852,
    /// `DataFrame.has_column(self, name: str) -> bool`.
    M37TabDfHasColumn      = 853,
    /// `DataFrame.show(self, n: i64) -> str` — ASCII table; n=-1 for all.
    M37TabDfShow           = 854,
    // ── Phase B: I/O (read_csv / write_csv / from_sql / from_rows) ──
    /// `tabular.read_csv(path: str, schema: List[Tuple[str,str]]) -> DataFrame`.
    M37TabReadCsv          = 855,
    /// `tabular.write_csv(path: str, df: DataFrame) -> None`.
    M37TabWriteCsv         = 856,
    /// `tabular.from_sql(cur: Cursor, schema: List[Tuple[str,str]]) -> DataFrame`.
    M37TabFromSql          = 857,
    /// `tabular.from_rows(rows: List[List[str]], schema: List[Tuple[str,str]]) -> DataFrame`.
    M37TabFromRows         = 858,
    // ── Phase C: per-Column comparison methods → ColumnBool masks ──
    /// `ColumnI64.eq(self, x: i64) -> ColumnBool`.
    M37TabColI64Eq         = 859,
    /// `ColumnI64.gt(self, x: i64) -> ColumnBool`.
    M37TabColI64Gt         = 860,
    /// `ColumnI64.lt(self, x: i64) -> ColumnBool`.
    M37TabColI64Lt         = 861,
    /// `ColumnF64.eq(self, x: f64) -> ColumnBool`.
    M37TabColF64Eq         = 862,
    /// `ColumnF64.gt(self, x: f64) -> ColumnBool`.
    M37TabColF64Gt         = 863,
    /// `ColumnF64.lt(self, x: f64) -> ColumnBool`.
    M37TabColF64Lt         = 864,
    /// `ColumnStr.eq(self, x: str) -> ColumnBool`.
    M37TabColStrEq         = 865,
    /// `ColumnStr.contains(self, needle: str) -> ColumnBool`.
    M37TabColStrContains   = 866,
    /// `ColumnBool.and_(self, other: ColumnBool) -> ColumnBool`.
    M37TabMaskAnd          = 867,
    /// `ColumnBool.or_(self, other: ColumnBool) -> ColumnBool`.
    M37TabMaskOr           = 868,
    /// `ColumnBool.not_(self) -> ColumnBool`.
    M37TabMaskNot          = 869,
    /// `ColumnBool.count_true(self) -> i64`.
    M37TabMaskCountTrue    = 870,
    // ── Phase C: DataFrame filter / projection / row ops ──
    /// `DataFrame.filter(self, mask: ColumnBool) -> DataFrame`.
    M37TabDfFilter         = 871,
    /// `DataFrame.select(self, cols: List[str]) -> DataFrame`.
    M37TabDfSelect         = 872,
    /// `DataFrame.drop(self, cols: List[str]) -> DataFrame`.
    M37TabDfDrop           = 873,
    /// `DataFrame.head(self, n: i64) -> DataFrame`.
    M37TabDfHead           = 874,
    /// `DataFrame.tail(self, n: i64) -> DataFrame`.
    M37TabDfTail           = 875,
    /// `DataFrame.row(self, i: i64) -> List[str]`.
    M37TabDfRow            = 876,
    // ── Phase D: stable sort_by ──
    /// `DataFrame.sort_by(self, col_name: str, ascending: bool) -> DataFrame`.
    M37TabDfSortBy         = 877,
    // 878-879 reserved for v0.4 follow-ups (rename / between / etc.)

    // ── 250–289: M22 P2A (argparse + collections + csv) ─────────────────
    // Phase 2 starts here.  P2A's job is to bring three high-ROI stdlib
    // modules online on top of the M19 stdlib-module-table:
    //   - argparse — declarative CLI arg parsing.  Currently every CLI
    //     tool (`echo.spy`, `sum_args.spy`, `minigrep.spy`) hand-parses
    //     `sys.argv`; this is the ergonomic upgrade.
    //   - collections — Counter / deque.  M10's `wordcount.spy` rolled
    //     a hand-built freq table; `collections.counter_*` replaces it.
    //   - csv — parser/writer.  M10's `csv_aggregate.spy` had a one-pass
    //     scanner; this module packages it as named natives.
    //
    // Storage choices (documented in detail in spec §9.15-§9.17):
    //   - argparse uses `Dict[str, str]` as both parser-handle and args.
    //     A v0.3 typed `ArgParser` / `Args` class would be cleaner but
    //     needs stdlib-class registration that we don't have yet.
    //   - Counter is `Dict[str, i64]` (typed alias).  No new heap shape.
    //   - Deque is `List[i64]` (typed alias).  `pop_front` is O(n) until
    //     v0.3 ships a real deque.
    /// `argparse.new(prog: str) -> Dict[str, str]` — fresh parser handle.
    ArgparseNew         = 250,
    /// `argparse.add_flag(p, name: str, default: bool) -> None`.
    ArgparseAddFlag     = 251,
    /// `argparse.add_arg(p, name: str) -> None` — positional argument.
    ArgparseAddArg      = 252,
    /// `argparse.add_opt(p, name: str, default: str) -> None` — `--key VAL`.
    ArgparseAddOpt      = 253,
    /// `argparse.parse(p, argv: List[str]) -> Dict[str, str]`.  Raises
    /// `ValueError` on unknown flag/opt, missing positional, or option-
    /// without-value.  `argv[0]` is treated as program name and skipped.
    ArgparseParse       = 254,
    /// `argparse.get_flag(a, name) -> bool` — read parsed flag.  Returns
    /// false if not present.
    ArgparseGetFlag     = 255,
    /// `argparse.get_arg(a, name) -> str` — read parsed positional.
    ArgparseGetArg      = 256,
    /// `argparse.get_opt(a, name) -> str` — read parsed option value.
    ArgparseGetOpt      = 257,
    /// `argparse.help_text(p) -> str` — render a human-readable usage
    /// line + per-arg block (`USAGE: <prog> [flags] <positionals>`...).
    ArgparseHelpText    = 258,
    /// `argparse.help_requested(argv) -> bool` — true iff `argv` contains
    /// `-h` or `--help`.  Pair with `help_text` + `sys.exit(0)`.
    ArgparseHelpRequested = 259,
    /// `collections.counter_new() -> Dict[str, i64]`.
    CollCounterNew      = 265,
    /// `collections.counter_increment(c, key) -> None` — `c[key] += 1`.
    CollCounterIncrement = 266,
    /// `collections.counter_add(c, key, n) -> None` — `c[key] += n`.
    CollCounterAdd      = 267,
    /// `collections.counter_get(c, key) -> i64` — 0 if absent.
    CollCounterGet      = 268,
    /// `collections.counter_top_keys(c, n: i32) -> List[str]` — top-N
    /// keys by descending count, ties broken alphabetically.
    CollCounterTopKeys  = 269,
    /// `collections.deque_new() -> List[i64]` — fresh empty deque.
    CollDequeNew        = 270,
    /// `collections.deque_push_back(d, v: i64) -> None`.
    CollDequePushBack   = 271,
    /// `collections.deque_pop_front(d) -> i64`.  Raises IndexError on
    /// empty.  O(n) shift in v0.2 — a real deque is v0.3 work.
    CollDequePopFront   = 272,
    /// `collections.deque_len(d) -> i32`.
    CollDequeLen        = 273,
    /// `collections.deque_is_empty(d) -> bool`.
    CollDequeIsEmpty    = 274,
    /// `csv.parse_line(line: str) -> List[str]` — parse one CSV line,
    /// honouring quoted fields and `""` escapes.  Does not handle
    /// embedded newlines (use `parse` for multi-line).
    CsvParseLine        = 275,
    /// `csv.parse(text: str) -> List[List[str]]` — parse multi-line
    /// CSV; quoted fields may contain newlines.
    CsvParse            = 276,
    /// `csv.read_file(path: str) -> List[List[str]]`.  Raises IOError.
    CsvReadFile         = 277,
    /// `csv.write_file(path: str, rows: List[List[str]]) -> None`.
    CsvWriteFile        = 278,
    /// `csv.escape(field: str) -> str` — quote if needed.
    CsvEscape           = 279,
    /// `csv.format_row(row: List[str]) -> str` — comma-joined, escaped.
    CsvFormatRow        = 280,

    // ── 390–419: `datetime` module (M23 P3a-B) ──────────────────────────
    // Calendar arithmetic + timezone-aware time, layered on top of the
    // M20b `time` epoch primitives.  Both `DateTime` and `Duration` are
    // plain `i64` in v0.2 (seconds since unix epoch / seconds span).  All
    // calendar conversions reuse M20b's `civil_from_days` (and its inverse
    // `days_from_civil`) — Howard Hinnant's public-domain algorithm.  No
    // `chrono` dep; we stay vm-side-only.
    /// `datetime.now() -> i64` — current unix epoch seconds (UTC).
    DateTimeNow                 = 390,
    /// `datetime.from_unix(secs: i64) -> i64` — identity assertion; any
    /// `i64` is a valid DateTime by construction.
    DateTimeFromUnix            = 391,
    /// `datetime.from_ymd(year: i32, month: i32, day: i32) -> i64`.
    /// Builds a DateTime from civil date at UTC midnight.  Raises
    /// ValueError on invalid (year out of [-10000, 10000]; month not in
    /// 1..=12; day out-of-range for month including leap years).
    DateTimeFromYmd             = 392,
    /// `datetime.from_ymd_hms(y, m, d, hour, minute, second) -> i64`.
    /// Same validation + 0..=23 / 0..=59 / 0..=60 (leap second allowed).
    DateTimeFromYmdHms          = 393,
    /// `datetime.year(dt: i64) -> i32` — UTC year (may be negative).
    DateTimeYear                = 394,
    /// `datetime.month(dt: i64) -> i32` — UTC month, 1..=12.
    DateTimeMonth               = 395,
    /// `datetime.day(dt: i64) -> i32` — UTC day-of-month, 1..=31.
    DateTimeDay                 = 396,
    /// `datetime.hour(dt: i64) -> i32` — UTC hour, 0..=23.
    DateTimeHour                = 397,
    /// `datetime.minute(dt: i64) -> i32` — 0..=59.
    DateTimeMinute              = 398,
    /// `datetime.second(dt: i64) -> i32` — 0..=59 (or 60 for leap second).
    DateTimeSecond              = 399,
    /// `datetime.weekday(dt: i64) -> i32` — 0..=6, Monday=0 (ISO).
    DateTimeWeekday             = 400,
    /// `datetime.ymd(dt: i64) -> Tuple[i32, i32, i32]` — packed via
    /// `alloc_tuple_obj` (the same path `path.splitext` uses).
    DateTimeYmd                 = 401,
    /// `datetime.add_seconds(dt: i64, secs: i64) -> i64`.
    DateTimeAddSeconds          = 402,
    /// `datetime.add_days(dt: i64, days: i64) -> i64`.
    DateTimeAddDays             = 403,
    /// `datetime.diff_seconds(a: i64, b: i64) -> i64` — `a - b`.
    DateTimeDiffSeconds         = 404,
    /// `datetime.diff_days(a: i64, b: i64) -> i64` — floor of `(a-b)/86400`.
    DateTimeDiffDays            = 405,
    /// `datetime.to_iso(dt: i64) -> str` — `"YYYY-MM-DDTHH:MM:SSZ"`.
    DateTimeToIso               = 406,
    /// `datetime.to_date_str(dt: i64) -> str` — `"YYYY-MM-DD"`.
    DateTimeToDateStr           = 407,
    /// `datetime.to_time_str(dt: i64) -> str` — `"HH:MM:SS"`.
    DateTimeToTimeStr           = 408,
    /// `datetime.from_iso(s: str) -> i64`.  Accepts `"YYYY-MM-DDTHH:MM:SSZ"`,
    /// `"YYYY-MM-DDTHH:MM:SS+00:00"`, and `"YYYY-MM-DD"` (UTC midnight).
    /// Raises ValueError on bad input.
    DateTimeFromIso             = 409,
    /// `datetime.from_date_str(s: str) -> i64` — `"YYYY-MM-DD"` → UTC midnight.
    DateTimeFromDateStr         = 410,
    /// `datetime.local_offset_minutes() -> i32` — process-local TZ offset
    /// from UTC in minutes (e.g. -480 for PST).  Captures "what is the
    /// offset now"; doesn't depend on a specific DateTime.
    DateTimeLocalOffsetMinutes  = 411,
    // 412-419 reserved for v0.3 (timezone-named DateTimes, fractional secs,
    // strftime/strptime, etc.).

    // ── 420-439: M23 P3a-C — `threading` + `queue` ─────────────────────
    // Synchronisation primitives that extend M6's Thread/Channel surface,
    // and a generic min-priority-queue.  Lock + Semaphore live in slot
    // tables on `SharedVm` (`locks`, `semaphores`); PriorityQueue lives
    // in `priority_queues`.  Same opaque-i64-handle shape as the existing
    // channels/dicts/files tables.
    //
    // Items 422 (lock_release) is non-recursive — Python's `threading.Lock`
    // semantics, not `RLock` — so acquiring a held lock from the same
    // thread DEADLOCKS.  `RLock`, `Event`, `Condition`, `Barrier` are v0.3.
    /// `threading.lock_new() -> i64` — allocate a fresh unheld lock.
    ThreadingLockNew         = 420,
    /// `threading.lock_acquire(handle: i64) -> None` — block until acquired.
    ThreadingLockAcquire     = 421,
    /// `threading.lock_release(handle: i64) -> None` — raises RuntimeError
    /// if the lock isn't currently held.
    ThreadingLockRelease     = 422,
    /// `threading.lock_try_acquire(handle: i64) -> bool` — non-blocking;
    /// returns true if the lock was obtained.
    ThreadingLockTryAcquire  = 423,
    /// `threading.semaphore_new(initial: i32) -> i64` — N initial permits.
    ThreadingSemaphoreNew         = 424,
    /// `threading.semaphore_acquire(handle: i64) -> None` — block until a
    /// permit is available.
    ThreadingSemaphoreAcquire     = 425,
    /// `threading.semaphore_release(handle: i64) -> None` — increment and
    /// wake one waiter.
    ThreadingSemaphoreRelease     = 426,
    /// `threading.semaphore_try_acquire(handle: i64) -> bool` — non-blocking.
    ThreadingSemaphoreTryAcquire  = 427,
    /// `queue.pq_new_i64() -> i64` — fresh min-priority queue with i64 items.
    QueuePqNewI64       = 428,
    /// `queue.pq_push_i64(handle, priority: f64, item: i64) -> None`.
    QueuePqPushI64      = 429,
    /// `queue.pq_pop_min_i64(handle) -> Tuple[f64, i64]` — lowest priority.
    /// Raises IndexError on empty.
    QueuePqPopMinI64    = 430,
    /// `queue.pq_peek_min_i64(handle) -> Tuple[f64, i64]` — non-destructive.
    QueuePqPeekMinI64   = 431,
    /// `queue.pq_new_str() -> i64` — fresh PQ with str items.
    QueuePqNewStr       = 432,
    /// `queue.pq_push_str(handle, priority: f64, item: str) -> None`.
    QueuePqPushStr      = 433,
    /// `queue.pq_pop_min_str(handle) -> Tuple[f64, str]`.
    QueuePqPopMinStr    = 434,
    /// `queue.pq_peek_min_str(handle) -> Tuple[f64, str]`.
    QueuePqPeekMinStr   = 435,
    /// `queue.pq_len(handle: i64) -> i32` — type-erased (works for both
    /// i64 and str queues; the handle alone is enough since item payloads
    /// are uniformly u64 slots).
    QueuePqLen          = 436,
    /// `queue.pq_is_empty(handle: i64) -> bool` — type-erased.
    QueuePqIsEmpty      = 437,
    // 438-439 reserved for v0.3 (e.g. pq_clear, pq_drain).

    // ── 550-569: `logging` module (M27 P3c-E) ───────────────────────────
    // Application logging — flat global-logger surface for v0.2 (Python's
    // class-heavy Logger/Handler/Formatter hierarchy is v0.3 work, blocked
    // on stdlib-class registration).  Threshold + optional file sink live
    // as per-instance state on `SharedVm` (`log_level` AtomicI32 +
    // `log_file` Mutex<Option<File>>).  Format is fixed
    // `"YYYY-MM-DDTHH:MM:SSZ LEVEL message\n"` — matches CPython's default
    // `%(asctime)s %(levelname)s %(message)s`.  Level constants
    // (10/20/30/40/50) match CPython exactly so the API is interchangeable.
    /// `logging.basic_config(level: str) -> None` — initialise global logger
    /// to write to stderr.  Idempotent; calling again resets the level and
    /// drops any prior file sink.  `level` is one of "DEBUG", "INFO",
    /// "WARNING", "ERROR", "CRITICAL".  Raises `ValueError` on unknown level.
    LoggingBasicConfig          = 550,
    /// `logging.basic_config_to_file(level: str, filename: str) -> None` —
    /// initialise to write to `filename` instead of stderr.  Opens the file
    /// in append mode; raises `IOError` on open failure.
    LoggingBasicConfigToFile    = 551,
    /// `logging.set_level(level: str) -> None` — change current threshold.
    LoggingSetLevel             = 552,
    /// `logging.get_level() -> str` — current level name.
    LoggingGetLevel             = 553,
    /// `logging.debug(msg: str) -> None`.
    LoggingDebug                = 554,
    /// `logging.info(msg: str) -> None`.
    LoggingInfo                 = 555,
    /// `logging.warning(msg: str) -> None`.
    LoggingWarning              = 556,
    /// `logging.error(msg: str) -> None`.
    LoggingError                = 557,
    /// `logging.critical(msg: str) -> None`.
    LoggingCritical             = 558,
    /// `logging.log(level: str, msg: str) -> None` — generic emit.
    LoggingLog                  = 559,
    /// `logging.is_enabled_for(level: str) -> bool` — gate expensive
    /// message-building code.
    LoggingIsEnabledFor         = 560,
    // 561-569 reserved for v0.3 (named loggers, structured records,
    // custom formatters, rotating-file handlers, etc.).

    // ── 570-599: `socket` module (M28 P3b-A) ────────────────────────────
    // Raw TCP/UDP networking. Backed by `std::net` (no new crate dep);
    // every socket lives in one of three SharedVm slot tables
    // (`tcp_streams`, `tcp_listeners`, `udp_sockets`) keyed by i64 handle.
    // Slot 0 reserved per the usual convention so "handle == 0" means
    // "no socket". IPv6 is supported transparently — `TcpStream::connect`
    // accepts both v4 and v6 addresses; the only place IP family shows up
    // is `gethostbyname`, which prefers the first IPv4 address (spec
    // §9.40). Bytes ride on `str` with each codepoint a byte 0..255 (the
    // same str-as-byte-buffer convention M22 `struct` and M27 `gzip`/`zip`
    // already use). See spec §9.40.
    /// `socket.connect_tcp(host: str, port: i32) -> i64` — open a TCP
    /// connection to `host:port` and return its handle. Raises IOError
    /// on resolution / connect failure.
    SocketConnectTcp        = 570,
    /// `socket.send(handle: i64, data: str) -> i32` — write the byte
    /// buffer; return the number of bytes actually sent (may be less than
    /// `len(data)` on partial writes; callers loop or use higher-level
    /// helpers).
    SocketSend              = 571,
    /// `socket.recv(handle: i64, max_bytes: i32) -> str` — read up to
    /// `max_bytes`; returns "" on EOF.
    SocketRecv              = 572,
    /// `socket.recv_exact(handle: i64, n: i32) -> str` — read exactly
    /// `n` bytes or raise IOError on EOF / short read.
    SocketRecvExact         = 573,
    /// `socket.close(handle: i64) -> None` — shut down + drop the stream.
    /// Calls `flush()` first to ensure pending bytes hit the wire (the
    /// Windows close-drops-pending-bytes gotcha called out in §9.40).
    SocketClose             = 574,
    /// `socket.set_timeout_secs(handle: i64, secs: f64) -> None` — apply
    /// the same duration to both read and write. Use `0.0` to clear
    /// (block forever).
    SocketSetTimeoutSecs    = 575,
    /// `socket.peer_addr(handle: i64) -> str` — "ip:port" string for the
    /// remote endpoint.
    SocketPeerAddr          = 576,
    /// `socket.local_addr(handle: i64) -> str` — "ip:port" for the local
    /// endpoint.
    SocketLocalAddr         = 577,
    /// `socket.listen_tcp(host: str, port: i32, backlog: i32) -> i64` —
    /// bind a TCP listener and return its handle. `backlog` currently
    /// only informs the socket option; Rust's `TcpListener::bind` doesn't
    /// expose `listen()`'s backlog directly, so it's accepted-but-ignored
    /// on Windows (documented in spec).
    SocketListenTcp         = 578,
    /// `socket.accept(listener: i64) -> Tuple[i64, str]` — block until a
    /// peer connects; return `(new_stream_handle, peer_addr_string)`.
    SocketAccept            = 579,
    /// `socket.close_listener(listener: i64) -> None`.
    SocketCloseListener     = 580,
    /// `socket.udp_socket() -> i64` — open a UDP socket bound to
    /// "0.0.0.0:0" (OS-assigned port).
    SocketUdpSocket         = 581,
    /// `socket.udp_bind(host: str, port: i32) -> i64` — bind a UDP
    /// socket to the given endpoint.
    SocketUdpBind           = 582,
    /// `socket.udp_send_to(handle: i64, data: str, host: str, port: i32)
    /// -> i32` — single datagram send; returns the bytes sent.
    SocketUdpSendTo         = 583,
    /// `socket.udp_recv_from(handle: i64, max_bytes: i32) -> Tuple[str,
    /// str, i32]` — receive one datagram; returns `(data, sender_host,
    /// sender_port)`.
    SocketUdpRecvFrom       = 584,
    /// `socket.udp_close(handle: i64) -> None`.
    SocketUdpClose          = 585,
    /// `socket.gethostbyname(host: str) -> str` — DNS lookup; returns the
    /// first IPv4 address (or first IPv6 if no v4) as a printable string.
    SocketGethostbyname     = 586,
    /// `socket.resolve(host: str, port: i32) -> List[str]` — return every
    /// "ip:port" the resolver produces for the host.
    SocketResolve           = 587,
    /// `socket.gethostname() -> str` — local host name.
    SocketGethostname       = 588,
    // 589-599 reserved for v0.3 (set_nodelay, set_keepalive, shutdown,
    // unix-domain sockets, raw-socket support, etc.).

    // ── 440-469: `sqlite3` module (M23 P3a-D) ───────────────────────────
    // Backed by the `rusqlite` crate (with the `bundled` feature so
    // libsqlite3.c is statically linked into the VM binary — no system
    // SQLite required).  Connections are modelled as i64 handles into a
    // `SharedVm.sqlite_connections` table; user code passes the handle
    // through every API call.  All cell values are stringified on
    // output: INTEGER -> "42", REAL -> "3.14", TEXT -> the text, NULL ->
    // the empty string, BLOB -> base16 of the bytes (rare in v0.2
    // because programs can't construct BLOBs without a real `bytes`
    // type).  Parameter binding always treats arguments as TEXT; SQLite
    // applies its usual type-coercion rules when comparing against
    // INTEGER / REAL columns.  See spec §9.24.
    //
    // The "all results as str" simplification covers ~every config-store
    // / cache / queue-table use case; programs that need typed result
    // columns (BLOBs in particular) wait for v0.3's `bytes` type.
    /// `sqlite3.connect(path: str) -> i64` — open or create the DB file.
    /// Pass `":memory:"` for an in-memory DB.  Raises IOError on failure.
    Sqlite3Connect          = 440,
    /// `sqlite3.close(conn: i64) -> None` — release the connection.
    Sqlite3Close            = 441,
    /// `sqlite3.execute(conn: i64, sql: str) -> None` — run a statement
    /// that returns no rows.  Raises ValueError on SQL error.
    Sqlite3Execute          = 442,
    /// `sqlite3.execute_params(conn: i64, sql: str, params: List[str])
    /// -> None` — same as `execute` but with `?` placeholders bound from
    /// `params` (all bound as TEXT in v0.2).
    Sqlite3ExecuteParams    = 443,
    /// `sqlite3.query(conn: i64, sql: str) -> List[List[str]]` — run a
    /// SELECT and return all rows.  Each cell is stringified.
    Sqlite3Query            = 444,
    /// `sqlite3.query_params(conn: i64, sql: str, params: List[str]) ->
    /// List[List[str]]` — same as `query` with bound parameters.
    Sqlite3QueryParams      = 445,
    /// `sqlite3.last_insert_rowid(conn: i64) -> i64`.
    Sqlite3LastInsertRowid  = 446,
    /// `sqlite3.changes(conn: i64) -> i32` — rows affected by the most
    /// recent INSERT/UPDATE/DELETE.
    Sqlite3Changes          = 447,
    /// `sqlite3.column_names(conn: i64, sql: str) -> List[str]` — prepare
    /// the statement and return its column-name vector (no rows fetched).
    Sqlite3ColumnNames      = 448,

    // ── 450-479: `shutil` + `tempfile` modules (M27 P3c-A) ──────────────
    // High-level filesystem operations (`shutil`) and temp file/dir
    // creation (`tempfile`).  Both are pure `std::fs` / `std::path`
    // surface (no new FFI) — `tempfile` adds the `tempfile` crate as the
    // single new dependency for atomic temp-dir / temp-file creation
    // with the platform-correct permissions (700 on Unix; ACL'd to the
    // current user on Windows).  See spec §9.30 (shutil) and §9.31
    // (tempfile).
    //
    // `shutil` ID layout (450-459 used, 460-469 reserved):
    //   450 copy           — single-file copy
    //   451 copytree       — recursive directory copy
    //   452 move           — rename with cross-filesystem fallback
    //   453 rmtree         — recursive directory removal (closes M24-D)
    //   454 which          — PATH lookup with Windows .exe extension dance
    //   455 disk_usage     — (total, used, free) bytes for a mount-point
    // `tempfile` ID layout (470-472 used, 473-479 reserved):
    //   470 mkdtemp        — create temp directory, return absolute path
    //   471 mkstemp        — create temp file, return absolute path
    //   472 gettempdir     — system temp directory (env-var aware)
    /// `shutil.copy(src: str, dst: str) -> None` — file-content copy.
    /// Preserves bytes but not permission bits (matches `shutil.copyfile`,
    /// which is what 99% of Python `shutil.copy` callers actually want).
    /// Raises `IOError` on filesystem failure.
    ShutilCopy              = 450,
    /// `shutil.copytree(src: str, dst: str) -> None` — recursive directory
    /// copy.  `dst` must NOT already exist (matches CPython 3.7+).
    ShutilCopytree          = 451,
    /// `shutil.move(src: str, dst: str) -> None` — rename, with copy+delete
    /// fallback across filesystems (matches Python `shutil.move`).
    ShutilMove              = 452,
    /// `shutil.rmtree(path: str) -> None` — recursive directory removal.
    /// Closes the v0.2 gap M24-D documented (no recursive `os.removedirs`).
    ShutilRmtree            = 453,
    /// `shutil.which(cmd: str) -> str?` — PATH lookup; returns None if the
    /// command isn't found.  On Windows, also tries `.exe`/`.bat`/`.cmd`
    /// extensions when the input has none (matches CPython 3.x).
    ShutilWhich             = 454,
    /// `shutil.disk_usage(path: str) -> Tuple[i64, i64, i64]` — disk
    /// space stats for the volume containing `path`.  Tuple slots:
    /// (total, used, free) in bytes.
    ShutilDiskUsage         = 455,
    // 456-469 reserved (chmod, chown, copytree-ignore, copy2 with metadata,
    // make_archive, unpack_archive, get_terminal_size in v0.3).

    /// `tempfile.mkdtemp(prefix: str = "tmp") -> str` — create a temp
    /// directory under the system temp root and return its absolute
    /// path.  The directory is NOT auto-cleaned (caller's responsibility,
    /// usually via `shutil.rmtree`).
    TempfileMkdtemp         = 470,
    /// `tempfile.mkstemp(prefix: str = "tmp", suffix: str = "") -> str`
    /// — create a temp file and return its absolute path.  The file is
    /// created (zero bytes) and closed; caller re-opens via `open(...)`.
    TempfileMkstemp         = 471,
    /// `tempfile.gettempdir() -> str` — system temp directory path
    /// (`$TMPDIR` / `$TEMP` / `/tmp` etc., per `std::env::temp_dir`).
    TempfileGettempdir      = 472,
    // 473-479 reserved (NamedTemporaryFile, SpooledTemporaryFile,
    // TemporaryDirectory context-manager wrapper — all need v0.3
    // stdlib classes).
    // ── 480-499: `glob` + `fnmatch` modules (M27 P3c-B) ─────────────────
    // Unix-shell-style wildcard expansion (`glob`) and single-string
    // wildcard matching (`fnmatch`).  Backed by the `glob` crate (which
    // provides both pattern matching and directory walking) so the v0.2
    // surface is "ship a thin native handler per spec function" — no
    // hand-rolled FSM.  `fnmatch.translate` converts a shell-glob to a
    // regex string so callers can compose with `re` (M20c).
    //
    // Case-sensitivity follows CPython: `fnmatch.fnmatch` is
    // case-INsensitive on Windows / sensitive on Unix; `fnmatchcase` is
    // always sensitive.  `glob.glob` mirrors the platform's filesystem
    // case-sensitivity (Windows: case-insensitive, Unix: sensitive).
    /// `glob.glob(pattern: str) -> List[str]` — non-recursive paths
    /// matching a `*`/`?`/`[abc]` pattern, sorted ascending.
    GlobGlob              = 480,
    /// `glob.recursive(pattern: str) -> List[str]` — like `glob.glob`
    /// but `**` matches arbitrarily-deep subdirectories.
    GlobRecursive         = 481,
    /// `glob.escape(s: str) -> str` — escape glob metacharacters in `s`
    /// so it matches literally (`[` becomes `[[]`, `*` → `[*]`, etc.).
    GlobEscape            = 482,
    /// `fnmatch.fnmatch(name: str, pattern: str) -> bool` — shell-glob
    /// match.  Case-INsensitive on Windows; case-sensitive on Unix.
    FnmatchFnmatch        = 483,
    /// `fnmatch.fnmatchcase(name: str, pattern: str) -> bool` — always
    /// case-sensitive, regardless of platform.
    FnmatchFnmatchcase    = 484,
    /// `fnmatch.filter(names: List[str], pattern: str) -> List[str]` —
    /// keep only names matching `pattern`, preserving input order.
    /// Case sensitivity follows `fnmatch.fnmatch` (i.e. platform-dependent).
    FnmatchFilter         = 485,
    /// `fnmatch.translate(pattern: str) -> str` — convert a shell-glob
    /// pattern into an anchored regex string suitable for `re.match`.
    FnmatchTranslate      = 486,
    // 487-499 reserved for v0.3 (e.g. recursive variants of fnmatch,
    // `glob.iglob` lazy iterator, `glob.has_magic`).
    // ── 500–519: compression modules (M27 P3c-C) ────────────────────────
    // Three stdlib modules wrap the `flate2` and `bzip2` crates.  All
    // entry points use the str-as-byte-buffer convention (M22 P2D
    // struct): each codepoint 0..=255 is one byte, so a binary blob
    // round-trips losslessly through the str type without needing v0.3
    // `bytes`.  Compression failures and malformed inputs surface as
    // `ValueError`.
    /// `gzip.compress(data: str) -> str` — RFC 1952 gzip, default level 6.
    GzipCompress         = 500,
    /// `gzip.compress_level(data: str, level: i32) -> str` — level 0..9.
    GzipCompressLevel    = 501,
    /// `gzip.decompress(data: str) -> str` — gzip-decompress; ValueError on bad data.
    GzipDecompress       = 502,
    /// `zlib.compress(data: str) -> str` — RFC 1950 zlib, default level 6.
    ZlibCompress         = 503,
    /// `zlib.compress_level(data: str, level: i32) -> str` — level 0..9.
    ZlibCompressLevel    = 504,
    /// `zlib.decompress(data: str) -> str` — zlib-decompress; ValueError on bad data.
    ZlibDecompress       = 505,
    /// `zlib.crc32(data: str) -> i64` — CRC-32 checksum (returned as i64 to
    /// avoid u32/i32 sign ambiguity; always in 0..=0xFFFF_FFFF).
    ZlibCrc32            = 506,
    /// `zlib.adler32(data: str) -> i64` — Adler-32 checksum.
    ZlibAdler32          = 507,
    /// `bz2.compress(data: str) -> str` — bzip2 format, default level 6.
    Bz2Compress          = 508,
    /// `bz2.compress_level(data: str, level: i32) -> str` — level 1..9.
    Bz2CompressLevel     = 509,
    /// `bz2.decompress(data: str) -> str` — bzip2-decompress; ValueError on bad data.
    Bz2Decompress        = 510,
    // 511-519 reserved for v0.3 streaming hashers (gzip.open / etc.)
    // and lzma/xz support if a real-world program needs them.
    // ── 520-549: `zipfile` + `tarfile` modules (M27 P3c-D) ──────────────
    // Both ride the same opaque-handle / slot-table pattern as sqlite3
    // (M23 P3a-D).  Archive handles are i64 indexes into per-process
    // `SharedVm.zip_readers` / `zip_writers` / `tar_readers` /
    // `tar_writers` tables.  Entry contents round-trip through str
    // (str-as-byte-buffer convention — codepoints 0..255 inclusive map
    // 1:1 to bytes; v0.2 has no `bytes` type).  See spec §9.30 / §9.31.

    // -- zipfile (520-529) ---------------------------------------------
    /// `zipfile.open_read(path: str) -> i64` — open existing .zip for
    /// reading.  Raises IOError on missing file / bad archive.
    ZipfileOpenRead     = 520,
    /// `zipfile.open_write(path: str) -> i64` — create new .zip for
    /// writing (existing file is truncated).  Raises IOError on
    /// permission failure.
    ZipfileOpenWrite    = 521,
    /// `zipfile.names(handle: i64) -> List[str]` — list entry names in a
    /// read-mode archive.  ValueError if `handle` is for a write handle.
    ZipfileNames        = 522,
    /// `zipfile.read(handle: i64, name: str) -> str` — read entry as
    /// bytes-packed-into-str.  Raises ValueError if entry missing.
    ZipfileRead         = 523,
    /// `zipfile.write(handle: i64, name: str, data: str) -> None` — add a
    /// new entry to a write-mode archive.
    ZipfileWrite        = 524,
    /// `zipfile.close(handle: i64) -> None` — finalize and close.  No-op
    /// on already-closed (or 0) handles.
    ZipfileClose        = 525,
    /// `zipfile.is_zipfile(path: str) -> bool` — quick local-file probe.
    ZipfileIsZipfile    = 526,
    /// `zipfile.info(handle: i64, name: str) -> Tuple[i64, i64, i64]` —
    /// (compressed_size, uncompressed_size, crc32).  Returns
    /// (-1, -1, -1) if entry missing.
    ZipfileInfo         = 527,
    // 528-529 reserved.

    // -- tarfile (530-549) ---------------------------------------------
    /// `tarfile.open_read(path: str, mode: str) -> i64` — mode is one of
    /// "r", "r:gz", "r:bz2".  Loads entries into memory (v0.2 keeps the
    /// whole archive resident — fine for the ~tens-of-MB scale typical
    /// of build / log / backup archives).
    TarfileOpenRead     = 530,
    /// `tarfile.open_write(path: str, mode: str) -> i64` — mode is one of
    /// "w", "w:gz", "w:bz2".
    TarfileOpenWrite    = 531,
    /// `tarfile.names(handle: i64) -> List[str]`.
    TarfileNames        = 532,
    /// `tarfile.read(handle: i64, name: str) -> str` — entry contents
    /// as bytes-packed-into-str.
    TarfileRead         = 533,
    /// `tarfile.write_file(handle: i64, src_path: str, arcname: str)
    /// -> None` — add a file from disk to a write archive.
    TarfileWriteFile    = 534,
    /// `tarfile.write_data(handle: i64, arcname: str, data: str)
    /// -> None` — add in-memory data as an entry.
    TarfileWriteData    = 535,
    /// `tarfile.close(handle: i64) -> None`.
    TarfileClose        = 536,
    /// `tarfile.is_tarfile(path: str) -> bool` — peek the first 512
    /// bytes and look for a tar header signature.
    TarfileIsTarfile    = 537,
    // 538-549 reserved.

    // ── 600-619: `ssl` module (M28 P3b-B) ──────────────────────────────
    // TLS-over-TCP client.  Each open connection is an opaque i64 handle
    // into `SharedVm.tls_streams`, holding a
    // `rustls::StreamOwned<ClientConnection, TcpStream>` — the canonical
    // "TLS over TCP" wrapper that implements Read+Write.  Sends/receives
    // round-trip through the str-as-byte-buffer convention (each
    // codepoint 0..=255 maps 1:1 to a byte; same trick `struct` /
    // `zipfile` / `tarfile` use).  See spec §9.41.
    /// `ssl.connect(host: str, port: i32) -> i64` — open a TCP socket
    /// and perform a TLS handshake against `host:port`.  Returns an
    /// opaque connection handle.  Raises IOError on TCP / TLS failure.
    SslConnect          = 600,
    /// `ssl.send(handle: i64, data: str) -> i32` — encrypt + send `data`
    /// (interpreted as packed bytes).  Returns the number of plaintext
    /// bytes written.
    SslSend             = 601,
    /// `ssl.recv(handle: i64, max_bytes: i32) -> str` — decrypt up to
    /// `max_bytes` of incoming data; returns a packed-byte str.  A zero-
    /// length result indicates clean EOF (peer closed the connection).
    SslRecv             = 602,
    /// `ssl.recv_exact(handle: i64, n: i32) -> str` — read exactly `n`
    /// bytes or raise IOError on short read / EOF.
    SslRecvExact        = 603,
    /// `ssl.close(handle: i64) -> None` — send close_notify, drop the
    /// handle.  No-op on zero / already-closed handles.
    SslClose            = 604,
    /// `ssl.peer_addr(handle: i64) -> str` — "ip:port" of the remote
    /// endpoint.  Empty string if the handle is closed.
    SslPeerAddr         = 605,
    /// `ssl.peer_cert_subject(handle: i64) -> str` — the subject CN of
    /// the peer's certificate (best-effort parse out of the DER subject).
    /// Empty string if no cert or no CN attribute.
    SslPeerCertSubject  = 606,
    /// `ssl.set_timeout_secs(handle: i64, secs: f64) -> None` — apply
    /// the timeout to both read and write on the underlying TCP socket.
    /// `secs <= 0.0` clears the timeout.
    SslSetTimeoutSecs   = 607,
    /// `ssl.set_verify_certs(enabled: bool) -> None` — global flag,
    /// affects subsequent `connect` calls.  Default true.
    SslSetVerifyCerts   = 608,
    /// `ssl.get_verify_certs() -> bool` — read the global flag back.
    SslGetVerifyCerts   = 609,
    // ── M28.5 P3b-D: server-side TLS extension to the `ssl` module ─────
    // Closes the gap M28 P3b-B deferred: accepting an inbound TCP
    // connection on a `socket.listen_tcp` handle and presenting a
    // PEM-loaded cert chain + private key to the peer.  Server-side
    // TLS handles are issued from a separate id space (starting at
    // 1_000_000 in `SharedVm.next_tls_server_id`) so the shared
    // `ssl.send` / `ssl.recv` / `ssl.close` / `ssl.peer_addr` / etc.
    // handlers can disambiguate them from client-side handles by
    // value alone.  See spec §9.41.
    /// `ssl.load_server_config(cert_pem_path: str, key_pem_path: str) -> i64`
    /// — parse the PEM-encoded cert chain + private key from disk and
    /// stash them in `SharedVm.tls_server_configs`.  Returns an opaque
    /// config handle reusable across many `accept_tls` calls.  Raises
    /// IOError on file-not-found, ValueError on PEM parse failure.
    SslLoadServerConfig = 610,
    /// `ssl.accept_tls(tcp_listener: i64, server_config: i64) -> Tuple[i64, str]`
    /// — accept the next inbound TCP connection on the listener (a
    /// handle returned by `socket.listen_tcp`), wrap it in a
    /// server-side TLS handshake, and stash the resulting stream in
    /// `SharedVm.tls_server_streams`.  Returns `(tls_handle, peer_addr)`.
    /// The returned `tls_handle` is interchangeable with a client-side
    /// `ssl.connect` handle from the caller's point of view —
    /// `ssl.send` / `recv` / `recv_exact` / `close` / `peer_addr` /
    /// `set_timeout_secs` all work.  Raises IOError on handshake
    /// failure (bad client cert, protocol mismatch, etc.).
    SslAcceptTls        = 611,
    /// `ssl.free_server_config(config: i64) -> None` — drop a config
    /// handle previously returned by `load_server_config`.  Existing
    /// `accept_tls` streams keep an `Arc<ServerConfig>` of their own,
    /// so freeing the config slot does NOT terminate in-flight sessions.
    /// No-op on zero / unknown handles.
    SslFreeServerConfig = 612,
    // 613-619 reserved for v0.3 (SNI override, ALPN, mutual auth,
    // per-connection CA bundles, session resumption, OCSP stapling).
    // ── 620–649: `http_client` module (M28 P3b-C) ──────────────────────
    // Synchronous HTTP/1.1 client built on `ureq` (rustls for TLS).  All
    // handlers are stateless — each call opens a fresh socket via ureq,
    // sends the request, reads the response, closes.  No SharedVm slot
    // table.  See spec §9.42.
    /// `http_client.get(url: str) -> Tuple[i32, str]` — convenience
    /// GET; auto-detects http:// vs https://; default timeout 30s.
    HttpClientGet              = 620,
    /// `http_client.post(url: str, body: str, content_type: str)
    /// -> Tuple[i32, str]`.
    HttpClientPost             = 621,
    /// `http_client.put(url: str, body: str, content_type: str)
    /// -> Tuple[i32, str]`.
    HttpClientPut              = 622,
    /// `http_client.delete(url: str) -> Tuple[i32, str]`.
    HttpClientDelete           = 623,
    /// `http_client.head(url: str) -> Tuple[i32, str]` — body is the
    /// empty string by HTTP semantics.
    HttpClientHead             = 624,
    /// `http_client.request(method, url, body, headers, timeout_secs)
    /// -> Tuple[i32, str]` — configurable request.  Headers are a
    /// `List[Tuple[str, str]]` of (name, value) pairs.
    HttpClientRequest          = 625,
    /// `http_client.request_with_headers(method, url, body, headers,
    /// timeout_secs) -> Tuple[i32, List[Tuple[str, str]], str]`.
    HttpClientRequestWithHeaders = 626,
    /// `http_client.urlencode(pairs: List[Tuple[str, str]]) -> str`
    /// — `key=value&key2=value2`, percent-encoded.
    HttpClientUrlencode        = 627,
    /// `http_client.urldecode(s: str) -> str`.
    HttpClientUrldecode        = 628,
    /// `http_client.url_parse(url: str) -> Tuple[str, str, i32, str]`
    /// — `(scheme, host, port, path_and_query)`.  Port defaults to
    /// 80/443 based on scheme if missing.
    HttpClientUrlParse         = 629,
    /// `http_client.status_text(code: i32) -> str` — `200` → `"OK"`,
    /// `404` → `"Not Found"`, etc.
    HttpClientStatusText       = 630,
    // 631-649 reserved for v0.3 (connection pooling, cookies, etc.).

    // ── 700-749: `asyncio` + async-variant sockets (M32 v0.3) ───────────
    // Shape A scheduler: each spawn allocates a Future slot and an OS
    // thread; await blocks on a Condvar inside the slot.  The v0.4 swap
    // to a mio/polling-based event loop preserves these ids and the API
    // surface — see spec §9.43.4.
    //
    // 700-719: asyncio module entry points.
    /// `asyncio.run_i32(target: fn() -> i32) -> i32` — top-level entry.
    AsyncioRunI32       = 700,
    /// `asyncio.run_unit(target: fn() -> None) -> None`.
    AsyncioRunUnit      = 701,
    /// `asyncio.spawn_i32(target: fn() -> i32) -> Future[i32]`.
    AsyncioSpawnI32     = 702,
    /// `asyncio.spawn_i64(target: fn() -> i64) -> Future[i64]`.
    AsyncioSpawnI64     = 703,
    /// `asyncio.spawn_str(target: fn() -> str) -> Future[str]`.
    AsyncioSpawnStr     = 704,
    /// `asyncio.spawn_bool(target: fn() -> bool) -> Future[bool]`.
    AsyncioSpawnBool    = 705,
    /// `asyncio.spawn_unit(target: fn() -> None) -> Future[None]`.
    AsyncioSpawnUnit    = 706,
    /// `asyncio.sleep(secs: f64) -> None`.
    AsyncioSleep        = 707,
    /// `Future.await()` — type-erased on the value side (the value is
    /// stored as `u64` in the slot and the static type at the call site
    /// drives interpretation).  Receiver dispatch via the
    /// `resolve_native_method` path on a `Generic { Future, .. }` recv.
    AsyncioFutureAwait  = 708,
    /// `Future.is_ready() -> bool`.
    AsyncioFutureIsReady = 709,
    /// `asyncio.gather_2_i32` / variants — fixed-arity (no variadics).
    AsyncioGather2I32   = 710,
    AsyncioGather2Str   = 711,
    AsyncioGather3I32   = 712,
    AsyncioGather3Str   = 713,
    AsyncioGather4I32   = 714,
    // 715-719 reserved for v0.3 follow-ups (more gather variants).

    // 720-729: async-variant socket functions.
    /// `socket.async_accept(listener: i64) -> Future[Tuple[i64, str]]`.
    SocketAsyncAccept   = 720,
    /// `socket.async_recv(handle: i64, max_bytes: i32) -> Future[str]`.
    SocketAsyncRecv     = 721,
    /// `socket.async_send(handle: i64, data: str) -> Future[i32]`.
    SocketAsyncSend     = 722,
    // 723-729 reserved for v0.3 follow-ups (async connect, etc.).
    // 730-749 reserved for v0.4 extensions (async ssl / file I/O).

    // ── 750-789: M34 — `json` typed `JsonValue` tree ────────────────────
    // First stdlib *classes* in the project.  The 7-class hierarchy
    // (JsonValue + JNull/JBool/JInt/JFloat/JString/JList/JObject) is
    // registered in `seed_prelude` alongside Channel/Thread/io.File
    // because v0.3 doesn't yet have proper module-scoped class
    // registration; the stdlib import path falls through to the
    // pre-existing prelude binding (resolver.rs's legacy "prelude wins"
    // branch) so user code can still write `from json import JsonValue`.
    //
    // Subclass instances carry one concrete field at offset 0:
    //   - JNull:    no field
    //   - JBool:    value: bool (stored as u64)
    //   - JInt:     value: i64
    //   - JFloat:   value: f64 (bit-cast through u64)
    //   - JString:  value: str (StringRepr pointer)
    //   - JList:    data: opaque ListRepr pointer (elements are JsonValue ptrs)
    //   - JObject:  data: opaque ListRepr pointer of interleaved [key_ptr, value_ptr]
    //
    /// `json.parse(s: str) -> JsonValue` — parse JSON into a typed
    /// JsonValue tree.  Raises `ValueError` on malformed input.
    JsonParse           = 750,
    /// `json.stringify(v: JsonValue) -> str` — compact canonical
    /// serialization (no whitespace, sorted-key-friendly).
    JsonStringify       = 751,
    /// `json.stringify_pretty(v: JsonValue, indent: i32) -> str` —
    /// indented serialization (typical indent 2 or 4; clamped to [0,32]).
    JsonStringifyPretty = 752,
    /// `JNull()` — construct a JsonNull singleton instance.  Also the
    /// `json.j_null()` convenience helper.
    JsonJNullNew        = 753,
    /// `JBool(b: bool)` — construct.  Also `json.j_bool(b)`.
    JsonJBoolNew        = 754,
    /// `JInt(n: i64)`.  Also `json.j_int(n)`.
    JsonJIntNew         = 755,
    /// `JFloat(f: f64)`.  Also `json.j_float(f)`.
    JsonJFloatNew       = 756,
    /// `JString(s: str)`.  Also `json.j_string(s)`.
    JsonJStringNew      = 757,
    /// `JList(items: List[JsonValue])`.  Also `json.j_list(items)`.
    JsonJListNew        = 758,
    /// `JObject(entries: List[Tuple[str, JsonValue]])` constructor
    /// (receiver-style, see `JsonJ*New` block comment).
    JsonJObjectNew      = 759,
    // 760-766: `json.j_*` module helpers.  Same effect as the class
    // constructors above but allocate-and-populate in one call (no
    // pre-allocated receiver from the IR).  Two ID slots per shape
    // (e.g. `j_string` vs `JString(...)`) keeps the handler bodies
    // separate so a future v0.4 split is easier.
    JsonHelperJNull     = 760,
    JsonHelperJBool     = 761,
    JsonHelperJInt      = 762,
    JsonHelperJFloat    = 763,
    JsonHelperJString   = 764,
    JsonHelperJList     = 765,
    JsonHelperJObject   = 766,
    // 767-769 reserved for v0.4 (e.g. j_bigint).
    /// `JList.length(self) -> i64`.
    JsonJListLength     = 770,
    /// `JList.get(self, i: i64) -> JsonValue` — raises `IndexError`
    /// out-of-bounds.
    JsonJListGet        = 771,
    /// `JList.items(self) -> List[JsonValue]` — defensive copy of the
    /// underlying list so user code can iterate without aliasing.
    JsonJListItems      = 772,
    // 773-779 reserved for v0.4 (append / set / pop on a mutable JList).
    /// `JObject.get(self, k: str) -> JsonValue?` — none if absent.
    JsonJObjectGet      = 780,
    /// `JObject.has(self, k: str) -> bool`.
    JsonJObjectHas      = 781,
    /// `JObject.keys(self) -> List[str]` — insertion-order copy.
    JsonJObjectKeys     = 782,
    /// `JObject.length(self) -> i64` — entry count.
    JsonJObjectLength   = 783,
    // 784-789 reserved for v0.4 (set / iter_items / values on a
    // mutable JObject).

    // ── 800-819: M35 P4-B — `sqlite3.Connection` + `sqlite3.Cursor` ─────
    //
    // The M23 P3a-D flat function surface (`sqlite3.connect(path) -> i64`
    // + `sqlite3.execute(handle, sql)` etc.) remains in place; this
    // block adds typed classes that wrap the same i64 connection slot
    // table on `SharedVm.sqlite_connections` plus a new per-Cursor
    // sidecar table (`sqlite_cursors`).  Pattern mirrors M34's class-
    // constructor split: ID 800 is the receiver-style `__init__` for
    // `Connection(handle)` (called from `sqlite3.open` via Alloc +
    // NativeCall), 811 is the same for `Cursor`.  Method NativeFns
    // (802-810, 812-816) receive the class instance as arg 0 and read
    // the i64 slot handle out of payload offset 0.
    //
    // Layouts (registered in resolver::seed_prelude):
    //   Connection: { handle: i64 } — 8 bytes payload
    //   Cursor:     { handle: i64 } — 8 bytes payload (index into
    //                                  SharedVm.sqlite_cursors)
    /// `Connection.__init__(self, handle: i64) -> None` — receiver-style.
    Sqlite3ConnectionInit       = 800,
    /// `sqlite3.open(path: str) -> Connection` — alloc+init helper.
    Sqlite3OpenTyped            = 801,
    /// `Connection.execute(self, sql: str) -> None`.
    Sqlite3ConnectionExecute    = 802,
    /// `Connection.execute_params(self, sql: str, params: List[str]) -> None`.
    Sqlite3ConnectionExecuteParams = 803,
    /// `Connection.query(self, sql: str) -> Cursor`.
    Sqlite3ConnectionQuery      = 804,
    /// `Connection.query_params(self, sql: str, params: List[str]) -> Cursor`.
    Sqlite3ConnectionQueryParams = 805,
    /// `Connection.last_insert_rowid(self) -> i64`.
    Sqlite3ConnectionLastInsertRowid = 806,
    /// `Connection.changes(self) -> i32`.
    Sqlite3ConnectionChanges    = 807,
    /// `Connection.close(self) -> None` — idempotent.
    Sqlite3ConnectionClose      = 808,
    // 809-810 reserved for v0.3 follow-ups (commit / rollback once
    // explicit transactions land).
    /// `Cursor.__init__(self, handle: i64) -> None` — receiver-style.
    Sqlite3CursorInit           = 811,
    /// `Cursor.fetchone(self) -> List[str]?` — next row or `none`
    /// when exhausted.  Uses `NONE_SENTINEL` (`0x8000_0000_0000_0000`),
    /// NOT zero — see M34 report's NONE_SENTINEL gotcha.
    Sqlite3CursorFetchOne       = 812,
    /// `Cursor.fetchall(self) -> List[List[str]]` — remaining rows.
    Sqlite3CursorFetchAll       = 813,
    /// `Cursor.column_names(self) -> List[str]`.
    Sqlite3CursorColumnNames    = 814,
    /// `Cursor.row_count(self) -> i64` — total rows the underlying
    /// query produced (not "rows remaining").
    Sqlite3CursorRowCount       = 815,
    // 816-819 reserved for v0.4 (Cursor iteration support /
    // Connection.commit / Connection.rollback).
    // ── 790-799: M35 P4-A — compiled `re.Pattern` class ─────────────────
    // First stdlib class to use the *opaque-handle* shape (the JsonValue
    // family at 750-789 uses real heap-field layouts so pattern matching
    // works).  A `Pattern` instance carries one i64 field at offset 0
    // that indexes into `SharedVm.compiled_regexes`; the actual
    // `regex::Regex` lives in that slot table.  Construction is gated:
    // `re.compile(s)` is the only way to mint a slot, and the Pattern
    // class's NativeFn-init handler just plumbs the freshly-allocated
    // slot id into the new instance.
    //
    // Methods (`matches` / `find` / `find_all` / `replace` /
    // `replace_all` / `split` / `source`) all share the same shape:
    // read the i64 handle off the receiver, look up the Regex, dispatch
    // to the matching `regex::Regex` API (identical to the existing
    // `Re*` handlers at 220-226 but skipping the re-compile step).
    //
    /// `Pattern.__init__(handle: i64)` — receiver-style.  Stores the
    /// compiled-regex slot handle into the receiver's only field.
    /// Users don't call this directly; `re.compile` allocates the
    /// slot then invokes the constructor.
    PatternCtor        = 790,
    /// `re.compile(pattern: str) -> Pattern` — compile + intern.
    /// Raises `ValueError` on bad regex syntax.
    RePatternCompile   = 791,
    /// `Pattern.matches(self, s: str) -> bool` — full-string match
    /// (mirrors `re.fullmatch` semantics).
    PatternMatches     = 792,
    /// `Pattern.find(self, s: str) -> str?` — first match's text, or
    /// none.  Differs from the flat `re.find` which returns
    /// `(i32, i32)` indices.
    PatternFind        = 793,
    /// `Pattern.find_all(self, s: str) -> List[str]`.
    PatternFindAll     = 794,
    /// `Pattern.replace(self, s: str, repl: str) -> str` — first
    /// match only (one-shot replacement).
    PatternReplace     = 795,
    /// `Pattern.replace_all(self, s: str, repl: str) -> str`.
    PatternReplaceAll  = 796,
    /// `Pattern.split(self, s: str) -> List[str]`.
    PatternSplit       = 797,
    /// `Pattern.source(self) -> str` — original pattern string.
    PatternSource      = 798,
    // 799 reserved for v0.4 PatternIterFinds (lazy iterator).

    // ── 120+: misc ──────────────────────────────────────────────────────
    /// Fallback for any unrecognised prelude/stdlib symbol the M3 lowerer
    /// encounters. The VM treats this as a runtime error.
    Unknown = 0xFFFF_FFFF,
}

impl NativeFn {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            1 => Some(Self::Println),
            2 => Some(Self::Print),
            3 => Some(Self::Len),
            4 => Some(Self::Range),
            5 => Some(Self::Assert),
            6 => Some(Self::StrFromI32),
            7 => Some(Self::StrFromI64),
            8 => Some(Self::StrFromF64),
            9 => Some(Self::StrFromBool),
            10 => Some(Self::StrFromChar),
            11 => Some(Self::StrConcat),
            12 => Some(Self::Abs),
            13 => Some(Self::Min),
            14 => Some(Self::Max),
            15 => Some(Self::I32FromI64),
            16 => Some(Self::I64FromI32),
            17 => Some(Self::F64FromI32),
            18 => Some(Self::F64FromI64),
            19 => Some(Self::I32FromF64),
            20 => Some(Self::StrFromBytes),
            21 => Some(Self::StrFromAny),
            22 => Some(Self::BoolFromAny),
            23 => Some(Self::CharFromI32),
            24 => Some(Self::StrSlice),
            25 => Some(Self::StrAppendChar),
            // real-world: csv_aggregate
            26 => Some(Self::F64FromStr),
            27 => Some(Self::I64FromStr),
            28 => Some(Self::StrSplit),
            29 => Some(Self::I64FromF64),
            30 => Some(Self::IoOpen),
            31 => Some(Self::FileRead),
            32 => Some(Self::FileWrite),
            33 => Some(Self::FileClose),
            34 => Some(Self::FileEnter),
            35 => Some(Self::FileExit),
            50 => Some(Self::ChannelNew),
            51 => Some(Self::ChannelSend),
            52 => Some(Self::ChannelRecv),
            53 => Some(Self::ChannelTryRecv),
            54 => Some(Self::ChannelClose),
            60 => Some(Self::ThreadNew),
            61 => Some(Self::ThreadStart),
            62 => Some(Self::ThreadJoin),
            70 => Some(Self::MathSqrt),
            71 => Some(Self::MathSin),
            72 => Some(Self::MathCos),
            73 => Some(Self::MathTan),
            74 => Some(Self::MathLog),
            75 => Some(Self::MathExp),
            76 => Some(Self::MathPow),
            77 => Some(Self::MathFloor),
            78 => Some(Self::MathCeil),
            79 => Some(Self::MathAbsF),
            90 => Some(Self::ListAppend),
            91 => Some(Self::ListGet),
            92 => Some(Self::ListSet),
            93 => Some(Self::ListLen),
            94 => Some(Self::DictGet),
            95 => Some(Self::DictSet),
            96 => Some(Self::DictHas),
            97 => Some(Self::DictKeys),
            98 => Some(Self::DictValues),
            99 => Some(Self::DictLen),
            100 => Some(Self::SetAdd),
            101 => Some(Self::SetHas),
            102 => Some(Self::SetLen),
            103 => Some(Self::DictNew),
            104 => Some(Self::StrCharAt),
            105 => Some(Self::ListSort),
            106 => Some(Self::ListSorted),
            // real-world: fix — see `ListPop` definition above.
            107 => Some(Self::ListPop),
            // M19: sys module.
            130 => Some(Self::SysArgv),
            131 => Some(Self::SysExit),
            132 => Some(Self::SysPlatform),
            133 => Some(Self::SysVersion),
            // M20a: os module.
            140 => Some(Self::OsEnv),
            141 => Some(Self::OsSetEnv),
            142 => Some(Self::OsGetCwd),
            143 => Some(Self::OsChdir),
            144 => Some(Self::OsListDir),
            145 => Some(Self::OsRemove),
            146 => Some(Self::OsMkdir),
            147 => Some(Self::OsExists),
            148 => Some(Self::OsIsFile),
            149 => Some(Self::OsIsDir),
            150 => Some(Self::OsReadFile),
            151 => Some(Self::OsWriteFile),
            // M20a: path module.
            160 => Some(Self::PathJoin),
            161 => Some(Self::PathJoin3),
            162 => Some(Self::PathDirname),
            163 => Some(Self::PathBasename),
            164 => Some(Self::PathSplitext),
            165 => Some(Self::PathSep),
            // M20a: io module.
            170 => Some(Self::IoInput),
            171 => Some(Self::IoInputPrompt),
            172 => Some(Self::IoWriteStdout),
            173 => Some(Self::IoWriteStderr),
            174 => Some(Self::IoFlushStdout),
            // M20b: time module.
            175 => Some(Self::TimeNow),
            176 => Some(Self::TimeNowMs),
            177 => Some(Self::TimeMonotonic),
            178 => Some(Self::TimeSleepS),
            179 => Some(Self::TimeSleepMs),
            180 => Some(Self::TimeFormatIso),
            // M20b: random module.
            185 => Some(Self::RandomSeed),
            186 => Some(Self::RandomRandint),
            187 => Some(Self::RandomRandom),
            188 => Some(Self::RandomChoiceI64),
            189 => Some(Self::RandomChoiceF64),
            190 => Some(Self::RandomChoiceStr),
            191 => Some(Self::RandomShuffleI64),
            192 => Some(Self::RandomShuffleF64),
            193 => Some(Self::RandomShuffleStr),
            194 => Some(Self::RandomSampleI64),
            195 => Some(Self::RandomSampleF64),
            196 => Some(Self::RandomSampleStr),
            // M20b: math module extensions.
            200 => Some(Self::MathLog2),
            201 => Some(Self::MathLog10),
            202 => Some(Self::MathFloorI),
            203 => Some(Self::MathCeilI),
            204 => Some(Self::MathGcd),
            205 => Some(Self::MathFactorial),
            206 => Some(Self::MathIsNan),
            207 => Some(Self::MathIsInf),
            208 => Some(Self::MathConstPi),
            209 => Some(Self::MathConstE),
            210 => Some(Self::MathConstTau),
            211 => Some(Self::MathConstInf),
            212 => Some(Self::MathConstNan),
            // M20c: json module.
            213 => Some(Self::JsonParseToString),
            214 => Some(Self::JsonIsValid),
            215 => Some(Self::JsonPretty),
            216 => Some(Self::JsonEscape),
            217 => Some(Self::JsonMinify),
            // M20c: re module.
            220 => Some(Self::ReMatch),
            221 => Some(Self::ReSearch),
            222 => Some(Self::ReFind),
            223 => Some(Self::ReFindAll),
            224 => Some(Self::ReReplace),
            225 => Some(Self::ReSplit),
            226 => Some(Self::ReIsValid),
            // M22 P2A: argparse module.
            250 => Some(Self::ArgparseNew),
            251 => Some(Self::ArgparseAddFlag),
            252 => Some(Self::ArgparseAddArg),
            253 => Some(Self::ArgparseAddOpt),
            254 => Some(Self::ArgparseParse),
            255 => Some(Self::ArgparseGetFlag),
            256 => Some(Self::ArgparseGetArg),
            257 => Some(Self::ArgparseGetOpt),
            258 => Some(Self::ArgparseHelpText),
            259 => Some(Self::ArgparseHelpRequested),
            // M22 P2A: collections module.
            265 => Some(Self::CollCounterNew),
            266 => Some(Self::CollCounterIncrement),
            267 => Some(Self::CollCounterAdd),
            268 => Some(Self::CollCounterGet),
            269 => Some(Self::CollCounterTopKeys),
            270 => Some(Self::CollDequeNew),
            271 => Some(Self::CollDequePushBack),
            272 => Some(Self::CollDequePopFront),
            273 => Some(Self::CollDequeLen),
            274 => Some(Self::CollDequeIsEmpty),
            // M22 P2A: csv module.
            275 => Some(Self::CsvParseLine),
            276 => Some(Self::CsvParse),
            277 => Some(Self::CsvReadFile),
            278 => Some(Self::CsvWriteFile),
            279 => Some(Self::CsvEscape),
            280 => Some(Self::CsvFormatRow),
            // M22 P2B: base64 module.
            290 => Some(Self::Base64Encode),
            291 => Some(Self::Base64Decode),
            292 => Some(Self::Base64EncodeUrlSafe),
            293 => Some(Self::Base64DecodeUrlSafe),
            // M22 P2B: hashlib module.
            300 => Some(Self::HashlibMd5),
            301 => Some(Self::HashlibSha1),
            302 => Some(Self::HashlibSha256),
            303 => Some(Self::HashlibSha512),
            304 => Some(Self::HashlibHmacSha256),
            // M35 P4-C: streaming Hasher.
            820 => Some(Self::HasherCtor),
            821 => Some(Self::HashlibNew),
            822 => Some(Self::HasherUpdate),
            823 => Some(Self::HasherHexdigest),
            824 => Some(Self::HasherAlgorithm),
            // M22 P2C: itertools module.
            310 => Some(Self::ItertoolsRangeStep),
            311 => Some(Self::ItertoolsEnumerateStr),
            312 => Some(Self::ItertoolsEnumerateI64),
            313 => Some(Self::ItertoolsZipStrStr),
            314 => Some(Self::ItertoolsZipI64I64),
            315 => Some(Self::ItertoolsChainStr),
            316 => Some(Self::ItertoolsChainI64),
            317 => Some(Self::ItertoolsTakeStr),
            318 => Some(Self::ItertoolsDropStr),
            319 => Some(Self::ItertoolsPairwiseStr),
            320 => Some(Self::ItertoolsAccumulateI64),
            321 => Some(Self::ItertoolsFlattenStr),
            // M22 P2C: statistics module.
            322 => Some(Self::StatsMean),
            323 => Some(Self::StatsMedian),
            324 => Some(Self::StatsStdev),
            325 => Some(Self::StatsVariance),
            326 => Some(Self::StatsMinMax),
            327 => Some(Self::StatsSum),
            328 => Some(Self::StatsQuantile),
            329 => Some(Self::StatsModeStr),
            // M22 P2D: struct module (330-341, 12 ids).
            330 => Some(Self::StructPackU32Be),
            331 => Some(Self::StructPackU32Le),
            332 => Some(Self::StructPackU64Be),
            333 => Some(Self::StructPackU64Le),
            334 => Some(Self::StructPackF64Be),
            335 => Some(Self::StructPackF64Le),
            336 => Some(Self::StructUnpackU32Be),
            337 => Some(Self::StructUnpackU32Le),
            338 => Some(Self::StructUnpackU64Be),
            339 => Some(Self::StructUnpackU64Le),
            340 => Some(Self::StructUnpackF64Be),
            341 => Some(Self::StructUnpackF64Le),
            // M22 P2D: urllib_parse module (342-347, 6 ids).
            342 => Some(Self::UrlQuote),
            343 => Some(Self::UrlQuotePlus),
            344 => Some(Self::UrlUnquote),
            345 => Some(Self::UrlUnquotePlus),
            346 => Some(Self::UrlEncode),
            347 => Some(Self::UrlParseQuery),
            // 348-349 reserved for v0.3 (parse_url + join_url).
            // M23 P3a-A: subprocess module (350-355, 6 ids; 356-369 reserved).
            350 => Some(Self::SubprocessRun),
            351 => Some(Self::SubprocessRunWithStdin),
            352 => Some(Self::SubprocessSpawn),
            353 => Some(Self::SubprocessWait),
            354 => Some(Self::SubprocessTryWait),
            355 => Some(Self::SubprocessKill),
            // M23 P3a-A: pathlib module (370-383, 14 ids; 384-389 reserved).
            370 => Some(Self::PathlibJoin),
            371 => Some(Self::PathlibWithSuffix),
            372 => Some(Self::PathlibWithName),
            373 => Some(Self::PathlibParent),
            374 => Some(Self::PathlibName),
            375 => Some(Self::PathlibStem),
            376 => Some(Self::PathlibSuffix),
            377 => Some(Self::PathlibParts),
            378 => Some(Self::PathlibIsAbsolute),
            379 => Some(Self::PathlibAbsolute),
            380 => Some(Self::PathlibRelativeTo),
            381 => Some(Self::PathlibReadText),
            382 => Some(Self::PathlibWriteText),
            383 => Some(Self::PathlibReadLines),
            // M23 P3a-B: datetime module (390-411; 412-419 reserved).
            390 => Some(Self::DateTimeNow),
            391 => Some(Self::DateTimeFromUnix),
            392 => Some(Self::DateTimeFromYmd),
            393 => Some(Self::DateTimeFromYmdHms),
            394 => Some(Self::DateTimeYear),
            395 => Some(Self::DateTimeMonth),
            396 => Some(Self::DateTimeDay),
            397 => Some(Self::DateTimeHour),
            398 => Some(Self::DateTimeMinute),
            399 => Some(Self::DateTimeSecond),
            400 => Some(Self::DateTimeWeekday),
            401 => Some(Self::DateTimeYmd),
            402 => Some(Self::DateTimeAddSeconds),
            403 => Some(Self::DateTimeAddDays),
            404 => Some(Self::DateTimeDiffSeconds),
            405 => Some(Self::DateTimeDiffDays),
            406 => Some(Self::DateTimeToIso),
            407 => Some(Self::DateTimeToDateStr),
            408 => Some(Self::DateTimeToTimeStr),
            409 => Some(Self::DateTimeFromIso),
            410 => Some(Self::DateTimeFromDateStr),
            411 => Some(Self::DateTimeLocalOffsetMinutes),
            // M23 P3a-C: threading + queue (420-437).
            420 => Some(Self::ThreadingLockNew),
            421 => Some(Self::ThreadingLockAcquire),
            422 => Some(Self::ThreadingLockRelease),
            423 => Some(Self::ThreadingLockTryAcquire),
            424 => Some(Self::ThreadingSemaphoreNew),
            425 => Some(Self::ThreadingSemaphoreAcquire),
            426 => Some(Self::ThreadingSemaphoreRelease),
            427 => Some(Self::ThreadingSemaphoreTryAcquire),
            428 => Some(Self::QueuePqNewI64),
            429 => Some(Self::QueuePqPushI64),
            430 => Some(Self::QueuePqPopMinI64),
            431 => Some(Self::QueuePqPeekMinI64),
            432 => Some(Self::QueuePqNewStr),
            433 => Some(Self::QueuePqPushStr),
            434 => Some(Self::QueuePqPopMinStr),
            435 => Some(Self::QueuePqPeekMinStr),
            436 => Some(Self::QueuePqLen),
            437 => Some(Self::QueuePqIsEmpty),
            // M23 P3a-D: sqlite3 module (440-448, 9 ids; 449-469 reserved).
            440 => Some(Self::Sqlite3Connect),
            441 => Some(Self::Sqlite3Close),
            442 => Some(Self::Sqlite3Execute),
            443 => Some(Self::Sqlite3ExecuteParams),
            444 => Some(Self::Sqlite3Query),
            445 => Some(Self::Sqlite3QueryParams),
            446 => Some(Self::Sqlite3LastInsertRowid),
            447 => Some(Self::Sqlite3Changes),
            448 => Some(Self::Sqlite3ColumnNames),
            // M27 P3c-A: shutil + tempfile (450-472, 9 ids; 456-469 + 473-479 reserved).
            450 => Some(Self::ShutilCopy),
            451 => Some(Self::ShutilCopytree),
            452 => Some(Self::ShutilMove),
            453 => Some(Self::ShutilRmtree),
            454 => Some(Self::ShutilWhich),
            455 => Some(Self::ShutilDiskUsage),
            470 => Some(Self::TempfileMkdtemp),
            471 => Some(Self::TempfileMkstemp),
            472 => Some(Self::TempfileGettempdir),
            // M27 P3c-E: logging module (550-560, 11 ids; 561-569 reserved).
            550 => Some(Self::LoggingBasicConfig),
            551 => Some(Self::LoggingBasicConfigToFile),
            552 => Some(Self::LoggingSetLevel),
            553 => Some(Self::LoggingGetLevel),
            554 => Some(Self::LoggingDebug),
            555 => Some(Self::LoggingInfo),
            556 => Some(Self::LoggingWarning),
            557 => Some(Self::LoggingError),
            558 => Some(Self::LoggingCritical),
            559 => Some(Self::LoggingLog),
            560 => Some(Self::LoggingIsEnabledFor),
            // M27 P3c-B: glob + fnmatch modules (480-486).
            480 => Some(Self::GlobGlob),
            481 => Some(Self::GlobRecursive),
            482 => Some(Self::GlobEscape),
            483 => Some(Self::FnmatchFnmatch),
            484 => Some(Self::FnmatchFnmatchcase),
            485 => Some(Self::FnmatchFilter),
            486 => Some(Self::FnmatchTranslate),
            // M27 P3c-C: gzip + zlib + bz2 (500-510, 11 ids; 511-519 reserved).
            500 => Some(Self::GzipCompress),
            501 => Some(Self::GzipCompressLevel),
            502 => Some(Self::GzipDecompress),
            503 => Some(Self::ZlibCompress),
            504 => Some(Self::ZlibCompressLevel),
            505 => Some(Self::ZlibDecompress),
            506 => Some(Self::ZlibCrc32),
            507 => Some(Self::ZlibAdler32),
            508 => Some(Self::Bz2Compress),
            509 => Some(Self::Bz2CompressLevel),
            510 => Some(Self::Bz2Decompress),
            // M27 P3c-D: zipfile (520-527; 528-529 reserved).
            520 => Some(Self::ZipfileOpenRead),
            521 => Some(Self::ZipfileOpenWrite),
            522 => Some(Self::ZipfileNames),
            523 => Some(Self::ZipfileRead),
            524 => Some(Self::ZipfileWrite),
            525 => Some(Self::ZipfileClose),
            526 => Some(Self::ZipfileIsZipfile),
            527 => Some(Self::ZipfileInfo),
            // M27 P3c-D: tarfile (530-537; 538-549 reserved).
            530 => Some(Self::TarfileOpenRead),
            531 => Some(Self::TarfileOpenWrite),
            532 => Some(Self::TarfileNames),
            533 => Some(Self::TarfileRead),
            534 => Some(Self::TarfileWriteFile),
            535 => Some(Self::TarfileWriteData),
            536 => Some(Self::TarfileClose),
            537 => Some(Self::TarfileIsTarfile),
            // M28 P3b-A: socket module (570-588; 589-599 reserved).
            570 => Some(Self::SocketConnectTcp),
            571 => Some(Self::SocketSend),
            572 => Some(Self::SocketRecv),
            573 => Some(Self::SocketRecvExact),
            574 => Some(Self::SocketClose),
            575 => Some(Self::SocketSetTimeoutSecs),
            576 => Some(Self::SocketPeerAddr),
            577 => Some(Self::SocketLocalAddr),
            578 => Some(Self::SocketListenTcp),
            579 => Some(Self::SocketAccept),
            580 => Some(Self::SocketCloseListener),
            581 => Some(Self::SocketUdpSocket),
            582 => Some(Self::SocketUdpBind),
            583 => Some(Self::SocketUdpSendTo),
            584 => Some(Self::SocketUdpRecvFrom),
            585 => Some(Self::SocketUdpClose),
            586 => Some(Self::SocketGethostbyname),
            587 => Some(Self::SocketResolve),
            588 => Some(Self::SocketGethostname),
            // M28 P3b-B: ssl module (600-609; 610-619 reserved).
            600 => Some(Self::SslConnect),
            601 => Some(Self::SslSend),
            602 => Some(Self::SslRecv),
            603 => Some(Self::SslRecvExact),
            604 => Some(Self::SslClose),
            605 => Some(Self::SslPeerAddr),
            606 => Some(Self::SslPeerCertSubject),
            607 => Some(Self::SslSetTimeoutSecs),
            608 => Some(Self::SslSetVerifyCerts),
            609 => Some(Self::SslGetVerifyCerts),
            610 => Some(Self::SslLoadServerConfig),
            611 => Some(Self::SslAcceptTls),
            612 => Some(Self::SslFreeServerConfig),
            // M28 P3b-C: http_client module (620-630, 11 ids; 631-649 reserved).
            620 => Some(Self::HttpClientGet),
            621 => Some(Self::HttpClientPost),
            622 => Some(Self::HttpClientPut),
            623 => Some(Self::HttpClientDelete),
            624 => Some(Self::HttpClientHead),
            625 => Some(Self::HttpClientRequest),
            626 => Some(Self::HttpClientRequestWithHeaders),
            627 => Some(Self::HttpClientUrlencode),
            628 => Some(Self::HttpClientUrldecode),
            629 => Some(Self::HttpClientUrlParse),
            630 => Some(Self::HttpClientStatusText),
            // M32: asyncio + async-variant sockets (700-722; 723-749 reserved).
            700 => Some(Self::AsyncioRunI32),
            701 => Some(Self::AsyncioRunUnit),
            702 => Some(Self::AsyncioSpawnI32),
            703 => Some(Self::AsyncioSpawnI64),
            704 => Some(Self::AsyncioSpawnStr),
            705 => Some(Self::AsyncioSpawnBool),
            706 => Some(Self::AsyncioSpawnUnit),
            707 => Some(Self::AsyncioSleep),
            708 => Some(Self::AsyncioFutureAwait),
            709 => Some(Self::AsyncioFutureIsReady),
            710 => Some(Self::AsyncioGather2I32),
            711 => Some(Self::AsyncioGather2Str),
            712 => Some(Self::AsyncioGather3I32),
            713 => Some(Self::AsyncioGather3Str),
            714 => Some(Self::AsyncioGather4I32),
            720 => Some(Self::SocketAsyncAccept),
            721 => Some(Self::SocketAsyncRecv),
            722 => Some(Self::SocketAsyncSend),
            // M34: typed JsonValue tree.
            750 => Some(Self::JsonParse),
            751 => Some(Self::JsonStringify),
            752 => Some(Self::JsonStringifyPretty),
            753 => Some(Self::JsonJNullNew),
            754 => Some(Self::JsonJBoolNew),
            755 => Some(Self::JsonJIntNew),
            756 => Some(Self::JsonJFloatNew),
            757 => Some(Self::JsonJStringNew),
            758 => Some(Self::JsonJListNew),
            759 => Some(Self::JsonJObjectNew),
            760 => Some(Self::JsonHelperJNull),
            761 => Some(Self::JsonHelperJBool),
            762 => Some(Self::JsonHelperJInt),
            763 => Some(Self::JsonHelperJFloat),
            764 => Some(Self::JsonHelperJString),
            765 => Some(Self::JsonHelperJList),
            766 => Some(Self::JsonHelperJObject),
            770 => Some(Self::JsonJListLength),
            771 => Some(Self::JsonJListGet),
            772 => Some(Self::JsonJListItems),
            780 => Some(Self::JsonJObjectGet),
            781 => Some(Self::JsonJObjectHas),
            782 => Some(Self::JsonJObjectKeys),
            783 => Some(Self::JsonJObjectLength),
            // M35 P4-B: sqlite3.Connection + Cursor classes (800-815).
            // 816-819 reserved for v0.4.
            800 => Some(Self::Sqlite3ConnectionInit),
            801 => Some(Self::Sqlite3OpenTyped),
            802 => Some(Self::Sqlite3ConnectionExecute),
            803 => Some(Self::Sqlite3ConnectionExecuteParams),
            804 => Some(Self::Sqlite3ConnectionQuery),
            805 => Some(Self::Sqlite3ConnectionQueryParams),
            806 => Some(Self::Sqlite3ConnectionLastInsertRowid),
            807 => Some(Self::Sqlite3ConnectionChanges),
            808 => Some(Self::Sqlite3ConnectionClose),
            811 => Some(Self::Sqlite3CursorInit),
            812 => Some(Self::Sqlite3CursorFetchOne),
            813 => Some(Self::Sqlite3CursorFetchAll),
            814 => Some(Self::Sqlite3CursorColumnNames),
            815 => Some(Self::Sqlite3CursorRowCount),
            // M35 P4-A: compiled re.Pattern class.
            790 => Some(Self::PatternCtor),
            791 => Some(Self::RePatternCompile),
            792 => Some(Self::PatternMatches),
            793 => Some(Self::PatternFind),
            794 => Some(Self::PatternFindAll),
            795 => Some(Self::PatternReplace),
            796 => Some(Self::PatternReplaceAll),
            797 => Some(Self::PatternSplit),
            798 => Some(Self::PatternSource),
            // M37: tabular module (DataFrame + sealed Column hierarchy).
            830 => Some(Self::M37TabColI64),
            831 => Some(Self::M37TabColI64Simple),
            832 => Some(Self::M37TabColF64),
            833 => Some(Self::M37TabColF64Simple),
            834 => Some(Self::M37TabColStr),
            835 => Some(Self::M37TabColStrSimple),
            836 => Some(Self::M37TabColBool),
            837 => Some(Self::M37TabColBoolSimple),
            838 => Some(Self::M37TabColDateTime),
            839 => Some(Self::M37TabFromColumns),
            840 => Some(Self::M37TabColLength),
            841 => Some(Self::M37TabColDtype),
            842 => Some(Self::M37TabColIsNull),
            843 => Some(Self::M37TabColNullCount),
            844 => Some(Self::M37TabColI64Get),
            845 => Some(Self::M37TabColF64Get),
            846 => Some(Self::M37TabColStrGet),
            847 => Some(Self::M37TabColBoolGet),
            848 => Some(Self::M37TabColDateTimeGetMs),
            849 => Some(Self::M37TabDfLength),
            850 => Some(Self::M37TabDfNcols),
            851 => Some(Self::M37TabDfColumns),
            852 => Some(Self::M37TabDfDtypes),
            853 => Some(Self::M37TabDfHasColumn),
            854 => Some(Self::M37TabDfShow),
            855 => Some(Self::M37TabReadCsv),
            856 => Some(Self::M37TabWriteCsv),
            857 => Some(Self::M37TabFromSql),
            858 => Some(Self::M37TabFromRows),
            859 => Some(Self::M37TabColI64Eq),
            860 => Some(Self::M37TabColI64Gt),
            861 => Some(Self::M37TabColI64Lt),
            862 => Some(Self::M37TabColF64Eq),
            863 => Some(Self::M37TabColF64Gt),
            864 => Some(Self::M37TabColF64Lt),
            865 => Some(Self::M37TabColStrEq),
            866 => Some(Self::M37TabColStrContains),
            867 => Some(Self::M37TabMaskAnd),
            868 => Some(Self::M37TabMaskOr),
            869 => Some(Self::M37TabMaskNot),
            870 => Some(Self::M37TabMaskCountTrue),
            871 => Some(Self::M37TabDfFilter),
            872 => Some(Self::M37TabDfSelect),
            873 => Some(Self::M37TabDfDrop),
            874 => Some(Self::M37TabDfHead),
            875 => Some(Self::M37TabDfTail),
            876 => Some(Self::M37TabDfRow),
            877 => Some(Self::M37TabDfSortBy),
            0xFFFF_FFFF => Some(Self::Unknown),
            _ => None,
        }
    }

    /// Look up a native id by name. Used by the IR lowerer when it sees a
    /// call to a prelude-registered symbol.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "println"     => Some(Self::Println),
            "print"       => Some(Self::Print),
            "len"         => Some(Self::Len),
            "range"       => Some(Self::Range),
            "assert"      => Some(Self::Assert),
            "abs"         => Some(Self::Abs),
            "min"         => Some(Self::Min),
            "max"         => Some(Self::Max),

            "str"         => Some(Self::StrFromAny),
            "bool"        => Some(Self::BoolFromAny),
            "i32"         => Some(Self::I32FromF64),     // primitive ctor; codegen disambiguates by arg type
            "i64"         => Some(Self::I64FromI32),
            "f64"         => Some(Self::F64FromI64),
            "char"        => Some(Self::CharFromI32),

            "open"        => Some(Self::IoOpen),
            // io.File methods
            "read"        => Some(Self::FileRead),
            "write"       => Some(Self::FileWrite),
            "close"       => Some(Self::FileClose),

            // M35 P4-A: `Pattern(handle)` constructor.  Users go
            // through `re.compile()` which is the canonical entry
            // point, but in case anything wires the bare class name
            // through `from_name` (the IR's is_native constructor
            // path does), route it to the PatternCtor handler.
            "Pattern"     => Some(Self::PatternCtor),

            // Channel methods (also covers method calls on Channel[T])
            "send"        => Some(Self::ChannelSend),
            "recv"        => Some(Self::ChannelRecv),
            "try_recv"    => Some(Self::ChannelTryRecv),

            // Thread methods
            "start"       => Some(Self::ThreadStart),
            "join"        => Some(Self::ThreadJoin),

            // math
            "sqrt"        => Some(Self::MathSqrt),
            "sin"         => Some(Self::MathSin),
            "cos"         => Some(Self::MathCos),

            // list/dict ops by method name
            "append"      => Some(Self::ListAppend),
            "get"         => Some(Self::DictGet),
            "has"         => Some(Self::DictHas),
            "keys"        => Some(Self::DictKeys),
            "values"      => Some(Self::DictValues),
            "slice"       => Some(Self::StrSlice),

            // real-world: csv_aggregate — str→number parsing.
            "parse_f64"   => Some(Self::F64FromStr),
            "parse_i64"   => Some(Self::I64FromStr),

            // real-world: every text-processing stress test wants split.
            "split"       => Some(Self::StrSplit),

            // real-world: stress tests producing ranked output. Note that
            // `from_name("sort")` is invoked through resolve_native_method
            // (method-call path) and `from_name("sorted")` through the
            // top-level lower_call path.
            "sort"        => Some(Self::ListSort),
            "sorted"      => Some(Self::ListSorted),
            // real-world: fix — `xs.pop()` lowered through the method
            // dispatcher. The receiver is implicit (the list pointer),
            // so the IR appends it as the first argument before the call.
            "pop"         => Some(Self::ListPop),

            // M32: Future[T] method dispatch (special-cased to
            // `AsyncioFutureAwait` / `AsyncioFutureIsReady` in the IR via
            // `resolve_native_method`, but having `from_name` know about
            // them keeps the table consistent for any caller that walks
            // method names by string).
            "await"       => Some(Self::AsyncioFutureAwait),
            "is_ready"    => Some(Self::AsyncioFutureIsReady),

            _ => None,
        }
    }
}
