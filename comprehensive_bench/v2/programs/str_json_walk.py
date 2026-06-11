# str_json_walk: parse a JSON document once per iteration and WALK it
# (extract nested fields, sum array of numbers) via dict access.

import json


def main() -> None:
    raw = '{"user":{"name":"alice","id":42},"scores":[3,1,4,1,5,9,2,6],"tags":["x","y","z"],"limits":{"max":100,"min":1}}'
    n = 30000
    id_sum = 0
    score_sum = 0
    tag_count = 0
    limit_sum = 0
    for _ in range(n):
        doc = json.loads(raw)
        id_sum += doc["user"]["id"]
        score_sum += sum(doc["scores"])
        tag_count += len(doc["tags"])
        lim = doc["limits"]
        limit_sum += lim["max"] + lim["min"]
    print(f"id_sum={id_sum}")
    print(f"score_sum={score_sum}")
    print(f"tag_count={tag_count}")
    print(f"limit_sum={limit_sum}")


if __name__ == "__main__":
    main()
