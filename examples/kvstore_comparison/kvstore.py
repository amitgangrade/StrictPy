import os
import struct
import hashlib

# Class representing an offset in the log file
class RecordOffset:
    def __init__(self, offset: int, length: int):
        self.offset = offset
        self.length = length

# Binary append-only log Key-Value Store
class BinaryKVStore:
    def __init__(self, file_path: str):
        self.file_path = file_path
        self.index = {}
        # Touch or create the file
        if not os.path.exists(file_path):
            with open(file_path, "wb") as f:
                pass
        self.load_index()

    def load_index(self):
        with open(self.file_path, "rb") as f:
            content = f.read()

        n = len(content)
        offset = 0
        while offset + 72 <= n:
            checksum_bytes = content[offset:offset + 64]
            checksum = checksum_bytes.decode('ascii')
            klen = struct.unpack(">i", content[offset + 64:offset + 68])[0]
            vlen = struct.unpack(">i", content[offset + 68:offset + 72])[0]

            total_len = 72 + klen + vlen
            if vlen == -1:
                tombstone_len = 72 + klen
                total_len = tombstone_len

            if offset + total_len > n:
                break # Incomplete trailing write

            key_bytes = content[offset + 72:offset + 72 + klen]
            key = key_bytes.decode('utf-8')
            
            if vlen == -1:
                # Mark as deleted in index
                self.index[key] = RecordOffset(-1, -1)
            else:
                self.index[key] = RecordOffset(offset + 72 + klen, vlen)

            offset += total_len

    def set(self, key: str, value: str):
        key_bytes = key.encode('utf-8')
        val_bytes = value.encode('utf-8')

        # Checksum of key + value (hex digest)
        checksum = hashlib.sha256(key_bytes + val_bytes).hexdigest()
        checksum_bytes = checksum.encode('ascii')
        klen = len(key_bytes)
        vlen = len(val_bytes)

        header = checksum_bytes + struct.pack(">i", klen) + struct.pack(">i", vlen)
        record = header + key_bytes + val_bytes

        # Get size (offset for append)
        with open(self.file_path, "rb") as f_read:
            offset = len(f_read.read())

        # Append record
        with open(self.file_path, "ab") as f_write:
            f_write.write(record)

        # Update index
        self.index[key] = RecordOffset(offset + 72 + klen, vlen)

    def get(self, key: str) -> str:
        entry = self.index.get(key)
        if entry is None or entry.length == -1:
            return None

        # Read the value from file using seek
        with open(self.file_path, "rb") as f:
            f.seek(entry.offset)
            val_bytes = f.read(entry.length)
        return val_bytes.decode('utf-8')

    def delete(self, key: str) -> bool:
        entry = self.index.get(key)
        if entry is None or entry.length == -1:
            return False

        key_bytes = key.encode('utf-8')
        # Write tombstone record
        checksum = hashlib.sha256(key_bytes).hexdigest()
        checksum_bytes = checksum.encode('ascii')
        klen = len(key_bytes)
        vlen = -1

        header = checksum_bytes + struct.pack(">i", klen) + struct.pack(">i", vlen)
        record = header + key_bytes

        with open(self.file_path, "ab") as f_write:
            f_write.write(record)

        # Update index with tombstone
        self.index[key] = RecordOffset(-1, -1)
        return True

def main():
    db_file = "test_kv.bin"
    
    # Ensure starting clean
    if os.path.exists(db_file):
        os.remove(db_file)

    db = BinaryKVStore(db_file)
    
    db.set("user:101", "Alice")
    db.set("user:102", "Bob")
    
    u101 = db.get("user:101")
    if u101 is not None:
        print("GET user:101 -> " + u101)
    
    u102 = db.get("user:102")
    if u102 is not None:
        print("GET user:102 -> " + u102)

    # Overwrite user:101
    db.set("user:101", "Alice Smith")
    u101_v2 = db.get("user:101")
    if u101_v2 is not None:
        print("GET user:101 (v2) -> " + u101_v2)

    # Delete user:102
    del_ok = db.delete("user:102")
    print("DEL user:102 -> " + str(del_ok))
    
    u102_del = db.get("user:102")
    if u102_del is None:
        print("GET user:102 after delete -> <none>")

    # Restart database to verify replay
    print("Restarting database...")
    db2 = BinaryKVStore(db_file)
    
    u101_replay = db2.get("user:101")
    if u101_replay is not None:
        print("Replay GET user:101 -> " + u101_replay)
        
    u102_replay = db2.get("user:102")
    if u102_replay is None:
        print("Replay GET user:102 -> <none>")

    # Cleanup
    if os.path.exists(db_file):
        os.remove(db_file)

if __name__ == "__main__":
    main()
