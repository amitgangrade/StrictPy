//! Runtime helpers callable from JIT'd code.
//!
//! M9 extends the JIT to cover heap-mutating list ops (`ArraySet`,
//! `ListPush`, `ArrayNew` / `ListNew`) plus class allocation (`New`). The
//! pure inline forms (read-only loads, simple stores) live in `jit.rs`;
//! anything that needs to touch the GC-managed heap goes through one of
//! the `rt_*` helpers in this module.
//!
//! ## ABI
//!
//! Every helper uses Rust `extern "C"`, which Cranelift's
//! [`crate::jit::host_call_conv`] matches via `WindowsFastcall` on Windows
//! and `SystemV` elsewhere. The first parameter is always the opaque
//! [`crate::jit::VmCtx`] pointer (downcast to `*mut Interpreter`).
//!
//! ## GC safety (temporary)
//!
//! JIT'd code holds heap pointers in CPU registers that the conservative
//! GC's frame-register scan can't see. To stay safe in M9 we suppress GC
//! entirely while a JIT'd frame is on the call stack — see
//! `SharedVm.in_jit` plus `Interpreter::maybe_collect`. The arena keeps
//! growing for the duration of the program, which is fine for the
//! benchmark workloads (well under 16 MB live). M10 will replace this
//! with precise stack maps.

#![cfg(feature = "jit")]

use crate::interp::Interpreter;
use crate::jit::VmCtx;
use crate::object::{GcKind, ListRepr, RuntimeType};
use std::sync::Arc;

/// Push `value` onto the back of `list`, growing the backing buffer if
/// needed.
///
/// Mirrors `Interpreter::list_push` (interp.rs). The grow path goes
/// through the shared heap's `realloc_raw`, which is mutex-guarded.
///
/// # Safety
/// * `vm` is a valid `*mut Interpreter` for the duration of the call.
/// * `list` is a valid `*mut ListRepr` (allocated through the same
///   heap; live for the duration of the call).
/// * `value` is one raw 8-byte slot — interpretation is up to the
///   caller (matches the type-erased list representation in M3+).
#[no_mangle]
pub unsafe extern "C" fn rt_list_push(
    vm: *mut VmCtx,
    list: *mut ListRepr,
    value: u64,
) {
    // SAFETY: caller contract above.
    let interp = unsafe { &mut *(vm as *mut Interpreter) };
    // GC safepoint: `list` and `value` are live in the caller's published
    // shadow window, so a collection here is safe. Collect before growing
    // the backing buffer so a long append loop stays heap-bounded.
    interp.jit_safepoint();
    if list.is_null() {
        // Match the interpreter's behaviour: a null-list push is an
        // error, but JIT'd code can't propagate exceptions yet, so
        // we just no-op to stay safe.
        return;
    }
    // SAFETY: caller asserts `list` is a valid ListRepr.
    unsafe { interp.list_push(list, value) };
}

/// Allocate a fresh `ListRepr` plus a `capacity`-element backing buffer.
/// `elem_size` is currently unused (every element is one u64 slot per
/// M3's type-erased representation) but is kept in the signature so a
/// future precise-typed list path can specialise without changing the
/// JIT-side call site.
///
/// # Safety
/// * `vm` is a valid `*mut Interpreter` for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn rt_list_new(
    vm: *mut VmCtx,
    _elem_size: u32,
    capacity: u32,
) -> *mut ListRepr {
    // SAFETY: caller contract above.
    let interp = unsafe { &mut *(vm as *mut Interpreter) };
    // GC safepoint before allocating the fresh list (caller's roots are
    // published in the shadow window).
    interp.jit_safepoint();
    interp.alloc_list(capacity as usize)
}

/// Allocate a fresh `ListRepr` of the given length, with `length =
/// capacity` (every slot zero-initialised). Used by `ArrayNew` which
/// promises an indexable buffer up to `length`.
///
/// # Safety
/// * `vm` is a valid `*mut Interpreter` for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn rt_array_new(
    vm: *mut VmCtx,
    _elem_size: u32,
    length: u64,
) -> *mut ListRepr {
    // SAFETY: caller contract above.
    let interp = unsafe { &mut *(vm as *mut Interpreter) };
    // GC safepoint before allocating (caller's roots are in the shadow
    // window).
    interp.jit_safepoint();
    let len = length as usize;
    let p = interp.alloc_list(len);
    // SAFETY: `p` was just allocated as a valid ListRepr.
    unsafe {
        (*p).length = len;
    }
    p
}

