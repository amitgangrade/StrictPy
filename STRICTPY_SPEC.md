# StrictPy Language & Virtual Machine Specification

**Version:** 0.1 (Draft)
**Status:** Implementation reference
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
0           // i32 by default
42          // i32
0i64        // i64
0u32        // u32
0xff        // i32 hex
0b1010      // i32 binary
0o755       // i32 octal
1_000_000   // underscores allowed
```

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
f"value: {x}"       // f-string (formatted)
```

Escape sequences: `\n \r \t \\ \' \" \0 \xHH \uHHHH \U{HEX}`.

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
simple_stmt     ::= ( let_stmt | assign_stmt | return_stmt | expr_stmt
                    | break_stmt | continue_stmt | pass_stmt | raise_stmt
                    | assert_stmt | del_stmt ) NEWLINE

let_stmt        ::= identifier ":" type "=" expr
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
postfix         ::= primary { call | attr_ref | subscript | null_coalesce }
call            ::= "(" [ arg_list ] ")"
attr_ref        ::= "." identifier
subscript       ::= "[" expr { "," expr } "]"
null_coalesce   ::= "??" unary

primary         ::= literal | identifier | "(" expr ")" | tuple_literal
                  | list_literal | dict_literal | set_literal | lambda_expr
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
| `Dict[K, V]`    | Hash table; `K` must implement `Hash`  |
| `Set[T]`        | Hash set                               |
| `Tuple[...]`    | Heterogeneous fixed-size product       |
| `BigInt`        | Arbitrary-precision integer            |
| User classes    | Class instances                        |

Reference types are heap-allocated and accessed via pointers. Two reference values are `is`-equal iff they point to the same object.

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

##### v0.1 limits (deferred to v0.2)

- **No generic classes.** Parser accepts `class Box[T]:` but the type
  checker rejects field-typed references to `T`. (v0.2 will extend
  monomorphisation through class layouts.)
- **No bounds.** `T: Comparable` parses but the checker ignores the
  bound. A body that uses `<` on `T` typechecks, and instantiations
  where `<` is unsupported (e.g. user-defined class without comparison)
  trap at runtime rather than reject at compile time. (v0.2 will add
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

There is **no implicit numeric coercion**. `i32 + i64` is a type error. Use explicit conversions:

```python
x: i32 = 1
y: i64 = 2
z: i64 = i64(x) + y    // OK
```

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

- Signed overflow: traps in debug builds (raises `OverflowError`), wraps in release builds.
- Unsigned overflow: always wraps.
- Division by zero: raises `DivisionByZeroError`.
- `//` is truncated division for ints, floored for floats.
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
```

The single argument MUST be a `str`-typed expression. The exception
class name must be one of the built-in names in §7.5.1; user-defined
exception subclasses are deferred (see §7.5.6).

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

#### 7.5.5 Catch-all order

Multiple `except` clauses are matched top-to-bottom. The first arm
whose filter matches the thrown `type_name` runs. Use `except
Exception as e:` LAST to catch anything not handled by an earlier
specific arm.

#### 7.5.6 Out of scope for v0.1

These constructs parse but are not lowered (or are deferred entirely):

* `raise X from cause` — `from` clause is parsed and ignored.
* `except (A, B) as e:` — multi-type tuple in one arm.
* `else:` clause on `try`.
* Bare `raise` (re-raise).
* User-defined exception classes subclassing `Exception` (the
  parser/resolver accepts the syntax, but the runtime's type-name
  match doesn't recognise the user name).
* Exception chaining (`__cause__`, `__context__`), tracebacks.

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
argv:     List[str]      # program args; argv[0] is the .spyc path
platform: str            # "windows" | "linux" | "macos" | "unknown"
version:  str            # banner string, e.g. "StrictPy v0.2"

fn exit(code: i32) -> Never
```

Semantics:

* `sys.argv` — lazy `List[str]`. Materialised on first read by the VM
  and cached so subsequent reads return the same heap object (allowing
  `sys.argv.append(...)` to be visible across the program).
  `argv[0]` is conventionally the path to the `.spyc` that was invoked.
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

fn consumer(ch: Channel[i32]) -> None:
    while true:
        v: i32? = ch.try_recv()
        if v is none:
            break
        println("got " + str(v))

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
