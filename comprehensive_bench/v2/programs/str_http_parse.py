# str_http_parse: realistic HTTP/1.1 request parser using native string
# methods: split request-line/headers/body, lowercase header names, extract
# Content-Length. Parse a fixed request N times; print aggregate counters.


def parse_one(req: str) -> int:
    sep = req.find("\r\n\r\n")
    if sep < 0:
        return 0
    head = req[0:sep]
    body = req[sep + 4:len(req)]
    lines = head.split("\r\n")
    req_line = lines[0]
    parts = req_line.split(" ")
    method = parts[0]
    path = parts[1]

    score = 0
    if method == "GET":
        score += 1
    if "?" in path:
        score += 1

    content_length = -1
    for line in lines[1:]:
        ci = line.find(":")
        if ci >= 0:
            hname = line[0:ci].lower()
            hval = line[ci + 1:len(line)].strip()
            score += 10
            if hname == "content-length":
                content_length = int(hval)
            if hname.startswith("x-"):
                score += 1
    if content_length == len(body):
        score += 100
    return score + content_length


def main() -> None:
    req = (
        "GET /api/v2/items?page=3 HTTP/1.1\r\n"
        "Host: example.com:8080\r\n"
        "User-Agent: bench/1.0\r\n"
        "Accept: application/json\r\n"
        "X-Request-Id: abc-123-def\r\n"
        "Content-Type: application/json\r\n"
        "Content-Length: 26\r\n"
        "\r\n"
        '{"query":"items","page":3}'
    )
    n = 12000
    total = 0
    for _ in range(n):
        total += parse_one(req)
    print(f"total={total}")
    print(f"per_req={total // n}")


if __name__ == "__main__":
    main()
