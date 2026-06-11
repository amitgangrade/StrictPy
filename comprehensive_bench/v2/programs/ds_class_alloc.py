# ds_class_alloc: allocate N small 3-field objects in a loop, read fields
# back, sum. Uses __slots__ (idiomatic for perf-sensitive small objects).


class Particle:
    __slots__ = ("x", "y", "z")

    def __init__(self, x, y, z):
        self.x = x
        self.y = y
        self.z = z


def main() -> None:
    n = 500000
    parts = [Particle(i % 101, i % 211, i % 307) for i in range(n)]
    sx = 0
    sy = 0
    sz = 0
    for p in parts:
        sx += p.x
        sy += p.y
        sz += p.z
    print(f"n={len(parts)}")
    print(f"sx={sx}")
    print(f"sy={sy}")
    print(f"sz={sz}")


if __name__ == "__main__":
    main()
