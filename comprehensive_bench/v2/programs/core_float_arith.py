# core_float_arith: float kernel — mandelbrot escape-iteration counts over a grid.


def main():
    w = 300
    h = 200
    max_iter = 80
    total = 0
    for py in range(h):
        ci = -1.2 + 2.4 * py / (h - 1)
        for px in range(w):
            cr = -2.0 + 3.0 * px / (w - 1)
            zr = 0.0
            zi = 0.0
            it = 0
            while it < max_iter:
                zr2 = zr * zr
                zi2 = zi * zi
                if zr2 + zi2 > 4.0:
                    break
                zi = 2.0 * zr * zi + ci
                zr = zr2 - zi2 + cr
                it += 1
            total += it
    print(f"float_arith_iters={total}")


if __name__ == "__main__":
    main()
