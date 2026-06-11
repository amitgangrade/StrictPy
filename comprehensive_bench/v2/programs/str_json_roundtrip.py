# str_json_roundtrip: parse + stringify a nested JSON payload N times using
# the json module (json.loads / json.dumps compact); print total serialized length.

import json


def main() -> None:
    raw = '{"id":12345,"name":"benchmark","active":true,"tags":["alpha","beta","gamma"],"items":[{"sku":"a1","qty":3},{"sku":"b2","qty":7},{"sku":"c3","qty":11}],"meta":{"version":2,"region":"us-east","flags":[1,2,3,4,5]}}'
    n = 20000
    total_len = 0
    for _ in range(n):
        parsed = json.loads(raw)
        ser = json.dumps(parsed, separators=(",", ":"))
        total_len += len(ser)
    print(f"total_len={total_len}")
    print(f"one_len={total_len // n}")


if __name__ == "__main__":
    main()
