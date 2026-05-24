# StrictPy — language guide for AI coding tools

**Status**: live document. Updated whenever a new language feature, stdlib module, or surface change lands. Last refresh: post-M52 (2026-05-24).

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

### tabular (M37, extended by M38, M39, M40, M41, M42, M43, M44, M45, M46, M47, M49, M50a, M50b, M50c)

First Pandas-shaped data package — native Rust impl (real pandas can't import). Sealed `Column` hierarchy + `DataFrame` with named columns + per-column null mask. First stdlib package to register its classes module-scoped from the start (no prelude bloat — see §6.2). M38 rounds out the M37 STOP-CRITERIA debt and adds per-column aggregations, `df.describe`, `Column.fill_null`, `tabular.from_dict`, and hash-based group-by via a new `GroupedDataFrame` class. M39 adds the Phase 4 reshape surface: per-dtype `unique`, `value_counts`, `concat_rows`/`concat_cols`, `df.merge` (hash-join), `df.pivot`, and `df.melt`. M40 closes the time-series / null-handling / cumulative / range-slicing surface: per-column cumulative ops, whole-frame `dropna` / `fillna_*`, `df.iloc` range slicing, rolling-window aggregations, `df.resample` time-bucketing, and `df.asof_merge`.

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

#### M38 additions

**Typed DataFrame accessors.** Return `none` if the column is absent OR has a different dtype (so callers don't need a separate dtype check).

```python
df.get_column_i64(name: str) -> ColumnI64?
df.get_column_f64(name: str) -> ColumnF64?
df.get_column_str(name: str) -> ColumnStr?
df.get_column_bool(name: str) -> ColumnBool?
df.get_column_datetime(name: str) -> ColumnDateTime?
```

**Restored Phase C comparison ops.** Same null-propagation as M37's `eq`/`gt`/`lt`.

```python
ColumnI64.ne / ge / le(x: i64) -> ColumnBool
ColumnI64.between(lo: i64, hi: i64) -> ColumnBool   # inclusive both ends
ColumnF64.ne / ge / le(x: f64) -> ColumnBool
ColumnF64.between(lo: f64, hi: f64) -> ColumnBool
ColumnStr.starts_with(prefix: str) -> ColumnBool
ColumnStr.ends_with(suffix: str) -> ColumnBool

df.rename(renames: List[Tuple[str, str]]) -> DataFrame
```

**Per-column aggregations.** Skip null cells. `count_*` always returns a concrete `i64`; other aggregations return `T?` and produce `none` when every cell is null. NaN cells on `ColumnF64` are NOT treated as null — they propagate per IEEE-754 (see §11).

```python
ColumnI64.sum() -> i64?       # 0 only on all-null is wrong; returns none.
ColumnI64.mean() -> f64?      # i64 column, f64 result.
ColumnI64.min() -> i64?
ColumnI64.max() -> i64?
ColumnI64.count() -> i64      # non-null cell count.
ColumnI64.std() -> f64?       # sample stdev (n-1); none if <2 non-null.
ColumnI64.var() -> f64?       # sample variance.
ColumnI64.median() -> f64?    # linear-interpolated 0.5 quantile.

ColumnF64.{sum,mean,min,max,count,std,var,median}   # same shape; sum/min/max return f64?
ColumnStr.{count,min,max}                            # min/max lexicographic, return str?
ColumnBool.count() -> i64                            # count_true/false/null from M37 already.
ColumnDateTime.{count,min,max}                       # min/max return i64? (epoch-ms).
```

**`df.describe`.** Returns a 5-row × (1 + ncols) summary frame. The row-index column is named "statistic" with values `["count", "mean", "std", "min", "max"]`; every cell is stringified. Non-numeric columns only populate the "count" row.

```python
df.describe() -> DataFrame
```

**`Column.fill_null(v)`.** Returns a fresh column with null cells replaced by `v` and the nulls mask zeroed.

```python
ColumnI64.fill_null(v: i64) -> ColumnI64
ColumnF64.fill_null(v: f64) -> ColumnF64
ColumnStr.fill_null(v: str) -> ColumnStr
ColumnBool.fill_null(v: bool) -> ColumnBool
ColumnDateTime.fill_null(v_ms: i64) -> ColumnDateTime
```

**`tabular.from_dict(d: Dict[str, Column]) -> DataFrame`.** Constructs a frame from a `(name → column)` dict. Column order follows lexicographic key sort (the M5 `Dict` storage does NOT preserve insertion order — see §11).

**Group-by.** New `GroupedDataFrame` class. Multi-column keys serialize via `\x01` separators; null cells in a key column form their own "null" bucket (matches pandas's `dropna=False` mode; v1 default).

```python
df.group_by(cols: List[str]) -> GroupedDataFrame

gdf.size() -> DataFrame                       # group_key cols + a "size" i64 col
gdf.keys() -> DataFrame                       # just the group-key cols, one row per group

# Aggregation shortcuts (skip key columns; apply to numeric cols
# for sum/mean and to numeric+datetime for min/max):
gdf.sum() -> DataFrame
gdf.mean() -> DataFrame
gdf.min() -> DataFrame
gdf.max() -> DataFrame
gdf.count() -> DataFrame                      # i64 column for every non-key column.

# Custom agg via (col_name, agg_name) spec list:
gdf.agg(specs: List[Tuple[str, str]]) -> DataFrame
# specs e.g. [("price", "sum"), ("qty", "mean")] yields columns
# "price_sum" + "qty_mean".  Valid agg names: sum / mean / min /
# max / count / std / var / median.
```

See `examples/tabular_groupby_demo.spy` for an end-to-end M38 walkthrough (filter → aggregate → group-by → rename).

#### M39 additions — reshape

**Per-dtype `unique` accessors.** Return a fresh column of distinct non-null values in first-occurrence order. Return `none` when the named column is absent OR has a different dtype (so callers don't need a separate dtype check).

```python
df.unique_i64(col: str) -> ColumnI64?
df.unique_f64(col: str) -> ColumnF64?     # NaN-aware: bit-pattern equality
df.unique_str(col: str) -> ColumnStr?
df.unique_bool(col: str) -> ColumnBool?
df.unique_datetime(col: str) -> ColumnDateTime?
```

**`df.value_counts(col)`.** Returns a 2-column frame: the source column's values (dtype preserved, name preserved) + a `count: i64` column. Sorted by count descending; ties broken by first-occurrence order (stable). Null cells are excluded. Raises `ValueError` if the column is missing.

```python
df.value_counts(col: str) -> DataFrame    # 2 cols: <col> + "count"
```

**`tabular.concat_rows(dfs)`.** Vertical concatenation (stack rows). All input dfs must have identical column schemas — same names + dtypes in the same order. Empty input list raises `ValueError`.

**`tabular.concat_cols(dfs)`.** Horizontal concatenation (stitch columns). All input dfs must have identical row counts. Column names must be globally unique across all input dfs (no auto-rename in v1).

```python
tabular.concat_rows(dfs: List[DataFrame]) -> DataFrame
tabular.concat_cols(dfs: List[DataFrame]) -> DataFrame
```

**`df.merge(other, on, how)`.** Hash-join. `on` is a list of column names that must exist in both frames with matching dtypes. `how` ∈ `"inner" | "left" | "right" | "outer"` (any other value raises `ValueError`). Output schema is `self`'s columns in order, then `other`'s non-`on` columns in order — no duplicate column names. A row whose `on` cells contain any null never matches (see §11.20).

```python
df.merge(other: DataFrame, on: List[str], how: str) -> DataFrame
```

**`df.pivot(index, columns, values)`.** Long-to-wide reshape. `index` chooses the row label column, `columns` chooses the value source whose unique values become new column headers, and `values` fills the cells. Output column names for the pivoted columns are stringified versions of the unique `columns` values. Raises `ValueError` on duplicate `(index, columns)` pairs (see §11.21). Missing pairs emit null cells.

```python
df.pivot(index: str, columns: str, values: str) -> DataFrame
```

**`df.melt(id_vars, value_vars)`.** Wide-to-long reshape. All `value_vars` columns must share a dtype (else `ValueError`). Output is `len(id_vars) + 2` columns × `nrows * len(value_vars)` rows: the id_vars columns + a `variable: str` column + a `value: <shared dtype>` column. Row order is source-row-major then value_var-minor.

```python
df.melt(id_vars: List[str], value_vars: List[str]) -> DataFrame
```

See `examples/tabular_reshape_demo.spy` for an end-to-end M39 walkthrough (unique → value_counts → merge → pivot → melt → concat).

#### M40 additions — time series, cumulative, null handling, range slicing

Phase 5 of the Pandas-shaped data package. Closes the time-series + null-handling + range-slicing surface that real workflows hit constantly. **DatetimeIndex is deferred** — time-series ops take a column-name argument matching the existing `tabular` idiom (`df.sort_by("date", true)`, `df.group_by(["category"])`).

**Cumulative reductions on numeric columns.** Running aggregates with null-propagation: once a null cell is encountered, the output is null at that position and every position after (v1 simplification — pandas's `min_periods=1` skip-nulls behavior is not implemented). NaN on f64 propagates per IEEE-754.

```python
ColumnI64.cumsum() -> ColumnI64
ColumnI64.cumprod() -> ColumnI64
ColumnI64.cummax() -> ColumnI64
ColumnI64.cummin() -> ColumnI64

ColumnF64.cumsum() -> ColumnF64
ColumnF64.cumprod() -> ColumnF64
ColumnF64.cummax() -> ColumnF64
ColumnF64.cummin() -> ColumnF64
```

**Whole-frame null handling.** `dropna` drops every row with at least one null in any column; `dropna_subset` only considers the listed columns. The `fillna_*` per-dtype methods replace nulls in matching columns; other columns pass through unchanged (the per-dtype split mirrors M38's `get_column_*` — `fillna(any)` with a runtime-dispatched value doesn't fit StrictPy's typing).

```python
df.dropna() -> DataFrame
df.dropna_subset(cols: List[str]) -> DataFrame

df.fillna_i64(v: i64) -> DataFrame          # fills only ColumnI64 columns
df.fillna_f64(v: f64) -> DataFrame
df.fillna_str(v: str) -> DataFrame
df.fillna_bool(v: bool) -> DataFrame
df.fillna_datetime(v: i64) -> DataFrame     # epoch-ms
```

**Range slicing.** Half-open `[start, stop)`. Negative indices raise `ValueError` (v1 simplification — pandas accepts -1). `stop > nrows` clamps to `nrows`; `start > nrows` yields an empty frame.

```python
df.iloc(start: i64, stop: i64) -> DataFrame
```

**Rolling-window aggregations.** Output length = input length. Cells `0..window-1` are null (incomplete window — matches pandas's default `min_periods=window`). A window containing any input null produces null in that output position. `rolling_mean` / `rolling_std` return `ColumnF64` even on i64 input. `rolling_std` is sample std (n-1 denominator). `window < 1` or `window > nrows` raises `ValueError`.

```python
ColumnI64.rolling_sum(window: i64) -> ColumnI64
ColumnI64.rolling_mean(window: i64) -> ColumnF64
ColumnI64.rolling_min(window: i64) -> ColumnI64
ColumnI64.rolling_max(window: i64) -> ColumnI64
ColumnI64.rolling_std(window: i64) -> ColumnF64

ColumnF64.rolling_{sum,mean,min,max,std}(window: i64) -> ColumnF64
```

**Time-bucket resample.** `time_col` names a `ColumnDateTime` column. `rule` is `<i64><m|h|d>` (e.g. `"5m"`, `"1h"`, `"1d"`, `"7d"`); other patterns raise `ValueError`. `agg` ∈ `{"sum", "mean", "min", "max", "count"}` applied to every non-time numeric column. String and bool columns are dropped from the output. Empty buckets emit a non-null bucket-start time but null cells for the aggregated columns.

```python
df.resample(time_col: str, rule: str, agg: str) -> DataFrame
```

**As-of merge.** Left-joins where each self row matches the largest other row with `other[on_other] <= self[on_self]`. Both keys must share dtype (`ColumnDateTime` or `ColumnI64`) — otherwise `ValueError`. Output is all self columns + all other columns except `on_other` (no duplicate keys). Self rows with no matching other row get null in the right-side slots.

```python
df.asof_merge(other: DataFrame, on_self: str, on_other: str) -> DataFrame
```

See `examples/tabular_timeseries_demo.spy` for an end-to-end M40 walkthrough (cleaned → cumsum → cummax → rolling_mean → resample → asof_merge → iloc → dropna).

#### M41 additions — DatetimeIndex (minimum viable) + pivot_table

Phase 5b of the Pandas-shaped data package. Closes the headline M40 omission (DatetimeIndex) plus adds pandas's most-loved DataFrame method (`pivot_table`).

**Optional index slot.** `DataFrame` gains an internal optional index column + the original column name. Constructors default to a `RangeIndex` (no index — today's behavior), so existing user code keeps working byte-identically. Opt in with `set_index`.

```python
df.set_index(col_name: str) -> DataFrame    # raises ValueError if col_name absent
                                            # or df already has an index
df.reset_index() -> DataFrame               # restores index as col at position 0
                                            # (no-op if no index)
df.has_index() -> bool
df.index() -> Column?                       # none if RangeIndex
df.index_name() -> str?                     # none if RangeIndex
df.sort_index(ascending: bool) -> DataFrame # stable; preserves the index
                                            # raises ValueError if no index
```

**EXPLICIT SCOPE-DOWN (M41) — index-dropping is the v1 default.** Every existing DataFrame method that returns a fresh frame (`filter`, `sort_by`, `head`, `tail`, `iloc`, `select`, `drop`, `merge`, `pivot`, `melt`, `concat`, `dropna`, `fillna_*`, `resample`, `asof_merge`, `set_index` itself, etc.) **drops the index in v1 (M41)** — the result is a `RangeIndex` frame. Only the following M41 methods preserve the index:

- `sort_index(ascending)`
- `resample_index(rule, agg)`
- `asof_merge_index(other)`
- `select_by_label_{i64,str,datetime}(label)` (one-row outputs)

Full index propagation through every method shipped across M42 and M43; M44 adds MultiIndex storage + multi-column `group_by` promotion + minimal propagation through 4 row-selection ops. **Status post-M44: single-column-index surface is closed for v1; MultiIndex surface is M44a (storage + group_by promotion + filter/head/tail/iloc propagation), with M44b lifting the drops through the remaining ops.** See the "M42 additions", "M43 additions", and "M44 additions" subsections below; §11.26 for the single-col propagation table; §11.32 for the M44a/M44b MultiIndex drop table.

**Index-aware time-series.** Variants of M40's `resample` / `asof_merge` that read the key from the DataFrame's index.

```python
df.resample_index(rule: str, agg: str) -> DataFrame
# Requires a ColumnDateTime index.  rule + agg vocabulary identical to
# M40 resample.  Output preserves its own (bucket-start) index.

df.asof_merge_index(other: DataFrame) -> DataFrame
# Both frames must have an index of matching dtype (ColumnDateTime or
# ColumnI64).  Output preserves self's index.
```

**Lookup by index label.** Per-dtype variants (StrictPy has no runtime-dispatched generics — same shape as M38's `get_column_*`). Returns a one-row `DataFrame` or `none` if absent. Duplicate labels are legal but unusual — only the first matching row is returned in v1.

```python
df.select_by_label_i64(label: i64) -> DataFrame?       # requires ColumnI64 index
df.select_by_label_str(label: str) -> DataFrame?       # requires ColumnStr index
df.select_by_label_datetime(label: i64) -> DataFrame?  # epoch-ms; ColumnDateTime
```

**`pivot_table` — pandas's pivot + group-by + agg in one call.** Combines the three operations in a single method.

```python
df.pivot_table(index_col: str, columns_col: str,
               values_col: str, aggfunc: str) -> DataFrame
# aggfunc ∈ {"sum", "mean", "min", "max", "count"} — same vocabulary as
# M38's group-by shortcuts.  Output rows are the unique index_col
# values (first-seen order); output cols are one per unique columns_col
# value (stringified, first-seen order); cells are the aggregated
# values, null where no (index, columns) pair existed in the source.
# Output uses RangeIndex (no propagation in v1).
#
# Output dtype: matches values_col, EXCEPT
#   - "mean" → ColumnF64 (matches M38 mean)
#   - "count" → ColumnI64 (matches M38 count)
```

See `examples/tabular_index_demo.spy` for an end-to-end M41 walkthrough (set_index → resample_index → sort_index → pivot_table → asof_merge_index → select_by_label_str → reset_index).

#### M42 additions — index propagation through existing methods

M42 closes the M41 v1 scope-down: 11 existing DataFrame methods that returned a fresh frame now PROPAGATE the index through their row/column transformations. No new methods, no new IDs — only behavior changes:

- **Row-selection ops (Phase A):** `filter`, `sort_by`, `head`, `tail`, `iloc` — the parent's index is permuted by the same row-selection vector that produces the new column data, then attached to the result.
- **Column-list ops (Phase B):** `select`, `drop`, `rename` — these don't touch row order; the index is cloned unchanged.
- **Null handling (Phase C):** `dropna` and `dropna_subset` permute the index by the surviving-row vector. `fillna_i64` / `fillna_f64` / `fillna_str` / `fillna_bool` / `fillna_datetime` are pure row-pass-throughs and clone the index unchanged.
- **Merge (Phase D):** `merge(other, on, how)` carries an index per pandas-style rules per `how`:
  - `inner` / `left`: result index = self's index, permuted to the kept rows.
  - `right`: result index = other's index.
  - `outer`: result index = self's index for matched/left-only rows + other's index for right-only rows. Requires matching dtypes; on dtype mismatch the result falls back to RangeIndex (v1 simplification — see §11.26).

In every case, if the parent (or the chosen side, for merge) has no index, the result is a RangeIndex frame — same as pre-M42 behavior on un-indexed inputs. **index_name policy:** preserved from the parent (lhs wins for inner/left/outer merge; rhs wins for right merge).

The following methods still drop the index in v1 (M42 scope): `pivot`, `melt`, `group_by` + agg, `pivot_table`, `concat_rows`, `concat_cols`, plus the M40 time-series shortcuts `resample` and `asof_merge` (the index-aware `resample_index` / `asof_merge_index` already carry the index). M43+ may revisit. Column-returning ops (`unique_*`, `value_counts`) trivially have no index — they return a `Column` or a 2-column `DataFrame`.

See `examples/tabular_index_propagation_demo.spy` for an end-to-end M42 walkthrough (set_index → filter → sort_by → dropna_subset → fillna_f64 → merge → select → sort_index).

#### M43 additions — index propagation through reshape + group_by + pivot_table

M43 closes the index-propagation story for the remaining reshape ops. After M43, the `tabular` package is **fully index-aware end-to-end for single-column indexes** (multi-column / MultiIndex is M44+).

- **`pivot_table(index_col, columns_col, values_col, aggfunc)` (Phase A):** the `index_col`'s unique values now become the output's index (`index_name = index_col`). Regular columns are just the unique `columns_col` values.
- **`GroupedDataFrame.{sum, mean, min, max, count, agg, size, keys}` (Phase A):** when called on a single-column `group_by([col])`, the group-key column is promoted to the output's index (`index_name = col`). For `keys()`, the output is a 0-regular-column frame whose index IS the unique keys. **Multi-column `group_by([col1, col2])` retains today's behavior** — group keys stay as regular columns with a RangeIndex. MultiIndex is M44+ territory.
- **`pivot(index, columns, values)` (Phase B):** the `index` argument's unique values become the output's index (same shape change as `pivot_table`).
- **`concat_rows(dfs)` (Phase B):** when every input frame has an index AND all share the same dtype AND all share the same `index_name`, the output's index is the cell-wise concatenation of input indexes (parallel to the column concatenation). Otherwise the output falls back to RangeIndex.
- **`concat_cols(dfs)` (Phase B):** **lhs's index wins** (consistent with M42's merge lhs-wins policy). If the first frame has an index, the output gets that index; otherwise RangeIndex.
- **`melt(id_vars, value_vars)` (Phase C):** if the input has an index, the output's index is the input's index with **each label repeated `len(value_vars)` times** (matches pandas's default melt-on-indexed-frame behavior). Index name + dtype preserved. No index → RangeIndex.

See `examples/tabular_index_reshape_demo.spy` for an end-to-end M43 walkthrough (pivot_table → group_by mean → concat_rows → melt with index-repeat).

#### M44 additions — MultiIndex (storage + multi-col group_by promotion + minimal propagation)

M44 adds the headline missing piece from the v1 single-index story: nested indices that let `group_by([col1, col2])` produce a structured row label rather than retaining the keys as regular columns.

**Storage.** The `DataFrame` payload grows 40 → 56 bytes to carry an optional MultiIndex (`index_levels: List[Column]? + index_names: List[str]?`) alongside the existing M41 single-column index. The two index representations are **mutually exclusive**: a frame has one OR the other OR neither (RangeIndex). `set_index` clears any MultiIndex; `set_index_multi` clears any single-col index.

**6 new methods:**

```python
df.set_index_multi(cols: List[str]) -> DataFrame
# Removes cols from the regular column list, attaches them as a
# MultiIndex.  Raises ValueError if cols is empty, any col is absent,
# or the frame already has any kind of index.

df.reset_index_multi() -> DataFrame
# Drops the MultiIndex, re-inserts each level as a regular column at
# the start (named by index_names[i]).  Returns RangeIndex.  No-op
# if no MultiIndex.

df.index_nlevels() -> i64
# 0 = RangeIndex; 1 = single-col index (M41); N = MultiIndex with N
# levels.  Supplements has_index() — keep has_index() working as
# "any kind of index" (returns true iff nlevels >= 1).

df.index_level(i: i64) -> Column?
# Returns the i-th index level as a Column.  None if i is out of range
# or df has no index.  For a single-col index (nlevels=1), level(0)
# returns the same column as index().

df.index_level_name(i: i64) -> str?
# Returns the i-th level's name.  None if out of range or no index.

df.sort_index_multi(ascending: bool) -> DataFrame
# Stable lexicographic sort by level 0, then level 1, etc.
# ascending=false reverses the lexicographic order.  Raises
# ValueError if df has no MultiIndex (use sort_index() for the
# single-column case).
```

**Multi-column `group_by` promotion.** All 8 group_by aggregation methods (`sum`, `mean`, `min`, `max`, `count`, `agg`, `size`, `keys`) now promote multi-column group keys to a MultiIndex on the result. Detection is by key count:
- 1 key  → M41 single-col index (M43 behavior).
- ≥2 keys → M44 MultiIndex (all keys become levels in order).

`keys()` with ≥2 keys returns a 0-regular-column DataFrame whose MultiIndex is the unique (col1, col2, ...) tuples — same shape principle as M43's single-column `keys()`.

**Propagation in M44a was minimal**; M45 closes the rest.  See §11.32.

See `examples/tabular_multiindex_demo.spy` for an end-to-end M44 walkthrough (multi-col `group_by` sum → `sort_index_multi` → `filter` preserves MultiIndex → `index_level(i)` access → `reset_index_multi` round-trip).

#### M45 additions — full MultiIndex propagation through M42 + M43 ops

M44a shipped MultiIndex propagation through just 4 row-selection ops (`filter`, `head`, `tail`, `iloc`); every other op dropped a MultiIndex back to RangeIndex (explicit M44b anchor).  **M45 lifts the anchor for 14 handlers** — every M42 row/column-transforming op and every M43 reshape op (except `pivot` / `pivot_table`, which can't preserve a MultiIndex because they reshape the row dimension) now propagates a MultiIndex correctly.

**Phase A — M42 ops** (the 8 row/column-transforming handlers):

- `sort_by(col, ascending)` — every level is permuted by the sort permutation.
- `dropna()` / `dropna_subset(cols)` — every level keep-vectored by the row-keep mask.
- `fillna_i64` / `fillna_f64` / `fillna_str` / `fillna_bool` / `fillna_datetime` — pure row pass-through; every level is cloned.
- `select(cols)` / `drop(cols)` / `rename(pairs)` — column-list ops; every level is cloned unchanged.
- `merge(other, on, how)` — same per-`how` policy as M42's single-col merge applied to MultiIndexes: `inner` / `left` use lhs's MultiIndex (each level permuted by the merge's left-row vector); `right` uses rhs's MultiIndex. `outer` with a MultiIndex falls back to RangeIndex (M46 anchor — same shape as M42's existing dtype-mismatch outer fallback).

**Phase B — M43 reshape ops** (the 4 handlers that have a clean target):

- `melt(id_vars, value_vars)` — every level is repeated `len(value_vars)` times (same take-vector pattern as M43's single-col melt).
- `tabular.concat_rows(dfs)` — strict reconciliation: if every frame has a MultiIndex with matching `nlevels`, matching per-level dtype, AND matching per-level name, the output MultiIndex is the cell-wise concatenation per level; any mismatch falls back to the M43 single-col path (which itself falls back to RangeIndex on mismatch).
- `tabular.concat_cols(dfs)` — lhs's MultiIndex wins (every level cloned), matching M42's merge lhs-wins policy.

**Explicit drops in M45** (no clean target — reshape the row dimension): `pivot` and `pivot_table` drop a MultiIndex on input and promote the `index_col` to a fresh single-col index on output.  Same shape as the M43 single-col case.

**Still deferred to M46**: `stack` / `unstack`, `df.loc[label_list]` range-by-label, and the outer-merge MultiIndex fallback (replacing the current RangeIndex fallback for dtype-mismatched indexes).

See `examples/tabular_multiindex_propagation_demo.spy` for an end-to-end M45 walkthrough — a 6-row sales frame's MultiIndex survives `sort_by` → `dropna_subset` → `fillna_i64` → `rename` → `set_index_multi` round-trip → `concat_cols` → `select` → `melt` end to end.

#### M46 additions — stack/unstack + df.loc range + outer-merge MultiIndex + time-series MI + extensions

M46 closes the M45 "what M46 should pick up" list.  After M46 the `tabular` v1 surface is **functionally complete** except for v0.4 polish items (rolling Welford std, categorical column dtype, `df.iloc[rows, cols]` 2-D indexing, negative iloc, more resample rules, desktop UI).

**Phase A — `stack` / `unstack`** (pandas's MultiIndex bread-and-butter):

- `df.stack() -> DataFrame` — pivots every regular column into a new innermost MultiIndex level + a single `value` column.  All regular columns must share a dtype (else `ValueError`).  Output `nlevels = input nlevels + 1` (RangeIndex input → single-col index output; single-col → 2-level MI; MI → (N+1)-level MI).
- `df.unstack() -> DataFrame` — inverse: takes the innermost MultiIndex level and turns it into wide columns.  Input must have a MultiIndex (raises on single-col or no-index).  Output `nlevels = input nlevels - 1`; if the result has 1 level it becomes a single-col index, if 0 a RangeIndex.  Missing `(row_key, col_key)` cells become null.  v1 simplification: unstack distributes the first regular column only.

**Phase B — `df.loc_range_*(start, stop)`** (extends M41's `select_by_label_*`):

- `df.loc_range_i64(start: i64, stop: i64) -> DataFrame`
- `df.loc_range_f64(start: f64, stop: f64) -> DataFrame`
- `df.loc_range_str(start: str, stop: str) -> DataFrame`
- `df.loc_range_bool(start: bool, stop: bool) -> DataFrame`
- `df.loc_range_datetime(start: i64, stop: i64) -> DataFrame`

Returns rows where `start <= index_label <= stop` (inclusive both ends, pandas's `.loc` semantics).  Preserves the parent's row order (does not sort).  Requires a single-col index of the matching dtype; raises on no-index, MultiIndex (M47 follow-up), or dtype mismatch.  Empty range → 0-row frame with the same column schema.

**Phase C — outer-merge MultiIndex fallback + `set_index_list` + pivot_table extensions:**

- **Outer-merge dtype-mismatch fallback**: M42 previously fell back to RangeIndex when outer-joining with dtype-mismatched single-col indexes (`lhs` has `ColumnI64`, `rhs` has `ColumnStr`).  M46 replaces with a **NaN-padded 2-level MultiIndex** — level 0 is the `lhs` index column (with null for right-only rows), level 1 is the `rhs` index column (with null for left-only rows).  Level names follow `lhs.index_name() / rhs.index_name()` (falling back to `"lhs" / "rhs"`).  Matches pandas's outer-merge-with-mismatched-indexes behavior.
- `df.set_index_list(cols: List[str]) -> DataFrame` — unifies single-col + multi-col `set_index` by length dispatch.  1-element list routes to `set_index(cols[0])` (single-col index); ≥2 elements routes to `set_index_multi(cols)` (MultiIndex); empty raises `ValueError`.  Existing `set_index` / `set_index_multi` keep working unchanged.
- `df.pivot_table_aggfunc_list(index_col, columns_col, values_col, aggfuncs: List[str]) -> DataFrame` — same as `pivot_table` but emits one set of value columns per aggfunc.  Output column shape: `"{columns_value}_{aggfunc}"` (e.g. `"north_sum"`, `"north_mean"`, ...).  Same aggfunc vocabulary as M41: `"sum"|"mean"|"min"|"max"|"count"`.
- `df.pivot_table_margins(index_col, columns_col, values_col, aggfunc) -> DataFrame` — same as `pivot_table` but adds a trailing `"All"` row + `"All"` column with the aggfunc applied across the row/column slice.  The bottom-right intersecting cell is the aggfunc over the whole values column.

**Phase D — time-series ops MultiIndex handling:**

- `resample(time_col, rule, agg)` / `resample_index(rule, agg)` — **drop a MultiIndex** (they reshape the row dimension into time buckets — no clean target).  Same shape as `pivot` / `pivot_table` in M45.
- `asof_merge(other, on_self, on_other)` — now **preserves the lhs's MultiIndex** through the left-join (every output row corresponds to one lhs row in order, so the take vector is just `0..l_nrows`).
- `asof_merge_index(other)` — preserves the lhs single-col DateTime index.  MI-only inputs raise because the function's preamble requires a single-col DateTime index.

See `examples/tabular_m46_extensions_demo.spy` for an end-to-end M46 walkthrough — a 6-row wide sales frame threads through `set_index_list(["region"])` → `stack` → `unstack` round-trip → `loc_range_str` → `pivot_table_aggfunc_list` → `pivot_table_margins` → `set_index_list(["category","month"])`.

#### M47 additions — iloc 2-D + negative iloc + rolling Welford/min_periods + ColumnCategorical

M47 is the v0.4 polish round after M46 closed the v1 surface.  Smaller items that didn't fit M46's scope.

**Phase A — `df.iloc_2d` + negative iloc**:

- `df.iloc_2d(row_start: i64, row_stop: i64, col_start: i64, col_stop: i64) -> DataFrame` — half-open `[row_start, row_stop) × [col_start, col_stop)` slice.  Both axes clamped to bounds.  Both accept Python-style negative indices (`-1` = last row / column).  Preserves the parent's index (M44-style propagation through the row dimension).
- `df.iloc(start, stop)` extended to accept negative indices: `df.iloc(-3, -1)` returns the second-to-last 2 rows.  v1's reject-negative contract is lifted.  See §11.35.

**Phase B — rolling Welford std + `*_min_periods` variants**:

- 10 new `Column.rolling_<op>_min_periods(window: i64, min_periods: i64)` methods (sum / mean / min / max / std on `ColumnI64` + `ColumnF64`).  At each output position the handler counts non-null cells in the window `[i-window+1, i]` (or `[0, i]` for incomplete leading windows); emits the aggregate if the count is `>= min_periods` else null.  Valid `min_periods` range: `1 <= min_periods <= window` (else `ValueError`).
- `rolling_std_min_periods` (and `rolling_<op>` indirectly via this code path) uses Welford's online algorithm internally for numerical stability on large values / windows (vs M40's `sumsq - n*mean²` formula which loses precision via catastrophic cancellation).  Tip: on small / well-conditioned inputs Welford produces bit-identical results to M40's naive formula.
- The original `rolling_std(window)` keeps M40's formula for backwards bit-identicality — pin the Welford variant via `rolling_std_min_periods(window, window)` if you need the new code path.

**Phase C — `ColumnCategorical` sealed subclass**:

A new sealed `Column` subclass storing `codes: List[i64]` (per-cell category index) + `categories: List[str]` (distinct values in first-appearance order) + the standard `nulls: List[bool]` mask + `length: i64`.

Constructors:

- `tabular.col_categorical(values: List[str]) -> ColumnCategorical` — categories built by first-appearance order; all inputs non-null.
- `tabular.col_categorical_with_nulls(values: List[str], nulls: List[bool]) -> ColumnCategorical` — null cells get `codes[i] = 0` (don't-care; the nulls mask controls).

Surface:

- Shared `Column` methods (`length`, `dtype`, `is_null`, `null_count`, `get`) — `dtype()` returns `"categorical"`, `get(i)` returns the category string (or none).
- `cc.codes() -> ColumnI64` — the underlying code column (inherits the null mask).
- `cc.categories() -> ColumnStr` — the distinct values, ordered by first appearance.
- `cc.to_strings() -> ColumnStr` — full materialization to a string column.  **This is the v1 integration idiom**: any op that doesn't have a categorical-specific handler should be invoked on `cc.to_strings()` instead.  Examples: `group_by`, `merge`, `filter` (build a `ColumnBool` mask off `cc.to_strings().eq(...)`), `sort_by` (string ordering — see §11.36).
- `df.get_column_categorical(name: str) -> ColumnCategorical?` — typed accessor; returns none on absent name or wrong dtype.

What's NOT in v1 (M48 follow-up):

- Optimized codes-based hashing for `group_by` / `merge` — v1 coerces via `to_strings()` (slow but correct).
- Ordered categorical (where `sort_by` follows the `categories[]` order rather than alphabetical strings).
- `pandas.Categorical.from_codes` reverse constructor.

See `examples/tabular_m47_polish_demo.spy` for an end-to-end M47 walkthrough — an 8-row sales frame threads through `col_categorical(...)` → `categories()` / `codes()` inspection → `to_strings()` + `group_by` → `rolling_mean_min_periods(3, 1)` → `iloc_2d(-5, -1, 0, 2)` → `iloc(-3, 8)` → `get_column_categorical`.

After M47 the `tabular` polish list is largely complete.  Remaining v0.4 items: categorical optimized codes paths (M48), more resample rules (`1w` / `1M` / `1Y` — needs a calendar layer), `df.rolling` chainable, desktop UI (its own milestone).

#### M49 additions — categorical codes optimization + ordered categorical + small polish

M49 ships the PRIMARY follow-up from M48's benchmark: `group_by` and `merge` on `ColumnCategorical` keys now hash on the i64 codes vector directly instead of routing through `to_strings()`.  No API change — the optimization is transparent.  Bench numbers vs M48 baseline (10K rows × 4266 distinct categories at `medium_card_5000`):

- `group_by_cat_via_strings`: StrictPy 5446 ms → **85 ms** (~64× speedup; now ~13× faster than pandas).
- `group_by_cat_via_strings` at low cardinality (`medium`, 8 distinct): StrictPy ~12.8 s (M48 baseline) → **66 ms** (~194× speedup).

**Transparent codes-hash** (no API):

- `df.group_by([col])` and `df.group_by([col1, col2, ...])` detect ColumnCategorical key columns and dispatch to `m49_build_group_index_codes`.  Mixed-dtype group_by (e.g. categorical + i64) still falls back to the string-hash path, which is now ColumnCategorical-aware so the fallback is correct.
- `df.merge(other, on, how)` detects when every `on` column on BOTH sides is ColumnCategorical with bit-identical `categories[]` orderings (same length + same strings in same indices) and hashes on i64 codes.  Mismatched categories[] (different orderings) falls back to the string-hash path automatically.  See §11.38.

**Ordered categorical surface** (3 new NativeFns: 1061-1063):

- `tabular.col_categorical_ordered(values: List[str], categories: List[str]) -> ColumnCategorical` — pin the categories[] ordering up front.  All values must appear in categories (else `ValueError`).  Duplicate categories raise `ValueError`.  Codes are positions in `categories`.  The typical merge-on-codes workflow is `df1_cat = col_categorical_ordered(vals_a, cats)` + `df2_cat = col_categorical_ordered(vals_b, cats)` so both sides share categories[] and the codes-hash fastpath fires.
- `tabular.col_categorical_from_codes(codes: List[i64], categories: List[str]) -> ColumnCategorical` — reverse constructor (useful for round-tripping `cc.codes()` + `cc.categories()` → back into a ColumnCategorical).  Each code must satisfy `0 <= code < len(categories)`.
- `cc.is_ordered() -> bool` — heuristic predicate.  Returns true iff `categories[]` has unreferenced entries (the signature of an explicit-categories build).  See §11.36 for the v1 nuance.

**Phase D small polish**:

- `df.resample(time_col, "1w", agg)` — weekly buckets (fixed-width: 7 × 86_400_000 ms).  `df.resample(time_col, "1M", agg)` and `"1Y"` — monthly / yearly buckets via calendar arithmetic (Howard Hinnant's `days_from_civil` / `civil_from_days` for proleptic Gregorian).  End-of-month clamping applies: Jan 31 + 1M = Feb 29 (leap year) or Feb 28 (non-leap).  Feb 29 + 1Y in a non-leap year clamps to Feb 28.  See §11.37.
- **Outer-merge MultiIndex on either side** — extends M46's NaN-padded 2-level fallback (which previously fired only for dtype-mismatched single-col indexes on both sides).  Now handles three new cases: lhs MultiIndex + rhs single-col (rhs becomes the last level), lhs single-col + rhs MultiIndex (lhs becomes the first level), both MultiIndex with equal level count + matching level dtypes (stitched level-by-level).  Mismatched level counts or level dtypes falls back to RangeIndex (documented v1 limitation).
- **`df.unstack()` distributes every regular column** (M46 only used the first).  Single-regular-column input preserves M46's `"{innermost_value}"` output naming (byte-compatible); multi-regular uses `"{innermost_value}_{source_col_name}"` (pandas behavior).
- **`df.loc_range_multi_{i64, str, datetime}(start, stop)`** — innermost-level range filter on a MultiIndex (outer levels left intact).  Raises `ValueError` if the frame has no MultiIndex.  Range filtering on outer levels is M51 work.

See `examples/tabular_m49_codes_demo.spy` for an M49 walkthrough — builds a small ordered categorical, exercises codes-hash group_by + merge, runs `1w` / `1M` resample, and verifies `unstack` distributes both regular columns.

After M49 the `tabular` v0.4 polish surface is essentially complete.  M51 should pick up `RollingWindow` chainable + `center=True` + pandas-style ordered-sort on `ColumnCategorical` + range filtering on outer MultiIndex levels.

#### M50a additions — `tabular.serve` HTTP transport + minimal browser-tab UI

M50a starts the desktop-UI track: ship a localhost HTTP server that exposes a DataFrame as JSON + a minimal bundled HTML/JS frontend.  Implementation lives in `vm/src/builtins.rs::m50a_serve_loop` and uses `std::net::TcpListener` directly — no `hyper`, no `axum`, no `tokio`, no crate deps beyond libstd.  The M28 socket stdlib and M29 webserver framework are deliberately NOT a dependency; see §11.39.

```python
import tabular
from tabular import DataFrame

tabular.serve(df: DataFrame, port: i32) -> i32
# Boot a localhost HTTP/1.1 server on 127.0.0.1:<port>.  Runs until
# Ctrl-C or the parent process dies (24h hard cap as a safety net).
# Returns the exit code (0 = clean shutdown, nonzero = bind / I/O
# error).  Blocks the calling thread — for concurrency wrap in
# Thread.new (M5).

tabular.serve_with_timeout(df: DataFrame, port: i32,
                            timeout_ms: i64) -> i32
# Same as serve(...) but shuts down after timeout_ms milliseconds.
# Use this in tests and scripts that need deterministic shutdown
# (calling the unbounded serve() from a test would hang).
```

Endpoints exposed by both functions:

- `GET /` — bundled HTML+JS frontend (vanilla DOM, ~550 LOC of JS, sticky sortable headers + index-column prefix rendering + composite-filter panel + groupby checkbox UI w/ sort toggle + Pivot panel + Chart panel (canvas-based bar/line/histogram) + CSV download + reset-to-primary button + DOM-recycle pagination capped at 5000 rendered rows).
- `GET /api/schema?df=ID` — `{"names": [...], "dtypes": [...], "nrows": N, "has_index": bool, "index_name": "...", "index_nlevels": N, "index_names": [...], "index_dtypes": [...]}`.  The `index_names`/`index_dtypes` arrays are per-level (M50c) and let the frontend render index columns as a distinct prefix.
- `GET /api/rows?df=ID&start=N&stop=M` — `{"start": N, "stop": M, "nrows": Total, "rows": [[...], ...], "index": [[...], ...]}`.  Cells are typed JSON values (`i64` → number, `f64` → number or null on NaN/Inf, `str` → string, `bool` → boolean, `datetime` → epoch-ms number, **`categorical` → string** since M50b — M50a rendered as null).  Null cells emit `null`.  The `index` array is parallel to `rows` (one entry per row, each entry is a list of N level values) and is empty arrays for RangeIndex frames (M50c).
- `GET /api/cell?df=ID&row=R&col=C` — `{"value": <cell>}` or 400 with a JSON error body on bad indices.
- `GET /api/csv?df=ID` — `text/csv` body for the full frame (M50b).  Header row + one row per data row; null cells become empty fields; datetime cells are ISO-8601; categorical cells resolve through codes → category strings.
- `POST /api/filter` — body `{"df": ID, "column": "name", "op": "eq|ne|gt|lt|ge|le", "value": <typed>}`; returns `{"df": NEW_ID, "nrows": N}` on success.  Server-side DataFrame ID registry holds derived dfs at fresh IDs.
- `POST /api/filter_multi` — body `{"df": ID, "logic": "and"|"or", "clauses": [{column, op, value}, ...]}` (M50b).  Composite AND/OR filtering; each clause shape matches `/api/filter`.
- `POST /api/groupby` — body `{"df": ID, "by": ["col1", "col2"], "agg": {"col3": "sum", "col4": "mean"}, "sort": false}`.  M50c added the optional `sort` flag — when true, the agg result is sorted by the group keys (uses `sort_index_multi` for multi-key group_by where keys live in a MultiIndex; `sort_index` for single-key where the key is the index; falls back to `sort_by(by[0])` for the rare regular-column shape).  Returns `{"df": NEW_ID, "nrows": N}`.
- `POST /api/sort` — body `{"df": ID, "column": "name", "ascending": true|false}` (M50b).  Returns `{"df": NEW_ID, "nrows": N}`.  The bundled frontend wires column-header clicks to this endpoint with toggling ascending/descending.
- `POST /api/pivot` — body `{"df": ID, "index": "X", "columns": "Y", "values": "Z", "aggfunc": "sum|mean|min|max|count"}` (M50c).  Server-side wrapper around `df.pivot_table`.  Returns `{"df": NEW_ID, "nrows": N}` or 400 on missing/unknown column or invalid aggfunc.
- `POST /api/forget` — body `{"df": ID}` (M50b).  Drops a derived DataFrame from the registry.  Returns `{"ok": true|false}` (false for unknown IDs and for the primary df, id=0, which is refused).

Missing `df` query param defaults to ID 0 (the primary df passed to `serve`).  Unknown `df` returns 404.

See `examples/tabular_serve_demo.spy` for a working walkthrough.  The demo includes a ColumnCategorical column so M50b categorical rendering shows up in the frontend, and the interactive Pivot + Chart panels (M50c) work against it out of the box.

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

### gfx (M52-M53)

The 2D graphics, windowing, and input package, built on native SDL2 and SDL2_image. Provides the core primitives to create game loops, draw rectangles/lines/pixels, load sprite sheets, and poll keyboard/mouse events.

```python
import gfx
from gfx import Window, Event, Image
```

#### Classes

- `gfx.Window` — Opaque handle for an OS window and hardware-accelerated 2D renderer. Must be closed explicitly.
- `gfx.Image` — Opaque handle for a loaded GPU texture. Must be freed explicitly.
- `gfx.Event` — Open class representing a user input event. Fields:
  - `kind: str` — One of `"key_down"`, `"key_up"`, `"mouse_down"`, `"mouse_up"`, `"mouse_move"`, or `"quit"`.
  - `key: str` — The key name for keyboard events (e.g. `"left"`, `"right"`, `"escape"`, `"space"`, `"enter"`, lowercase letters like `"a"`).
  - `x: i32`, `y: i32` — Coordinates for mouse events.
  - `button: i32` — Mouse button for mouse events (1 = left, 2 = middle, 3 = right).

#### Functions

```python
gfx.init() -> i32
# Initialize SDL2 video and events subsystems. Idempotent. Returns 0 on success.

gfx.create_window(title: str, width: i32, height: i32) -> Window
# Create an OS window with logical width and height.

gfx.close_window(win: Window) -> None
# Destroy the window and its renderer. Calling other gfx functions with this window raises ValueError.

gfx.poll_event(win: Window) -> Event?
# Return the next input event, or none if the queue is empty. Non-blocking.

gfx.clear(win: Window, r: i32, g: i32, b: i32) -> None
# Fill the window with a solid color.

gfx.present(win: Window) -> None
# Flip the back buffer to the screen. Call once per frame after drawing.

gfx.draw_rect(win: Window, x: i32, y: i32, w: i32, h: i32, r: i32, g: i32, b: i32, a: i32) -> None
# Draw a filled rectangle with transparency (alpha: 0..255).

gfx.draw_rect_outline(win: Window, x: i32, y: i32, w: i32, h: i32, r: i32, g: i32, b: i32, a: i32) -> None
# Draw a 1-pixel rectangle outline.

gfx.draw_line(win: Window, x1: i32, y1: i32, x2: i32, y2: i32, r: i32, g: i32, b: i32, a: i32) -> None
# Draw a 1-pixel line segment from (x1, y1) to (x2, y2).

gfx.draw_point(win: Window, x: i32, y: i32, r: i32, g: i32, b: i32, a: i32) -> None
# Draw a single pixel.

gfx.window_size(win: Window) -> Tuple[i32, i32]
# Returns (width, height) tuple of the window.

gfx.set_window_title(win: Window, title: str) -> None
# Change the window title dynamically.

gfx.load_image(win: Window, path: str) -> Image
# Load a PNG/JPG/BMP image into a GPU texture. Path is relative to the current working directory. Raises IOError if not found, ValueError on unsupported format.

gfx.image_size(img: Image) -> Tuple[i32, i32]
# Returns the (width, height) of the image.

gfx.draw_image(win: Window, img: Image, dst_x: i32, dst_y: i32) -> None
# Blit the image onto the window at its native size.

gfx.draw_image_rect(win: Window, img: Image, src_x: i32, src_y: i32, src_w: i32, src_h: i32, dst_x: i32, dst_y: i32, dst_w: i32, dst_h: i32) -> None
# Blit a sub-rectangle of the image (src_*) onto a destination area (dst_*) of the window, scaling if necessary.

gfx.draw_image_rotated(win: Window, img: Image, dst_x: i32, dst_y: i32, dst_w: i32, dst_h: i32, angle_deg: f64) -> None
# Draw the image scaled to dst_* and rotated around its center by angle_deg degrees.

gfx.free_image(img: Image) -> None
# Explicitly drop/free the image texture resource. Calling other functions with this image raises ValueError.
```

See `examples/_smoke_window.spy` and `examples/_smoke_sprite.spy` for minimal game loop and sprite examples.

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

**M37 `tabular` is the first stdlib package to register its classes module-scoped from the start (no prelude bloat).** The 6 classes — `Column` + 5 final subclasses (`ColumnI64` / `ColumnF64` / `ColumnStr` / `ColumnBool` / `ColumnDateTime`) + `DataFrame` — are reachable only via `from tabular import …` (or `import tabular` + `tabular.ColumnI64` style annotations). There is no bare-name fallback. M38 adds a 7th class to the module: `GroupedDataFrame` (returned by `df.group_by(cols)`) — same module-scoped registration. See §5 `tabular` entry for the full surface.

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

### 11.18 `tabular` f64 aggregations propagate NaN (don't skip)

Aggregations over a `ColumnF64` (`sum` / `mean` / `min` / `max` / `std` / `var` / `median`) skip cells whose **null mask** is true, but they do NOT skip cells whose **value** is NaN. NaN propagates per IEEE-754: a `sum()` that touches a NaN cell returns NaN, a `mean()` of a NaN-bearing column is NaN, and `min`/`max` of a column containing any NaN return NaN. This matches how `numpy.sum`/`mean` (not `numpy.nansum`/`nanmean`) behave. If you want NaN-skip semantics, either set the null mask for the NaN cells or filter them out via `col.fill_null(...)` after coercing them to a sentinel.

The null mask and "NaN" are two distinct concepts in `tabular`: `nulls[i] == true` means "the cell is missing"; a finite-NaN value at `values[i]` means "the cell holds the IEEE-754 NaN value" (legal for f64). The factory functions don't sniff NaN — they trust whatever you put in `values`.

### 11.19 `tabular.from_dict` sorts column order lexicographically

`tabular.from_dict(d: Dict[str, Column])` returns a DataFrame whose column order follows `sorted(d.keys())`. This is a v1 simplification: the underlying `Dict[K, V]` storage (M5) is a `HashMap` and does NOT preserve insertion order, so we sort to get deterministic output. If you need a specific column order, either pass the names directly to `tabular.from_columns(names, cols)` or `select` after construction.

A v0.4 migration of `Dict` to an order-preserving `IndexMap` would change this to "follow insertion order", matching real pandas's `pd.DataFrame(dict)` behavior.

### 11.20 `tabular` merge: null join keys never match

For the M39 `df.merge(other, on, how)`: a row whose `on` cells contain any null does NOT match anything on the other side, regardless of how the other side's null mask looks. This matches pandas's `null != null` SQL semantics. Consequence: in `inner`/`right` joins such rows are dropped; in `left`/`outer` joins they emit with the right-side columns null-filled. If you want null-keyed rows to match each other, replace nulls with a sentinel (`col.fill_null(...)`) before merging.

### 11.21 `tabular` pivot raises on duplicate (index, columns) pairs

`df.pivot(index, columns, values)` requires that each `(index_value, columns_value)` pair appears at most once in the source frame — otherwise it would have to choose between conflicting `values` cells. v1 raises `ValueError` on the first duplicate. If you have duplicates and want to aggregate them, run `df.group_by([index, columns]).agg(...)` first, then pivot the result. (A future `df.pivot_table(..., aggfunc=...)` matching pandas would fold the agg in, but v1 keeps the two operations separate.)

### 11.22 `tabular` cumulative ops propagate nulls forward

For the M40 `Column.cum{sum,prod,max,min}` family: once a null cell is encountered, the output is null at that position AND every position after. v1 picks this "propagate from first null forward" semantics rather than pandas's default `min_periods=1` skip-nulls behavior — it's simpler to reason about, and skip-null behavior can be obtained explicitly with `col.fill_null(identity).cum*()` (use 0 for sum, 1 for prod). NaN on f64 separately propagates per IEEE-754; only the explicit null mask triggers the "all output forward is null" rule.

### 11.23 `tabular` rolling-window leading cells are null

For the M40 `Column.rolling_*(window)` family: the first `window - 1` output cells are always null (incomplete window — matches pandas's default `min_periods=window`). A window containing any input null produces a null in that output position. `window < 1` or `window > nrows` raises `ValueError`. `rolling_mean` and `rolling_std` return `ColumnF64` even on `ColumnI64` input.

### 11.24 `tabular` resample rule format and DatetimeIndex deferral

For the M40 `df.resample(time_col, rule, agg)`: M40 does NOT add a DatetimeIndex; the `time_col` argument names a `ColumnDateTime` column in the frame, matching the existing `tabular` idiom (`df.sort_by("date", true)`, `df.group_by(["category"])`). The `rule` string is `<i64><m|h|d>` only (e.g. `"5m"`, `"1h"`, `"1d"`, `"7d"`); week / month / year suffixes are NOT supported in v1 — `7d` is the closest "weekly" approximation. String + bool columns are silently dropped from the output (no v1 agg for them). Empty buckets emit a non-null bucket-start time but null cells in every aggregated column.

### 11.25 `tabular.asof_merge` requires same-dtype keys

For the M40 `df.asof_merge(other, on_self, on_other)`: both key columns must share dtype — either both `ColumnDateTime` (matching the typical time-series-merge case) or both `ColumnI64`. Mixed-dtype keys raise `ValueError`. The match rule is `other[on_other] <= self[on_self]` for the largest such row; self rows with no matching other row get null in the right-side columns. Null cells in either key column are treated as non-matchable.

### 11.26 `tabular` DatetimeIndex propagation rules (post-M45)

M41 shipped the optional DatetimeIndex slot but every existing method that returned a fresh frame **dropped the index in v1**. M42 closed that scope-down for the 11 most-used DataFrame methods; M43 closed it for the remaining reshape ops. M44 added MultiIndex storage + multi-column `group_by` promotion + minimal propagation through 4 row-selection ops; M45 lifted the M44a anchor for 14 more handlers. Today the `tabular` package is **fully index-aware end-to-end for both single-column indexes AND MultiIndex** — see §11.32 for the post-M45 drop list (only `pivot` / `pivot_table` and the time-series ops still drop a MultiIndex, plus the outer-merge dtype-mismatch fallback).

**Preserve the parent's index (post-M43):**

- Row-selection ops (M42): `filter`, `sort_by`, `head`, `tail`, `iloc` — index permuted by the row-selection vector.
- Column-list ops (M42): `select`, `drop`, `rename` — index cloned unchanged.
- Null handling (M42): `dropna`, `dropna_subset`, `fillna_i64`, `fillna_f64`, `fillna_str`, `fillna_bool`, `fillna_datetime`.
- Merge (M42): `merge(other, on, how)` — per-`how` rules below.
- Reshape that promotes a key to the index (M43): `pivot_table(index_col, ...)`, `pivot(index, ...)` — the `index_col` / `index` argument becomes the output's index.
- Single-column group_by aggregation (M43): `group_by([col]).{sum, mean, min, max, count, agg, size, keys}` — the key column is promoted to the result's single-col index.
- Multi-column group_by aggregation (M44): `group_by([col1, col2, ...]).{sum, mean, min, max, count, agg, size, keys}` — all keys are promoted to a MultiIndex on the result. See §11.32 for the M44a `filter / head / tail / iloc` minimal-propagation contract and the M44b anchor list (every other op currently drops the MultiIndex back to RangeIndex).
- Concatenation (M43): `concat_rows(dfs)` concatenates indexes when all share dtype + name (else RangeIndex fallback — see §11.31). `concat_cols(dfs)` takes lhs's index (consistent with merge lhs-wins).
- Reshape that repeats the index (M43): `melt(id_vars, value_vars)` — each input row's index label appears once per `value_var` in the output (see §11.30).
- M41 index-aware ops: `sort_index`, `resample_index`, `asof_merge_index`, `select_by_label_*`.

**Still drop the index (v1 scope, M44b+ anchors):**

- M40 time-series shortcuts that don't read the index: `resample`, `asof_merge` (use the `_index` variants to preserve an index).
- For frames carrying a MultiIndex (M44+): every op other than `filter / head / tail / iloc` drops the MultiIndex back to RangeIndex in M44a — see §11.32 for the explicit list.

Column-returning ops (`unique_*`, `value_counts`) trivially carry no index — they return a `Column` or a small 2-column `DataFrame`.

**Merge index-propagation rules per `how`:**

- `inner`: result index = self's index restricted to matched rows. Falls back to RangeIndex if self has no index.
- `left`: result index = self's index (all left rows preserved). Falls back to RangeIndex if self has no index.
- `right`: result index = other's index (all right rows preserved). Falls back to RangeIndex if other has no index.
- `outer`: result index = self's index for matched/left-only rows + other's index for right-only rows. **Requires both indexes share a dtype**; on mismatch the result falls back to RangeIndex (v1 simplification — pandas's NaN-padded MultiIndex output is M44+ territory).
- `index_name` policy: lhs wins for inner/left/outer; rhs wins for right.

### 11.27 `tabular.select_by_label_*` returns the first matching row on duplicates

The M41 `df.select_by_label_{i64, str, datetime}(label)` family returns a one-row `DataFrame` (or `none` if the label is absent from the index). If the index has duplicate labels — legal but unusual — only the **first matching row** in current row order is returned. To get every matching row, use `df.filter(df.get_column_*(name).eq(label))` — post-M42 the filtered frame preserves the index (see §11.26 for the full propagation table).

### 11.28 `tabular.pivot_table` aggfunc vocabulary

The M41 `df.pivot_table(index_col, columns_col, values_col, aggfunc)` accepts the same `aggfunc` vocabulary as M38's group-by shortcuts: `"sum" | "mean" | "min" | "max" | "count"`. Other values raise `ValueError`. Output value-cell dtype matches `values_col` except: `"mean"` always produces `ColumnF64`, `"count"` always produces `ColumnI64`. Missing `(index, columns)` cells are null. Row + column orderings are first-seen-in-source. **Post-M43:** the output's index is the unique `index_col` values (`index_name = index_col`); regular columns are just the `columns_col` value-stringifications. Pre-M43 the index_col was the first regular column with a RangeIndex output.

### 11.30 `tabular.melt` repeats the input index per `value_var`

Post-M43, if `df.melt(id_vars, value_vars)` is called on an indexed frame, the output's index is the input's index with **each label repeated `len(value_vars)` times** (one occurrence per value_var per input row). The index name and dtype are preserved. If the input has no index, the output uses RangeIndex. This matches pandas's default behavior for melt on an indexed frame.

### 11.31 `tabular.concat_rows` index reconciliation rules

Post-M43, `tabular.concat_rows(dfs)` reconciles per-frame indexes by these rules:

- If **every** input frame has an index AND all input indexes share the same dtype AND all share the same `index_name`: the output's index is the cell-wise concatenation of input indexes (parallel to the per-column concatenation).
- Otherwise (any frame missing an index, or any mismatch in dtype / name): the output falls back to RangeIndex.

For `concat_cols(dfs)`, **lhs's index wins** — if the first frame has an index, the output gets it (cloned); otherwise RangeIndex. Other dfs' indexes are ignored. This mirrors M42's merge lhs-wins policy.

### 11.32 `tabular` MultiIndex propagation rules (post-M46)

M44a shipped MultiIndex storage + multi-column `group_by` promotion + minimal propagation through 4 row-selection ops.  M45 lifted that scope-down for 14 row/column-transforming and reshape handlers.  **M46 closes the remaining anchors**: `stack` / `unstack` ship; `asof_merge` now preserves the lhs's MultiIndex; outer-merge with dtype-mismatched single-col indexes now produces a 2-level NaN-padded MultiIndex (replacing the old RangeIndex fallback).

**Preserves the MultiIndex (post-M46):**

- M44a-shipped row-selection ops: `filter(mask)`, `head(n)`, `tail(n)`, `iloc(start, stop)` — every level is permuted/sliced by the row-selection vector.
- M45 Phase A row/column-transforming ops: `sort_by`, `dropna`, `dropna_subset`, `fillna_i64` / `fillna_f64` / `fillna_str` / `fillna_bool` / `fillna_datetime`, `select`, `drop`, `rename` — every level is permuted (row-touching ops) or cloned (pass-through ops).
- M45 Phase A `merge(other, on, how)`: `inner` / `left` take lhs's MultiIndex (every level permuted by emit's left rows); `right` takes rhs's MultiIndex.  `outer` with a MultiIndex on either side falls back to RangeIndex (still — the *index alignment* part of outer-merge can't trivially produce a MI when the carrying side's MI doesn't match the other side's index shape).
- M45 Phase B `melt(id_vars, value_vars)` — every level repeats `len(value_vars)` times.
- M45 Phase B `tabular.concat_rows(dfs)` — strict reconciliation: every frame must have a MultiIndex with the same `nlevels`, same per-level dtype, AND same per-level name; on success the output MultiIndex is the cell-wise concatenation per level.  Any mismatch falls back to M43's single-col concat reconciliation.
- M45 Phase B `tabular.concat_cols(dfs)` — lhs's MultiIndex wins.
- **M46 Phase A** `stack` — adds a new innermost level (column-name MI level); output `nlevels = input nlevels + 1`.
- **M46 Phase A** `unstack` — drops the innermost MI level (turns it into wide columns); output `nlevels = input nlevels - 1`.
- **M46 Phase D** `asof_merge(other, on_self, on_other)` — preserves the lhs's MultiIndex (left-join semantics; every output row corresponds to one lhs row in order, so the take vector is just `0..l_nrows`).

**Drops the MultiIndex back to RangeIndex (post-M46):**

- `pivot(index, columns, values)` / `pivot_table(index, columns, values, aggfunc)` / `pivot_table_aggfunc_list` / `pivot_table_margins` — all reshape the row dimension; the promoted `index_col` becomes the output's single-col index per M43.  No clean target for the input MultiIndex.
- Time-series resampling: `resample(time_col, rule, agg)`, `resample_index(rule, agg)` — both reshape the row dimension into time buckets.  Same shape as the pivot family.

**Outer-merge dtype-mismatch fallback (post-M46):** when both `lhs` and `rhs` have a **single-col index** of **different dtypes** AND the merge is `how="outer"`, the result now carries a **NaN-padded 2-level MultiIndex** (level 0 = lhs's index column with null for right-only rows; level 1 = rhs's index column with null for left-only rows).  Matches pandas.  Pre-M46 this case fell back to RangeIndex.

**Still RangeIndex (post-M46):** outer-merge with a MultiIndex on either side falls back to RangeIndex (interleaving a MI level-by-level across an outer join is more design than v1 needs — M47+ if anyone hits the case).

### 11.33 `tabular.stack` must-share-dtype constraint

`df.stack()` pivots every regular column into a new innermost MultiIndex level + a single `value` column.  **All regular columns must share a dtype** — otherwise the output `value` column's dtype is ambiguous.  Raises `ValueError` on mixed dtypes.  Same restriction as `melt(value_vars)`.  If you have a mix of dtypes and need to stack a subset, `select` the homogeneous subset first.

### 11.34 `tabular.unstack` must-have-MultiIndex constraint

`df.unstack()` is the inverse of `stack` — it takes the innermost MultiIndex level and turns it into wide columns.  **Input must have a MultiIndex** (raises `ValueError` on single-col index or RangeIndex inputs).  To unstack a single-col index (i.e. wide-form pivot on the index), use `pivot_table` instead.  Output `nlevels = input nlevels - 1`; if the result has 1 level it becomes a single-col index, if 0 a RangeIndex.  Missing `(row_key, col_key)` combinations become null cells.  v1 simplification: only the first regular column's values are distributed — multi-column unstack is M47+ territory.

### 11.35 `tabular.iloc` negative-index semantics (post-M47)

M40 shipped `df.iloc(start, stop)` with an explicit "negative indices raise `ValueError`" contract.  **M47 lifts that contract**: `iloc` now accepts Python-style negative indices on both bounds (`-1` = last row, `-N` = `nrows - N`).  Mixed positive + negative is fine (`iloc(-3, nrows)` = last 3 rows; `iloc(-5, -1)` = rows `[nrows-5, nrows-1)`).  Out-of-range positive bounds still clamp silently to `[0, nrows]` — only the explicit-rejection contract changed.  The same semantics apply to both bounds of `df.iloc_2d(row_start, row_stop, col_start, col_stop)` on both axes.

### 11.36 `tabular.ColumnCategorical` sort uses alphabetical string ordering (post-M49 nuance)

`ColumnCategorical` ships in M47 with a v1 limitation: any op that needs to compare two categorical cells (e.g. `sort_by` on a frame whose key column is categorical) compares **the materialized strings alphabetically**, NOT the `categories[]` declaration order.  This matches the v1 to_strings() coercion contract.  M49 ships codes-hash for `group_by` and `merge` (transparent, no API change — see §11.38), and adds `cc.is_ordered() -> bool` so callers can detect explicit-categories builds, but the sort itself still uses alphabetical string ordering.  Pandas's "ordered categorical" sort semantics (where the order of `categories[]` is meaningful for comparison) is **M51 work**.

**Workarounds for ordered sort in M49:**

1. Build a small `Dict[str, i64]` mapping each category to its desired ordinal and replace the categorical column with the i64 column for sorting.
2. Build the categorical via `tabular.col_categorical_ordered(values, categories)`, call `cc.codes()` to materialize the codes column, sort on that, and use the resulting permutation to reorder the rest of the frame.

`cc.is_ordered()` heuristic: returns true iff `categories[]` has unreferenced entries (the signature of an explicit-categories build via `col_categorical_ordered` / `col_categorical_from_codes`).  A value-rich ordered categorical where every category happens to be used will return false (looks identical to the M47 first-appearance build).  Use `cc.categories()` directly when you need to inspect the ordering.

### 11.37 `tabular.resample` calendar-arithmetic for `1M` / `1Y` (post-M49)

M40 shipped fixed-width resample rules: `Nm` (minutes), `Nh` (hours), `Nd` (days).  M49 adds:

- `Nw` (weeks) — fixed-width (`N × 7 × 86_400_000` ms).
- `NM` (months) — calendar arithmetic via Howard Hinnant's `days_from_civil` / `civil_from_days`.  A `1M` bucket starting at `t` ends just before the same calendar day of month in the following month, with **end-of-month clamping**: Jan 31 + 1M = Feb 29 (leap year) or Feb 28 (non-leap), then Mar 31, Apr 30, etc.  The bucket-start anchor is `t_min` (first non-null timestamp in the time column); subsequent bucket starts are computed by advancing the anchor's calendar year/month by `N` and clamping the day to the new month's length.
- `NY` (years) — same shape as month but stepping in calendar years.  Feb 29 + 1Y in a non-leap year clamps to Feb 28.

**Anchor policy**: the first bucket starts at the data's `t_min` (no floor to calendar boundary).  This matches the existing M40 behavior for fixed-width rules — the user's data range defines the buckets, not the wall clock.  Users who want calendar-floored buckets (e.g. "the first Monday on or before t_min") can do their own floor before calling resample.

**Months/years are NOT fixed-width**: each `1M` bucket may contain 28-31 days; each `1Y` bucket contains 365 or 366 days.  Aggregations (`sum`, `mean`, `min`, `max`, `count`) operate on the rows that fall in each bucket regardless of the bucket's day-count.

### 11.38 `tabular.merge` codes-hash requires bit-identical `categories[]` (post-M49)

When `df.merge(other, on, how)` is called and every `on` column on BOTH sides is a `ColumnCategorical`, M49 checks whether the two sides' `categories[]` are **bit-identical** (same length + same strings at the same indices).  If yes → codes-hash fastpath fires (hash on i64 codes vector).  If no (different orderings, or different category sets) → falls back to the string-hash path.

**Why bit-identical, not equal-as-set**: two ColumnCategorical with the same string values but different `categories[]` orderings produce different codes — e.g. `["a","b","c"]` vs `["c","b","a"]` map `a` to code 0 vs code 2.  Comparing codes from mismatched tables would silently produce wrong join results.  Pandas has the same constraint (it requires both sides to be the same `CategoricalDtype` for the fastpath).

**Workflow for fast merge**:

```python
shared_cats: List[str] = ["a", "b", "c"]
cc1 = tabular.col_categorical_ordered(values_a, shared_cats)
cc2 = tabular.col_categorical_ordered(values_b, shared_cats)
# Build df1, df2 with cc1, cc2 as the `on` column; df1.merge(df2, ["k"], "inner") takes the codes-hash fastpath.
```

The fallback (string-hash) is still correct — it just doesn't get the codes-hash speedup.  At low cardinality the difference is negligible; at high cardinality (~1000+ distinct values), the fastpath is ~10-100× faster.

### 11.39 `tabular.serve` deliberate scope-down (post-M50c)

`tabular.serve` (and `serve_with_timeout`) is the localhost desktop-UI for a DataFrame.  M50a shipped the HTTP transport + a minimal frontend; M50b polished the frontend and added missing endpoints; M50c added the interactive pivot panel, canvas-based chart rendering, sortable group-by, and surfaced index columns through the rows/schema endpoints.

**What landed in M50c (this milestone):**

- **`POST /api/pivot`** — interactive pivot UI backend.  Body `{"df":N, "index":"X", "columns":"Y", "values":"Z", "aggfunc":"sum|mean|min|max|count"}`.  Server-side wrapper around `df.pivot_table`.  The bundled frontend's Pivot panel surfaces this — pick index/columns/values columns + aggfunc, click "Pivot" — returns a derived df that becomes the active view.
- **`/api/groupby` now accepts `"sort":true`** — sorts the agg result by the group keys.  Single-key group_by → `sort_index`; multi-key group_by → `sort_index_multi` (M44 MultiIndex shape); fallback to `sort_by(by[0])` for the rare regular-column shape.  Failure of the chosen sort path falls back to the unsorted aggregate rather than 400'ing the request.  The frontend's group-by panel exposes a "sort by key" checkbox.
- **Canvas-based chart rendering** — `/` now ships a Chart panel with three chart types: `bar` (categorical X + numeric Y), `line` (numeric X + numeric Y; defaults X to row index if non-numeric), `histogram` (single numeric column, 20 bins).  Charts read from the currently-loaded rows in the DOM table (no extra API roundtrip); for million-row frames scroll-load first.  Pure-JS Canvas2D — no charting library bundled.
- **Index columns surfaced through `/api/rows` and `/api/schema`** — `/api/schema` now includes `index_names` + `index_dtypes` arrays (per-level for MultiIndex); `/api/rows` includes a parallel `index` array (one entry per row, each entry is a list of N level values).  The bundled frontend renders index columns as a distinct prefix (light-blue `td.idx` cells with a divider).  This was a blocker for the pivot/groupby experience — without surfacing the index, group-by results showed only the aggregated value and the keys were invisible.

**Earlier milestones (still relevant context):**

- **M50b**: `POST /api/sort` (sortable column headers, ▲/▼ click toggle), `POST /api/filter_multi` (composite AND/OR), `GET /api/csv` (CSV download), `POST /api/forget` (drop derived df from registry; refuses id=0), `ColumnCategorical` cells serialize as their resolved category strings, DOM-node recycling capped at 5000 rendered rows.

**Remaining deliberate v1 simplifications:**

- **No HTTPS** — localhost-only.  M28 P3b-B's `ssl` stdlib could plumb in rustls, but in-process bind on `127.0.0.1` is a single-machine boundary; encrypted localhost adds no security and a lot of cert-handling complexity.
- **No keep-alive** — each connection serves one request, then closes.  Low-frequency UI traffic doesn't notice; production HTTP traffic routes through M29's framework instead.
- **Chart rendering operates on visible rows only** — the chart panel reads cells out of the rendered DOM, so for a 1M-row frame the chart reflects whatever the user has scroll-loaded (capped at the M50b 5000-row DOM cap).  A future v0.5 could add a `/api/chart_data?col=X&kind=histogram` endpoint that computes server-side over the full frame.
- **No streaming pivot** — `/api/pivot` materializes the full pivoted DataFrame.  For very wide pivots (1000+ distinct columns_col values) this can blow up.
- **DOM-node recycling is not true virtual scroll** — the frontend evicts from the top instead of repositioning rows by absolute pixels.  For frames where the user wants to scroll back to row 0 after exploring row 800K, the head rows have to refetch.  Pragmatic tradeoff: avoids the fixed-row-height constraint and the rangemap bookkeeping a real virtual scroller needs.
- **DataFrame ID registry has no LRU eviction** — derived frames stick around until the server shuts down OR the user clicks "Reset to primary" (which calls `/api/forget`).  Long-running unattended sessions still grow; v0.5 could add a max-derived-count + LRU.
- **No request-rate limiting** — a malicious client could DoS the server by spamming /api/groupby.  localhost-only mitigates this.
- **Index columns are not click-sortable in v1** — only regular column headers wire to `/api/sort`.  Workaround: clear the current view via "Reset to primary" + re-group with `sort:true`.

The server is **single-threaded** — one connection at a time.  Two simultaneous browser tabs hitting it will see the second tab queue behind the first.  This is fine for interactive use and tests; production-grade concurrency is M29's framework.

### 11.40 `gfx` deliberate scope-down (post-M52)

- **Single window only:** The `gfx` module only supports one window at a time to simplify event loop routing.
- **Explicit window disposal required:** The garbage collector does not support finalization hooks. Thus, calling `gfx.close_window(win)` is required to avoid OS resource leaks.
- **No HiDPI / retina scaling:** HiDPI scaling is disabled by default to keep the drawing canvas logical pixels consistent on all systems.
- **No audio or fonts in M52-M53:** Text rendering and audio playback are deferred to later milestones (M54).
- **No gamepad/joystick support:** Controller inputs are not supported in M52.
- **Testing requires dummy drivers:** For headless verification (such as in CI), you must set `SDL_VIDEODRIVER=dummy` and `SDL_AUDIODRIVER=dummy` in the environment before invoking SDL2.

### 11.41 `gfx.Image` deliberate scope-down (post-M53)

- **Explicit image disposal required:** Like windows, GPU image textures must be explicitly freed via `gfx.free_image(img)` to avoid GPU memory leaks.
- **Texture bound to creation window:** Image textures are created under a specific window context. If the parent window is closed, any subsequent operations using the loaded images will raise a `ValueError`.
- **No runtime format conversion:** The library relies on SDL2_image to decode files, returning the native format without color conversion or runtime format manipulation.

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
