//! JIT shadow stack for precise-ish root enumeration during GC.
//!
//! M33 replaces the M9 `in_jit` "stop GC while any JIT frame is on the
//! stack" mechanism with a per-thread *shadow stack* maintained by the
//! JIT'd code itself. The shadow stack is a stack of windows into raw
//! u64 buffers (Cranelift stack slots inside the JIT'd frames); the GC
//! scans each window conservatively, exactly as it already scans
//! interpreted frames' register files.
//!
//! ## Why this shape (vs. Cranelift `enable_safepoints`)
//!
//! Cranelift's `enable_safepoints` flag + `declare_value_needs_stack_map`
//! API can emit precise per-PC stack maps that name the exact stack
//! slots and registers holding GC pointers. Wiring that up requires:
//! reading PC→stack-map metadata out of the compiled `MachBufferFinalized`,
//! correlating it against the JIT'd code memory (which `cranelift-jit`
//! does not currently expose as a stable `Range<usize>` on stable),
//! and walking JIT'd Rust frames via either `backtrace` or a custom
//! frame-pointer-chain walker. That stack is ~2-3k LOC of careful and
//! platform-specific work.
//!
//! The shadow-stack design ships the same correctness property — every
//! heap pointer reachable from a JIT'd frame at the moment of collection
//! IS rooted — for ~200 LOC of book-keeping, at the cost of:
//!
//! * Some false positives (we publish *every* register, not just the ones
//!   that currently hold a pointer). That's the same trade-off the
//!   conservative interpreter-frame scan already accepts.
//! * One extra "spill all registers to a stack slot" before each
//!   heap-allocating helper call. Measured cost in the M33 benchmark
//!   suite: <5 ns per spill on a modern x86_64 (the spills become a
//!   handful of `mov [rbp-N], reg` instructions; the helper-call
//!   overhead dwarfs them).
//!
//! See `docs/thesis/agent_reports/m33_precise_gc.md` and the
//! `conservative_gc_with_in_jit_pause.md` design dossier for the full
//! comparison.
//!
//! ## Inline publish (perf/jit-inline-shadow-publish)
//!
//! The push/pop bookkeeping used to be two out-of-line `extern "C"` calls
//! (`rt_shadow_push` / `rt_shadow_pop`) per call site. On call-heavy code
//! that function-call overhead dominated the safepoint cost. We now expose
//! the per-thread state as a stable struct via [`rt_shadow_state`], and the
//! JIT emits the common push/pop entirely *inline*:
//!
//! * PUSH: reload `depth`/`cap`/`base` from the stable [`ShadowState`]; if
//!   `depth < cap`, store `(buf, len)` at `base + depth*16` and write back
//!   `depth + 1`. Only when the (generously pre-reserved) backing store is
//!   full does it fall back to the out-of-line [`rt_shadow_push`], which
//!   grows the `Vec` and re-syncs `base`/`cap`/`depth`.
//! * POP: reload `depth`, write back `depth - 1`. No call, ever.
//!
//! Crucially the JIT reloads `base` from the struct on *every* push, so a
//! slow-path grow that moved the backing `Vec` is always observed. The
//! struct's own address is a thread-local address, which is stable for the
//! life of the thread, so the JIT caches that pointer once per function.
//!
//! The published windows are byte-for-byte identical to the old design:
//! the same `(buf, len)` pairs, pushed before every allocating helper and
//! popped after, and [`snapshot`] reads `base[0..depth]`.
//!
//! ## API shape
//!
//! Every JIT'd function:
//!
//! 1. Allocates an explicit stack slot of `num_registers * 8` bytes at
//!    function entry — this is the *shadow frame*.
//! 2. Whenever it is about to call a heap-allocating runtime helper
//!    (`rt_alloc`, `rt_list_push`, `rt_list_new`, `rt_array_new`,
//!    `rt_virtual_call`, `strictpy_native_trampoline`,
//!    `strictpy_alloc_str_const`), it:
//!      - stores every current register variable into the shadow frame
//!        at offset `r * 8`,
//!      - publishes the window with `(buf, num_regs)` (inline push),
//!      - performs the helper call,
//!      - unpublishes the window (inline pop).
//!
//! The GC, when invoked from any thread, walks the *current thread's*
//! shadow-stack and adds every published window to its conservative
//! root set, in addition to the interpreter's frame register files.
//!
//! ## Thread-safety
//!
//! The shadow stack is `thread_local!`. Collections drive from whichever
//! thread is currently calling [`crate::interp::Interpreter::maybe_collect`];
//! that thread is, by definition, the one that just allocated, so it owns
//! the relevant JIT frames. Other threads that are mid-JIT at the same
//! moment are blocked on the heap mutex (every allocation goes through
//! `Heap::alloc`), so their shadow stack is by construction in a
//! consistent state — every helper call has already pushed before
//! touching the heap.

