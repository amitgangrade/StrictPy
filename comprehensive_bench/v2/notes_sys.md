# notes_sys.md — findings from building the sys_* benchmark pairs

Observations from writing/verifying the 16 `sys_*` StrictPy/CPython pairs in
`comprehensive_bench/v2/programs/` (spy.exe built 2026-06-10, branch
`perf/single-alloc-strings`).

## Bugs (confirmed against the current spy.exe)

- **asyncio is completely broken at runtime**: `asyncio.run_i32`,
  `asyncio.run_unit`, and `asyncio.spawn_*` all crash instantly with an access
  violation (exit 0xC0000005), even with a no-op closure
  (`asyncio.run_unit(w)` where `w` is `fn w() -> None: pass`). `import asyncio`
  and `asyncio.sleep()` outside the runtime are fine. As a result
  `sys_async_tasks.spy` compiles cleanly but cannot run; its CPython twin is
  verified. Minimal repro: `scratch/apitest/az_c.spy`.
- **`del d[k]` on a Dict is a silent no-op**: it parses and type-checks, but
  lowers to nothing (`compiler/src/ir.rs`: `Stmt::Del { .. } => Some(())`).
  `len(d)` and `d.get(k)` are unchanged after `del`. Combined with the absence
  of `Dict.remove/pop`, there is **no way to remove a Dict key**, which rules
  out classic eviction-list LRU caches; `sys_lru_cache` had to use a
  two-generation (segmented) eviction design instead.
- **char → int conversion traps at runtime**: `i32(c)` / `i64(c)` on a `char`
  compiles but dies with `VM trap: CALL_NATIVE: native id 0xFFFF_FFFF
  (Unknown) is not callable`. There is also no `ord()`/`chr()`. This blocks
  byte-level checksumming of strings (worked around with `len()`-based
  checksums in `sys_file_io`).
- **Integer literal inference bug in annotated lets**:
  `cost: i64 = 1 + state % 4` (state: i64) fails with
  `error[E2001]: expected i32, got i64`, pointing at `state`. Workarounds:
  `1i64 + state % 4` or reordering to `state % 4 + 1`.

## LANGUAGE_GUIDE.md drift (trust the generated examples / resolver instead)

- `threading.lock()` documented; actual is `threading.lock_new()`
  (plus `lock_acquire` / `lock_release`, which match).
- `queue` documented as `priority_queue()` / `pq_push(handle, i64, str)`;
  actual surface is `pq_new_i64()` / `pq_new_str()`,
  `pq_push_i64(h, prio: f64, item: i64)`, `pq_pop_min_i64(h) -> Tuple[f64, i64]`,
  `pq_peek_min_*`, `pq_len`, `pq_is_empty`. Priorities are always `f64`.
- `struct` documented as `pack_i32/pack_i64/pack_f32/pack_f64`; actual is
  endian-explicit unsigned/float only: `pack_u32_be/le`, `pack_u64_be/le`,
  `pack_f64_be/le` and matching `unpack_*(buf, offset)`. No i32/i64/f32 packers.
- `urllib_parse.parse_qs(query) -> Dict[str, str]` documented; actual is
  `parse_query(query) -> List[Tuple[str, str]]` (matches Python's `parse_qsl`).
  Also available (undocumented): `quote_plus` / `unquote_plus`.
- `Channel()` (guide §4.2/§6.5) does not compile; the real constructor needs an
  explicit type arg **and** capacity: `Channel[i32](1024)`. Channels are
  bounded; `send` blocks when full.
- `Dict.length()` (guide §6.3) does not exist on Dict (`E2004`); `len(d)` works.
  `List.length()` works.

## Semantics notes useful for paired benchmarks

- `urllib_parse.quote` percent-encodes `/` — match CPython with
  `quote(s, safe="")`. `urlencode` uses form-encoding (`+` for space) exactly
  like CPython's default; `parse_query` ≡ `parse_qsl`.
- `datetime.to_iso/from_iso/weekday/year/month/day/add_days` match CPython
  `datetime.fromtimestamp(ts, timezone.utc)` exactly (verified over 60k
  LCG-random timestamps; weekday is 0=Mon like Python).
- `math.sqrt/sin/cos/log/exp` produced i64-quantized results identical to
  CPython across 300k values on this machine (Rust and MSVC both lower to the
  same ucrt/SSE2 routines); raw float printing still differs (`str(f64)`
  formatting), so quantize to ints.
- sqlite cells are always stringified; `SUM(...)` comes back as e.g. `"19980000"`
  — fine for integer columns, but float aggregates would need careful printing.
- Inline closures (`fn() -> T: expr`) are single-expression only; multi-statement
  bodies don't parse. Thread/async bodies therefore call a named function:
  `Thread(fn() -> None: worker(args))`.

## Missing capabilities relevant to distributed-systems work

- No working async runtime (see crash above), no thread pool, no
  select/epoll-style readiness API, no non-blocking socket mode — one OS thread
  per connection is the only concurrency model that runs today.
- No Dict key removal (see above) — server-style caches/session tables can only
  grow, be rebuilt wholesale, or use generation rotation.
- No `socket.set_reuse_addr` — the TCP listener port-retry loop from the
  generated examples is the necessary idiom.
- `queue` priority queues are handle-based (`i64`), values limited to
  `i64`/`str` payloads with `f64` priorities; no arbitrary task objects.
- No connection pooling / keep-alive HTTP server primitives (http_client is
  client-only; serving HTTP means hand-rolling on TCP sockets).
- Generic `gather` is limited to 2-4 futures (`gather_2_i32` ...); awaiting N
  futures means a `List[Future[i32]]` + loop (compiles, but see asyncio crash).

## Tooling note

- `cargo build --release` initially failed with `LNK1181: cannot open input
  file 'SDL2_image.lib'`; the import lib must be present at
  `target/release/build/sdl2-sys-*/out/lib/` (copy from
  `third_party/SDL2_image/SDL2_image-2.8.2/lib/x64/`).

## Verification status

All 16 pairs produce byte-identical stdout and exit 0 on both sides, except
`sys_async_tasks` whose StrictPy side crashes in `asyncio.run_i32` (CPython
side verified). Runner: `programs/verify_pairs.py` (runs both twins from a
scratch cwd and diffs stdout).

## Correction (post-analysis, 2026-06-11)

The asyncio crash was initially suspected to be a regression from the
uncommitted single-alloc-strings VM changes. This was DISPROVEN: a clean
binary built from committed HEAD (3825315) crashes identically
(0xC0000005 on any asyncio.run_*/spawn_*). The WIP changes are
performance-neutral across the whole v2 suite (results_v2_wip.json vs
results_v2_clean.json).
