# Wave 3 — top-PyPI leverage modules: `requests` (M65), `ndarray` (M66), `crypto` (M67)

Goal: close the three biggest gaps between StrictPy's stdlib and the top-50
PyPI download list (HTTP-for-humans, numpy-shaped arrays, crypto+JWT).

Execution model: the scaffold (this commit) froze all interfaces; three lanes
implement in parallel worktree sub-agents; the coordinator does commits, the
merge train, release rebuild, and full-suite verification.

## Frozen contracts — do not change signatures or ids

| Lane | Module | Spec | NativeFn ids | Test file |
|------|--------|------|--------------|-----------|
| A | `requests` | STRICTPY_SPEC.md §9.51 | 1500–1539 | `vm/tests/m65_requests.rs` |
| B | `ndarray`  | STRICTPY_SPEC.md §9.52 | 1600–1657 | `vm/tests/m66_ndarray.rs` |
| C | `crypto`   | STRICTPY_SPEC.md §9.53 | 1700–1711 | `vm/tests/m67_crypto.rs` |

The spec section is the single source of truth for every signature, id, and
semantic rule. If the contract cannot be implemented as written, STOP and
report the blocker — do not redesign.

## What the scaffold already landed (do not re-add)

- `shared/src/native.rs`: ALL wave-3 `NativeFn` enum variants + `from_u32`
  entries. Complete — lanes do not touch this file.
- `compiler/src/ir.rs`: `m65_requests_class_method_native_id_by_name` and
  `m66_ndarray_class_method_native_id_by_name` dispatch tables + their hooks
  in `lower_method_call`. Complete — lanes do not touch this file.
- `vm/src/builtins.rs`: grouped trap arms inside `=== WAVE3 LANE X ===`
  marker regions. Each lane REPLACES the contents of its own region with
  real handlers. Never edit outside your own markers.
- `compiler/src/resolver.rs`: `=== WAVE3 LANE X ===` marker regions inside
  `seed_stdlib_modules` where each lane builds its `StdlibModule` (and, for
  A/B, registers its native classes). Never edit outside your own markers.
- `vm/src/interp.rs`: `=== WAVE3 LANE A/B ===` marker regions on the
  `SharedVm` struct and BOTH constructors for slot-table fields.
- `vm/Cargo.toml`: all deps (ureq `cookies` feature; aes-gcm, pbkdf2, hkdf,
  ed25519-dalek, getrandom, subtle). Already built once — lanes do not touch
  Cargo.toml.

## Rules for every lane (agent brief)

- Work only in: your marker regions (resolver.rs, builtins.rs, and for A/B
  interp.rs), plus your OWN new test file, plus lane-private helper code.
  If you need a helper module, create a new file (e.g. `vm/src/requests.rs`)
  and wire it with a single `mod` line inside your builtins.rs region — do
  not add shared helpers.
- Prefix locals in shared files: `m65_` (A), `m66_` (B), `m67_` (C).
- Exemplars to copy (find by grep, not by line number):
  - resolver module shape: `insert("argparse"` (flat fns),
    `insert("http_client"` (tuple returns), `insert("json"` (JsonValue refs).
  - resolver native-class registration: `class_name_to_id.insert("DataFrame"`
    and the `Hasher` block (`is_native: true, payload_size: 0`).
  - builtins handler shape: `NativeFn::HashlibSha256`, `NativeFn::HttpClientGet`,
    tabular `M37TabDf*` arms (slot-table access), `Hasher*` arms.
  - SharedVm slot tables: `sqlite_cursors` + `next_cursor_id` convention
    (Mutex<HashMap<i64, Slot>>, handle 0 reserved, first handle = 1).
- `vm/src/builtins.rs` is 28k lines and `compiler/src/resolver.rs` is 8k:
  NEVER read whole files — Grep for anchors, then read with offset/limit.
- Batch all edits, then build once. Do not build after every file.
- Verify with `cargo build` and `cargo test --test m6X_<yourlane>` ONLY.
  Do NOT run the workspace suite — the coordinator does that centrally.
- When done: `git add -A`. Do NOT `git commit` / `git push` / use `gh` /
  use env-var-prefixed commands (they are permission-blocked anyway).
- Final report: worktree path, what's implemented, test count + pass status,
  any contract blockers, anything the coordinator must verify centrally.

## Lane A — `requests` (M65)

Everything TLS/certs is already solved by the engine: ureq bundles rustls +
webpki-roots (certifi parity). The work is the ergonomic layer per §9.51:
Response/Session native classes (handle-backed), session = persistent
`ureq::Agent` (cookie store on via the `cookies` feature), one-shot module
fns, `download` streaming to file, str-as-byte-buffer bodies (10 MiB cap).
Look at how the existing `http_client` builtins arms build ureq requests and
map errors to IOError — reuse those conventions verbatim. Tests: start a
local TCP listener thread inside the test (grep existing http/socket tests
in `vm/tests/` for the pattern) — tests must not hit the real internet.

## Lane B — `ndarray` (M66)

Handle-backed NDArray per §9.52 — a `SharedVm` slot table holds
`{ shape: Vec<i64>, data: Vec<f64> }` (row-major); NO new GC heap object
type; copies-not-views everywhere. The compiler-side work (class
registration + module table in your resolver region) mirrors DataFrame; the
VM-side work is a big but simple family of handlers over the slot table.
Broadcasting rules, axis semantics, and error cases are all pinned in §9.52
— implement exactly those, nothing more. Precision target for tests:
exact equality for structural ops, 1e-12 tolerance for float math.

## Lane C — `crypto` (M67)

Flat functions per §9.53 over RustCrypto crates (already in Cargo.toml).
No classes, no handles, no SharedVm changes — resolver region + builtins
region + tests only. HMAC/digests already live in `hashlib` — do not
duplicate. JWT: base64url without padding (the `base64` crate is in-tree);
enforce the alg-confusion rule in §9.53. Tests MUST include published
vectors: NIST AES-GCM, RFC 6070 PBKDF2, RFC 5869 HKDF, RFC 8032 Ed25519,
plus a jwt.io-style HS256 round-trip.

## Phase 2 — merge train + verification (coordinator only)

1. Commit each lane's staged worktree on its branch (agents can't commit).
2. Merge order: C → A → B (lowest risk first).
3. After merging: `cargo build --release` — integration tests shell out to
   `target/release/spy.exe`; stale-binary symptom is
   `VM trap: CALL_NATIVE: unknown native id 15xx/16xx/17xx`.
4. Full suite: `cargo test --workspace --config 'env.RUST_MIN_STACK="16777216"'`.
5. Perf sanity: back-to-back binary A/B on comprehensive_bench.
6. `graphify update .`; drop this file once everything lives in code + spec.
