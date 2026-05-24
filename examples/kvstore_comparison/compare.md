# Binary Key-Value Store: StrictPy vs. Standard Python

This project compares a binary append-only key-value store (resembling a mini-Bitcask engine) implemented in StrictPy and Python. It highlights how the two languages handle byte representation, serialization (`struct`), and file access.

## Comparison Table

| Feature | StrictPy (`kvstore.spy`) | Python (`kvstore.py`) |
| :--- | :--- | :--- |
| **String/Byte Model** | Single `str` type. Internally UTF-8, but acts as a **raw byte buffer** for binary modules (codepoints 0..255). | Strict separation between Unicode text (`str`) and binary data (`bytes`). |
| **Encoding/Decoding** | Implicit. Slices and operations treat the string as a buffer. | Explicit via `key.encode('utf-8')` and `bytes.decode('utf-8')`. |
| **File I/O Mode** | Text modes `"r"`, `"w"`, `"a"`. No distinct binary mode. | Binary modes `"rb"`, `"wb"`, `"ab"`. |
| **Random Access** | No `seek`/`tell` support. Simulated by reading the whole file into memory and using `content.slice(offset, len)`. | Full `.seek(offset)` and `.read(length)` support on file handles. |
| **Binary Packing** | Typed functions `struct.pack_i32(n)` and `struct.unpack_i32(data, offset)`. | String-formatted `struct.pack(">i", n)` and `struct.unpack(">i", data)`. |
| **Checksums** | `hashlib.sha256(data)` returns a 64-character hex string. | `hashlib.sha256(bytes).hexdigest()` returns a 64-character hex string. |

## Key Syntax Differences

### 1. Serialization (`struct`)
StrictPy uses typed functions mapping directly to Rust's byte serialization:
```python
# StrictPy
let header: str = checksum + struct.pack_i32(klen) + struct.pack_i32(vlen)
# Unpacking requires offset:
let klen: i32 = struct.unpack_i32(content, offset + 64i64)
```
Python uses generic format strings to specify byte ordering and sizes:
```python
# Python
header = checksum_bytes + struct.pack(">i", klen) + struct.pack(">i", vlen)
# Unpacking requires slicing the buffer first:
klen = struct.unpack(">i", content[offset + 64:offset + 68])[0]
```

### 2. File Seek vs. In-Memory Slicing
Because StrictPy has no `seek` method in its `io.File` implementation, we read the entire file into a string and slice out the requested offset:
```python
# StrictPy
let f: io.File = open(self.file_path, "r")
let content: str = f.read()
f.close()
let val: str = content.slice(entry.offset, entry.offset + i64(entry.length))
```
Python avoids loading the entire file into memory by seeking directly:
```python
# Python
with open(self.file_path, "rb") as f:
    f.seek(entry.offset)
    val_bytes = f.read(entry.length)
```

### 3. Binary Hashing
StrictPy's `hashlib` takes `str` representing a byte buffer and returns a hex digest string:
```python
# StrictPy
let checksum: str = hashlib.sha256(key + value)
```
Python's `hashlib` requires a `bytes` object and must have `.hexdigest()` called:
```python
# Python
checksum = hashlib.sha256(key_bytes + val_bytes).hexdigest()
```
