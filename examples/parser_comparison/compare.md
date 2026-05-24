# Typed Parser Combinator: StrictPy vs. Standard Python

This project compares a generic parser combinator library and Abstract Syntax Tree (AST) evaluator using StrictPy's static generics and structural pattern matching, against standard Python's type annotations and pattern matching.

## Comparison Table

| Feature | StrictPy (`parser.spy`) | Python (`parser.py`) |
| :--- | :--- | :--- |
| **Generics** | **Monomorphised compile-time generics.** The JIT generates specialized code per type parameter instantiation (`Parser__char`, `Parser__Expr`). | **Erased type parameters.** Generics exist only as annotations for static linters (like mypy) and do not exist at runtime. |
| **Sealed Classes** | Supported natively via `sealed class`. The compiler raises exhaustiveness warnings if any subclass case is missing. | No native concept. Simulated via linting conventions or `typing.Union`. No runtime checking. |
| **Pattern Matching Syntax** | Positional destructuring based on field declaration order (`case NumExpr(val):`). | Keyword-based destructuring (`case NumExpr(val=val):`) unless special class attributes are set. |
| **Closures** | Typed closures (`(str, i64) -> Tuple[T, i64]?`) compiled into high-performance JIT-optimized frames. | Dynamic `Callable` objects. |

## Key Syntax Differences

### 1. Generic Class Declaration
StrictPy compiles specialized code for type parameters and checks field assignment types statically:
```python
# StrictPy
final class Parser[T]:
    impl: (str, i64) -> Tuple[T, i64]?

    fn __init__(self, impl: (str, i64) -> Tuple[T, i64]?) -> None:
        self.impl = impl
```
Python uses type variables and dynamic runtime instantiation:
```python
# Python
from typing import Generic, TypeVar

T = TypeVar('T')

class Parser(Generic[T]):
    def __init__(self, impl: Callable[[str, int], Optional[Tuple[T, int]]]):
        self.impl = impl
```

### 2. Sealed AST Class Hierarchy
StrictPy uses `sealed class` to block external subclassing and enforce exhaustive pattern matching:
```python
# StrictPy
sealed class Expr:
    fn __init__(self) -> None:
        pass

final class NumExpr(Expr):
    val: i64
    # ...
```
Python does not enforce class hierarchy boundaries natively:
```python
# Python
class Expr:
    pass

class NumExpr(Expr):
    def __init__(self, val: int):
        self.val = val
```

### 3. Destructuring in Match Cases
StrictPy binds fields positionally based on their order in the class definition:
```python
# StrictPy
match e:
    case NumExpr(val):
        return val
    case PlusExpr(lhs, rhs):
        return eval_expr(lhs) + eval_expr(rhs)
```
Python uses PEP 634 keyword-based pattern matching (unless `__match_args__` is defined on the class):
```python
# Python
match e:
    case NumExpr(val=val):
        return val
    case PlusExpr(lhs=lhs, rhs=rhs):
        return eval_expr(lhs) + eval_expr(rhs)
```
