"""Binary pack/unpack loop of mixed primitives (u32 + f64 + u64, big-endian);
print integer checksum."""
import struct

FMT = ">IdQ"  # no padding in big-endian mode: 4 + 8 + 8 = 20 bytes


def main():
    checksum = 0
    for i in range(400000):
        a = i % 4096
        x = i * 0.5
        b = i * 1234567

        buf = struct.pack(FMT, a, x, b)
        a2, x2, b2 = struct.unpack(FMT, buf)

        checksum += a2 + int(x2 * 2.0) + (b2 % 1000)

    print("checksum=" + str(checksum))
    print("iters=" + str(400000))


if __name__ == "__main__":
    main()
