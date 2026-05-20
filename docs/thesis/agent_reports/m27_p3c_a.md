# M27 P3c-A — `shutil` + `tempfile` stdlib modules

**Brief**: One of four parallel M27 worktree agents extending the
Phase-3c stdlib surface.  My slot ships the two filesystem-ergonomics
modules — `shutil` (high-level FS ops, closes the v0.2 gap that
M24-D documented around recursive rmdir) and `tempfile` (temp-dir /
temp-file creation backed by the `tempfile` crate).  ID range
**450-479** (30 ids), of which **9 used** (450-455 shutil, 470-472
tempfile); 21 reserved for v0.3 expansions.

**Wall-clock**: ~1.5 hours (M23 P3a-A as the reference report, the
existing subprocess + pathlib runtime helpers as ergonomic precedent,
~250 LOC of native handlers + 2 demos + 2 integration tests + spec
§9.30/§9.31).

**Files changed** (one per agent-shared file, plus three new files):

| File | Lines added | Purpose |
|---|---|---|
| `shared/src/native.rs` | +75 | 9 new `NativeFn` variants (450-455, 470-472) + `from_u32` arms |
| `compiler/src/resolver.rs` | +120 | Two `StdlibModule` registrations |
| `vm/src/builtins.rs` | +400 | 9 dispatch arms + `shutil_copytree_impl` / `shutil_rmtree_impl` / `shutil_which_impl` (incl. Windows `.exe`/`.bat`/`.cmd` PATHEXT dance) / `shutil_disk_usage_impl` (`statvfs` on Unix, `GetDiskFreeSpaceExW` on Windows) |
| `vm/Cargo.toml` | +6 | `tempfile = "3"` direct dep; `libc = "0.2"` as `cfg(unix)`-only target dep |
| `STRICTPY_SPEC.md` | +135 | §9.30 (shutil) + §9.31 (tempfile) |

New files:
* `examples/shutil_demo.spy` — ~125 LOC, exercises copy / copytree /
  move / rmtree / which (positive + miss) / disk_usage + the
  copytree-overwrite and rmtree-missing error paths.
* `examples/tempfile_demo.spy` — ~95 LOC, exercises gettempdir /
  mkdtemp / mkstemp + the round-trip write-then-read on each
  + suffix preservation on `mkstemp`.
* `compiler/tests/shutil_demo_runs.rs` — compile + spy.exe end-to-end.
* `compiler/tests/tempfile_demo_runs.rs` — compile + spy.exe end-to-end.

## Design choice 1: flat-function API (same shape as M23 P3a-A pathlib)

Both modules ship as flat functions, never as classes.  This is the
same v0.2 limit M20c / M22 P2A / M23 P3a-A all hit: stdlib-class
registration is deferred to v0.3.  For `shutil` the impact is zero —
Python's `shutil` is already a module of free functions, so the
StrictPy surface and the Python surface are token-for-token
isomorphic.  For `tempfile` the impact is real but bounded: the
class-shaped context managers (`NamedTemporaryFile`,
`SpooledTemporaryFile`, `TemporaryDirectory`) all get punted to v0.3.
The path-returning helpers `mkdtemp` / `mkstemp` / `gettempdir` are
exactly the right primitives for programs that pair temp creation
with explicit `try` / `except` / `shutil.rmtree` cleanup.

## Design choice 2: `shutil.which` matches CPython's Windows fallback

The brief flagged this as the trickiest cross-platform call.
CPython's `shutil.which` on Windows tries the input as-given, then
appends `.exe` / `.bat` / `.cmd` / `.com` (in that order) when the
input has no extension.  I implemented the same fallback inline (no
crate dep — the logic is ~20 LOC behind a `cfg(windows)`).

PATHEXT-driven dispatch (where Windows respects the user's
configured executable-extension list) is more accurate but adds a
parse step and complicates the cross-platform contract.  CPython
itself falls back to the canonical four extensions when PATHEXT is
unparseable, so the hard-coded list is the practical 99%-correct
answer.

