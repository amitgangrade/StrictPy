# Financial ETL & Time-Series: StrictPy vs. Standard Python (Pandas)

This project compares a data cleaning, SQLite merging, and time-series resampling pipeline using StrictPy's Rust-native `tabular` library and Python's standard `pandas`.

## Comparison Table

| Feature | StrictPy (`etl.spy`) | Python (`etl.py` with pandas) |
| :--- | :--- | :--- |
| **Data Engine** | `tabular` module written in native Rust (independent of CPython). | `pandas` library backed by `numpy` C-extensions. |
| **Null Semantics** | Parallel boolean mask (`nulls: List[bool]`). No sentinels. | Sentinel-based (e.g., `NaN` on floats, `NaT` on datetimes). |
| **Column Storage** | Sealed `Column` subclass hierarchy (`ColumnI64`, `ColumnF64`, etc.). | BlockManager holding contiguous memory arrays. |
| **Type Checking** | Compile-time checking of column getters (e.g. `get_column_f64`). | Runtime duck-typing; operations fail dynamically. |
| **Time Series Resampling** | Explicitly named column function `df.resample(col, rule, agg)`. | Method chaining `.set_index().resample().mean()`. |
| **SQLite Fetching** | Native `tabular.from_sql(cur, schema)` draining a cursor. | Standard `pd.read_sql_query(sql, conn)`. |

## Key Syntax Differences

### 1. SQLite Cursor Binding
StrictPy requires a concrete schema declaration to parse database records directly into native columns:
```python
# StrictPy
let cur: Cursor = conn.query("SELECT symbol, company, sector FROM company_sectors")
let meta_schema: List[Tuple[str, str]] = []
meta_schema.append(("Symbol", "str"))
meta_schema.append(("Company", "str"))
meta_schema.append(("Sector", "str"))
let meta_df: DataFrame = tabular.from_sql(cur, meta_schema)
```
Python uses SQL execution libraries to load everything dynamically:
```python
# Python
meta_df = pd.read_sql_query("SELECT symbol, company, sector FROM company_sectors", conn)
```

### 2. Time-Series Resampling
StrictPy uses string-based rules (`<i64><m|h|d>`) and explicitly named aggregations. Non-numeric columns are automatically dropped:
```python
# StrictPy
let resampled_df: DataFrame = merged_df.resample("Timestamp", "2m", "mean")
```
Python converts column types to timestamps, shifts them to indexes, and applies resamplers:
```python
# Python
resampled_df = merged_df.set_index('Timestamp').resample('2Min').mean(numeric_only=True).reset_index()
```

### 3. Safe Column Retrieval & Unwrapping
Because StrictPy has static type checking and nullable types (`T?`), accessing values from columns requires explicit non-null checks:
```python
# StrictPy
let price_col: ColumnF64? = sorted_df.get_column_f64("Price")
if price_col is not none:
    let rolling_p: ColumnF64 = price_col.rolling_mean(3i64)
    # roll_val: f64? is checked against none before printing
```
Python accesses series directly and propagates NaNs silently:
```python
# Python
sorted_df['RollMean'] = sorted_df['Price'].rolling(3).mean()
```
