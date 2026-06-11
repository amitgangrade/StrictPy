# ds_match_dispatch: expression tree (Num/Add/Mul) evaluated many times with
# an isinstance chain. All arithmetic is mod 1000003 to avoid drift.


class Expr:
    pass


class Num(Expr):
    __slots__ = ("val",)

    def __init__(self, val):
        self.val = val


class Add(Expr):
    __slots__ = ("lhs", "rhs")

    def __init__(self, lhs, rhs):
        self.lhs = lhs
        self.rhs = rhs


class Mul(Expr):
    __slots__ = ("lhs", "rhs")

    def __init__(self, lhs, rhs):
        self.lhs = lhs
        self.rhs = rhs


def build(depth, salt):
    if depth == 0:
        return Num(salt % 10 + 1)
    if salt % 2 == 0:
        return Add(build(depth - 1, salt + 1), build(depth - 1, salt * 3 + 1))
    return Mul(build(depth - 1, salt + 2), build(depth - 1, salt * 2 + 1))


def eval_expr(e):
    if isinstance(e, Num):
        return e.val
    if isinstance(e, Add):
        return (eval_expr(e.lhs) + eval_expr(e.rhs)) % 1000003
    return (eval_expr(e.lhs) * eval_expr(e.rhs)) % 1000003


def main() -> None:
    tree = build(10, 12345)
    rounds = 150
    acc = 0
    for _ in range(rounds):
        acc = (acc + eval_expr(tree)) % 1000003
    print(f"rounds={rounds}")
    print(f"eval={eval_expr(tree)}")
    print(f"acc={acc}")


if __name__ == "__main__":
    main()
