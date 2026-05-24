# Concurrent Web Crawler: StrictPy vs. Standard Python

This project compares a concurrent web crawler that spins up a mock loopback HTTP server and crawls page links using worker threads.

## Comparison Table

| Feature | StrictPy (`crawler.spy`) | Python (`crawler.py`) |
| :--- | :--- | :--- |
| **Type Safety** | Mandatory static typing on all parameters, returns, and variables. No dynamic fields. | Dynamically typed. Optional type hints are not checked at runtime. |
| **Concurrency Core** | Native OS threads (`Thread`) + thread-safe typed queues (`Channel[T]`). | Native OS threads (`threading.Thread`) + thread-safe queue (`queue.Queue`). |
| **Locks** | Low-level handle-based locks (`lock = threading.lock()`, `lock_acquire`, `lock_release`). | Object-oriented locks with context managers (`with lock:`). |
| **HTTP Client** | Stdlib `http_client.get(url)` returning `Tuple[i32, str]`. | Stdlib `urllib.request.urlopen(url)` returning a response object. |
| **Regex Matching** | `re.compile` returning `Pattern` objects with basic methods like `find_all()`. | Standard `re` module with rich match objects and groups support. |
| **GIL Bottlenecks** | None. True parallel thread execution on JIT-compiled bytecode. | Heavily restricted by the Global Interpreter Lock (GIL) for CPU-bound tasks. |

## Key Syntax Differences

### 1. Visited URL Tracker
In StrictPy, class variables and constructors must be fully annotated, and empty dictionaries require type declarations:
```python
# StrictPy
final class VisitedTracker:
    lock: i64
    visited: Dict[str, bool]

    fn __init__(self) -> None:
        self.lock = threading.lock()
        self.visited = {}
```
In Python, fields are dynamically initialized:
```python
# Python
class VisitedTracker:
    def __init__(self):
        self.lock = threading.Lock()
        self.visited = set()
```

### 2. Thread Locks
StrictPy uses handles (integers of type `i64`) returned by the runtime to represent locks, and uses functions to lock/unlock:
```python
# StrictPy
threading.lock_acquire(self.lock)
# ... critical section ...
threading.lock_release(self.lock)
```
Python uses object-oriented locks and supports the context manager `with` syntax:
```python
# Python
with self.lock:
    # ... critical section ...
```

### 3. Work Queues
StrictPy features built-in `Channel[T]` queues that block on `recv()` and return typed objects. Python uses `queue.Queue` with a timeout pattern to handle interruptions:
```python
# StrictPy
let url: str = queue.recv() # Typed & block-forever

# Python
url = q.get(timeout=0.5) # Throws queue.Empty on timeout
```
