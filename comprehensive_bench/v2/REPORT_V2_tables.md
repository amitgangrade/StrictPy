# StrictPy vs CPython — Comprehensive Benchmark v2

_Generated 2026-06-22 15:09:26 · StrictPy `spy 0.2.0` vs CPython 3.12.10 · Windows 11 · best-of-3 full-process runs, interleaved_

Process startup floor: StrictPy 18 ms · CPython 64 ms (included in every number below).

## Scoreboard

| Benchmarks | StrictPy wins (≥1.15×) | Ties | CPython wins (≥1.15×) | Wrong output | Failed to run |
|---|---|---|---|---|---|
| **59** | **28** | 4 | **27** | 0 | 0 |

**Geometric-mean speedup across all passing benchmarks: 1.28× vs CPython.**

## Core compute (JIT-friendly numeric / control flow)

| Benchmark | StrictPy (ms) | CPython (ms) | Verdict |
|---|---:|---:|---|
| `core_bitops` | 38.5 | 915.2 | ✅ 23.75× faster |
| `core_branchy` | 43.6 | 297.4 | ✅ 6.82× faster |
| `core_int_arith` | 32.6 | 216.2 | ✅ 6.64× faster |
| `core_loops_nested` | 28.0 | 184.3 | ✅ 6.57× faster |
| `core_float_arith` | 41.1 | 234.9 | ✅ 5.71× faster |
| `core_quicksort` | 36.8 | 170.9 | ✅ 4.65× faster |
| `core_recursion_fib` | 58.6 | 239.8 | ✅ 4.09× faster |
| `core_recursion_ack` | 22.9 | 85.2 | ✅ 3.72× faster |
| `core_nbody` | 51.0 | 178.5 | ✅ 3.50× faster |
| `core_calls` | 73.2 | 244.7 | ✅ 3.34× faster |
| `core_matrix` | 38.5 | 125.6 | ✅ 3.26× faster |
| `core_sieve` | 153.7 | 251.3 | ✅ 1.64× faster |

## Data structures & language features

| Benchmark | StrictPy (ms) | CPython (ms) | Verdict |
|---|---:|---:|---|
| `ds_list_ops` | 177.3 | 300.0 | ✅ 1.69× faster |
| `ds_class_alloc` | 223.5 | 315.9 | ✅ 1.41× faster |
| `ds_tuple_ops` | 229.3 | 256.3 | ➖ tie |
| `ds_list_sort` | 329.7 | 343.0 | ➖ tie |
| `ds_generics` | 282.0 | 270.6 | ➖ tie |
| `ds_match_dispatch` | 114.0 | 96.5 | ❌ 1.18× SLOWER |
| `ds_string_keys_aggregation` | 194.1 | 162.7 | ❌ 1.19× SLOWER |
| `ds_virtual_dispatch` | 211.5 | 170.6 | ❌ 1.24× SLOWER |
| `ds_comprehensions` | 275.8 | 187.8 | ❌ 1.47× SLOWER |
| `ds_sort_by_key` | 800.8 | 476.9 | ❌ 1.68× SLOWER |
| `ds_nullable` | 446.4 | 263.4 | ❌ 1.69× SLOWER |
| `ds_generators` | 716.8 | 403.2 | ❌ 1.78× SLOWER |
| `ds_exceptions` | 432.5 | 187.8 | ❌ 2.30× SLOWER |
| `ds_closures_hof` | 562.6 | 228.9 | ❌ 2.46× SLOWER |
| `ds_set_ops` | 398.7 | 122.2 | ❌ 3.26× SLOWER |
| `ds_dict_ops` | 1,609.9 | 407.9 | ❌ 3.95× SLOWER |

## Strings, text & serialization

| Benchmark | StrictPy (ms) | CPython (ms) | Verdict |
|---|---:|---:|---|
| `str_regex` | 107.8 | 214.4 | ✅ 1.99× faster |
| `str_slice_scan` | 168.9 | 234.0 | ✅ 1.39× faster |
| `str_concat_build` | 256.7 | 333.2 | ✅ 1.30× faster |
| `str_search` | 204.9 | 197.5 | ➖ tie |
| `str_json_roundtrip` | 377.7 | 264.8 | ❌ 1.43× SLOWER |
| `str_wordcount` | 290.2 | 196.5 | ❌ 1.48× SLOWER |
| `str_json_walk` | 360.5 | 188.5 | ❌ 1.91× SLOWER |
| `str_base64` | 471.3 | 227.7 | ❌ 2.07× SLOWER |
| `str_join_build` | 627.9 | 294.2 | ❌ 2.13× SLOWER |
| `str_template_render` | 468.0 | 183.7 | ❌ 2.55× SLOWER |
| `str_methods_mix` | 1,019.9 | 372.2 | ❌ 2.74× SLOWER |
| `str_split_scan` | 682.8 | 247.4 | ❌ 2.76× SLOWER |
| `str_fstring_format` | 704.4 | 248.7 | ❌ 2.83× SLOWER |
| `str_http_parse` | 333.3 | 109.5 | ❌ 3.04× SLOWER |
| `str_csv_parse` | 900.2 | 276.9 | ❌ 3.25× SLOWER |

## Systems, concurrency & stdlib

| Benchmark | StrictPy (ms) | CPython (ms) | Verdict |
|---|---:|---:|---|
| `sys_channel_throughput` | 50.1 | 407.3 | ✅ 8.12× faster |
| `sys_rate_limiter` | 35.7 | 244.6 | ✅ 6.84× faster |
| `sys_threads_spawn` | 34.5 | 209.9 | ✅ 6.09× faster |
| `sys_pqueue` | 120.4 | 472.0 | ✅ 3.92× faster |
| `sys_url_codec` | 177.6 | 661.1 | ✅ 3.72× faster |
| `sys_async_tasks` | 78.0 | 252.6 | ✅ 3.24× faster |
| `sys_tcp_echo` | 86.0 | 201.4 | ✅ 2.34× faster |
| `sys_datetime` | 110.5 | 237.8 | ✅ 2.15× faster |
| `sys_udp_packets` | 62.6 | 133.5 | ✅ 2.13× faster |
| `sys_random_math` | 198.0 | 310.2 | ✅ 1.57× faster |
| `sys_hash_sha256` | 128.4 | 192.3 | ✅ 1.50× faster |
| `sys_sqlite` | 241.1 | 183.5 | ❌ 1.31× SLOWER |
| `sys_lock_contention` | 311.0 | 207.7 | ❌ 1.50× SLOWER |
| `sys_struct_pack` | 516.7 | 246.7 | ❌ 2.09× SLOWER |
| `sys_lru_cache` | 480.7 | 218.5 | ❌ 2.20× SLOWER |
| `sys_file_io` | 492.6 | 130.6 | ❌ 3.77× SLOWER |
