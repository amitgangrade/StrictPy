# StrictPy vs CPython — Comprehensive Benchmark v2

_Generated 2026-06-11 20:06:46 · StrictPy `spy 0.2.0` vs CPython 3.12.10 · Windows 11 · best-of-3 full-process runs, interleaved_

Process startup floor: StrictPy 13 ms · CPython 31 ms (included in every number below).

## Scoreboard

| Benchmarks | StrictPy wins (≥1.15×) | Ties | CPython wins (≥1.15×) | Wrong output | Failed to run |
|---|---|---|---|---|---|
| **59** | **24** | 1 | **34** | 0 | 0 |

**Geometric-mean speedup across all passing benchmarks: 0.77× vs CPython.**

## Core compute (JIT-friendly numeric / control flow)

| Benchmark | StrictPy (ms) | CPython (ms) | Verdict |
|---|---:|---:|---|
| `core_bitops` | 32.1 | 829.2 | ✅ 25.84× faster |
| `core_branchy` | 38.5 | 255.1 | ✅ 6.62× faster |
| `core_int_arith` | 26.9 | 175.8 | ✅ 6.52× faster |
| `core_loops_nested` | 24.2 | 153.1 | ✅ 6.32× faster |
| `core_float_arith` | 37.5 | 195.7 | ✅ 5.21× faster |
| `core_recursion_fib` | 57.1 | 211.8 | ✅ 3.71× faster |
| `core_quicksort` | 34.7 | 127.2 | ✅ 3.66× faster |
| `core_calls` | 68.1 | 209.6 | ✅ 3.08× faster |
| `core_nbody` | 45.4 | 137.8 | ✅ 3.03× faster |
| `core_recursion_ack` | 17.1 | 50.9 | ✅ 2.98× faster |
| `core_matrix` | 34.5 | 92.4 | ✅ 2.68× faster |
| `core_sieve` | 114.8 | 212.8 | ✅ 1.85× faster |

## Data structures & language features

| Benchmark | StrictPy (ms) | CPython (ms) | Verdict |
|---|---:|---:|---|
| `ds_class_alloc` | 130.0 | 279.0 | ✅ 2.15× faster |
| `ds_list_ops` | 152.9 | 296.5 | ✅ 1.94× faster |
| `ds_comprehensions` | 136.0 | 136.2 | ➖ tie |
| `ds_tuple_ops` | 269.2 | 198.2 | ❌ 1.36× SLOWER |
| `ds_virtual_dispatch` | 194.5 | 125.7 | ❌ 1.55× SLOWER |
| `ds_list_sort` | 523.6 | 330.5 | ❌ 1.58× SLOWER |
| `ds_generators` | 687.9 | 340.8 | ❌ 2.02× SLOWER |
| `ds_generics` | 410.0 | 202.5 | ❌ 2.02× SLOWER |
| `ds_nullable` | 532.3 | 244.2 | ❌ 2.18× SLOWER |
| `ds_match_dispatch` | 152.1 | 66.0 | ❌ 2.30× SLOWER |
| `ds_string_keys_aggregation` | 289.3 | 113.4 | ❌ 2.55× SLOWER |
| `ds_sort_by_key` | 1,092.2 | 365.0 | ❌ 2.99× SLOWER |
| `ds_exceptions` | 579.1 | 167.9 | ❌ 3.45× SLOWER |
| `ds_closures_hof` | 704.1 | 161.0 | ❌ 4.37× SLOWER |
| `ds_set_ops` | 454.6 | 87.5 | ❌ 5.19× SLOWER |
| `ds_dict_ops` | 1,633.1 | 283.6 | ❌ 5.76× SLOWER |

## Strings, text & serialization

| Benchmark | StrictPy (ms) | CPython (ms) | Verdict |
|---|---:|---:|---|
| `str_search` | 244.8 | 154.3 | ❌ 1.59× SLOWER |
| `str_json_roundtrip` | 364.8 | 210.0 | ❌ 1.74× SLOWER |
| `str_regex` | 263.5 | 148.2 | ❌ 1.78× SLOWER |
| `str_concat_build` | 511.7 | 255.1 | ❌ 2.01× SLOWER |
| `str_wordcount` | 497.8 | 154.7 | ❌ 3.22× SLOWER |
| `str_json_walk` | 498.2 | 136.9 | ❌ 3.64× SLOWER |
| `str_base64` | 757.0 | 161.5 | ❌ 4.69× SLOWER |
| `str_methods_mix` | 1,642.8 | 344.3 | ❌ 4.77× SLOWER |
| `str_join_build` | 1,475.5 | 274.5 | ❌ 5.38× SLOWER |
| `str_csv_parse` | 1,306.3 | 214.0 | ❌ 6.10× SLOWER |
| `str_template_render` | 791.6 | 128.7 | ❌ 6.15× SLOWER |
| `str_split_scan` | 1,371.4 | 185.4 | ❌ 7.40× SLOWER |
| `str_fstring_format` | 1,686.6 | 190.6 | ❌ 8.85× SLOWER |
| `str_http_parse` | 1,462.5 | 79.3 | ❌ 18.45× SLOWER |
| `str_slice_scan` | 28,662.5 | 173.4 | ❌ 165.33× SLOWER |

## Systems, concurrency & stdlib

| Benchmark | StrictPy (ms) | CPython (ms) | Verdict |
|---|---:|---:|---|
| `sys_rate_limiter` | 26.6 | 187.2 | ✅ 7.04× faster |
| `sys_threads_spawn` | 23.8 | 133.1 | ✅ 5.60× faster |
| `sys_channel_throughput` | 78.5 | 321.5 | ✅ 4.09× faster |
| `sys_pqueue` | 105.2 | 349.8 | ✅ 3.33× faster |
| `sys_url_codec` | 289.8 | 547.4 | ✅ 1.89× faster |
| `sys_async_tasks` | 70.4 | 126.7 | ✅ 1.80× faster |
| `sys_datetime` | 102.0 | 176.4 | ✅ 1.73× faster |
| `sys_tcp_echo` | 74.4 | 128.3 | ✅ 1.73× faster |
| `sys_random_math` | 168.5 | 255.5 | ✅ 1.52× faster |
| `sys_udp_packets` | 59.7 | 76.3 | ✅ 1.28× faster |
| `sys_lock_contention` | 254.7 | 163.7 | ❌ 1.56× SLOWER |
| `sys_hash_sha256` | 246.3 | 146.0 | ❌ 1.69× SLOWER |
| `sys_sqlite` | 304.5 | 123.0 | ❌ 2.48× SLOWER |
| `sys_lru_cache` | 592.5 | 165.1 | ❌ 3.59× SLOWER |
| `sys_struct_pack` | 907.1 | 192.4 | ❌ 4.71× SLOWER |
| `sys_file_io` | 411.7 | 85.6 | ❌ 4.81× SLOWER |
