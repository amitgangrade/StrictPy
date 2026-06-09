//! Runtime object model. See spec §8.
//!
//! Every heap value begins with a 16-byte [`ObjectHeader`]: an 8-byte vtable
//! pointer (to a [`RuntimeType`]) and 8 bytes of GC metadata. Fields follow
//! the header packed in declaration order.
//!
//! In M4 the allocator is intentionally simple: each object is its own
//! `Box<[u8]>` whose raw pointer is tracked by the [`crate::gc::Heap`]. The
//! GC is a stop-the-world mark-sweep that scans live frames conservatively.
//!
//! The on-disk `TypeInfo` ([`crate::loader::TypeInfo`]) is the *file* form;
//! [`RuntimeType`] here is the *runtime* form — translated once at load time
//! and pointed to from every object header.

use std::ffi::c_void;
use std::os::raw::c_char;
use std::sync::Arc;

/// 16-byte object header that begins every heap-allocated StrictPy value.
///
/// `gc_meta` packs (low → high) the mark bit, age, lock bits, and (in
/// future) a hash cache. For M4 only the mark bit (bit 0) is used.
#[repr(C)]
pub struct ObjectHeader {
    /// Pointer to the [`RuntimeType`] describing this object's class.
    /// Stored as a raw pointer because every object header has the same
    /// `#[repr(C)]` shape and the GC inspects this slot directly.
    pub vtable: *const RuntimeType,
    /// GC color/age/lock/hash-cache bits, packed.
    pub gc_meta: u64,
}

// Compile-time check: header must be exactly 16 bytes.
const _: [(); 16] = [(); std::mem::size_of::<ObjectHeader>()];

/// Field descriptor inside a [`TypeInfo`] (legacy `#[repr(C)]` shape kept for
/// future JIT use). M4 uses [`RuntimeType`] internally.
#[repr(C)]
pub struct FieldInfo {
    pub name: *const c_char,
    pub type_id: u32,
    pub offset: u32,
}

/// Itable entry mapping a protocol id to a slice of method function pointers
/// (spec §14.4 / §8.2).
#[repr(C)]
pub struct ItableEntry {
    pub protocol_type_id: u32,
    pub method_count: u16,
    pub _pad: u16,
    pub methods: *const *const c_void,
}

/// Legacy `#[repr(C)]` placeholder for the static per-class type descriptor.
/// Kept so future JIT-generated code has a stable address layout to compile
/// against. M4 stores its own [`RuntimeType`] separately and points object
/// headers at that.
#[repr(C)]
pub struct TypeInfo {
    pub type_id: u32,
    pub size: u32,
    pub field_count: u32,
    pub vtable_len: u32,
    pub fields: *const FieldInfo,
    pub gc_kind: u8,
    pub _pad: [u8; 7],
    pub qualified_name: *const c_char,
    pub base: *const TypeInfo,
    pub itable: *const ItableEntry,
    pub itable_count: u32,
    pub vtable: *const *const c_void,
}

/// Runtime per-class descriptor used by the M4 interpreter.
///
/// The interpreter doesn't need raw pointers (no JIT yet), so this is a
/// Rust-friendly view that owns its layout vectors. Each loaded module
/// produces one `Arc<RuntimeType>` per type-table entry; objects hold a
/// `*const RuntimeType` to one of those.
///
/// `field_is_ref` is used by the GC marker to identify reference fields
/// that should be traced. In M4 we conservatively mark every 8-byte slot of
/// every object except primitive arrays, so this is informational only —
/// see `gc.rs` for the actual scan policy.
#[derive(Debug)]
pub struct RuntimeType {
    pub type_id: u32,
    pub name: String,
    pub size: u32,
    pub field_offsets: Vec<u32>,
    pub field_is_ref: Vec<bool>,
    /// One slot per virtual-table entry; values are function ids into the
    /// module's function table. `u32::MAX` means "unresolved".
    pub vtable: Vec<u32>,
    /// Type-kind: 0 = primitive, 1 = user class, 2 = protocol, 3+ = misc.
    pub kind: u8,
    /// Tag for the GC scanner. Specialised type kinds (List, String,
    /// Closure) need bespoke scanning.
    pub gc_kind: GcKind,
}

