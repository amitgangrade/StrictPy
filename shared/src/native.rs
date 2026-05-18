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
