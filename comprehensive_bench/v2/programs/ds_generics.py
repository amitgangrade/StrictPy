# ds_generics: Box / Pair container classes used in a hot loop (plain Python
# classes; generics are typing-only in CPython).


class Box:
    __slots__ = ("value",)

    def __init__(self, v):
        self.value = v

    def unwrap(self):
        return self.value


class Pair:
    __slots__ = ("key", "value")

    def __init__(self, k, v):
        self.key = k
        self.value = v


def main() -> None:
    n = 400000
    box_sum = 0
    pair_sum = 0
    key_len_sum = 0
    for i in range(n):
        b = Box(i % 1000)
        box_sum += b.unwrap()
        p = Pair("p" + str(i % 50), i % 313)
        pair_sum += p.value
        key_len_sum += len(p.key)
    sb = Box("checksum")
    print(f"box_sum={box_sum}")
    print(f"pair_sum={pair_sum}")
    print(f"key_len_sum={key_len_sum}")
    print(f"str_box={sb.unwrap()}")


if __name__ == "__main__":
    main()
