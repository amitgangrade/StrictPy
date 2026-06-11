"""In-memory sqlite: CREATE TABLE, parameterized INSERTs, ranged SELECTs
with WHERE, aggregate SUM; print integer results."""
import sqlite3


def main():
    conn = sqlite3.connect(":memory:")
    conn.execute("CREATE TABLE events (id INTEGER PRIMARY KEY, name TEXT, score INTEGER)")

    n = 40000
    for i in range(n):
        conn.execute(
            "INSERT INTO events (name, score) VALUES (?, ?)",
            (f"event_{i % 500}", (i * 37) % 1000),
        )

    # Ranged SELECTs with WHERE.
    range_rows = 0
    range_score = 0
    for j in range(400):
        lo = (j * 61) % n + 1
        hi = lo + 200
        cur = conn.execute(
            "SELECT id, score FROM events WHERE id >= ? AND id < ? ORDER BY id",
            (lo, hi),
        )
        for _id, score in cur.fetchall():
            range_rows += 1
            range_score += score

    # Aggregate SUM over the whole table.
    total = conn.execute("SELECT SUM(score) FROM events").fetchone()[0]

    conn.close()
    print("inserted=" + str(n))
    print("range_rows=" + str(range_rows))
    print("range_score=" + str(range_score))
    print("total_score=" + str(total))


if __name__ == "__main__":
    main()
