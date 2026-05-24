import socket
import threading
import urllib.request
import urllib.error
import re
import queue

# Visited tracking class (thread-safe wrapper around set)
class VisitedTracker:
    def __init__(self):
        self.lock = threading.Lock()
        self.visited = set()

    def mark_visited(self, url: str) -> bool:
        with self.lock:
            if url not in self.visited:
                self.visited.add(url)
                return True
            return False

    def is_visited(self, url: str) -> bool:
        with self.lock:
            return url in self.visited

# Mock HTTP Server running on a background thread
def run_mock_server(port: int):
    lis = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    lis.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    lis.bind(("127.0.0.1", port))
    lis.listen(16)
    requests_handled = 0
    while requests_handled < 10:
        try:
            conn, peer = lis.accept()
            req = conn.recv(1024).decode('utf-8')
            resp_body = ""
            status_line = "HTTP/1.1 200 OK\r\n"
            
            if "GET /page1" in req or "GET / " in req or "GET /index.html" in req:
                resp_body = "<html><body><h1>Page 1</h1><a href=\"/page2\">Page 2</a> <a href=\"/page3\">Page 3</a></body></html>"
            elif "GET /page2" in req:
                resp_body = "<html><body><h1>Page 2</h1><a href=\"/page3\">Page 3</a> <a href=\"/index.html\">Page 1</a></body></html>"
            elif "GET /page3" in req:
                resp_body = "<html><body><h1>Page 3</h1><a href=\"/index.html\">Back to Home</a></body></html>"
            else:
                status_line = "HTTP/1.1 404 Not Found\r\n"
                resp_body = "Not Found"
            
            resp = f"{status_line}Content-Type: text/html\r\nContent-Length: {len(resp_body)}\r\n\r\n{resp_body}"
            conn.sendall(resp.encode('utf-8'))
            conn.close()
            requests_handled += 1
        except Exception:
            pass
    lis.close()

# Crawler worker function
def crawler_worker(
    worker_id: int,
    base_url: str,
    q: queue.Queue,
    tracker: VisitedTracker,
    pending_lock: threading.Lock,
    pending: list,
    results: queue.Queue
):
    regex = re.compile(r'href="(/[^"]*)"')
    while True:
        try:
            # Block to get a URL, wait if empty
            url = q.get(timeout=0.5)
        except queue.Empty:
            continue

        if url == "SHUTDOWN":
            q.put("SHUTDOWN") # Propagate shutdown to other workers
            break

        print(f"Worker {worker_id} crawling: {url}")
        
        # Simulate fetch
        full_url = base_url + url
        success = False
        body = ""
        try:
            with urllib.request.urlopen(full_url, timeout=5) as response:
                if response.status == 200:
                    success = True
                    body = response.read().decode('utf-8')
                else:
                    print(f"Worker {worker_id} failed: {url} status={response.status}")
        except Exception as e:
            print(f"Worker {worker_id} error: {url} message={e}")

        if success:
            results.put(url)
            matches = regex.findall(body)
            for path in matches:
                if not tracker.is_visited(path):
                    is_new = tracker.mark_visited(path)
                    if is_new:
                        with pending_lock:
                            pending[0] += 1
                        q.put(path)

        # Finished processing this item, decrement pending
        with pending_lock:
            pending[0] -= 1
            done = (pending[0] == 0)

        if done:
            q.put("SHUTDOWN")

def main():
    port = 50048
    server_t = threading.Thread(target=run_mock_server, args=(port,))
    server_t.start()

    base_url = f"http://127.0.0.1:{port}"
    q = queue.Queue()
    tracker = VisitedTracker()
    pending_lock = threading.Lock()
    pending = [1]
    results = queue.Queue()

    # Seed first page
    tracker.mark_visited("/index.html")
    q.put("/index.html")

    # Spawn 3 crawler workers
    workers = []
    for i in range(3):
        t = threading.Thread(
            target=crawler_worker,
            args=(i+1, base_url, q, tracker, pending_lock, pending, results)
        )
        t.start()
        workers.append(t)

    for t in workers:
        t.join()

    server_t.join()

    print("\nCrawl complete. Visited pages:")
    while not results.empty():
        print(f"- {results.get()}")

if __name__ == "__main__":
    main()
