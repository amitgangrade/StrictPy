from typing import Generic, TypeVar, Tuple, Callable, Optional

T = TypeVar('T')
U = TypeVar('U')

# Class wrapping a generic parse function
class Parser(Generic[T]):
    def __init__(self, impl: Callable[[str, int], Optional[Tuple[T, int]]]):
        self.impl = impl

    def parse(self, src: str, pos: int) -> Optional[Tuple[T, int]]:
        return self.impl(src, pos)

# 1. Base Char Parser
def char_parser(expected: str) -> Parser[str]:
    def impl(src: str, pos: int) -> Optional[Tuple[str, int]]:
        if pos < len(src):
            if src[pos] == expected:
                return (expected, pos + 1)
        return None
    return Parser(impl)

# 2. Map Parser Combinator
def map_parser(p: Parser[T], f: Callable[[T], U]) -> Parser[U]:
    def impl(src: str, pos: int) -> Optional[Tuple[U, int]]:
        res = p.parse(src, pos)
        if res is not None:
            val, next_pos = res
            return (f(val), next_pos)
        return None
    return Parser(impl)

# 3. Choice Combinator (Or Else)
def or_else(p1: Parser[T], p2: Parser[T]) -> Parser[T]:
    def impl(src: str, pos: int) -> Optional[Tuple[T, int]]:
        res1 = p1.parse(src, pos)
        if res1 is not None:
            return res1
        return p2.parse(src, pos)
    return Parser(impl)

# ── AST Node Definitions ──
class Expr:
    pass

class NumExpr(Expr):
    def __init__(self, val: int):
        self.val = val

class PlusExpr(Expr):
    def __init__(self, lhs: Expr, rhs: Expr):
        self.lhs = lhs
        self.rhs = rhs

# ── Custom Parsers for our Simple Plus Language ──

# Parses an integer literal
def int_parser() -> Parser[int]:
    def impl(src: str, pos: int) -> Optional[Tuple[int, int]]:
        start = pos
        current = pos
        while current < len(src):
            c = src[current]
            if c.isdigit():
                current += 1
            else:
                break
        if current > start:
            val = int(src[start:current])
            return (val, current)
        return None
    return Parser(impl)

# Parses a NumExpr AST node
def num_expr_parser() -> Parser[Expr]:
    p_int = int_parser()
    def impl(src: str, pos: int) -> Optional[Tuple[Expr, int]]:
        res = p_int.parse(src, pos)
        if res is not None:
            val, next_pos = res
            return (NumExpr(val), next_pos)
        return None
    return Parser(impl)

# Parses an addition operation (NumExpr + NumExpr)
def plus_parser() -> Parser[Expr]:
    p_num = num_expr_parser()
    p_plus = char_parser('+')
    def impl(src: str, pos: int) -> Optional[Tuple[Expr, int]]:
        r1 = p_num.parse(src, pos)
        if r1 is not None:
            lhs, next1 = r1
            r2 = p_plus.parse(src, next1)
            if r2 is not None:
                _, next2 = r2
                r3 = p_num.parse(src, next2)
                if r3 is not None:
                    rhs, next3 = r3
                    return (PlusExpr(lhs, rhs), next3)
        return None
    return Parser(impl)

# Combines Plus and Num expressions
def expr_parser() -> Parser[Expr]:
    p_plus = plus_parser()
    p_num = num_expr_parser()
    return or_else(p_plus, p_num)

# Evaluates the AST using Python 3.10+ pattern matching
def eval_expr(e: Expr) -> int:
    match e:
        case NumExpr(val=val):
            return val
        case PlusExpr(lhs=lhs, rhs=rhs):
            return eval_expr(lhs) + eval_expr(rhs)
        case _:
            raise ValueError("invalid expression")

def main():
    parser = expr_parser()

    # 1. Parse simple literal "42"
    r1 = parser.parse("42", 0)
    if r1 is not None:
        e1, _ = r1
        val1 = eval_expr(e1)
        print(f"parsed '42' -> eval value: {val1}")
    else:
        print("failed to parse '42'")

    # 2. Parse addition "123+456"
    r2 = parser.parse("123+456", 0)
    if r2 is not None:
        e2, _ = r2
        val2 = eval_expr(e2)
        print(f"parsed '123+456' -> eval value: {val2}")
    else:
        print("failed to parse '123+456'")

    # 3. Fail case
    r3 = parser.parse("abc", 0)
    if r3 is None:
        print("correctly failed to parse 'abc'")
    else:
        print("unexpected success parsing 'abc'")

if __name__ == "__main__":
    main()
