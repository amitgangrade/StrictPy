# M19 — import resolution + `sys` module

**Brief**: The parser has accepted `import` / `from ... import` since
M0 — the AST has carried `Module::imports` since the very first commit
— but every milestone before M19 either ignored those nodes or
flattened the imported names into the prelude (`from threading import
Channel` worked only because the prelude already bound `Channel`).
M19 is the first milestone where the resolver, typechecker, and IR
lowerer wire imports through to a real module table, where attribute
access (`sys.argv`) and module-namespaced calls (`sys.exit(0)`)
dispatch through `CALL_NATIVE`, and where the CLI plumbs trailing
args from `clap` into the program as a real `List[str]`.

**Wall-clock**: ~3 hours (read-through + module-table design +
typecheck attr / method dispatch + IR lowering + VM `Exit` variant +
CLI argv plumbing + four new examples / 18 tests + spec/docs).
**Files changed**: 10 source files + 4 new test files + 3 new
examples + 1 spec update + 1 agent report.
**Tests**: 267 baseline + **18 new** (10 M19 inline + 8 example
subprocess) = **285 passing, 0 failing**.

## The design choice: built-in stdlib table, not a `.spy` loader

The brief offered two paths:

1. **Built-in table** — register every stdlib symbol inside the
   resolver, with its static type and a `NativeFn` discriminant. The
   stdlib is part of the compiler, not a separate artefact.
2. **`.spy` modules** — ship the stdlib as a bunch of `.spy` files,
   compile them into bytecode, bundle them with the VM, and resolve
   imports by name against an on-disk module cache.

The brief recommended path (1) for v0.2 and I followed it. The reasons
are visible in the diff: the new `StdlibModuleTable` is **35 lines** in
`resolver.rs`, the seed routine is another 50 lines, and *everything
else* — typecheck, IR, VM — got tiny three-to-twelve-line patches.
Path (2) would have needed a packaging story (where do `.spy` files
live? bundled into the VM binary? a side directory? what about
versioning across `spy.exe` and the stdlib?), a multi-module
compilation pipeline (one .spy → many .spyc), and resolution against
a *non*-prelude scope. Path (2) is what user-defined modules will
need eventually (v0.3 work), but doing it just for `sys` would be
massive yak-shaving.

The key shape:

```rust
pub struct StdlibItem {
    pub name: String,
    pub kind: StdlibItemKind,   // Const | Function
    pub ty: Ty,                  // static type for typecheck
    pub native_id: u32,          // NativeFn::* discriminant
}

pub struct StdlibModule { pub name: String, pub items: Vec<StdlibItem> }
```

Resolver populates a `HashMap<String, StdlibModule>` at the start of
`resolve()`, before `seed_prelude` (so prelude flat-names still win
for legacy stdlibs). `import sys` introduces a `BuiltinModule` symbol
and records the mapping `sym_id → module_name` in
`module_alias` so `import sys as s` and `import sys` both route to the
same table entry. `from sys import argv` introduces a top-level
symbol *with the item's full type baked in* and records `sym_id →
StdlibItem` in `import_item` so the IR lowerer can look up the
native id later.

## `sys.argv` lazy materialisation

`sys.argv` is a value-typed `List[str]`, lazily allocated on first
read and cached. The cache lives on `Interpreter`:

```rust
pub(crate) sys_argv_cache: Option<u64>,   // heap pointer
```

`NativeFn::SysArgv` checks the cache first; if absent it pulls the
already-set `interp.argv: Vec<String>` and walks it building
`alloc_list` + `alloc_string` + `list_push` exactly as `StrSplit`
does (so the GC sees identical heap shapes). The cached pointer means
`sys.argv is sys.argv` is true (relevant for identity comparisons
and for any `sys.argv.append(...)` mutation to be visible on the next
read).

CLI plumbing: `vm/src/main.rs` used to have a `let _ = &cli.args;`
"TODO(spec)". The full path is now:

```
spy.exe foo.spyc alpha beta
        ↓ clap (trailing_var_arg)
        ↓ run_file_with_args(path, ["alpha", "beta"])
        ↓ Interpreter::set_argv(["foo.spyc", "alpha", "beta"])
        ↓ argv = [path] ++ args
        ↓ Interpreter::argv (Vec<String>)
        ↓ NativeFn::SysArgv first reader
        ↓ List[str] on the heap; cached