/// GC scanning tag attached to each runtime type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcKind {
    /// No reference fields. Skip scanning.
    NoRefs,
    /// User class with reference fields described by `field_offsets` /
    /// `field_is_ref`. For M4 we scan all u64-aligned slots conservatively.
    Class,
    /// `List[T]`: scan the inline length / capacity / data triple, and walk
    /// the data buffer treating every u64 slot as a potential pointer.
    List,
    /// `str`: opaque byte buffer, no inner pointers to trace.
    Str,
    /// Closure: scan captures.
    Closure,
    /// `io.File` handle wrapper. No traceable refs; the OS handle lives
    /// outside the StrictPy heap.
    File,
    /// `Channel[T]` handle wrapper. No traceable refs in the GC sense; the
    /// payloads are inspected by the channel-handle table separately.
    Channel,
    /// `Thread` handle wrapper.
    Thread,
    /// `Dict[K, V]` handle wrapper. The keys/values live in the dict-handle
    /// table which the GC roots separately.
    Dict,
    /// M62b: a generator object (`Iterator[T]` produced by a generator
    /// function). Carries a saved call frame — the suspended function's
    /// register file lives inline in the allocation, right after the fixed
    /// `GeneratorRepr` fields. The GC must scan those saved registers
    /// conservatively because they can hold the only live reference to a heap
    /// object across a `yield`. See [`Heap::trace_object`].
    Generator,
}

/// Heap layout for `List[T]` (spec §8.4). The element buffer is separately
/// allocated and pointed to by `data`. Elements are u64-sized slots (M3
/// erases the element type).
#[repr(C)]
pub struct ListRepr {
    pub header: ObjectHeader,
    pub length: usize,
    pub capacity: usize,
    pub data: *mut u8,
}

/// Heap layout for `str` (spec §8.5).
#[repr(C)]
pub struct StringRepr {
    pub header: ObjectHeader,
    /// Length in Unicode code points (M4: equal to `byte_len` for ASCII).
    pub length: usize,
    /// Length of the UTF-8 byte buffer pointed to by `data`.
    pub byte_len: usize,
    /// Pointer to the UTF-8 byte buffer (allocated via the heap as a
    /// raw byte allocation, independent of the StringRepr itself).
    pub data: *mut u8,
    /// Bit 0 = interned, bit 1 = ascii-only, bit 2 = has-side-index.
    pub flags: u64,
    /// Allocated capacity (in bytes) of the `data` buffer. Normally equal to
    /// `byte_len` (strings are immutable), but the compiler-proven-unique
    /// accumulator path (`s = s + e` lowered to `StrAppendInPlace`) over-
    /// allocates with doubling so repeated appends are amortised O(1) instead
    /// of allocating + copying the whole string every step. Appended at the
    /// end of the struct so existing field offsets are unchanged.
    pub capacity: usize,
}

/// Heap layout for closures (spec §8.6). Captures follow inline.
#[repr(C)]
pub struct ClosureRepr {
    pub header: ObjectHeader,
    pub fn_id: u32,
    pub capture_n: u32,
    // Captures (`capture_n` u64s) follow immediately after.
}

/// Heap layout for an open `io.File`. The actual `std::fs::File` lives in a
/// VM-global handle table (see [`crate::interp::Interpreter::files`]); the
/// `handle` here is an index into that table. A `handle == 0` means the file
/// has been closed.
#[repr(C)]
pub struct FileRepr {
    pub header: ObjectHeader,
    pub handle: u64,
    /// bit 0 = readable, bit 1 = writable.
    pub mode_flags: u32,
    pub _padding: u32,
}

/// Heap layout for a `Channel[T]`. Like `FileRepr`, the underlying sender /
/// receiver pair lives in a VM-global handle table.
#[repr(C)]
pub struct ChannelRepr {
    pub header: ObjectHeader,
    pub handle: u64,
}

/// Heap layout for a `Thread`. The underlying `JoinHandle` and target
/// closure live in a VM-global handle table.
#[repr(C)]
pub struct ThreadRepr {
    pub header: ObjectHeader,
    pub handle: u64,
    /// 0 = not started, 1 = started.
    pub started: u32,
    /// 0 = still running / not started, 1 = joined.
    pub finished: u32,
}

