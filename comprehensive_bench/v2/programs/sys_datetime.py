"""Datetime epoch math: ISO roundtrips, add days, field sums and weekday
distribution over N LCG-derived timestamps; print numeric checksums only."""
from datetime import datetime, timedelta, timezone


def main():
    n = 60000
    state = 123456789
    roundtrip_ok = 0
    ymd_sum = 0
    shifted_wd_sum = 0
    wd_counts = [0] * 7

    for _ in range(n):
        state = (state * 1103515245 + 12345) % 2147483648
        ts = state % 2000000000

        dt = datetime.fromtimestamp(ts, timezone.utc)
        iso = dt.isoformat()
        back = int(datetime.fromisoformat(iso).timestamp())
        if back == ts:
            roundtrip_ok += 1

        ymd_sum += dt.year + dt.month + dt.day

        wd_counts[dt.weekday()] += 1

        shifted = dt + timedelta(days=30)
        shifted_wd_sum += shifted.weekday()

    dist = ",".join(str(c) for c in wd_counts)

    print("roundtrip_ok=" + str(roundtrip_ok))
    print("ymd_sum=" + str(ymd_sum))
    print("wd_counts=" + dist)
    print("shifted_wd_sum=" + str(shifted_wd_sum))


if __name__ == "__main__":
    main()