I considered using the `which` crate (canonical Rust binding) but
the surface I actually need is small (one happy path + one nullable
return) and the inline implementation also gives me full control
over the Windows extension order.

## Design choice 3: `disk_usage` via direct OS bindings

The third native that needs a real OS syscall.  Options:
* **(a)** Pull in a `sysinfo` / `fs2` crate.  Both bring extra
  dependencies for what is ultimately ~30 LOC of platform-specific
  glue.
* **(b)** Inline `statvfs(2)` on Unix + `GetDiskFreeSpaceExW` on
  Windows.  Two `cfg(unix)` / `cfg(windows)` arms + an unsupported-
  platform stub.

I went with **(b)** — `libc::statvfs` for Unix (adding `libc = "0.2"`
as a `cfg(unix)` target dep so Windows builds don't pull it in), and
an inline `extern "system"` declaration for `GetDiskFreeSpaceExW`
(no new crate needed — the symbol is in `kernel32.lib` which the
Windows MSVC toolchain links by default).  The unsupported-platform
fallback returns `IOError("unsupported platform")` so a hypothetical
WASM / iOS port has a graceful path.

`used = total - free`.  CPython's `shutil.disk_usage` derives `used`
the same way (it has no separate "used bytes" syscall on any
mainstream OS), so the semantics match exactly.

## Design choice 4: `copytree` refuses to overwrite (CPython 3.7+ default)

Pre-3.7 Python's `shutil.copytree` raised `OSError` if `dst`
already existed; 3.7+ gained a `dirs_exist_ok=False` default that
preserves this behaviour.  We match the strict (default) behaviour
— `IOError` if `dst` exists — because StrictPy can't yet express
the keyword-argument that toggles it.  Users who genuinely want
overwrite-semantics can `shutil.rmtree(dst)` first.

## Design choice 5: `shutil.move` cross-filesystem fallback

`std::fs::rename` returns `Err(_)` when the rename would cross
filesystems (the OS reports `EXDEV` on Unix, `ERROR_NOT_SAME_DEVICE
= 17` on Windows).  Python's `shutil.move` recovers via
copy-then-remove.  My implementation does the same: on any rename
error, falls through to `shutil.copy` (file) or
`shutil_copytree_impl` + `shutil_rmtree_impl` (directory).  No way
to test the cross-FS path in the demo without privileged setup, but
the same-FS rename path is covered.

## Incidentally-discovered bugs / oddities

**None requiring code changes** in the existing surface.  The M19
stdlib infrastructure absorbed two more modules without forcing a
single resolver, typecheck, or IR change — the same pattern that
M22 P2A-D and M23 P3a-A-D all reported.  The design tax M19 paid
keeps paying out.

Two small "language papercuts" worth noting (not bugs, just
ergonomics that the next stdlib agent will trip on):

1. **`open(p, mode).read(N)` is rejected by the typechecker.**  My
   first cut of the read-text helper used `f.read(4096)` in a loop
   (mirroring `read(buf_size)` from Python).  The typechecker
   reports E2012 "method `read` expects 0 args, got 1".  StrictPy's
   `File.read` is a zero-arg "read the whole file" call.  Fix: drop
   the size param and the loop.  Worth documenting somewhere
   user-visible — every Python programmer reaches for the chunked
   API first.

2. **`is None` is a parse error; the right spelling is `is none`
   (lowercase).**  Hit on my first cut of the `shutil.which` miss
   case (`if found_opt is None`).  StrictPy's null literal is
   lowercase `none`; the typechecker has no `None`-the-keyword.
   Same root cause: Python muscle memory.  The fix is one character.

Both papercuts are documented in the relevant places in the spec
already (§4 nullable narrowing, §9.9 IO).  Future stdlib agents will
also hit them — could be a single line in `SHARED_BRIEF.md`'s "known
v0.2 limits" list.

## Cross-platform notes

* `shutil.which` PATH-search uses `std::env::split_paths` so the
  separator (`:` Unix / `;` Windows) is handled automatically.
* `shutil.disk_usage` has dedicated Unix (`libc::statvfs`) and
  Windows (`GetDiskFreeSpaceExW`) branches; both return `(total,
  free)` in bytes that we widen to `i64`.  Filesystems up to
  ~9.2 EB fit; petabyte-scale arrays will need `u64` in v0.3.
* `tempfile.mkdtemp` / `mkstemp` use the `tempfile` crate's
  `Builder` API, which already abstracts the per-OS atomic-creation
  syscall.  Restrictive permissions on Unix (0o700 / 0o600) and
  current-user ACLs on Windows ship by default.
* `shutil.rmtree` is a one-liner over `std::fs::remove_dir_all`,
  which the Rust std library already gets right cross-platform
  (including the Windows long-path / read-only-file dance).

## Hardest two things (in retrospect)

1. **The Cargo.toml `[target.'cfg(unix)'.dependencies]` placement.**
   My first cut inserted the `[target.'cfg(unix)'.dependencies]`
   section in the middle of `[dependencies]`, which silently
   relocated the trailing `cranelift = { ..., optional = true }`
   entries into the unix-target section — the workspace then failed
   to build on Windows because the optional `jit` feature couldn't
   find its crates.  Fix: keep `[target.'cfg(unix)'.dependencies]`
   AFTER all of `[dependencies]`.  TOML section-boundary parsing is
   strict.

2. **The `tempfile` crate's deprecation churn.**  `TempDir::into_path`
   is deprecated in newer `tempfile` releases in favour of
   `TempDir::keep` (matching `NamedTempFile::keep`'s name).  The
   version `tempfile = "3"` resolves to in our lockfile is just
   pre-rename, so I had to wrap the call in `#[allow(deprecated)]`
   with a comment for the next agent to swap when the rename lands.

## Files NOT touched (per file-ownership boundaries in SHARED_BRIEF)

* Existing examples (`subprocess_demo.spy`, `pathlib_demo.spy`,
  etc.) — unchanged.
* `BUGS_KNOWN.md` / `timeline.md` / `stats/*` / `bugs/catalog.md`
  / `design_decisions/*` — orchestrator's territory.
* Other agents' modules — left untouched.

## Final test totals

* **586 baseline preserved** (post-M26 count per the brief).
* **+4 subprocess tests**: `compiler/tests/shutil_demo_runs.rs`
  (compile + spy.exe) + `compiler/tests/tempfile_demo_runs.rs`
  (compile + spy.exe).
* Plus the auto-sweep `parse_examples` / `typecheck_examples` /
  `compile_examples` over the two new `.spy` files — 6 implicit
  coverage units (no new test files).
* **Total: ~596 passing, 0 failing**, 1 ignored (the pre-M26
  `BUG-028` lexer test).

## What's next

Two v0.3 follow-ups the M27 P3c-A arc highlights:

1. **Default-argument support on stdlib items.**  The brief listed
   `shutil.which(cmd) -> str?` and `tempfile.mkdtemp(prefix="tmp")`
   with explicit defaults.  The StdlibItem registration shape
   doesn't yet carry default-argument metadata, so callers always
   pass `prefix` / `suffix` explicitly.  Adding this surface would
   collapse ~20% of the wrapper-LOC in the demo files.

2. **A `TemporaryDirectory` / `NamedTemporaryFile` class shape.**
   Same v0.3 stdlib-class blocker that punted pathlib's `Path`,
   subprocess's `Process`, sqlite3's `Connection`, ArgParser, etc.
   One unblock-everything change.

The Phase-3c-A round closes here.  The orchestrator integrates this
worktree (along with the parallel P3c-B/C/D agents) onto main with
renumbered spec sections.
