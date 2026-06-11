# str_split_scan: split a large synthetic log text into lines then fields,
# accumulate field lengths; print totals.


def build_log(lines: int) -> str:
    rows = []
    for i in range(lines):
        rows.append(
            "2026-01-" + str((i % 28) + 1) + " INFO user" + str(i % 50)
            + " action" + str(i % 7) + " value=" + str((i * 13) % 997)
        )
    return "\n".join(rows)


def main() -> None:
    text = build_log(4000)
    reps = 100
    line_count = 0
    field_count = 0
    char_total = 0
    for _ in range(reps):
        lines = text.split("\n")
        line_count += len(lines)
        for line in lines:
            fields = line.split(" ")
            field_count += len(fields)
            for f in fields:
                char_total += len(f)
    print(f"lines={line_count}")
    print(f"fields={field_count}")
    print(f"chars={char_total}")


if __name__ == "__main__":
    main()
