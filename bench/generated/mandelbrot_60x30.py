WIDTH = 60
HEIGHT = 30
MAX_ITER = 50

def main():
    row = 0
    while row < HEIGHT:
        col = 0
        line = ""
        while col < WIDTH:
            cx = (float(col) / float(WIDTH)) * 3.5 - 2.5
            cy = (float(row) / float(HEIGHT)) * 2.0 - 1.0
            zx = 0.0
            zy = 0.0
            it = 0
            escaped = False
            while it < MAX_ITER:
                zx2 = zx * zx
                zy2 = zy * zy
                if zx2 + zy2 > 4.0:
                    escaped = True
                    break
                new_zx = zx2 - zy2 + cx
                new_zy = 2.0 * zx * zy + cy
                zx = new_zx
                zy = new_zy
                it += 1
            line += " " if escaped else "#"
            col += 1
        print(line)
        row += 1

main()
