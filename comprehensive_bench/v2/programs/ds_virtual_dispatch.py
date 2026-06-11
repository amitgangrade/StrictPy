# ds_virtual_dispatch: Shape base class with overridden method, list of mixed
# subclass instances, sum method results in a hot loop.


class Shape:
    def area(self):
        return 0


class Square(Shape):
    __slots__ = ("side",)

    def __init__(self, side):
        self.side = side

    def area(self):
        return self.side * self.side


class Rect(Shape):
    __slots__ = ("w", "h")

    def __init__(self, w, h):
        self.w = w
        self.h = h

    def area(self):
        return self.w * self.h


class Tri(Shape):
    __slots__ = ("base", "height")

    def __init__(self, base, height):
        self.base = base
        self.height = height

    def area(self):
        return self.base * self.height // 2


def main() -> None:
    n = 90000
    shapes = []
    for i in range(n):
        k = i % 3
        if k == 0:
            shapes.append(Square(i % 13 + 1))
        elif k == 1:
            shapes.append(Rect(i % 7 + 1, i % 11 + 1))
        else:
            shapes.append(Tri(i % 9 + 1, i % 17 + 1))
    passes = 10
    total = 0
    for _ in range(passes):
        for s in shapes:
            total += s.area()
    print(f"n={len(shapes)}")
    print(f"passes={passes}")
    print(f"total={total}")


if __name__ == "__main__":
    main()
