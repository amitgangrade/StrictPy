# StrictPy vs CPython — Comprehensive Benchmark v2

_Generated 2026-06-11 01:36:19 · StrictPy `spy 0.2.0` vs CPython 3.12.10 · Windows 11 · best-of-3 full-process runs, interleaved_

Process startup floor: StrictPy 20 ms · CPython 49 ms (included in every number below).

## Scoreboard

| Benchmarks | StrictPy wins (≥1.15×) | Ties | CPython wins (≥1.15×) | Wrong output | Failed to run |
|---|---|---|---|---|---|
| **59** | **23** | 1 | **34** | 0 | 1 |

**Geometric-mean speedup across all passing benchmarks: 0.72× vs CPython.**

## Core compute (JIT-friendly numeric / control flow)

| Benchmark | StrictPy (ms) | CPython (ms) | Verdict |
|---|---:|---:|---|
| `core_bitops` | 37.3 | 692.2 | ✅ 18.58× faster |
| `core_branchy` | 40.7 | 229.1 | ✅ 5.63× faster |
| `core_int_arith` | 31.0 | 167.9 | ✅ 5.42× faster |
| `core_loops_nested` | 27.0 | 145.6 | ✅ 5.40× faster |
| `core_float_arith` | 38.9 | 185.1 | ✅ 4.75× faster |
| `core_quicksort` | 34.9 | 127.3 | ✅ 3.64× faster |
| `core_recursion_fib` | 55.6 | 200.7 | ✅ 3.61× faster |
| `core_calls` | 64.0 | 201.9 | ✅ 3.15× faster |
| `core_recursion_ack` | 22.6 | 66.7 | ✅ 2.95× faster |
| `core_nbody` | 46.0 | 134.9 | ✅ 2.93× faster |
| `core_matrix` | 35.7 | 99.1 | ✅ 2.77× faster |
| `core_sieve` | 99.8 | 190.8 | ✅ 1.91× faster |

## Data structures & language features

| Benchmark | StrictPy (ms) | CPython (ms) | Verdict |
|---|---:|---:|---|
| `ds_class_alloc` | 111.8 | 251.5 | ✅ 2.25× faster |
| `ds_list_ops` | 122.7 | 241.2 | ✅ 1.97× faster |
| `ds_comprehensions` | 140.8 | 151.4 | ➖ tie |
| `ds_tuple_ops` | 267.2 | 208.9 | ❌ 1.28× SLOWER |
| `ds_virtual_dispatch` | 191.7 | 140.6 | ❌ 1.36× SLOWER |
| `ds_match_dispatch` | 126.4 | 75.2 | ❌ 1.68× SLOWER |
| `ds_list_sort` | 469.5 | 265.3 | ❌ 1.77× SLOWER |
| `ds_generators` | 602.7 | 327.2 | ❌ 1.84× SLOWER |
| `ds_nullable` | 429.2 | 208.6 | ❌ 2.06× SLOWER |
| `ds_generics` | 480.2 | 216.3 | ❌ 2.22× SLOWER |
| `ds_string_keys_aggregation` | 318.3 | 132.4 | ❌ 2.40× SLOWER |
| `ds_exceptions` | 507.4 | 156.8 | ❌ 3.24× SLOWER |
| `ds_sort_by_key` | 1,206.8 | 355.4 | ❌ 3.40× SLOWER |
| `ds_closures_hof` | 663.5 | 177.2 | ❌ 3.74× SLOWER |
| `ds_set_ops` | 424.4 | 93.6 | ❌ 4.53× SLOWER |
| `ds_dict_ops` | 1,917.7 | 297.4 | ❌ 6.45× SLOWER |

## Strings, text & serialization

| Benchmark | StrictPy (ms) | CPython (ms) | Verdict |
|---|---:|---:|---|
| `str_search` | 212.0 | 161.4 | ❌ 1.31× SLOWER |
| `str_regex` | 300.5 | 175.2 | ❌ 1.72× SLOWER |
| `str_json_roundtrip` | 361.6 | 210.5 | ❌ 1.72× SLOWER |
| `str_concat_build` | 632.3 | 256.3 | ❌ 2.47× SLOWER |
| `str_wordcount` | 507.2 | 155.3 | ❌ 3.27× SLOWER |
| `str_json_walk` | 536.1 | 148.7 | ❌ 3.60× SLOWER |
| `str_base64` | 818.7 | 183.3 | ❌ 4.47× SLOWER |
| `str_methods_mix` | 1,796.5 | 316.4 | ❌ 5.68× SLOWER |
| `str_template_render` | 948.2 | 149.7 | ❌ 6.33× SLOWER |
| `str_csv_parse` | 1,590.6 | 224.6 | ❌ 7.08× SLOWER |
| `str_join_build` | 2,483.6 | 244.3 | ❌ 10.16× SLOWER |
| `str_fstring_format` | 2,059.3 | 202.5 | ❌ 10.17× SLOWER |
| `str_split_scan` | 2,534.6 | 203.4 | ❌ 12.46× SLOWER |
| `str_slice_scan` | 12,502.4 | 189.0 | ❌ 66.17× SLOWER |
| `str_http_parse` | 25,078.1 | 86.3 | ❌ 290.62× SLOWER |

## Systems, concurrency & stdlib

| Benchmark | StrictPy (ms) | CPython (ms) | Verdict |
|---|---:|---:|---|
| `sys_rate_limiter` | 31.5 | 196.4 | ✅ 6.24× faster |
| `sys_threads_spawn` | 30.2 | 150.3 | ✅ 4.97× faster |
| `sys_channel_throughput` | 86.3 | 342.1 | ✅ 3.97× faster |
| `sys_pqueue` | 105.8 | 331.0 | ✅ 3.13× faster |
| `sys_tcp_echo` | 77.6 | 156.3 | ✅ 2.01× faster |
| `sys_datetime` | 114.8 | 195.6 | ✅ 1.70× faster |
| `sys_url_codec` | 315.1 | 518.8 | ✅ 1.65× faster |
| `sys_udp_packets` | 65.7 | 103.7 | ✅ 1.58× faster |
| `sys_random_math` | 165.4 | 260.8 | ✅ 1.58× faster |
| `sys_lock_contention` | 242.3 | 167.0 | ❌ 1.45× SLOWER |
| `sys_hash_sha256` | 261.5 | 147.7 | ❌ 1.77× SLOWER |
| `sys_sqlite` | 308.6 | 143.4 | ❌ 2.15× SLOWER |
| `sys_lru_cache` | 556.7 | 174.3 | ❌ 3.19× SLOWER |
| `sys_file_io` | 418.2 | 105.7 | ❌ 3.96× SLOWER |
| `sys_struct_pack` | 1,019.2 | 207.1 | ❌ 4.92× SLOWER |
| `sys_async_tasks` | — | — | 💥 FAIL |

## Correctness & stability issues

- **sys_async_tasks**: SPY RUN FAIL: rc=3221225477
stdout:

stderr:

