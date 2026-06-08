//! Garbage collector. See spec §15.
//!
//! M4 ships the **simplest collector that is correct**: a stop-the-world
//! mark-sweep over a flat list of independently-allocated objects.
//! Generational + write barriers land in M6.
//!
//! ## Allocation
//!
//! Each [`alloc`](Heap::alloc) call asks the system allocator for `size`
//! bytes, installs an [`ObjectHeader`] at offset 0, and records the raw
//! pointer in `Heap.objects`. The pointer is returned to the caller.
//!
//! ## Marking
//!
//! [`Heap::collect`] takes the live root slice (every register of every
//! frame on the active stack) and treats each u64 in that slice as a
//! potential pointer. Any pointer that matches an entry in `Heap.objects`
//! and isn't already marked gets its mark bit set, then its scannable
//! fields are pushed onto a worklist. This is **conservative scanning** —
//! integers that happen to alias a live heap address will keep the object
//! alive but the result is still memory-safe.
//!
//! ## Sweep
//!
//! After marking, every object in `Heap.objects` whose mark bit is clear
//! is deallocated. Survivors get their mark bit cleared.

use std::alloc::{alloc as sys_alloc, dealloc, Layout};
use std::collections::HashSet;

use crate::object::{
    is_marked, set_marked, GcKind, ListRepr, ObjectHeader, RuntimeType, StringRepr,
};

/// Live-allocation bookkeeping entry.
#[derive(Debug)]
struct LiveObj {
    ptr: *mut u8,
    size: usize,
    align: usize,
    kind: GcKind,
}

/// The whole heap. Owns every live allocation.
pub struct Heap {
    objects: Vec<LiveObj>,
    /// Raw byte buffers (list element storage, string data). Not scanned
    /// directly — owned by their parent object and freed during sweep.
    raw_buffers: Vec<(*mut u8, Layout)>,
    /// Bytes allocated since the last collection.
    bytes_since_gc: usize,
    /// Threshold at which a GC is suggested.
    gc_threshold: usize,
    /// Total live bytes (approximate; recomputed each sweep).
    live_bytes: usize,
}

// SAFETY: `Heap` stores raw `*mut u8` pointers obtained from the system
// allocator. Those pointers are valid in any thread that observes them; the
// `Heap` itself is always accessed through `Arc<Mutex<Heap>>` in the VM
// (see `SharedVm`), so there is no concurrent access to the inner state.
// Moving a `Heap` between threads (or, more precisely, accessing it from a
// thread other than the one that created it) is therefore sound.
unsafe impl Send for Heap {}
// SAFETY: same reasoning — all access is mutex-serialised.
unsafe impl Sync for Heap {}

impl Heap {
    pub fn new() -> Self {
        Self {
            objects: Vec::new(),
            raw_buffers: Vec::new(),
            bytes_since_gc: 0,
            gc_threshold: 4 * 1024 * 1024,
            live_bytes: 0,
        }
    }

    /// Allocate `size` bytes, install `vtable` into the header, and return
    /// the raw pointer to the start of the allocation.
    pub fn alloc(
        &mut self,
        size: usize,
        vtable: *const RuntimeType,
        kind: GcKind,
    ) -> *mut u8 {
        let size = size.max(std::mem::size_of::<ObjectHeader>());
        let layout = Layout::from_size_align(size, 8).expect("bad alloc layout");
        // SAFETY: Layout is non-zero and aligned to 8 (power of two).
        let ptr = unsafe { sys_alloc(layout) };
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        // SAFETY: ptr points to a freshly-allocated `size`-byte region we
        // own. Zero-init then install header.
        unsafe {
            std::ptr::write_bytes(ptr, 0, size);
            let hdr = ptr as *mut ObjectHeader;
            (*hdr).vtable = vtable;
            (*hdr).gc_meta = 0;
        }
        self.objects.push(LiveObj {
            ptr,
            size,
            align: 8,
            kind,
        });
        self.bytes_since_gc += size;
        self.live_bytes += size;
        ptr
    }

    /// Allocate a raw byte buffer (no header). Returned pointer is owned
    /// by the heap and tracked in `raw_buffers`.
    pub fn alloc_raw(&mut self, size: usize, align: usize) -> *mut u8 {
        if size == 0 {
            return std::ptr::null_mut();
        }
        let layout = Layout::from_size_align(size, align).expect("bad raw layout");
        // SAFETY: layout valid.
        let ptr = unsafe { sys_alloc(layout) };
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        // SAFETY: just-allocated.
        unsafe { std::ptr::write_bytes(ptr, 0, size) };
        self.raw_buffers.push((ptr, layout));
        self.bytes_since_gc += size;
        ptr
    }

