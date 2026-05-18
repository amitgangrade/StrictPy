# M20a — `os` / `path` / `io` stdlib modules

**Brief**: Ride on top of M19's stdlib-module-table infrastructure
to ship three more modules — environment + filesystem (`os`),
pure path manipulation (`path`), and stdin/stdout/stderr line IO
(`io`) — without touching the resolver/typecheck/IR architecture
that landed in M19. Each new module is one entry in
`seed_stdlib_modules` plus a handful of per-item native handlers
in `vm/src/builtins.rs`.

**Wall-clock**: ~2 hours (read-through, three module
registrations, ~250 LOC of native handlers, 20 in-process tests +
9 subprocess tests + 4 example programs + spec).
**Files changed**: 5 source files + 1 spec section + 1 agent report
+ 4 new examples + 5 new test files.
**Tests**: 285 baseline (M19) + **29 new** (20 in-process + 9
subprocess) = **314 passing, 0 failing, 1 ignored**.

## The smooth ride: M19 paid the design tax

M19's report flagged the four-intercept-point `Attr + (` parser-fold
gotcha and built a `StdlibModuleTable` shape generic over modules.
M20a confirmed that bet: zero changes to the resolver's import
resolution, zero changes to `typecheck.rs` apart from one
"legacy fall-through" tweak (see below), zero changes to the IR
lowering paths. Three new modules slot in with one `seed_stdlib_modules`
call and one match arm per native id. The full diff for adding the
`os` module is **~110 lines** in `resolver.rs` + **~120 lines** of
handlers in `builtins.rs` + 12 enum variants in `shared/src/native.rs`.

## The one snag: the legacy `io` BuiltinModule

Pre-M19 the prelude registered `io` as a `BuiltinModule` symbol so that
`io.File` would resolve under the joined prelude name `"io.File"`
(M5-era). M19 added the module-attr lookup that goes through the
`stdlib_modules` table *first*; when the new `io` module appeared in
that table (with no `File` entry — `File` is a class, not a
`NativeFn`), `io.File` started erroring with "module `io` has no
attribute `File`" before the legacy joined-name fall-through could
trigger.

The fix is a four-line reorder in `typecheck.rs::Expr::Attr`: if the
stdlib module exists but the item isn't found, try the legacy
flattened-name path *before* erroring. The IR lowerer didn't need
any change — it already falls through to the standard `Load(offset)`
path when no stdlib item matches.

The same shape exists in `Expr::MethodCall` — but I couldn't find any
program using `io.X(...)` as a direct call (every legacy use is
`io.File.read()` on a value-typed receiver, which doesn't hit this
path). I patched it anyway for defensive symmetry.

## Native ID block layout (M20a takes 140–179)

| Range | Module | Count |
|---|---|---|
| 140–151 | `os` | 12 |
| 160–165 | `path` | 6 |
| 170–174 | `io` | 5 |

`os` got 30 slots reserved (140–159) but only 12 used — leaves room
for `os.copy`, `os.rename`, `os.symlink`, etc. without renumbering.
Same idea for `path` (160–169, 6 used) and `io` (170–179, 5 used).

## OS-syscall mapping

| StrictPy native | Rust call |
|---|---|
| `os.env(key)` | `std::env::var(key)` |
| `os.set_env(k, v)` | `std::env::set_var(k, v)` |
| `os.getcwd()` | `std::env::current_dir()` |
| `os.chdir(p)` | `std::env::set_current_dir(p)` |
| `os.listdir(p)` | `std::fs::read_dir(p)` |
| `os.remove(p)` | `std::fs::remove_file(p)` |
| `os.mkdir(p)` | `std::fs::create_dir(p)` |
| `os.exists(p)` | `std::path::Path::exists` |
| `os.is_file(p)` | `std::path::Path::is_file` |
| `os.is_dir(p)` | `std::path::Path::is_dir` |
| `os.read_file(p)` | `std::fs::read_to_string(p)` |
| `os.write_file(p, c)` | `std::fs::write(p, c)` |
| `path.join(a, b)` | `Path::new(a).join(b)` |
| `path.dirname(p)` | `Path::new(p).parent()` |
| `path.basename(p)` | `Path::new(p).file_name()` |
| `io.input()` | `io::stdin().lock().read_line(...)` |
| `io.flush_stdout()` | `io::stdout().lock().flush()` |
| `io.write_stderr(s)` | `io::stderr().lock().write_all(...)` |

Every fallible call gets the M15 IOError translation pattern (lifted
from the existing `IoOpen` native):

```rust
.map_err(|e| VmError::UncaughtException {
    type_name: "IOError".into(),
    message: format!("listdir({:?}): {}", path, e),
})?;
```

## Cross-platform decisions

**Path separator** — `path.sep` returns `"/"` on Unix, `"\\"` on
Windows via `cfg!(windows)`. `path.join` delegates to
`std::path::Path::join` so the output is OS-native regardless of
input. The subprocess tests for `list_dir` and `file_stats` assert on
substrings (e.g. `"Cargo.toml"`, `"basename : Cargo.toml"`) that don't
depend on which separator joins the parent.

