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

            _ => None,
        }
    }
}