/// Heap layout for `Dict[K, V]`. The backing `HashMap` lives in a VM-global
/// handle table (see [`crate::interp::Interpreter::dicts`]).
#[repr(C)]
pub struct DictRepr {
    pub header: ObjectHeader,
    pub handle: u64,
    /// `key_kind`: 0 = string keys, 1 = integer keys. M5 uses string keys
    /// almost exclusively; integer keys are reserved for M6.
    pub key_kind: u32,
    pub _padding: u32,
}

/// M62b: heap layout for a generator object (`Iterator[T]`).
///
/// A generator owns a *suspended call frame*. The fixed fields below are
/// followed inline by `nregs` saved register slots (u64 each). The interpreter
/// restores those slots into a fresh [`crate::interp::Frame`] on resume and
/// re-saves them on the next `yield`.
///
/// GC: this object's [`crate::object::GcKind`] is `Generator`; the collector
/// scans the inline saved-register area as a conservative pointer window so a
/// heap value held only in a suspended local stays alive across `yield`.
///
/// `state`: 0 = not-started (saved registers are the initial argument window,
/// `saved_pc` is the function entry pc), 1 = suspended at a `yield`, 2 = done
/// (exhausted — further `GenNext` immediately reports done).
#[repr(C)]
pub struct GeneratorRepr {
    pub header: ObjectHeader,
    /// Function-table id of the generator function body.
    pub fn_id: u32,
    /// 0 = not started, 1 = suspended, 2 = done.
    pub state: u32,
    /// Saved program counter (absolute byte offset into the module code blob).
    /// Meaningful when `state == 0` (entry pc) or `state == 1` (resume pc).
    pub saved_pc: u64,
    /// Number of saved register slots that follow inline.
    pub nregs: u64,
    // `nregs` u64 register slots follow immediately after this struct.
}

/// M35 P4-C: heap layout for a streaming `Hasher` instance.  The actual
/// in-progress hash state lives in `SharedVm.hashers` keyed by `handle`;
/// the heap object itself carries no Rust-side hasher (so GC tracing
/// stays trivial — same pattern as FileRepr / ChannelRepr / ThreadRepr).
#[repr(C)]
pub struct HasherRepr {
    pub header: ObjectHeader,
    /// Index into `SharedVm.hashers`.  0 means "freed/never set" but in
    /// practice every freshly allocated HasherRepr has a positive
    /// handle assigned by `Interpreter::alloc_hasher`.
    pub handle: i64,
}

/// Shared global pool of [`RuntimeType`]s built once at load time. The
/// interpreter holds one of these and hands raw pointers to objects.
///
/// Allocations live for the lifetime of the interpreter; types never die.
#[derive(Debug, Default)]
pub struct TypeRegistry {
    pub types: Vec<Arc<RuntimeType>>,
    /// Singleton type for `List[T]` (one entry, type-erased).
    pub list_type: Option<Arc<RuntimeType>>,
    /// Singleton type for `str`.
    pub str_type: Option<Arc<RuntimeType>>,
    /// Singleton type for `Closure`.
    pub closure_type: Option<Arc<RuntimeType>>,
}

impl TypeRegistry {
    pub fn new() -> Self {
        Self::default()
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Mark-bit helpers (live in the gc_meta header field)
// ─────────────────────────────────────────────────────────────────────────

pub const GC_MARK_BIT: u64 = 1 << 0;

/// # Safety
/// `obj` must point at a valid [`ObjectHeader`].
pub unsafe fn is_marked(obj: *const ObjectHeader) -> bool {
    (*obj).gc_meta & GC_MARK_BIT != 0
}

/// # Safety
/// `obj` must point at a valid mutable [`ObjectHeader`].
pub unsafe fn set_marked(obj: *mut ObjectHeader, val: bool) {
    if val {
        (*obj).gc_meta |= GC_MARK_BIT;
    } else {
        (*obj).gc_meta &= !GC_MARK_BIT;
    }
}