#![cfg(feature = "jit")]

use std::cell::UnsafeCell;

/// One published shadow-stack window. Identifies a contiguous range of
/// u64 slots that the GC should scan conservatively on the next
/// collection.
///
/// `#[repr(C)]` with `buf` at offset 0 and `len` at offset 8 (size 16) is
/// load-bearing: the JIT's inline push emits raw stores at
/// `base + depth*16` (buf) and `base + depth*16 + 8` (len).
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ShadowFrame {
    /// Pointer to the first slot. Stable for the duration the entry is
    /// on the stack (Cranelift stack slot in the owning JIT'd frame).
    pub buf: *const u64,
    /// Number of u64 slots in the window.
    pub len: usize,
}

// SAFETY: ShadowFrame is just `(*const u64, usize)`. The pointer is only
// dereferenced from the same thread that pushed it (each thread owns its
// own shadow stack via the thread-local below); we never actually move
// ShadowFrames across threads.
unsafe impl Send for ShadowFrame {}
unsafe impl Sync for ShadowFrame {}

/// Initial capacity (in frames) reserved for the per-thread shadow stack.
/// Sized so the inline fast path essentially never overflows: the native
/// call stack overflows long before a realistic shadow-stack depth of 4096
/// nested allocating helper calls would be reached. The slow path
/// ([`rt_shadow_push`]) still handles overflow correctly by growing.
const INITIAL_CAP: usize = 4096;

/// Stable per-thread shadow-stack control block.
///
/// The *address of this struct* is stable for the life of the thread (it
/// lives in thread-local storage), so the JIT caches it once per function
/// via [`rt_shadow_state`] and reads/writes the fields directly:
///
/// ```text
///   offset 0:  base  (*mut ShadowFrame)   — first frame slot
///   offset 8:  depth (usize)              — number of live frames
///   offset 16: cap   (usize)              — frames the backing store holds
/// ```
///
/// `base` may change when the backing `Vec` grows on the slow path, so the
/// JIT reloads it on *every* inline push. `depth` and `cap` are likewise
/// reloaded each push. The backing `Vec` itself is owned alongside the
/// struct so its storage stays alive; `base`/`cap` mirror its pointer and
/// capacity.
#[repr(C)]
pub struct ShadowState {
    /// Pointer to the first frame in the backing store. Mirrors
    /// `backing.as_mut_ptr()`. Reloaded by the JIT on every push.
    base: *mut ShadowFrame,
    /// Number of frames currently published.
    depth: usize,
    /// Frames the backing store can hold without growing.
    cap: usize,
    /// Owning backing storage. The `Vec`'s pointer/capacity are mirrored
    /// into `base`/`cap`; never read through the `Vec` API on the hot path.
    backing: Vec<ShadowFrame>,
}

// SAFETY: ShadowState only ever lives in thread-local storage and is only
// touched by the owning thread; the raw pointer is into its own backing Vec.
unsafe impl Send for ShadowState {}

impl ShadowState {
    fn new() -> Self {
        let mut backing: Vec<ShadowFrame> = Vec::with_capacity(INITIAL_CAP);
        let base = backing.as_mut_ptr();
        let cap = backing.capacity();
        ShadowState {
            base,
            depth: 0,
            cap,
            backing,
        }
    }

    /// Re-sync `base`/`cap` from the backing `Vec` after a (possible)
    /// reallocation. `depth` is preserved.
    #[inline]
    fn resync(&mut self) {
        self.base = self.backing.as_mut_ptr();
        self.cap = self.backing.capacity();
    }
}