**Line endings** — `io.input()` strips both `\n` and `\r\n`. This
matters because Windows pipes deliver `\r\n` and CI bots on
both Linux and Windows pipe lines into stdin to drive
`echo_interactive.spy`.

**Env-var case sensitivity** — `std::env::var` preserves whatever the
host gives. Windows env vars are case-insensitive at the OS level, so
`os.env("path")` returns the same value as `os.env("PATH")` there. The
test sets `STRICTPY_M20A_TEST_VAR` and reads back the exact case —
works the same on both platforms.

**`splitext` semantics** — I went with Python's rule: a leading dot
isn't an extension (`.bashrc` → `(".bashrc", "")`). Rust's
`Path::extension()` agrees on that case but disagrees on
`"file.tar.gz"` (Rust returns `"gz"`, Python returns `".gz"` — both
correct, just different conventions). I rolled a hand-coded
`splitext_python` to nail down the Python semantics; six lines of
`rfind('.')` plus a "skip leading dots" check.

## The tuple-from-native dance

`path.splitext` returns a `(str, str)` tuple. The IR-side tuple
allocation uses `Alloc(class_id)` because the per-program tuple type
table records a `type_id` for each distinct shape at compile time.
Native code can't see that table, so I added an `alloc_tuple_obj`
helper on `Interpreter` that allocates `HDR + N*8` bytes with a
**null** type pointer and `GcKind::Class`. The GC scans that as a
uniform sequence of 8-byte slots — exactly the right behaviour for
both elements (heap string pointers).

The IR-side `Load(offset)` doesn't consult the type pointer at all —
it just dereferences `obj + HDR + offset`. So the tuple round-trips
through `t.0` / `t.1` cleanly even with no runtime type metadata.

## The `??` (null-coalesce) snag

While writing tests for `os.env` I hit a long-standing v0.2 bug:
`expr ?? fallback` lowers to `Copy(fallback)` regardless of whether
`expr` is `none` (see `ir.rs::Expr::NullCoalesce` — it doesn't emit
the branch). I worked around it in the test (`if v is none`) and in
`examples/env_dump.spy` (same pattern). Worth filing a separate
bug — it's not new to M20a but M20a is the first milestone where
nullable-returning natives are a thing user code wants to chain. Left
for the orchestrator to catalogue.

## Example programs

1. **`list_dir.spy`** — `os.getcwd` + `os.listdir(".")`. Test asserts
   the project root listing contains `Cargo.toml` and `examples`.
2. **`env_dump.spy`** — read `PATH`, split by `:` or `;`. Test sets
   a synthetic PATH so the assertion is portable.
3. **`file_stats.spy`** — composes `sys.argv`, `os.exists`,
   `os.is_file`/`is_dir`, `path.dirname`/`basename`/`splitext`. One
   tuple destructure of `splitext`'s `(str, str)` return. Test feeds
   it `"Cargo.toml"` and asserts each report line.
4. **`echo_interactive.spy`** — `io.input_with_prompt("> ")`. Test
   pipes `"hello stdin\n"` into the subprocess's stdin and asserts
   both the prompt `"> "` and the echo appear in stdout.

(The stretch goal `grep.spy` I skipped — it needs reading stdin to
EOF in a loop, and `io.input()` raises on EOF rather than returning
`none`, which would force a try/except inside the loop. Cleaner once
v0.3 adds an `io.try_input() -> str?` companion.)

## Hardest three things (in retrospect)

1. **The legacy `io.File` resolution path.** Took ten minutes of
   confused debugging before I realised the prelude `io` symbol pre-
   exists and that adding a real `io` stdlib module shadows the legacy
   joined-name resolution. The fix was small but I had to read three
   paths of `typecheck.rs` carefully.
2. **Returning a tuple from a native function.** I almost gave up and
   reshaped `splitext` to return two parallel functions
   (`without_ext(p)` / `extension(p)`) — much uglier and a regression
   from Python. The `alloc_tuple_obj` helper (null type pointer +
   `GcKind::Class`) sidesteps the whole runtime-type-table problem
   and matches the IR's `Load(offset)` access pattern.
3. **Forcing the test for `io.input_with_prompt` to be hermetic.**
   `std::process::Command` + `Stdio::piped()` + manually closing the
   stdin handle after write so the program sees EOF cleanly. The pipe
   closes when `wait_with_output` drops the `Child`, but only if you
   `drop(stdin)` first — the trick was scoping the `stdin` borrow.

## What's next

Per the orchestrator's M20 batch: `json`, `re`, `time`, `random`,
`math+` are siblings of this work — each one is its own
`seed_stdlib_modules` entry plus the per-native handlers, no new
infrastructure needed. The hardest of those will be `re` (a real
regex engine vs. a hand-rolled one — probably `regex` crate behind a
feature flag) and `json` (whether to parse into a `Map[str, Any]`
which v0.2 doesn't really have, or a tagged `JsonValue` sealed class
the way `producer.spy` does). `time` and `random` are mechanical.
