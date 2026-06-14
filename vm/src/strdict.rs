//! String-keyed dict storage with explicit (cacheable) hashes.
//!
//! Perf background (comprehensive_bench/v2 weakness #4): `ds_dict_ops` was
//! ~5.5× slower than CPython because every `Dict[str, V]` operation paid
//! three avoidable costs per call:
//!
//! 1. copying the key's bytes out of the heap `StringRepr` into a fresh
//!    Rust `String` (`arg_str`),
//! 2. hashing the full key with std's DoS-hardened SipHash, and
//! 3. doing both *again* on the next operation with the same key object.
//!
//! [`StrDict`] fixes all three. It is a thin wrapper over
//! `hashbrown::HashTable` that stores `(hash, key, value)` entries and takes
//! the 64-bit hash *as a parameter*, so the Dict* natives can:
//!
//! * hash with FxHash (one multiply per 8 key bytes) instead of SipHash,
//! * cache the hash in the key string's object header (`gc_meta` bits — see
//!   [`crate::object::cached_str_hash`]) and reuse it on every later
//!   operation on the same string object, and
//! * look keys up by a borrowed `&[u8]` view of the heap string — zero
//!   String allocations on get/has/remove and on overwriting inserts.
//!
//! ## Hasher choice: FxHash vs SipHash (DoS hardening tradeoff)
//!
//! std's default SipHash-1-3 is keyed and collision-resistant so untrusted
//! keys can't degenerate the table to O(n) per op (hash-flooding DoS).
//! FxHash is not collision-resistant: an adversary who controls dict keys
//! can construct arbitrarily many colliding keys. We accept that tradeoff
//! deliberately: StrictPy's VM runs *trusted, locally compiled programs* —
//! it is not a network-facing service parsing attacker-supplied input into
//! dicts by default. A program that feeds untrusted input into a dict takes
//! the same risk every Fx-keyed compiler table (rustc itself) takes. If a
//! hardened profile is ever needed, swapping the two `fx_hash32` call sites
//! for a keyed hasher is contained to this module.
//!
//! The stored hash is the *spread* 32-bit Fx hash (see [`dict_hash_bytes`]):
//! 32 cache bits are what fit in the `gc_meta` header word next to the GC
//! bits, and the Fibonacci spread re-fills all 64 bits so hashbrown's
//! control bytes (top 7 bits) and bucket index (low bits) both get entropy.

use std::hash::Hasher;

use hashbrown::hash_table::{Entry as TableEntry, HashTable};

use crate::object::{cached_str_hash, publish_str_hash, ObjectHeader, StringRepr};

/// 32-bit FxHash of a byte string. The high half of the 64-bit Fx state is
/// kept — Fx's final mix concentrates entropy there.
#[inline]
pub fn fx_hash32(bytes: &[u8]) -> u32 {
    let mut h = rustc_hash::FxHasher::default();
    h.write(bytes);
    (h.finish() >> 32) as u32
}

/// Spread a 32-bit hash over 64 bits (Fibonacci multiply). Deterministic —
/// this is the *only* function that turns a cached 32-bit hash into the
/// table hash, so cached and freshly computed hashes always agree.
#[inline]
pub fn spread32(h32: u32) -> u64 {
    ((h32 as u64) ^ ((h32 as u64) << 32)).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// The canonical 64-bit table hash of a key's bytes. Every [`StrDict`]
/// entry's stored hash equals `dict_hash_bytes(key.as_bytes())`.
#[inline]
pub fn dict_hash_bytes(bytes: &[u8]) -> u64 {
    spread32(fx_hash32(bytes))
}

/// Hash + borrowed byte view of a heap string used as a dict key.
///
/// Returns the canonical table hash and the key's UTF-8 bytes *borrowed in
/// place* from the heap block (no copy). On a cache miss the 32-bit hash is
/// computed and published into the string's header so the next operation on
/// the same string object skips the hash entirely.
///
/// Null pointer / null data → the empty key (matches `read_str`).
///
/// # Safety
/// `p` must be null or point at a valid `StringRepr` on the VM heap that
/// stays alive (rooted) and unmutated for the returned borrow's use. The
/// caller must not run user code / GC while holding the byte view.
pub unsafe fn str_key_hash_bytes<'a>(p: *const StringRepr) -> (u64, &'a [u8]) {
    if p.is_null() {
        return (dict_hash_bytes(&[]), &[]);
    }
    let data = (*p).data;
    let len = (*p).byte_len;
    if data.is_null() || len == 0 {
        return (dict_hash_bytes(&[]), &[]);
    }
    let bytes = std::slice::from_raw_parts(data, len);
    let hdr = p as *const ObjectHeader;
    let h32 = match cached_str_hash(hdr) {
        Some(h) => h,
        None => {
            let h = fx_hash32(bytes);
            publish_str_hash(hdr as *mut ObjectHeader, h);
            h
        }
    };
    (spread32(h32), bytes)
}

