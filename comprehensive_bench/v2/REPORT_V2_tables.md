# StrictPy vs CPython — Comprehensive Benchmark v2

_Generated 2026-06-15 00:34:16 · StrictPy `spy 0.2.0` vs CPython 3.12.10 · Windows 11 · best-of-3 full-process runs, interleaved_

Process startup floor: StrictPy 16 ms · CPython 60 ms (included in every number below).

## Scoreboard

| Benchmarks | StrictPy wins (≥1.15×) | Ties | CPython wins (≥1.15×) | Wrong output | Failed to run |
|---|---|---|---|---|---|
| **59** | **26** | 0 | **33** | 0 | 0 |

**Geometric-mean speedup across all passing benchmarks: 0.96× vs CPython.**

## Core compute (JIT-friendly numeric / control flow)

| Benchmark | StrictPy (ms) | CPython (ms) | Verdict |
|---|---:|---:|---|
| `core_bitops` | 35.3 | 855.7 | ✅ 24.23× faster |
| `core_loops_nested` | 26.3 | 169.6 | ✅ 6.44× faster |
| `core_int_arith` | 31.1 | 194.3 | ✅ 6.25× faster |
| `core_branchy` | 44.9 | 269.2 | ✅ 6.00× faster |
| `core_float_arith` | 40.4 | 211.8 | ✅ 5.24× faster |
| `core_recursion_fib` | 49.0 | 198.0 | ✅ 4.04× faster |
| `core_quicksort` | 34.5 | 139.2 | ✅ 4.04× faster |
| `core_recursion_ack` | 18.3 | 69.6 | ✅ 3.80× faster |
| `core_calls` | 68.2 | 224.4 | ✅ 3.29× faster |
| `core_matrix` | 35.4 | 113.0 | ✅ 3.20× faster |
| `core_nbody` | 46.6 | 142.5 | ✅ 3.06× faster |
| `core_sieve` | 94.0 | 185.7 | ✅ 1.98× faster |

## Data structures & language features

| Benchmark | StrictPy (ms) | CPython (ms) | Verdict |
|---|---:|---:|---|
| `ds_class_alloc` | 104.2 | 242.5 | ✅ 2.33× faster |
| `ds_list_ops` | 116.1 | 234.5 | ✅ 2.02× faster |
| `ds_comprehensions` | 126.0 | 147.5 | ✅ 1.17× faster |
| `ds_tuple_ops` | 256.2 | 204.3 | ❌ 1.25× SLOWER |
| `ds_virtual_dispatch` | 181.2 | 130.4 | ❌ 1.39× SLOWER |
| `ds_list_sort` | 377.9 | 257.7 | ❌ 1.47× SLOWER |
| `ds_match_dispatch` | 117.5 | 68.0 | ❌ 1.73× SLOWER |
| `ds_generators` | 584.1 | 322.4 | ❌ 1.81× SLOWER |
| `ds_nullable` | 385.9 | 207.6 | ❌ 1.86× SLOWER |
| `ds_string_keys_aggregation` | 229.6 | 122.7 | ❌ 1.87× SLOWER |
| `ds_generics` | 397.8 | 212.4 | ❌ 1.87× SLOWER |
| `ds_sort_by_key` | 1,016.1 | 342.7 | ❌ 2.97× SLOWER |
| `ds_exceptions` | 459.8 | 150.9 | ❌ 3.05× SLOWER |
| `ds_set_ops` | 327.9 | 87.1 | ❌ 3.76× SLOWER |
| `ds_closures_hof` | 654.6 | 173.1 | ❌ 3.78× SLOWER |
| `ds_dict_ops` | 1,552.5 | 279.4 | ❌ 5.56× SLOWER |

## Strings, text & serialization

| Benchmark | StrictPy (ms) | CPython (ms) | Verdict |
|---|---:|---:|---|
| `str_slice_scan` | 131.6 | 175.1 | ✅ 1.33× faster |
| `str_search` | 197.1 | 149.6 | ❌ 1.32× SLOWER |
| `str_json_roundtrip` | 309.9 | 201.3 | ❌ 1.54× SLOWER |
| `str_regex` | 256.7 | 159.2 | ❌ 1.61× SLOWER |
| `str_concat_build` | 491.8 | 247.0 | ❌ 1.99× SLOWER |
| `str_wordcount` | 344.9 | 148.8 | ❌ 2.32× SLOWER |
| `str_json_walk` | 432.8 | 137.0 | ❌ 3.16× SLOWER |
| `str_split_scan` | 672.4 | 191.1 | ❌ 3.52× SLOWER |
| `str_base64` | 726.6 | 174.4 | ❌ 4.17× SLOWER |
| `str_methods_mix` | 1,411.9 | 296.9 | ❌ 4.76× SLOWER |
| `str_template_render` | 715.8 | 139.7 | ❌ 5.12× SLOWER |
| `str_join_build` | 1,222.0 | 225.6 | ❌ 5.42× SLOWER |
| `str_csv_parse` | 1,243.9 | 216.2 | ❌ 5.75× SLOWER |
| `str_fstring_format` | 1,575.3 | 191.7 | ❌ 8.22× SLOWER |
| `str_http_parse` | 1,004.5 | 80.2 | ❌ 12.53× SLOWER |

## Systems, concurrency & stdlib

| Benchmark | StrictPy (ms) | CPython (ms) | Verdict |
|---|---:|---:|---|
| `sys_rate_limiter` | 25.8 | 187.9 | ✅ 7.28× faster |
| `sys_threads_spawn` | 22.5 | 146.0 | ✅ 6.50× faster |
| `sys_channel_throughput` | 77.5 | 315.6 | ✅ 4.07× faster |
| `sys_pqueue` | 98.7 | 317.6 | ✅ 3.22× faster |
| `sys_async_tasks` | 59.0 | 173.4 | ✅ 2.94× faster |
| `sys_tcp_echo` | 69.2 | 144.0 | ✅ 2.08× faster |
| `sys_url_codec` | 262.3 | 503.8 | ✅ 1.92× faster |
| `sys_datetime` | 102.3 | 184.9 | ✅ 1.81× faster |
| `sys_udp_packets` | 54.3 | 94.2 | ✅ 1.74× faster |
| `sys_random_math` | 152.2 | 254.5 | ✅ 1.67× faster |
| `sys_hash_sha256` | 204.3 | 138.5 | ❌ 1.48× SLOWER |
| `sys_lock_contention` | 229.4 | 152.8 | ❌ 1.50× SLOWER |
| `sys_sqlite` | 269.8 | 133.3 | ❌ 2.02× SLOWER |
| `sys_lru_cache` | 458.6 | 167.7 | ❌ 2.73× SLOWER |
| `sys_file_io` | 370.1 | 95.5 | ❌ 3.87× SLOWER |
| `sys_struct_pack` | 842.6 | 198.6 | ❌ 4.24× SLOWER |