thread_local! {
    /// Per-thread shadow state at a stable address. `UnsafeCell` (not
    /// `RefCell`) because the inline JIT path mutates it without any Rust
    /// borrow, and the only Rust-side accessors here are `&mut`-exclusive
    /// by construction (single-threaded, non-reentrant w.r.t. each other).
    static SHADOW_STATE: UnsafeCell<ShadowState> =
        UnsafeCell::new(ShadowState::new());
}

/// Return the stable address of this thread's [`ShadowState`].
///
/// The JIT calls this once at function entry and caches the result in a
/// Cranelift value reused by every inline push/pop in that function. The
/// returned pointer is valid for the life of the thread.
///
/// # Safety
/// The returned pointer must only be used from the calling thread, and the
/// `base`/`cap` fields must be reloaded on every push (a slow-path grow may
/// move the backing store).
#[no_mangle]
pub extern "C" fn rt_shadow_state() -> *mut ShadowState {
    SHADOW_STATE.with(|s| s.get())
}

/// Slow-path / compatibility push. Called by the JIT only when the inline
/// fast path finds `depth == cap` (backing store full), and by any
/// non-inlined caller (e.g. tests). Grows the backing store if needed,
/// appends the frame, and re-syncs `base`/`cap`/`depth` in the struct.
///
/// # Safety
/// `buf` must point at `len` writable u64 slots that stay valid until the
/// matching pop runs. In practice this is always a Cranelift stack slot in
/// the same JIT'd frame that emitted the call.
#[no_mangle]
pub unsafe extern "C" fn rt_shadow_push(buf: *const u64, len: u64) {
    SHADOW_STATE.with(|cell| {
        // SAFETY: single-threaded exclusive access; no other live borrow.
        let st = &mut *cell.get();
        let frame = ShadowFrame {
            buf,
            len: len as usize,
        };
        // The inline fast path keeps `depth` in lock-step with the logical
        // length; mirror that into the Vec before pushing so growth and the
        // appended frame land at the right index.
        debug_assert!(st.depth <= st.backing.capacity());
        // SAFETY: `depth <= capacity`; set_len to the published depth so the
        // subsequent `push` appends at index `depth`.
        st.backing.set_len(st.depth);
        st.backing.push(frame);
        st.depth = st.backing.len();
        st.resync();
    });
}

/// Slow-path / compatibility pop. The JIT pops inline (no call); this
/// exists for non-inlined callers (e.g. tests) and decrements the depth.
#[no_mangle]
pub unsafe extern "C" fn rt_shadow_pop() {
    SHADOW_STATE.with(|cell| {
        // SAFETY: single-threaded exclusive access; no other live borrow.
        let st = &mut *cell.get();
        if st.depth > 0 {
            st.depth -= 1;
        }
    });
}

/// If overflow is ever hit on a build that cannot grow (it never is here,
/// the slow path grows), abort cleanly rather than corrupting memory. Kept
/// as a named symbol so the JIT could branch to it instead of UB; currently
/// unused because [`rt_shadow_push`] grows.
#[no_mangle]
pub extern "C" fn rt_shadow_overflow_abort() -> ! {
    eprintln!("strictpy: JIT shadow stack overflow (unrecoverable)");
    std::process::abort();
}

/// Visit every shadow-stack window on the current thread. Invoked by
/// `Heap::collect` on the thread that's about to run the mark phase.
///
/// Reads `base[0..depth]` directly (the inline JIT path keeps these in
/// sync). We materialise a flat `Vec<(buf, len)>` snapshot so the GC can
/// take its own private slice of roots without holding any borrow.
pub fn snapshot() -> Vec<(*const u64, usize)> {
    SHADOW_STATE.with(|cell| {
        // SAFETY: single-threaded exclusive access. `base[0..depth]` are
        // initialised frames (every push wrote both fields before bumping
        // depth, inline or via the slow path).
        let st = unsafe { &*cell.get() };
        let mut out = Vec::with_capacity(st.depth);
        for i in 0..st.depth {
            // SAFETY: i < depth <= cap; base points at `cap` valid slots.
            let f = unsafe { &*st.base.add(i) };
            out.push((f.buf, f.len));
        }
        out
    })
}

