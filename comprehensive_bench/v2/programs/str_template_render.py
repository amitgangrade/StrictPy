# str_template_render: tiny mustache-style template renderer ({{name}}
# substitution via replace) over N renders; print total output length.


def main() -> None:
    template = "Hello {{name}}! You have {{count}} new {{thing}} in {{place}}."
    n = 150000
    total_len = 0
    hits = 0
    last = ""
    for i in range(n):
        out = template.replace("{{name}}", "user" + str(i % 500))
        out = out.replace("{{count}}", str(i % 100))
        out = out.replace("{{thing}}", "messages")
        out = out.replace("{{place}}", "inbox" + str(i % 10))
        total_len += len(out)
        if "user42!" in out:
            hits += 1
        last = out
    print(f"total_len={total_len}")
    print(f"hits={hits}")
    print(f"last={last}")


if __name__ == "__main__":
    main()
