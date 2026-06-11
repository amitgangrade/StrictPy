"""URL codec: quote/unquote plus urlencode/parse of query strings, N
iterations; print accumulated lengths and counts.

Note: safe='' matches StrictPy's quote (percent-encodes '/' too)."""
from urllib.parse import parse_qsl, quote, unquote, urlencode


def main():
    n = 30000
    quoted_len = 0
    unquote_ok = 0
    encoded_len = 0
    parsed_pairs = 0

    for i in range(n):
        s = f"name {i}&value={i * 7}/path?q={i % 100}"

        q = quote(s, safe="")
        quoted_len += len(q)
        if unquote(q) == s:
            unquote_ok += 1

        pairs = [(f"key {i % 50}", s), ("page", str(i % 10))]
        enc = urlencode(pairs)
        encoded_len += len(enc)

        dec = parse_qsl(enc)
        parsed_pairs += len(dec)

    print("quoted_len=" + str(quoted_len))
    print("unquote_ok=" + str(unquote_ok))
    print("encoded_len=" + str(encoded_len))
    print("parsed_pairs=" + str(parsed_pairs))


if __name__ == "__main__":
    main()