/// One `(hash, key, value)` entry. The hash is stored so table growth
/// rehashes by copying a u64 instead of re-running Fx over every key.
struct Entry {
    hash: u64,
    key: String,
    val: u64,
}

/// String-keyed `u64` table backing one VM dict slot (see
/// [`crate::interp::DictSlot`]).
///
/// The plain-`&str` methods (`get` / `insert` / `contains_key` / `remove`)
/// mirror the `HashMap<String, u64>` API this replaced, so the many
/// Rust-side callers (sets, counters, argparse, tabular) compile unchanged.
/// The `*_hashed` methods are the hot path used by the Dict* natives: they
/// take the precomputed table hash plus a borrowed byte view of the key.
#[derive(Default)]
pub struct StrDict {
    table: HashTable<Entry>,
}

impl StrDict {
    pub fn new() -> Self {
        Self {
            table: HashTable::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }

    // ── Hot path: precomputed hash + borrowed key bytes ─────────────────
    //
    // Key equality is byte equality. Stored keys are produced from valid
    // UTF-8 (every VM string comes from a Rust `&str`), so this matches
    // String equality on every reachable input.

    pub fn get_hashed(&self, hash: u64, key: &[u8]) -> Option<u64> {
        self.table
            .find(hash, |e| e.key.as_bytes() == key)
            .map(|e| e.val)
    }

    pub fn contains_hashed(&self, hash: u64, key: &[u8]) -> bool {
        self.table.find(hash, |e| e.key.as_bytes() == key).is_some()
    }

    /// Insert / overwrite. Allocates an owned key String only when the key
    /// is actually new. Returns the previous value, if any.
    pub fn insert_hashed(&mut self, hash: u64, key: &[u8], val: u64) -> Option<u64> {
        match self
            .table
            .entry(hash, |e| e.key.as_bytes() == key, |e| e.hash)
        {
            TableEntry::Occupied(mut o) => {
                let prev = o.get().val;
                o.get_mut().val = val;
                Some(prev)
            }
            TableEntry::Vacant(v) => {
                v.insert(Entry {
                    hash,
                    key: String::from_utf8_lossy(key).into_owned(),
                    val,
                });
                None
            }
        }
    }

    pub fn remove_hashed(&mut self, hash: u64, key: &[u8]) -> Option<u64> {
        match self.table.find_entry(hash, |e| e.key.as_bytes() == key) {
            Ok(occupied) => Some(occupied.remove().0.val),
            Err(_) => None,
        }
    }

    // ── HashMap-compatible convenience API (Rust-side String keys) ──────

    pub fn get(&self, key: &str) -> Option<&u64> {
        self.table
            .find(dict_hash_bytes(key.as_bytes()), |e| e.key == key)
            .map(|e| &e.val)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.contains_hashed(dict_hash_bytes(key.as_bytes()), key.as_bytes())
    }

    pub fn insert(&mut self, key: String, val: u64) -> Option<u64> {
        let hash = dict_hash_bytes(key.as_bytes());
        match self
            .table
            .entry(hash, |e| e.key == key, |e| e.hash)
        {
            TableEntry::Occupied(mut o) => {
                let prev = o.get().val;
                o.get_mut().val = val;
                Some(prev)
            }
            TableEntry::Vacant(v) => {
                v.insert(Entry { hash, key, val });
                None
            }
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<u64> {
        self.remove_hashed(dict_hash_bytes(key.as_bytes()), key.as_bytes())
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.table.iter().map(|e| &e.key)
    }

    pub fn values(&self) -> impl Iterator<Item = &u64> {
        self.table.iter().map(|e| &e.val)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &u64)> {
        self.table.iter().map(|e| (&e.key, &e.val))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashed_and_plain_paths_agree() {
        let mut d = StrDict::new();
        let (k, v) = ("hello", 42u64);
        let h = dict_hash_bytes(k.as_bytes());
        assert_eq!(d.insert_hashed(h, k.as_bytes(), v), None);
        // Plain-path lookup must find the hashed-path insert and vice versa.
        assert_eq!(d.get(k).copied(), Some(v));
        assert!(d.contains_key(k));
        assert_eq!(d.insert("hello".to_string(), 7), Some(42));
        assert_eq!(d.get_hashed(h, k.as_bytes()), Some(7));
        assert_eq!(d.remove_hashed(h, k.as_bytes()), Some(7));
        assert!(d.is_empty());
    }

    #[test]
    fn overwrite_does_not_grow() {
        let mut d = StrDict::new();
        let h = dict_hash_bytes(b"k");
        d.insert_hashed(h, b"k", 1);
        d.insert_hashed(h, b"k", 2);
        assert_eq!(d.len(), 1);
        assert_eq!(d.get_hashed(h, b"k"), Some(2));
    }

    #[test]
    fn many_keys_survive_growth_rehash() {
        // Growth rehashes via the stored `e.hash` — verify lookups still
        // work after multiple table resizes.
        let mut d = StrDict::new();
        for i in 0..10_000u64 {
            let k = format!("key-{i}");
            d.insert_hashed(dict_hash_bytes(k.as_bytes()), k.as_bytes(), i);
        }
        assert_eq!(d.len(), 10_000);
        for i in (0..10_000u64).step_by(7) {
            let k = format!("key-{i}");
            assert_eq!(d.get_hashed(dict_hash_bytes(k.as_bytes()), k.as_bytes()), Some(i));
        }
        for i in 0..5_000u64 {
            let k = format!("key-{i}");
            assert_eq!(d.remove(&k), Some(i));
        }
        assert_eq!(d.len(), 5_000);
        assert!(!d.contains_key("key-0"));
        assert!(d.contains_key("key-9999"));
    }

    #[test]
    fn unicode_keys() {
        let mut d = StrDict::new();
        for k in ["héllo", "日本語", "🦀", "ключ"] {
            d.insert(k.to_string(), k.len() as u64);
        }
        for k in ["héllo", "日本語", "🦀", "ключ"] {
            let h = dict_hash_bytes(k.as_bytes());
            assert_eq!(d.get_hashed(h, k.as_bytes()), Some(k.len() as u64), "{k}");
        }
    }

    #[test]
    fn iteration_views() {
        let mut d = StrDict::new();
        d.insert("a".into(), 1);
        d.insert("b".into(), 2);
        let mut keys: Vec<String> = d.keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["a", "b"]);
        let total: u64 = d.values().sum();
        assert_eq!(total, 3);
        let mut pairs: Vec<(String, u64)> = d.iter().map(|(k, v)| (k.clone(), *v)).collect();
        pairs.sort();
        assert_eq!(pairs, [("a".to_string(), 1), ("b".to_string(), 2)]);
    }

    #[test]
    fn spread_is_deterministic_and_nonzero_quality() {
        // Same bytes → same table hash, regardless of path.
        assert_eq!(dict_hash_bytes(b"abc"), spread32(fx_hash32(b"abc")));
        // Distinct short keys shouldn't collapse to one bucket pattern.
        let hashes: std::collections::HashSet<u64> =
            (0..1000u32).map(|i| dict_hash_bytes(i.to_string().as_bytes())).collect();
        assert!(hashes.len() > 990, "fx32+spread collides too much: {}", hashes.len());
    }
}
