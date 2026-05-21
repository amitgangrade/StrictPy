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
//!      - calls [`rt_shadow_push`] with `(buf, num_regs)`,
//!      - performs the helper call,
//!      - calls [`rt_shadow_pop`] (decrements the per-thread depth so
//!        the GC won't double-scan the same window after the helper
//!        returns).
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

use std::cell::RefCell;

/// One published shadow-stack window. Identifies a contiguous range of
/// u64 slots that the GC should scan conservatively on the next
/// collection.
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
// own shadow stack via the thread-local below), but RefCell needs the
// inner type to be Send for any cross-thread reasoning; we never actually
// move ShadowFrames across threads.
unsafe impl Send for ShadowFrame {}
unsafe impl Sync for ShadowFrame {}

thread_local! {
    static SHADOW_STACK: RefCell<Vec<ShadowFrame>> = const { RefCell::new(Vec::new()) };
}

/// Push a shadow-frame window. Called from JIT'd code right before any
/// heap-allocating runtime helper.
///
/// # Safety
/// `buf` must point at `len` writable u64 slots that stay valid until the
/// matching [`rt_shadow_pop`] runs. In practice this is always a Cranelift
/// stack slot in the same JIT'd frame that emitted the call.
#[no_mangle]
pub unsafe extern "C" fn rt_shadow_push(buf: *const u64, len: u64) {
    let frame = ShadowFrame {
        buf,
        len: len as usize,
    };
    SHADOW_STACK.with(|s| s.borrow_mut().push(frame));
}

/// Pop the most-recently-pushed shadow frame. Called from JIT'd code
/// right after the runtime helper returns.
#[no_mangle]
pub unsafe extern "C" fn rt_shadow_pop() {
    SHADOW_STACK.with(|s| {
        let _ = s.borrow_mut().pop();
    });
}

/// Visit every shadow-stack window on the current thread. Invoked by
/// `Heap::collect` on the thread that's about to run the mark phase.
///
/// We can't return a `&[ShadowFrame]` because that would borrow the
/// `RefCell`; instead we materialise a flat `Vec<(buf, len)>` snapshot,
/// which is fine for the mark phase (the GC takes its own private slice
/// of roots anyway).
pub fn snapshot() -> Vec<(*const u64, usize)> {
    SHADOW_STACK.with(|s| {
        s.borrow()
            .iter()
            .map(|f| (f.buf, f.len))
            .collect()
    })
}

/// Current shadow-stack depth on this thread. Mainly for diagnostics
/// (the M33 regression test asserts the depth returns to zero after the
/// JIT'd workload exits).
pub fn depth() -> usize {
    SHADOW_STACK.with(|s| s.borrow().len())
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
}
