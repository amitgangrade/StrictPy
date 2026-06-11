# str_base64: encode+decode loop over ASCII payloads (Python base64 works on
# bytes); printed lengths/round-trip counts match StrictPy.

import base64


def main() -> None:
    reps = 20000
    total_enc = 0
    ok = 0
    sample = ""
    for i in range(reps):
        piece = "data:" + str(i) + ";chunk-" + str((i * 31) % 997) + ";"
        payload = piece * 40
        enc = base64.b64encode(payload.encode("ascii")).decode("ascii")
        dec = base64.b64decode(enc).decode("ascii")
        total_enc += len(enc)
        if dec == payload:
            ok += 1
        sample = enc[0:24]
    print(f"total_enc={total_enc}")
    print(f"ok={ok}")
    print(f"sample={sample}")


if __name__ == "__main__":
    main()
