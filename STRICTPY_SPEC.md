# StrictPy Language & Virtual Machine Specification

**Version:** 0.2 (M0–M30 implementation reference)
**Status:** Frozen at v0.2 release (2026-05-21). Spec was originally
frozen at v0.1 on day one (M0); subsequent milestones extended it in
place (M16 match patterns, M19 imports, M25 CLI, M27–M29 stdlib
sections, M30 lexer line-continuation rule). v0.3 work begins at M31.
**Audience:** Compiler & VM implementers

---

## Table of Contents

1. [Introduction](#1-introduction)
2. [Design Principles](#2-design-principles)
3. [Lexical Structure](#3-lexical-structure)
4. [Grammar (EBNF)](#4-grammar-ebnf)
5. [Type System](#5-type-system)
6. [Static Semantics](#6-static-semantics)
7. [Dynamic Semantics](#7-dynamic-semantics)
8. [Memory & Object Model](#8-memory--object-model)
9. [Standard Library (Core)](#9-standard-library-core)
10. [Compiler Architecture](#10-compiler-architecture)
11. [Intermediate Representation](#11-intermediate-representation)
12. [Bytecode File Format](#12-bytecode-file-format)
13. [Opcode Reference](#13-opcode-reference)
14. [Virtual Machine](#14-virtual-machine)
15. [Garbage Collector](#15-garbage-collector)
16. [Concurrency Model](#16-concurrency-model)
17. [Foreign Function Interface](#17-foreign-function-interface)
18. [Error Model & Diagnostics](#18-error-model--diagnostics)
19. [Implementation Roadmap](#19-implementation-roadmap)
20. [Conformance Tests](#20-conformance-tests)
21. [Examples](#21-examples)
22. [Appendix A: Reserved for Future Use](#appendix-a-reserved-for-future-use)
23. [Appendix B: Opcode Quick Reference Table](#appendix-b-opcode-quick-reference-table)

---

## 1. Introduction

**StrictPy** is a statically typed dialect of Python designed to be compiled to a type-specialized bytecode and executed on a dedicated virtual machine. Its goals are:

- Familiar Python-like syntax with mandatory, concrete type annotations.
- Bytecode in which every operation knows its operand types — no runtime type dispatch on the hot path.
- A VM that executes typed bytecode without boxing primitives, without a GIL, and with cache-friendly object layouts.
- Predictable performance: a well-typed program should compile to code roughly competitive with hand-written C for numeric and OO workloads.

### 1.1 Goals

- **Static, mandatory typing.** Every binding has a concrete, statically known type.
- **Closed-world compilation.** Whole-program visibility enables monomorphization, devirtualization, and inlining.
- **Specialized bytecode.** Each opcode encodes the type it operates on.
- **No GIL.** True multi-threading.
- **Zero-cost FFI** to C.
- **Bootstrap-friendly.** The compiler and VM can each be implemented in ~10 KLOC of host code.

### 1.2 Non-goals

- 100% Python compatibility. StrictPy is a *dialect*, not a superset.
- Dynamic features: monkeypatching, `eval`/`exec`, runtime class mutation, metaclass programming.
- Arbitrary-precision integers by default (available as `BigInt`).
- A REPL with live class redefinition.

### 1.3 Differences from CPython at a glance

| Feature                        | CPython                  | StrictPy                                  |
|--------------------------------|--------------------------|-------------------------------------------|
| Type annotations               | Optional, unenforced     | Mandatory, enforced at compile time       |
| `int`                          | Arbitrary precision      | `i32`/`i64` machine ints; `BigInt` opt-in |
| Class layout                   | `__dict__` per instance  | Fixed struct with vtable                  |
| Method dispatch                | Dict lookup              | Direct or vtable slot                     |
| GIL                            | Yes                      | No                                        |
| Compilation                    | AST → bytecode           | AST → typed IR → typed bytecode → JIT    |
| `eval`/`exec`/monkeypatch      | Yes                      | Forbidden                                 |
| Subclassing                    | Open                     | `final` by default; `open` to allow       |

---

## 2. Design Principles

1. **Static knowledge enables performance.** Anything the compiler cannot prove statically becomes an error, not a runtime fallback.
2. **One way to do it.** Where Python offers multiple equivalent forms, StrictPy picks one.
3. **No hidden allocations.** Boxing, conversions, and heap allocations are explicit or obvious from types.
4. **Errors are values when possible, exceptions when not.** Recoverable failure uses `Result[T, E]`; truly exceptional conditions use exceptions.
5. **Explicit is better than implicit** — applied seriously, not as a slogan.

---

## 3. Lexical Structure

### 3.1 Source encoding

Source files are **UTF-8**. The byte order mark (BOM) is permitted at file start and ignored. File extension: `.spy`.

### 3.2 Line structure

- Logical lines are terminated by `\n` (LF) or `\r\n` (CRLF, normalized to LF).
- Physical line continuation: backslash before newline (`\\\n`).
- Implicit continuation inside `()`, `[]`, `{}`.
- Implicit continuation after a trailing binary operator: a physical
  newline is also suppressed when the last significant token on the line
  is a binary operator/keyword requiring a right-hand operand —
  arithmetic (`+ - * / // % **`), assignment (`= += -= *= /= //= %= **= &= |= ^= <<= >>=`),
  comparison (`== != < > <= >=`), boolean (`and`, `or`), bitwise
  (`& | ^ << >>`), membership/cast keywords (`in`, `is`, `as`), or
  null-coalesce (`??`). The continuation line's leading indentation is
  ignored (no `INDENT`/`DEDENT` emitted). `:`, `.`, `,`, `->`, `@`,
  unary `not`/`~` do NOT trigger this rule.

### 3.3 Indentation

Indentation is significant. Rules:

- Indentation is **spaces only**. Tabs are a lexer error.
- Indent unit is exactly **4 spaces**. (Stricter than PEP 8; eliminates ambiguity.)
- The lexer emits `INDENT` and `DEDENT` tokens, identical in role to CPython's.

### 3.4 Comments

Single-line: `# ...` to end of line. No multi-line comment syntax (use triple-quoted strings as docstrings).

### 3.5 Keywords

Reserved and may not be used as identifiers:

```
and        as         assert     async      await      break
case       class      continue   def        del        elif
else       except     final      finally    fn         for
from       global     if         import     in         is
lambda     match      not        open       or         pass
protocol   raise      return     sealed     try        while
with       yield      true       false      none
```

Reserved but unused (forward compatibility): `effect`, `region`, `unsafe`.

### 3.6 Identifiers

```
identifier ::= id_start id_continue*
id_start   ::= '_' | letter
id_continue ::= id_start | digit
```

ASCII only in v0.1. Identifiers starting with `__` are reserved for the compiler/runtime.

### 3.7 Literals

#### Integer literals
```
0           // i64 by default
42          // i64
0i64        // i64
0i32        // i32 (explicit suffix)
0u32        // u32
0xff        // i64 hex
0b1010      // i64 binary
0o755       // i64 octal
1_000_000   // underscores allowed
```

A bare (unsuffixed) integer literal defaults to **`i64`**. In a typed
context the literal adopts the annotation when it fits (`x: i32 = 0`
makes `0` an `i32`, and `t: Tuple[i32, i32] = (1, 2)` makes both `1` and
`2` `i32`); otherwise use an explicit suffix (`0i32`). Generic call sites
infer the type parameter from the *synthesised* argument type, so a bare
literal there infers `i64` — annotate the literal (`id(0i32)`) when a
narrower instantiation is wanted.

If the literal value exceeds the chosen type's range, it is a compile error.

#### Float literals
```
3.14        // f64 by default
2.0f32      // f32
1e10        // f64
.5          // f64
```

#### Boolean literals
```
true
false
```

(Note: lowercase. `True`/`False` are not keywords.)

#### None literal
```
none
```

`none` has type `Null`, which is a subtype of every `T?`.

#### String literals
```
"hello"             // str (UTF-8 backed, immutable)
"line\nbreak"       // escape sequences
"""triple"""        // multi-line
r"raw\nstring"      // raw, no escapes
b"bytes"            // bytes literal
f"value: {x}"       // f-string — basic interpolation only (no format specs)
```

Escape sequences: `\n \r \t \\ \' \" \0 \xHH \uHHHH \U{HEX}`.

An f-string desugars to string concatenation: each `{expr}` interpolation
becomes `str(expr)` and the literal chunks are joined with `+`. Format
specifiers (`f"{x:.2f}"`) and nested f-strings are NOT supported in v0.3 —
a `:` inside an interpolation is a compile error.

#### Char literal
```
'a'         // char (Unicode scalar, 4 bytes)
'\n'        // char
```

Single-quoted strings are **char**, not `str`. Empty char `''` is illegal.

### 3.8 Operators and punctuation

```
+   -   *   /   //  %   **
=   +=  -=  *=  /=  //= %=  **=
==  !=  <   >   <=  >=
and or  not in  is
&   |   ^   ~   <<  >>
&=  |=  ^=  <<= >>=
(   )   [   ]   {   }
,   :   ;   .   ->  @   ?   ??
```

`?` after a type marks nullability. `??` is the null-coalescing operator.

`->` introduces return types.

---

## 4. Grammar (EBNF)

The grammar is LALR(1)-friendly. EBNF, with `{X}` meaning zero-or-more and `[X]` meaning optional.

```ebnf
module          ::= { import_stmt } { top_decl } EOF

import_stmt     ::= "from" dotted_name "import" import_list NEWLINE
                  | "import" dotted_name [ "as" identifier ] NEWLINE
import_list     ::= identifier [ "as" identifier ] { "," identifier [ "as" identifier ] }
dotted_name     ::= identifier { "." identifier }

top_decl        ::= func_decl | class_decl | protocol_decl | const_decl | type_alias

const_decl      ::= "final" identifier ":" type "=" expr NEWLINE
type_alias      ::= "type" identifier [ generic_params ] "=" type NEWLINE

func_decl       ::= [ decorator ] "fn" identifier [ generic_params ] "(" [ params ] ")" "->" type ":" block
params          ::= param { "," param }
param           ::= identifier ":" type [ "=" expr ]
generic_params  ::= "[" type_param { "," type_param } "]"
type_param      ::= identifier [ ":" type_bound ]
type_bound      ::= type { "+" type }
decorator       ::= "@" dotted_name [ "(" [ arg_list ] ")" ] NEWLINE
                    # NOTE: decorators parse but have NO semantics in v1. Any
                    # decorator is a hard compile error (E2071) rather than a
                    # silent no-op — `@lru_cache`/`@retry` etc. must not appear
                    # to "work" while doing nothing. The allow-list is empty
                    # until a real decorator is implemented and lowered.

class_decl      ::= [ class_modifier ] "class" identifier [ generic_params ]
                    [ "(" type_list ")" ] ":" class_body
class_modifier  ::= "final" | "open" | "sealed"
class_body      ::= INDENT { class_member } DEDENT
class_member    ::= field_decl | method_decl | init_decl
field_decl      ::= identifier ":" type [ "=" expr ] NEWLINE
method_decl     ::= [ decorator ] [ "open" ] "fn" identifier "(" "self" [ "," params ] ")" "->" type ":" block
init_decl       ::= "fn" "__init__" "(" "self" [ "," params ] ")" "->" "None" ":" block

protocol_decl   ::= "protocol" identifier [ generic_params ] ":" INDENT { proto_member } DEDENT
proto_member    ::= "fn" identifier "(" "self" [ "," params ] ")" "->" type NEWLINE

block           ::= NEWLINE INDENT { stmt } DEDENT

stmt            ::= simple_stmt | compound_stmt
simple_stmt     ::= ( let_stmt | destructure_stmt | assign_stmt | return_stmt
                    | expr_stmt | break_stmt | continue_stmt | pass_stmt
                    | raise_stmt | assert_stmt | del_stmt ) NEWLINE

let_stmt        ::= identifier ":" type "=" expr
// Tuple destructure binds names to a tuple RHS; star-unpack binds names to a
// List[T] RHS, with the starred name capturing a fresh List[T] of the middle
// elements. At most one "*" target is allowed.
destructure_stmt ::= target { "," target } "=" expr           // ≥2 targets, or
                   | { target "," } "*" identifier { "," target } "=" expr
target          ::= identifier [ ":" type ]
assign_stmt     ::= lhs aug_op expr
                  | lhs "=" expr
lhs             ::= identifier | attr_ref | subscript
aug_op          ::= "+=" | "-=" | "*=" | "/=" | "//=" | "%=" | "**="
                  | "<<=" | ">>=" | "&=" | "|=" | "^="

return_stmt     ::= "return" [ expr ]
break_stmt      ::= "break"
continue_stmt   ::= "continue"
pass_stmt       ::= "pass"
raise_stmt      ::= "raise" expr [ "from" expr ]
assert_stmt     ::= "assert" expr [ "," expr ]
del_stmt        ::= "del" lhs

compound_stmt   ::= if_stmt | while_stmt | for_stmt | match_stmt
                  | try_stmt | with_stmt

if_stmt         ::= "if" expr ":" block { "elif" expr ":" block } [ "else" ":" block ]
while_stmt      ::= "while" expr ":" block [ "else" ":" block ]
for_stmt        ::= "for" identifier ":" type "in" expr ":" block [ "else" ":" block ]
match_stmt      ::= "match" expr ":" INDENT { case_clause } DEDENT
case_clause     ::= "case" pattern [ "if" expr ] ":" block
try_stmt        ::= "try" ":" block { except_clause } [ "else" ":" block ] [ "finally" ":" block ]
except_clause   ::= "except" type [ "as" identifier ] ":" block
with_stmt       ::= "with" expr [ "as" identifier ":" type ] ":" block

pattern         ::= literal_pattern | identifier_pattern | wildcard_pattern
                  | constructor_pattern | tuple_pattern
literal_pattern ::= literal
identifier_pattern ::= identifier
wildcard_pattern   ::= "_"
constructor_pattern ::= type "(" [ pattern { "," pattern } ] ")"
tuple_pattern   ::= "(" pattern "," pattern { "," pattern } ")"

expr            ::= ternary
ternary         ::= or_expr [ "if" or_expr "else" ternary ]
or_expr         ::= and_expr { "or" and_expr }
and_expr        ::= not_expr { "and" not_expr }
not_expr        ::= "not" not_expr | comparison
comparison      ::= bit_or { comp_op bit_or }
comp_op         ::= "==" | "!=" | "<" | ">" | "<=" | ">=" | "is" | "is" "not" | "in" | "not" "in"
bit_or          ::= bit_xor { "|" bit_xor }
bit_xor         ::= bit_and { "^" bit_and }
bit_and         ::= shift { "&" shift }
shift           ::= addition { ("<<" | ">>") addition }
addition        ::= multiplication { ("+" | "-") multiplication }
multiplication  ::= unary { ("*" | "/" | "//" | "%") unary }
unary           ::= ("+" | "-" | "~") unary | power
power           ::= postfix [ "**" unary ]
postfix         ::= primary { call | attr_ref | subscript | slice | null_coalesce }
call            ::= "(" [ arg_list ] ")"
attr_ref        ::= "." identifier
subscript       ::= "[" expr { "," expr } "]"
// Slice (str and List[T]): each bound optional; negative bounds count from
// the end; negative step reverses. `seq[a:b:c]`, `seq[::-1]`, `seq[:n]`, ...
slice           ::= "[" [ expr ] ":" [ expr ] [ ":" [ expr ] ] "]"
null_coalesce   ::= "??" unary

primary         ::= literal | identifier | "(" expr ")" | tuple_literal
                  | list_literal | dict_literal | set_literal | lambda_expr
                  | f_string
literal         ::= INT | FLOAT | STRING | CHAR | "true" | "false" | "none"
tuple_literal   ::= "(" [ expr { "," expr } [","] ] ")"     // singleton: (x,)
list_literal    ::= "[" [ expr { "," expr } ] "]"
dict_literal    ::= "{" [ expr ":" expr { "," expr ":" expr } ] "}"
set_literal     ::= "{" expr { "," expr } "}"
lambda_expr     ::= "fn" "(" [ params ] ")" "->" type ":" expr

arg_list        ::= arg { "," arg }
arg             ::= [ identifier "=" ] expr

type            ::= type_atom [ "?" ]
type_atom       ::= dotted_name [ "[" type { "," type } "]" ]
                  | fn_type
                  | tuple_type
fn_type         ::= "fn" "(" [ type { "," type } ] ")" "->" type
tuple_type      ::= "(" type "," type { "," type } ")"
type_list       ::= type { "," type }
```

---

## 5. Type System

### 5.1 Type universe

```
Type ::= Primitive
       | Reference
       | Function
       | Tuple
       | Generic
       | Nullable
       | Never
```

#### 5.1.1 Primitive types

| Type     | Size      | Range                                  |
|----------|-----------|----------------------------------------|
| `bool`   | 1 byte    | `true`, `false`                        |
| `i8`     | 1 byte    | -128 … 127                             |
| `i16`    | 2 bytes   | -32768 … 32767                         |
| `i32`    | 4 bytes   | -2³¹ … 2³¹-1                           |
| `i64`    | 8 bytes   | -2⁶³ … 2⁶³-1                           |
| `u8`     | 1 byte    | 0 … 255                                |
| `u16`    | 2 bytes   | 0 … 65535                              |
| `u32`    | 4 bytes   | 0 … 2³²-1                              |
| `u64`    | 8 bytes   | 0 … 2⁶⁴-1                              |
| `f32`    | 4 bytes   | IEEE 754 binary32                      |
| `f64`    | 8 bytes   | IEEE 754 binary64                      |
| `char`   | 4 bytes   | Unicode scalar value                   |
| `Never`  | 0 bytes   | Empty type; expressions that diverge   |

Primitives are *value types* — they are stored inline, never heap-allocated, never have identity beyond their value.

`int` is **not** a type. Use `i32`, `i64`, or `BigInt`.

#### 5.1.2 Reference types

| Type            | Description                            |
|-----------------|----------------------------------------|
| `str`           | Immutable UTF-8 string                 |
| `bytes`         | Immutable byte sequence                |
| `List[T]`       | Mutable contiguous array               |
| `Dict[K, V]`    | Hash table; **`K` must be `str` in v1** (see note) |
| `Set[T]`        | Hash set                               |
| `Tuple[...]`    | Heterogeneous fixed-size product       |
| `BigInt`        | Arbitrary-precision integer            |
| User classes    | Class instances                        |

Reference types are heap-allocated and accessed via pointers. Two reference values are `is`-equal iff they point to the same object.

> **`Dict` key restriction (v1).** The runtime dict is implemented as a
> string-keyed hash table (`vm/src/strdict.rs`). The type checker therefore
> rejects any `Dict[K, V]` whose key type `K` is not `str` with `E2072`.
> Previously a non-`str` key (e.g. `Dict[i64, V]`, `Dict[Tuple[...], V]`)
> compiled and then SEGFAULTed at subscript; the guard closes that crash.
> Full non-`str`-key support is deferred to a later milestone.

#### 5.1.3 Function types

`fn(T1, T2, ...) -> R` is the type of a function value. Function values include free functions, lambdas, and bound methods.

#### 5.1.4 Nullable types

`T?` is the union `T | Null`. There is no `Optional[T]` — use `T?`.

The only operations valid on `T?` without narrowing are:
- `==`, `!=` against `none`
- `??` (null-coalesce)
- Pattern matching
- Passing to a function expecting `T?`

To use a `T?` as `T`, narrow it:

```python
x: i32? = maybe()
if x is not none:
    y: i32 = x        // OK: x narrowed to i32
```

#### 5.1.5 Generic types

Declared with `[T]` brackets. All type parameters are **erased at the source level but monomorphized at compile time** — each distinct instantiation produces a separately compiled copy.

```python
fn identity[T](x: T) -> T:
    return x

class Box[T]:
    value: T
    fn get(self) -> T:
        return self.value
```

Bounds (declarative — v0.2):
```python
fn sum_all[T: Numeric](items: List[T]) -> T:
    ...
```

##### v0.1 (M17) generic free functions

Implemented:

1. **Declaration.** A free function may carry one or more type parameters
   between square brackets after the name. Type parameters are visible
   inside parameter annotations, the return type, and local-binding
   annotations.

   ```python
   fn id[T](x: T) -> T:
       return x

   fn first[K, V](p: Tuple[K, V]) -> K:
       return p.0

   fn first_of[T](xs: List[T]) -> T:
       return xs[0]
   ```

2. **Call-site inference.** Type arguments are *not* written at the call;
   the type checker unifies each parameter annotation with the
   corresponding argument's static type. The substitution must collapse
   to a unique solution per type parameter; otherwise the call site
   reports `E2001` pointing at the offending argument.

   ```python
   id(7)            # T := i32
   id("hi")         # T := str
   first((1, "x"))  # K := i32, V := str
   id(true)         # T := bool
   ```

   Inside a generic-fn body, calls to *other* generic functions also
   participate in inference: the substitution active for the enclosing
   body resolves any `Ty::Var` in the argument types before unification,
   so a call to a generic helper threads the outer `T` through to the
   inner instantiation.

3. **Monomorphisation.** The IR lowerer emits exactly one bytecode
   function per distinct `(fn_sym, type_args)` pair discovered at
   call sites. The original generic template is **not** emitted;
   every call site dispatches to a mangled per-instantiation `FuncId`.
   The lowerer drives a worklist — instantiations discovered transitively
   inside a generic body are queued and lowered in turn — so every
   reachable specialisation is emitted exactly once.

4. **Mangling scheme.** A mangled name has the form
   `<src_name>__<arg1_mangled>_<arg2_mangled>...`. Each element follows:

   | Type             | Mangle |
   |------------------|--------|
   | `i32`/`i64`/etc. | `i32`/`i64`/... (lowercase primitive name)        |
   | `bool`, `char`, `str` | `bool`, `char`, `str`                             |
   | `class C` (id=N) | `class<N>`                                        |
   | `Tuple[A, B]`    | `tuple_<A>_<B>`                                   |
   | `List[T]`        | `list_<T>` (constructor name lowercased)          |
   | `Dict[K, V]`     | `dict_<K>_<V>`                                    |
   | `T?`             | `opt_<T>`                                         |

   Example: `quicksort__list_i64_i64_i64` is the i64 instantiation of
   `fn quicksort[T](a: List[T], lo: i64, hi: i64)`.

5. **Per-instantiation operator binding.** Comparison and arithmetic
   operators inside a generic body see `Ty::Var(T)` operands at type-
   check time; the type-checker accepts them deferred. At IR lowering
   the active substitution applies, so `xs[j] < pivot` lowers to `ILt`
   in `quicksort__list_i64_i64_i64` and to `FLt` in
   `quicksort__list_f64_i64_i64` from the *same* source body.

6. **Recursion.** A generic function calling itself with the same
   substitution dispatches to its own mangled `FuncId` — i.e. recursion
   inside `quicksort[T]` lowers to a direct call to the matching
   per-instantiation function, not back to the template.

##### v0.3 (M31) generic classes

Implemented:

1. **Declaration.** A class may carry one or more type parameters
   between square brackets after the name. Type parameters are visible
   inside field annotations, method-parameter / return annotations, and
   inside method bodies (so a method can declare locals of type `T`,
   construct `List[T]`, etc.).

   ```python
   final class Box[T]:
       value: T

       fn __init__(self, value: T) -> None:
           self.value = value

       fn unwrap(self) -> T:
           return self.value

   final class Pair[K, V]:
       first: K
       second: V

       fn __init__(self, first: K, second: V) -> None:
           self.first = first
           self.second = second
   ```

2. **Constructor-site inference.** As with generic free functions, type
   arguments are not written at the call; the type checker unifies the
   class's `__init__` parameter types (which carry `Ty::Var(...)` for
   each declared type parameter) against the constructor arguments. The
   substitution must collapse to a unique solution; otherwise the call
   site reports `E2001`.

   ```python
   bi: Box[i64] = Box(big)          # T := i64 (from argument type)
   bs: Box[str] = Box("hi")         # T := str
   p: Pair[str, i32] = Pair(s, n)   # K := str, V := i32
   ```

   A generic class with no `__init__` and a non-empty type-parameter
   list cannot be constructed by argument inference alone — every type
   variable would remain unbound. (Explicit type-argument syntax such
   as `Box[i64]()` is deferred to v0.4.)

3. **Field layout invariance.** Every field declared with an abstract
   type parameter occupies one 8-byte slot in the heap payload,
   regardless of the concrete substituted type. Concrete primitives
   that would naturally fit in a smaller slot (`i32`, `bool`, ...) are
   still allocated a full 8 bytes when stored at a generic-class field
   site. This keeps field *offsets* identical across instantiations —
   essential for the IR, which emits a single `Load`/`Store` operand
   per source-level field access and that offset must work for every
   monomorphisation.

4. **Monomorphisation.** Each distinct `(class_id, type_args)` pair
   produces:

   - one `TypeTableEntry` with substituted field types (and therefore
     substituted per-field type ids) and its own vtable;
   - one mangled `__init__` IRFunction (if the source declares one);
   - one mangled IRFunction per declared method.

   Mangled names follow `<class_name>__<arg1>_<arg2>...` (e.g.
   `Box__i64`, `Pair__str_i32`, `Stack__class3`). Method IRFunctions
   are named `<mangled_class>.<method>` (e.g. `Box__i64.unwrap`).
   Different instantiations have distinct runtime `type_id`s, so
   `isinstance(x, Box)` semantics for one instantiation do not
   accidentally match another — though the explicit `isinstance` form
   itself does not yet support parameterised target types (v0.4).

5. **Method dispatch on parameterised receivers.** A `MethodCall` whose
   receiver is statically typed `Ty::Generic { base: TypeCtor::Class(c),
   args: [T1, ...] }` dispatches to the per-instantiation method
   `FuncId` via a `DirectCall` — never through the abstract template's
   vtable slot. The receiver's substituted type args are mangled to the
   same key the type-table emission used, so dispatch is O(1) and the
   VM's vtable infrastructure (`VirtualCall`) is not needed.

6. **Field reads / writes on parameterised receivers.** Both
   `field_offset` (the IR helper) and the typechecker's `attr_type`
   substitute the receiver's type args through the field's declared
   type before returning. So `Box[i64].value` types as `i64`, not as
   `Ty::Var(0)`; assignments require the concrete substituted type.

##### v0.3 limits (deferred to v0.4)

- **No bounded class generics.** `class Box[T: Comparable]:` parses
  but the bound is ignored (same status as free-function bounds —
  see §5.1.5 v0.2 limits below).
- **No variance.** All generic class parameters are invariant. There
  is no syntax for marking a parameter covariant / contravariant.
- **No higher-kinded type parameters.** `class Container[F[_]]:` is
  rejected at parse time.
- **No explicit type-argument syntax at constructor sites.**
  `Box[i64]()` is parsed as an indexing expression on `Box`, not as
  an explicit type-application. Every type variable must be pinned by
  a constructor argument's static type (or, for `Stack[T]` where the
  field is `List[T]`, by a sentinel argument whose type drives `T`).
- **Subclassing a parameterised class** is not supported. Generic
  classes participate in the inheritance hierarchy only as leaves;
  their `base_type` is always `NO_BASE_TYPE`.
- **Transitive construction in a generic body** is best-effort. The
  typechecker records every concrete `(class, type_args)` pair it
  observes at user-visible call sites, and the IR pre-registers each.
  A `class Outer[T]:` whose method body constructs `Box[T]` will work
  when the outer is instantiated from a non-generic context (the
  inner instantiation gets discovered at the outer's typecheck);
  fully-internal cycles where a generic body manufactures a class
  instantiation that the typechecker never sees are documented to
  fall back to a `u32::MAX` placeholder tid (the VM traps cleanly).

##### v0.2 free-function generic limits (deferred to v0.4)

- **No bounds.** `T: Comparable` parses but the checker ignores the
  bound. A body that uses `<` on `T` typechecks, and instantiations
  where `<` is unsupported (e.g. user-defined class without comparison)
  trap at runtime rather than reject at compile time. (v0.4 will add
  protocol-typed bounds and per-instantiation re-typecheck.)
- **No auto-inference from return-type context.** `let x: i64 = id(0)`
  does *not* propagate the i64 expectation into the call; the
  unsuffixed `0` synths as i32 and inference picks `T := i32`. Users
  pin the type via a typed local (`big: i64 = 0; id(big)`), a literal
  suffix (`0i64`), or an explicit cast (`i64(0)`).
- **No higher-rank generics.** `fn apply(f: fn[T](T) -> T)` is rejected.

#### 5.1.6 Protocols

Structural interfaces.

```python
protocol Hashable:
    fn hash(self) -> i64

protocol Comparable:
    fn compare(self, other: Self) -> i32
```

A type satisfies a protocol if it provides all the required methods with matching signatures. No explicit declaration is required. Calls through a protocol-typed value use **itable** dispatch (see §14.4).

### 5.2 Subtyping

Subtyping is **structural for protocols, nominal for classes**.

- `T <: T` (reflexive)
- `T <: U` and `U <: V` implies `T <: V` (transitive)
- `Never <: T` for all `T`
- For class `C` with base `B`: `C <: B` iff `B` is `open` or `sealed`
- For protocols: `C <: P` iff `C` provides all methods of `P` with compatible signatures
- `T <: T?` for all non-nullable `T`
- Function types are **contravariant in arguments, covariant in result**
- `List[T]` is **invariant** in `T` (no `List[Child] <: List[Parent]`)
- `Tuple[T1, ..., Tn] <: Tuple[U1, ..., Un]` iff `Ti <: Ui` pointwise (covariant)

### 5.3 Numeric coercion

A binary operation on two numeric operands of **different** types applies
**lossless implicit widening** to a common type, then operates there. The
narrower operand is widened with a lossless cast inserted at IR lowering:

| operands                              | common type |
|---------------------------------------|-------------|
| `i8`/`i16`/`i32` with `i64`           | `i64`       |
| `i8`/`i16` with `i32`                 | `i32`       |
| any integer with `f64`                | `f64`       |
| `f32` with any integer                | `f64`       |
| `f32` with `f32`                      | `f32`       |
| `f64` with anything numeric           | `f64`       |

```python
x: i32 = 1
y: i64 = 2
z: i64 = x + y         // OK — x widens to i64
w: f64 = y + 0.5       // OK — y widens to f64
```

Widening is **conservative**: only the conversions above (each backed by a
lossless cast) are implicit. Mixed signedness (`u32 + i32`), `u64`, and
disjoint small widths still require an **exact match** or an explicit
conversion — there is no lossless cast for them. Narrowing is never
implicit: the *result* of a widened op cannot be silently stored into a
narrower target (`a: i32 = i32_var + i64_var` is an error because the
result is `i64`).

Allowed lossless explicit conversions never fail. Lossy conversions (e.g., `i64` → `i32`) trap on overflow in debug builds, wrap silently in release builds (configurable per-module via `@overflow("trap")` / `@overflow("wrap")`).

### 5.4 Inference

Type **annotations are mandatory** on:
- Function parameters
- Function return types
- Class fields
- `let` bindings at function scope

Type **inference is permitted** for:
- Lambda parameter types if the lambda is in a typed context
- Generic type argument inference at call sites (Hindley-Milner-lite — see §10.4)
- Literal types when assigned to a typed binding (`x: i64 = 0` makes `0` an `i64`)

### 5.5 Tuples (v0.1 — M14)

Tuples are **heterogeneous fixed-size product types**. The runtime
representation is a heap object with one 8-byte slot per element; the
type system enforces arity and per-element types at every use site.

**Type annotation.** Two equivalent forms:

```python
t: Tuple[i32, str]            # subscript form (preferred)
t: (i32, str)                 # parenthesised form (also legal)
```

**Literal.** Parenthesised, at least one comma. A single value in
parens is just parenthesisation — there is **no `(x,)` 1-tuple
shorthand** in v0.1.

```python
t: Tuple[i32, str] = (42i32, "hi")
```

**Field access.** Bare integer index with attr syntax:

```python
println(t.0)   # → 42
println(t.1)   # → "hi"
```

**Destructuring let.** Both annotated and inferred forms:

```python
x: i32, y: str = pair()       # all annotated
x, y = pair()                  # element types inferred from RHS
```

Per-name annotations must agree element-wise with the RHS tuple type;
mismatches raise `E_TYPE_MISMATCH`. Arity must match exactly.

**Return-position tuples.** A function may declare `Tuple[T1, ..., Tn]`
as its return type and `return (e1, ..., en)`.

**Equality.** `t == u` is element-wise (`!=` is `not (eq)`). Defined
when both operands have the same `Ty::Tuple` shape; strings compare by
value (per M12 BUG-034 fix), floats use IEEE semantics, classes use
reference equality.

**`str(t)`.** Returns `"(e0, e1, ..., eN)"`. Bare-element form: string
elements have no surrounding quotes, separator is `", "`. Mirrors the
informal Python repr but without the type-decoration.

**v0.1 limits.**

- Arity must be in `2..=8`. Smaller (0/1) and larger (9+) tuples are
  not supported in v0.1.
- Tuples are immutable after creation: there is no `t.0 = x`. Build a
  new tuple instead.
- Tuples are not iterable (no `for x in t:` over a tuple). Use static
  indexing: `t.0`, `t.1`, ...
- `len(t)` is not defined (the arity is part of the type).
- Tuples cannot be `Dict` keys in v0.1 (no `Hash` impl).
- Pattern matching on tuples in `match`/`case` is supported as of M16
  (positional only — `case (a, b):` — with `Identifier` and `Wildcard`
  sub-patterns).

**Subtyping.** `Tuple[T1, ..., Tn] <: Tuple[U1, ..., Un]` iff
`Ti <: Ui` pointwise (covariant per §5.2).

### 5.6 Forbidden constructs

The following are compile errors:

- `Any` type or `object` as catch-all
- Untyped function parameters or returns
- `setattr` / `getattr` with non-literal name
- `eval`, `exec`, `compile` at runtime
- Metaclass declaration
- Modifying `__dict__` of an instance
- Modifying a class after definition (`Foo.bar = ...`)
- Dynamic base classes
- Multiple inheritance (single inheritance only; use protocols for mixins)

---

## 6. Static Semantics

### 6.1 Modules

Each `.spy` file is a module. Module name is its filename without extension. Packages are directories containing `__init__.spy`.

Module-level execution order: top-down. Top-level statements other than imports, type aliases, const declarations, and class/function/protocol declarations are **forbidden** — modules contain only declarations. Initialization runs inside a generated `__init__` function called once when the module is loaded.

### 6.2 Name resolution

Scopes, innermost-first:
1. Local (function body, comprehension)
2. Enclosing function (for nested functions)
3. Module
4. Built-ins

There is **no `global` or `nonlocal`** — outer scope variables are read-only from inner functions. To mutate, pass a mutable reference explicitly.

### 6.3 Definite assignment

Every `let` binding must be initialized at declaration. Re-assignment requires the LHS to have been declared.

```python
x: i32 = 0      // declaration + initialization
x = 1           // assignment OK
y = 2           // ERROR: y not declared
y: i32 = 2      // OK
y: i32 = 3      // ERROR: y already declared in this scope
```

### 6.4 Flow-sensitive narrowing

Nullable and union types narrow based on:
- `is none` / `is not none`
- `isinstance(x, T)` checks
- Pattern matching
- `assert x is not none`
- Early returns / `raise`

### 6.5 Exhaustiveness

`match` statements must be exhaustive over the scrutinee type, or include a wildcard `_` case.

**M16 v0.1 implementation note.** The full algebraic-datatypes exhaustiveness pass is deferred. The current compiler emits a *warning* to stderr (not a hard error) when:

* the scrutinee's static type is a `sealed class` with declared subclasses, AND
* the `match` has no `case _:` arm and no terminal `case x:` identifier pattern, AND
* at least one direct subclass appears in *no* `case Constructor(...):` arm.

Open-class scrutinees, primitive scrutinees, and tuple scrutinees are not yet exhaustiveness-checked at all. An unmatched scrutinee at runtime simply falls out of the `match` block — there is no `MatchError` exception. v0.2 will tighten this into a compile error with full algebraic coverage.

### 6.5.1 Supported pattern forms (M16 v0.1)

| Pattern                       | Source           | Notes                                                                 |
|-------------------------------|------------------|-----------------------------------------------------------------------|
| `case _:`                     | `Wildcard`       | Always matches; binds nothing.                                        |
| `case x:`                     | `Identifier`     | Always matches; binds the scrutinee to `x`.                           |
| `case ClassName(p1, p2, ...):`| `Constructor`    | Runtime `isinstance` test; sub-patterns bind fields by declaration order. Only `Identifier` and `Wildcard` sub-patterns are supported in v0.1 — nested constructor patterns are deferred. |
| `case (p1, p2, ...):`         | `Tuple`          | Unconditional positional destructure (arity already verified by the typechecker). Only `Identifier` and `Wildcard` sub-patterns in v0.1.                              |
| `case 42:` / `case "hi":`     | `Literal`        | Equality test against an `int` / `float` / `str` / `char` / `bool` / `none` literal. |

**Deferred (v0.2+):** or-patterns (`case A | B:`), guard clauses (`case Pat if cond:`), keyword-arg constructor patterns (`case Foo(x=1):`), range patterns, mapping patterns (`case {"k": v}:`), and nested constructor patterns (`case Pair(Number(n), c):`).

The scrutinee expression is evaluated **exactly once**, regardless of how many arms reference it; the value is stashed in a hidden local slot at the top of the lowered match.

### 6.6 Purity (advisory)

Functions marked `@pure` are checked to have no observable side effects. The compiler may freely reorder, cache, or eliminate calls to pure functions.

### 6.7 Imports and modules (v0.2 — M19)

The parser has accepted `import` and `from ... import` since M0 (the AST has carried `Module::imports` since the start), but M19 is the first milestone where the resolver, typechecker, and IR lowerer wire them through to the runtime.

**Syntax** (also see §4 grammar):

```
import_stmt ::= "from" dotted_name "import" import_list NEWLINE
              | "import" dotted_name [ "as" identifier ] NEWLINE
import_list ::= identifier [ "as" identifier ] { "," identifier [ "as" identifier ] }
```

Three forms, all bound to top-level (module) scope:

* `import sys` — introduces the name `sys` as a module reference.
* `import sys as s` — same, bound under an alternate local name `s`.
* `from sys import argv, exit` — each named item is introduced as a top-level binding (a value-typed `argv: List[str]`, a function-typed `exit: fn(i32) -> Never`). Aliases (`from sys import exit as bail`) are accepted per spec §4.

Imports must appear at the top of the module (before any decl); the parser enforces this.

**v0.2 stdlib is built-in.** All built-in modules are compiled into the resolver as a `StdlibModuleTable` (resolver-internal). Each entry lists its items, their static types, and the `NativeFn` discriminant the IR lowerer emits. There is no on-disk stdlib package, no module loader, and no `__init__.spy`. The shipped table is intentionally tiny:

| Module | Items |
|--------|-------|
| `sys`  | `argv`, `exit`, `platform`, `version` (see §9.x below) |

**Resolution rules.**

* `import M` (or `import M as A`): if `M` is in the stdlib table, bind a `BuiltinModule` symbol; otherwise produce `E4001 "no stdlib module named '<M>' (user-defined modules are v0.3)"`.
* `from M import x`: if `M` is in the stdlib table, look up `x` in `M.items`; on hit, bind `x` (or its alias) as a top-level symbol whose type is the item's declared type. On miss, produce `E4002 "module 'M' has no item named 'x'"`.
* Legacy compatibility: `from threading import Channel` continues to work as a no-op because `Channel` is already in the prelude. v0.3 will retire this shim once `threading` migrates to the stdlib table.
* Reading a module attribute (`sys.argv` or `s.argv` after `import sys as s`) is dispatched by the typechecker via the same stdlib table — module-attribute access is not a real field load.

**Out of scope for v0.2** (deferred to v0.3+):

* User-defined modules — multi-file `.spy` programs that import each other.
* Submodules — `import os.path` (parser accepts dotted names but resolution rejects them).
* Star imports — `from sys import *`.
* Re-exports / `__all__`.
* Cyclic-import detection (waits for user modules to exist).

**Runtime model.** Module items dispatch through the standard `CALL_NATIVE` opcode using `NativeFn` ids in the `130-149` range (`sys`). A *constant* like `sys.argv` lowers to a 0-arg `CALL_NATIVE`; a *function* like `sys.exit(0)` lowers to an N-arg `CALL_NATIVE`. Constants are lazily materialised (and cached) by the VM on first access; see `Interpreter::sys_argv_cache`.

---

## 7. Dynamic Semantics

### 7.1 Evaluation order

Strict left-to-right for arguments. Short-circuit for `and`, `or`, `??`.

### 7.2 Integer arithmetic

- Signed overflow: traps in debug builds (raises `OverflowError`), wraps in release builds. (`+`, `-`, `*` on `i32`/`i64` are checked in debug.)
- Unsigned overflow: always wraps.
- `/` is **true division** (Python 3 semantics): it always yields `f64`,
  even for integer operands (`7 / 2 == 3.5`). Both operands are widened to
  `f64` first. Use `//` for integer division. Because the result is `f64`,
  `/=` requires a float target; an integer target is a compile error
  (use `//=`).
- `//` is integer (truncated, toward zero) division for ints — keeping the
  (widened) integer operand type — and plain division for floats.
- Division by zero: integer `/0` and `//0` raise `ZeroDivisionError`
  (legacy alias `DivisionByZeroError`). Float `/0.0` yields `inf`/`nan`
  per IEEE 754 (no trap).
- `%` follows the sign of the divisor (Python semantics).

### 7.3 Float arithmetic

IEEE 754. NaN, Inf permitted. Comparisons follow IEEE rules (NaN != NaN).

### 7.4 String operations

- `str` is UTF-8 internally.
- Indexing `s[i]` returns the i-th *code point* (O(1) only if the string is ASCII; otherwise the runtime maintains a side index for O(log n) access).
- `len(s)` returns the number of code points.
- `bytes(s)` returns the underlying byte buffer.
- Concatenation produces a new string.

### 7.5 Exceptions

Raised via `raise`. Caught via `try`/`except`.

```python
try:
    f: io.File = open("data.csv", "r")
    contents: str = f.read()
except IOError as e:
    println("missing file: " + e.message)
except Exception as e:
    println("other: " + e.type_name + ": " + e.message)
finally:
    cleanup()
```

#### 7.5.1 Built-in exception types (M15 v0.1)

The following exception type names are recognised by both `raise` and
`except`. Matching is by exact `type_name` string (case-sensitive),
except for `Exception` which is the catch-all and matches any thrown
type.

| Name                  | Raised when                                                            |
|-----------------------|------------------------------------------------------------------------|
| `Exception`           | Catch-all; never raised directly by the runtime (user code may raise). |
| `IOError`             | `open()` fails (missing file, permission denied); file read/write fails. |
| `IndexError`          | `xs[i]` out of range; `list.pop()` on empty list.                       |
| `KeyError`            | Reserved — emitted from dict-key lookups in future revisions.           |
| `ValueError`          | Malformed argument to a numeric constructor, etc.                       |
| `TypeError`           | Reserved — emitted from reflection / FFI mismatches.                    |
| `ZeroDivisionError`   | `a / 0` for integer `a`. (Legacy name `DivisionByZeroError` is also recognised.) |
| `AssertionError`      | `assert cond` failed.                                                   |
| `NullPointerError`    | Dereference of `none` via `.field` or `[i]`.                            |
| `RuntimeError`        | Reserved — generic catch-all surface for runtime traps.                 |
| `StopIteration`       | Reserved — for iterator protocols.                                      |
| `ChannelClosedError`  | `Channel.send` / `recv` after close.                                    |

#### 7.5.2 Exception value surface

When a handler binds via `as e`, the caught exception is a heap object
with exactly two readable fields:

* `e.type_name: str` — the exception's runtime type name (e.g. `"IOError"`).
* `e.message: str`   — the message argument passed to the constructor
                       (or a runtime-supplied diagnostic for native-raised
                       errors like `"index 10 out of range for length 2"`).

#### 7.5.3 `raise` syntax (v0.1)

```python
raise IOError("file not found")
raise IndexError("…")
raise KeyError("wrapped") from cause   # exception chaining (see below)
```

The single argument MUST be a `str`-typed expression. The exception
class name must be one of the built-in names in §7.5.1 (or a user-defined
subclass of `Exception`).

**Exception chaining (`raise X from Y`).** The `from` cause `Y` must be an
exception value (a subclass of `Exception`); a non-exception cause is rejected
at compile time (`E2050`). Because the exception object carries only the
two fields in §7.5.2 (no dedicated `__cause__` slot yet), the cause is
*preserved by folding it into the raised exception's `message`* as a chained
suffix: `"<msg> [caused by <CauseType>: <cause msg>]"`. This keeps the cause
observable via `e.message` rather than silently discarding it (the historical
behaviour). A real `__cause__` field / traceback is deferred.

#### 7.5.4 `finally` semantics

A `finally` block runs in every exit path from the `try`:

1. After successful body completion.
2. After a matched `except` handler completes.
3. While propagating an unhandled exception (runs first, then the
   exception continues unwinding past the `finally`).

Early `return` from inside a `try` body is undefined for v0.1 — the
implementation may or may not run the finally. Programs that need
guaranteed cleanup should restructure to put cleanup after the `try` or
use a `finally`-only construct (no `except`) that captures via the
propagating path.

#### 7.5.5 Catch-all order, tuple filters, and `else`

Multiple `except` clauses are matched top-to-bottom. The first arm
whose filter matches the thrown `type_name` runs. Use `except
Exception as e:` LAST to catch anything not handled by an earlier
specific arm.

**Tuple filters (`except (A, B) as e:`).** A parenthesised tuple of exception
types catches the raised exception iff its type matches *any* listed type (or
a subclass thereof) — exactly Python's semantics. It does NOT catch unrelated
types: `except (ValueError, KeyError)` lets an `IndexError` propagate.
The bound `e` has the type of the first listed exception class (every listed
type is an `Exception` subclass, so the inherited `message`/`type_name` fields
in §7.5.2 are always available).

> Historical note: a tuple filter previously degraded silently to a bare
> catch-everything (`except:`), swallowing exceptions the program never meant
> to catch. This is fixed.

**`else:` clause.** A `try` may carry an `else:` block. It runs iff the body
completed with NO exception, and it runs *after* the handler frame is popped,
so an exception raised inside `else` is NOT caught by the same `try`'s
handlers (Python semantics). When both `else` and `finally` are present,
`else` runs before `finally` on the success path.

> Historical note: the `else` block was previously dropped at lowering (its
> body silently never ran). This is fixed.

#### 7.5.6 Out of scope for v0.1

These constructs parse but are not lowered (or are deferred entirely):

* Bare `raise` (re-raise) outside a handler.
* A real `__cause__` / `__context__` field and tracebacks. `raise X from Y`
  *is* supported and preserves the cause via the `message` chain (§7.5.3),
  but there is no separate `__cause__` slot to introspect.

Functions containing a `try` or `raise` statement fall back to the
bytecode interpreter — the Cranelift JIT does not compile them in
v0.1. See `compiler/src/decompile.rs::opcode_name` (the `Throw |
EnterTry | LeaveTry | Rethrow` arm).

Exception classes must inherit from `Exception`. The compiler tracks declared exception types for documentation but does not enforce checked exceptions (open design question — see §22).

Stack unwinding releases resources via `with` blocks (RAII-style).

### 7.6 Resource management

```python
with open("file.txt") as f: str:
    contents: str = f.read()
```

`with` requires the context manager to implement protocol `Context[T]`:

```python
protocol Context[T]:
    fn __enter__(self) -> T
    fn __exit__(self, exc: Exception?) -> bool
```

**Implementation status (v1).** General `__enter__`/`__exit__` dispatch is not
yet wired through lowering. The only context manager whose cleanup is actually
run is `io.File` (via `open(...)`), whose `__exit__` lowers to a `FileClose`
on normal exit. Using `with` on any *other* resource is a hard compile error
(`E2070`) — previously such a `with` silently lowered its cleanup to a no-op,
so locks / DB handles / etc. were never released (broken RAII). Until full
context-manager dispatch lands, use an explicit `try/finally` for non-`io.File`
resources.

---

## 8. Memory & Object Model

### 8.1 Object header

Every heap-allocated object begins with a 16-byte header:

```
offset 0:  vtable_ptr      (8 bytes) → points to TypeInfo (see §8.2)
offset 8:  gc_meta         (8 bytes) → GC color bits, lock bit, age, hash cache
```

Fields follow at offset 16, packed in declaration order with natural alignment.

### 8.2 TypeInfo

A static structure emitted by the compiler per class:

```
TypeInfo {
    u32  type_id           // unique per type
    u32  size              // in bytes
    u32  field_count
    u32  vtable_len
    *FieldInfo  fields     // for GC scanning + reflection
    u8   gc_kind           // 0=no refs, 1=has refs, 2=variable size (lists), 3=string
    *str  qualified_name
    *TypeInfo  base        // null for root types
    *ItableEntry  itable   // protocol dispatch
    fn_ptr  vtable[N]      // method table inline at end
}
```

### 8.3 Memory layout examples

```python
final class Point:
    x: f64
    y: f64
```

Layout: `[header:16][x:8][y:8]` = 32 bytes.

```python
class Particle(Point):
    mass: f32
    velocity_x: f64
    velocity_y: f64
```

Layout: `[header:16][x:8][y:8][mass:4][pad:4][velocity_x:8][velocity_y:8]` = 56 bytes.

### 8.4 List layout

```
List[T] {
    header:    16 bytes
    length:    8 bytes   (usize)
    capacity:  8 bytes   (usize)
    data_ptr:  8 bytes   (*T)
}
```

`data` is a separately allocated contiguous buffer of `T` values.

### 8.5 String layout

```
str {
    header:     16 bytes
    length:     8 bytes  (code points)
    byte_len:   8 bytes  (UTF-8 bytes)
    data_ptr:   8 bytes  (*u8, may be inline for small strings)
    flags:      8 bytes  (interned, ascii-only, has-side-index)
}
```

### 8.6 Closures

A closure is a heap object containing a function pointer and captured values:

```
Closure {
    header:      16 bytes
    fn_ptr:      8 bytes
    capture_n:   4 bytes
    captures:    [variable]
}
```

Captures are immutable. Mutable closures require explicit boxing (`Cell[T]`).

---

## 9. Standard Library (Core)

The minimum library required for the VM to be useful. Implemented partly in StrictPy, partly via FFI to a runtime written in the host language.

### 9.1 Module `builtins` (auto-imported)

```
fn print(s: str) -> None
fn println(s: str) -> None
fn len[T: Sized](x: T) -> usize
fn abs[T: Numeric](x: T) -> T
fn min[T: Comparable](a: T, b: T) -> T
fn max[T: Comparable](a: T, b: T) -> T
fn range(start: i64, stop: i64, step: i64 = 1) -> Range
fn assert(cond: bool, msg: str = "") -> None
fn isinstance(x: any, T: type) -> bool   # M16

# isinstance(x, T) — runtime class check. T must name a user class;
# protocols, primitive types, and generic instantiations are NOT valid
# second arguments in v0.1. Walks the parent chain via the runtime
# type table, so `isinstance(sub_instance, Base)` is true.
#
# Acts as a flow-narrowing predicate inside `if isinstance(x, T):` —
# within the then-branch `x` has static type `T`. The narrowing is
# rolled back at the end of the branch. Narrowing does NOT compose
# through `and` / `or` in v0.1 (i.e. `if isinstance(x, A) and x.f > 0:`
# does not see `x: A` in the right operand).

# str(x: T) -> str  -- canonical text form of x
#   * For floats, integral values keep one trailing decimal place
#     (`str(3.0) == "3.0"`) so the result is unambiguously a float
#     when round-tripped through parse_f64. Non-integral values use
#     Rust's shortest-round-trip representation (`str(3.14) == "3.14"`).
#   * For chars, the single-codepoint string (`str('h') == "h"`).
#   * For ints/bool, the obvious decimal / "true"/"false" form.
# Primitive constructors (i32(x), i64(x), f64(x), char(x)) dispatch on
# the *static* type of the argument; mixing arg types is well-defined
# (e.g. `i32(i64_var)` truncates, `f64(i64_var)` widens). See M11 fix
# notes — pre-M11 they all read the bit pattern as if it were f64.

class Exception:
    message: str
    fn __init__(self, message: str) -> None

class ValueError(Exception): pass
class IndexError(Exception): pass
class KeyError(Exception): pass
class TypeError(Exception): pass        // only raised by FFI / reflection
class OverflowError(Exception): pass
class DivisionByZeroError(Exception): pass
class IOError(Exception): pass
class NullPointerError(Exception): pass

protocol Sized:    fn __len__(self) -> usize
protocol Hashable: fn __hash__(self) -> i64
protocol Comparable:
    fn __lt__(self, other: Self) -> bool
    fn __eq__(self, other: Self) -> bool
protocol Iterable[T]:
    fn __iter__(self) -> Iterator[T]
protocol Iterator[T]:
    fn __next__(self) -> T?
protocol Numeric:
    fn __add__(self, other: Self) -> Self
    fn __sub__(self, other: Self) -> Self
    fn __mul__(self, other: Self) -> Self
    fn __neg__(self) -> Self
```

### 9.2 Module `collections`

```
class List[T]:           // primitive in the runtime
class Dict[K: Hashable, V]:
class Set[T: Hashable]:
class Deque[T]:
class HashMap[K: Hashable, V]:
```

### 9.3 Module `io`

```
class File:
    fn open(path: str, mode: str) -> File
    fn read(self) -> str
    fn read_bytes(self) -> bytes
    fn write(self, s: str) -> None
    fn close(self) -> None
    fn __enter__(self) -> File
    fn __exit__(self, exc: Exception?) -> bool
```

### 9.4 Module `math`

```
final PI: f64 = 3.14159265358979323846
final E:  f64 = 2.71828182845904523536

fn sqrt(x: f64) -> f64
fn sin(x: f64) -> f64
fn cos(x: f64) -> f64
fn pow(x: f64, y: f64) -> f64
fn floor(x: f64) -> f64
fn ceil(x: f64) -> f64
```

### 9.5 Module `result`

```
sealed class Result[T, E]:
    pass

final class Ok[T, E](Result[T, E]):
    value: T

final class Err[T, E](Result[T, E]):
    error: E
```

### 9.6 Module `sys` (v0.2 — M19)

The first stdlib module that lives behind the M19 import-resolver
infrastructure (rather than being flattened into the prelude). Foundation
for the larger M20 batch (`os`, `os.path`, `io`, `json`, `re`, `time`,
`random`, `math+`).

```
argv:     List[str]      # program args; argv[0] is the script path the user typed
platform: str            # "windows" | "linux" | "macos" | "unknown"
version:  str            # banner string, e.g. "StrictPy v0.2"

fn exit(code: i32) -> Never
```

Semantics:

* `sys.argv` — lazy `List[str]`. Materialised on first read by the VM
  and cached so subsequent reads return the same heap object (allowing
  `sys.argv.append(...)` to be visible across the program).
  `argv[0]` is conventionally the script path the user typed at the
  command line: the `.spy` source for `spy script.spy`, the `.spyc`
  for `spy module.spyc`, or the literal string `"-c"` for `spy -c
  "code"` (matching CPython's convention).
* `sys.exit(code)` — terminates the program with the given exit code.
  **Not catchable.** Calling `sys.exit` from inside a `try ... except
  Exception:` walks straight past the handler (mirrors Python:
  `SystemExit` derives from `BaseException`, not `Exception`).
  Implemented as a non-`UncaughtException` VM error variant
  (`VmError::Exit(i32)`) that only the top-level CLI driver translates
  into a process exit code.
* `sys.platform` — derived from the host platform at runtime
  (`cfg!(target_os = ...)` inside the VM).
* `sys.version` — pinned per build; will move to a `Cargo.toml`-derived
  constant in M20 so the runtime and spec can't drift.

I/O handles (`sys.stdin` / `sys.stdout` / `sys.stderr`) are deferred to
M20 — the M5 `io.File` type requires going through `open(...)` and the
pseudo-files for stdin/stdout/stderr don't fit that constructor cleanly.

### 9.7 Module `os` (v0.2 — M20a)

Environment-variable and filesystem syscalls. Every function below wraps a
Rust `std::env` or `std::fs` call directly; OS errors surface as
`IOError` (catchable via `try ... except IOError`) — the same pattern
as M5's `open(...)`.

```
fn env(key: str) -> str?
fn set_env(key: str, value: str) -> None
fn getcwd() -> str
fn chdir(path: str) -> None
fn listdir(path: str) -> List[str]
fn remove(path: str) -> None
fn mkdir(path: str) -> None
fn exists(path: str) -> bool
fn is_file(path: str) -> bool
fn is_dir(path: str) -> bool
fn read_file(path: str) -> str
fn write_file(path: str, content: str) -> None
```

Semantics:

* `env` returns `none` for both "variable unset" and "value not valid
  UTF-8". The distinction isn't preserved (use `??` or `is none` to
  branch).
* `set_env` mutates the process environment — visible to child processes
  spawned afterward but **not** to already-running threads on Windows.
* `listdir` returns bare entry names (no path prefix, no trailing
  separator). Symbolic links surface as their name; their target type
  is queryable via `is_file`/`is_dir`.
* `mkdir` is non-recursive (no `mkdir -p`). A future `os.makedirs` is
  v0.3 work.
* `read_file` / `write_file` are convenience wrappers over the M5
  `open()` + `read`/`write` + `close` dance; they're not in CPython's
  `os` (they live in `pathlib`/`open(...).read()`) but bundling them
  saves ~6 lines per script.
* The path argument is interpreted by the host OS. Forward slashes work
  on Windows (they're translated by the Win32 API); use `path.sep` if
  you want to be explicit.

### 9.8 Module `path` (v0.2 — M20a)

Pure path-string manipulation. Python ships these under `os.path` but
StrictPy v0.2 has no submodule support (deferred to v0.3), so they live
under a flat top-level `path` module.

```
fn join(a: str, b: str) -> str
fn join3(a: str, b: str, c: str) -> str
fn dirname(p: str) -> str
fn basename(p: str) -> str
fn splitext(p: str) -> Tuple[str, str]

sep: str
```

Semantics:

* `join` / `join3` delegate to Rust's `std::path::Path::join`. The OS
  separator is used (`/` on Unix, `\` on Windows). 3-arg `join3` exists
  because the v0.2 language has no variadic functions.
* `dirname("/a/b/c.txt")` → `"/a/b"`; `dirname("bare")` → `""`. Empty
  result is intentional (Python returns `""` too).
* `basename("/a/b/c.txt")` → `"c.txt"`. Trailing separators are
  stripped (Python compatibility).
* `splitext("a.txt")` → `("a", ".txt")` — the dot is part of the
  extension. Leading-dot filenames (`.bashrc`) are treated as having no
  extension: `splitext(".bashrc")` → `(".bashrc", "")`. Returns a
  heap-allocated `(str, str)` tuple (M14).
* `sep` is the OS path separator string — `"/"` or `"\\"`. Read it once
  at module load if you're building paths from data.

### 9.9 Module `io` (v0.2 — M20a)

Line-based standard-stream IO. Complements the M5 `io.File` class
(opened-file reads/writes) with stdin/stdout/stderr access.

```
fn input() -> str
fn input_with_prompt(prompt: str) -> str
fn write_stdout(s: str) -> None
fn write_stderr(s: str) -> None
fn flush_stdout() -> None
```

Semantics:

* `input()` reads one line from stdin, stripping the trailing `\n`
  (and `\r\n` on Windows). Raises `IOError` on EOF before any byte or
  on a read error.
* `input_with_prompt(prompt)` writes `prompt` to stdout (no newline),
  flushes, then reads a line. Convenience for CLI tools.
* `write_stdout(s)` and `write_stderr(s)` write `s` literally — no
  auto-appended newline. `print` / `println` cover the newline case;
  these are for streaming output or for emitting partial lines.
* `flush_stdout()` flushes the process's OS-level stdout buffer. Needed
  before reading stdin if the program wrote a prompt without a trailing
  newline (most terminals line-buffer stdout, so without flushing the
  prompt won't appear until the line read completes).

Note: in v0.2 there are no `sys.stdin` / `sys.stdout` / `sys.stderr`
`File`-typed handles. The functions above are the supported interface.

### 9.10 Module `time` (v0.2 — M20b)

Wall-clock + monotonic clock + sleep. Sister to M20a's `os` / `io` —
plugs into the same M19 stdlib-module-table infrastructure.

```
fn now() -> f64
fn now_ms() -> i64
fn monotonic() -> f64
fn sleep_s(seconds: f64) -> None
fn sleep_ms(millis: i64) -> None
fn format_iso(epoch_s: f64) -> str
```

Semantics:

* `now()` returns Unix-epoch seconds with fractional precision (Rust's
  `SystemTime::now().duration_since(UNIX_EPOCH).as_secs_f64()`).  The
  underlying system clock can jump backwards under NTP adjustments — for
  benchmarking, use `monotonic()` instead.
* `now_ms()` returns Unix-epoch milliseconds as `i64`.  Convenient when
  the program will compare two timestamps without needing sub-ms
  precision (network logs, jitter samples, etc.).
* `monotonic()` returns seconds-since-this-interpreter-started (anchored
  to a per-process `Instant` set at VM construction).  Guaranteed to be
  non-decreasing across reads; not affected by wall-clock adjustments.
  This is the correct primitive for micro-benchmarks.
* `sleep_s(secs)` and `sleep_ms(ms)` block the calling thread for the
  given duration.  Negative or NaN inputs are silently no-ops (matches
  Python's `time.sleep`).  Sleep granularity is OS-dependent — Windows'
  default tick is ~15.6ms; Linux is typically 1ms.  Tests should use
  generous (≥50ms) floor assertions to absorb OS scheduling variance.
* `format_iso(epoch_s)` produces a fixed `"YYYY-MM-DDTHH:MM:SSZ"` UTC
  string for the given epoch seconds (no fractional component, no
  timezone offsets — UTC only).  The conversion uses Howard Hinnant's
  `civil_from_days` algorithm, so dates pre-1970 and far into the
  future round-trip correctly.

### 9.11 Module `random` (v0.2 — M20b)

Seeded pseudo-random number generation.  Backed by a linear-congruential
generator (LCG) with the Numerical Recipes constants
(multiplier 1103515245, increment 12345, modulus 2^31).  LCG state lives
on the interpreter — calling `random.seed(s)` resets it; default state
at process start is `0`.

```
fn seed(s: i64) -> None
fn randint(lo: i64, hi: i64) -> i64
fn random() -> f64

# Monomorphic variants — see "Generics" below.
fn choice_i64(xs: List[i64]) -> i64
fn choice_f64(xs: List[f64]) -> f64
fn choice_str(xs: List[str]) -> str

fn shuffle_i64(xs: List[i64]) -> None
fn shuffle_f64(xs: List[f64]) -> None
fn shuffle_str(xs: List[str]) -> None

fn sample_i64(xs: List[i64], n: i32) -> List[i64]
fn sample_f64(xs: List[f64], n: i32) -> List[f64]
fn sample_str(xs: List[str], n: i32) -> List[str]
```

Semantics:

* `randint(lo, hi)` returns a uniform integer in `[lo, hi]` (inclusive
  on both ends).  Raises `ValueError` when `hi < lo`.
* `random()` returns a uniform `f64` in `[0.0, 1.0)`.  Two LCG draws are
  combined for ~53 bits of mantissa entropy.
* `choice_T(xs)` returns one element of `xs` chosen uniformly at random.
  Raises `IndexError` on an empty list.
* `shuffle_T(xs)` shuffles `xs` in place via Fisher-Yates.  Preserves
  length and element identity (no copies).
* `sample_T(xs, n)` returns a fresh `List[T]` of `n` distinct elements
  drawn uniformly from `xs`.  Raises `ValueError` on `n < 0` or
  `n > len(xs)`.  Implementation is a partial Fisher-Yates over a copy
  of `xs`, so the source list is unchanged.

**Generics**: stdlib functions cannot be generic in v0.2 (the M17
generic-fn worklist only sees user-defined `.spy` fns).  We ship
monomorphic `_i64` / `_f64` / `_str` variants for `choice` / `shuffle` /
`sample`.  A true generic `random.choice[T](xs: List[T]) -> T` is
deferred to v0.3.

**Quality**: LCGs are reproducible and fast, not crypto-quality.  The
NR constants are well-known and have decent statistical properties for
casual use (games, fuzzers, monte-carlo demos), but the period is only
~2^31.  Programs requiring cryptographic randomness should use the
underlying OS RNG (a future v0.3 `os.urandom` is the planned answer).

### 9.12 Module `math` (v0.2 — M20b)

Numeric helpers, namespaced.  This module *extends* the §9.4 surface —
the existing prelude bare-name functions (`sqrt(x)`, `sin(x)`, `cos(x)`,
`pow(x, y)`, `floor(x)`, `ceil(x)`, `log(x)`, `exp(x)`) remain available
without `import math`, for backward compatibility with v0.1 programs.

```
final pi:  f64    # π
final e:   f64    # Euler's number
final tau: f64    # 2π
final inf: f64    # +∞
final nan: f64    # NaN

# Wrapped — same NativeFn ids as the prelude bare-name versions.
fn sqrt(x: f64) -> f64
fn sin(x: f64) -> f64
fn cos(x: f64) -> f64
fn log(x: f64) -> f64      # natural log
fn exp(x: f64) -> f64
fn pow(x: f64, y: f64) -> f64

# New in M20b.
fn log2(x: f64) -> f64
fn log10(x: f64) -> f64
fn floor(x: f64) -> i64    # returns int, not float (cf. Python's math.floor)
fn ceil(x: f64) -> i64
fn gcd(a: i64, b: i64) -> i64
fn factorial(n: i64) -> i64
fn is_nan(x: f64) -> bool
fn is_inf(x: f64) -> bool  # true for both +∞ and -∞
```

Semantics:

* Constants are `final f64` (M20b's stdlib-table `Const` items lower to
  zero-arg `CallNative` instructions that return the bit pattern).
* `floor` / `ceil` return `i64` to match Python 3's `math.floor` /
  `math.ceil` semantics (Python returns `int`, not `float`).  Non-finite
  inputs raise `ValueError`.
* `gcd(a, b)` runs the Euclidean algorithm on `|a|` / `|b|`; result is
  non-negative.  `gcd(0, 0) == 0`.
* `factorial(n)` is defined for `0 ≤ n ≤ 20`.  Outside that range:
  * `n < 0` → `ValueError`,
  * `n > 20` → `OverflowError` (21! exceeds `i64::MAX`).
* `is_nan(x)` and `is_inf(x)` are NaN-/inf-safe (don't trap on inputs
  that would otherwise be problematic in arithmetic comparisons).

The wrapped functions (`math.sqrt`, etc.) route to the same `NativeFn`
ids as the prelude `sqrt` etc. — the only difference is the source-level
surface (namespaced vs bare).  Both forms type-check, both lower to the
same opcode, and both produce identical results.

### 9.13 Module `json` (v0.2 — M20c)

Validate and re-serialize JSON.  Backed by the `serde_json` crate.

```
fn parse_to_string(s: str) -> str   # parse + canonical compact form
fn minify(s: str) -> str            # alias of parse_to_string
fn is_valid(s: str) -> bool         # true iff `s` parses as JSON
fn pretty(s: str, indent: i32) -> str   # parse + N-space pretty print
fn escape(s: str) -> str            # render `s` as a JSON string literal
```

Semantics:

* `parse_to_string(s)` parses `s`, then re-serializes it.  Whitespace
  is normalised away; object keys are sorted lexicographically (a
  property of serde_json's BTreeMap representation).  Raises
  `ValueError` on malformed input.
* `minify(s)` is exactly `parse_to_string(s)` — both names exist for
  readability at call sites.
* `is_valid(s)` returns `true` if `s` parses as a valid JSON document;
  `false` otherwise.  Never raises.
* `pretty(s, indent)` parses `s` and emits an indented form with
  `indent` spaces per level.  `indent` is clamped to `[0, 32]`; an
  `indent` of `0` produces the compact form.  Raises `ValueError` on
  malformed input.
* `escape(s)` returns `s` wrapped as a JSON string literal: surrounding
  double quotes, control characters / non-ASCII escaped per RFC 8259.
  Useful for hand-building JSON output (escape variables, concatenate
  with the structural parts).

What v0.2 does **not** ship (but v0.3 — see §9.13.1 below — does):

* ~~A typed `JsonValue` tree exposed through the stdlib.~~ Shipped in
  M34 (v0.3) — see §9.13.1.
* Streaming / incremental parse.  Pull or SAX-style interfaces are out
  of scope for v0.3.

#### 9.13.1 Typed `JsonValue` tree (v0.3 — M34)

The flat surface above (`parse_to_string` / `is_valid` / `pretty` /
`escape` / `minify`) covers programs that want validation and
canonical reserialization but not structural access.  M34 adds a
*typed* surface for programs that want to walk a JSON document with
pattern matching — the same shape `examples/json_parse_v2.spy`
defines in user code, but now built into the standard library.

```
sealed class JsonValue: ...

final class JNull(JsonValue): ...
final class JBool(JsonValue):
    value: bool
final class JInt(JsonValue):
    value: i64
final class JFloat(JsonValue):
    value: f64
final class JString(JsonValue):
    value: str
final class JList(JsonValue):
    # Internal storage: List[JsonValue].  Destructure via
    # `case JList(items):` to access directly.
    fn length(self) -> i64
    fn get(self, i: i64) -> JsonValue       # raises IndexError
    fn items(self) -> List[JsonValue]       # defensive copy
final class JObject(JsonValue):
    # Internal storage: parallel keys + values lists.
    fn get(self, k: str) -> JsonValue?      # none if absent
    fn has(self, k: str) -> bool
    fn keys(self) -> List[str]              # insertion order
    fn length(self) -> i64

# Top-level functions on the json module:
fn parse(s: str) -> JsonValue               # raises ValueError on malformed input
fn stringify(v: JsonValue) -> str           # compact canonical form
fn stringify_pretty(v: JsonValue, indent: i32) -> str

# Constructor helpers — exactly equivalent to the class constructors,
# named for symmetry with `j_object`/`j_list` where the class shape
# isn't as natural a `List[Tuple[...]]` argument:
fn j_null() -> JsonValue
fn j_bool(b: bool) -> JsonValue
fn j_int(n: i64) -> JsonValue
fn j_float(f: f64) -> JsonValue
fn j_string(s: str) -> JsonValue
fn j_list(items: List[JsonValue]) -> JsonValue
fn j_object(entries: List[Tuple[str, JsonValue]]) -> JsonValue
```

Canonical use shape:

```python
import json

let parsed: JsonValue = json.parse('{"name": "alice", "age": 30}')
match parsed:
    case JObject(_):
        if isinstance(parsed, JObject):
            let name_v: JsonValue? = parsed.get("name")
            if name_v is not none:
                match name_v:
                    case JString(s):
                        println("name = " + s)
```

Semantics:

* `parse(s)` recursively builds a `JsonValue` tree.  JSON `null` /
  `true` / `false` map to `JNull` / `JBool`; numbers map to `JInt`
  when the input has no fractional part and fits in `i64`, else
  `JFloat`; strings to `JString`; arrays to `JList`; objects to
  `JObject` preserving insertion order.  Malformed input raises
  `ValueError`.
* `stringify(v)` emits compact canonical JSON.  `JObject` keys appear
  in insertion order (matching `parse`'s preserved order — *unlike*
  `parse_to_string` which sorts keys lexicographically via serde_json's
  `BTreeMap`).  `JFloat` values that have no fractional part are
  emitted without a trailing `.0` (so an integer round-trips through
  `JFloat` and back as the integer-shaped JSON form).  `NaN` /
  `+Inf` / `-Inf` are emitted as `null` because JSON has no encoding
  for them.
* `stringify_pretty(v, indent)` is the indented form; `indent` is
  clamped to `[0, 32]`.
* `JList.get(self, i)` and `JList.length()` work on the underlying
  `List[JsonValue]`.  `case JList(items):` lets user code skip the
  method API and use list operations (`items[i]`, `len(items)`)
  directly — both shapes are supported.
* `JObject.get(self, k)` returns `none` if `k` is absent (not
  `JNull`); use `has` for a presence check.

Construction shapes — both produce the same heap object:

```python
# Class constructor:
let v: JsonValue = JString("alice")

# Module helper (equivalent):
let v: JsonValue = json.j_string("alice")
```

Implementation notes (informative):

* The 7 classes are registered as ordinary `is_native: false` classes
  in the prelude (alongside `Channel` / `Thread` / `io.File`) so they
  participate in M11's vtable / M16's isinstance + match infrastructure
  without bespoke runtime code.  `from json import JsonValue` is a
  no-op because the names are already in scope (legacy "prelude wins"
  fall-through in the import resolver).
* JList stores its data as a `*const ListRepr` pointer at field offset 0;
  JObject stores parallel `keys` / `values` list pointers at offsets 0 / 8.
  The GC's `GcKind::Class` scanner traces those pointers automatically.
  No bespoke root scan is needed.
* Constructors with payload (everything except `JNull`) are special-
  cased in the IR's `lower_call`: the IR emits an `Alloc` op (with the
  correct runtime type id, so `isinstance` works) followed by a
  `NativeCall` to a class-specific init handler that stores the payload
  field.  Module-helper calls (`json.j_string(s)`) instead go through
  a single `NativeCall` to a parallel `JsonHelper*` handler that
  allocates + populates + returns.

What v0.3 does **not** ship (deferred to v0.4):

* Mutation methods (`JList.append` / `JList.set` / `JObject.set` /
  `JObject.remove`).  JsonValue is immutable in v0.3 — the typical
  parse → walk → re-serialise pattern doesn't need mutation, and
  keeping immutability lets the parser share JString instances across
  multiple occurrences of the same key.
* A dedicated `JBigInt` variant for JSON numbers above `i64::MAX`.
  v0.3 routes such numbers through `JFloat` (lossy above 2^53); v0.4
  will add `JBigInt` once the BigInt prelude work catches up.
* Iteration helpers like `JObject.iter_items() -> List[Tuple[str, JsonValue]]`
  for paired key/value walks.  v0.3 users compose `keys()` + `get(k)`.
* Module-scoped class registration.  Per the v0.4 plan, the prelude
  registration above is an interim — v0.4's stdlib-class infrastructure
  will let `from json import JsonValue` actually do something (rather
  than just shadow a no-op), and the JsonValue family will be
  un-shadowed from the prelude scope.  No source-level API change.

### 9.14 Module `re` (v0.2 — M20c)

Regex matching, search, replace, and split.  Backed by the `regex`
crate (linear-time NFA-based matcher, no catastrophic backtracking).

```
fn fullmatch(pattern: str, s: str) -> bool
fn search(pattern: str, s: str) -> bool
fn find(pattern: str, s: str) -> Tuple[i32, i32]
fn find_all(pattern: str, s: str) -> List[str]
fn replace(pattern: str, replacement: str, s: str) -> str
fn split(pattern: str, s: str) -> List[str]
fn is_valid(pattern: str) -> bool
```

Semantics:

* `fullmatch(pattern, s)` returns `true` iff `pattern` matches the
  *entire* string `s` (anchored at both ends — equivalent to wrapping
  the pattern in `^...$`).  Python's `re.match` would only anchor at
  the start; M20c picks the fullmatch semantic because it's the more
  useful "does this match exactly" question.  The name `fullmatch`
  (rather than `match`) sidesteps StrictPy's `match` keyword.
* `search(pattern, s)` returns `true` if the pattern matches anywhere
  in `s`.
* `find(pattern, s)` returns `(start, end)` byte offsets of the first
  match, or `(-1, -1)` if there's no match.  The tuple shape reuses
  the M14 tuple machinery (and the `alloc_tuple_obj` helper from
  M20a's `path.splitext`).
* `find_all(pattern, s)` returns all non-overlapping match substrings
  as `List[str]`.  Empty if no match.
* `replace(pattern, replacement, s)` substitutes every non-overlapping
  match.  Argument order matches Python's `re.sub(pattern, repl, s)`.
  The replacement string supports `$1` / `${name}` backreferences per
  the `regex` crate's `replace_all` semantics (literal `$` must be
  escaped as `\$`).
* `split(pattern, s)` returns the substrings of `s` between matches of
  `pattern`, as `List[str]`.  Empty captures produce empty strings.
* `is_valid(pattern)` returns `true` if `pattern` compiles, `false`
  otherwise.  Never raises — useful for "did the user typo their
  regex?" checks before a `try/except` around the real call.

All other functions raise `ValueError` on an invalid pattern (compile
error), with the message `"re: invalid pattern \"...\": <regex-crate
error>"`.

#### 9.14.1 Compiled `Pattern` class (v0.3 — M35 P4-A)

For hot-loop regex use, `re.compile(s) -> Pattern` returns a cached
handle that skips the per-call recompile cost paid by the flat
surface above.  The slot table backing each `Pattern` instance holds
the parsed `regex::Regex`; method dispatch on the instance reuses it
without re-parsing the pattern string.

```
fn compile(pattern: str) -> Pattern

final class Pattern:
    fn matches(self, s: str) -> bool          # full-string match
    fn find(self, s: str) -> str?             # first match or none
    fn find_all(self, s: str) -> List[str]
    fn replace(self, s: str, repl: str) -> str         # first match only
    fn replace_all(self, s: str, repl: str) -> str     # every match
    fn split(self, s: str) -> List[str]
    fn source(self) -> str                    # original pattern string
```

Semantics:

* `re.compile(s)` parses `s` and returns a `Pattern` whose handle is
  valid for the lifetime of the program.  Raises `ValueError` on a
  syntax error with the message
  `"re.compile: invalid pattern \"...\": <regex-crate error>"` —
  same shape as the flat surface, different prefix.
* `Pattern.matches(s)` mirrors the flat `fullmatch` semantic — the
  pattern must match the entire string.  (`fullmatch` was the chosen
  name on the flat surface to dodge the `match` keyword; the method
  on a `Pattern` instance can safely be called `matches` since
  method names are not keywords.)
* `Pattern.find(s)` differs from the flat `re.find` (which returns
  `(start, end)` indices): the method returns the **matched text** as
  `str?`, or `none` if there is no match.  Indices were the more
  useful shape on the flat surface (where callers don't have a
  Pattern instance to attach helpers to); the matched text is the
  more common need in code that's already paying the cost of holding
  a Pattern.
* `Pattern.find_all` / `Pattern.replace_all` / `Pattern.split` are
  identical to their flat counterparts in semantics; they just avoid
  the recompile.  `Pattern.replace` adds a one-shot replacement
  variant (Python `re.sub(..., count=1)` shape) that the flat
  surface didn't expose.
* `Pattern.source(self)` returns the original pattern string as
  passed to `re.compile`.  Useful for diagnostics and for
  serialising patterns through a build pipeline.

The flat functions (`re.fullmatch` / `re.search` / `re.find` /
`re.find_all` / `re.replace` / `re.split` / `re.is_valid`) remain
on the module — they pay the recompile cost but are still useful for
one-shot use.

What v0.3 does **not** yet ship:

* Capture groups.  `find` returns the whole match; named-capture
  extraction is v0.4 work (needs an `iter_captures` shape with a
  Match-row type for which the prelude infrastructure isn't ready).
* A lazy `iter_finds()` on `Pattern` — NativeFn id 799 is reserved
  for it.  `find_all` materialises the result eagerly.
* Python-specific syntax that `regex` doesn't support: lookbehind
  with variable width, `(?P<name>...)` (use `(?<name>...)` instead),
  and the look-around assertions Python's `re` carries for backwards
  compat.  The `regex` crate's syntax is documented at
  `https://docs.rs/regex/latest/regex/#syntax` — by and large it's a
  superset of `re` minus catastrophic-backtracking constructs.

### 9.15 Module `itertools` (v0.2 — M22 P2C)

Iteration helpers.  All functions take and return concrete `List[T]` /
`List[Tuple[U, V]]` values; lazy iterators (yielding generators) are
v0.3 work.

```
fn range_step(start: i64, stop: i64, step: i64) -> List[i64]
fn enumerate_str(xs: List[str]) -> List[Tuple[i32, str]]
fn enumerate_i64(xs: List[i64]) -> List[Tuple[i32, i64]]
fn zip_str_str(xs: List[str], ys: List[str]) -> List[Tuple[str, str]]
fn zip_i64_i64(xs: List[i64], ys: List[i64]) -> List[Tuple[i64, i64]]
fn chain_str(xs: List[str], ys: List[str]) -> List[str]
fn chain_i64(xs: List[i64], ys: List[i64]) -> List[i64]
fn take_str(xs: List[str], n: i32) -> List[str]
fn drop_str(xs: List[str], n: i32) -> List[str]
fn pairwise_str(xs: List[str]) -> List[Tuple[str, str]]
fn accumulate_i64(xs: List[i64]) -> List[i64]
fn flatten_str(xs: List[List[str]]) -> List[str]
```

Semantics:

* `range_step(start, stop, step)` materialises an `i64` list of values
  from `start` (inclusive) to `stop` (exclusive) by `step`.  `step`
  may be negative.  Raises `ValueError` on `step == 0`.  Traps on
  attempts to materialise more than 1 000 000 elements (a soft guard
  against user bugs; v0.3 will add lazy iteration).
* `enumerate_str` / `enumerate_i64` walk `xs` and produce a parallel
  list of `(i32, T)` tuples — `[(0, xs[0]), (1, xs[1]), ...]`.  The
  index slot is `i32` to match Python's idiomatic small-index use.
* `zip_str_str` / `zip_i64_i64` pair elements from two lists,
  truncating to the shorter input length (no padding, no error on
  unequal lengths).
* `chain_str` / `chain_i64` return a new list that is `xs ++ ys`.
* `take_str(xs, n)` returns the first `min(n, len(xs))` elements;
  negative `n` is treated as 0.  `drop_str(xs, n)` skips the first
  `min(n, len(xs))` elements and returns the rest.
* `pairwise_str(xs)` returns `[(xs[0], xs[1]), (xs[1], xs[2]), ...]`;
  empty input or single-element input returns the empty list.
* `accumulate_i64([x0, x1, x2, ...])` returns
  `[x0, x0+x1, x0+x1+x2, ...]` — Python's `itertools.accumulate`.
  Empty input returns the empty list.  Overflow wraps (i64 modular
  arithmetic).
* `flatten_str(xs)` concatenates a `List[List[str]]` into a single
  `List[str]`.

Why monomorphic per-type variants (`enumerate_str` vs `enumerate_i64`)?
Stdlib functions aren't generic in v0.2: the M17 generic-fn worklist
only sees user-defined `.spy` functions, not stdlib-registered
NativeFns.  M20b shipped `random.choice_i64/_f64/_str` for the same
reason.  Generic stdlib functions are a v0.3 milestone ("stdlib +
M17 integration").  Functions whose element type doesn't affect the
return shape (`range_step` is always `List[i64]`; `flatten_str` is
always `List[List[str]] -> List[str]`) ship as single non-generic
entries.

What v0.2 does **not** ship:

* Lazy iterators / generators.  `itertools.product`, `combinations`,
  `cycle`, `tee` etc. all require a yielding-iterator type StrictPy
  doesn't have.  v0.3 work.
* `f64` element variants of zip / chain / take / drop / pairwise /
  flatten.  Easy to add — same body, different typecheck signature —
  but not currently needed by any program.  Scope-down for v0.2.
* `group_consecutive` and the `accumulate_str` reducer — both touch
  custom-comparator / reduce machinery that wants closures from
  user code.  Defer to v0.3.

### 9.16 Module `statistics` (v0.2 — M22 P2C)

Descriptive statistics over `List[f64]` and `List[str]`.  Pure-Rust
math — no external crate.

```
fn mean(xs: List[f64]) -> f64
fn median(xs: List[f64]) -> f64
fn stdev(xs: List[f64]) -> f64
fn variance(xs: List[f64]) -> f64
fn min_max(xs: List[f64]) -> Tuple[f64, f64]
fn sum(xs: List[f64]) -> f64
fn quantile(xs: List[f64], q: f64) -> f64
fn mode_str(xs: List[str]) -> str
```

Semantics:

* `mean(xs)` returns the arithmetic mean.  Raises `ValueError` on
  empty input.
* `median(xs)` returns the median (middle value of a sorted copy; for
  even-length input, the average of the two centre values).  Raises
  `ValueError` on empty input.  NaN-tolerant in the sort comparator
  (NaN is treated as the greatest value).
* `variance(xs)` and `stdev(xs)` are the **sample** (Bessel-corrected,
  n-1 denominator) variance and standard deviation.  Both raise
  `ValueError` when `len(xs) < 2`.  v0.2 does not ship population
  variants (`pvariance` / `pstdev`); they're a one-line denominator
  change and slated for v0.3.
* `min_max(xs)` is a single-pass min+max returning a
  `Tuple[f64, f64]`.  Equivalent to `(min(xs), max(xs))` but walks the
  list once.  Raises `ValueError` on empty input.
* `sum(xs)` is the f64 total.  Empty input returns `0.0` (matches
  Python's built-in `sum`).  Naïve left-fold; no Kahan compensation
  (a v0.3 improvement for ill-conditioned inputs).
* `quantile(xs, q)` returns the `q`-quantile by linear interpolation
  between order statistics (Python's `statistics.quantiles` "exclusive"
  method 7).  `q == 0.5` is the median; `q == 0.0` and `q == 1.0` are
  the min and max respectively.  Raises `ValueError` when `q` is NaN
  or outside `[0.0, 1.0]`.
* `mode_str(xs)` returns the most frequent string.  Ties broken by
  first-seen order (the earlier-seen value wins).  Raises
  `ValueError` on empty input.  This is `mode_str` (not `mode`)
  because v0.2 doesn't yet ship the f64 / i64 variants — `mode_i64`
  is straightforward and slated for v0.3.

What v0.2 does **not** ship:

* `pvariance` / `pstdev` (population denominators) and `harmonic_mean`
  / `geometric_mean`.  All single-formula changes; deferred to v0.3.
* `mode_i64` / `mode_f64`.  Same shape as `mode_str` with a
  different HashMap key type; deferred to v0.3 alongside generic
  stdlib.
* `correlation` / `covariance` / `linear_regression`.  Two-input
  statistics; want a richer typed surface and may benefit from a
  matrix/dataset type.  v0.3 or later.
* Online / streaming variants (Welford's algorithm for variance,
  reservoir sampling).  Need mutable accumulator types that v0.2's
  builtin-class machinery doesn't yet expose.

### 9.17 Module `struct` (v0.2 — M22 P2D)

Fixed-width binary pack/unpack for unsigned integers (u32, u64) and
IEEE 754 doubles, in big-endian and little-endian variants.  No Python-
style format strings — every type/endianness pair is an explicit
function name.

```
fn pack_u32_be(value: i64) -> str
fn pack_u32_le(value: i64) -> str
fn pack_u64_be(value: i64) -> str
fn pack_u64_le(value: i64) -> str
fn pack_f64_be(value: f64) -> str
fn pack_f64_le(value: f64) -> str
fn unpack_u32_be(bytes: str, offset: i32) -> i64
fn unpack_u32_le(bytes: str, offset: i32) -> i64
fn unpack_u64_be(bytes: str, offset: i32) -> i64
fn unpack_u64_le(bytes: str, offset: i32) -> i64
fn unpack_f64_be(bytes: str, offset: i32) -> f64
fn unpack_f64_le(bytes: str, offset: i32) -> f64
```

**Binary-as-str encoding.**  StrictPy doesn't ship a runtime `bytes`
type yet (that's v0.3), so `pack` returns a `str` in which each
"byte" is one Unicode codepoint in the range `0..=255`.  Concretely:

* Byte `0xAB` becomes the char `U+00AB`.  The resulting `str`'s
  `len(...)` (char count) equals the byte count: a `pack_u32_be(...)`
  result has `len == 4`.
* The underlying UTF-8 representation is wider — codepoints in
  `128..=255` are 2 UTF-8 bytes each — so the result is NOT a wire-
  format byte buffer.  Two pack results may be concatenated with `+`
  to build longer buffers; `unpack` indexes by codepoint not by
  UTF-8 byte, so the concatenation is correct.
* v0.3 will replace the `str` return type with a real `bytes` runtime
  type once that primitive exists; the function names and semantics
  stay the same.

`unpack` raises `ValueError` if `offset + N > len(bytes)` (short
buffer) or if any codepoint in the read window is outside `0..=255`
(buffer not produced by a `pack`).

What v0.2 does **not** ship:

* `pack_u16_be / le` and matching `unpack_u16_be / le`.  Easy add for
  v0.3 — reserved IDs 348/349 in the M22 P2D range.
* `pack_i32`, `pack_i64`, signed-int unpacks.  Wrap via two's
  complement on the caller side (`pack_u32_be(value & 0xFFFFFFFF)`
  for `i32` round-trips).
* Variable-width format strings (`">If"`).  Python's `struct.pack(">If",
  ...)` style is convenient for protocol declarations but doesn't fit
  the v0.2 fixed-arity native-function discipline.  v0.3 may add a
  parser; for now, compose the per-type calls.

### 9.18 Module `urllib_parse` (v0.2 — M22 P2D)

Percent-encoding and query-string round-tripping for URLs.  The module
name uses `_` rather than `.` because StrictPy doesn't have submodule
support — `urllib.parse` is v0.3, at which point `urllib_parse`
becomes a back-compat alias.

```
fn quote(s: str) -> str
fn quote_plus(s: str) -> str
fn unquote(s: str) -> str
fn unquote_plus(s: str) -> str
fn urlencode(pairs: List[Tuple[str, str]]) -> str
fn parse_query(qs: str) -> List[Tuple[str, str]]
```

Semantics:

* `quote(s)` percent-encodes every byte in `s`'s UTF-8 representation
  *except* the unreserved set `A-Z a-z 0-9 - _ . ~` (RFC 3986
  §2.3).  Spaces become `%20`.  Output uses upper-case hex.
* `quote_plus(s)` is identical to `quote` except ASCII space becomes
  `+` instead of `%20` — the form-encoding flavour used in
  `application/x-www-form-urlencoded` query strings.
* `unquote(s)` decodes `%HH` triples back to bytes, leaves other
  characters untouched.  The resulting byte sequence is interpreted
  as UTF-8; invalid sequences are recovered lossily (Python's
  `errors='replace'` default).
* `unquote_plus(s)` additionally decodes `+` → ASCII space.
* `urlencode(pairs)` builds `k₁=v₁&k₂=v₂&...` from a list of
  `(key, value)` tuples, applying `quote_plus` to each side.
* `parse_query(qs)` is the inverse: split on `&`, then on the first
  `=` per chunk, applying `unquote_plus` to both sides.  A chunk
  with no `=` yields `(chunk, "")`.  Empty input yields the empty list.

Errors: `unquote` / `unquote_plus` / `parse_query` raise `ValueError`
on a malformed `%HH` escape (truncated or non-hex digits).  All other
functions are total.

What v0.2 does **not** ship:

* `parse_url(url) -> Tuple[str, str, str, str, str]` returning
  `(scheme, host, port, path, query)`.  Hand-rolling a robust URL
  parser is genuinely complicated (port handling, userinfo, IPv6
  literals); deferred to v0.3 with reservation against IDs 348/349
  in the M22 P2D block.
* `join_url(base, ref)` for relative-URL resolution.  Needs
  `parse_url` first.
* Per-key collection semantics.  `parse_query("a=1&a=2")` returns
  both `("a", "1")` and `("a", "2")` as separate list entries —
  v0.3 may add a `parse_query_dict` that collapses duplicates by
  picking the last value (Python's default).

### 9.19 Module `base64` (v0.2 — M22 P2B)

Standard (RFC 4648 §4) and URL-safe (RFC 4648 §5) base64 codecs over
`str`.  Backed by the `base64` crate's `Engine` API.

```
fn encode(data: str) -> str             # std alphabet, `=` padded
fn decode(b64: str) -> str              # std alphabet; raises ValueError
fn encode_url_safe(data: str) -> str    # url-safe alphabet, no padding
fn decode_url_safe(b64: str) -> str     # url-safe; raises ValueError
```

Semantics:

* `encode(s)` UTF-8-encodes `s`, then base64-encodes the bytes using
  the standard alphabet (`A-Z`, `a-z`, `0-9`, `+`, `/`) with `=`
  padding.  Output is ASCII.
* `decode(b64)` parses `b64` as standard base64, then UTF-8-decodes
  the result back to `str`.  Raises `ValueError` on malformed base64
  input *and* on a non-UTF-8 payload.  Programs that need to round-trip
  arbitrary bytes (binary files, encrypted blobs) need to wait for the
  v0.3 `bytes` surface.
* `encode_url_safe(s)` uses the URL-safe alphabet (`A-Z`, `a-z`,
  `0-9`, `-`, `_`) with **no padding**.  Output is safe to embed in
  URLs and filenames without further escaping.
* `decode_url_safe(b64)` decodes URL-safe base64.  Strict: it rejects
  the standard alphabet's `+` / `/` characters.

What v0.2 does **not** ship:

* `encode_bytes(data: bytes) -> str` and `decode_bytes(b64: str) -> bytes`.
  StrictPy v0.2's `bytes` is a legacy prelude alias for `str` and has
  no first-class byte-buffer API; a real `bytes` surface is v0.3
  infrastructure.  Until then, programs round-trip text payloads only.
* MIME-style line-wrapped output (76-char lines).  All four entry
  points produce single-line output.  Wrapping is one `re.split` /
  string-builder loop in user code if needed.
* The `b32` (base32) and `b16` (hex) family.  Hex digests are already
  available via `hashlib`; base32 has no shipping example program.

### 9.20 Module `hashlib` (v0.2 — M22 P2B)

Cryptographic + checksum digests.  Backed by the `md-5`, `sha1`,
`sha2`, and `hmac` crates (pure-Rust, RustCrypto family).

```
fn md5(data: str)                       -> str
fn sha1(data: str)                      -> str
fn sha256(data: str)                    -> str
fn sha512(data: str)                    -> str
fn hmac_sha256(key: str, data: str)     -> str
```

Semantics:

* Each entry point UTF-8-encodes its input, runs the named digest,
  and returns the lowercase hex form of the standard length:

  | Function      | Output length |
  |---|---|
  | `md5`         | 32 chars      |
  | `sha1`        | 40 chars      |
  | `sha256`      | 64 chars      |
  | `sha512`      | 128 chars     |
  | `hmac_sha256` | 64 chars      |

  Output matches Python `hashlib.<algo>(data.encode()).hexdigest()`
  and `hmac.new(key.encode(), data.encode(), hashlib.sha256).hexdigest()`
  byte-for-byte.
* `md5` and `sha1` are kept for compatibility with config files,
  legacy hashes, and non-cryptographic checksum use cases.  They are
  **not** suitable for security-sensitive work; use `sha256` /
  `sha512` for that.
* `hmac_sha256(key, data)` accepts a key of any length (the HMAC
  construction handles oversize keys by SHA-256-folding them first).
  The key MUST NOT be empty in practice — pass a non-trivial secret.

What v0.2 does **not** ship (but v0.3 — see §9.20.1 below — does):

* A streaming `update()` API.  See §9.20.1 — M35 ships the typed
  `Hasher` class with `update` / `hexdigest` / `algorithm` methods,
  layered on top of the same RustCrypto crates as the one-shot
  helpers.
* SHA-3 (Keccak), BLAKE2/BLAKE3, RIPEMD.  Adding each is one crate +
  one handler; held back until a real-world example program asks.
* `pbkdf2_hmac` / `scrypt` / `argon2`.  Password-hashing primitives
  are deferred to v0.4.

#### 9.20.1 Streaming `Hasher` class (v0.3 — M35 P4-C)

The streaming counterpart to the one-shot digests in §9.20.  Use this
when the input arrives in pieces (file chunks, log lines, streamed
uploads, etc.) — feeding the whole input to `hashlib.sha256` would
require materialising it as one string.

```
final class Hasher:
    fn update(self, data: str) -> None
    fn hexdigest(self) -> str
    fn algorithm(self) -> str

fn hashlib.new(algorithm: str) -> Hasher
```

* `hashlib.new(algorithm)` allocates a fresh `Hasher` of the named
  algorithm.  Supported names: `"sha256"`, `"sha512"`, `"sha1"`,
  `"md5"`.  Any other name raises `ValueError`.
* `Hasher.update(data)` feeds `data` into the in-progress hash.
  `data` is treated as a byte buffer — each codepoint 0..=255
  contributes one byte (the StrictPy str-as-byte-buffer convention,
  matching the M22 `struct` / M27 `gzip` handlers).  For ASCII input
  (the overwhelming common case) this is byte-identical to the UTF-8
  encoding used by `hashlib.sha256(data)`.
* `Hasher.hexdigest()` returns the lowercase hex digest of the
  in-progress state.  Calling `hexdigest()` does NOT invalidate the
  Hasher — the underlying state is cloned before finalising, so the
  user can call `hexdigest()` multiple times for intermediate
  digests AND continue calling `update()` afterwards.  This is the
  **clone-not-consume** policy: friendlier than CPython's slightly
  ambiguous "you can call hexdigest multiple times but the digest is
  final after the next update" wording.

  Concretely:

  ```
  h: Hasher = hashlib.new("sha256")
  h.update("hello")
  d1: str = h.hexdigest()      # sha256("hello")
  d1_again: str = h.hexdigest()  # same as d1
  h.update(", world")
  d2: str = h.hexdigest()      # sha256("hello, world")
  ```
* `Hasher.algorithm()` returns the canonical algorithm name passed
  to `hashlib.new` (one of the four strings above).  Useful for
  generic code that handles a Hasher without knowing which algorithm
  it was created for.

The streaming digest matches the one-shot digest of the
concatenation byte-for-byte:

```
let h: Hasher = hashlib.new("sha256")
h.update(chunk_a); h.update(chunk_b); h.update(chunk_c)
h.hexdigest() == hashlib.sha256(chunk_a + chunk_b + chunk_c)  # true
```

Implementation:

* The `Hasher` class is registered in the resolver prelude (same
  table as `io.File`, `Channel`, `Thread`); `final`, handle-backed,
  with `is_native: true` and zero declared fields.  The heap object
  is a private `HasherRepr` carrying an `i64` slot handle.
* `SharedVm.hashers` is a `HashMap<i64, HasherSlot>` where each slot
  owns one of `sha2::Sha256` / `sha2::Sha512` / `sha1::Sha1` /
  `md5::Md5` plus the algorithm-name string.  `next_hasher_id` is a
  monotonic `AtomicI64` starting at 1.
* Method dispatch routes through the M34 class-by-name table in
  `ir::lower_method_call`, so the method names `update` / `hexdigest`
  / `algorithm` don't have to compete with any future class using
  the same names via `NativeFn::from_name`.

What v0.3 does **not** ship for `Hasher`:

* `Hasher.copy()` — explicit branch-the-state operation.  Trivial
  given clone-not-consume already does the underlying work; held
  for v0.4 once a real-world program asks.
* `Hasher.digest_size: i64` constant.  Add when needed.
* SHA-3 / BLAKE2 / BLAKE3 algorithm names.  One crate + one
  `HasherState` variant + one `hashlib.new` arm each — pure
  additive work; held back until justified by a shipping demo.

### 9.21 Module `argparse` (v0.2 — M22 P2A)

Declarative CLI argument parsing.  Builder-style API for the common
"flag + positional + option" shape.  Replaces the hand-walked
`sys.argv` parsing in pre-M22 example programs.

```
fn new(prog: str) -> Dict[str, str]
fn add_flag(p: Dict[str, str], name: str, default: bool) -> None
fn add_arg(p: Dict[str, str], name: str) -> None
fn add_opt(p: Dict[str, str], name: str, default: str) -> None
fn parse(p: Dict[str, str], argv: List[str]) -> Dict[str, str]
fn get_flag(a: Dict[str, str], name: str) -> bool
fn get_arg(a: Dict[str, str], name: str) -> str
fn get_opt(a: Dict[str, str], name: str) -> str
fn help_text(p: Dict[str, str]) -> str
fn help_requested(argv: List[str]) -> bool
```

Semantics:

* `new(prog)` returns an opaque parser handle, internally a
  `Dict[str, str]`.  v0.2 lacks stdlib-class registration (deferred
  to v0.3), so the dict-of-strings shim stands in for a typed
  `ArgParser`.
* `add_flag(p, name, default)` registers a boolean flag.  Both
  `--verbose` and short forms (`-v`) are accepted at parse time.
* `add_arg(p, name)` registers a required positional in declaration
  order.  No default values for positionals in v0.2.
* `add_opt(p, name, default)` registers `--key VALUE`.  Both
  `--key value` and `--key=value` parse correctly.
* `parse(p, argv)` walks `argv[1..]` (skipping the `.spyc` path at
  index 0). Returns the populated args dict.  Raises `ValueError`
  on unknown flag/option, option-without-value, missing required
  positional, or unexpected positional.
* `get_flag` / `get_arg` / `get_opt` look up values by name, returning
  the registered default if absent.
* `help_text(p)` returns the multi-line `usage: <prog> [options]
  <positionals>` block plus per-option lines.
* `help_requested(argv)` is true iff `argv` contains `-h` or `--help`.
  Idiomatic use: check before `parse`, print `help_text(p)`, then
  `sys.exit(0)`.

What v0.2 does **not** ship: typed `ArgParser`/`Args` sealed classes
(v0.3 stdlib-class work); subparsers/subcommands; type coercion
(`type=int`); variadic positionals; mutually-exclusive groups;
per-arg help strings.

### 9.22 Module `collections` (v0.2 — M22 P2A)

Counter (multiset) and deque (double-ended queue), built on the M7
`Dict[K, V]` and `List[T]` heap types.  Both are typed aliases:
Counter is `Dict[str, i64]`, Deque is `List[i64]`.

```
# Counter — multiset over str keys.
fn counter_new() -> Dict[str, i64]
fn counter_increment(c: Dict[str, i64], key: str) -> None
fn counter_add(c: Dict[str, i64], key: str, n: i64) -> None
fn counter_get(c: Dict[str, i64], key: str) -> i64
fn counter_top_keys(c: Dict[str, i64], n: i32) -> List[str]

# Deque — double-ended queue over i64.
fn deque_new() -> List[i64]
fn deque_push_back(d: List[i64], v: i64) -> None
fn deque_pop_front(d: List[i64]) -> i64
fn deque_len(d: List[i64]) -> i32
fn deque_is_empty(d: List[i64]) -> bool
```

Semantics:

* `counter_get` returns `0` for absent keys (Python `Counter.get`
  parity).
* `counter_top_keys(c, n)` returns up to `n` keys with the highest
  counts, descending; alphabetical tie-break.  `n <= 0` returns the
  empty list.
* `deque_pop_front` raises `IndexError` on empty.  v0.2 uses an O(n)
  shift over the underlying `List[i64]`; a real ring-buffer deque is
  v0.3 work.

What v0.2 does **not** ship: generic `Counter[K]` / `Deque[T]`
(deferred to v0.3 generic-class story); `Counter.subtract` /
`update` / `elements`; `defaultdict` (use `counter_get`'s "default 0"
or wrap a `Dict` manually).

### 9.23 Module `csv` (v0.2 — M22 P2A)

RFC 4180-ish CSV parser and writer.  Quoting rules match Python's
`csv` module with the default dialect (`,` separator, `"` quote
char, `\n` row terminator).

```
fn parse_line(line: str) -> List[str]
fn parse(text: str) -> List[List[str]]
fn read_file(path: str) -> List[List[str]]
fn write_file(path: str, rows: List[List[str]]) -> None
fn escape(field: str) -> str
fn format_row(row: List[str]) -> str
```

Semantics:

* `parse_line` parses one CSV line; the input must not contain a
  trailing newline.  Embedded newlines inside quoted fields are
  not honoured — use `parse` for that.
* `parse` parses multi-line CSV.  Quoted fields may span multiple
  lines.  Recognises both `\n` and `\r\n` as row separators outside
  quotes.
* `read_file(path)` reads UTF-8 text and runs `parse`.  Raises
  `IOError` on filesystem errors.
* `write_file(path, rows)` writes CSV with a single trailing `\n`
  after the last row.  Raises `IOError` on filesystem errors.
* `escape(s)` quotes `s` (doubling internal `"`) iff it contains
  `,`, `"`, `\n`, or `\r`; otherwise returns `s` unchanged.
* `format_row(row)` joins fields with `,` after per-field `escape`.

Quoting (Python default dialect): fields starting with `"` are
quoted (opening quote stripped); `""` inside a quoted field is a
literal `"`; unquoted fields cannot contain `,`, `\n`, or `\r`.

What v0.2 does **not** ship: dialect configuration; `DictReader` /
`DictWriter` (build on top of `parse` + index lookups); streaming
reading (`parse` buffers the whole file); `QUOTE_ALL` /
`QUOTE_NONNUMERIC` policies.

### 9.24 Module `subprocess` (v0.2 — M23 P3a-A)

Cross-platform OS process spawn, capture, and lifecycle control,
backed by Rust's `std::process::Command`.  Spawned processes are
exposed to user code as opaque `i64` handles into a VM-owned process
registry — the same shape M5 uses for `io.File` handles, because
stdlib classes are still v0.3 work (same blocker that punted the
typed `JsonValue` in M20c and `ArgParser` in M22 P2A).

```
fn run(prog: str, args: List[str]) -> Tuple[i32, str, str]
fn run_with_stdin(prog: str, args: List[str], stdin_data: str)
        -> Tuple[i32, str, str]
fn spawn(prog: str, args: List[str]) -> i64
fn wait(handle: i64) -> i32
fn try_wait(handle: i64) -> i32?
fn kill(handle: i64) -> None
```

(Continued semantics below; datetime is renumbered as §9.26.)

### 9.26 Module `datetime` (v0.2 — M23 P3a-B)

Calendar arithmetic + ISO 8601 parse/format, layered on top of the
M20b `time` module's epoch primitives.  Two value shapes — both
represented as plain `i64` in v0.2 (no stdlib classes yet):

* **`DateTime`** — a moment in time, as unix epoch seconds (UTC).
  Negative values denote pre-1970 moments.  All arithmetic is
  integer; fractional seconds are out of scope (use `time.now()`'s
  f64 form for sub-second precision).
* **`Duration`** — a span in seconds.  Same `i64` type — a
  `Duration` is just the difference of two `DateTime` values.

```
fn now() -> i64                                  # unix epoch seconds (UTC)
fn from_unix(secs: i64) -> i64                   # identity / type-assertion
fn from_ymd(year: i32, month: i32, day: i32) -> i64
fn from_ymd_hms(year, month, day, hour, minute, second: i32) -> i64

fn year(dt: i64) -> i32
fn month(dt: i64) -> i32                         # 1..=12
fn day(dt: i64) -> i32                           # 1..=31
fn hour(dt: i64) -> i32                          # 0..=23
fn minute(dt: i64) -> i32                        # 0..=59
fn second(dt: i64) -> i32                        # 0..=60 (leap sec allowed)
fn weekday(dt: i64) -> i32                       # 0..=6, Monday=0 (ISO)
fn ymd(dt: i64) -> Tuple[i32, i32, i32]          # (year, month, day)

fn add_seconds(dt: i64, secs: i64) -> i64
fn add_days(dt: i64, days: i64) -> i64
fn diff_seconds(a: i64, b: i64) -> i64           # a - b
fn diff_days(a: i64, b: i64) -> i64              # floor((a-b)/86400)

fn to_iso(dt: i64) -> str                        # "YYYY-MM-DDTHH:MM:SSZ"
fn to_date_str(dt: i64) -> str                   # "YYYY-MM-DD"
fn to_time_str(dt: i64) -> str                   # "HH:MM:SS"
fn from_iso(s: str) -> i64                       # parse ISO 8601
fn from_date_str(s: str) -> i64                  # "YYYY-MM-DD" → UTC midnight

fn local_offset_minutes() -> i32                 # process-local TZ offset
```

Semantics (subprocess):

* `run(prog, args)` spawns `prog args...`, blocks until the child
  exits, captures both stdout and stderr, and returns the 3-tuple
  `(exit_code, stdout, stderr)`.  `prog` is searched on `PATH` unless
  it's an absolute path.  Raises `IOError` if the spawn itself fails
  (executable not found, permission denied, etc.).
* `run_with_stdin(prog, args, stdin_data)` is identical to `run`
  except `stdin_data` is written to the child's stdin (and the pipe
  closed) before waiting.  Convenient for filter-style children
  (`sort`, `wc`, `grep`).
* `spawn(prog, args)` starts the process *without* waiting and
  returns an opaque `i64` handle.  Stdin/stdout/stderr are inherited
  from the parent — no piping.  Raises `IOError` on spawn failure.
* `wait(handle)` blocks until the spawned child exits; returns its
  exit code.  Raises `IOError` if the handle is invalid or already
  waited.
* `try_wait(handle)` is a non-blocking poll: returns the exit code if
  the child has exited, `none` if it's still running.  Raises
  `IOError` on invalid handle.
* `kill(handle)` force-terminates the child (SIGKILL on Unix,
  TerminateProcess on Windows).  Silently succeeds on an
  already-exited child (matches Python's `Popen.kill`).

**Exit-code encoding.**  On Unix, a child terminated by a signal has
no integer exit code; we follow Python's `subprocess.run` convention
and report `-signal_number` (negative).  `wait`/`run` always return an
`i32` so user code can branch on `code < 0`.

**Cross-platform notes:**

* On Windows `echo`, `dir`, `type`, etc. are shell builtins, not real
  executables.  `subprocess.run("echo", ["hi"])` will fail with
  "program not found".  Wrap shell commands as `cmd.exe /c <cmd>`
  (Windows) or `sh -c <cmd>` (Unix); use `sys.platform` to dispatch.
* Argument quoting differs (Windows applies CommandLineToArgvW; Unix
  preserves args verbatim).  Rust's `std::process::Command::args`
  does the host-appropriate thing — pass arguments as separate
  `List[str]` elements, not pre-joined.

What v0.2 does **not** ship:

* **Streaming stdin/stdout/stderr** (`Popen.stdout.read(...)`).  Would
  need readable byte-stream handles, which v0.3 will introduce
  alongside the `bytes` runtime type.
* **Environment-variable injection** (`subprocess.run(..., env=...)`).
  Use `os.set_env` in the parent (it inherits to children) for v0.2.
* **`shell=True` form** taking a single command string.  Spell it
  explicitly as `subprocess.run("sh", ["-c", cmd])` or the cmd.exe
  equivalent.
* **`check=True` raise-on-nonzero**.  User code that wants this can
  inspect `r.0` and raise its own `RuntimeError`.

### 9.25 Module `pathlib` (v0.2 — M23 P3a-A)

Object-oriented path manipulation shipped as a *flat-function* API
over `str`-typed paths.  The Pythonic `Path("a") / "b"` chaining
isn't expressible in v0.2 because stdlib classes don't yet have a
registration path (deferred to v0.3 alongside `JsonValue`, `ArgParser`,
and `Counter[K, V]` from M20c / M22).  Functions consume and produce
`str` so they compose freely with the M20a `os` and `path` modules.

```
fn join(a: str, b: str) -> str
fn with_suffix(p: str, new_suffix: str) -> str
fn with_name(p: str, new_name: str) -> str
fn parent(p: str) -> str
fn name(p: str) -> str
fn stem(p: str) -> str
fn suffix(p: str) -> str
fn parts(p: str) -> List[str]
fn is_absolute(p: str) -> bool
fn absolute(p: str) -> str
fn relative_to(p: str, base: str) -> str
fn read_text(p: str) -> str
fn write_text(p: str, content: str) -> None
fn read_lines(p: str) -> List[str]
```

Semantics:

* `join(a, b)` concatenates `a` and `b` using the OS-native separator
  (`/` on Unix, `\` on Windows).  Alias of `path.join` — duplicated
  under `pathlib` for namespace coherence.
* `with_suffix(p, ".x")` replaces the last extension.  Names with no
  extension get the suffix appended (`with_suffix("README", ".md")`
  → `"README.md"`).  A leading dot is not an extension
  (`with_suffix(".bashrc", ".tmp")` → `".bashrc.tmp"`).
* `with_name(p, "x")` replaces the basename: `with_name("a/b/c.txt",
  "new.csv")` → `"a/b/new.csv"`.
* `parent(p)` / `name(p)` are aliases over `path.dirname` /
  `path.basename`.
* `stem(p)` is the basename minus the LAST extension.  Python's
  pathlib convention: `"a.txt"` → `"a"`; `"archive.tar.gz"` →
  `"archive.tar"`; `".bashrc"` → `".bashrc"`.
* `suffix(p)` is the last extension including the leading dot.
  `"a.txt"` → `".txt"`; `"README"` → `""`.
* `parts(p)` splits the path into components via
  `std::path::Path::components`.  `"a/b/c"` → `["a", "b", "c"]`.
  Drive prefixes and root markers are emitted verbatim on each
  platform.
* `is_absolute(p)` is the cross-platform absolute-path test.
* `absolute(p)` makes `p` absolute relative to the current working
  directory.  Does NOT resolve symlinks (that's `os.realpath`
  territory, deferred to v0.3).  Raises `IOError` if the current
  directory can't be queried.
* `relative_to(p, base)` strips the `base` prefix from `p`.  Raises
  `ValueError` if `p` is not a sub-path of `base`.
* `read_text(p)` returns the entire file as a UTF-8 string.  Raises
  `IOError`.
* `write_text(p, content)` writes via `std::fs::write` (truncating).
  Raises `IOError`.
* `read_lines(p)` reads + splits on `\n`.  A trailing newline is
  stripped (so `"a\nb\n"` → `["a", "b"]`).  CRLF line endings are
  normalised — the trailing `\r` of each `\n`-split chunk is removed.
  Raises `IOError`.

What v0.2 does **not** ship:

* **A real `Path` class** with operator overloads (`/`) and chained
  methods.  Needs stdlib-class registration (v0.3).
* **`glob` / `iterdir` / `match`**.  Python's `pathlib` includes
  these but they overlap with `os.listdir` + the `re` module already
  in v0.2.  v0.3 may add a thin layer.
* **`unlink` / `mkdir` / `rmdir`**.  Use `os.remove`, `os.mkdir`
  from M20a directly; we deliberately don't duplicate the FS-mutation
  surface across modules.
* **`exists` / `is_file` / `is_dir`**.  Same reasoning — already
  available under `os`.
* **`symlink_to` / `readlink` / `resolve` (symlink-following
  `realpath`)**.  Deferred to v0.3; `absolute` is the lexical-only
  alternative.

Semantics (datetime):

* All `DateTime` values are UTC.  The interpretation as a civil date
  uses the proleptic Gregorian calendar via Howard Hinnant's
  public-domain `civil_from_days` / `days_from_civil` algorithms
  (the same code path M20b uses for `time.format_iso`).
* `from_ymd` and `from_ymd_hms` validate every field and raise
  `ValueError` on out-of-range inputs (year not in `-10000..=10000`;
  month not in `1..=12`; day not valid for the given month including
  leap-year rules; hour `0..=23`; minute `0..=59`; second `0..=60`).
* `weekday(dt)` returns ISO weekday — Monday is 0, Sunday is 6.  The
  calculation uses `div_euclid`, so pre-1970 dates produce the right
  weekday (e.g. 1969-12-31 → 2 for Wednesday).
* `add_seconds` and `diff_seconds` use wrapping arithmetic; programs
  passing pathological i64s get the silent wrap rather than a panic.
  `add_days` saturates the seconds multiply at i64 range.
* `diff_days(a, b)` uses floor division (Python's `//`), so a span
  of `-12h` reports `-1` day (not `0`).
* `to_iso` produces `"YYYY-MM-DDTHH:MM:SSZ"` (no fractional seconds,
  always Zulu).  `from_iso` accepts that exact form plus
  `"YYYY-MM-DDTHH:MM:SS+HH:MM"`, `"YYYY-MM-DDTHH:MM:SS-HH:MM"`,
  `"YYYY-MM-DDTHH:MM:SS+HHMM"`, `"YYYY-MM-DDTHH:MM:SS"` (naive —
  treated as UTC), the date-only form `"YYYY-MM-DD"`, and the
  space-separated variant `"YYYY-MM-DD HH:MM:SS"`.  Malformed input
  raises `ValueError`.
* `local_offset_minutes()` captures the current process-local
  timezone offset from UTC in minutes (e.g. `-480` for PST, `0` for
  UTC).  Implementation is platform-specific FFI — no `chrono` or
  `libc` crate dep.  On Windows it calls `GetTimeZoneInformation`;
  on Unix it calls `localtime_r`.  On unsupported platforms it
  falls back to `0` (UTC).  The value reflects "the offset right
  now", so DST transitions during the program's lifetime are not
  tracked retroactively.

What v0.2 does **not** ship: named timezones (`"America/New_York"`
— would need tzdata); fractional seconds (`DateTime` is integer; a
v0.3 widening to `i64 ns` is the obvious upgrade); `strftime` /
`strptime` format strings (use the fixed ISO format); historical
timezone transitions; the `datetime.timedelta` class shape (use
arithmetic on i64 seconds).

### 9.27 Module `threading` — Lock + Semaphore (v0.2 — M23 P3a-C)

Extends the M6 `Thread` / `Channel` runtime classes with the missing
synchronisation primitives. Both `Lock` and `Semaphore` are opaque
`i64` handles into per-process slot tables on `SharedVm`; the C-level
storage uses `std::sync::Mutex` + `std::sync::Condvar`.

```
fn lock_new() -> i64
fn lock_acquire(handle: i64) -> None
fn lock_release(handle: i64) -> None
fn lock_try_acquire(handle: i64) -> bool

fn semaphore_new(initial: i32) -> i64
fn semaphore_acquire(handle: i64) -> None
fn semaphore_release(handle: i64) -> None
fn semaphore_try_acquire(handle: i64) -> bool
```

`threading.Lock` semantics match Python's (not `RLock`): acquiring a
held lock from the same thread DEADLOCKS. `lock_release` on a not-held
lock raises `RuntimeError`. `lock_try_acquire` is non-blocking.

`Semaphore` is counting: `semaphore_new(n)` allocates with N permits;
acquire blocks when count is 0; release wakes one blocked acquirer.

Concurrency-safe across M6 threads. Per-table mutex is dropped before
parking on the condvar.

What v0.2 doesn't ship: `RLock`, `Event`, `Condition`, `Barrier`,
timed `acquire(timeout=...)`, `with`-statement sugar. v0.3.

### 9.28 Module `queue` — PriorityQueue (v0.2 — M23 P3a-C)

Min-priority queue with `f64` priorities. Items pinned to i64 or str
per v0.2's lack of generic stdlib functions.

```
fn pq_new_i64() -> i64
fn pq_push_i64(handle: i64, priority: f64, item: i64) -> None
fn pq_pop_min_i64(handle: i64) -> Tuple[f64, i64]
fn pq_peek_min_i64(handle: i64) -> Tuple[f64, i64]

fn pq_new_str() -> i64
fn pq_push_str(handle: i64, priority: f64, item: str) -> None
fn pq_pop_min_str(handle: i64) -> Tuple[f64, str]
fn pq_peek_min_str(handle: i64) -> Tuple[f64, str]

fn pq_len(handle: i64) -> i32
fn pq_is_empty(handle: i64) -> bool
```

Semantics: `pq_pop_min_*` pops the lowest-priority entry as
`(priority, item)`. Raises `IndexError` on empty. Ties at equal
priority break FIFO. NaN priorities sort as larger than any real
number. Backed by `BinaryHeap<Reverse<...>>`.

v0.2 doesn't ship: `pq_clear`, `pq_drain`, bounded queues, blocking
push, decrease-key. v0.3.

#### 9.28.1 Plain FIFO `Queue` (v0.3 — stdlib expansion)

A first-in-first-out queue alongside the priority queue above. Same
monomorphic `_i64` / `_str` split (stdlib functions aren't generic);
`fifo_empty` / `fifo_qsize` are type-erased over the handle.

```
fn fifo_new_i64() -> i64
fn fifo_put_i64(handle: i64, item: i64) -> None
fn fifo_get_i64(handle: i64) -> i64

fn fifo_new_str() -> i64
fn fifo_put_str(handle: i64, item: str) -> None
fn fifo_get_str(handle: i64) -> str

fn fifo_empty(handle: i64) -> bool
fn fifo_qsize(handle: i64) -> i64
```

Semantics: `fifo_get_*` removes and returns the oldest enqueued item;
items come out in exactly insertion order. Raises `IndexError` on an
empty queue. Backed by a `VecDeque` behind an i64 handle. See
`examples/queue_fifo_demo.spy`.

### 9.29 Module `sqlite3` (v0.2 — M23 P3a-D)

SQLite via the `rusqlite` crate, statically linked through the
`bundled` feature so libsqlite3.c ships inside the VM binary — there
is no dependency on a system SQLite install on any platform.

Connections are modelled as `i64` handles into a per-process slot
table on `SharedVm`. User code receives a handle from `connect`,
passes it through every other API call, and releases it via `close`.

```
fn connect(path: str) -> i64
fn close(conn: i64) -> None
fn execute(conn: i64, sql: str) -> None
fn execute_params(conn: i64, sql: str, params: List[str]) -> None
fn query(conn: i64, sql: str) -> List[List[str]]
fn query_params(conn: i64, sql: str, params: List[str]) -> List[List[str]]
fn last_insert_rowid(conn: i64) -> i64
fn changes(conn: i64) -> i32
fn column_names(conn: i64, sql: str) -> List[str]
```

Semantics:

* `connect(path)` opens or creates the database file.  The special
  path `":memory:"` opens an ephemeral, in-process database (matching
  the underlying SQLite semantics).  Raises `IOError` on filesystem
  / permission failure.
* `close(conn)` releases the underlying connection.  Calling `close`
  on an already-closed (or zero) handle is a no-op — mirrors Python's
  `Connection.close()`.
* `execute(conn, sql)` runs a no-row statement (`CREATE`, `INSERT`,
  `UPDATE`, `DELETE`, `BEGIN`, `COMMIT`, etc.).  Use raw `BEGIN` /
  `COMMIT` / `ROLLBACK` SQL for transactions in v0.2; there's no
  separate transaction handle.
* `execute_params(conn, sql, params)` runs a no-row statement with
  `?` placeholders bound from `params`.  Each parameter is bound as
  `TEXT` — SQLite's normal type-coercion rules apply when the bound
  value is compared against an INTEGER / REAL column.  Parameter
  binding is the SQL-injection-safe path: a value of `"'; drop table
  notes;--"` is stored as literal text, never parsed as SQL.
* `query(conn, sql)` and `query_params(conn, sql, params)` run a
  row-returning statement and materialise the whole result set as a
  `List[List[str]]`.  Every cell is stringified by type:
    * `INTEGER` → decimal text (`42` → `"42"`)
    * `REAL`    → `format!("{}", f64)` (`3.5` → `"3.5"`)
    * `TEXT`    → the text as-is
    * `NULL`    → the empty string `""`
    * `BLOB`    → lowercase hex of the bytes
  This stringified-result simplification covers ~every config-store
  and cache use case; programs that genuinely need typed cells (BLOBs
  in particular) wait for v0.3's `bytes` type.
* `last_insert_rowid(conn)` returns the rowid of the most recent
  successful INSERT on this connection (per SQLite's
  `sqlite3_last_insert_rowid()` semantics).
* `changes(conn)` returns the number of rows affected by the most
  recent INSERT / UPDATE / DELETE.  Returns `0` if no row-modifying
  statement has run.
* `column_names(conn, sql)` prepares the SQL but does not iterate
  rows; returns just the result-set column names.  Useful for
  schema discovery against `SELECT *`.

Errors:

* Invalid or closed connection handle → `ValueError`.
* SQL prepare / execute / row-fetch failure → `ValueError` with the
  underlying SQLite message in the body.
* `connect` filesystem failures → `IOError`.

What v0.2 does **not** ship: prepared-statement caching (each call
re-prepares); typed result rows (everything is `str` — see the
stringification rules above); a `Connection`/`Cursor` class surface
(needs stdlib-class registration, deferred to v0.3); explicit
transaction or savepoint handles (use raw `BEGIN`/`COMMIT`/`ROLLBACK`
SQL); `executemany`; row iterators (the whole result set comes back
as `List[List[str]]`); BLOB streaming; user-defined functions / hooks
(would need closure-across-NativeFn-boundary support, deferred);
parameter binding to non-`str` types directly (workaround: format the
value into a `str` first — SQLite coerces back to INTEGER / REAL on
the column side).

Concurrency: connections live in `SharedVm.sqlite_connections` behind
a mutex.  The native-handler glue briefly locks the table to look up
the connection, takes it out, drops the table lock, runs the SQL,
and puts the connection back.  Sibling `connect` / `close` calls on
other threads can therefore run in parallel with a long-running
query.  Re-entrant use of the *same* connection handle inside a
single SQL call's lifetime (e.g. a callback that queries again on the
same conn) is a runtime error ("connection in use") rather than a
deadlock — but v0.2 has no callback / hook surface so user code can't
hit this path organically.

#### 9.29.1 `Connection` and `Cursor` classes (v0.3 — M35 P4-B)

The flat handle-passing surface above remains the underlying primitive
— the M29 web framework and existing demos use it unchanged — but
v0.3 adds two typed classes that give method-call ergonomics for new
code:

```
final class Connection:
    handle: i64                              # slot index — internal
    fn execute(self, sql: str) -> None
    fn execute_params(self, sql: str, params: List[str]) -> None
    fn query(self, sql: str) -> Cursor
    fn query_params(self, sql: str, params: List[str]) -> Cursor
    fn last_insert_rowid(self) -> i64
    fn changes(self) -> i32
    fn close(self) -> None                   # idempotent

final class Cursor:
    handle: i64                              # slot index — internal
    fn fetchone(self) -> List[str]?          # next row, or none if exhausted
    fn fetchall(self) -> List[List[str]]     # remaining rows
    fn column_names(self) -> List[str]
    fn row_count(self) -> i64                # total rows from the query
```

Entry point:

```
fn open(path: str) -> Connection
```

Named `open` rather than `connect` because the flat-function `connect`
is already a top-level name in this module.  Semantics match the flat
`connect`: opens or creates the DB file, raises `IOError` on
filesystem / permission failure.

Method semantics:

* `Connection.execute` / `execute_params` / `last_insert_rowid` /
  `changes` / `close` are direct wrappers over the matching flat
  functions — the i64 stored in the `handle: i64` field at payload
  offset 0 is threaded through to the same M23 P3a-D logic.
* `Connection.query` / `query_params` eagerly materialise the result
  set into a `CursorState` on `SharedVm.sqlite_cursors` and return a
  fresh `Cursor` instance whose `handle: i64` field points at the new
  slot.  Each query produces an independent cursor; multiple cursors
  over the same connection coexist.
* `Cursor.fetchone()` reads the next row from the cursor's row buffer
  and advances the iteration index.  Returns the row as `List[str]`
  if available, or `none` when exhausted.  Subsequent calls after
  exhaustion continue to return `none`.
* `Cursor.fetchall()` returns the *remaining* rows (not all rows from
  the original query) and advances the iteration index to the end —
  so a `fetchone` followed by `fetchall` yields disjoint sets, and a
  second `fetchall` after the first is the empty list.
* `Cursor.column_names()` and `row_count()` are independent of the
  iteration cursor; they return the column-name list reported by the
  prepared statement and the total row count from the original query.
* Cell stringification is identical to the flat surface (`INTEGER` →
  decimal text, `REAL` → `format!("{}", f64)`, `TEXT` → as-is,
  `NULL` → `""`, `BLOB` → lowercase hex).
* Cursors outlive their parent `Connection`: closing the connection
  doesn't invalidate cursors that were created from it (every row was
  copied out at query time), so an `INSERT`/`query`/`close` sequence
  is fine to follow with cursor iteration.

Backwards compatibility: nothing on the flat surface changes.  The
two surfaces share `SharedVm.sqlite_connections` so a `Connection`
opened via `sqlite3.open` cannot be passed to a flat-function call
(the i64 handle is encapsulated in the class instance), and vice
versa — but this is by design, not a limitation.

What's still deferred to v0.4:

* Cursor iteration via `for row in cur:` — needs the iterator-protocol
  hook on stdlib classes.  v0.3 users compose `fetchone` in a
  `while true:` loop, see `examples/sqlite_class_demo.spy`.
* Explicit transaction methods (`Connection.commit` / `rollback`) —
  v0.3 users still run raw `BEGIN`/`COMMIT`/`ROLLBACK` SQL through
  `execute`.
* Typed parameter binding (bind non-`str` values directly without
  formatting them first) — same constraint as the flat surface.

NativeFn IDs 800-819 are reserved for this block (800: `ConnectionCtor`,
801: `Sqlite3OpenTyped`, 802-808: Connection methods, 811: `CursorCtor`,
812-815: Cursor methods, 809-810 + 816-819 reserved for v0.4 follow-ups).

### 9.39 Module `logging` (v0.2 — M27 P3c-E)

Application logging.  v0.2 ships a **single global logger** — threshold,
optional file sink, and a fixed record format.  Python's class-heavy
`Logger` / `Handler` / `Formatter` hierarchy (and named loggers
addressable by dotted module path) depend on stdlib-class registration
and are v0.3 work.

```
fn basic_config(level: str) -> None
fn basic_config_to_file(level: str, filename: str) -> None
fn set_level(level: str) -> None
fn get_level() -> str

fn debug(msg: str) -> None
fn info(msg: str) -> None
fn warning(msg: str) -> None
fn error(msg: str) -> None
fn critical(msg: str) -> None
fn log(level: str, msg: str) -> None

fn is_enabled_for(level: str) -> bool
```

Level names: `"DEBUG"`, `"INFO"`, `"WARNING"`, `"ERROR"`, `"CRITICAL"`.
The aliases `"WARN"` (== WARNING) and `"FATAL"` (== CRITICAL) are also
accepted for CPython compatibility.  Comparison is case-insensitive
(`"info"` and `"INFO"` both resolve to level 20).  Unknown level names
raise `ValueError`.

The numeric levels match CPython exactly so the surface is
interchangeable: `DEBUG = 10`, `INFO = 20`, `WARNING = 30`, `ERROR = 40`,
`CRITICAL = 50`.

**Format** — fixed, matching CPython's default
`%(asctime)s %(levelname)s %(message)s`:

```
2026-05-20T13:42:55Z INFO Some message here
```

Timestamps are UTC, integer-seconds (no fractional component), formatted
with the same Howard Hinnant `civil_from_days` algorithm used by
`time.format_iso` and `datetime.to_iso`.  Each log record ends with a
single trailing `\n`.

Semantics:

* `basic_config(level)` initialises the global logger to write to
  process stderr.  Idempotent — calling it again resets the threshold
  and drops any prior file sink.
* `basic_config_to_file(level, filename)` opens `filename` in
  create-or-append mode and directs all subsequent emits there.
  Raises `IOError` on open failure.  Idempotent — calling either
  `basic_config` variant again replaces the sink.
* `set_level(level)` adjusts the threshold without touching the sink.
* `get_level()` returns the current level's canonical upper-case name.
  After process start (before any `basic_config`), the default is
  `"WARNING"` — matches CPython's `logging.WARNING` pre-`basicConfig`
  default.
* `debug` / `info` / `warning` / `error` / `critical` emit at the
  corresponding level.  Each is a one-line shortcut for
  `log("LEVEL", msg)`.  Messages below the current threshold are
  silently dropped — no formatting cost beyond the level comparison.
* `log(level, msg)` is the generic emit; useful when the level is
  dynamic (computed from a config string).
* `is_enabled_for(level)` returns `true` iff emitting at `level` would
  produce output.  Use it to gate expensive message-building:

    ```python
    if logging.is_enabled_for("DEBUG"):
        logging.debug(f"big: {expensive_thing()}")
    ```

  Without the gate the `f"..."` interpolation (and `expensive_thing()`)
  runs even when the message is dropped — exactly the same pattern as
  Python's `logger.isEnabledFor(logging.DEBUG)`.

Thread safety: emits on the file path acquire the per-instance
`Mutex<Option<File>>` only for the single `write_all` of the formatted
record, so concurrent log lines never interleave bytes within a record.
Stderr emits skip the mutex — the OS-level stderr is line-atomic for
the short writes we produce — but two concurrent stderr emits can
appear in either order in the output.

Implementation: state lives on `SharedVm` (`log_level: AtomicI32`,
`log_file: Mutex<Option<File>>`) so all interpreter instances spawned
on top of the same VM (the M6 threading runtime) share one logger,
matching Python's single-process-global-logger model.

What v0.2 does **not** ship: named loggers (`logging.getLogger("app.db")`);
custom formatters (the format string is fixed); rotating file handlers;
multiple handlers per logger; structured / JSON output; log records with
exception tracebacks (`logger.exception(...)`); a `disable` /
`shutdown` API; the `LogRecord` object.  All of those are v0.3 work,
gated on stdlib classes.

---

### 9.30 Module `shutil` (v0.2 — M27 P3c-A)

High-level filesystem operations layered on top of M20a's `os` /
`path` modules.  Closes the long-standing v0.2 gap M24-D documented:
there was no recursive directory removal in the stdlib.

```
fn copy(src: str, dst: str) -> None
fn copytree(src: str, dst: str) -> None
fn move(src: str, dst: str) -> None
fn rmtree(path: str) -> None
fn which(cmd: str) -> str?
fn disk_usage(path: str) -> Tuple[i64, i64, i64]
### 9.32 Module `glob` (v0.2 — M27 P3c-B)

Unix-shell-style pathname wildcard expansion.  Backed by the `glob`
crate, which ships both pattern matching (`glob::Pattern`) and a
directory walker (`glob::glob` / `glob::glob_with`).  Each native
handler is a thin wrapper — no hand-rolled FSM or directory walker
lives in the VM.

```
fn glob(pattern: str) -> List[str]
fn recursive(pattern: str) -> List[str]
fn escape(s: str) -> str
```

Semantics:

* `copy(src, dst)` copies a single file's bytes from `src` to `dst`.
  Matches `shutil.copyfile` rather than full `shutil.copy` — the
  permission bits are NOT preserved (Python's `copy` preserves
  permissions via `copymode`, which is itself a v0.3 gap because
  StrictPy doesn't yet have a `chmod` surface).  Raises `IOError`
  on filesystem failure.
* `copytree(src, dst)` recursively copies the directory rooted at
  `src` to `dst`.  Matches CPython 3.7+: `dst` must NOT already
  exist; the call raises `IOError` if it does.  Symlinks are
  followed (CPython default `symlinks=False`).
* `move(src, dst)` renames in place when possible (fast path on the
  same filesystem).  On cross-filesystem moves where `rename`
  returns `ENXDEV` / `ERROR_NOT_SAME_DEVICE`, falls back to
  copy-then-remove (file or directory tree as appropriate).  This
  is the same fallback Python's `shutil.move` uses.
* `rmtree(path)` removes a directory and all its contents
  recursively.  Raises `IOError` if `path` doesn't exist OR if
  `path` is not a directory (use `os.remove` for files).  Maps to
  `std::fs::remove_dir_all` under the hood.
* `which(cmd)` searches `$PATH` (`%PATH%` on Windows) for an
  executable named `cmd`.  Returns the absolute path to the first
  hit, or `none` if not found.  On Windows, when `cmd` has no
  extension, also tries `.exe` / `.bat` / `.cmd` / `.com` in CPython's
  preferred order — matches CPython 3.x `shutil.which` behaviour.
  Absolute / relative-with-separator inputs are checked directly
  without consulting `$PATH`.
* `disk_usage(path)` returns `(total, used, free)` bytes for the
  filesystem mount-point containing `path`.  `used` is always
  `total - free` (CPython derives it the same way — no separate
  syscall).  Uses `statvfs` on Unix and `GetDiskFreeSpaceExW` on
  Windows.

Errors:

* All filesystem-level failures (permission denied, path not found,
  cross-volume rename failure that the copy fallback also rejected,
  etc.) surface as `IOError` with the underlying OS message in the
  body.
* `copytree(src, existing_dst)` raises `IOError` with a "destination
  already exists" message — DOES NOT overwrite silently.

What v0.2 does **not** ship: `copy2` (which preserves metadata —
needs `chmod` + stat-time preservation); `copytree(..., ignore=...)`
filter callbacks (need closures-across-NativeFn-boundary, deferred);
`make_archive` / `unpack_archive` (need full `tar` / `zip` parsers
— deferred to v0.3 alongside the `bytes` runtime type); `chown`
(needs Unix-specific uid/gid surface); `get_terminal_size` (TTY
introspection); `rmtree(..., onerror=...)` recovery hooks.

### 9.31 Module `tempfile` (v0.2 — M27 P3c-A)

Temporary file and directory creation, backed by the `tempfile`
Rust crate.  The crate handles the per-OS atomic-creation syscall
(`mkdtemp` on Unix, NTFS-aware on Windows) and applies restrictive
permissions (`0o700` directories, `0o600` files on Unix; ACL'd to
the current user on Windows).

```
fn mkdtemp(prefix: str = "tmp") -> str
fn mkstemp(prefix: str = "tmp", suffix: str = "") -> str
fn gettempdir() -> str
```

(StrictPy v0.2 doesn't yet have default-argument support on stdlib
items; callers pass `prefix` and `suffix` explicitly.  Pass `""`
for the suffix when you don't want one.)

Semantics:

* `mkdtemp(prefix)` creates a fresh directory under the system
  temp root (per `tempfile.gettempdir()`) with a randomised name
  starting with `prefix`.  Returns the absolute path.  The
  directory is NOT auto-cleaned on program exit — the caller is
  responsible (typically via `shutil.rmtree`).  Matches Python's
  `tempfile.mkdtemp` guarantee.
* `mkstemp(prefix, suffix)` creates a fresh file under the system
  temp root, with a randomised name like `<prefix><random><suffix>`,
  and returns its absolute path.  The file is created (zero bytes)
  and immediately closed; the caller re-opens via `open(...)` for
  use.  Unlike Python's `mkstemp` (which returns `(fd, path)`),
  StrictPy returns the path only because the runtime doesn't expose
  raw file descriptors.
* `gettempdir()` returns the system temp directory path, honouring
  `$TMPDIR` / `$TEMP` / `$TMP` environment variables (per
  `std::env::temp_dir`).  Typically `/tmp` on Unix, `%LOCALAPPDATA%\
  Temp` (or `C:\Windows\Temp`) on Windows.

Errors:

* `mkdtemp` / `mkstemp` raise `IOError` if the OS rejects the
  creation (e.g. permission denied on the temp root, disk full,
  filename collision after the configured number of retries — the
  `tempfile` crate retries internally before surfacing the failure).

What v0.2 does **not** ship: `NamedTemporaryFile`,
`SpooledTemporaryFile`, `TemporaryDirectory` — all three are
class-shaped context managers, blocked on stdlib-class registration
(the same v0.3 work that unblocks typed `JsonValue`, `ArgParser`,
and pathlib's `Path`).  Programs that want auto-cleanup-on-scope-exit
should pair `mkdtemp` / `mkstemp` with `try` / `except` and an
explicit `shutil.rmtree` / `os.remove` in the cleanup arm.
* `glob(pattern)` matches paths under the current working directory
  against `pattern` using shell-style wildcards: `*` matches any
  run of characters within a single path component, `?` matches a
  single character, and `[abc]` / `[a-z]` are character classes.
  Returns a `List[str]` of matching paths, **sorted ascending**.
  Does NOT recurse into subdirectories — `*` stops at the first
  path separator.  Returns the empty list (not an error) when no
  paths match.  Case sensitivity follows the platform's filesystem:
  Windows is case-insensitive, Unix is case-sensitive.
* `recursive(pattern)` is `glob(pattern)` with `**` enabled — a
  `**` segment matches arbitrarily-deep subdirectories.  Equivalent
  to Python's `glob.glob(pattern, recursive=True)`.  Same sort and
  case-sensitivity rules.
* `escape(s)` quotes the glob metacharacters `*`, `?`, and `[` in
  `s` by wrapping each in a single-character class (so `a*b`
  becomes `a[*]b`).  Other characters pass through unchanged.  Use
  this when a literal filename contains a metacharacter and you
  want to match it exactly.  Matches CPython's `glob.escape`.

Errors:

* `glob` / `recursive` raise `ValueError` on a malformed pattern
  (unterminated `[`, etc.) — message is `"glob: invalid pattern
  ...: <crate error>"`.
* Individual `read_dir` failures for one subdirectory during a walk
  are silently skipped (matches CPython's behaviour — `glob.glob`
  doesn't abort on a single permission-denied subtree).

Not in v0.2:

* `glob.iglob` — lazy iterator.  v0.3 work; v0.2 always materialises
  the full list (programs that need streaming can still cap memory
  by globbing one subdirectory at a time).
* `glob.has_magic(s) -> bool` — `True` iff `s` contains glob
  metacharacters.  Trivial to add; deferred to v0.3 alongside
  `iglob` so the surface ships in one batch.
* Tilde / shell-variable expansion (`~`, `$HOME`).  CPython's `glob`
  doesn't do these either — callers compose with `os.env(...)` and
  `pathlib.join(...)` instead.

### 9.33 Module `fnmatch` (v0.2 — M27 P3c-B)

Single-string shell-glob matching (no I/O).  Backed by the same
`glob::Pattern` matcher used by `glob.glob`, so the pattern syntax
is identical: `*` matches any run of characters, `?` matches one,
`[abc]` / `[a-z]` are character classes, and `[!abc]` is CPython's
negated class.

```
fn fnmatch(name: str, pattern: str) -> bool
fn fnmatchcase(name: str, pattern: str) -> bool
fn filter(names: List[str], pattern: str) -> List[str]
fn translate(pattern: str) -> str
```

Semantics:

* `fnmatch(name, pattern)` returns `true` iff `name` matches
  `pattern`.  **Case sensitivity is platform-dependent**, matching
  CPython: Windows is case-INsensitive, Unix is case-sensitive.
  Programs that care about portable behaviour should use
  `fnmatchcase` instead.
* `fnmatchcase(name, pattern)` is `fnmatch` with case sensitivity
  forced ON regardless of platform.  This is the recommended form
  for cross-platform code.
* `filter(names, pattern)` returns a new `List[str]` containing
  every element of `names` that matches `pattern`, **in input
  order**.  Case sensitivity follows `fnmatch` (i.e.
  platform-dependent).  Empty input list yields an empty result
  with no allocation overhead.
* `translate(pattern)` converts a shell-glob pattern into an
  anchored regex string callers can feed into `re.fullmatch` (or
  `re.search` since the regex is end-anchored with `\z`).  The
  output shape is `"(?s:<body>)\z"` where `<body>` has each glob
  construct rewritten as the regex equivalent:
    * `*` → `.*`
    * `?` → `.`
    * `[abc]` / `[a-z]` → passed through unchanged
    * `[!abc]` → `[^abc]` (CPython's negation translates to regex)
    * Regex metacharacters in literal positions (`.`, `^`, `$`, `+`,
      `(`, `)`, `|`, `{`, `}`, `\`) are backslash-escaped.
  The `(?s:...)` group enables dot-matches-newline so `*` truly is
  "any character".  The `\z` end anchor is the Rust `regex` crate's
  end-of-input assertion (CPython uses `\Z`; the semantic is the
  same for `re.fullmatch`).

Errors:

* `fnmatch` / `fnmatchcase` / `filter` / `translate` raise
  `ValueError` on a malformed pattern (unterminated `[`, etc.)
  — message `"fnmatch: invalid pattern ...: <crate error>"`.
  CPython raises `re.error` from `translate`; we keep `ValueError`
  for surface consistency with the rest of the v0.2 stdlib.

Not in v0.2:

* No precompiled-pattern handle.  Every call re-parses the pattern.
  For tight loops over a fixed pattern, the `glob::Pattern`-cache
  optimisation is v0.3 work (same shape as the deferred
  `re.compile`).
* No `fnmatch.translate` round-trip back into a glob pattern.
  Translate is one-way.
### 9.34 Module `gzip` (v0.2 — M27 P3c-C)

gzip-format (RFC 1952) compressor / decompressor over `str`.  Backed
by the `flate2` crate's `GzEncoder` / `GzDecoder`.

```
fn compress(data: str)                       -> str
fn compress_level(data: str, level: i32)     -> str   # level 0..=9
fn decompress(data: str)                     -> str
```

**The str-as-byte-buffer convention (binary surface).** All three
modules in this trio (`gzip`, `zlib`, `bz2`) interpret `str` as a
buffer of bytes 0..=255 where each byte is one Unicode codepoint
(C0 + Latin-1).  This is the same convention M22 P2D `struct` adopted:
`len(buf)` equals the byte count, indexing returns one-byte
codepoints, and concatenation works at the byte level.  Programs feed
text in through plain ASCII (each codepoint < 128 is exactly its
byte value); binary blobs are built codepoint-by-codepoint with
`chr(b)` where `0 ≤ b ≤ 255`.  Compressed output bytes outside the
ASCII range are emitted as the corresponding U+0080..U+00FF
codepoints, so the resulting `str`'s `byte_len` is ≥ the underlying
byte count — but `len(buf)` is still the byte count.  A real `bytes`
runtime type is v0.3 work; until then this convention round-trips
losslessly.

Semantics:

* `compress(data)` runs DEFLATE at level 6 (Python's default), wraps
  the result in the gzip header / footer (RFC 1952), and emits the
  bytes as a packed-byte `str`.  Empty input still produces a
  non-empty result (the gzip header is always written).
* `compress_level(data, level)` accepts level 0..=9.  0 means
  "store" (no compression — just gzip framing around the raw
  bytes); 9 is maximum compression.  Out-of-range levels raise
  `ValueError`.
* `decompress(data)` parses the gzip framing and runs DEFLATE
  inflate.  Malformed input (bad magic, truncated stream, bad CRC,
  ...) raises `ValueError` with the underlying flate2 error in the
  message.

What v0.2 does **not** ship:

* Streaming `GzipFile` handle for incremental compression.  Pulls in
  stdlib-class registration (M20c's deferred work).  v0.3.
* `mtime` / `filename` header field control.  The encoder emits
  default values (no filename, mtime = 0); v0.3 may add a tuple-
  returning helper if a real-world program needs to inspect them.
* Multi-member gzip streams.  flate2 reads only the first member;
  `gunzip -c | cat` semantics await v0.3.

### 9.35 Module `zlib` (v0.2 — M27 P3c-C)

Raw zlib-format (RFC 1950) DEFLATE codec — the same algorithm as
`gzip` minus the gzip header / footer, plus a 2-byte zlib header
and a 4-byte Adler-32 trailer.  Also exposes the CRC-32 and
Adler-32 checksum primitives directly.  Backed by `flate2`
(`ZlibEncoder` / `ZlibDecoder` / `flate2::Crc`).

```
fn compress(data: str)                       -> str
fn compress_level(data: str, level: i32)     -> str   # level 0..=9
fn decompress(data: str)                     -> str
fn crc32(data: str)                          -> i64
fn adler32(data: str)                        -> i64
```

The str-as-byte-buffer convention (see §9.34) applies to the three
data-shaped entry points and to the two checksum inputs.

Semantics:

* `compress` / `compress_level` / `decompress` mirror the `gzip`
  variants except the wire format is RFC 1950 (zlib) — about 18
  fewer bytes of framing than gzip, but the body is identical
  DEFLATE.  Use `zlib.compress` for embedded protocol data (HTTP
  `Content-Encoding: deflate`, PNG `IDAT`, git objects); use
  `gzip.compress` for `.gz` files on disk.
* `crc32(data)` runs the IEEE CRC-32 polynomial (the same checksum
  the gzip trailer carries).  Returns a non-negative i64 in
  0..=0xFFFF_FFFF.  Output matches Python's `zlib.crc32(data)`
  exactly.  Empty input returns 0.
* `adler32(data)` runs RFC 1950 §9's Adler-32 over the bytes (the
  same checksum the zlib trailer carries).  Returns a non-negative
  i64 in 0..=0xFFFF_FFFF.  Output matches Python's
  `zlib.adler32(data)`.  Empty input returns 1 (the standard
  initial state).
* Returning the checksums as `i64` avoids the
  signed-vs-unsigned ambiguity that would arise from packing a `u32`
  into `i32` (the high bit would surface as a negative number on
  most inputs).  Callers can compare against literal hex constants
  directly: `if z == 0xCBF43926: ...`.

What v0.2 does **not** ship:

* Streaming `Compress` / `Decompress` handles for incremental work.
  Same v0.3 stdlib-class blocker as `gzip`.
* Dictionary-primed compression (RFC 1950 FDICT bit).
* `decompressobj.flush(Z_SYNC_FLUSH)` for partial flushes — the
  current API is one-shot only.

### 9.36 Module `bz2` (v0.2 — M27 P3c-C)

bzip2-format (BWT + MTF + RLE + Huffman) compressor / decompressor.
Backed by the `bzip2` crate.

```
fn compress(data: str)                       -> str
fn compress_level(data: str, level: i32)     -> str   # level 1..=9
fn decompress(data: str)                     -> str
```

The str-as-byte-buffer convention (see §9.34) applies to all three.

Semantics:

* `compress(data)` runs bzip2 at level 6.  Output is a packed-byte
  `str` carrying the standard `BZh6` bzip2 header.  Empty input
  produces a 14-byte "empty bzip2 stream" output.
* `compress_level(data, level)` accepts level 1..=9.  Note bzip2's
  level scale differs from gzip / zlib (which use 0..=9): there is
  no "store" mode.  Out-of-range levels raise `ValueError`.
* `decompress(data)` runs bzip2 inflate.  Malformed input raises
  `ValueError`.
* bzip2 typically yields tighter output than gzip / zlib on text-
  heavy inputs (English prose, source code, JSON) at the cost of
  ~5-10x slower compression and ~3-5x slower decompression.  Pick
  bz2 for archival storage where size dominates time; pick zlib for
  network protocols where latency dominates.

What v0.2 does **not** ship:

* Streaming `BZ2File` handle.  Same blocker as gzip / zlib.
* Multi-stream bzip2 input.  The decoder reads the first stream
  only.
* Parallel block-level decoding (`lbzip2` / `pbzip2` style).
  bzip2's block structure makes this tractable in principle; v0.3
  may add it if a real-world program needs it.

---

### 9.37 Module `zipfile` (v0.2 — M27 P3c-D)

A read+write surface for `.zip` archives, wrapping the pure-Rust `zip`
crate.  Archives are referenced by opaque `i64` handles into a
per-process slot table on `SharedVm`; separate slot tables guard read
and write handles so closing one mode does not collide with the other.

```
fn open_read(path: str) -> i64
fn open_write(path: str) -> i64
fn names(handle: i64) -> List[str]
fn read(handle: i64, name: str) -> str
fn write(handle: i64, name: str, data: str) -> None
fn close(handle: i64) -> None
fn is_zipfile(path: str) -> bool
fn info(handle: i64, name: str) -> Tuple[i64, i64, i64]
```

Semantics:

* `open_read(path)` opens an existing zip for reading.  Raises
  `IOError` if the file is missing, unreadable, or not a valid zip
  archive.  The returned handle stays valid until `close` is called on
  it (or the VM exits).
* `open_write(path)` creates (or truncates) a zip file for writing.
  New entries are appended with DEFLATE compression.  Raises `IOError`
  on permission failure / unwritable path.
* `names(handle)` returns the entry names of a read-mode archive in
  whatever order the underlying central directory records them.
  `ValueError` if the handle is invalid / closed.
* `read(handle, name)` returns the entry's decompressed payload as a
  `str` whose chars are each codepoint 0..255 — the str-as-byte-buffer
  convention v0.2 uses for binary data (same trick `struct.pack` uses).
  `len(s)` in chars equals the byte count.  Convert to UTF-8 text with
  the standard codec helpers if the entry is known-textual.
* `write(handle, name, data)` adds a new entry to a write-mode
  archive.  `data` is interpreted as a packed-byte string (each char
  must be a codepoint 0..255); use `bytes_to_packed_str`-equivalent
  helpers if writing UTF-8 text data.
* `close(handle)` finalizes the write central directory (for writers)
  or drops the read handle.  Calling `close` on a zero or already-closed
  handle is a no-op.
* `is_zipfile(path)` returns `True` iff the file exists, is readable,
  and parses as a valid zip archive.  Does not raise on missing files.
* `info(handle, name)` returns `(compressed_size, uncompressed_size,
  crc32)` as `i64`.  Returns `(-1, -1, -1)` if the entry does not
  exist in the archive — chosen over raising so user code can probe
  for an entry's presence without a try/except wrapper.

Errors:

* Invalid / closed handle → `ValueError`.
* Filesystem open failure → `IOError`.
* Entry-not-found in `read` → `ValueError` (it's an explicit lookup;
  `info` returns `(-1, -1, -1)` for the probe shape).
* Out-of-range codepoint in `write` data → `ValueError` (the
  packed-byte invariant is broken).

What v0.2 does **not** ship: append-mode (existing entries can't be
modified — open a new writer + copy them over); zip64 streaming (entries
must fit in memory at write time); password-protected entries; entry
metadata beyond `info`'s triple (no timestamps, no per-entry compression
mode choice — all writes go through DEFLATE); per-entry comments; the
`Connection`/`Cursor`-style class surface (needs stdlib classes, v0.3).

Concurrency: like `sqlite3`, the read and write slot tables live behind
mutexes on `SharedVm`.  A long `read` on one handle does not block
sibling `open_read` / `open_write` calls on other threads — but two
threads holding the *same* handle and calling `read` concurrently
serialise on the inner archive borrow.

---

### 9.38 Module `tarfile` (v0.2 — M27 P3c-D)

A read+write surface for POSIX `.tar` archives, with optional gzip /
bz2 transparent compression.  Wraps the pure-Rust `tar` crate plus
`flate2` (gzip) and `bzip2` (bz2) as compression layers.  Mode strings
match Python's `tarfile.open(name, mode)` exactly.

```
fn open_read(path: str, mode: str) -> i64
fn open_write(path: str, mode: str) -> i64
fn names(handle: i64) -> List[str]
fn read(handle: i64, name: str) -> str
fn write_file(handle: i64, src_path: str, arcname: str) -> None
fn write_data(handle: i64, arcname: str, data: str) -> None
fn close(handle: i64) -> None
fn is_tarfile(path: str) -> bool
```

Semantics:

* `open_read(path, mode)` opens an existing tar for reading.  `mode`
  is one of:
    * `"r"` — plain (uncompressed) tar.
    * `"r:gz"` — gzip-wrapped.
    * `"r:bz2"` — bz2-wrapped.
  All entries are eagerly decoded into a `name -> bytes` map at open
  time, so subsequent `read` calls are O(1) hashmap lookups.  This
  keeps the API simple and works fine for the tens-of-MB scale typical
  of build / log / backup archives; streaming-decode of arbitrarily
  large archives is a v0.3 candidate.
* `open_write(path, mode)` creates a new tar for writing.  `mode` is
  one of `"w"`, `"w:gz"`, `"w:bz2"`.
* `names(handle)` returns the entry names in the archive's natural
  (declaration) order — tar files are streamed in that order and the
  shape matches Python's `TarFile.getnames()`.  Only regular files are
  listed (directory / symlink / device entries are skipped, since
  `read` would return empty for them anyway and v0.2 has no `bytes`
  type to distinguish "empty payload" from "not a file").
* `read(handle, name)` returns the entry's decompressed payload as a
  packed-byte `str` (same convention as `zipfile.read`).  `ValueError`
  if the entry is not present.
* `write_file(handle, src_path, arcname)` appends a file from disk to
  the archive under `arcname`.  The on-disk file's mode bits and mtime
  are recorded in the tar header.
* `write_data(handle, arcname, data)` appends in-memory bytes (as a
  packed-byte string) under `arcname` with mode `0o644` and the
  current time stamped on the entry.
* `close(handle)` finishes the tar stream (and the underlying gz / bz2
  encoder, for compressed modes).  Closing a zero / already-closed
  handle is a no-op.
* `is_tarfile(path)` returns `True` iff the file exists and its first
  512 bytes contain the `"ustar"` POSIX/GNU header magic at offset
  257.  Deliberately does **not** probe gzipped / bz2'd tarballs —
  callers who want that behaviour can compose `is_tarfile` after
  decompressing the leading block themselves (the cost of a triple
  decode-probe didn't seem worth wiring into v0.2).

Errors:

* Invalid / closed handle → `ValueError`.
* Unsupported mode string → `ValueError`.
* Filesystem open failure → `IOError`.
* Entry-not-found in `read` → `ValueError`.
* Out-of-range codepoint in `write_data` payload → `ValueError`.

What v0.2 does **not** ship: streaming reads of very large archives
(everything is loaded at open time — fine for the tens-of-MB scale;
multi-GB archives wait for v0.3); per-entry uid/gid/owner-name
customisation in `write_data` (mode bits are fixed at `0o644`); xz / lzma
compression modes (the `tar` crate supports them via `xz2`; deferred
until there's a use case); listing of directory / device / symlink
entries (the v0.2 names list only contains regular files); appending
to an existing tar (open a fresh writer and copy entries across).

Concurrency: same shape as `zipfile` — per-table mutex, sibling
`open` / `close` calls on other threads do not block a long-running
`write_file`, but two threads writing to the *same* handle serialise on
the per-slot enum.

---

### 9.40 Module `socket` (v0.2 — M28 P3b-A)

Raw TCP / UDP networking layered on `std::net`.  Three opaque-handle
slot tables on `SharedVm` (`tcp_streams`, `tcp_listeners`,
`udp_sockets`) own the underlying OS file descriptors; user code only
ever sees `i64` handles.  Bytes ride on `str` with each codepoint a
byte 0..255 (the str-as-byte-buffer convention shared with
`struct.pack`, `zipfile.read`, and `gzip.decompress`).  No new crate
dep — `std::net` covers TCP / UDP / DNS resolution on Linux, macOS
and Windows.

```
# TCP client
fn connect_tcp(host: str, port: i32) -> i64
### 9.41 Module `ssl` (v0.2 — M28 P3b-B; server-side M28.5 P3b-D)

TLS-over-TCP client.  Wraps the pure-Rust `rustls` 0.23 crate with the
`ring` crypto provider so the build needs no system OpenSSL.  Trust
defaults to the Mozilla root bundle that `webpki-roots` ships
statically — the same set CPython falls back to on systems without a
configured trust store.  Connections are opaque `i64` handles into a
per-process slot map on `SharedVm.tls_streams`, identical in shape to
the `sqlite3` / `zipfile` / `tarfile` handle convention.

```
fn connect(host: str, port: i32) -> i64
fn send(handle: i64, data: str) -> i32
fn recv(handle: i64, max_bytes: i32) -> str
fn recv_exact(handle: i64, n: i32) -> str
fn close(handle: i64) -> None
fn set_timeout_secs(handle: i64, secs: f64) -> None
fn peer_addr(handle: i64) -> str
fn local_addr(handle: i64) -> str

# TCP server
fn listen_tcp(host: str, port: i32, backlog: i32) -> i64
fn accept(listener: i64) -> Tuple[i64, str]   # (new_stream, peer_addr)
fn close_listener(listener: i64) -> None

# UDP
fn udp_socket() -> i64
fn udp_bind(host: str, port: i32) -> i64
fn udp_send_to(handle: i64, data: str, host: str, port: i32) -> i32
fn udp_recv_from(handle: i64, max_bytes: i32) -> Tuple[str, str, i32]
fn udp_close(handle: i64) -> None

# DNS / utility
fn gethostbyname(host: str) -> str
fn resolve(host: str, port: i32) -> List[str]
fn gethostname() -> str
fn peer_addr(handle: i64) -> str
fn peer_cert_subject(handle: i64) -> str
fn set_timeout_secs(handle: i64, secs: f64) -> None
fn set_verify_certs(enabled: bool) -> None
fn get_verify_certs() -> bool
### 9.42 Module `http_client` (v0.2 — M28 P3b-C)

A synchronous HTTP/1.1 client backed by the `ureq` crate (which bundles
`rustls` + `webpki-roots` for TLS).  Every call is stateless: a fresh
TCP socket is opened, the request sent, the response read, and the
socket closed.  No connection pooling, no cookie jar, no async runtime —
those are v0.3 territory.  The module covers the 80% case (one-shot
GETs and POSTs) with the convenience entry points and exposes a
configurable form for everything else.

```
# Convenience methods (no custom headers, default 30s timeout)
fn get(url: str) -> Tuple[i32, str]
fn post(url: str, body: str, content_type: str) -> Tuple[i32, str]
fn put(url: str, body: str, content_type: str) -> Tuple[i32, str]
fn delete(url: str) -> Tuple[i32, str]
fn head(url: str) -> Tuple[i32, str]

# Configurable request (the 20% case)
fn request(method: str, url: str, body: str,
           headers: List[Tuple[str, str]],
           timeout_secs: f64) -> Tuple[i32, str]
fn request_with_headers(method: str, url: str, body: str,
                        headers: List[Tuple[str, str]],
                        timeout_secs: f64)
    -> Tuple[i32, List[Tuple[str, str]], str]

# Utilities
fn urlencode(pairs: List[Tuple[str, str]]) -> str
fn urldecode(s: str) -> str
fn url_parse(url: str) -> Tuple[str, str, i32, str]
fn status_text(code: i32) -> str
```

Semantics:

* `connect_tcp(host, port)` resolves `host` to one or more addresses
  and connects to the first one that succeeds (matches the standard
  `TcpStream::connect` behaviour).  IPv6 is transparent — pass
  `"::1"` for loopback or any v6 literal and the same code path is
  used.  Raises `IOError` on resolution / connect failure.
* `send(handle, data)` writes at most `len(data)` bytes; returns the
  actual count.  May be less than `len(data)` on partial writes (rare
  for small buffers on a healthy connection but possible for large
  buffers on a slow link).  Callers should loop until satisfied or use
  `recv_exact`'s siblings (a `send_all` helper is a v0.3 candidate).
* `recv(handle, max_bytes)` reads up to `max_bytes`; returns the empty
  string `""` on EOF (the peer cleanly closed).  Length of the
  returned str is the actual byte count.
* `recv_exact(handle, n)` reads exactly `n` bytes or raises `IOError`
  on short read / EOF.  Useful for length-prefixed protocols.
* `close(handle)` calls `flush()` then `shutdown(Both)` then drops the
  socket.  The flush is load-bearing on Windows: closing a winsock
  socket with bytes still buffered in user-space can drop them; the
  explicit flush + shutdown sequence matches what every cross-platform
  TCP example recommends.  Closing slot 0 / a zero / an already-closed
  handle raises `ValueError`.
* `set_timeout_secs(handle, secs)` applies the same duration to both
  `set_read_timeout` and `set_write_timeout`.  `0.0` (or any
  non-finite value) clears the timeout — `Inf` / `NaN` raises
  `ValueError`.  Operations that time out raise `IOError`.
* `peer_addr` / `local_addr` return printable `"ip:port"` strings
  (IPv4 → `"127.0.0.1:53412"`, IPv6 → `"[::1]:53412"`).
* `listen_tcp(host, port, backlog)` binds + listens; `backlog` is
  accepted for API symmetry but Rust's `TcpListener::bind` does not
  expose `listen()`'s backlog argument, so the parameter is currently
  documented-but-not-enforced.  Use `port = 0` to ask the OS for an
  ephemeral port; the bound port can be discovered via `local_addr`
  on an accepted stream.
* `accept(listener)` blocks until a peer connects and returns
  `(new_stream_handle, peer_addr)`.  No timeout in v0.2 — callers who
  need one should set `O_NONBLOCK` via a v0.3 `set_nonblocking` API
  (deferred).  The returned stream inherits the listener's
  blocking / timeout state.
* `close_listener(handle)` releases the listener slot **and**
  interrupts any thread currently blocked inside `accept(handle)` on
  the same listener (M30 BUG-040 fix; matches Python's `socket.close`
  behaviour, where closing a socket from one thread wakes a `recv` /
  `accept` on another).  The shape of the wake-up is platform-
  dependent:
  - **Unix (Linux, macOS):** the blocked `accept` returns with
    `IOError` (the underlying `shutdown(SHUT_RDWR)` causes the kernel
    to return `EINVAL` / `ECONNABORTED`, which the VM maps to
    `IOError`).  User code should put the call in a `try / except
    IOError` block when shutdown-from-another-thread is part of its
    lifecycle.
  - **Windows:** the blocked `accept` returns *successfully* with a
    throwaway connection — Winsock does not wake `accept` from
    `shutdown`, so the stdlib delivers a wake-up via a self-connect
    to the listener's bound address (see KB-179942 for the rationale
    against `closesocket` as the alternative).  User code that
    inspects a flag-or-shared-state to detect "shutdown was
    requested" will see the close, drop the throwaway connection,
    and exit its accept loop cleanly.
  Programs that want a single error-shape across platforms can also
  follow the M29.5 webserver pattern: set a `shutdown_requested`
  flag before calling `close_listener`, then check the flag in the
  accept loop after every successful accept.  The flag check is
  cheap and handles both wake-up shapes uniformly.  Closing slot 0 /
  an already-closed listener raises `ValueError`.
* `udp_socket()` binds to `0.0.0.0:0` (OS-assigned ephemeral v4
  port); `udp_bind(host, port)` binds to a fixed endpoint.
* `udp_send_to(handle, data, host, port)` sends one datagram; returns
  the number of bytes the kernel queued (always equal to `len(data)`
  for v4/v6 datagrams under MTU on loopback).
* `udp_recv_from(handle, max_bytes)` returns `(data, src_host,
  src_port)`.  `src_host` is the printable IP, NOT the resolved host
  name (matches POSIX `recvfrom`'s behaviour).
* `gethostbyname(host)` resolves and returns the first IPv4 address
  if any v4 results came back, otherwise the first v6 address.  The
  v4-preference is deliberate — most legacy / loopback code expects
  `"127.0.0.1"` rather than `"::1"`.
* `resolve(host, port)` returns every `"ip:port"` the system resolver
  produced (v4 first, then v6 on most platforms).
* `gethostname()` returns the local host name.  v0.2 uses the
  `HOSTNAME` / `COMPUTERNAME` environment-variable fallback chain
  (CPython's own gethostname has the same env-var fast-path); falls
  back to `"localhost"` if neither is set.  A proper `gethostname(2)`
  syscall wrapper is a v0.3 candidate (would need a thin libc shim
  on Unix and `GetComputerNameW` on Windows).

Errors:

* Invalid / closed handle → `ValueError`.
* DNS / connect / bind failure → `IOError`.
* `recv_exact` short-read → `IOError`.
* Out-of-range codepoint in `data` payload (any byte > 255) →
  `ValueError`.
* Non-finite or negative timeout → `ValueError`.

Cross-platform notes:

* Linux / macOS: `std::net` wraps the POSIX socket syscalls; no
  surprises.
* Windows: `std::net` uses winsock under the hood.  Two gotchas
  baked into the API:
  - `close` calls `flush()` first to avoid the close-drops-pending
    -bytes behaviour described above.
  - `recv_exact` on a peer-closed socket reports `UnexpectedEof`,
    which we re-raise as `IOError` (same shape as the Unix path).
* IPv6 is supported transparently on all three platforms; the API
  does not split into v4 / v6 variants.

What v0.2 does **not** ship: `set_nonblocking` / `set_nodelay` /
`set_keepalive` (the M6 thread surface already covers the
"don't block the main thread" use case); UNIX-domain sockets;
TLS / SSL wrapping (callers should layer a TLS module on top, v0.3+);
multicast / broadcast (the UDP surface is unicast-only in v0.2);
`shutdown(Read)` / `shutdown(Write)` half-close (only `Both` via
`close`); a `send_all` / `recv_into` helper.  All are v0.3 candidates.

Concurrency: socket handles are safe to share across threads via the
same mechanism as `threading.Lock` (the slot tables live behind
`Mutex`).  Two threads `recv`-ing on the same handle serialise on the
inner stream borrow but each gets a self-consistent byte chunk;
interleaved bytes would only happen if the application protocol
violates message framing rather than from the runtime.
* `connect(host, port)` opens a `TcpStream` to `host:port`, builds a
  `rustls::ClientConnection` using the current verify flag, wraps the
  pair in a `StreamOwned`, and forces the handshake to completion
  with a `flush()`.  Any TCP, name-resolution, or TLS-handshake
  failure raises `IOError`.  The returned handle is a monotonic i64
  starting at 1; handles are never re-used inside the same process,
  so a use-after-close raises `ValueError` instead of accidentally
  reaching a fresh connection.
* `send(handle, data)` decodes `data` as a packed-byte string (each
  codepoint must be 0..255 — same str-as-byte-buffer convention as
  `struct` / `zipfile`) and writes all bytes; returns the number of
  plaintext bytes written.  Raises `ValueError` if a codepoint is
  out of range, `IOError` on socket / TLS failure.
* `recv(handle, max_bytes)` reads up to `max_bytes` plaintext bytes
  from the stream and returns them as a packed-byte str.  A zero-
  length result signals clean EOF (peer sent close_notify).
* `recv_exact(handle, n)` reads exactly `n` plaintext bytes or raises
  `IOError` on short read / EOF.  `n < 0` raises `ValueError`.
* `close(handle)` drops the underlying stream, which causes `rustls`
  to write `close_notify` and shut down the TCP socket.  Calling
  `close` on handle `0` or on an already-closed handle is a no-op.
* `peer_addr(handle)` returns the remote endpoint formatted as
  `"<ip>:<port>"` (IPv4) or `"[<ip>]:<port>"` (IPv6, matching
  `std::net::SocketAddr::Display`).  Empty string if the handle is
  closed.
* `peer_cert_subject(handle)` returns the subject Common Name (CN)
  attribute extracted from the peer certificate's DER subject, or
  the empty string if the peer presented no cert (extremely rare)
  or the cert has no CN attribute (modern certs sometimes rely on
  Subject Alternative Names only).  v0.2 does not expose the full
  SAN list, the issuer, the validity window, or the serial number
  — `peer_cert_subject` is a presence-check shortcut, not a full
  certificate-inspection API.
* `set_timeout_secs(handle, secs)` applies the same duration to both
  the underlying TCP socket's read and write deadlines.  `secs <=
  0.0` or non-finite clears the timeout (blocking forever).  Raises
  `ValueError` for invalid handles.
* `set_verify_certs(enabled)` is a **process-global** flag that
  affects only subsequent `connect` calls.  Default `true`.
  Setting it to `false` installs a "trust everything" verifier;
  this is strictly for testing against self-signed loopback /
  staging certificates.  Production code that needs alternative
  trust should use the per-connection custom-CA API once it lands
  in v0.3 (ID 610 is reserved).
* `get_verify_certs()` reads the current flag.

Concurrency: the handle map lives behind a single `std::sync::Mutex`
on `SharedVm`.  `connect` / `close` calls are short and only contend
on the map lock; the actual I/O happens after the lock is released
(the handler holds the slot's stream by `&mut`, blocking sibling
operations on the *same* handle but not on other handles).

#### Server-side TLS (M28.5 P3b-D)

The client surface above bundles TCP + TLS into a single `connect`
call because v0.2 has no separate `socket` module to plug into.
M28 P3b-A then shipped the `socket` module (§9.40), and M28.5 P3b-D
closes the v0.3-deferred gap: the StrictPy stdlib can now accept an
inbound TCP connection on a `socket.listen_tcp` handle and present a
PEM-loaded cert chain + private key to the peer.  The three new
functions reuse the same opaque-handle convention as the rest of the
module, slot into the IDs (610-612) the original P3b-B brief
reserved, and produce handles that are interchangeable with
`ssl.connect` handles from the caller's point of view — the existing
`send` / `recv` / `recv_exact` / `close` / `peer_addr` /
`peer_cert_subject` / `set_timeout_secs` work on either side.

```
fn load_server_config(cert_pem_path: str, key_pem_path: str) -> i64
fn accept_tls(tcp_listener: i64, server_config: i64) -> Tuple[i64, str]
fn free_server_config(config: i64) -> None
```

**Loading certs and keys.**
`load_server_config(cert_pem_path, key_pem_path)` reads PEM-encoded
data from the two paths, parses out the certificate chain and
private key with `rustls-pemfile`, and builds a `rustls::ServerConfig`
using the same `ring` crypto provider the client side does.  Returns
an opaque non-zero `i64` config handle.  The config is reusable —
one handle can back many `accept_tls` calls, so a long-running
server loads its cert once at startup and uses that handle for every
connection.  Errors:

* `cert_pem_path` or `key_pem_path` not readable → `IOError`.
* PEM parse failure / no certs / no private key → `ValueError`.
* Cert + key signature-algorithm mismatch (e.g. RSA cert + ECDSA
  key) → `ValueError`.

The private key may be PKCS#1, PKCS#8, or SEC1 — `rustls-pemfile`'s
`private_key` accepts all three (`-----BEGIN PRIVATE KEY-----`,
`-----BEGIN RSA PRIVATE KEY-----`, `-----BEGIN EC PRIVATE KEY-----`).
The cert file must contain at least one `-----BEGIN CERTIFICATE-----`
block; intermediate chains are picked up in order if present.

**`ssl.accept_tls` semantics.**
`accept_tls(tcp_listener, server_config)` blocks until a peer
connects to the listener, then performs a server-side TLS handshake
using the supplied config.  The TCP listener must be a handle
returned by `socket.listen_tcp` (§9.40); the config must be a
handle returned by `load_server_config`.  Returns
`(tls_handle, peer_addr)` where:

* `tls_handle` is a fresh opaque `i64` allocated from a disjoint id
  range (`>= 1_000_000`) so the shared `send` / `recv` / `close` /
  etc. handlers can dispatch on handle value alone — a single
  handle unambiguously identifies which slot table to look in.
* `peer_addr` is the connecting client's `"ip:port"` (IPv4 →
  `"127.0.0.1:53412"`; IPv6 → `"[::1]:53412"`), same format as
  `socket.accept`.

The handshake is driven to completion inside `accept_tls` (matching
the client side's eager-handshake behaviour) so a bad client hello,
protocol mismatch, or unexpected close surfaces as `IOError` here
rather than at the first `send` / `recv`.  Errors:

* Unknown / freed `server_config` handle → `ValueError`.
* Unknown / closed `tcp_listener` handle → `ValueError`.
* `accept()` syscall failure (rare on loopback; possible if the
  listener was closed concurrently) → `IOError`.
* TLS handshake failure (peer sent garbage, cipher-suite negotiation
  failed, peer hung up mid-handshake) → `IOError`.

After `accept_tls` returns, the returned handle behaves like any
other ssl handle — `ssl.send(handle, ...)`, `ssl.recv(handle, n)`,
`ssl.recv_exact(handle, n)`, `ssl.close(handle)`, `ssl.peer_addr(handle)`,
`ssl.set_timeout_secs(handle, secs)` all work transparently.
`peer_cert_subject(handle)` returns the empty string in v0.2 because
the server config is built with `with_no_client_auth()` (mutual auth
is a v0.3 candidate).

**Releasing a config.**
`free_server_config(handle)` drops the config slot.  Live `accept_tls`
streams keep an internal `Arc<ServerConfig>` of their own, so
freeing the config does *not* terminate in-flight sessions — only
*future* `accept_tls` calls against the freed handle fail with
`ValueError`.  Calling `free_server_config(0)` or freeing an already-
freed handle is a no-op.  For a typical server (load cert once, run
forever) `free_server_config` is unnecessary; it's available for
programs that rotate certs at runtime.

**Handle id ranges.**
Client handles allocated by `connect` start at 1 and increment per
process.  Server handles allocated by `accept_tls` start at
1_000_000 and increment per process.  These id spaces are disjoint
by construction, so any handle value alone tells the VM which slot
table holds the stream.  This is why `ssl.send(handle, ...)` etc.
need no separate "client-side" / "server-side" variants — the
dispatch is on handle value, not API name.

**HTTPS server pattern.**
```
import socket
import ssl

server_cfg: i64 = ssl.load_server_config("./server.crt", "./server.key")
listener: i64 = socket.listen_tcp("0.0.0.0", 8443, 64i32)
while true:
    pair: Tuple[i64, str] = ssl.accept_tls(listener, server_cfg)
    conn: i64 = pair.0
    # ... read request, write response ...
    ssl.close(conn)
```

The example `examples/https_server_demo.spy` runs the same pattern
end-to-end inside a single process — a server thread (using
`socket.listen_tcp` + `ssl.accept_tls`) and a client thread (using
`ssl.connect` with `set_verify_certs(false)`) exchange one request
and response.  `compiler/tests/https_server_demo_runs.rs` generates
a self-signed cert at test time via `rcgen`, writes the PEMs to a
tempdir, and asserts every waypoint of the demo round-trips
correctly.

What v0.2 does **not** ship:

* Mutual auth (client certificates).  The `with_no_client_auth()`
  builder is hard-coded; mutual auth is a v0.3 candidate.
* Per-connection custom CA / pinned certificate verification.  The
  verify flag is binary (full Mozilla bundle vs trust everything);
  per-connection custom verifiers are v0.3.
* SNI override (the SNI name always equals the `host` argument on
  the client side; server side uses the cert as-is).
* ALPN negotiation, session resumption, custom cipher-suite lists,
  OCSP stapling.
* `unwrap_socket` / `wrap_socket` decomposition.  `ssl.connect` still
  bundles TCP + TLS on the client side; `accept_tls` similarly
  bundles the TCP `accept` with the TLS handshake (callers wanting
  the plaintext socket back should reach for `socket.accept`
  instead).

Examples: `examples/ssl_demo.spy` round-trips messages (including a
high-byte payload exercising the packed-byte convention) against a
loopback echo server.  See `compiler/tests/ssl_demo_runs.rs` for the
self-signed-cert server setup the test harness uses.
`examples/https_server_demo.spy` exercises the server-side surface
end-to-end with both client and server inside one StrictPy program.
* `get(url)` / `delete(url)` / `head(url)` — fire-and-forget request
  with the default 30-second timeout and `User-Agent:
  StrictPy/0.2 http_client`.  Returns `(status_code, body_str)`.
  Auto-detects `http://` vs `https://` from the URL scheme (TLS is
  transparent through rustls; no extra setup).  The body is returned
  as UTF-8 `str`; non-UTF-8 bytes are recovered lossily (matches
  Python `requests.text` with the default `apparent_encoding` path).
* `post(url, body, content_type)` / `put(url, body, content_type)` —
  send `body` as the request payload.  If `content_type` is the empty
  string the `Content-Type` header is omitted (the server's default
  applies).  Otherwise the header is set on the outgoing request.
* `request(method, url, body, headers, timeout_secs)` — full control.
  `method` is any HTTP verb (`"GET"`, `"POST"`, `"PATCH"`, …).
  `headers` is a list of `(name, value)` tuples; identical names are
  sent as separate header lines (no de-dup).  `timeout_secs <= 0` or
  non-finite is replaced with the default 30s.  `body` is sent for
  POST / PUT / PATCH; it's ignored for GET / HEAD / DELETE.
* `request_with_headers(...)` — same as `request` but the response
  headers are also returned, in receive order.  Header names follow
  the casing the server sent (which for HTTP/1.1 is what the server
  put on the wire; HTTP/2 + ureq's normalisation lowercases them).
* `urlencode(pairs)` — render `pairs` as a query string
  (`key=value&key2=value2`).  Both keys and values are percent-encoded
  using the unreserved-character set from RFC 3986
  (`A-Z a-z 0-9 - _ . ~`).  Spaces become `%20` (not `+`); programs
  that want form-encoding (where spaces become `+`) should reach for
  `urllib_parse.urlencode` instead.
* `urldecode(s)` — inverse of `urlencode`'s per-component encoding.
  `%HH` triples decode to single bytes; the resulting byte sequence
  is interpreted as UTF-8.  Malformed `%XY` (non-hex digits) raises
  `ValueError`.
* `url_parse(url)` — parse `url` into `(scheme, host, port, path_and_query)`.
  When the URL omits an explicit port, the default for the scheme is
  filled in (`80` for `http`, `443` for `https`, `0` for anything
  else).  `path_and_query` always begins with `/`; a bare host like
  `"http://example.com"` yields `"/"` for this slot.  Malformed URLs
  raise `ValueError`.
* `status_text(code)` — IANA HTTP status reason phrase for the
  supplied code (`200` → `"OK"`, `404` → `"Not Found"`, `418` →
  `"I'm a teapot"`).  Unknown codes fall back to the class name based
  on the hundreds digit (`"Informational"` / `"Success"` /
  `"Redirection"` / `"Client Error"` / `"Server Error"`), matching
  Python's `http.HTTPStatus` fallback.

Errors:

* Transport failure (DNS, TCP, TLS, timeout) → `IOError` with the
  underlying ureq error message.
* 4xx / 5xx responses are **not** raised — they are returned as the
  status code so the caller can branch on them.  Matches `requests`'s
  default behaviour (`raise_for_status` is opt-in).
* `url_parse` on a malformed URL → `ValueError`.
* `urldecode` on a malformed `%XY` escape → `ValueError`.

Body cap: every response body is read up to a 64 MiB ceiling.  Larger
responses are truncated.  Programs that need to stream multi-GiB
responses should fall back to a v0.3 streaming API (out of scope for
v0.2).

What v0.2 does **not** ship:

* **Connection pooling** — each call opens a fresh socket.  For
  high-throughput callers, batch via threads (the GIL-equivalent
  doesn't apply; the StrictPy interpreter releases the lock around
  network I/O).
* **Cookies / session state** — the `ureq` cookie store feature is
  disabled.  Programs that need cookies should manage the `Cookie`
  request header by hand via `request_with_headers`.
* **Redirect-following customisation** — ureq follows up to 5
  redirects by default; this is not configurable in v0.2.
* **Chunked / streaming request bodies** — the body argument is
  always a complete `str` sent with `Content-Length`.
* **HTTP/2 or HTTP/3** — ureq is HTTP/1.1 only.  For HTTP/2-only
  endpoints (rare in 2026) callers will see a 505 / TLS-ALPN
  mismatch.
* **Authentication helpers** — basic / bearer auth is just a
  `("Authorization", "Bearer …")` header pair via `request_with_headers`.
* **Proxy configuration** — ureq respects the `HTTP_PROXY` / `HTTPS_PROXY`
  / `NO_PROXY` environment variables but exposes no programmatic
  override.

Testing pattern: integration tests should spawn a loopback HTTP
server (`std::net::TcpListener::bind("127.0.0.1:0")`) inside the test
harness and point StrictPy code at the bound port.  This avoids any
public-network dependency.  See
`compiler/tests/http_client_demo_runs.rs` for the canonical shape.

### 9.43 Module `asyncio` (v0.3 — M32)

A library-level asynchronous I/O surface.  StrictPy v0.3 ships the
*public API shape* of an async runtime — `asyncio.run` /
`asyncio.spawn_*` / `asyncio.sleep` / `asyncio.gather_*` / `Future[T]`
and the async-variant `socket` functions — backed internally by a
thread-per-task scheduler (Shape A).  The same API surface will become
backed by a real `mio`/`polling`-based single-thread event loop in
v0.4 (Shape B); user code written against v0.3 will continue to work
unchanged.  This honours the spec's "API shape now, internal perf
swap later" rule (see §16.4 history note).

#### 9.43.1 API surface

```
# Top-level entry — runs the closure as the root task and blocks
# until it completes.  Returns its result.  Mirrors Python's
# asyncio.run(main()).
fn run_i32(target: fn() -> i32) -> i32
fn run_unit(target: fn() -> None) -> None

# Task management — start a closure as a concurrent task; returns
# immediately with a Future the caller can later .await().
# Monomorphic variants because stdlib functions are not generic
# (see §9.28 — pq_*_i64 / pq_*_str for the same pattern).
fn spawn_i32(target: fn() -> i32)  -> Future[i32]
fn spawn_i64(target: fn() -> i64)  -> Future[i64]
fn spawn_str(target: fn() -> str)  -> Future[str]
fn spawn_bool(target: fn() -> bool) -> Future[bool]
fn spawn_unit(target: fn() -> None) -> Future[None]

# Yield control for `secs` seconds.  In v0.3 (Shape A) this is just
# `thread::sleep`; in v0.4 (Shape B) it yields to the event loop.
fn sleep(secs: f64) -> None

# Wait for both / three / four futures concurrently; return their
# results as a tuple.  Positional variants because StrictPy has no
# variadics yet (cf. §5.5).
fn gather_2_i32(a: Future[i32], b: Future[i32]) -> Tuple[i32, i32]
fn gather_2_str(a: Future[str], b: Future[str]) -> Tuple[str, str]
fn gather_3_i32(a: Future[i32], b: Future[i32], c: Future[i32])
    -> Tuple[i32, i32, i32]
fn gather_3_str(a: Future[str], b: Future[str], c: Future[str])
    -> Tuple[str, str, str]
fn gather_4_i32(a: Future[i32], b: Future[i32], c: Future[i32], d: Future[i32])
    -> Tuple[i32, i32, i32, i32]
```

#### 9.43.2 `Future[T]`

`Future[T]` is a parameterised runtime type — the receiver shape
matches `Channel[T]` (§16.3) and `Atomic[T]` (§16.4).  Two methods:

```
# Block until the future is ready; return its value.
fn await(self) -> T

# Non-blocking ready check.
fn is_ready(self) -> bool
```

Calling `.await()` twice on the same future is allowed; the second
call returns the cached value without re-blocking.  The future is
released (its slot freed in the runtime's future table) when the
parent program exits or — in v0.4 — when the last reference drops.

The supported element types in v0.3 are `i32`, `i64`, `str`, `bool`,
and `None` (the latter is the return type of `Future[None]` from
`spawn_unit`).  Other element types are a v0.4 extension and trigger
a typecheck error.  This mirrors the v0.2 stdlib monomorphisation
rule (see §9.28 / §9.16) — async surface scales by element type the
same way the rest of the stdlib does.

#### 9.43.3 Non-blocking socket variants

The async surface extends the `socket` module (§9.40) with three
non-blocking variants of the existing accept / recv / send pair.  All
three return `Future[...]` instances that resolve when the
corresponding background work is done:

```
# Returns immediately with a Future that resolves to (conn_handle,
# peer_addr_string).  The accept is performed in a spawned task
# (Shape A) or registered with the event loop (Shape B).
fn async_accept(listener: i64) -> Future[Tuple[i64, str]]

# Background recv up to max_bytes; resolves with the received str
# (empty str on EOF).
fn async_recv(handle: i64, max_bytes: i32) -> Future[str]

# Background send; resolves with the actual bytes written.
fn async_send(handle: i64, data: str) -> Future[i32]
```

`async_accept` returns a `Future` over a Tuple, which v0.3 doesn't
yet support as a monomorphic spawn variant in §9.43.1's positive
list.  The async-socket surface is allowed to mint these
"compound-element" futures because they go through dedicated
NativeFn handlers (in the 720-729 ID range) rather than the generic
`spawn_*` plumbing.

#### 9.43.4 Implementation shape: thread-per-task (v0.3) → event loop (v0.4)

The v0.3 runtime is deliberately a thin façade over the existing M6
thread infrastructure (see §16.1).  Each `spawn_*` allocates a
`Future[T]` slot in `SharedVm.futures`, spawns an OS thread to run
the target closure, and stores the closure's return value back into
the slot under the slot's `Mutex` + `Condvar` when the thread
completes.  `Future.await()` parks on the same `Condvar` until the
slot transitions to "ready".  `gather_*` is just sequential
`.await()` over the inputs (because the underlying threads are
already running concurrently, sequential awaits don't serialise the
work — they only serialise the result observation).

```text
asyncio.spawn_i32(f) ─────────┐
                              │ alloc Future slot
                              │ spawn OS thread to run f
                              │ return Future[i32] handle
                              ▼
                     ┌─────────────────┐
                     │ Future slot 17  │
                     │ ready: false    │
                     │ value: ???      │
                     │ cv: Condvar     │
                     └─────────────────┘
                              ▲
                              │ thread completes → set ready+value, notify cv
                              │
asyncio.await on Future[i32] ─┘ block on cv.wait until ready
                                return value
```

**Real perf gap**: in v0.3 the runtime still consumes one OS thread
per concurrent task — there is no perf improvement over plain
`threading.Thread`.  The benefit is the *API shape*: programs written
against `asyncio.spawn_*` + `socket.async_*` will continue to work
when v0.4 swaps the internal scheduler for a `mio`/`polling`-based
single-threaded event loop and the OS-thread cost vanishes.

**v0.4 swap plan**:

* Add a single global `EventLoop` to `SharedVm` (replaces the
  per-task `JoinHandle`).
* `spawn_*` becomes "register state-machine coroutine"; `socket.async_*`
  registers a non-blocking-socket interest with the loop.
* `Future.await` either picks up the cached result or runs the loop
  until the future's slot becomes ready.
* `gather_*` becomes meaningfully concurrent (today it's already
  concurrent via OS threads — v0.4 makes it concurrent on a single
  thread).

The handle shape (`Future[T]` opaque i64), the public surface, and
the typechecker treatment do not change.  v0.4 is a perf swap, not
an API swap.

#### 9.43.5 Limitations (v0.3)

* No `async` / `await` keyword — the surface is library-only.  The
  parser does not recognise `async def`; `await` is not a reserved
  word.  Adding keyword-level syntax is a v0.4 task.
* No async file I/O.  Sockets only.
* No cancellation / timeouts on Future — `await` blocks until the
  task completes or the program exits.  v0.4.
* No variadic `asyncio.gather(*futures)`.  Ship `gather_2_*` /
  `gather_3_*` / `gather_4_*` positional variants; the underlying
  monomorphisation rule is the same as the rest of the stdlib.
* `Future[T]` is not yet a fully open generic — its element type is
  pinned at the spawn-call site by the `spawn_*` variant.  Bridging
  the M31 user-defined-generic-class machinery to stdlib types is a
  v0.4 task (see §16.4 history note).
* `asyncio.sleep` blocks the calling OS thread (Shape A).  In Shape B
  the wall-clock duration matches but the OS thread is free to run
  other tasks.

### 9.44 Expanded `str` methods (v0.3 — stdlib expansion)

Beyond the core set (§8.5: `slice`, `char_at`, `split`, `strip`/
`lstrip`/`rstrip`, `find`, `replace`, `startswith`/`endswith`/
`contains`, `join`, `lower`/`upper`, `repeat`), the following CPython
`str` methods are available as receiver-method calls. They dispatch
through `NativeFn::from_name` (collision-free names) and need no
explicit ir.rs override.

```
# search (code-point indices; *index/*rindex raise ValueError if absent)
s.count(sub: str)  -> i64        s.rfind(sub: str)  -> i64
s.index(sub: str)  -> i64        s.rindex(sub: str) -> i64

# splitting
s.splitlines()        -> List[str]                  # universal newlines
s.partition(sep: str) -> Tuple[str, str, str]       # first occurrence
s.rpartition(sep:str) -> Tuple[str, str, str]       # last occurrence

# padding (width in code points; fill is ' ', or '0' for zfill)
s.zfill(width: i64)  -> str      s.center(width: i64) -> str
s.ljust(width: i64)  -> str      s.rjust(width: i64)  -> str

# case / formatting
s.title()      -> str    s.swapcase()   -> str
s.casefold()   -> str    s.capitalize() -> str

# predicates (empty string is false for digit/alpha/alnum/space)
s.isdigit() -> bool   s.isalpha() -> bool   s.isalnum() -> bool
s.isspace() -> bool   s.isupper() -> bool   s.islower() -> bool

# trimming / tabs
s.removeprefix(prefix: str) -> str    s.removesuffix(suffix: str) -> str
s.expandtabs(tabsize: i64 = 8)       -> str
```

`partition`/`rpartition` on an absent separator return `(s, "", "")`
and `("", "", s)` respectively. `zfill` keeps a leading `+`/`-` in
front of the inserted zeros. `center` biases the extra pad to the
right. See `examples/str_methods_extra_demo.spy`.

### 9.45 Module `functools` (v0.3 — stdlib expansion)

Function decorators are no-ops in StrictPy, so `lru_cache` ships as an
explicit string-keyed memo object you wrap an expensive call around.

```
fn cache_new() -> i64
fn cache_set_i64(c: i64, key: str, value: i64) -> None
fn cache_get_i64(c: i64, key: str) -> i64       # KeyError if missing
fn cache_set_f64(c: i64, key: str, value: f64) -> None
fn cache_get_f64(c: i64, key: str) -> f64
fn cache_set_str(c: i64, key: str, value: str) -> None
fn cache_get_str(c: i64, key: str) -> str
fn cache_has(c: i64, key: str)  -> bool
fn cache_len(c: i64)            -> i64
fn cache_clear(c: i64)         -> None
fn cache_hits(c: i64)          -> i64           # hit counter (introspection)
```

Idiom: `if not functools.cache_has(c, k): functools.cache_set_i64(c, k,
compute())` then `functools.cache_get_i64(c, k)`. See
`examples/functools_demo.spy`.

### 9.46 Module `enum` (v0.3 — stdlib expansion)

A registry-backed IntEnum-style facility: the static model has no
class-with-attributes Enum, so each enum is a namespace mapping member
names to i64 values, looked up in either direction.

```
fn new() -> i64
fn add(e: i64, member: str, value: i64) -> None
fn value_of(e: i64, member: str) -> i64        # KeyError if absent
fn name_of(e: i64, value: i64)   -> str        # ValueError if absent
fn has_name(e: i64, member: str) -> bool
fn len(e: i64) -> i64
```

`name_of` returns the first member registered with that value (insertion
order). See `examples/enum_demo.spy`.

### 9.47 Module `bytearray` (v0.3 — stdlib expansion)

A growable, mutable byte buffer behind an i64 handle. Bytes are
`0..=255`; out-of-range writes raise `ValueError`, out-of-range indices
raise `IndexError`. Negative indices count from the end.

```
fn new() -> i64
fn from_str(s: str) -> i64                      # UTF-8 bytes of s
fn append(b: i64, byte: i64) -> None
fn get(b: i64, idx: i64) -> i64
fn set(b: i64, idx: i64, byte: i64) -> None
fn len(b: i64) -> i64
fn to_str(b: i64) -> str                        # ValueError if not UTF-8
fn hex(b: i64) -> str                           # lowercase, 2 chars/byte
fn pop(b: i64) -> i64                           # IndexError if empty
fn clear(b: i64) -> None
```

See `examples/bytearray_demo.spy`.

### 9.48 Module `decimal` (v0.3 — stdlib expansion)

Exact fixed-point arithmetic: a value is `mantissa * 10^-scale` stored
as an `i128` mantissa + scale behind an i64 handle. `0.1 + 0.2` is
exactly `0.3` — no binary-float drift. Add/sub align scales; mul adds
scales.

```
fn from_str(s: str) -> i64                      # ValueError on bad literal
fn add(a: i64, b: i64) -> i64
fn sub(a: i64, b: i64) -> i64
fn mul(a: i64, b: i64) -> i64
fn to_str(d: i64) -> str                        # exact rendering
fn to_f64(d: i64) -> f64
fn cmp(a: i64, b: i64) -> i64                   # -1 / 0 / 1
```

No division in v0.3 (would need rounding-mode policy). See
`examples/decimal_fractions_demo.spy`.

### 9.49 Module `fractions` (v0.3 — stdlib expansion)

Exact rationals kept in lowest terms with a positive denominator,
behind an i64 handle.

```
fn new(num: i64, den: i64) -> i64               # ZeroDivisionError if den==0
fn add(a: i64, b: i64) -> i64
fn sub(a: i64, b: i64) -> i64
fn mul(a: i64, b: i64) -> i64
fn div(a: i64, b: i64) -> i64                   # ZeroDivisionError on 0 frac
fn num(f: i64) -> i64
fn den(f: i64) -> i64
fn to_str(f: i64) -> str                        # "num/den"
```

Results are automatically reduced (e.g. `2/3 * 3/4` → `1/2`). See
`examples/decimal_fractions_demo.spy`.

### 9.50 Module `unittest` (v0.3 — stdlib expansion)

A minimal assertion collector + runner matching how the repo structures
its own `.spy` checks. Make a result accumulator, record `assert_*`
outcomes, then `run` to print a summary and get the failure count back
(0 == all green). Each `assert_*` returns a bool (true == passed) and
records into the accumulator.

```
fn new() -> i64
fn assert_true(t: i64, cond: bool, label: str) -> bool
fn assert_eq_i64(t: i64, got: i64, want: i64, label: str) -> bool
fn assert_eq_f64(t: i64, got: f64, want: f64, label: str) -> bool   # exact
fn assert_eq_str(t: i64, got: str, want: str, label: str) -> bool
fn run(t: i64)      -> i64                       # prints summary; returns #fail
fn failures(t: i64) -> i64
fn ran(t: i64)      -> i64
```

`run` prints each failure as `FAIL: <message>` then a summary line
`ran N checks: OK` or `ran N checks: M FAILED`. See
`examples/unittest_demo.spy`.

---

## 10. Compiler Architecture

### 10.1 Pipeline

```
.spy source
    │
    ▼
┌────────────┐
│   Lexer    │ → token stream
├────────────┤
│   Parser   │ → untyped AST
├────────────┤
│  Resolver  │ → AST with name bindings (scope info)
├────────────┤
│ Type checker│ → typed AST
├────────────┤
│  IR lower  │ → typed SSA IR
├────────────┤
│ Optimizer  │ → optimized IR
├────────────┤
│ Codegen    │ → typed bytecode (.spyc)
└────────────┘
```

### 10.2 Lexer

State machine emitting `Token { kind, lexeme, line, col, byte_offset }`. Handles indentation by tracking a stack of indent levels and emitting `INDENT`/`DEDENT` synthetic tokens. F-strings are split into a sequence: `FSTR_START`, `STR_CHUNK`, `FSTR_EXPR_START`, expression tokens, `FSTR_EXPR_END`, ... `FSTR_END`.

### 10.3 Parser

Recursive descent with Pratt-style expression parsing. Produces a strongly-typed AST (one Rust/Zig/Python enum variant per syntactic form, see §10.6).

### 10.4 Type checker

Bi-directional type checking:
- **Synthesis mode**: compute the type of an expression bottom-up.
- **Checking mode**: verify that an expression has an expected type (used when a context provides one, e.g., function arguments).

Generic inference: for each call to `f[T](args)`, solve for `T` using local unification:
- Walk parameters and arguments in order; unify `param_type[T := ?T_var]` with `arg_type`.
- Each `?T_var` accumulates equality constraints.
- After all arguments are seen, each `?T_var` must have a unique solution; otherwise the user must supply explicit type args.

Pseudocode:
```
fn check_call(fn_decl, type_args, args):
    if type_args supplied:
        substitution = zip(fn_decl.type_params, type_args)
    else:
        substitution = {tp: fresh_var() for tp in fn_decl.type_params}
    for (param, arg) in zip(fn_decl.params, args):
        expected = substitute(param.type, substitution)
        actual = synth(arg)
        unify(expected, actual, substitution)
    return substitute(fn_decl.return_type, substitution)
```

### 10.5 IR lowering

The typed AST is lowered to a Sea-of-Nodes or basic-block SSA IR (see §11). Key transformations:
- Desugar `for x in iter: body` → `__iter__()` + `__next__()` loop.
- Desugar `with` → `try`/`finally` calling `__enter__`/`__exit__`.
- Desugar augmented assignments.
- Desugar comprehensions to explicit loops.
- Insert nullness checks where the type system requires them.
- Insert bounds checks on indexed access.

### 10.6 AST node sketch (pseudocode)

```
Expr =
  | IntLit { value: i128, ty: PrimType }
  | FloatLit { value: f64, ty: PrimType }
  | StrLit { value: String }
  | BoolLit { value: bool }
  | NoneLit
  | Var { name: Symbol, ty: Type }
  | BinOp { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr>, ty: Type }
  | UnaryOp { op: UnaryOp, operand: Box<Expr>, ty: Type }
  | Call { callee: Box<Expr>, args: Vec<Expr>, ty: Type }
  | MethodCall { receiver: Box<Expr>, method: Symbol, args: Vec<Expr>, ty: Type }
  | Attr { obj: Box<Expr>, name: Symbol, ty: Type, offset: u32 }
  | Index { obj: Box<Expr>, idx: Box<Expr>, ty: Type }
  | New { class: TypeId, args: Vec<Expr> }
  | Cast { expr: Box<Expr>, target: Type }
  | Lambda { params: Vec<Param>, body: Block, captures: Vec<Symbol>, ty: Type }
  | If { cond: Box<Expr>, then_branch: Box<Expr>, else_branch: Box<Expr>, ty: Type }
  | Match { scrutinee: Box<Expr>, arms: Vec<MatchArm>, ty: Type }

Stmt =
  | Let { name: Symbol, ty: Type, init: Expr }
  | Assign { target: Lvalue, value: Expr }
  | Return { value: Option<Expr> }
  | If { cond: Expr, then_block: Block, else_block: Option<Block> }
  | While { cond: Expr, body: Block }
  | For { var: Symbol, var_ty: Type, iter: Expr, body: Block }
  | Try { body: Block, handlers: Vec<Handler>, finally_block: Option<Block> }
  | Raise { exc: Expr }
  | Expr { expr: Expr }
  | Break
  | Continue
  | Pass
```

### 10.7 Optimization passes (suggested order)

1. **Constant folding & propagation**
2. **Dead code elimination**
3. **Inlining** of small (heuristic: <32 IR nodes) and `@inline` functions
4. **Devirtualization**: replace `CALL_VIRTUAL` with `CALL_DIRECT` when the receiver's class is statically known
5. **Escape analysis & scalar replacement**: stack-allocate or unbox objects that don't escape
6. **Common subexpression elimination**
7. **Loop-invariant code motion**
8. **Bounds-check elimination** when index is provably in range
9. **Null-check elimination** after narrowing
10. **Register allocation** (linear-scan)
11. **Peephole pass** on bytecode

### 10.8 Command-line driver (v0.2 — M25)

StrictPy ships a single `spy` binary modelled on CPython's `python`
command. There is no separate compiler binary — compile-only invocations
are served via the `--compile-only` flag.

```
spy SCRIPT [ARGS...]                # compile-if-stale + run
spy -c CODE [ARGS...]               # compile inline + run
spy --compile-only SCRIPT [-o OUT]  # compile only; do not execute
```

* **`SCRIPT`** may end in `.spy` (StrictPy source) or `.spyc` (already-
  compiled bytecode). The driver dispatches on the extension:
    * `.spy` — compile if the cache is missing or stale, then run.
    * `.spyc` — load and run directly; no cache lookup.
  Any other extension is an error.
* **Bytecode cache** — when `.spy` source is given, the driver caches
  the produced bytecode at `<dir-of-source>/__spycache__/<basename>.spyc`
  (mirroring CPython's `__pycache__/foo.cpython-NNN.pyc` convention).
  The cache directory is created on first run if absent.
* **Staleness rule** — the cache is reused iff the cached `.spyc`
  exists AND `source mtime <= cache mtime`. Any inequality the other
  way forces a recompile. This is the same rule CPython uses for
  `.pyc` files (modulo CPython's magic-number / size / hash variants).
  Cache rewrites are atomic-ish: `compile_file` writes the new bytes
  via a single `fs::write` call.
* **`-c CODE`** — compiles the literal string as a one-shot StrictPy
  program and runs it. The program must define `fn main() -> i32:`.
  `-c` is never cached.
* **`--compile-only SCRIPT [-o OUT]`** — produces a `.spyc` and exits.
  When `-o OUT` is omitted, the output goes to the same
  `__spycache__/<basename>.spyc` path the run mode would use. The
  source is NOT executed. This is the analogue of
  `python -m py_compile script.py`.
* **`sys.argv[0]`** is the script path the user typed (NOT the
  cached `.spyc` path). For `-c` mode it is the literal string
  `"-c"`. Trailing tokens become `sys.argv[1..]`.

The compiler exposes the same operations as a library
(`strictpy_compiler::compile_file` / `compile_source`) for in-process
tooling that wants to skip the binary.

---

## 11. Intermediate Representation

### 11.1 Form

Three-address SSA over basic blocks. Each function is a CFG.

### 11.2 Value model

```
Value = {
    id:    u32           // SSA id, unique within function
    ty:    Type          // every value typed
    kind:  ValueKind
}

ValueKind =
  | Const { lit: Literal }
  | Param { idx: u32 }
  | Op { op: IROp, args: Vec<ValueId> }
  | Phi { incoming: Vec<(BlockId, ValueId)> }
```

### 11.3 IR operations

(Selected; complete list in implementation.)

```
Arithmetic:   IAdd, ISub, IMul, IDiv, IRem, INeg, IShl, IShr, IAnd, IOr, IXor, INot
              FAdd, FSub, FMul, FDiv, FNeg
Conversion:   IExt, ITrunc, FtoI, ItoF, FExt, FTrunc, Bitcast
Comparison:   IEq, INe, ILt, ILe, IGt, IGe, FEq, FNe, FLt, FLe, FGt, FGe
Memory:       Alloc{class}, Load{offset, ty}, Store{offset, ty},
              ArrayNew{elem_ty, len}, ArrayGet, ArraySet, ArrayLen
Control:      Branch{block}, CondBranch{cond, true_block, false_block},
              Ret{value?}, Throw{exc}, Catch{exc_var, body, recovery}
Call:         DirectCall{fn_id, args}, VirtualCall{vtable_slot, recv, args},
              IfaceCall{itable_id, slot, recv, args}, IndirectCall{fn_value, args}
Generic:      Phi, Copy, Select{cond, true, false}
Safety:       BoundsCheck{idx, len}, NullCheck{ptr}, TypeCheck{obj, type_id}
```

### 11.4 Example IR

Source:
```python
fn dot(a: List[f64], b: List[f64]) -> f64:
    sum: f64 = 0.0
    i: i64 = 0
    while i < len(a):
        sum = sum + a[i] * b[i]
        i = i + 1
    return sum
```

After lowering & optimization:
```
fn dot(v0: List<f64>, v1: List<f64>) -> f64:
  bb0:
    v2 = ArrayLen v0
    v3 = FConst 0.0
    v4 = IConst 0
    Branch bb1
  bb1:
    v5 = Phi (bb0 -> v3, bb2 -> v9)
    v6 = Phi (bb0 -> v4, bb2 -> v10)
    v7 = ILt v6, v2
    CondBranch v7, bb2, bb3
  bb2:
    // bounds check elided: v6 < v2 proven
    v8a = ArrayGet v0, v6
    v8b = ArrayGet v1, v6
    v8  = FMul v8a, v8b
    v9  = FAdd v5, v8
    v10 = IAdd v6, IConst 1
    Branch bb1
  bb3:
    Ret v5
```

---

## 12. Bytecode File Format

### 12.1 File extension

`.spyc` (compiled module). Multiple modules can be packed into a `.spya` archive.

### 12.2 Endianness

Little-endian throughout.

### 12.3 Top-level layout

```
+----------------------+
| Header               |  32 bytes
+----------------------+
| Constant pool        |  variable
+----------------------+
| Type table           |  variable
+----------------------+
| Function table       |  variable
+----------------------+
| Code section         |  variable
+----------------------+
| String table         |  variable
+----------------------+
| Debug info (opt)     |  variable
+----------------------+
```

### 12.4 Header (32 bytes)

```
offset  size  field
0       4     magic              0x53 0x50 0x59 0x43   ("SPYC")
4       2     version_major      e.g., 0x0001
6       2     version_minor      e.g., 0x0000
8       4     flags              bit0: has_debug; bit1: jit_pre-warmed
12      4     const_pool_offset
16      4     type_table_offset
20      4     function_table_offset
24      4     code_offset
28      4     string_table_offset
```

### 12.5 Constant pool entry

```
1 byte  tag
N bytes payload
```

Tags:
```
0x01  i32      4 bytes
0x02  i64      8 bytes
0x03  u32      4 bytes
0x04  u64      8 bytes
0x05  f32      4 bytes
0x06  f64      8 bytes
0x07  string   { u32 string_table_idx }
0x08  bytes    { u32 length; bytes... }
0x09  char     4 bytes
0x0A  bigint   { u32 length; bytes (two's complement) }
```

### 12.6 Type table entry

```
u32  type_id
u8   kind        (0=primitive, 1=class, 2=protocol, 3=tuple, 4=function, 5=list, 6=dict, 7=set, 8=nullable, 9=generic_inst)
u32  name_idx    (into string table)
u32  size        (bytes, for layout)
u16  field_count
u16  vtable_len
u32  base_type   (or 0xFFFFFFFF)
[FieldInfo × field_count]
[u32 method_fn_id × vtable_len]
[ItableEntry × itable_count]

FieldInfo = { u32 name_idx; u32 type_id; u32 offset; }
ItableEntry = { u32 protocol_type_id; u32 itable_fn_ids_offset; u16 method_count; }
```

### 12.7 Function table entry

```
u32  fn_id
u32  name_idx
u32  type_id          (function type)
u32  code_offset      (within code section)
u32  code_length
u16  num_params
u16  num_locals
u16  num_registers    (max register file high-water mark)
u16  flags            (bit0=public; bit1=pure; bit2=jit_hot)
u32  exception_table_offset
u32  debug_info_offset (or 0)
```

### 12.8 Exception table entry

```
u32  start_pc
u32  end_pc
u32  handler_pc
u32  caught_type_id   (0xFFFFFFFF = catch-all)
```

### 12.9 Code section

Concatenation of function bodies, each a sequence of bytecode instructions (§13).

### 12.10 String table

Sequence of length-prefixed UTF-8 strings:
```
[u32 byte_length][bytes...]
```
Referenced by index. Strings are deduplicated by the compiler.

---

## 13. Opcode Reference

### 13.1 Instruction encoding

Variable length. Each instruction:
```
1 byte  opcode
N bytes operands (per opcode spec)
```

Registers are encoded as `u16` (max 65535 per function).
Constant indices as `u32`.
Branch offsets as signed `i32` relative to next instruction.

### 13.2 Register conventions

Each call frame has a contiguous register file. Registers are typed by static analysis; the VM does not tag them at runtime. The compiler ensures that opcodes match the static type of their register operands.

### 13.3 Opcode categories and full list

#### 13.3.1 Constants & moves

```
0x01  CONST_I32     dst:r16, val:u32
0x02  CONST_I64     dst:r16, idx:u32         (const pool)
0x03  CONST_F32     dst:r16, val:u32
0x04  CONST_F64     dst:r16, idx:u32
0x05  CONST_STR     dst:r16, idx:u32         (string table)
0x06  CONST_TRUE    dst:r16
0x07  CONST_FALSE   dst:r16
0x08  CONST_NONE    dst:r16
0x09  MOVE          dst:r16, src:r16
```

#### 13.3.2 Integer arithmetic (signed)

```
0x10  IADD_I32      dst:r16, a:r16, b:r16
0x11  ISUB_I32      dst:r16, a:r16, b:r16
0x12  IMUL_I32      dst:r16, a:r16, b:r16
0x13  IDIV_I32      dst:r16, a:r16, b:r16    // traps on div by zero
0x14  IREM_I32      dst:r16, a:r16, b:r16
0x15  INEG_I32      dst:r16, a:r16
0x16  IAND_I32      dst:r16, a:r16, b:r16
0x17  IOR_I32       dst:r16, a:r16, b:r16
0x18  IXOR_I32      dst:r16, a:r16, b:r16
0x19  ISHL_I32      dst:r16, a:r16, b:r16
0x1A  ISHR_I32      dst:r16, a:r16, b:r16
0x1B  INOT_I32      dst:r16, a:r16

0x20..0x2B          same for I64 (IADD_I64, ISUB_I64, ...)
0x30..0x3B          same for U32 (USHR is logical)
0x40..0x4B          same for U64
```

#### 13.3.3 Float arithmetic

```
0x50  FADD_F32      dst:r16, a:r16, b:r16
0x51  FSUB_F32      dst:r16, a:r16, b:r16
0x52  FMUL_F32      dst:r16, a:r16, b:r16
0x53  FDIV_F32      dst:r16, a:r16, b:r16
0x54  FNEG_F32      dst:r16, a:r16
0x58..0x5C          same for F64
```

#### 13.3.4 Comparisons

```
0x60  IEQ_I32       dst:r16, a:r16, b:r16    // dst is bool
0x61  INE_I32       ...
0x62  ILT_I32       ...
0x63  ILE_I32       ...
0x64  IGT_I32       ...
0x65  IGE_I32       ...
0x68..0x6D          same for I64
0x70..0x75          same for U32
0x78..0x7D          same for U64
0x80..0x85          same for F32
0x88..0x8D          same for F64
0x90  STR_EQ        dst:r16, a:r16, b:r16
0x91  REF_EQ        dst:r16, a:r16, b:r16    // pointer equality (is)
```

#### 13.3.5 Conversions

```
0xA0  I32_TO_I64    dst:r16, a:r16
0xA1  I64_TO_I32    dst:r16, a:r16           // traps on overflow in debug
0xA2  I32_TO_F64    dst:r16, a:r16
0xA3  F64_TO_I32    dst:r16, a:r16           // traps on overflow / NaN
0xA4  F32_TO_F64    dst:r16, a:r16
0xA5  F64_TO_F32    dst:r16, a:r16
0xA6  I32_TO_BIGINT dst:r16, a:r16
0xA7  BOOL_TO_I32   dst:r16, a:r16
...
```

#### 13.3.6 Object / heap

```
0xB0  NEW           dst:r16, type_id:u32                     // allocate, call no init
0xB1  NEW_INIT      dst:r16, type_id:u32, fn_id:u32, argc:u8, args:r16×argc
0xB2  LOAD_FIELD    dst:r16, obj:r16, offset:u32, ty_tag:u8
0xB3  STORE_FIELD   obj:r16, offset:u32, src:r16, ty_tag:u8
0xB4  LOAD_VTABLE   dst:r16, obj:r16                         // for is_instance checks
0xB5  IS_INSTANCE   dst:r16, obj:r16, type_id:u32
0xB6  CAST_CHECKED  dst:r16, obj:r16, type_id:u32            // throws on failure
0xB7  NULL_CHECK    obj:r16                                  // throws NullPointerError
```

`ty_tag` values: 0=i8, 1=i16, 2=i32, 3=i64, 4=u8, ..., 7=u64, 8=f32, 9=f64, 10=bool, 11=ref.

#### 13.3.7 Arrays

```
0xC0  ARRAY_NEW     dst:r16, elem_type_id:u32, len:r16
0xC1  ARRAY_LEN     dst:r16, arr:r16
0xC2  ARRAY_GET     dst:r16, arr:r16, idx:r16, ty_tag:u8   // does bounds check
0xC3  ARRAY_SET     arr:r16, idx:r16, src:r16, ty_tag:u8
0xC4  ARRAY_GET_UNCHECKED  dst:r16, arr:r16, idx:r16, ty_tag:u8
0xC5  ARRAY_SET_UNCHECKED  arr:r16, idx:r16, src:r16, ty_tag:u8
```

The `_UNCHECKED` variants are emitted by the optimizer after bounds-check elimination.

#### 13.3.8 Calls

```
0xD0  CALL_DIRECT   dst:r16, fn_id:u32, argc:u8, args:r16×argc
0xD1  CALL_VIRTUAL  dst:r16, recv:r16, vtable_slot:u16, argc:u8, args:r16×argc
0xD2  CALL_IFACE    dst:r16, recv:r16, iface_id:u32, slot:u16, argc:u8, args:r16×argc
0xD3  CALL_INDIRECT dst:r16, fn:r16, argc:u8, args:r16×argc           // closure / fn ptr
0xD4  TAIL_CALL_DIRECT     fn_id:u32, argc:u8, args:r16×argc
0xD5  CALL_NATIVE   dst:r16, native_id:u32, argc:u8, args:r16×argc    // FFI
```

`dst:r16 = 0xFFFF` indicates the result is discarded (for `-> None` functions).

#### 13.3.9 Control flow

```
0xE0  JUMP          offset:i32
0xE1  JUMP_IF       cond:r16, offset:i32         // jump if true
0xE2  JUMP_IF_NOT   cond:r16, offset:i32
0xE3  RET           src:r16
0xE4  RET_VOID
0xE5  THROW         exc:r16
0xE6  ENTER_TRY     handler_table_idx:u32        // pushes handler frame
0xE7  LEAVE_TRY                                  // pops handler frame
0xE8  RETHROW
0xE9  SWITCH        scrutinee:r16, table_offset:u32, n_cases:u16
                    // followed by n_cases × (i32 value, i32 offset)
```

#### 13.3.10 Lists, dicts, strings (builtin operations)

```
0xF0  LIST_NEW      dst:r16, elem_type_id:u32, capacity:u32
0xF1  LIST_PUSH     list:r16, value:r16, ty_tag:u8
0xF2  LIST_POP      dst:r16, list:r16, ty_tag:u8
0xF3  DICT_NEW      dst:r16, key_type:u32, val_type:u32
0xF4  DICT_GET      dst:r16, dict:r16, key:r16
0xF5  DICT_SET      dict:r16, key:r16, value:r16
0xF6  DICT_HAS      dst:r16, dict:r16, key:r16
0xF7  STR_CONCAT    dst:r16, a:r16, b:r16
0xF8  STR_LEN       dst:r16, s:r16
0xF9  STR_CHAR_AT   dst:r16, s:r16, idx:r16
```

#### 13.3.11 Closures

```
0xFA  CLOSURE_NEW   dst:r16, fn_id:u32, capture_n:u8, captures:r16×capture_n
0xFB  CLOSURE_CALL  dst:r16, closure:r16, argc:u8, args:r16×argc
```

#### 13.3.12 GC / runtime hooks

```
0xFC  GC_SAFEPOINT
0xFD  GC_WRITE_BARRIER  obj:r16, field_offset:u32, value:r16    // for generational GC
0xFE  DEBUG_NOP     line:u32, col:u16                            // debug info marker
0xFF  HALT                                                       // unreachable / trap
```

(Numbering of categories has spare ranges; specific opcode numbers may shift in implementation.)

---

## 14. Virtual Machine

### 14.1 Architecture

```
┌─────────────────────────────────────────────────┐
│                 Bytecode Loader                 │
│   (verifies .spyc, links types, resolves refs)  │
├─────────────────────────────────────────────────┤
│              Threaded Interpreter               │
│   (direct-threaded dispatch; one handler per    │
│    opcode; tail-calls if compiler supports)     │
├─────────────────────────────────────────────────┤
│              Baseline JIT (tier 1)              │
│   (per-method, after N invocations)             │
├─────────────────────────────────────────────────┤
│              Optimizing JIT (tier 2)            │
│   (uses Cranelift or LLVM)                      │
├─────────────────────────────────────────────────┤
│         Object Model  │  Garbage Collector      │
├───────────────────────┴─────────────────────────┤
│              Native FFI / Syscalls              │
└─────────────────────────────────────────────────┘
```

### 14.2 Interpreter

Direct-threaded dispatch. Each opcode handler ends by computing the next opcode's address and jumping to it (via `goto *handler_table[next_op]` in C, or `&&label`-style computed goto). Where the host language supports tail-call optimization (e.g., Wasm, MIR, Cranelift), each opcode is a function that tail-calls the next.

The interpreter loop maintains:

```
struct Frame {
    fn_id:        u32
    pc:           *u8           // instruction pointer
    registers:    *u8           // typed register file
    return_addr:  *Frame
    return_reg:   u16
    handler_top:  u32           // top of handler stack
}
```

Each thread has its own stack of frames.

### 14.3 Register file

The register file for a frame is a single contiguous byte buffer. Each register occupies 8 bytes regardless of type (smaller types occupy the low bits; the compiler tracks the actual type per register statically). References are 8-byte pointers.

Rationale: simplifies frame layout; cost of "wasting" upper bits is negligible. Alternative is a "typed register file" with separate `i32[]`, `i64[]`, `f64[]`, `ref[]` arrays — slower indexing, less cache-friendly.

### 14.4 Dispatch

#### Direct call
Compile-time known function. Push frame, copy arguments, jump to code.

#### Virtual call
Receiver's vtable pointer is loaded from object header. Method address is fetched at the known vtable slot. Jump.

```
fn_ptr = receiver->vtable[slot]
call fn_ptr(receiver, args...)
```

#### Interface (protocol) call
Receiver's TypeInfo has an itable mapping protocol IDs to method tables. Lookup:

```
itable = find_itable(receiver->vtable, protocol_id)   // small linear scan, usually 1–4 entries
fn_ptr = itable[slot]
call fn_ptr(receiver, args...)
```

Optimization: when a protocol type is monomorphized to a single concrete type via the optimizer, the call is rewritten to direct.

### 14.5 JIT tiers

- **Tier 0**: Interpreter. All code starts here.
- **Tier 1 (Baseline JIT)**: Triggered after a function is called N times (default 100) or contains a loop with K iterations (default 10000). Emits straightforward native code using a register allocator that mirrors the bytecode register file. No deep optimizations; main wins are dispatch removal and inlining small primitives.
- **Tier 2 (Optimizing JIT)**: Triggered after T1 code has executed M times. Lifts the typed bytecode back to an SSA IR (same as compiler IR) and runs aggressive passes plus an LLVM/Cranelift backend.

Because all types are static, the JIT does **not** need speculation, deoptimization, or guard insertion (unlike JS/Python JITs). This is the central engineering simplification.

### 14.6 Calling convention (JIT)

System V AMD64 / Windows x64 calling convention for native code. References live in pointer-width GP registers. Floats live in XMM. The runtime guarantees stack alignment for FFI.

---

## 15. Garbage Collector

### 15.1 Algorithm

**Generational mark-and-sweep** with a copying young generation. Three generations: young (eden + survivor), tenured, large-object space (objects > 32 KB allocated directly into LOS).

Rationale: generational hypothesis holds strongly for Python-like workloads; copying young gen gives O(survivor) collection cost; mark-and-sweep on tenured avoids moving long-lived objects (cheaper barriers).

### 15.2 Allocation

Bump-pointer in TLAB (thread-local allocation buffer). On overflow, request a new TLAB or trigger a young-gen collection.

### 15.3 Write barrier

Card-marking. The heap is divided into 512-byte cards. Storing a reference into a tenured object marks the corresponding card dirty. Young-gen collections scan dirty cards for inter-generational roots.

The `GC_WRITE_BARRIER` opcode (or its JIT equivalent) is emitted by the compiler at every field store that writes a reference into a possibly-tenured object.

### 15.4 Roots

- All thread stacks (frame register files + live-reference maps emitted by the compiler per call site).
- Global module variables.
- Native runtime references (registered via FFI).

### 15.5 Safepoints

Threads must periodically reach a safepoint for GC to run. Safepoints are inserted:
- At every back-edge in a loop.
- At every function call return.
- At allocation sites.

Polled via the `GC_SAFEPOINT` opcode (or a memory-protected page in JIT-ed code).

### 15.6 Finalization

Avoided. Resources are managed via `with` blocks. There is no `__del__`. (Open: weak references — deferred to v0.2.)

### 15.7 Implementation status (v0.3)

The v0.3 VM ships a **stop-the-world conservative mark-and-sweep** that approximates the spec's generational design enough to be memory-safe under the current benchmark and acceptance workloads. The specification above describes the v1.x design target; the milestones listed here trace which subset is live.

- **M4** — conservative mark-and-sweep over a flat live-object list (no generations, no TLAB, no card marking). Roots: interpreter frame register files plus dict side-table values, all scanned conservatively (any aligned u64 that aliases a live allocation marks that object live).
- **M9** — Cranelift JIT lands. JIT'd code holds heap pointers in CPU registers that the conservative scan can't see, so the GC was *paused* whenever any JIT'd frame was on the stack (via the `in_jit: AtomicUsize` counter on `SharedVm`). Correct but pathological for long-running allocation-heavy JIT code: the heap could grow unbounded.
- **M33** — replaced the `in_jit` pause with **precise stack maps via per-thread shadow stack**. Each JIT'd function allocates a Cranelift stack slot sized to its register file; before every heap-allocating runtime helper (`rt_alloc`, `rt_list_*`, `rt_array_new`, `rt_virtual_call`, the native trampoline, `strictpy_alloc_str_const`) the JIT spills every register variable into that slot and pushes a `(buf, len)` window onto the per-thread shadow stack via `rt_shadow_push`; it pops after the helper returns. The GC scans every published window in addition to the interpreter's frame register files. GC now runs even while JIT'd code is on the stack. The Cranelift `enable_safepoints` / `declare_value_needs_stack_map` path was evaluated and deferred to v0.4: the shadow stack ships the same correctness property for ~200 LOC of book-keeping vs. machine-stack walking, at the cost of one conservative spill before each allocation-call (measured cost <5 ns per spill on x86_64).
- Still deferred to v1.x: generational layout, TLABs, card-marking write barrier, safepoints on long-loop back-edges (a pure-compute JIT'd loop with no allocations still pauses other threads until it finishes), and any moving / compacting collector. v0.3's heap layout (object-header at offset 0, vtable pointer first, `GcKind` discriminator on the type table) assumes non-moving.

---

## 16. Concurrency Model

### 16.1 Threads

`Thread` is a runtime class:

```python
class Thread:
    fn __init__(self, target: fn() -> None) -> None: ...
    fn start(self) -> None: ...
    fn join(self) -> None: ...
```

No GIL. Threads are OS threads.

### 16.2 Memory model

Sequential consistency for data-race-free programs. Races on primitive types are well-defined (tearing may occur for types wider than the platform's atomic-access width). Races on reference types are well-defined to read either the pre- or post-write value (atomic pointer writes assumed).

`Atomic[T]` provides explicit atomic operations:

```python
class Atomic[T: Numeric]:
    fn load(self) -> T
    fn store(self, value: T) -> None
    fn cas(self, expected: T, new: T) -> bool
    fn fetch_add(self, delta: T) -> T
```

### 16.3 Channels

```python
class Channel[T]:
    fn __init__(self, capacity: i32) -> None
    fn send(self, value: T) -> None
    fn recv(self) -> T
    fn try_recv(self) -> T?
    fn close(self) -> None
```

### 16.4 Async (deferred to v0.2)

`async`/`await` reserved syntax but not implemented in v0.1.

---

## 17. Foreign Function Interface

### 17.1 Declaring an extern function

```python
@extern("c")
fn strlen(s: *u8) -> usize

@extern("c", "libm")
fn cos(x: f64) -> f64
```

### 17.2 Pointer types

```
*T        // mutable raw pointer
*const T  // immutable raw pointer
```

Pointer types are **unsafe** — operations on them require the function to be marked `@unsafe`:

```python
@unsafe
fn deref(p: *i32) -> i32:
    return p[0]
```

### 17.3 Calling convention

Default `"c"` calling convention. Strings passed across FFI as `*u8` with a length (no implicit null-termination).

### 17.4 Loading dynamic libraries

```python
final lib: DynLib = DynLib.load("libfoo.so")
fn_ptr: fn(i32) -> i32 = lib.symbol("foo")
```

---

## 18. Error Model & Diagnostics

### 18.1 Compiler errors

Each error has:
- A stable error code: `E0001`, `E0002`, ...
- A primary location (file:line:col + span).
- Optional secondary spans (e.g., where the conflicting declaration is).
- A short message and an optional detailed explanation.

Example output:
```
error[E0123]: type mismatch in assignment
  --> example.spy:14:5
   |
14 |     count = "ten"
   |             ^^^^^ expected `i32`, found `str`
   |
help: convert the string with `i32.parse(...)`
```

### 18.2 Diagnostic categories

- `E0xxx`: parse errors
- `E1xxx`: name resolution
- `E2xxx`: type errors
- `E3xxx`: semantic errors (e.g., non-exhaustive match)
- `E4xxx`: linker / module errors
- `W0xxx`: warnings (e.g., unused variable)

Selected type-error codes introduced for the v1 correctness pass (all `E2xxx`):

| Code    | Meaning                                                              |
|---------|---------------------------------------------------------------------|
| `E2050` | `raise X from Y` / `except X` where the type is not an `Exception`   |
| `E2070` | `with EXPR` where `EXPR` is not a supported (`io.File`) context mgr  |
| `E2071` | unknown/unsupported decorator (no v1 decorator semantics)           |
| `E2072` | `Dict[K, V]` with a non-`str` key type `K`                          |

### 18.3 Runtime errors

Mapped to exception classes. Runtime constructs a stack trace using the debug info section of `.spyc` files.

---

## 19. Implementation Roadmap

### Milestone 1: Lexer + Parser (Week 1–2)
- Tokenizer with indent tracking.
- Recursive-descent parser producing AST.
- Pretty-printer for AST round-tripping.
- Deliverable: parses `examples/*.spy` without errors.

### Milestone 2: Type Checker (Week 3–5)
- Symbol table & scoping.
- Type representation.
- Bidirectional type checking for the core expression and statement set.
- Generic monomorphization.
- Deliverable: rejects ill-typed programs; accepts well-typed ones; emits typed AST.

### Milestone 3: IR & Bytecode Emission (Week 6–8)
- AST → typed SSA IR.
- Basic optimization passes (constant folding, DCE, simple inlining).
- IR → bytecode emitter.
- `.spyc` writer.
- Deliverable: produces runnable bytecode for examples.

### Milestone 4: Interpreter (Week 9–11)
- `.spyc` loader and verifier.
- Direct-threaded interpreter loop.
- Object model: allocation, fields, methods, vtables.
- Built-in `List`, `Dict`, `str`.
- Basic GC (mark-and-sweep, stop-the-world).
- Deliverable: runs all example programs end-to-end.

### Milestone 5: Standard Library (Week 12–13)
- `builtins`, `math`, `io`, `collections`, `result`.
- FFI for system calls.

### Milestone 6: Optimizing passes & better GC (Week 14–17)
- Devirtualization, escape analysis, scalar replacement, BCE.
- Generational GC with write barriers.
- Threading + channels.

### Milestone 7: JIT (Week 18–24)
- Baseline JIT producing native code.
- Profiling counters for tier-up.
- Optimizing JIT via Cranelift.

### Recommended implementation languages

- **Compiler**: Rust (good ADTs, fast, mature parsing tooling) or OCaml.
- **VM**: C, Rust, or Zig. C for portability and direct-threaded dispatch via `&&` labels; Zig for safer alternative; Rust if accepting that the dispatch loop uses `unsafe`.

---

## 20. Conformance Tests

The reference test suite covers:

### 20.1 Lexer tests
- All token kinds.
- Indent/dedent edge cases.
- F-string parsing.
- Invalid sources (tab indent, unterminated string).

### 20.2 Parser tests
- Each grammar production.
- Pretty-print round-trip preserves semantics.

### 20.3 Type checker tests
- Positive: well-typed programs accepted.
- Negative: each error code triggered at least once.
- Generic inference corner cases.
- Subtype lattice tests.

### 20.4 IR / bytecode tests
- Each IR op lowers to expected bytecode.
- Optimization passes produce expected IR (golden file comparison).
- `.spyc` files round-trip via reader/writer.

### 20.5 Runtime tests
- Each opcode executes correctly.
- GC stress tests: allocate-heavy loops with cycle detection.
- Threading: producer-consumer with channels.
- FFI: calling C `cos`, `strlen`, etc.

### 20.6 End-to-end programs
A directory of `examples/` each with expected stdout. Test runner compiles each, runs the resulting bytecode, and diffs output.

Suggested initial examples:
1. `hello.spy` — println.
2. `fib.spy` — recursive fib.
3. `dot.spy` — vector dot product.
4. `wordcount.spy` — reads a file, counts words via Dict.
5. `tree.spy` — binary tree with virtual methods.
6. `producer.spy` — two threads, one channel.
7. `mandelbrot.spy` — escape-time fractal in nested loops.

---

## 21. Examples

### 21.1 Hello world

```python
fn main() -> i32:
    println("Hello, StrictPy!")
    return 0
```

### 21.2 Generic container with protocol bound

```python
protocol Comparable:
    fn __lt__(self, other: Self) -> bool

fn max_in[T: Comparable](items: List[T]) -> T?:
    if len(items) == 0:
        return none
    best: T = items[0]
    i: i64 = 1
    while i < len(items):
        if best.__lt__(items[i]):
            best = items[i]
        i = i + 1
    return best

fn main() -> i32:
    nums: List[i32] = [3, 1, 4, 1, 5, 9, 2, 6]
    result: i32? = max_in[i32](nums)
    if result is not none:
        println("max = " + str(result))
    return 0
```

### 21.3 Class hierarchy with virtual dispatch

```python
open class Shape:
    open fn area(self) -> f64:
        return 0.0

final class Circle(Shape):
    radius: f64
    fn __init__(self, r: f64) -> None:
        self.radius = r
    fn area(self) -> f64:
        return 3.14159 * self.radius * self.radius

final class Square(Shape):
    side: f64
    fn __init__(self, s: f64) -> None:
        self.side = s
    fn area(self) -> f64:
        return self.side * self.side

fn total_area(shapes: List[Shape]) -> f64:
    sum: f64 = 0.0
    i: i64 = 0
    while i < len(shapes):
        sum = sum + shapes[i].area()
        i = i + 1
    return sum
```

### 21.4 Threads + channel

```python
from threading import Thread, Channel

fn producer(ch: Channel[i32]) -> None:
    i: i32 = 0
    while i < 100:
        ch.send(i)
        i = i + 1
    ch.close()

# Blocking `recv()` + ChannelClosedError is the reliable drain idiom:
# `try_recv` returns the same `none` for "empty" and "closed", so a
# polling consumer can exit early and leave the producer blocked forever
# on a full channel (BUGS_KNOWN.md BUG-046).
fn consumer(ch: Channel[i32]) -> None:
    running: bool = true
    while running:
        try:
            v: i32 = ch.recv()
            println("got " + str(v))
        except ChannelClosedError:
            running = false

fn main() -> i32:
    ch: Channel[i32] = Channel[i32](16)
    t1: Thread = Thread(fn() -> None: producer(ch))
    t2: Thread = Thread(fn() -> None: consumer(ch))
    t1.start()
    t2.start()
    t1.join()
    t2.join()
    return 0
```

### 21.5 FFI

```python
@extern("c", "libm")
fn sin(x: f64) -> f64

@extern("c", "libm")
fn cos(x: f64) -> f64

fn main() -> i32:
    println("sin(0) = " + str(sin(0.0)))
    println("cos(0) = " + str(cos(0.0)))
    return 0
```

---

## Appendix A: Reserved for Future Use

- `async`/`await` and async runtime.
- Weak references.
- Reflection API.
- Module hot reload.
- Effects / capabilities (`effect` keyword).
- Ownership / borrow inference (`region` keyword).
- SIMD primitive types.
- Inline assembly via `@unsafe`.

---

## Appendix B: Opcode Quick Reference Table

| Range       | Category                       |
|-------------|--------------------------------|
| 0x01–0x09   | Constants & moves              |
| 0x10–0x1B   | i32 arithmetic                 |
| 0x20–0x2B   | i64 arithmetic                 |
| 0x30–0x3B   | u32 arithmetic                 |
| 0x40–0x4B   | u64 arithmetic                 |
| 0x50–0x5C   | f32 / f64 arithmetic           |
| 0x60–0x8D   | Comparisons (all numeric types)|
| 0x90–0x91   | str / ref comparisons          |
| 0xA0–0xA7+  | Conversions                    |
| 0xB0–0xB7   | Object / heap                  |
| 0xC0–0xC5   | Arrays                         |
| 0xD0–0xD5   | Calls                          |
| 0xE0–0xE9   | Control flow                   |
| 0xF0–0xF9   | List / Dict / String builtins  |
| 0xFA–0xFB   | Closures                       |
| 0xFC–0xFF   | GC / runtime / debug / halt    |

---

## End of Specification

This document defines StrictPy v0.1. Subsequent revisions should bump the `version_minor` in the file header for backward-compatible changes and `version_major` for breaking ones.

Open design questions and prior-art notes are tracked in a separate `DESIGN_NOTES.md`.