    /// Replace one raw buffer with a fresh one of the new size, copying
    /// `copy_bytes` from the old buffer into the new one. The old buffer
    /// is freed. Returns the new pointer.
    pub fn realloc_raw(
        &mut self,
        old: *mut u8,
        old_size: usize,
        new_size: usize,
        align: usize,
        copy_bytes: usize,
    ) -> *mut u8 {
        let new_ptr = self.alloc_raw(new_size, align);
        if !old.is_null() && copy_bytes > 0 && !new_ptr.is_null() {
            // SAFETY: caller asserts old/new are valid for the copy range.
            unsafe {
                std::ptr::copy_nonoverlapping(old, new_ptr, copy_bytes);
            }
        }
        self.free_raw(old, old_size, align);
        new_ptr
    }

    /// True if a collection is recommended.
    pub fn should_collect(&self) -> bool {
        self.bytes_since_gc >= self.gc_threshold
    }

    /// Run a full mark-sweep collection.
    pub fn collect(&mut self, roots: &[&[u64]]) {
        // 1. Build a set of live object pointers.
        let mut alive: HashSet<usize> = HashSet::with_capacity(self.objects.len());
        for o in &self.objects {
            alive.insert(o.ptr as usize);
        }

        // 2. Mark from roots, then trace.
        let mut worklist: Vec<*mut u8> = Vec::new();
        for slot_window in roots {
            for &slot in *slot_window {
                let p = slot as usize;
                if alive.contains(&p) {
                    let obj = p as *mut u8;
                    // SAFETY: alive set guarantees `obj` is a valid header.
                    unsafe {
                        let hdr = obj as *mut ObjectHeader;
                        if !is_marked(hdr) {
                            set_marked(hdr, true);
                            worklist.push(obj);
                        }
                    }
                }
            }
        }

        while let Some(obj) = worklist.pop() {
            // SAFETY: obj is alive and marked.
            unsafe { self.trace_object(obj, &alive, &mut worklist) };
        }

        // 3. Sweep — drop unmarked.
        let mut survived = Vec::with_capacity(self.objects.len());
        let mut freed_bytes = 0usize;
        // Drain into a temporary so we can call &mut self methods.
        let drained: Vec<LiveObj> = self.objects.drain(..).collect();
        for live in drained {
            // SAFETY: live.ptr is one we allocated.
            unsafe {
                let hdr = live.ptr as *mut ObjectHeader;
                if is_marked(hdr) {
                    set_marked(hdr, false);
                    survived.push(live);
                } else {
                    self.free_inner_buffers(&live);
                    let layout =
                        Layout::from_size_align(live.size, live.align).expect("layout");
                    dealloc(live.ptr, layout);
                    freed_bytes += live.size;
                }
            }
        }
        self.objects = survived;
        self.live_bytes = self.live_bytes.saturating_sub(freed_bytes);
        self.bytes_since_gc = 0;

        // Adaptive: schedule next GC after live_bytes doubles.
        let target = (self.live_bytes.saturating_mul(2)).max(1024 * 1024);
        self.gc_threshold = target;
    }

    /// Trace the reachable graph from one already-marked object.
    ///
    /// # Safety
    /// `obj` must be a pointer the heap returned from `alloc`.
    unsafe fn trace_object(
        &self,
        obj: *mut u8,
        alive: &HashSet<usize>,
        worklist: &mut Vec<*mut u8>,
    ) {
        let hdr = obj as *mut ObjectHeader;
        let ty = (*hdr).vtable;
        if ty.is_null() {
            return;
        }
        let kind = (*ty).gc_kind;
        match kind {
            GcKind::NoRefs
            | GcKind::Str
            | GcKind::File
            | GcKind::Channel
            | GcKind::Thread
            | GcKind::Dict => {
                // File/Channel/Thread/Dict store their state in side tables
                // owned by the interpreter; the heap object itself contains
                // no traceable refs. Side-table contents are kept alive by
                // a separate root scan in `Interpreter::maybe_collect`.
            }
            GcKind::Class | GcKind::Closure | GcKind::Generator => {
                // M62b: `Generator` is scanned exactly like `Class`/`Closure`
                // — every 8-byte slot past the header is treated as a
                // potential pointer. For a generator this conservatively
                // covers the inline saved-register window, keeping any heap
                // value held only in a suspended local alive across `yield`.
                // The fixed numeric fields (fn_id / state / saved_pc / nregs)
                // are scanned too, but a stray integer that aliases a live
                // heap address is merely kept alive — never unsafe.
                let total_size = self.size_of(obj);
                let header_size = std::mem::size_of::<ObjectHeader>();
                let mut off = header_size;
                while off + 8 <= total_size {
                    let slot_ptr = obj.add(off) as *const u64;
                    let val = std::ptr::read_unaligned(slot_ptr);
                    Self::maybe_push(val, alive, worklist);
                    off += 8;
                }
            }
            GcKind::List => {
                let lst = obj as *const ListRepr;
                let length = (*lst).length;
                let data = (*lst).data;
                if !data.is_null() {
                    for i in 0..length {
                        let slot_ptr = (data as *const u64).add(i);
                        let val = std::ptr::read_unaligned(slot_ptr);
                        Self::maybe_push(val, alive, worklist);
                    }
                }
            }
        }
    }

