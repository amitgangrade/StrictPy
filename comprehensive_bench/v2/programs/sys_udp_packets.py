"""Loopback UDP: send/recv 5000 datagrams one at a time (send then recv, so
nothing is dropped); print received count + byte total."""
import socket


def main():
    n = 5000

    # Bind the receiver with port retry.
    chosen_port = 0
    recv_sock = None
    attempt = 0
    while attempt < 16 and chosen_port == 0:
        candidate = 51300 + attempt
        try:
            recv_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
            recv_sock.bind(("127.0.0.1", candidate))
            chosen_port = candidate
        except OSError:
            chosen_port = 0
            if recv_sock:
                recv_sock.close()
        attempt += 1

    if chosen_port == 0:
        print("error=no_port")
        return

    send_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)

    received = 0
    byte_total = 0
    for i in range(n):
        payload = f"pkt-{i}-payload".encode()
        send_sock.sendto(payload, ("127.0.0.1", chosen_port))
        data, _addr = recv_sock.recvfrom(256)
        received += 1
        byte_total += len(data)

    send_sock.close()
    recv_sock.close()
    print("received=" + str(received))
    print("bytes=" + str(byte_total))


if __name__ == "__main__":
    main()
