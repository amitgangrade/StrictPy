# M11-C6 — Toy Lisp interpreter

**Brief**: write a Lisp interpreter with sealed AST, recursive eval,
closures, environment chains. Designed as the most ambitious stress test
of the class system.

**Wall-clock**: ~35 minutes
**Files added**: `examples/lisp.spy` (647 lines, ~70% docstring/design
notes documenting the bug hunt; ~190 lines of pure interpreter code),
`compiler/tests/lisp_runs.rs` (75).

## Result

End-to-end. Handles `define`, `if`, `lambda`, `quote`, `begin`, arithmetic
and comparison builtins, and lexically-scoped closures.

Test program:
```
(define x 10)
(define y 20)
(print (+ x y))
(define square (lambda (n) (* n n)))
(print (square 7))
```

stdout: `30\n49\n`

## TWO NEW critical bugs discovered

### N1: Base class with >4 virtual methods — slots ≥4 are unreachable

**Repro** (minimal):
```python
open class Value:
    open fn m0(self) -> i32: return 0
    open fn m1(self) -> i32: return 1
    open fn m2(self) -> i32: return 2
    open fn m3(self) -> i32: return 3
    open fn m4(self) -> i32: return 4    # UNREACHABLE
    open fn m5(self) -> i32: return 5    # UNREACHABLE

final class A(Value):
    fn m4(self) -> i32: return 104

fn main() -> i32:
    v: Value = A()
    v.m4()    # traps: "vtable slot 4 out of range"
```

**Sharpens BUG-017** — the cap is not just "≤3 sibling overrides per
slot" but also "≤4 total virtual slots allocated on the base class."

### N2: Heap corruption on subclass-with-class-ref-fields + base virtual call

**Repro** (deterministic):
```python
open class Value:
    open fn tag(self) -> i32: return 0

final class Number(Value):
    n: i64
    fn __init__(self, n: i64) -> None: self.n = n
    fn tag(self) -> i32: return 1

final class Pair(Value):
    car: Value          # field is itself a Value reference
    cdr: Value?
    fn __init__(self, c: Value, cd: Value?) -> None:
        self.car = c; self.cdr = cd
    fn tag(self) -> i32: return 3

fn main() -> i32:
    n: Value = Number(10)
    p: Value = Pair(n, none)
    println(str(p.tag()))   # ACCESS VIOLATION
    return 0
```

**Severity**: high. Number-typed values work fine; ONLY Pair (with class-ref
fields) crashes. Independent of and earlier than BUG-026 (teardown crash).

This is the **deterministic sibling** of the previously-non-deterministic
BUG-026. In M11 fix pass it was confirmed to be **BUG-016 in disguise**:
subclass field offsets aliased the vtable pointer at offset 0; the first
`p.tag()` call loaded `Pair.car` (a heap pointer) as if it were a
RuntimeType pointer and dereferenced random memory.

## Confirmed BUGS_KNOWN entries

All six confirmed and worked around:
- §1 sealed dispatch — used `open class Value`
- §2 subclass field aliasing — hit it via N2 above
- §3 vtable mod-4 — sharpened to N1 above
- §4 non-deterministic heap corruption — hit repeatedly
- §5 position-sensitive crash — confirmed (reordering Number/Symbol/Pair changed crash site)
- §6 no line continuation across `+` — dodged with accumulators

## Workarounds used (substantial)

- **Tagged-union over inheritance**: single `final class Value { kind: i32, n: i64, name: str, car: Value?, cdr: Value? }`. No virtual dispatch, no vtable slots. Switch on `v.kind == 1/2/3` everywhere. Matches the JsonAtom pattern from json_parse.
- **Closures encoded as Pair chains**: `Pair(Symbol("<closure>"), Pair(Number(id), none))`. Real Lambda lives in a side `List[Lambda]`.
- **Built-ins as marker symbols**: `Symbol("<builtin:+>")` etc.
- **Env = parallel `List[str]` + `List[Value]`** (avoiding `Dict[str, Value]` per C2's note)
- **`unwrap_value(v: Value?) -> Value`** helper because nullable narrowing doesn't survive past the guard
- **Mutable `List[i64]` of length 1 for parser cursor** (no tuples / out-params)
- **String accumulators** for multi-line `+`
- **Subprocess test harness** to ride out the teardown HEAP_CORRUPTION

## Language-surface awkwardness

- **No `isinstance`, no Value→subclass cast, no working `match`**. Can't go from `Value` to `Pair` to read `car`. Only escape: virtual methods, but BUG-017/N1 caps that at 4. With nullable narrowing this becomes "every accessor must be virtual or be a free function returning T?".
- **Nullable narrowing per-expression not per-binding** — forces unwrap helpers everywhere.
- **No `List[Value]` in Dict values** when Value is a sealed hierarchy — closes the door on `Dict[str, Value]`.
- **No tuples / multiple returns / out-params** — positional state threaded via 1-element mutable lists.

## Final test totals

`cargo test --release`: **178 passed, 0 failed, 1 ignored**. 2 new tests
green.

## Why this report matters

This single agent task:
- Discovered the TWO most consequential bugs of M11 (N1, N2)
- N2 turned out to be the **deterministic repro** for the previously-flaky BUG-026, which then enabled the M11 root-cause analysis
- Demonstrated that the language can encode a working Lisp interpreter, but only via heavy workarounds around the class system
- Documented the "tagged-union over inheritance" pattern that every subsequent class-heavy program will reach for until the class system is improved
