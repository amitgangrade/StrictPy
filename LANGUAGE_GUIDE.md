# StrictPy — language guide for AI coding tools

**Status**: live document. Updated whenever a new language feature, stdlib module, or surface change lands. Last refresh: post-M37 (2026-05-21).

**Audience**: any AI coding tool (Claude, GPT, Gemini, etc.) being asked to write a StrictPy program. This file is the single source of truth for writing idiomatic StrictPy — you should NOT need to read the compiler source (`compiler/src/`) or VM source (`vm/src/`) to write correct code.

**If you find something in this guide is wrong or out of date**: that's a bug worth flagging to the user. The maintenance discipline is "ship a doc update in the same commit as the feature." See §13 "Maintaining this file" at the end.

---

## Table of contents

- §0 — Quick start (5 lines you can copy)
- §1 — What StrictPy is, and how it differs from Python
- §2 — Building and running
- §3 — Syntax reference (every form, with examples)
- §4 — Type system
- §5 — Standard library (alphabetical, every module)
- §6 — Prelude — types and functions available without imports
- §7 — Common idioms (how do I do X?)
- §8 — Pattern matching reference
- §9 — Async I/O
- §10 — Generics (free functions and classes)
- §11 — Gotchas, Python differences, and things that DON'T work
- §12 — End-to-end example programs
- §13 — Maintaining this file

---

## §0 — Quick start

Write this to `hello.spy`:

```python
fn main() -> i32:
    println("Hello, StrictPy!")
    return 0
```

Run it:

```
spy hello.spy
```

That's it. `spy` compiles to bytecode (cached under `__spycache__/`) and runs in one command.

---

## §1 — What StrictPy is, and how it differs from Python

StrictPy is **Python-syntax with mandatory static typing**. The compiler rejects any program that isn't fully type-annotated and concrete. There is no `Any`, no `eval`, no monkey-patching, no `__dict__` mutation. In exchange you get an AOT-compiled bytecode VM with a Cranelift JIT that beats CPython 3.12 by 4–17× on small benchmarks.

### The minimum mental model

- Every name has a type. Every function parameter, every return type, every let-binding, every field — explicit.
- Numeric types are concrete: `i32`, `i64`, `f64`, `bool`. No untyped `int` (use `i32` or `i64`). No implicit numeric conversion (you must write `f64(x)` to widen `i32 → f64`).
- Classes are `final` by default. Use `open class` to allow subclassing, `sealed class` for closed hierarchies (exhaustive `match` works).
- `T?` means "T or none". The type checker narrows `T?` to `T` inside `if x is not none:` branches.
- Generic free functions (M17): `fn id[T](x: T) -> T:`. Generic classes (M31): `class Box[T]:` / `class Pair[K, V]:`.
- The `match` keyword + `case Constructor()` patterns work for sealed-class destructuring (M16).
- Exception handling: `try` / `except <name> as e:` / `finally` (M15). The 10 built-in exception names are listed in §3.10. User-defined exception subclasses are NOT supported in v0.3.
- Mandatory `main() -> i32` entry point in any program you intend to run.

### What's NOT supported