/// Allocate a user-class instance whose runtime layout matches the type
/// referenced by `operand`. Mirrors the interpreter's `op_new` lookup
/// quirk: the operand may be either a real `type_id` or, for the M3
/// codegen path, a `class_id` index into the module's type table.
///
/// Returns a null pointer if the type couldn't be resolved — JIT'd code
/// has no error channel today, and a null deref will trap deterministically
/// on the very next field access, which is what we want.
///
/// # Safety
/// * `vm` is a valid `*mut Interpreter` for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn rt_alloc(vm: *mut VmCtx, type_or_class_id: u32) -> *mut u8 {
    // SAFETY: caller contract above.
    let interp = unsafe { &mut *(vm as *mut Interpreter) };
    // GC safepoint: `rt_alloc` is the prototypical "may collect" call — the
    // tight recursive/loop allocators (M26 btree(10k) shape) hammer it. The
    // caller's register window is already published in the shadow stack, so
    // collecting here keeps a fully-JIT'd allocation loop heap-bounded.
    interp.jit_safepoint();
    let ty: Arc<RuntimeType> = match interp.shared.types.types.get(&type_or_class_id) {
        Some(t) => t.clone(),
        None => {
            // Fall back to interpreting the operand as a class_id index.
            let idx = type_or_class_id as usize;
            match interp.shared.module.types.get(idx) {
                Some(entry) => match interp.shared.types.types.get(&entry.type_id) {
                    Some(t) => t.clone(),
                    None => return std::ptr::null_mut(),
                },
                None => return std::ptr::null_mut(),
            }
        }
    };
    let size = (ty.size as usize).max(std::mem::size_of::<crate::object::ObjectHeader>());
    let ty_ptr: *const RuntimeType = Arc::as_ptr(&ty);
    interp
        .shared
        .heap
        .lock()
        .unwrap()
        .alloc(size, ty_ptr, GcKind::Class)
}

/// Resolve a virtual-method call to a concrete `fn_id` and invoke it via
/// the interpreter's normal call path (which itself takes the JIT fast
/// path when the target function is JIT'd).
///
/// We can't `call_indirect` directly from JIT'd code because the runtime
/// vtable stores `fn_id`s, not raw function pointers — resolution has to
/// happen on the Rust side. The receiver is `args[0]`.
///
/// # Safety
/// * `vm` is a valid `*mut Interpreter` for the duration of the call.
/// * `args` points to at least `n_args` raw 8-byte slots.
/// * `args[0]` is the receiver pointer (may be null; we trap that here).
#[no_mangle]
pub unsafe extern "C" fn rt_virtual_call(
    vm: *mut VmCtx,
    vtable_slot: u32,
    args: *const u64,
    n_args: u32,
) -> u64 {
    // SAFETY: caller contract above.
    let interp = unsafe { &mut *(vm as *mut Interpreter) };
    // GC safepoint before dispatching into the (possibly allocation-heavy)
    // callee. Args, including the receiver, are published in the caller's
    // shadow window.
    interp.jit_safepoint();
    let slice: &[u64] = if args.is_null() || n_args == 0 {
        &[]
    } else {
        // SAFETY: caller asserts the slot count.
        unsafe { std::slice::from_raw_parts(args, n_args as usize) }
    };
    let recv = slice.first().copied().unwrap_or(0);
    if recv == 0 {
        // Match the interpreter's "virtual call on null receiver" trap by
        // returning 0. JIT'd code can't propagate the exception; this at
        // least keeps the process from segfaulting on the vtable read.
        return 0;
    }
    // SAFETY: the receiver is a heap pointer with the standard
    // `ObjectHeader` layout (vtable pointer in the first 8 bytes).
    let ty_ptr = unsafe { (*(recv as *const crate::object::ObjectHeader)).vtable };
    if ty_ptr.is_null() {
        return 0;
    }
    // SAFETY: the runtime type is interned in `TypeRegistry.types` and
    // lives for the program's lifetime.
    let ty = unsafe { &*ty_ptr };
    let fn_id = match ty.vtable.get(vtable_slot as usize).copied() {
        Some(id) if id != u32::MAX => id,
        _ => return 0,
    };
    // Dispatch through the interpreter's invoke. That path checks for a
    // JIT'd entry and short-circuits to native code when present.
    interp.invoke(fn_id, slice).unwrap_or(0)
}
