import os
import sqlite3
import pandas as pd

def setup_data():
    # 1. Write synthetic transactions to CSV
    csv_data = "Symbol,Timestamp,Price,Volume\n" + \
        "AAPL,2026-05-23T10:00:00Z,150.0,100\n" + \
        "AAPL,2026-05-23T10:01:00Z,152.0,150\n" + \
        "MSFT,2026-05-23T10:01:30Z,300.0,200\n" + \
        "AAPL,2026-05-23T10:02:00Z,151.0,120\n" + \
        "MSFT,2026-05-23T10:03:00Z,302.0,250\n" + \
        "AAPL,2026-05-23T10:04:00Z,153.0,110\n" + \
        "MSFT,2026-05-23T10:05:00Z,301.0,180\n" + \
        "AAPL,2026-05-23T10:06:00Z,154.0,130\n" + \
        "MSFT,2026-05-23T10:07:00Z,305.0,220\n"
    with open("transactions.csv", "w") as f:
        f.write(csv_data)

    # 2. Setup SQLite database with company metadata
    conn = sqlite3.connect("metadata.db")
    cur = conn.cursor()
    cur.execute("CREATE TABLE IF NOT EXISTS company_sectors (symbol TEXT, company TEXT, sector TEXT)")
    cur.execute("DELETE FROM company_sectors")
    cur.executemany("INSERT INTO company_sectors (symbol, company, sector) VALUES (?, ?, ?)", [
        ("AAPL", "Apple Inc.", "Technology"),
        ("MSFT", "Microsoft Corp.", "Technology")
    ])
    conn.commit()
    conn.close()

def main():
    setup_data()

    # Load transactions from CSV
    tx_df = pd.read_csv("transactions.csv")
    tx_df['Timestamp'] = pd.to_datetime(tx_df['Timestamp'])

    # Load metadata from SQLite
    conn = sqlite3.connect("metadata.db")
    meta_df = pd.read_sql_query("SELECT symbol, company, sector FROM company_sectors", conn)
    conn.close()

    # Rename symbol columns to make sure merge matches
    # Since CSV has 'Symbol' and SQLite has 'symbol' (lowercase in sql but we selected as symbol)
    # We standardise column casing
    tx_df = tx_df.rename(columns={'Symbol': 'Symbol'})
    meta_df = meta_df.rename(columns={'symbol': 'Symbol', 'company': 'Company', 'sector': 'Sector'})

    print("--- Transactions DataFrame ---")
    print(tx_df.to_string(index=False))
    print()

    print("--- Metadata DataFrame ---")
    print(meta_df.to_string(index=False))
    print()

    # Inner Join (merge) on Symbol
    merged_df = pd.merge(tx_df, meta_df, on="Symbol", how="inner")
    print("--- Merged DataFrame ---")
    print(merged_df.to_string(index=False))
    print()

    # Resample to 2-minute intervals (mean aggregation)
    # Note: resample is time-series based, so we set index first
    resampled_df = merged_df.set_index('Timestamp').resample('2Min').mean(numeric_only=True).reset_index()
    print("--- Resampled (2m Mean) ---")
    print(resampled_df.to_string(index=False))
    print()

    # Sort by Timestamp to compute rolling mean
    sorted_df = merged_df.sort_values('Timestamp').copy()
    # Compute rolling mean with min_periods=3 (window=3)
    sorted_df['RollMean'] = sorted_df['Price'].rolling(3).mean()

    print("--- Rolling Price Mean (Window=3) ---")
    for _, row in sorted_df.iterrows():
        roll_val = row['RollMean']
        roll_str = f"{roll_val:.1f}" if not pd.isna(roll_val) else "null"
        print(f"Symbol={row['Symbol']} Time={row['Timestamp'].strftime('%Y-%m-%dT%H:%M:%SZ')} Price={row['Price']} RollMean={roll_str}")

    # Cleanup files
    if os.path.exists("transactions.csv"):
        os.remove("transactions.csv")
    if os.path.exists("metadata.db"):
        os.remove("metadata.db")

if __name__ == "__main__":
    main()
