# str_csv_parse: parse a synthetic in-memory CSV string (~50k rows x 6 cols,
# no quoting) via split; aggregate a numeric column; print sums.


def build_csv(n: int) -> str:
    rows = []
    for i in range(n):
        flag = "Y" if i % 2 == 0 else "N"
        rows.append(
            str(i) + ",item" + str(i % 100) + "," + str(i % 7) + ","
            + str((i * 13) % 1000) + "," + flag + ",note" + str(i % 5)
        )
    return "\n".join(rows)


def main() -> None:
    nrows = 50000
    text = build_csv(nrows)
    reps = 8
    price_sum = 0
    qty_sum = 0
    y_count = 0
    rows = 0
    for _ in range(reps):
        lines = text.split("\n")
        for line in lines:
            fields = line.split(",")
            rows += 1
            qty_sum += int(fields[2])
            price_sum += int(fields[3])
            if fields[4] == "Y":
                y_count += 1
    print(f"rows={rows}")
    print(f"qty_sum={qty_sum}")
    print(f"price_sum={price_sum}")
    print(f"y_count={y_count}")


if __name__ == "__main__":
    main()
