"""Loopback TCP echo: client thread does 2000 8-byte roundtrips against an
in-process echo server; print roundtrip count (after join)."""
import socket
import threading


def recv_exact(sock, n):
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("connection closed early")
        buf += chunk
    return buf


def client(port, n):
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.connect(("127.0.0.1", port))
    s.settimeout(10.0)
    for _ in range(n):
        s.sendall(b"echo-png")
        recv_exact(s, 8)
    s.close()


def main():
    n = 2000
    chosen_port = 0
    listener = None
    attempt = 0
    while attempt < 16 and chosen_port == 0:
        candidate = 51100 + attempt
        try:
            listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            listener.bind(("127.0.0.1", candidate))
            listener.listen(8)
            chosen_port = candidate
        except OSError:
            chosen_port = 0
            if listener:
                listener.close()
        attempt += 1

    if chosen_port == 0:
        print("error=no_port")
        return

    t = threading.Thread(target=client, args=(chosen_port, n))
    t.start()

    conn, _addr = listener.accept()
    conn.settimeout(10.0)
    for _ in range(n):
        req = recv_exact(conn, 8)
        conn.sendall(req)

    conn.close()
    listener.close()
    t.join()
    print("roundtrips=" + str(n))


if __name__ == "__main__":
    main()