/// Current shadow-stack depth on this thread. Mainly for diagnostics
/// (the M33 regression test asserts the depth returns to zero after the
/// JIT'd workload exits).
pub fn depth() -> usize {
    SHADOW_STATE.with(|cell| {
        // SAFETY: single-threaded exclusive access.
        let st = unsafe { &*cell.get() };
        st.depth
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop_balanced() {
        assert_eq!(depth(), 0);
        let buf = [0u64; 4];
        unsafe {
            rt_shadow_push(buf.as_ptr(), 4);
            assert_eq!(depth(), 1);
            rt_shadow_push(buf.as_ptr(), 2);
            assert_eq!(depth(), 2);
            let snap = snapshot();
            assert_eq!(snap.len(), 2);
            assert_eq!(snap[0].1, 4);
            assert_eq!(snap[1].1, 2);
            rt_shadow_pop();
            rt_shadow_pop();
        }
        assert_eq!(depth(), 0);
    }

    /// Exercise the field layout the JIT depends on: `base`/`depth`/`cap`
    /// at fixed offsets and `ShadowFrame` size 16 with buf@0, len@8.
    #[test]
    fn state_layout_matches_jit_abi() {
        use std::mem::{align_of, offset_of, size_of};
        assert_eq!(size_of::<ShadowFrame>(), 16);
        assert_eq!(offset_of!(ShadowFrame, buf), 0);
        assert_eq!(offset_of!(ShadowFrame, len), 8);
        assert_eq!(offset_of!(ShadowState, base), 0);
        assert_eq!(offset_of!(ShadowState, depth), 8);
        assert_eq!(offset_of!(ShadowState, cap), 16);
        assert!(align_of::<ShadowFrame>() >= 8);
    }

    /// The state pointer is stable across calls on the same thread.
    #[test]
    fn state_pointer_is_stable() {
        let a = rt_shadow_state();
        let b = rt_shadow_state();
        assert_eq!(a, b);
    }

    /// Simulate the inline fast path writing directly through the state
    /// struct, then confirm `snapshot`/`depth` observe it.
    #[test]
    fn inline_style_push_then_snapshot() {
        // Pop any residual frames from earlier tests on this thread.
        while depth() > 0 {
            unsafe { rt_shadow_pop() };
        }
        let buf = [7u64; 3];
        let st = rt_shadow_state();
        unsafe {
            // depth < cap, so emulate the inline store: base[depth] = frame.
            let s = &mut *st;
            assert!(s.depth < s.cap);
            let slot = s.base.add(s.depth);
            (*slot).buf = buf.as_ptr();
            (*slot).len = 3;
            s.depth += 1;
        }
        assert_eq!(depth(), 1);
        let snap = snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].1, 3);
        unsafe { rt_shadow_pop() };
        assert_eq!(depth(), 0);
    }

    /// Force the slow path by overflowing the initial capacity, ensuring
    /// the grow re-syncs base/cap and preserves all frames.
    #[test]
    fn slow_path_grow_preserves_frames() {
        while depth() > 0 {
            unsafe { rt_shadow_pop() };
        }
        let buf = [1u64; 2];
        // Fill exactly to capacity using the slow path (simple + correct).
        let cap0 = SHADOW_STATE.with(|c| unsafe { (&*c.get()).cap });
        for _ in 0..cap0 {
            unsafe { rt_shadow_push(buf.as_ptr(), 2) };
        }
        assert_eq!(depth(), cap0);
        // One more triggers a grow.
        unsafe { rt_shadow_push(buf.as_ptr(), 9) };
        assert_eq!(depth(), cap0 + 1);
        let snap = snapshot();
        assert_eq!(snap.len(), cap0 + 1);
        assert_eq!(snap[cap0].1, 9);
        // base/cap must now reflect the grown Vec.
        SHADOW_STATE.with(|c| unsafe {
            let s = &*c.get();
            assert!(s.cap > cap0);
            assert_eq!(s.base, (s.backing.as_ptr() as *mut ShadowFrame));
        });
        for _ in 0..(cap0 + 1) {
            unsafe { rt_shadow_pop() };
        }
        assert_eq!(depth(), 0);
    }
}