    unsafe fn maybe_push(val: u64, alive: &HashSet<usize>, worklist: &mut Vec<*mut u8>) {
        let p = val as usize;
        if alive.contains(&p) {
            let inner = p as *mut u8;
            let inner_hdr = inner as *mut ObjectHeader;
            if !is_marked(inner_hdr) {
                set_marked(inner_hdr, true);
                worklist.push(inner);
            }
        }
    }

    fn size_of(&self, ptr: *mut u8) -> usize {
        for o in &self.objects {
            if o.ptr == ptr {
                return o.size;
            }
        }
        std::mem::size_of::<ObjectHeader>()
    }

    /// Free any heap-owned buffers reachable only through this object.
    ///
    /// # Safety
    /// `obj.ptr` must still be valid (called during sweep, before the
    /// outer allocation is freed).
    unsafe fn free_inner_buffers(&mut self, obj: &LiveObj) {
        match obj.kind {
            GcKind::List => {
                let lst = obj.ptr as *mut ListRepr;
                let data = (*lst).data;
                let cap = (*lst).capacity;
                if !data.is_null() && cap > 0 {
                    self.free_raw(data, cap * 8, 8);
                }
            }
            GcKind::Str => {
                let s = obj.ptr as *mut StringRepr;
                let data = (*s).data;
                let byte_len = (*s).byte_len;
                if !data.is_null() && byte_len > 0 {
                    self.free_raw(data, byte_len, 1);
                }
            }
            _ => {}
        }
    }

    fn free_raw(&mut self, ptr: *mut u8, size: usize, align: usize) {
        if ptr.is_null() || size == 0 {
            return;
        }
        if let Some(pos) = self.raw_buffers.iter().position(|(p, _)| *p == ptr) {
            let (_, layout) = self.raw_buffers.swap_remove(pos);
            // SAFETY: we own this allocation.
            unsafe { dealloc(ptr, layout) };
        } else {
            // Defensive fallback: rebuild the layout the caller declared.
            let layout = Layout::from_size_align(size, align).expect("layout");
            // SAFETY: caller asserts size/align match the original.
            unsafe { dealloc(ptr, layout) };
        }
    }

    fn drop_raw_buffers(&mut self) {
        for (ptr, layout) in self.raw_buffers.drain(..) {
            if !ptr.is_null() {
                // SAFETY: every entry was alloc'd by us and never freed.
                unsafe { dealloc(ptr, layout) };
            }
        }
    }
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        // Free everything still alive at VM shutdown.
        let drained: Vec<LiveObj> = self.objects.drain(..).collect();
        for live in drained {
            // SAFETY: live.ptr is one we allocated.
            unsafe {
                let layout =
                    Layout::from_size_align(live.size, live.align).expect("layout");
                dealloc(live.ptr, layout);
            }
        }
        self.drop_raw_buffers();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ty() -> RuntimeType {
        RuntimeType {
            type_id: 0,
            name: "T".into(),
            size: 16,
            field_offsets: vec![],
            field_is_ref: vec![],
            vtable: vec![],
            kind: 0,
            gc_kind: GcKind::NoRefs,
        }
    }

    #[test]
    fn alloc_and_collect_unreachable_frees_memory() {
        let mut h = Heap::new();
        let ty = Box::new(make_ty());
        let ty_ptr: *const RuntimeType = &*ty;
        let _ = h.alloc(32, ty_ptr, GcKind::NoRefs);
        let _ = h.alloc(32, ty_ptr, GcKind::NoRefs);
        assert_eq!(h.objects.len(), 2);
        // No roots → everything is garbage.
        h.collect(&[]);
        assert_eq!(h.objects.len(), 0);
    }

    #[test]
    fn alloc_and_collect_keeps_reachable() {
        let mut h = Heap::new();
        let ty = Box::new(make_ty());
        let ty_ptr: *const RuntimeType = &*ty;
        let p = h.alloc(32, ty_ptr, GcKind::NoRefs);
        let roots: Vec<u64> = vec![p as u64];
        h.collect(&[&roots[..]]);
        assert_eq!(h.objects.len(), 1);
    }
}