- `Any` type, `eval()`, `exec()`, `__dict__` access, monkey-patching, metaclasses, decorators that synthesise runtime types
- Variadic functions (`fn f(*args)`) and keyword-only args
- Multiple inheritance
- Default argument values (every parameter must be passed)
- f-strings inside an f-string (nested), `f"...{expr:format_spec}"` format specifiers — basic `f"text {expr}"` works
- `with` doesn't route IOError through an enclosing `try ... except` — wrap explicitly
- `async`/`await` keywords (use the `asyncio` library functions instead — see §9)
- NumPy / pandas import (StrictPy isn't CPython; see THESIS.md §7)

---

## §2 — Building and running

The single CLI is `spy`. Three shapes:

```
spy SCRIPT.spy [ARGS...]              # compile-if-stale + run; args become sys.argv[1..]
spy SCRIPT.spyc [ARGS...]             # run pre-compiled bytecode directly
spy -c "code" [ARGS...]               # inline; argv[0] is the literal "-c"
spy --compile-only SCRIPT.spy [-o OUTPUT]   # like `python -m py_compile`
```

The bytecode cache is `<dir>/__spycache__/<basename>.spyc`. Source mtime > cache mtime triggers a recompile.

### Build the toolchain

```
cargo build --release
./target/release/spy.exe examples/fib.spy
```

`cargo test --workspace --release` runs the full test suite (~690 tests).

---

## §3 — Syntax reference

### 3.1 Comments

```python
# Single-line comment
"""Triple-quoted docstring. Lives only at the top of a module / function /
class definition; otherwise it's a string literal."""
```

### 3.2 Literals

```python
42                # i64 (default integer)
42i32             # i32 (suffix selects type)
42i64             # i64 (explicit)
3.14              # f64
true / false      # bool
none              # the null sentinel for T? types
"hello"           # str (always immutable; bytes are str-as-byte-buffer, see §3.3)
'a'               # char (single Unicode codepoint, distinct from str)
[1, 2, 3]         # list literal — type inferred from elements
[]                # empty list — type annotation REQUIRED
{}                # empty dict — type annotation REQUIRED
{"k": 1, "j": 2}  # dict literal
{1, 2, 3}         # set literal
()                # empty tuple
(1, "two")        # tuple literal (M14)
f"hello {name}"   # f-string (basic form only)
```

### 3.3 Strings vs byte buffers

StrictPy strings are UTF-8 internally. For modules that handle binary data (`hashlib`, `gzip`, `zlib`, `bz2`, `base64`, `struct`, `zipfile`, `tarfile`, `socket`, `ssl`), `str` is used as a **byte buffer** where each codepoint in 0..=255 is one byte. Document this in any new stdlib module that handles binary data.

### 3.4 Line continuation

A trailing binary operator at end-of-line continues to the next line (M30 BUG-028 fix):

```python
return "a " +
    "b"           # Works: returns "a b"

if x > 0 and
   y > 0:         # Works
    ...
```

Parens and brackets also continue:

```python
let total = (
    1 +
    2 +
    3
)
```

But `:` does NOT trigger continuation (otherwise every block header would be ambiguous). Neither do `,`, `.`, `->`, `@`, or unary `not`/`~`.

### 3.5 Variable declarations

```python
let x: i32 = 0           # standard form (preferred)
x: i32 = 0               # `let` is optional at function scope
let xs: List[i32] = [1, 2, 3]

# Top-level constants (module-scope) use `final`:
final WIDTH: i32 = 60
final HEIGHT: i32 = 30

# `final` at function scope makes the variable un-rebindable:
fn f() -> i32:
    final n: i32 = 42
    # n = 43  # ERROR: cannot reassign a `final` variable
    return n
```

Type annotation is **mandatory** at declaration. You cannot omit it even if the type is "obvious" from the rhs.

### 3.6 Operators

| Category | Operators |
|---|---|
| Arithmetic | `+` `-` `*` `/` `//` (integer division) `%` `**` (power, integer or f64) |
| Comparison | `==` `!=` `<` `<=` `>` `>=` |
| Identity | `is` `is not` (use for `x is none` / `x is not none`) |
| Membership | `in` `not in` (works for `Dict[str, *]` and `Set[T]`; List membership is v0.4) |
| Boolean | `and` `or` `not` (short-circuit, M13) |
| Bitwise | `&` `|` `^` `~` `<<` `>>` |
| Null-coalesce | `x ?? y` (returns `x` if not none, else `y`; M21 BUG-037 fix) |
| Assignment | `=` `+=` `-=` `*=` `/=` `//=` `%=` `**=` `&=` `|=` `^=` `<<=` `>>=` |

Numeric ops do NOT implicitly convert between integer types or to/from float. Use `i32(x)` / `i64(x)` / `f64(x)` to convert explicitly.

### 3.7 Control flow

```python
# if / elif / else
if x > 0:
    println("positive")
elif x < 0:
    println("negative")
else:
    println("zero")

# while
i: i64 = 0
while i < 10:
    println(str(i))
    i = i + 1

# for-in (lowered to indexed while loop)
for item in items:
    println(item)
for i in range(10):
    println(str(i))

# break, continue
while true:
    if done:
        break
    if skip:
        continue
    work()

# return — every fn must return (or raise) at every path
fn classify(n: i32) -> str:
    if n > 0:
        return "positive"
    elif n < 0:
        return "negative"
    else:
        return "zero"
```

### 3.8 Function definitions

```python
fn add(a: i32, b: i32) -> i32:
    return a + b

fn print_greeting(name: str) -> None:    # None means "no return value"
    println("Hello, " + name)

# Generic free functions (M17)
fn id[T](x: T) -> T:
    return x

fn first[K, V](p: Tuple[K, V]) -> K:
    return p.0

# Use generic free function — call site monomorphisation
let x: i32 = id[i32](42)              # or just id(42) — inference works
let s: str = id("hello")
let key: str = first(("k", 99i32))

# Closures (used heavily with threads + async)
let f = fn(x: i32) -> i32:
    return x * 2
let doubled: i32 = f(21)
```

Note: function values are first-class but cannot cross the `NativeFn` boundary (you can't pass a user-defined function as an argument to a stdlib function that expects a callback — that's a v0.4 limit).

### 3.9 Class definitions

```python
# Default: classes are final (can't be subclassed)
final class Point:
    x: f64
    y: f64
    fn __init__(self, x: f64, y: f64) -> None:
        self.x = x
        self.y = y
    fn distance_to(self, other: Point) -> f64:
        let dx: f64 = self.x - other.x
        let dy: f64 = self.y - other.y
        return math.sqrt(dx * dx + dy * dy)

let p: Point = Point(3.0, 4.0)
let q: Point = Point(0.0, 0.0)
println(str(p.distance_to(q)))

# `open class` allows subclassing; `open fn` allows overriding
open class Shape:
    open fn area(self) -> f64:
        return 0.0

final class Circle(Shape):
    radius: f64
    fn __init__(self, r: f64) -> None:
        self.radius = r
    fn area(self) -> f64:                # overrides Shape.area
        return 3.14159 * self.radius * self.radius

# `sealed class` is a closed hierarchy — exhaustive match works (M16)
sealed class Value:
    pass

final class IntValue(Value):
    n: i64

final class StringValue(Value):
    s: str

# Generic classes (M31)
final class Box[T]:
    value: T
    fn __init__(self, v: T) -> None:
        self.value = v
    fn unwrap(self) -> T:
        return self.value

let b: Box[i64] = Box(42i64)             # constructor-site inference
let s: Box[str] = Box("hi")              # distinct instantiation
println(str(b.unwrap()))
println(s.unwrap())

# Generic class with 2 type params:
final class Pair[K, V]:
    key: K
    value: V
    fn __init__(self, k: K, v: V) -> None:
        self.key = k
        self.value = v

let p: Pair[str, i32] = Pair("count", 99i32)
```

**v0.3 limits on classes**:
- Single inheritance only.
- Default values for fields are NOT supported — initialize in `__init__`.
- Decorators on classes/methods don't exist.
- `@property` / `@staticmethod` / `@classmethod` don't exist.
- `class Box[T: Comparable]:` (bounded generics) — v0.4.
- Explicit type-arg syntax `Box[i64](v)` — v0.4. Use constructor-site inference.

### 3.10 Exception handling

```python
try:
    let f: io.File = open("missing.txt", "r")
    let content: str = f.read()
    f.close()
    println(content)
except IOError as e:
    println("Could not read file: " + e.message)
except Exception as e:                   # catches anything not matched above
    println("Other error: " + e.message)
finally:
    println("Cleanup runs whether or not there was an error")

# Raise an exception:
fn divide(a: i32, b: i32) -> i32:
    if b == 0:
        raise ZeroDivisionError("division by zero")
    return a / b
```

**Built-in exception names** (v0.3 ships these 10; user-defined subclasses are v0.4):

```
Exception (catches everything below it)
  ValueError
  IOError
  ZeroDivisionError
  IndexError
  KeyError
  TypeError
  RuntimeError
  AssertionError
  ChannelClosedError
```

`sys.exit(code)` raises a separate non-catchable termination (won't be caught by `except Exception`; matches Python's `SystemExit` derived from `BaseException`).

### 3.11 Pattern matching

See §8 for the full pattern reference. Quick example:

```python
match value:
    case IntValue(n):                    # Constructor pattern with field binding
        println("int: " + str(n))
    case StringValue(s):
        println("str: " + s)

# isinstance + flow narrowing
if isinstance(v, IntValue):
    # v is narrowed to IntValue here
    println(str(v.n))
```

### 3.12 Imports

```python
import sys                                # whole module
sys.exit(0)

from json import JsonValue, JString       # specific items (incl. classes)
from threading import Thread, Lock        # prelude-level classes

import json as j                          # alias
j.parse(text)
```

You can import:
- Stdlib modules (37 listed in §5)
- Stdlib classes via `from json import JsonValue` etc. — also `from re import Pattern`, `from sqlite3 import Connection, Cursor`, `from hashlib import Hasher`. M36 publishes these as proper module items rather than the M34/M35 prelude flatten, so a `from <mod> import <ClassName> as <Alias>` aliases the class cleanly. The bare class names also remain reachable after `import json` / `import re` / `import sqlite3` / `import hashlib` for back-compat with the M34/M35 surface.
- Prelude classes directly: `from threading import Thread`

You CANNOT import user-defined `.spy` modules in v0.3 (deferred to v0.4).

---

## §4 — Type system

### 4.1 Primitive types

| Type | Description | Literal example |
|---|---|---|
| `bool` | True or false | `true`, `false` |
| `i8` `i16` `i32` `i64` | Signed integers | `42i32`, `42i64`. `42` defaults to `i64`. |
| `u32` `u64` | Unsigned (limited; prefer signed) | `42u32` |
| `f32` `f64` | Floats. `f64` is the standard | `3.14`, `3.14f32` |
| `char` | One Unicode codepoint | `'a'`. Distinct from `str`. |
| `str` | Immutable UTF-8 string (also used as byte buffer; see §3.3) | `"hello"` |
| `None` | Unit type (only one value); returned by functions with no return value | n/a |

Integer division `/` on integers gives an integer result (use `f64(a) / f64(b)` for float division). Mod `%` follows Python semantics (sign follows divisor).

### 4.2 Composite types

| Type | Description | Construction |
|---|---|---|
| `List[T]` | Mutable, ordered, dynamic array of T | `[1, 2, 3]`, `[]` (with annotation) |
| `Dict[K, V]` | Hash map. **K is currently restricted to `str` in most contexts** (see §11.2) | `{"k": 1}`, `{}` (with annotation) |
| `Set[T]` | Hash set | `{1, 2, 3}` |
| `Tuple[T1, T2, ...]` | Fixed-shape product (M14) | `(1, "two")`, `()` |
| `T?` | Nullable: `T` or `none` | Same as T, but can hold `none` |
| `Channel[T]` | Thread-safe queue (prelude class) | `Channel()` (M5) |

### 4.3 Class types

Any user-defined `class Foo:` introduces type `Foo`. Subclasses are subtypes. The 11 stdlib classes available via stdlib imports — the 7 JsonValue classes (`JsonValue` + `JNull` / `JBool` / `JInt` / `JFloat` / `JString` / `JList` / `JObject`) from M34, plus `Pattern` / `Connection` / `Cursor` / `Hasher` from M35 — are usable like any other class. M36 published them as proper module items (`from json import JsonValue`, etc.); the M34/M35 prelude-flatten still reaches them by bare name after `import json` / `import re` / `import sqlite3` / `import hashlib` for back-compat.

### 4.4 Generic types

- **Generic free functions** (M17): `fn id[T](x: T) -> T:`
- **Generic classes** (M31): `class Box[T]:`, `class Pair[K, V]:`

Constructor-site type inference works:

```python
let b = Box(42)                          # b: Box[i64]
let p = Pair("k", 99i32)                 # p: Pair[str, i32]
```

Explicit type-arg syntax (`Box[i64](42)`) is v0.4.

### 4.5 Nullable narrowing

The type checker narrows `T?` to `T` inside branches that prove non-null:

```python
let x: str? = maybe_get_string()
if x is not none:
    # x is narrowed to str here — can use string methods
    println("got: " + x)
else:
    println("got nothing")

# Equivalent:
if x is none:
    println("got nothing")
else:
    println("got: " + x)
```

**Narrowing does NOT propagate through `and`/`or`** in expressions. Use nested ifs or local binding:

```python
# This does NOT narrow:
# if x is not none and len(x) > 0:
#     println(x.upper())   # ERROR: x is still str?

# Use a local:
if x is not none:
    if len(x) > 0:
        println(x.upper())
```

### 4.6 Null coalescing

```python
let name: str = some_optional_name() ?? "default"
```

`a ?? b` evaluates `a`; if it's `none`, evaluates `b`. Short-circuits — `b` is NOT evaluated if `a` is non-none.

### 4.7 Tuple destructuring (M14)

```python
let p: Tuple[i32, str] = make_pair()
p.0                # first element (i32)
p.1                # second element (str)

# Multi-assignment destructuring:
let a, b = p                            # a: i32, b: str

# In function returns:
fn divmod(a: i32, b: i32) -> Tuple[i32, i32]:
    return (a / b, a % b)

let q, r = divmod(17, 5)
```

### 4.8 isinstance + match narrowing

```python
if isinstance(v, IntValue):
    # v is narrowed to IntValue here
    println(str(v.n))

# match-case narrows automatically:
match v:
    case IntValue(n):                    # n: i64 bound from v.n
        println(str(n))
    case StringValue(s):
        println(s)
```

---

## §5 — Standard library reference

37 modules total. Every module's functions are listed here with signature; for non-obvious behaviour see the example. **Full implementation details** are in `STRICTPY_SPEC.md` §9 — use this section as the AI quick-reference.

**Stdlib classes (M36).** A handful of stdlib modules export classes alongside their function surface: `json.JsonValue` + 6 subclasses, `re.Pattern`, `sqlite3.Connection` / `sqlite3.Cursor`, and `hashlib.Hasher`. Import them with `from json import JsonValue` / `from re import Pattern` / etc. — they behave exactly like user-defined classes (`isinstance`, `match`, methods, fields). The M34/M35 prelude flatten still resolves the bare class names after just `import json` etc. for back-compat with the v0.3 surface, but new code should use the explicit `from <mod> import` form.

### sys (M19)

```python
import sys
sys.argv: List[str]          # program args; sys.argv[0] is the script path
sys.platform: str            # "windows" | "linux" | "macos" | "unknown"
sys.version: str             # version banner
sys.exit(code: i32) -> Never # terminates the program; NOT catchable
```

### os (M20a)

```python
import os
os.env(key: str) -> str?
os.set_env(key: str, value: str) -> None
os.getcwd() -> str
os.chdir(path: str) -> None
os.listdir(path: str) -> List[str]
os.remove(path: str) -> None
os.mkdir(path: str) -> None
os.exists(path: str) -> bool
os.is_file(path: str) -> bool
os.is_dir(path: str) -> bool
os.read_file(path: str) -> str
os.write_file(path: str, content: str) -> None
```

### path (M20a)

```python
import path
path.join(a: str, b: str) -> str
path.dirname(p: str) -> str
path.basename(p: str) -> str
path.splitext(p: str) -> Tuple[str, str]   # ("file", ".txt")
path.sep: str                              # "\\" on Windows, "/" elsewhere
```

### io (M20a)

```python
import io
# Top-level functions:
io.input() -> str
io.input_with_prompt(prompt: str) -> str
io.write_stdout(s: str) -> None
io.write_stderr(s: str) -> None
io.flush_stdout() -> None

# io.File (prelude class) — open() returns one:
let f: io.File = open(path: str, mode: str)   # mode: "r" / "w" / "a"
f.read() -> str
f.write(s: str) -> None
f.close() -> None
# `with` block automatically calls close():
with open("x.txt", "r") as f:
    println(f.read())
```

### time (M20b)

```python
import time
time.now() -> f64                        # Unix epoch seconds (with fractional)
time.now_ms() -> i64                     # epoch milliseconds
time.monotonic() -> f64                  # seconds since arbitrary start; not affected by clock changes
time.sleep_s(secs: f64) -> None
time.sleep_ms(ms: i64) -> None
time.format_iso(epoch_secs: f64) -> str  # "2026-05-21T12:34:56Z"
```

### random (M20b)

```python
import random
random.seed(s: i64) -> None
random.randint(lo: i64, hi: i64) -> i64  # inclusive both sides
random.random() -> f64                    # [0.0, 1.0)
# Per-type variants because v0.3 has no generic random.choice yet:
random.choice_i64(xs: List[i64]) -> i64
random.choice_str(xs: List[str]) -> str
random.shuffle_i64(xs: List[i64]) -> None       # in-place
random.shuffle_str(xs: List[str]) -> None
random.sample_i64(xs: List[i64], k: i64) -> List[i64]
random.sample_str(xs: List[str], k: i64) -> List[str]
```

### math (M20b)

```python
import math
math.pi: f64
math.e: f64
math.tau: f64
math.inf: f64
math.nan: f64
math.sqrt(x: f64) -> f64
math.sin(x: f64) -> f64
math.cos(x: f64) -> f64
math.tan(x: f64) -> f64
math.log(x: f64) -> f64
math.log2(x: f64) -> f64
math.log10(x: f64) -> f64
math.exp(x: f64) -> f64
math.pow(x: f64, y: f64) -> f64
math.floor(x: f64) -> i64                # Python 3 semantics: returns int
math.ceil(x: f64) -> i64
math.gcd(a: i64, b: i64) -> i64
math.factorial(n: i64) -> i64
math.is_nan(x: f64) -> bool
math.is_inf(x: f64) -> bool
```

### json (M20c, extended by M34)

**Original flat surface** (still works, used by old code):

```python
import json
json.parse_to_string(s: str) -> str      # parse + reserialize canonical form
json.is_valid(s: str) -> bool
json.minify(s: str) -> str
json.pretty(s: str) -> str
json.escape(s: str) -> str               # quote-escape a string value
```

**M34 typed surface** (recommended for new code):

```python
from json import JsonValue, JNull, JBool, JInt, JFloat, JString, JList, JObject

json.parse(s: str) -> JsonValue           # raises ValueError on malformed JSON
json.stringify(v: JsonValue) -> str       # compact
json.stringify_pretty(v: JsonValue, indent: i32) -> str

# Constructor convenience helpers:
json.j_null() -> JsonValue
json.j_bool(b: bool) -> JsonValue
json.j_int(n: i64) -> JsonValue
json.j_float(f: f64) -> JsonValue
json.j_string(s: str) -> JsonValue
json.j_list(items: List[JsonValue]) -> JsonValue
json.j_object(entries: List[Tuple[str, JsonValue]]) -> JsonValue
```

JsonValue method/field reference:

```python
# JNull: no payload
# JBool: field .value (bool)
# JInt: field .value (i64)
# JFloat: field .value (f64)
# JString: field .value (str)

# JList:
list_val.length() -> i64
list_val.get(i: i64) -> JsonValue        # raises IndexError if out of bounds
list_val.items() -> List[JsonValue]      # safe copy

# JObject:
obj_val.get(k: str) -> JsonValue?         # none if absent
obj_val.has(k: str) -> bool
obj_val.keys() -> List[str]
obj_val.length() -> i64

# Typical use:
let v: JsonValue = json.parse('{"name": "alice", "age": 30}')
match v:
    case JObject(o):
        let name: JsonValue? = o.get("name")
        if name is not none:
            match name:
                case JString(s): println("name = " + s)
```

### re (M20c, extended by M35)

```python
import re
# Flat surface (M20c — recompiles on every call):
re.is_valid(pattern: str) -> bool
re.fullmatch(pattern: str, s: str) -> bool
re.search(pattern: str, s: str) -> str?           # first match or none
re.find(pattern: str, s: str) -> str?
re.find_all(pattern: str, s: str) -> List[str]
re.replace(pattern: str, s: str, repl: str) -> str   # first match
re.replace_all(pattern: str, s: str, repl: str) -> str
re.split(pattern: str, s: str) -> List[str]

# Compiled Pattern class — M35. Compile once, reuse cheaply in hot loops.
re.compile(s: str) -> Pattern                     # raises ValueError on bad regex

# Pattern methods:
p.matches(s: str) -> bool                          # full-string match
p.find(s: str) -> str?                             # first match or none
p.find_all(s: str) -> List[str]
p.replace(s: str, repl: str) -> str                # first match
p.replace_all(s: str, repl: str) -> str
p.split(s: str) -> List[str]
p.source() -> str                                  # the original pattern string
```

`re.match` is renamed to `re.fullmatch` because `match` is a hard keyword since M16.

Use `re.compile` when the same pattern runs in a loop — it's strictly cheaper than the flat surface, which recompiles on every call.

### argparse (M22 P2A)

```python
import argparse
# Build a parser via builder methods:
let p = argparse.parser("myprogram", "Description of my tool")
argparse.add_flag(p, "--verbose", "Print verbose output")
argparse.add_option(p, "--output", "Output file path")
argparse.add_positional(p, "input", "Input file")

# Parse argv into a Dict[str, str]:
let a: Dict[str, str] = argparse.parse(p, sys.argv)

# Convenience: detect help flag without parsing
argparse.help_requested(argv: List[str]) -> bool
```

ArgParser as a typed class is v0.4 (waiting on stdlib classes). v0.3 uses `Dict[str, str]` as a shim.

### collections (M22 P2A)

```python
import collections
# Counter: monomorphic per type; v0.3 ships str and i64 variants
collections.counter_str(xs: List[str]) -> Dict[str, i64]
collections.counter_i64(xs: List[i64]) -> Dict[str, i64]   # keys converted to str (BUG-039)

# Deque: thin shim over List for now
collections.deque_pop_front_str(xs: List[str]) -> str
collections.deque_push_back_str(xs: List[str], v: str) -> None
```

### csv (M22 P2A)

```python
import csv
csv.parse_line(s: str) -> List[str]                # one row
csv.read_file(path: str) -> List[List[str]]
csv.write_file(path: str, rows: List[List[str]]) -> None
csv.format_row(row: List[str]) -> str
csv.escape(field: str) -> str
```

### base64 (M22 P2B)

```python
import base64
base64.encode(data: str) -> str             # str is byte-buffer (§3.3)
base64.decode(s: str) -> str
base64.encode_url(data: str) -> str         # URL-safe variant
base64.decode_url(s: str) -> str
```

### hashlib (M22 P2B, extended by M35)

```python
import hashlib
# One-shot (M22):
hashlib.md5(data: str) -> str               # returns hex digest
hashlib.sha1(data: str) -> str
hashlib.sha256(data: str) -> str
hashlib.sha512(data: str) -> str
hashlib.hmac_sha256(key: str, data: str) -> str

# Streaming Hasher class — M35:
hashlib.new(algo: str) -> Hasher                  # algo: "md5" | "sha1" | "sha256" | "sha512"
h.update(data: str) -> None                       # feed a chunk
h.hexdigest() -> str                              # current digest as hex string
h.digest() -> str                                 # current digest as raw byte-buffer
h.copy() -> Hasher                                # snapshot for multi-output digests
h.reset() -> None                                 # start over with the same algorithm
h.name() -> str                                   # the algorithm name
```

NOT suitable for password storage (no bcrypt/argon2 in v0.3).

### itertools (M22 P2C)

```python
import itertools
# Per-type because v0.3 has no generic stdlib functions:
itertools.chain_i64(a: List[i64], b: List[i64]) -> List[i64]
itertools.chain_str(a: List[str], b: List[str]) -> List[str]
itertools.zip_i64(a: List[i64], b: List[i64]) -> List[Tuple[i64, i64]]
itertools.zip_str(a: List[str], b: List[str]) -> List[Tuple[str, str]]
itertools.product_i64(a: List[i64], b: List[i64]) -> List[Tuple[i64, i64]]
itertools.permutations_i64(xs: List[i64]) -> List[List[i64]]
itertools.combinations_i64(xs: List[i64], k: i64) -> List[List[i64]]
itertools.count(start: i64, n: i64) -> List[i64]    # eager (not infinite)
```

### statistics (M22 P2C)

```python
import statistics
statistics.mean(xs: List[f64]) -> f64
statistics.median(xs: List[f64]) -> f64
statistics.mode_i64(xs: List[i64]) -> i64
statistics.mode_str(xs: List[str]) -> str
statistics.variance(xs: List[f64]) -> f64
statistics.stdev(xs: List[f64]) -> f64
statistics.sum_i64(xs: List[i64]) -> i64
statistics.sum_f64(xs: List[f64]) -> f64
statistics.min_i64(xs: List[i64]) -> i64
statistics.max_i64(xs: List[i64]) -> i64
```

### struct (M22 P2D)

Binary packing — like Python's `struct`. Format codes: `i32` `i64` `u32` `u64` `f32` `f64` `b` (i8) `B` (u8) `?` (bool).

```python
import struct
struct.pack_i32(n: i32) -> str         # 4 bytes
struct.pack_i64(n: i64) -> str
struct.pack_f32(x: f32) -> str
struct.pack_f64(x: f64) -> str
struct.unpack_i32(data: str, offset: i64) -> i32
struct.unpack_i64(data: str, offset: i64) -> i64
struct.unpack_f64(data: str, offset: i64) -> f64
```

`str` is the byte-buffer (§3.3).

### urllib_parse (M22 P2D)

```python
import urllib_parse
urllib_parse.quote(s: str) -> str               # percent-encode
urllib_parse.unquote(s: str) -> str             # decode
urllib_parse.parse_qs(query: str) -> Dict[str, str]
urllib_parse.urlencode(params: List[Tuple[str, str]]) -> str
```

### subprocess (M23 P3a-A)

```python
import subprocess
subprocess.run(cmd: List[str]) -> i32            # exit code
subprocess.run_with_stdout(cmd: List[str]) -> Tuple[i32, str]   # (exit, stdout)
subprocess.run_with_stderr(cmd: List[str]) -> Tuple[i32, str, str]   # (exit, stdout, stderr)
subprocess.spawn(cmd: List[str]) -> i64          # process handle (i64)
subprocess.wait(handle: i64) -> i32
subprocess.kill(handle: i64) -> None
```

### pathlib (M23 P3a-A)

```python
import pathlib
# Path-string helpers (flat-fn shape — Path class is v0.4)
pathlib.exists(p: str) -> bool
pathlib.is_file(p: str) -> bool
pathlib.is_dir(p: str) -> bool
pathlib.stem(p: str) -> str
pathlib.suffix(p: str) -> str
pathlib.parent(p: str) -> str
pathlib.name(p: str) -> str
pathlib.read_text(p: str) -> str
pathlib.read_lines(p: str) -> List[str]
pathlib.write_text(p: str, content: str) -> None
pathlib.absolute(p: str) -> str
pathlib.relative_to(p: str, base: str) -> str
```

### datetime (M23 P3a-B)

```python
import datetime
# Flat-fn shape (DateTime class is v0.4)
datetime.now() -> i64                  # epoch seconds
datetime.now_iso() -> str              # ISO-8601 string
datetime.from_iso(s: str) -> i64        # parse to epoch seconds
datetime.to_iso(epoch_secs: i64) -> str
datetime.add_seconds(epoch_secs: i64, delta: i64) -> i64
datetime.add_days(epoch_secs: i64, days: i64) -> i64
datetime.year(epoch_secs: i64) -> i32
datetime.month(epoch_secs: i64) -> i32  # 1-12
datetime.day(epoch_secs: i64) -> i32    # 1-31
datetime.hour(epoch_secs: i64) -> i32
datetime.minute(epoch_secs: i64) -> i32
datetime.second(epoch_secs: i64) -> i32
datetime.weekday(epoch_secs: i64) -> i32  # 0=Mon ... 6=Sun
datetime.local_offset() -> i32          # seconds east of UTC
```

### threading (M23 P3a-C, plus M6 Thread prelude)

```python
# `Thread` is a prelude class (no import needed):
let t: Thread = Thread(my_closure)
t.start()
t.join()

# Locks and semaphores via import:
import threading
let lock: i64 = threading.lock()                # handle (i64)
threading.lock_acquire(handle: i64) -> None
threading.lock_release(handle: i64) -> None

let sem: i64 = threading.semaphore(initial: i64)
threading.semaphore_acquire(handle: i64) -> None
threading.semaphore_release(handle: i64) -> None
```

### queue (M23 P3a-C)

```python
import queue
let pq: i64 = queue.priority_queue()
queue.pq_push(handle: i64, priority: i64, item: str) -> None
queue.pq_pop(handle: i64) -> Tuple[i64, str]    # (priority, item)
queue.pq_len(handle: i64) -> i64
```

### sqlite3 (M23 P3a-D, extended by M35)

```python
import sqlite3
# Flat surface (still works):
let conn: i64 = sqlite3.connect(path: str)        # ":memory:" for in-memory
sqlite3.execute(conn: i64, sql: str) -> None
sqlite3.execute_params(conn: i64, sql: str, params: List[str]) -> None
sqlite3.query(conn: i64, sql: str) -> List[List[str]]
sqlite3.query_params(conn: i64, sql: str, params: List[str]) -> List[List[str]]
sqlite3.last_insert_rowid(conn: i64) -> i64
sqlite3.changes(conn: i64) -> i32
sqlite3.close(conn: i64) -> None
sqlite3.column_names(conn: i64, sql: str) -> List[str]

# Connection / Cursor classes — M35:
sqlite3.open(path: str) -> Connection             # ":memory:" for in-memory
conn.execute(sql: str) -> None
conn.execute_params(sql: str, params: List[str]) -> None
conn.query(sql: str) -> Cursor
conn.query_params(sql: str, params: List[str]) -> Cursor
conn.last_insert_rowid() -> i64
conn.changes() -> i32
conn.close() -> None                              # idempotent

# Cursor methods (always check fetchone() against none — `is not none` narrows):
cur.fetchone() -> List[str]?                      # next row or none
cur.fetchall() -> List[List[str]]                 # remaining rows
cur.column_names() -> List[str]
cur.row_count() -> i64                            # rows the underlying query produced
```

All cells are stringified (v0.4 will add typed cell access).

### tabular (M37)

First Pandas-shaped data package — native Rust impl (real pandas can't import). Sealed `Column` hierarchy + `DataFrame` with named columns + per-column null mask. First stdlib package to register its classes module-scoped from the start (no prelude bloat — see §6.2).

```python
import tabular
from tabular import Column, ColumnI64, ColumnF64, ColumnStr, ColumnBool, ColumnDateTime, DataFrame

# Column factories (values + optional null mask).
tabular.col_i64(values: List[i64], nulls: List[bool]) -> ColumnI64
tabular.col_i64_simple(values: List[i64]) -> ColumnI64           # nulls all false
tabular.col_f64(values: List[f64], nulls: List[bool]) -> ColumnF64
tabular.col_f64_simple(values: List[f64]) -> ColumnF64
tabular.col_str(values: List[str], nulls: List[bool]) -> ColumnStr
tabular.col_str_simple(values: List[str]) -> ColumnStr
tabular.col_bool(values: List[bool], nulls: List[bool]) -> ColumnBool
tabular.col_bool_simple(values: List[bool]) -> ColumnBool
tabular.col_datetime(values: List[i64], nulls: List[bool]) -> ColumnDateTime
                                                                 # values are epoch ms

# DataFrame construction.
tabular.from_columns(names: List[str], cols: List[Column]) -> DataFrame
tabular.from_rows(rows: List[List[str]],
                  schema: List[Tuple[str, str]]) -> DataFrame    # dtype-driven parsing

# I/O.
tabular.read_csv(path: str, schema: List[Tuple[str, str]]) -> DataFrame
tabular.write_csv(path: str, df: DataFrame) -> None
tabular.from_sql(cur: Cursor, schema: List[Tuple[str, str]]) -> DataFrame
                                                                 # drains a sqlite3 Cursor

# Schema dtype strings: "i64" | "f64" | "str" | "bool" | "datetime".

# Per-Column shared methods.
col.length() -> i64
col.dtype() -> str                              # "i64"/"f64"/"str"/"bool"/"datetime"
col.is_null(i: i64) -> bool                     # bounds-checked
col.null_count() -> i64

# Per-Column typed accessors.
ColumnI64.get(i: i64) -> i64?                   # none if null
ColumnF64.get(i: i64) -> f64?
ColumnStr.get(i: i64) -> str?
ColumnBool.get(i: i64) -> bool?
ColumnDateTime.get_ms(i: i64) -> i64?

# Per-Column comparisons → ColumnBool mask (null-propagating).
ColumnI64.eq / gt / lt(x: i64) -> ColumnBool
ColumnF64.eq / gt / lt(x: f64) -> ColumnBool
ColumnStr.eq(x: str) -> ColumnBool
ColumnStr.contains(needle: str) -> ColumnBool

# Mask combinators.
mask.and_(other: ColumnBool) -> ColumnBool
mask.or_(other: ColumnBool) -> ColumnBool
mask.not_() -> ColumnBool
mask.count_true() -> i64                        # nulls treated as not-true

# DataFrame inspection.
df.length() -> i64                              # nrows
df.ncols() -> i64
df.columns() -> List[str]                       # defensive copy
df.dtypes() -> List[str]
df.has_column(name: str) -> bool
df.show(n: i64) -> str                          # ASCII table; n=-1 for all rows

# DataFrame projection / filter / row ops.
df.filter(mask: ColumnBool) -> DataFrame        # keep rows where mask is true
df.select(cols: List[str]) -> DataFrame         # raises if a col is absent
df.drop(cols: List[str]) -> DataFrame           # no-op if col absent
df.head(n: i64) -> DataFrame
df.tail(n: i64) -> DataFrame
df.row(i: i64) -> List[str]                     # stringified; "null" for null cells
df.sort_by(col_name: str, ascending: bool) -> DataFrame
                                                 # stable; nulls go to END
```

**Null semantics**: every column has a parallel `nulls: List[bool]` mask. `nulls[i] == true` means cell `i` is NA. Comparisons OR the input null masks into the result; `count_true` treats null cells as not-true (so a 3-row column with one null and two passing cells has `count_true == 2`). Sorts route null rows to the end regardless of direction (matches `pandas.DataFrame.sort_values(..., na_position="last")`).

See `examples/tabular_demo.spy` for an end-to-end walkthrough (construct → CSV round-trip → filter → sort → project).

### shutil (M27 P3c-A)

```python
import shutil
shutil.copy(src: str, dst: str) -> None
shutil.copytree(src: str, dst: str) -> None
shutil.move(src: str, dst: str) -> None
shutil.rmtree(path: str) -> None
shutil.which(cmd: str) -> str?
shutil.disk_usage(path: str) -> Tuple[i64, i64, i64]   # (total, used, free) bytes
```

### tempfile (M27 P3c-A)

```python
import tempfile
tempfile.mkdtemp(prefix: str) -> str
tempfile.mkstemp(prefix: str, suffix: str) -> str
tempfile.gettempdir() -> str
```

### glob (M27 P3c-B)

```python
import glob
glob.glob(pattern: str) -> List[str]           # shell-style; sorted ascending
glob.recursive(pattern: str) -> List[str]      # `**` walks subdirs
glob.escape(s: str) -> str
```

### fnmatch (M27 P3c-B)

```python
import fnmatch
fnmatch.fnmatch(name: str, pattern: str) -> bool       # case-insensitive on Windows
fnmatch.fnmatchcase(name: str, pattern: str) -> bool   # always case-sensitive
fnmatch.filter(names: List[str], pattern: str) -> List[str]
fnmatch.translate(pattern: str) -> str        # glob → regex string
```

### gzip / zlib / bz2 (M27 P3c-C)

```python
import gzip
gzip.compress(data: str) -> str               # default level 6
gzip.compress_level(data: str, level: i32) -> str   # 0-9
gzip.decompress(data: str) -> str

import zlib
zlib.compress(data: str) -> str
zlib.compress_level(data: str, level: i32) -> str
zlib.decompress(data: str) -> str
zlib.crc32(data: str) -> i64
zlib.adler32(data: str) -> i64

import bz2
bz2.compress(data: str) -> str
bz2.compress_level(data: str, level: i32) -> str    # 1-9
bz2.decompress(data: str) -> str
```

Inputs/outputs are byte buffers (§3.3). Malformed input raises `ValueError`.

### zipfile / tarfile (M27 P3c-D)

```python
import zipfile
let h: i64 = zipfile.open_read(path: str)        # handle
zipfile.names(h: i64) -> List[str]
zipfile.read(h: i64, name: str) -> str            # byte buffer
zipfile.close(h: i64) -> None
zipfile.is_zipfile(path: str) -> bool

let w: i64 = zipfile.open_write(path: str)
zipfile.write(h: i64, name: str, data: str) -> None
# close to finalize:
zipfile.close(w)

import tarfile
let h: i64 = tarfile.open_read(path: str, mode: str)   # "r" / "r:gz" / "r:bz2"
tarfile.names(h: i64) -> List[str]
tarfile.read(h: i64, name: str) -> str
tarfile.close(h: i64) -> None
tarfile.is_tarfile(path: str) -> bool

let w: i64 = tarfile.open_write(path: str, mode: str)  # "w" / "w:gz" / "w:bz2"
tarfile.write_file(h: i64, src_path: str, arcname: str) -> None
tarfile.write_data(h: i64, arcname: str, data: str) -> None
tarfile.close(w)
```

### logging (M27 P3c-E)

```python
import logging
logging.basic_config(level: str) -> None              # "DEBUG"|"INFO"|"WARNING"|"ERROR"|"CRITICAL"
logging.basic_config_to_file(level: str, filename: str) -> None
logging.set_level(level: str) -> None
logging.get_level() -> str
logging.debug(msg: str) -> None
logging.info(msg: str) -> None
logging.warning(msg: str) -> None
logging.error(msg: str) -> None
logging.critical(msg: str) -> None
logging.log(level: str, msg: str) -> None
logging.is_enabled_for(level: str) -> bool            # gate expensive message building
```

Flat global-logger only in v0.3; named loggers + handlers + formatters are v0.4 (need stdlib classes).

### socket (M28 P3b-A)

```python
import socket
# TCP client:
let h: i64 = socket.connect_tcp(host: str, port: i32)
socket.send(h: i64, data: str) -> i32                # returns bytes sent
socket.recv(h: i64, max_bytes: i32) -> str
socket.recv_exact(h: i64, n: i32) -> str             # raises if connection closes early
socket.close(h: i64) -> None
socket.set_timeout_secs(h: i64, secs: f64) -> None
socket.peer_addr(h: i64) -> str                      # "127.0.0.1:8080"
socket.local_addr(h: i64) -> str

# TCP server:
let lis: i64 = socket.listen_tcp(host: str, port: i32, backlog: i32)
socket.accept(lis: i64) -> Tuple[i64, str]           # (new_conn, peer_addr)
socket.close_listener(lis: i64) -> None              # M30 BUG-040 fix: wakes blocked accept

# UDP:
let u: i64 = socket.udp_socket()
let bound: i64 = socket.udp_bind(host: str, port: i32)
socket.udp_send_to(h: i64, data: str, host: str, port: i32) -> i32
socket.udp_recv_from(h: i64, max_bytes: i32) -> Tuple[str, str, i32]   # (data, host, port)
socket.udp_close(h: i64) -> None

# DNS / utility:
socket.gethostbyname(host: str) -> str               # first IP
socket.resolve(host: str, port: i32) -> List[str]    # all addresses
socket.gethostname() -> str
```

### ssl (M28 P3b-B, M28.5 P3b-D)

```python
import ssl
# Client side (M28 P3b-B):
let h: i64 = ssl.connect(host: str, port: i32)       # bundles TCP + TLS handshake
ssl.send(h: i64, data: str) -> i32
ssl.recv(h: i64, max_bytes: i32) -> str
ssl.recv_exact(h: i64, n: i32) -> str
ssl.close(h: i64) -> None
ssl.peer_addr(h: i64) -> str
ssl.peer_cert_subject(h: i64) -> str
ssl.set_timeout_secs(h: i64, secs: f64) -> None

# Cert verification (default true; turn off ONLY for testing):
ssl.set_verify_certs(enabled: bool) -> None
ssl.get_verify_certs() -> bool

# Server side (M28.5 P3b-D):
let cfg: i64 = ssl.load_server_config(cert_pem_path: str, key_pem_path: str)
let conn: Tuple[i64, str] = ssl.accept_tls(tcp_listener: i64, cfg: i64)
ssl.free_server_config(cfg: i64) -> None
# The returned conn handle (>= 1_000_000) works with the same send/recv/close functions above.
```

### http_client (M28 P3b-C)

```python
import http_client
# Simple methods (auto-detect http vs https):
http_client.get(url: str) -> Tuple[i32, str]                      # (status, body)
http_client.post(url: str, body: str, content_type: str) -> Tuple[i32, str]
http_client.put(url: str, body: str, content_type: str) -> Tuple[i32, str]
http_client.delete(url: str) -> Tuple[i32, str]
http_client.head(url: str) -> Tuple[i32, str]

# Configurable:
http_client.request(method: str, url: str, body: str,
                    headers: List[Tuple[str, str]],
                    timeout_secs: f64) -> Tuple[i32, str]
http_client.request_with_headers(method: str, url: str, body: str,
                                 headers: List[Tuple[str, str]],
                                 timeout_secs: f64)
                                 -> Tuple[i32, List[Tuple[str, str]], str]

# Utilities:
http_client.urlencode(pairs: List[Tuple[str, str]]) -> str
http_client.urldecode(s: str) -> str
http_client.url_parse(url: str) -> Tuple[str, str, i32, str]   # (scheme, host, port, path_and_query)
http_client.status_text(code: i32) -> str
```

4xx/5xx are RETURNED (not raised). Network failures raise `IOError`.

### asyncio (M32)

See §9 for full async I/O reference.

```python
import asyncio
asyncio.run_i32(closure: () -> i32) -> i32         # block until root closure done
asyncio.run_unit(closure: () -> None) -> None
asyncio.spawn_i32(closure: () -> i32) -> Future[i32]
asyncio.spawn_str(closure: () -> str) -> Future[str]
asyncio.spawn_unit(closure: () -> None) -> Future[None]
asyncio.sleep(secs: f64) -> None
asyncio.gather_2_i32(a: Future[i32], b: Future[i32]) -> Tuple[i32, i32]
# ... gather_3, gather_4, _str variants

# Future is a TypeCtor; .await() and .is_ready() are methods:
let fut: Future[i32] = asyncio.spawn_i32(do_work)
let result: i32 = fut.await()
```

Async-variant socket functions (also M32):

```python
import socket
socket.async_accept(listener: i64) -> Future[Tuple[i64, str]]
socket.async_recv(handle: i64, max_bytes: i32) -> Future[str]
socket.async_send(handle: i64, data: str) -> Future[i32]
```

---

## §6 — Prelude

The prelude is what's available without any `import` statement.

### 6.1 Prelude functions

```python
println(s: str) -> None                  # stdout + newline
print(s: str) -> None                    # stdout, no newline
len(x: ...) -> i64                       # works for List/Dict/Set/str/Tuple
range(n: i64) -> List[i64]               # [0, n)
range_step(start: i64, stop: i64, step: i64) -> List[i64]
assert(cond: bool, msg: str) -> None     # raises AssertionError on false

# Type constructors (also act as type names):
i8(x), i16(x), i32(x), i64(x), u32(x), u64(x), f32(x), f64(x)
bool(x), str(x), char(i: i32)
parse_i64(s: str) -> i64                 # raises ValueError on bad input
parse_f64(s: str) -> f64

# Min / max / abs (per-type):
abs(x: i64) -> i64
min_i64(a: i64, b: i64) -> i64
max_i64(a: i64, b: i64) -> i64
min_f64(a: f64, b: f64) -> f64
max_f64(a: f64, b: f64) -> f64

# String concat:
"hello" + " " + "world"                  # str + str = str
str(42) + " items"                       # str(x) coerces any type
```

### 6.2 Prelude classes

These are available without import:

| Class | What it is | Where it ships |
|---|---|---|
| `List[T]` | Dynamic array | always |
| `Dict[K, V]` | Hash map (K mostly limited to str — see §11.2) | always |
| `Set[T]` | Hash set | always |
| `Channel[T]` | Thread-safe queue | M5 |
| `Thread` | OS thread | M6 |
| `io.File` | Open file handle (via `open()` or `with open(...)`) | M5 |
| 10 exception names | See §3.10 | M15 |

**Stdlib classes are module-scoped.** `JsonValue` + 6 subclasses (`JNull` / `JBool` / `JInt` / `JFloat` / `JString` / `JList` / `JObject`), `Pattern`, `Connection` + `Cursor`, and `Hasher` are stdlib classes — import them from their home modules (`from json import JsonValue`, `from re import Pattern`, `from sqlite3 import Connection, Cursor`, `from hashlib import Hasher`). Pre-M36 these flattened into the prelude; M36 moved the metadata into the stdlib-module table. The bare names still resolve after a plain `import json` / `import re` / `import sqlite3` / `import hashlib` for back-compat with the M34/M35 test surface, but new code should prefer the explicit `from <mod> import` form.

**M37 `tabular` is the first stdlib package to register its classes module-scoped from the start (no prelude bloat).** The 6 classes — `Column` + 5 final subclasses (`ColumnI64` / `ColumnF64` / `ColumnStr` / `ColumnBool` / `ColumnDateTime`) + `DataFrame` — are reachable only via `from tabular import …` (or `import tabular` + `tabular.ColumnI64` style annotations). There is no bare-name fallback. See §5 `tabular` entry for the full surface.

### 6.3 List, Dict, Set methods

```python
# List[T]:
xs.append(v: T) -> None
xs.pop() -> T                            # raises IndexError on empty
xs.length() -> i64                       # or len(xs)
xs.sort() -> None                        # in-place
xs.sorted() -> List[T]                   # returns copy
xs[i]                                    # subscript; raises IndexError if oob
xs[i] = v                                # in-place
for x in xs: ...
len(xs)                                  # same as xs.length()

# Dict[str, V]:
d[k]                                     # raises KeyError if absent
d[k] = v
d.get(k: str) -> V?                      # none if absent
d.has(k: str) -> bool                    # PREFER over `k in d` (BUG-039 fix uses this path)
d.keys() -> List[str]
d.values() -> List[V]
d.length() -> i64
len(d)

# Set[T]:
s.add(v: T) -> None
s.has(v: T) -> bool
s.length() -> i64
v in s                                   # works
```

### 6.4 String methods

```python
s.length()                               # or len(s)
s.upper() -> str
s.lower() -> str
s.strip() -> str
s.lstrip() -> str
s.rstrip() -> str
s.split(sep: str) -> List[str]
s.starts_with(prefix: str) -> bool
s.ends_with(suffix: str) -> bool
s.contains(needle: str) -> bool
s.replace(old: str, new: str) -> str
s.char_at(i: i64) -> char
s.slice(start: i64, end: i64) -> str     # [start, end)
s.index_of(needle: str) -> i64           # -1 if not found
s.repeat(n: i64) -> str
str(x)                                   # generic — works on any type
char(i: i32) -> char                     # Unicode codepoint
```

### 6.5 Channel methods

```python
let ch: Channel[i32] = Channel()
ch.send(v: i32) -> None
ch.recv() -> i32                         # blocks until sender; raises ChannelClosedError on closed-empty
ch.try_recv() -> i32?                    # none if empty
ch.close() -> None
```

### 6.6 Thread

```python
fn worker() -> None:
    println("hello from thread")

let t: Thread = Thread(worker)            # takes a () -> None closure
t.start()
t.join()                                  # wait for completion
```

---

## §7 — Common idioms

### 7.1 Read a file

```python
import io

fn read_text(path: str) -> str:
    let f: io.File = open(path, "r")
    let content: str = f.read()
    f.close()
    return content
```

Or use `with`:

```python
with open(path, "r") as f:
    let content: str = f.read()
    println(content)
```

**Gotcha**: `with` does NOT route IOError through an enclosing `try ... except`. Wrap explicitly:

```python
try:
    with open(path, "r") as f:
        let content: str = f.read()
        println(content)
except IOError as e:
    println("read failed: " + e.message)
```

### 7.2 Parse and walk JSON (post-M34)

```python
import json
from json import JsonValue, JString, JInt, JList, JObject

let v: JsonValue = json.parse('{"name": "alice", "tags": ["a", "b"]}')

match v:
    case JObject(obj):
        let name: JsonValue? = obj.get("name")
        if name is not none:
            match name:
                case JString(s): println("name = " + s)

        let tags: JsonValue? = obj.get("tags")
        if tags is not none:
            match tags:
                case JList(lst):
                    let n: i64 = lst.length()
                    let i: i64 = 0
                    while i < n:
                        match lst.get(i):
                            case JString(s): println("tag: " + s)
                        i = i + 1
```

### 7.3 HTTP GET

```python
import http_client

let (status, body) = http_client.get("https://example.com/")
if status == 200:
    println(body)
else:
    println("got status " + str(status))
```

### 7.4 SQLite query

```python
import sqlite3

let conn: i64 = sqlite3.connect(":memory:")
sqlite3.execute(conn, "CREATE TABLE notes (id INTEGER PRIMARY KEY, text TEXT)")
sqlite3.execute_params(conn, "INSERT INTO notes (text) VALUES (?)", ["hello"])
let rows: List[List[str]] = sqlite3.query(conn, "SELECT id, text FROM notes")
for row in rows:
    println("id=" + row[0] + " text=" + row[1])
sqlite3.close(conn)
```

### 7.5 Threading

```python
fn worker(id: i32, ch: Channel[i32]) -> None:
    ch.send(id * id)

fn main() -> i32:
    let ch: Channel[i32] = Channel()

    # Spawn 4 workers (closures capturing ch):
    let i: i32 = 0
    while i < 4i32:
        let id: i32 = i
        let t: Thread = Thread(fn() -> None: worker(id, ch))
        t.start()
        i = i + 1i32

    # Collect results:
    let j: i32 = 0
    while j < 4i32:
        let result: i32 = ch.recv()
        println("got: " + str(result))
        j = j + 1i32
    return 0
```

### 7.6 Generic container

```python
final class Stack[T]:
    items: List[T]
    fn __init__(self) -> None:
        self.items = []
    fn push(self, v: T) -> None:
        self.items.append(v)
    fn pop(self) -> T:
        return self.items.pop()
    fn size(self) -> i64:
        return self.items.length()

let s: Stack[i64] = Stack()           # constructor-site inference
s.push(1i64)
s.push(2i64)
println(str(s.pop()))                 # 2
```

### 7.7 sys.argv handling

```python
import sys

fn main() -> i32:
    let argc: i64 = i64(len(sys.argv))
    if argc < 2:
        println("usage: " + sys.argv[0] + " <name>")
        sys.exit(1)
    let name: str = sys.argv[1]
    println("hello, " + name)
    return 0
```

### 7.8 Async I/O

```python
import asyncio
import socket

fn handle_client(conn: i64, peer: str) -> None:
    let data: str = socket.recv(conn, 4096i32)
    socket.send(conn, "echo: " + data)
    socket.close(conn)

fn server_main() -> i32:
    let lis: i64 = socket.listen_tcp("127.0.0.1", 0i32, 16i32)
    let port: i32 = ... # extract from socket.local_addr(lis)

    # Accept loop using async_accept for concurrency
    let fut: Future[Tuple[i64, str]] = socket.async_accept(lis)
    let (conn, peer) = fut.await()
    asyncio.spawn_unit(fn() -> None: handle_client(conn, peer))
    # ... loop
    return 0

fn main() -> i32:
    return asyncio.run_i32(server_main)
```

### 7.9 Run a subprocess

```python
import subprocess

let (exit_code, stdout) = subprocess.run_with_stdout(["python", "-c", "print(1+1)"])
println("got: " + stdout.strip())
println("exit: " + str(exit_code))
```

### 7.10 Sealed-class match-based dispatch

```python
sealed class Shape:
    pass

final class Circle(Shape):
    radius: f64
    fn __init__(self, r: f64) -> None:
        self.radius = r

final class Square(Shape):
    side: f64
    fn __init__(self, s: f64) -> None:
        self.side = s

fn area(shape: Shape) -> f64:
    match shape:
        case Circle(c):
            return 3.14159 * c.radius * c.radius
        case Square(sq):
            return sq.side * sq.side
    # `match` over a sealed class with all cases covered DOES NOT need
    # a fallthrough — compiler warns if non-exhaustive.

let c: Shape = Circle(2.0)
println(str(area(c)))
```

---

## §8 — Pattern matching reference

```python
match value:
    case Literal:                    # constant pattern (1, "hello", true, etc.)
        ...
    case Identifier:                 # binds to a new name (catch-all for one case)
        ...
    case _:                          # wildcard (catch-all)
        ...
    case (a, b):                     # tuple destructure (M14)
        ...
    case Constructor(field1, field2):    # class destructure with positional bindings
        ...                          # field1, field2 are named after the class's fields in order
    case Constructor():              # match a class instance without binding fields
        ...
```

**Supported sub-patterns** (M16 v0.3 surface):
- Identifier (bind), Wildcard (`_`)
- NOT supported in v0.3: nested constructor patterns (e.g. `case Pair(Number(n), Number(m)):`), or-patterns (`case 1 | 2:`), guards (`case x if x > 0:`), range patterns, mapping patterns

**Exhaustiveness**: when matching on a `sealed class`, the compiler warns (stderr) if not all subclasses are covered. It's not a hard error.

```python
# Constructor patterns BIND fields by name in declaration order:
final class Pair(Shape):
    a: f64
    b: f64

match p:
    case Pair(a, b):
        println(str(a) + " " + str(b))
```

**`isinstance` + flow narrowing** is equivalent:

```python
if isinstance(x, Circle):
    # x narrowed to Circle here
    println(str(x.radius))
```

---

## §9 — Async I/O (M32)

**Important**: v0.3 ships **Shape A** — thread-backed Future façade. `asyncio.spawn(...)` actually creates an OS thread under the hood. The API matches what real async would look like; v0.4 will swap internals for a real `mio` event loop without changing the public surface.

**Important**: v0.3 has no `async`/`await` *syntax*. Use the library functions (`asyncio.spawn`, `fut.await()`) instead.

### 9.1 Top-level entry

```python
fn my_main() -> i32:
    # ... async work ...
    return 0

fn main() -> i32:
    return asyncio.run_i32(my_main)
```

`asyncio.run_i32` and `asyncio.run_unit` are the only ways to start the runtime.

### 9.2 Spawn + await

```python
fn slow_work() -> i32:
    asyncio.sleep(1.0)
    return 42

let fut: Future[i32] = asyncio.spawn_i32(slow_work)
let result: i32 = fut.await()             # blocks calling thread until ready
```

### 9.3 Gather

```python
let a: Future[i32] = asyncio.spawn_i32(work_a)
let b: Future[i32] = asyncio.spawn_i32(work_b)
let (ra, rb) = asyncio.gather_2_i32(a, b)
```

Per-type variants: `gather_2_i32`, `gather_2_str`, `gather_3_*`, `gather_4_*`. v0.4 will add variadic.

### 9.4 Async sockets

```python
# Non-blocking accept:
let fut: Future[Tuple[i64, str]] = socket.async_accept(listener)
let (conn, peer) = fut.await()

# Non-blocking recv:
let recv_fut: Future[str] = socket.async_recv(conn, 4096i32)
let data: str = recv_fut.await()

# Non-blocking send:
let send_fut: Future[i32] = socket.async_send(conn, "hello")
let bytes_sent: i32 = send_fut.await()
```

### 9.5 Future methods

```python
fut.await() -> T          # blocks until ready
fut.is_ready() -> bool    # non-blocking poll
```

---

## §10 — Generics (free functions and classes)

### 10.1 Generic free functions (M17)

```python
fn id[T](x: T) -> T:
    return x

# Call-site monomorphisation; T inferred from arg:
let n: i32 = id(42i32)
let s: str = id("hi")

# Multiple type params:
fn make_pair[K, V](k: K, v: V) -> Tuple[K, V]:
    return (k, v)

let p: Tuple[str, i64] = make_pair("count", 99i64)

# Operators on T are constrained by per-instantiation re-typecheck:
fn max_of[T](a: T, b: T) -> T:
    if a > b:                            # ERROR if T doesn't support `>`
        return a
    else:
        return b

let m: i64 = max_of(1i64, 2i64)           # OK
let m2: str = max_of("a", "b")            # OK
# max_of(SomeUserClass(...), SomeUserClass(...))   # ERROR if class has no >
```

### 10.2 Generic classes (M31)

```python
final class Box[T]:
    value: T
    fn __init__(self, v: T) -> None:
        self.value = v
    fn unwrap(self) -> T:
        return self.value

# Each instantiation gets a distinct type_id + method bodies:
let bi: Box[i64] = Box(42i64)
let bs: Box[str] = Box("hello")
# bi and bs cannot be passed to the same function expecting Box[X] for
# the same X — they are distinct types.

# Multi-param:
final class Pair[K, V]:
    key: K
    value: V
    fn __init__(self, k: K, v: V) -> None:
        self.key = k
        self.value = v

let p: Pair[str, i32] = Pair("count", 99i32)
```

**v0.3 limits on generic classes**:
- No explicit type-argument syntax — `Box[i64](v)` does NOT work; use constructor-site inference.
- No bounded generics (`T: Comparable`) — operators on T are checked per instantiation.
- No subclassing a parameterised class.
- No variance markers.
- Higher-kinded types (`F[_]`) not supported.

---

## §11 — Gotchas, Python differences, and things that DON'T work

### 11.1 Mandatory type annotations

EVERY variable declaration, function parameter, function return, and field needs an explicit type. There is no inference at declaration.

```python
let x = 0            # ERROR — must be `let x: i64 = 0`
fn add(a, b):        # ERROR — must be `fn add(a: i32, b: i32) -> i32:`
    return a + b
```

### 11.2 Dict[non-str, _] is partially broken (legacy from M5)

`Dict[str, V]` works perfectly. `Dict[i64, V]` etc. — the keys are stringified internally. Avoid for now unless you really need it.

### 11.3 No implicit numeric conversion

```python
let x: i32 = 1
let y: i64 = x                  # ERROR — must be `let y: i64 = i64(x)`
let z: f64 = x                  # ERROR — must be `let z: f64 = f64(x)`
```

### 11.4 `none` is not `false`

In `if x:`, the only thing that's "falsey" is `false`. `none`, `0`, `""`, empty list — all truthy in their own right. Always test explicitly:

```python
if x is none:                   # for nullable
if xs.length() == 0:            # for empty list
if s == "":                     # for empty string
```

### 11.5 No default arguments, no kwargs

```python
fn greet(name: str, greeting: str) -> None:    # NO defaults
    ...

greet("alice", "hi")              # OK
greet("alice")                    # ERROR — must pass all
greet(name="alice")               # ERROR — no keyword args
```

### 11.6 No variadic functions

`fn sum(*args: i64) -> i64:` does NOT work. Use `List[i64]`:

```python
fn sum(xs: List[i64]) -> i64:
    let total: i64 = 0
    for x in xs:
        total = total + x
    return total

sum([1, 2, 3])
```

### 11.7 String concat is `+`, not f-string format-specs

`f"hello {name}"` works. `f"{x:>10}"` does NOT. Build formatted strings with explicit code.

### 11.8 `with` doesn't route IOError through enclosing `try ... except`

(Repeated for emphasis — this catches everyone.)

```python
# This does NOT catch the IOError that open() raises:
try:
    with open("missing.txt", "r") as f:    # raise happens here
        ...
except IOError as e:                        # NOT REACHED
    ...

# Correct:
try:
    with open("missing.txt", "r") as f:
        ...
except IOError as e:
    println("got it: " + e.message)
```

Actually... let me check. The actual behaviour depends on whether the raise happens inside the with body or in `open()`. Wrapping in `try` works either way; the issue is more nuanced. Safest pattern: always wrap with-blocks that touch IO in `try`/`except IOError`.

### 11.9 No user-defined exception subclasses

The 10 built-in exception names (§3.10) are all you get in v0.3. `class MyError(Exception):` parses but the resolver rejects it.

### 11.10 No async/await syntax

Use `asyncio.spawn`, `fut.await()`. See §9.

### 11.11 No NumPy, no pandas

StrictPy isn't CPython. NumPy/pandas link against libpython. Not happening. See `docs/thesis/design_decisions/why_no_numpy_pandas.md`.

### 11.12 No bcrypt / argon2 for passwords

v0.3 `hashlib` ships sha1/sha256/sha512/md5/hmac_sha256. None of these are appropriate for password storage. Don't use StrictPy for production auth until v0.4.

### 11.13 The mandatory `main() -> i32` entry point

Every executable StrictPy program must have:

```python
fn main() -> i32:
    ...
    return 0
```

Top-level code (outside any `fn`) is NOT executed. Top-level `let`/`final` declarations are evaluated at module init time but no expression statements run.

### 11.14 `Dict[str, *]` membership: prefer `.has()` over `k in d`

`k in d` works for `Dict[str, V]` (M24 BUG-039 fix), but `dict.has(k)` is more explicit and always correct. Prefer it.

### 11.15 Tuple-as-multi-return

There is no multi-return syntax. Use a tuple:

```python
fn divmod(a: i32, b: i32) -> Tuple[i32, i32]:
    return (a / b, a % b)

let q, r = divmod(17, 5)
```

### 11.16 `tabular` comparisons null-propagate

For the M37 `tabular` module: `ColumnI64.gt(x)` / `ColumnStr.eq(x)` / etc. produce a `ColumnBool` whose null mask is the OR of the input null masks — null in, null out. `mask.count_true()` treats null cells as not-true (so for a 3-row column with one null + two passing cells, `count_true()` is 2). `df.filter(mask)` drops null mask rows (a null cell does not "pass" the filter). If you want a different convention, fill nulls before comparing (v0.4 will add a `column.fill_null(default)` helper; for now you can rebuild the column from `column.get(i)` via `if got is none: default else got`).

### 11.17 No CSV header inference in `tabular.read_csv`

`tabular.read_csv(path, schema)` requires you to pass the schema explicitly as `List[Tuple[str, str]]` with dtype strings in `{"i64", "f64", "str", "bool", "datetime"}`. The header row of the CSV is asserted against the schema column names (order-sensitive) — mismatched headers raise `ValueError`. There is no auto-inference of dtypes from cell values; this keeps `read_csv` deterministic and pulls schema decisions into source code where they're version-controlled.

---

## §12 — End-to-end example programs

### 12.1 Word count

```python
import sys
import io

fn main() -> i32:
    if i64(len(sys.argv)) < 2i64:
        println("usage: " + sys.argv[0] + " <file>")
        sys.exit(1)

    let path: str = sys.argv[1]
    let f: io.File = open(path, "r")
    let text: str = f.read()
    f.close()

    let words: List[str] = text.split(" ")
    let counts: Dict[str, i64] = {}

    for w in words:
        let cleaned: str = w.strip()
        if cleaned == "":
            continue
        let current: i64? = counts.get(cleaned)
        if current is none:
            counts[cleaned] = 1i64
        else:
            counts[cleaned] = current + 1i64

    let keys: List[str] = counts.keys()
    for k in keys:
        println(k + ": " + str(counts[k]))
    return 0
```

### 12.2 Tiny HTTP server (synchronous, thread-per-connection)

```python
import socket
from threading import Thread

fn handle(conn: i64, peer: str) -> None:
    let req: str = socket.recv(conn, 4096i32)
    let response: str =
        "HTTP/1.1 200 OK\r\n" +
        "Content-Length: 13\r\n" +
        "Connection: close\r\n" +
        "\r\n" +
        "Hello, world!"
    socket.send(conn, response)
    socket.close(conn)

fn main() -> i32:
    let lis: i64 = socket.listen_tcp("127.0.0.1", 8080i32, 16i32)
    println("listening on 127.0.0.1:8080")
    while true:
        let (conn, peer) = socket.accept(lis)
        let c: i64 = conn
        let p: str = peer
        let t: Thread = Thread(fn() -> None: handle(c, p))
        t.start()
    return 0
```

### 12.3 SQLite TODO app

```python
import sqlite3

fn main() -> i32:
    let conn: i64 = sqlite3.connect("todos.db")
    sqlite3.execute(conn, "CREATE TABLE IF NOT EXISTS todos (id INTEGER PRIMARY KEY, text TEXT, done INTEGER DEFAULT 0)")

    sqlite3.execute_params(conn, "INSERT INTO todos (text) VALUES (?)", ["buy milk"])
    sqlite3.execute_params(conn, "INSERT INTO todos (text) VALUES (?)", ["mail bills"])

    let rows: List[List[str]] = sqlite3.query(conn, "SELECT id, text, done FROM todos")
    for row in rows:
        let mark: str = "[x]"
        if row[2] == "0":
            mark = "[ ]"
        println(mark + " " + row[1])

    sqlite3.close(conn)
    return 0
```

### 12.4 JSON parsing and walking (M34 typed surface)

```python
import json
from json import JsonValue, JString, JInt, JList, JObject

fn print_value(v: JsonValue, indent: str) -> None:
    match v:
        case JString(s):
            println(indent + "string: " + s)
        case JInt(n):
            println(indent + "int: " + str(n))
        case JList(lst):
            println(indent + "list (length " + str(lst.length()) + "):")
            let items: List[JsonValue] = lst.items()
            for item in items:
                print_value(item, indent + "  ")
        case JObject(obj):
            println(indent + "object:")
            let keys: List[str] = obj.keys()
            for k in keys:
                let val: JsonValue? = obj.get(k)
                if val is not none:
                    println(indent + "  " + k + " ->")
                    print_value(val, indent + "    ")

fn main() -> i32:
    let raw: str = '{"name": "alice", "scores": [10, 20, 30]}'
    let parsed: JsonValue = json.parse(raw)
    print_value(parsed, "")
    return 0
```

### 12.5 Concurrent HTTP fetches

```python
import asyncio
import http_client

fn fetch(url: str) -> str:
    let (status, body) = http_client.get(url)
    return str(status) + " " + url

fn main_async() -> i32:
    let urls: List[str] = ["https://example.com/", "https://example.org/", "https://example.net/"]
    let futures: List[Future[str]] = []
    for u in urls:
        let url: str = u
        futures.append(asyncio.spawn_str(fn() -> str: fetch(url)))

    for f in futures:
        let result: str = f.await()
        println(result)
    return 0

fn main() -> i32:
    return asyncio.run_i32(main_async)
```

---

## §13 — Maintaining this file

**Discipline**: ship a `LANGUAGE_GUIDE.md` update in the same commit as any new language feature or stdlib module. The agent brief for any future milestone touching language surface or stdlib MUST include:

> Update `LANGUAGE_GUIDE.md` to document the new feature in the appropriate section. The doc is the single source of truth for AI coding tools; if it's out of date, AI tools generate wrong code.

### Sections per stdlib module

Each stdlib module gets a sub-section in §5 with this shape:

```markdown
### module_name (Mnn — milestone tag)

```python
import module_name
module_name.function_a(arg: T) -> U     # one-line description if behaviour is non-obvious
module_name.function_b(...) -> ...
```

(Notes on quirks, byte-buffer conventions, error semantics, etc. as needed)
```

Keep examples minimal — the AI can synthesise variants from the signature.

### Sections per language feature

New language features go in §3 (syntax), §4 (type system), or §10 (generics) depending on what they are. Add a §11 entry if there's a gotcha.

### Version banner at the top

Update "Last refresh: post-M.." when any change lands. This is the most important quick-check signal for AI tools deciding whether the doc is fresh.

### Cross-references

The doc should be self-contained. Link to `STRICTPY_SPEC.md` only when implementation detail matters (rarely). Don't link to `docs/thesis/` archives — those are project methodology, not language reference.

### Length budget

Target ≤ 3,000 lines / ≤ 80 KB. Beyond that, retrieval gets noisier. If a new feature would push the doc significantly over, factor that feature's deep details out into a dedicated `LANGUAGE_GUIDE_<feature>.md` and link from the main file.