```

`argv[0]` is the `.spyc` path matching Python's convention. Tests with
no trailing args see `argc=1` (just the program path); tests with
trailing args see them at indices 1..n.

## `sys.exit` is *not* an exception

The other interesting design call was how `sys.exit(N)` interacts with
`try ... except`. Python's rule is that `SystemExit` derives from
`BaseException`, **not** `Exception`, so a bare `except Exception:`
won't catch it. StrictPy doesn't yet have the BaseException/Exception
distinction (M15 only ships `Exception` and its concrete subclasses),
so I went one rung up: `sys.exit` doesn't go through the exception
machinery *at all*.

Concretely there's a new VM error variant:

```rust
pub enum VmError {
    ...
    Exit(i32),
}
```

`NativeFn::SysExit` returns `Err(VmError::Exit(code))`. The
interpreter's `propagate_exception` loop only ever fires on
`VmError::UncaughtException`; an `Exit` walks straight up the call
stack past every handler frame and lands in `run_file_with_args`,
which has a fresh arm:

```rust
match interp.run_main() {
    Ok(code) => Ok(code),
    Err(VmError::Exit(code)) => Ok(code),
    Err(e) => Err(e),
}
```

The non-catchability is exercised by `sys_exit_is_not_caught_by_except_exception`,
which puts `sys.exit(5)` inside `except Exception as e:` and asserts
the program (a) exits with code 5, (b) does not print "caught:", and
(c) does not reach the post-`try` line.

## Files modified + LOC (approximate)

| File | Lines added | Purpose |
|---|---|---|
| `shared/src/native.rs` | +20 | Four new `NativeFn` variants (ids 130–133) |
| `compiler/src/error.rs` | +3 | `LINK_NO_SUCH_MODULE_ITEM = E4002` |
| `compiler/src/resolver.rs` | +180 | `StdlibModule(Item|Kind)`, `seed_stdlib_modules`, import-resolution rewrite |
| `compiler/src/typecheck.rs` | +60 | Module-attr lookup + MethodCall short-circuit on builtin-module receivers |
| `compiler/src/ir.rs` | +95 | Ident-from-import, Attr-on-module, Call-with-Attr, MethodCall-on-module — four lowering paths |
| `vm/src/error.rs` | +10 | `VmError::Exit(i32)` variant |
| `vm/src/interp.rs` | +20 | `argv: Vec<String>`, `sys_argv_cache: Option<u64>`, `set_argv` |
| `vm/src/builtins.rs` | +55 | Four `NativeFn::Sys*` handlers |
| `vm/src/lib.rs` | +35 | `run_file_with_args`, `run_file_capture_with_args` |
| `vm/src/main.rs` | +4 | Call new entry point with `cli.args` |
| `STRICTPY_SPEC.md` | +75 | §6.7 "Imports and modules" + §9.6 "Module sys" |

Plus the new examples and tests:

* `examples/echo.spy` — minimum-viable `sys.argv` consumer.
* `examples/sum_args.spy` — parse argv as i64, sum, exit(1) on parse error.
* `examples/print_env.spy` — banner using `version`, `platform`, `argv`; alias form.
* `compiler/tests/{echo,sum_args,print_env}_runs.rs` — eight subprocess assertions across `spy.exe`.
* `vm/tests/m19_sys_module.rs` — ten in-process tests for the resolver + typecheck + VM dispatch.

## What the three example programs demonstrate

1. **`echo.spy`** — proves the `argv → spy.exe → VM → List[str]`
   pipeline is intact end-to-end. The subprocess test invokes
   `spy.exe echo.spyc alpha beta gamma` and asserts all four argv
   lines (`argv[0..3]`) appear with the right values.
2. **`sum_args.spy`** — exercises three things at once: `sys.argv`
   indexing, `parse_i64` integration (M10's str→i64 native), and the
   non-catchable `sys.exit(1)` on parse failure. Confirms that a
   diagnostic message printed *before* `exit(1)` makes it to stdout
   while the post-`exit` "sum = ..." line doesn't.
3. **`print_env.spy`** — three module-attr reads in one program and
   the `import sys as s` alias form. The integration test asserts
   the version banner is the pinned `"StrictPy v0.2"` string and that
   the platform value is one of four legal strings.

## Hardest three things

1. **The `Attr + (` parser fold.** My first attempt at typecheck and
   IR special-cased `Expr::Call { callee: Expr::Attr { ... } }`.
   Tests for `import sys; println(sys.platform)` passed instantly but
   tests for `sys.exit(0)` failed with "unknown name `sys`" at the
   *typecheck* layer. The parser's `parse_postfix` (line 1434) eagerly
   folds an `Expr::Attr` immediately followed by `(` into an
   `Expr::MethodCall`. So `sys.exit(0)` is *never* a `Call` whose
   callee is an `Attr`; it's a `MethodCall { receiver: Ident("sys"),
   method: "exit", args: [0] }`. Both `synth_call` and `lower_call`
   need the module-namespace handling, *and* so do
   `synth_expr_inner::Expr::MethodCall` and `lower_method_call`. Four
   intercept points instead of two. The intercepts go *before*
   `synth_expr(receiver)` / `lower_expr(receiver)`, otherwise the
   receiver path hits the builtin-module placeholder and gives
   inscrutable downstream errors.

2. **Where to put `module_alias`.** The first cut had `import sys`
   register the symbol under the literal name `"sys"` and rely on
   that for module-table lookup. But `import sys as s` registers under
   `"s"`, and the table only knows about `"sys"`. The minimum solution
   is a sym_id → mod_name side map (resolver-owned, exposed on
   `ResolvedModule.module_alias`). The typechecker and IR both
   consult it to recover the canonical module name from a symbol. Five
   lines of code, fifteen minutes of debugging when I forgot to
   update the typecheck path.

3. **Argv ownership and lifetime.** The first `SysArgv` impl I wrote
   borrowed `&interp.argv` while calling `interp.alloc_string` —
   classic Rust double-borrow. The fix is just `interp.argv.clone()`
   up front (the strings are small, one-shot allocation per program
   run, and the clone happens at most once because the result is
   cached). Worth noting because future stdlib agents adding similar
   "read interpreter state then alloc heap stuff" natives will hit
   the same wall.

## Incidentally-discovered bugs / oddities

* **None requiring code changes.** The existing M15
  `propagate_exception` loop pattern-matches only on
  `VmError::UncaughtException`, so adding a new non-exception variant
  for `Exit` Just Worked without any defensive guards.
* Documentation gap noted but not fixed: `parse_examples.rs`,
  `typecheck_examples.rs`, and `compile_examples.rs` all sweep
  `examples/*.spy` automatically, so the three new files were already
  under test as soon as I created them. Worth calling out for future
  agents: dropping a file into `examples/` exercises five test
  binaries, not zero.
* `compile_examples.rs` was the first test to surface my early IR
  bugs because it walks every example through `compile_source` end-
  to-end before any inline test fires. The fact that all 3 new
  programs compiled on the first try after the IR fix means the
  builtin-module + import-item paths cover the surface area cleanly.

## What's next

M19 is the foundation. M20's parallel batch (`os`, `os.path`, `io`,
`json`, `re`, `time`, `random`, `math+`) plugs straight into this
table: each new module is one `seed_stdlib_modules`-style entry plus
a per-item native handler. The hardest call there will be `io` —
StringIO/BytesIO and friends want a richer `File`-class story than
the current native handle. Everything else is mechanical.

User-defined modules and submodules are v0.3 work and will need a
real loader, a per-module symbol table, and an answer to "where do
`.spyc` files for stdlib live on disk." Punted intentionally.
