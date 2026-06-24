//! Cranelift-based AOT-at-load native compiler for the supported subset of
//! StrictPy bytecode.
//!
//! Architecture
//! ============
//!
//! At module load time, every function in the loaded `.spyc` is run through
//! [`crate::decompile::decode_function`]; whichever functions use only the
//! op set [`crate::decompile::Op`] supports get a native code body via
//! [`Jit::compile_module`]. The rest stay in `fn_ptrs` as `None` and the
//! interpreter handles them via its normal dispatch.
//!
//! All JIT'd functions share a single ABI:
//!
//! ```text
//! unsafe extern "C" fn(vm: *mut VmCtx, args: *const u64) -> u64
//! ```
//!
//! - `vm`  — opaque pointer; threaded into the native-call trampoline so
//!           prints, allocations, etc. can re-enter the interpreter.
//! - `args` — pointer to the caller-prepared arg slot array. The callee
//!           loads `args[i]` once into each parameter register at entry.
//! - return — `i64` directly, `f64` via `f64::to_bits()`, pointers as `u64`.
//!
//! Register modelling
//! ------------------
//!
//! StrictPy registers are untyped 64-bit slots. Cranelift Variables are
//! strongly typed, so we model every register as an `I64` and bitcast when
//! a register's bytes need to be interpreted as `f32`/`f64`. The bytecode
//! itself already disambiguates via opcode width tags (e.g. `IAddI64` vs
//! `FAddF64`), so this is unambiguous.

#![cfg(feature = "jit")]

use std::collections::HashMap;
use std::sync::Mutex;

use cranelift::codegen::ir::{
    condcodes::{FloatCC, IntCC},
    types, AbiParam, FuncRef, Function, InstBuilder, MemFlags, Signature, StackSlotData,
    StackSlotKind, UserFuncName,
};
use cranelift::codegen::isa::CallConv;

/// Pick the calling convention that matches Rust's `extern "C"` on the
/// host. Cranelift's default `SystemV` is wrong on Windows (`extern "C"`
/// there is `WindowsFastcall`), so we select explicitly. This keeps both
/// JIT-to-JIT direct calls and JIT-to-Rust-trampoline calls in sync.
fn host_call_conv() -> CallConv {
    if cfg!(target_os = "windows") && cfg!(target_arch = "x86_64") {
        CallConv::WindowsFastcall
    } else {
        CallConv::SystemV
    }
}
use cranelift::codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module as _};

use crate::decompile::{
    self, DecodedFunction, FloatBinOp, FloatCmp, FloatWidth, IntBinOp, IntCmp, NumWidth, Op,
};
use crate::loader::{Constant, Module};

/// Trampoline signature: the JIT'd code calls this to re-enter the
/// interpreter's native-call dispatcher for `println`, `str(x)`, etc.
pub type NativeTrampoline = unsafe extern "C" fn(
    vm: *mut VmCtx,
    native_id: u32,
    args: *const u64,
    n_args: u32,
) -> u64;

/// Opaque context handed to JIT'd code; the trampoline downcasts it back to
/// `&mut Interpreter`. We use a separate marker type to keep the public API
/// honest about the pointer being raw.
pub enum VmCtx {}

/// Function pointer signature exposed to the interpreter's dispatcher.
pub type JitFn = unsafe extern "C" fn(*mut VmCtx, *const u64) -> u64;

/// Cranelift JIT wrapper. Owns the JIT module (and the executable memory it
/// allocates) plus a slot-table mapping `fn_id` → optional native code
/// pointer. Lives inside a `Mutex` on the interpreter's shared state so
/// worker threads can read it concurrently.
pub struct Jit {
    /// The Cranelift JIT module. Held in an `Option` so `Drop` can release
    /// it explicitly (the live function pointers are tied to its lifetime,
    /// which is fine because `Jit` outlives every running interpreter).
    module: Option<JITModule>,
    /// `fn_id` → entry pointer (None if the function falls back to interp).
    fn_ptrs: Vec<Option<JitFn>>,
    /// `fn_id` → declared Cranelift FuncId (for cross-function calls).
    decl: Vec<Option<FuncId>>,
    /// Declared trampoline for `CallNative`.
    native_trampoline_id: Option<FuncId>,
    /// Declared helper for `ConstStr` (allocates a string from the
    /// constant pool via the interpreter's heap).
    alloc_str_id: Option<FuncId>,
    /// M9 runtime helpers for heap-mutating list ops + class allocation.
    rt_list_push_id: Option<FuncId>,
    rt_list_new_id: Option<FuncId>,
    rt_array_new_id: Option<FuncId>,
    rt_alloc_id: Option<FuncId>,
    rt_virtual_call_id: Option<FuncId>,
    /// M14 closure helpers (allocate a `ClosureRepr` / dispatch a closure).
    rt_closure_new_id: Option<FuncId>,
    rt_closure_call_id: Option<FuncId>,
    /// M33 shadow-stack helpers. Called immediately around every
    /// heap-allocating runtime helper so the GC can see the JIT'd frame's
    /// register-resident pointers without walking machine stacks.
    m33_safepoint_push_id: Option<FuncId>,
    m33_safepoint_pop_id: Option<FuncId>,
    /// Returns the stable per-thread shadow-state address; cached once per
    /// JIT'd function and used by the inline push/pop sequences.
    m33_shadow_state_id: Option<FuncId>,
}

impl Jit {
    /// Build a JIT module configured for the host with default settings.
    pub fn new(trampoline: NativeTrampoline) -> Self {
        let mut flag_builder = settings::builder();
        flag_builder.set("use_colocated_libcalls", "false").unwrap();
        let _ = flag_builder.set("is_pic", "false");
        // `JITBuilder::new` picks up the host triple and a sensible default
        // ISA configuration; we just need to register the trampoline symbol
        // afterwards. (We previously tried `cranelift_native::builder()` but
        // that crate isn't a transitive dependency of cranelift-jit 0.115.)
        let _flags = settings::Flags::new(flag_builder);
        let mut builder =
            JITBuilder::new(cranelift_module::default_libcall_names())
                .expect("host architecture is supported by Cranelift");
        // Register the trampoline + small helpers as symbols the JIT'd
        // code can call.
        builder.symbol("strictpy_native_trampoline", trampoline as *const u8);
        builder.symbol(
            "strictpy_alloc_str_const",
            strictpy_alloc_str_const as *const u8,
        );
        // M9: runtime helpers for the heap-mutating ops + class allocation.
        // We register them under stable names so `declare_function` can
        // resolve them as `Linkage::Import` at codegen time.
        builder.symbol(
            "rt_list_push",
            crate::jit_runtime::rt_list_push as *const u8,
        );
        builder.symbol(
            "rt_list_new",
            crate::jit_runtime::rt_list_new as *const u8,
        );
        builder.symbol(
            "rt_array_new",
            crate::jit_runtime::rt_array_new as *const u8,
        );
        builder.symbol("rt_alloc", crate::jit_runtime::rt_alloc as *const u8);
        builder.symbol(
            "rt_virtual_call",
            crate::jit_runtime::rt_virtual_call as *const u8,
        );
        // M14: closure construction + dispatch helpers.
        builder.symbol(
            "rt_closure_new",
            crate::jit_runtime::rt_closure_new as *const u8,
        );
        builder.symbol(
            "rt_closure_call",
            crate::jit_runtime::rt_closure_call as *const u8,
        );
        // M33: per-thread shadow-stack publishers. Called around every
        // heap-allocating helper so the GC has a precise-enough root set
        // even while the JIT'd code is mid-execution.
        builder.symbol(
            "rt_shadow_push",
            crate::stackmap_registry::rt_shadow_push as *const u8,
        );
        builder.symbol(
            "rt_shadow_pop",
            crate::stackmap_registry::rt_shadow_pop as *const u8,
        );
        // Inline-publish: the JIT reads/writes the stable per-thread state
        // struct directly; `rt_shadow_state` hands it the address.
        builder.symbol(
            "rt_shadow_state",
            crate::stackmap_registry::rt_shadow_state as *const u8,
        );
        let module = JITModule::new(builder);
        Self {
            module: Some(module),
            fn_ptrs: Vec::new(),
            decl: Vec::new(),
            native_trampoline_id: None,
            alloc_str_id: None,
            rt_list_push_id: None,
            rt_list_new_id: None,
            rt_array_new_id: None,
            rt_alloc_id: None,
            rt_virtual_call_id: None,
            rt_closure_new_id: None,
            rt_closure_call_id: None,
            m33_safepoint_push_id: None,
            m33_safepoint_pop_id: None,
            m33_shadow_state_id: None,
        }
    }

    /// Compile every function in `module` that uses only the supported op
    /// set. Returns `(compiled, total)` and fills `self.fn_ptrs`.
    pub fn compile_module(&mut self, module: &Module) -> (usize, usize) {
        let max_fn_id = module
            .functions
            .iter()
            .map(|f| f.fn_id)
            .max()
            .map(|m| m as usize + 1)
            .unwrap_or(0);
        self.fn_ptrs.resize_with(max_fn_id, || None);
        self.decl.resize_with(max_fn_id, || None);

        // Pre-decode every function; this tells us which are JIT-eligible.
        let mut decoded: Vec<Option<DecodedFunction>> =
            Vec::with_capacity(module.functions.len());
        for f in &module.functions {
            decoded.push(decompile::decode_function(module, f.code_offset, f.code_length).ok());
        }

        // Mark a function ineligible if any `CallDirect` inside it points to
        // a function that itself didn't decode (so the JIT'd code couldn't
        // resolve the callee). This is conservative — a tighter analysis
        // would let mixed modules still JIT their leaf functions. Iterate
        // until fixpoint in case of mutual recursion through interp-only
        // functions.
        loop {
            let mut changed = false;
            for i in 0..module.functions.len() {
                let Some(df) = decoded[i].as_ref() else { continue };
                let bad = df.ops.iter().any(|op| match &op.kind {
                    Op::CallDirect { fn_id, .. } => {
                        // Find the called function's index.
                        let target_idx = module
                            .functions
                            .iter()
                            .position(|x| x.fn_id == *fn_id);
                        match target_idx {
                            Some(j) => decoded[j].is_none(),
                            None => true,
                        }
                    }
                    _ => false,
                });
                if bad {
                    decoded[i] = None;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        let jit_sig = make_jit_sig();
        let trampoline_sig = make_trampoline_sig();
        let alloc_str_sig = make_alloc_str_sig();
        let rt_list_push_sig = make_rt_list_push_sig();
        let rt_list_new_sig = make_rt_list_new_sig();
        let rt_array_new_sig = make_rt_array_new_sig();
        let rt_alloc_sig = make_rt_alloc_sig();
        let rt_virtual_call_sig = make_rt_virtual_call_sig();
        let rt_closure_new_sig = make_rt_closure_new_sig();
        let rt_closure_call_sig = make_rt_closure_call_sig();
        let m33_safepoint_push_sig = make_m33_safepoint_push_sig();
        let m33_safepoint_pop_sig = make_m33_safepoint_pop_sig();
        let m33_shadow_state_sig = make_m33_shadow_state_sig();

        // Declare the native trampoline.
        let m = self.module.as_mut().expect("jit module live");
        let tid = match m.declare_function(
            "strictpy_native_trampoline",
            Linkage::Import,
            &trampoline_sig,
        ) {
            Ok(id) => id,
            Err(_) => return (0, module.functions.len()),
        };
        self.native_trampoline_id = Some(tid);

        let asid = match m.declare_function(
            "strictpy_alloc_str_const",
            Linkage::Import,
            &alloc_str_sig,
        ) {
            Ok(id) => id,
            Err(_) => return (0, module.functions.len()),
        };
        self.alloc_str_id = Some(asid);

        let rt_lp_id = match m.declare_function(
            "rt_list_push",
            Linkage::Import,
            &rt_list_push_sig,
        ) {
            Ok(id) => id,
            Err(_) => return (0, module.functions.len()),
        };
        self.rt_list_push_id = Some(rt_lp_id);

        let rt_ln_id = match m.declare_function(
            "rt_list_new",
            Linkage::Import,
            &rt_list_new_sig,
        ) {
            Ok(id) => id,
            Err(_) => return (0, module.functions.len()),
        };
        self.rt_list_new_id = Some(rt_ln_id);

        let rt_an_id = match m.declare_function(
            "rt_array_new",
            Linkage::Import,
            &rt_array_new_sig,
        ) {
            Ok(id) => id,
            Err(_) => return (0, module.functions.len()),
        };
        self.rt_array_new_id = Some(rt_an_id);

        let rt_alloc_id = match m.declare_function("rt_alloc", Linkage::Import, &rt_alloc_sig) {
            Ok(id) => id,
            Err(_) => return (0, module.functions.len()),
        };
        self.rt_alloc_id = Some(rt_alloc_id);

        let rt_vc_id = match m.declare_function(
            "rt_virtual_call",
            Linkage::Import,
            &rt_virtual_call_sig,
        ) {
            Ok(id) => id,
            Err(_) => return (0, module.functions.len()),
        };
        self.rt_virtual_call_id = Some(rt_vc_id);

        let rt_cn_id = match m.declare_function(
            "rt_closure_new",
            Linkage::Import,
            &rt_closure_new_sig,
        ) {
            Ok(id) => id,
            Err(_) => return (0, module.functions.len()),
        };
        self.rt_closure_new_id = Some(rt_cn_id);

        let rt_cc_id = match m.declare_function(
            "rt_closure_call",
            Linkage::Import,
            &rt_closure_call_sig,
        ) {
            Ok(id) => id,
            Err(_) => return (0, module.functions.len()),
        };
        self.rt_closure_call_id = Some(rt_cc_id);

        let m33_safepoint_push_id = match m.declare_function(
            "rt_shadow_push",
            Linkage::Import,
            &m33_safepoint_push_sig,
        ) {
            Ok(id) => id,
            Err(_) => return (0, module.functions.len()),
        };
        self.m33_safepoint_push_id = Some(m33_safepoint_push_id);

        let m33_safepoint_pop_id = match m.declare_function(
            "rt_shadow_pop",
            Linkage::Import,
            &m33_safepoint_pop_sig,
        ) {
            Ok(id) => id,
            Err(_) => return (0, module.functions.len()),
        };
        self.m33_safepoint_pop_id = Some(m33_safepoint_pop_id);

        let m33_shadow_state_id = match m.declare_function(
            "rt_shadow_state",
            Linkage::Import,
            &m33_shadow_state_sig,
        ) {
            Ok(id) => id,
            Err(_) => return (0, module.functions.len()),
        };
        self.m33_shadow_state_id = Some(m33_shadow_state_id);

        // Declare every JIT-eligible function up front for cross-calls.
        for (i, f) in module.functions.iter().enumerate() {
            if decoded[i].is_some() {
                let name = format!("spy_fn_{}", f.fn_id);
                if let Ok(id) = m.declare_function(&name, Linkage::Local, &jit_sig) {
                    self.decl[f.fn_id as usize] = Some(id);
                }
            }
        }

        let mut ctx = m.make_context();
        let mut fnctx = FunctionBuilderContext::new();
        let mut compiled = 0usize;
        let total = module.functions.len();

        for (i, f) in module.functions.iter().enumerate() {
            let Some(decoded_fn) = decoded[i].as_ref() else { continue };
            let Some(decl_id) = self.decl[f.fn_id as usize] else { continue };

            ctx.clear();
            ctx.func.signature = jit_sig.clone();
            ctx.func.name = UserFuncName::user(0, f.fn_id);

            // Import every callee we'll need into this Function before
            // handing the FunctionBuilder out. We import all known direct
            // callees (cheap, ~M entries) plus the trampoline.
            let m = self.module.as_mut().expect("jit module live");
            let mut callee_refs: HashMap<u32, FuncRef> = HashMap::new();
            for (j, _) in module.functions.iter().enumerate() {
                if let Some(id) = self.decl[module.functions[j].fn_id as usize] {
                    let fref = m.declare_func_in_func(id, &mut ctx.func);
                    callee_refs.insert(module.functions[j].fn_id, fref);
                }
            }
            let trampoline_ref = m.declare_func_in_func(tid, &mut ctx.func);
            let alloc_str_ref = m.declare_func_in_func(asid, &mut ctx.func);
            let rt_list_push_ref = m.declare_func_in_func(rt_lp_id, &mut ctx.func);
            let rt_list_new_ref = m.declare_func_in_func(rt_ln_id, &mut ctx.func);
            let rt_array_new_ref = m.declare_func_in_func(rt_an_id, &mut ctx.func);
            let rt_alloc_ref = m.declare_func_in_func(rt_alloc_id, &mut ctx.func);
            let rt_virtual_call_ref = m.declare_func_in_func(rt_vc_id, &mut ctx.func);
            let rt_closure_new_ref = m.declare_func_in_func(rt_cn_id, &mut ctx.func);
            let rt_closure_call_ref = m.declare_func_in_func(rt_cc_id, &mut ctx.func);
            let m33_safepoint_push_ref =
                m.declare_func_in_func(m33_safepoint_push_id, &mut ctx.func);
            // rt_shadow_pop is no longer called by JIT'd code (pop is inline);
            // keep the FuncId declared for the exported symbol/tests, but we
            // don't import it per-function.
            let _ = m33_safepoint_pop_id;
            let m33_shadow_state_ref =
                m.declare_func_in_func(m33_shadow_state_id, &mut ctx.func);

            let helpers = RuntimeHelpers {
                rt_list_push: rt_list_push_ref,
                rt_list_new: rt_list_new_ref,
                rt_array_new: rt_array_new_ref,
                rt_alloc: rt_alloc_ref,
                rt_virtual_call: rt_virtual_call_ref,
                rt_closure_new: rt_closure_new_ref,
                rt_closure_call: rt_closure_call_ref,
                m33_safepoint_push: m33_safepoint_push_ref,
                m33_shadow_state: m33_shadow_state_ref,
            };

            let ok = translate_function(
                &mut ctx.func,
                &mut fnctx,
                decoded_fn,
                module,
                f.num_params as usize,
                f.num_registers as usize,
                &callee_refs,
                trampoline_ref,
                alloc_str_ref,
                &helpers,
            );
            if !ok {
                // Translation aborted (unsupported op encountered). The
                // function is declared, so to keep `finalize_definitions`
                // happy we must define *something* — emit a stub that
                // signals "fall back to interpreter" by returning 0 and
                // mark the slot for the dispatcher to ignore.
                //
                // Use a fresh `FunctionBuilderContext` here because the
                // aborted translation left `fnctx` in a partial state
                // (the FunctionBuilder was dropped without `finalize()`,
                // which is what normally resets the per-function context).
                ctx.clear();
                ctx.func.signature = jit_sig.clone();
                ctx.func.name = UserFuncName::user(0, f.fn_id);
                let mut stub_ctx = FunctionBuilderContext::new();
                emit_stub(&mut ctx.func, &mut stub_ctx);
                let m = self.module.as_mut().expect("jit module live");
                let _ = m.define_function(decl_id, &mut ctx);
                // Reset decl so `get()` returns None for this fn_id.
                self.decl[f.fn_id as usize] = None;
                // Replace the main fnctx too so the next function starts
                // from a clean slate.
                fnctx = FunctionBuilderContext::new();
                continue;
            }

            let m = self.module.as_mut().expect("jit module live");
            match m.define_function(decl_id, &mut ctx) {
                Ok(_) => compiled += 1,
                Err(_e) => {
                    // Verifier or codegen rejected the body. Replace with
                    // a stub and mark uncallable. Use a fresh
                    // `FunctionBuilderContext` for the same reason as the
                    // failed-translation path above.
                    ctx.clear();
                    ctx.func.signature = jit_sig.clone();
                    ctx.func.name = UserFuncName::user(0, f.fn_id);
                    let mut stub_ctx = FunctionBuilderContext::new();
                    emit_stub(&mut ctx.func, &mut stub_ctx);
                    let _ = m.define_function(decl_id, &mut ctx);
                    self.decl[f.fn_id as usize] = None;
                    fnctx = FunctionBuilderContext::new();
                }
            }
        }

        let m = self.module.as_mut().expect("jit module live");
        if m.finalize_definitions().is_err() {
            return (compiled, total);
        }
        for f in &module.functions {
            if let Some(id) = self.decl[f.fn_id as usize] {
                let ptr = m.get_finalized_function(id);
                // SAFETY: `ptr` is a freshly-finalised Cranelift function
                // pointer matching the unified JIT ABI declared above
                // (verified at compile time by the matching `Signature`).
                let jf: JitFn = unsafe { std::mem::transmute(ptr) };
                self.fn_ptrs[f.fn_id as usize] = Some(jf);
            }
        }

        // Per-function compilation status is dumped to stderr when the
        // env var is set — handy for triaging "why didn't this function
        // get JIT'd?" without rebuilding.
        if std::env::var("STRICTPY_JIT_DEBUG").is_ok() {
            for f in &module.functions {
                let nm = module
                    .strings
                    .get(f.name_idx as usize)
                    .map(|s| s.as_str())
                    .unwrap_or("?");
                let status = if self.fn_ptrs[f.fn_id as usize].is_some() {
                    "JIT"
                } else {
                    "interp"
                };
                eprintln!("[jit] {status:>6}  fn_id={} {}", f.fn_id, nm);
            }
            eprintln!("[jit] compiled {}/{}", compiled, total);
        }

        (compiled, total)
    }

    /// Look up the JIT entry for `fn_id`, or `None` if it wasn't compiled.
    pub fn get(&self, fn_id: u32) -> Option<JitFn> {
        self.fn_ptrs.get(fn_id as usize).and_then(|s| *s)
    }
}

impl Drop for Jit {
    fn drop(&mut self) {
        if let Some(m) = self.module.take() {
            // SAFETY: every interpreter holding a JitFn into this module is
            // dropped before the SharedVm, which owns the only `Jit`.
            unsafe { m.free_memory() };
        }
    }
}

/// Signature of every JIT'd StrictPy function.
fn make_jit_sig() -> Signature {
    let mut sig = Signature::new(host_call_conv());
    sig.params.push(AbiParam::new(types::I64)); // vm
    sig.params.push(AbiParam::new(types::I64)); // args
    sig.returns.push(AbiParam::new(types::I64));
    sig
}

fn make_trampoline_sig() -> Signature {
    let mut sig = Signature::new(host_call_conv());
    sig.params.push(AbiParam::new(types::I64)); // vm
    sig.params.push(AbiParam::new(types::I32)); // native_id
    sig.params.push(AbiParam::new(types::I64)); // args
    sig.params.push(AbiParam::new(types::I32)); // n_args
    sig.returns.push(AbiParam::new(types::I64));
    sig
}

fn make_alloc_str_sig() -> Signature {
    let mut sig = Signature::new(host_call_conv());
    sig.params.push(AbiParam::new(types::I64)); // vm
    sig.params.push(AbiParam::new(types::I32)); // str_idx
    sig.returns.push(AbiParam::new(types::I64));
    sig
}

/// `rt_list_push(vm: *mut, list: *mut ListRepr, value: u64)`.
fn make_rt_list_push_sig() -> Signature {
    let mut sig = Signature::new(host_call_conv());
    sig.params.push(AbiParam::new(types::I64)); // vm
    sig.params.push(AbiParam::new(types::I64)); // list ptr
    sig.params.push(AbiParam::new(types::I64)); // value (raw 8-byte slot)
    sig
}

/// `rt_list_new(vm, elem_size, capacity) -> *mut ListRepr`.
fn make_rt_list_new_sig() -> Signature {
    let mut sig = Signature::new(host_call_conv());
    sig.params.push(AbiParam::new(types::I64)); // vm
    sig.params.push(AbiParam::new(types::I32)); // elem_size
    sig.params.push(AbiParam::new(types::I32)); // capacity
    sig.returns.push(AbiParam::new(types::I64));
    sig
}

/// `rt_array_new(vm, elem_size, length) -> *mut ListRepr`.
fn make_rt_array_new_sig() -> Signature {
    let mut sig = Signature::new(host_call_conv());
    sig.params.push(AbiParam::new(types::I64)); // vm
    sig.params.push(AbiParam::new(types::I32)); // elem_size
    sig.params.push(AbiParam::new(types::I64)); // length
    sig.returns.push(AbiParam::new(types::I64));
    sig
}

/// `rt_alloc(vm, type_or_class_id) -> *mut u8`.
fn make_rt_alloc_sig() -> Signature {
    let mut sig = Signature::new(host_call_conv());
    sig.params.push(AbiParam::new(types::I64)); // vm
    sig.params.push(AbiParam::new(types::I32)); // type/class id
    sig.returns.push(AbiParam::new(types::I64));
    sig
}

/// Bundle of runtime helper `FuncRef`s passed into the translator so each
/// emitter site doesn't have to thread four separate fields.
struct RuntimeHelpers {
    rt_list_push: FuncRef,
    rt_list_new: FuncRef,
    rt_array_new: FuncRef,
    rt_alloc: FuncRef,
    rt_virtual_call: FuncRef,
    rt_closure_new: FuncRef,
    rt_closure_call: FuncRef,
    /// M33 shadow-stack publishers, called around every allocation
    /// helper so the GC sees this frame's register-resident pointers.
    m33_safepoint_push: FuncRef,
    /// `rt_shadow_state() -> *mut ShadowState`. Called once per function to
    /// obtain the stable per-thread state pointer for inline push/pop.
    m33_shadow_state: FuncRef,
}

/// `rt_virtual_call(vm, vtable_slot, args_ptr, n_args) -> u64`.
fn make_rt_virtual_call_sig() -> Signature {
    let mut sig = Signature::new(host_call_conv());
    sig.params.push(AbiParam::new(types::I64)); // vm
    sig.params.push(AbiParam::new(types::I32)); // vtable_slot
    sig.params.push(AbiParam::new(types::I64)); // args ptr
    sig.params.push(AbiParam::new(types::I32)); // n_args
    sig.returns.push(AbiParam::new(types::I64));
    sig
}

/// `rt_closure_new(vm, fn_id, caps_ptr, n_cap) -> *mut u8`.
fn make_rt_closure_new_sig() -> Signature {
    let mut sig = Signature::new(host_call_conv());
    sig.params.push(AbiParam::new(types::I64)); // vm
    sig.params.push(AbiParam::new(types::I32)); // fn_id
    sig.params.push(AbiParam::new(types::I64)); // caps ptr
    sig.params.push(AbiParam::new(types::I32)); // n_cap
    sig.returns.push(AbiParam::new(types::I64));
    sig
}

/// `rt_closure_call(vm, closure_ptr, args_ptr, n_args) -> u64`.
fn make_rt_closure_call_sig() -> Signature {
    let mut sig = Signature::new(host_call_conv());
    sig.params.push(AbiParam::new(types::I64)); // vm
    sig.params.push(AbiParam::new(types::I64)); // closure ptr
    sig.params.push(AbiParam::new(types::I64)); // args ptr
    sig.params.push(AbiParam::new(types::I32)); // n_args
    sig.returns.push(AbiParam::new(types::I64));
    sig
}

/// `rt_shadow_push(buf: *const u64, len: u64)` — M33 root publisher.
fn make_m33_safepoint_push_sig() -> Signature {
    let mut sig = Signature::new(host_call_conv());
    sig.params.push(AbiParam::new(types::I64)); // buf ptr
    sig.params.push(AbiParam::new(types::I64)); // len
    sig
}

/// `rt_shadow_pop()` — M33 root unpublisher.
fn make_m33_safepoint_pop_sig() -> Signature {
    Signature::new(host_call_conv())
}

/// `rt_shadow_state() -> *mut ShadowState` — returns the stable per-thread
/// shadow-state struct address used by the inline push/pop sequences.
fn make_m33_shadow_state_sig() -> Signature {
    let mut sig = Signature::new(host_call_conv());
    sig.returns.push(AbiParam::new(types::I64));
    sig
}

/// Emit a stub body that returns 0 immediately. Used when translation
/// aborts on an unsupported op: the function was already declared in the
/// JIT module, and finalize panics if a declaration is left undefined.
fn emit_stub(func: &mut Function, fnctx: &mut FunctionBuilderContext) {
    let mut b = FunctionBuilder::new(func, fnctx);
    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    b.seal_block(entry);
    let z = b.ins().iconst(types::I64, 0);
    b.ins().return_(&[z]);
    b.finalize();
}

/// Wrap the per-function Cranelift translation. Returns `true` on success;
/// `false` means we ran into an op the JIT doesn't support and the caller
/// should leave this function to the interpreter.
fn translate_function(
    func: &mut Function,
    fnctx: &mut FunctionBuilderContext,
    decoded: &DecodedFunction,
    module: &Module,
    num_params: usize,
    num_registers: usize,
    callee_refs: &HashMap<u32, FuncRef>,
    trampoline_ref: FuncRef,
    alloc_str_ref: FuncRef,
    helpers: &RuntimeHelpers,
) -> bool {
    let mut builder = FunctionBuilder::new(func, fnctx);

    // Map every bytecode block-start pc to a Cranelift block.
    let mut blocks: HashMap<usize, cranelift::codegen::ir::Block> = HashMap::new();
    for &pc in &decoded.block_starts {
        let b = builder.create_block();
        blocks.insert(pc, b);
    }

    let entry_pc = *decoded.block_starts.iter().next().expect("entry");
    let entry = blocks[&entry_pc];
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);

    let vm_ptr = builder.block_params(entry)[0];
    let args_ptr = builder.block_params(entry)[1];

    // Allocate one I64 variable per register.
    let nregs = num_registers.max(num_params).max(1);
    let mut reg_var: Vec<Variable> = Vec::with_capacity(nregs);
    for i in 0..nregs {
        let v = Variable::from_u32(i as u32);
        builder.declare_var(v, types::I64);
        reg_var.push(v);
    }

    // M33: one shadow stack slot per JIT'd function, sized for the full
    // register file. Spilled before each heap-allocating helper call so
    // the GC has a precise root window during collection.
    let m33_shadow_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        (nregs * 8) as u32,
        3,
    ));

    // Load each parameter from args[i] into the matching register variable.
    for i in 0..num_params {
        let off = (i * 8) as i32;
        let val = builder
            .ins()
            .load(types::I64, MemFlags::trusted(), args_ptr, off);
        builder.def_var(reg_var[i], val);
    }
    // Default everything else to zero so an early read doesn't grab UB.
    let zero64 = builder.ins().iconst(types::I64, 0);
    for i in num_params..nregs {
        builder.def_var(reg_var[i], zero64);
    }

    // M33 inline-publish: fetch the stable per-thread shadow-state address
    // once, here in the entry block, and cache it. Every inline push/pop in
    // this function reuses this value. The address lives in thread-local
    // storage and is stable for the life of the thread, so caching is sound
    // (a slow-path grow may move the backing Vec, but the JIT reloads
    // `base`/`cap`/`depth` from *this* struct on every push, so it sees the
    // updated pointer).
    let m33_state_call = builder.ins().call(helpers.m33_shadow_state, &[]);
    let m33_state_ptr = builder.inst_results(m33_state_call)[0];

    // Group ops by block.
    let mut by_block: HashMap<usize, Vec<&decompile::DecodedOp>> = HashMap::new();
    let mut current_block_pc = entry_pc;
    let mut current_ops: Vec<&decompile::DecodedOp> = Vec::new();
    for op in &decoded.ops {
        if decoded.block_starts.contains(&op.pc) && op.pc != current_block_pc {
            if !current_ops.is_empty() {
                by_block.insert(current_block_pc, std::mem::take(&mut current_ops));
            }
            current_block_pc = op.pc;
        }
        current_ops.push(op);
    }
    if !current_ops.is_empty() {
        by_block.insert(current_block_pc, current_ops);
    }

    let mut t = Translator {
        builder,
        reg_var,
        blocks,
        vm_ptr,
        callee_refs,
        trampoline_ref,
        alloc_str_ref,
        rt_list_push_ref: helpers.rt_list_push,
        rt_list_new_ref: helpers.rt_list_new,
        rt_array_new_ref: helpers.rt_array_new,
        rt_alloc_ref: helpers.rt_alloc,
        rt_virtual_call_ref: helpers.rt_virtual_call,
        rt_closure_new_ref: helpers.rt_closure_new,
        rt_closure_call_ref: helpers.rt_closure_call,
        m33_shadow_slot,
        m33_safepoint_push_ref: helpers.m33_safepoint_push,
        m33_state_ptr,
        module,
    };

    let mut block_pcs: Vec<usize> = decoded.block_starts.iter().copied().collect();
    block_pcs.sort();

    let mut terminated: HashMap<usize, bool> = HashMap::new();

    for &pc in &block_pcs {
        let cl_block = t.blocks[&pc];
        if pc != entry_pc {
            t.builder.switch_to_block(cl_block);
        }
        terminated.insert(pc, false);
        let mut term = false;
        if let Some(ops) = by_block.get(&pc) {
            for op in ops {
                if !t.emit(&op.kind) {
                    return false;
                }
                if matches!(
                    &op.kind,
                    Op::Ret { .. }
                        | Op::RetVoid
                        | Op::Jump { .. }
                        | Op::JumpIf { .. }
                        | Op::JumpIfNot { .. }
                ) {
                    term = true;
                    break;
                }
            }
        }
        if !term {
            let next_pc = block_pcs.iter().find(|&&p| p > pc).copied();
            if let Some(next) = next_pc {
                if let Some(&target) = t.blocks.get(&next) {
                    t.builder.ins().jump(target, &[]);
                } else {
                    let z = t.builder.ins().iconst(types::I64, 0);
                    t.builder.ins().return_(&[z]);
                }
            } else {
                let z = t.builder.ins().iconst(types::I64, 0);
                t.builder.ins().return_(&[z]);
            }
        }
        terminated.insert(pc, true);
    }

    // Seal everything.
    for (_pc, &b) in t.blocks.iter() {
        t.builder.seal_block(b);
    }
    t.builder.finalize();
    true
}

struct Translator<'a> {
    builder: FunctionBuilder<'a>,
    reg_var: Vec<Variable>,
    blocks: HashMap<usize, cranelift::codegen::ir::Block>,
    vm_ptr: cranelift::codegen::ir::Value,
    callee_refs: &'a HashMap<u32, FuncRef>,
    trampoline_ref: FuncRef,
    alloc_str_ref: FuncRef,
    rt_list_push_ref: FuncRef,
    rt_list_new_ref: FuncRef,
    rt_array_new_ref: FuncRef,
    rt_alloc_ref: FuncRef,
    rt_virtual_call_ref: FuncRef,
    rt_closure_new_ref: FuncRef,
    rt_closure_call_ref: FuncRef,
    /// M33: per-function shadow stack slot. Holds `nregs * 8` bytes — one
    /// u64 per StrictPy register. Spilled into right before every
    /// heap-allocating runtime helper call and published via
    /// `rt_shadow_push` so the GC can scan it as a root window.
    m33_shadow_slot: cranelift::codegen::ir::StackSlot,
    /// `rt_shadow_push` slow-path/overflow grow helper. Called inline only
    /// when the fast path finds the backing store full.
    m33_safepoint_push_ref: FuncRef,
    /// Cached result of `rt_shadow_state()` for this function: the stable
    /// per-thread `ShadowState` address. Computed once at entry and reused
    /// by every inline push/pop. The address is thread-local-stable so this
    /// is valid for the whole function.
    m33_state_ptr: cranelift::codegen::ir::Value,
    module: &'a Module,
}

impl<'a> Translator<'a> {
    fn read_reg(&mut self, r: u16) -> cranelift::codegen::ir::Value {
        if r == 0xFFFF || r as usize >= self.reg_var.len() {
            return self.builder.ins().iconst(types::I64, 0);
        }
        self.builder.use_var(self.reg_var[r as usize])
    }

    fn write_reg(&mut self, r: u16, v: cranelift::codegen::ir::Value) {
        if r == 0xFFFF || r as usize >= self.reg_var.len() {
            return;
        }
        let ty = self.builder.func.dfg.value_type(v);
        let v64 = if ty == types::I64 {
            v
        } else if ty == types::I32 {
            self.builder.ins().sextend(types::I64, v)
        } else if ty == types::I8 {
            self.builder.ins().uextend(types::I64, v)
        } else if ty == types::F32 {
            let bits = self.builder.ins().bitcast(types::I32, MemFlags::new(), v);
            self.builder.ins().uextend(types::I64, bits)
        } else if ty == types::F64 {
            self.builder.ins().bitcast(types::I64, MemFlags::new(), v)
        } else {
            v
        };
        self.builder.def_var(self.reg_var[r as usize], v64);
    }

    fn read_i32(&mut self, r: u16) -> cranelift::codegen::ir::Value {
        let v = self.read_reg(r);
        self.builder.ins().ireduce(types::I32, v)
    }

    fn read_f64(&mut self, r: u16) -> cranelift::codegen::ir::Value {
        let v = self.read_reg(r);
        self.builder.ins().bitcast(types::F64, MemFlags::new(), v)
    }

    fn read_f32(&mut self, r: u16) -> cranelift::codegen::ir::Value {
        let v = self.read_reg(r);
        let v32 = self.builder.ins().ireduce(types::I32, v);
        self.builder.ins().bitcast(types::F32, MemFlags::new(), v32)
    }

    fn alloca_u64_buf(&mut self, n: usize) -> cranelift::codegen::ir::Value {
        let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            (n.max(1) * 8) as u32,
            3,
        ));
        self.builder.ins().stack_addr(types::I64, slot, 0)
    }

    /// M33: spill every StrictPy register variable into the per-function
    /// shadow stack slot and publish the resulting window via
    /// `rt_shadow_push`. Call this immediately before any heap-allocating
    /// runtime helper; pair with [`Self::m33_safepoint_leave`] afterwards.
    ///
    /// The spill is conservative: we write every register, not just the
    /// ones currently holding a pointer. False positives are safe (the
    /// GC's `alive` set rejects integers that don't alias a live
    /// allocation), and emitting one store per register is far cheaper
    /// than running a Cranelift-level liveness analysis from inside the
    /// translator.
    fn m33_safepoint_enter(&mut self) {
        let nregs = self.reg_var.len();
        for r in 0..nregs {
            let v = self.builder.use_var(self.reg_var[r]);
            self.builder.ins().stack_store(
                v,
                self.m33_shadow_slot,
                (r * 8) as i32,
            );
        }
        let buf_addr = self
            .builder
            .ins()
            .stack_addr(types::I64, self.m33_shadow_slot, 0);
        let len_v = self.builder.ins().iconst(types::I64, nregs as i64);

        // Inline PUSH (replaces the rt_shadow_push call). The state struct
        // layout (see stackmap_registry::ShadowState, #[repr(C)]):
        //   [0]  base  : *mut ShadowFrame
        //   [8]  depth : usize
        //   [16] cap   : usize
        // ShadowFrame is { buf @0, len @8 }, size 16.
        let state = self.m33_state_ptr;
        let trusted = MemFlags::trusted();
        // Load depth and cap (base is reloaded only on the fast path, after
        // we know depth < cap, so a slow-path grow that moved the Vec is
        // always picked up here).
        let depth = self.builder.ins().load(types::I64, trusted, state, 8);
        let cap = self.builder.ins().load(types::I64, trusted, state, 16);

        let fast_blk = self.builder.create_block();
        let slow_blk = self.builder.create_block();
        let merge_blk = self.builder.create_block();

        // if depth < cap -> fast, else -> slow.
        let lt = self.builder.ins().icmp(
            cranelift::codegen::ir::condcodes::IntCC::UnsignedLessThan,
            depth,
            cap,
        );
        self.builder
            .ins()
            .brif(lt, fast_blk, &[], slow_blk, &[]);
        // Both have a single predecessor (this block); safe to seal now.
        self.builder.seal_block(fast_blk);
        self.builder.seal_block(slow_blk);

        // ── fast path: base[depth] = (buf, len); depth += 1 ──
        self.builder.switch_to_block(fast_blk);
        let base = self.builder.ins().load(types::I64, trusted, state, 0);
        // off = depth * 16
        let off = self.builder.ins().imul_imm(depth, 16);
        let slot_addr = self.builder.ins().iadd(base, off);
        self.builder
            .ins()
            .store(trusted, buf_addr, slot_addr, 0);
        self.builder.ins().store(trusted, len_v, slot_addr, 8);
        let depth1 = self.builder.ins().iadd_imm(depth, 1);
        self.builder.ins().store(trusted, depth1, state, 8);
        self.builder.ins().jump(merge_blk, &[]);

        // ── slow path: call rt_shadow_push(buf, len) (grows + syncs) ──
        self.builder.switch_to_block(slow_blk);
        self.builder
            .ins()
            .call(self.m33_safepoint_push_ref, &[buf_addr, len_v]);
        self.builder.ins().jump(merge_blk, &[]);

        // ── merge: continue emitting into this block ──
        self.builder.seal_block(merge_blk);
        self.builder.switch_to_block(merge_blk);
    }

    /// M33: pop the matching shadow-stack window after the helper call
    /// returns. Must balance every prior [`Self::m33_safepoint_enter`].
    ///
    /// Inline POP (replaces the rt_shadow_pop call): load depth, store
    /// depth - 1. No call. (depth is guaranteed > 0 here because every pop
    /// is emitted to balance a prior push in the same straight-line
    /// region.)
    fn m33_safepoint_leave(&mut self) {
        let state = self.m33_state_ptr;
        let trusted = MemFlags::trusted();
        let depth = self.builder.ins().load(types::I64, trusted, state, 8);
        let depth1 = self.builder.ins().iadd_imm(depth, -1);
        self.builder.ins().store(trusted, depth1, state, 8);
    }

    fn emit(&mut self, op: &Op) -> bool {
        match op {
            Op::ConstI32 { dst, val } => {
                let v = self.builder.ins().iconst(types::I64, *val as i64);
                self.write_reg(*dst, v);
            }
            Op::ConstI64 { dst, idx } => {
                let v = match self.module.constants.get(*idx as usize) {
                    Some(Constant::I64(x)) => *x,
                    Some(Constant::I32(x)) => *x as i64,
                    Some(Constant::U64(x)) => *x as i64,
                    Some(Constant::U32(x)) => *x as i64,
                    _ => return false,
                };
                let cv = self.builder.ins().iconst(types::I64, v);
                self.write_reg(*dst, cv);
            }
            Op::ConstF32 { dst, bits } => {
                let v = self.builder.ins().iconst(types::I64, *bits as i64);
                self.write_reg(*dst, v);
            }
            Op::ConstF64 { dst, idx } => {
                let bits = match self.module.constants.get(*idx as usize) {
                    Some(Constant::F64(v)) => v.to_bits(),
                    Some(Constant::F32(v)) => (*v as f64).to_bits(),
                    Some(Constant::I64(v)) => (*v as f64).to_bits(),
                    _ => return false,
                };
                let cv = self.builder.ins().iconst(types::I64, bits as i64);
                self.write_reg(*dst, cv);
            }
            Op::ConstTrue { dst } => {
                let v = self.builder.ins().iconst(types::I64, 1);
                self.write_reg(*dst, v);
            }
            Op::ConstFalse { dst } => {
                let v = self.builder.ins().iconst(types::I64, 0);
                self.write_reg(*dst, v);
            }
            Op::ConstStr { dst, idx } => {
                // Allocate the string lazily via the trampoline so each
                // call gets a fresh, GC-managed heap copy — matching the
                // interpreter's `op_const_str` semantics. The cost is a
                // call per execution of the op, but that's fine for the
                // current acceptance examples where ConstStr appears in
                // `println(...)` arguments and the like.
                //
                // M33: publish the caller's register window so the GC has
                // precise roots if the allocation triggers a collection.
                let idx_v = self.builder.ins().iconst(types::I32, *idx as i64);
                self.m33_safepoint_enter();
                let call = self
                    .builder
                    .ins()
                    .call(self.alloc_str_ref, &[self.vm_ptr, idx_v]);
                self.m33_safepoint_leave();
                let result = self.builder.inst_results(call)[0];
                self.write_reg(*dst, result);
            }
            Op::ConstNoneSentinel { dst } => {
                // Matches `crate::builtins::NONE_SENTINEL` (also written
                // by `op_const_none`).
                let v = self
                    .builder
                    .ins()
                    .iconst(types::I64, crate::builtins::NONE_SENTINEL as i64);
                self.write_reg(*dst, v);
            }
            Op::Move { dst, src } => {
                let v = self.read_reg(*src);
                self.write_reg(*dst, v);
            }

            Op::IBin { op, w, dst, a, b } => {
                let (av, bv) = match w {
                    NumWidth::W64 => (self.read_reg(*a), self.read_reg(*b)),
                    NumWidth::W32 => (self.read_i32(*a), self.read_i32(*b)),
                };
                let r = match op {
                    IntBinOp::Add => self.builder.ins().iadd(av, bv),
                    IntBinOp::Sub => self.builder.ins().isub(av, bv),
                    IntBinOp::Mul => self.builder.ins().imul(av, bv),
                    IntBinOp::And => self.builder.ins().band(av, bv),
                    IntBinOp::Or => self.builder.ins().bor(av, bv),
                    IntBinOp::Xor => self.builder.ins().bxor(av, bv),
                    IntBinOp::Shl => self.builder.ins().ishl(av, bv),
                    IntBinOp::ShrSigned => self.builder.ins().sshr(av, bv),
                    IntBinOp::ShrUnsigned => self.builder.ins().ushr(av, bv),
                };
                self.write_reg(*dst, r);
            }
            Op::INeg { w, dst, a } => {
                let av = match w {
                    NumWidth::W64 => self.read_reg(*a),
                    NumWidth::W32 => self.read_i32(*a),
                };
                let r = self.builder.ins().ineg(av);
                self.write_reg(*dst, r);
            }
            Op::INot { w, dst, a } => {
                let av = match w {
                    NumWidth::W64 => self.read_reg(*a),
                    NumWidth::W32 => self.read_i32(*a),
                };
                let r = self.builder.ins().bnot(av);
                self.write_reg(*dst, r);
            }
            Op::IDivChk { w, dst, a, b } => {
                let (av, bv) = match w {
                    NumWidth::W64 => (self.read_reg(*a), self.read_reg(*b)),
                    NumWidth::W32 => (self.read_i32(*a), self.read_i32(*b)),
                };
                let r = self.builder.ins().sdiv(av, bv);
                self.write_reg(*dst, r);
            }
            Op::IRemChk { w, dst, a, b } => {
                let (av, bv) = match w {
                    NumWidth::W64 => (self.read_reg(*a), self.read_reg(*b)),
                    NumWidth::W32 => (self.read_i32(*a), self.read_i32(*b)),
                };
                let r = self.builder.ins().srem(av, bv);
                self.write_reg(*dst, r);
            }
            Op::FBin { op, w, dst, a, b } => {
                let (av, bv) = match w {
                    FloatWidth::F64 => (self.read_f64(*a), self.read_f64(*b)),
                    FloatWidth::F32 => (self.read_f32(*a), self.read_f32(*b)),
                };
                let r = match op {
                    FloatBinOp::Add => self.builder.ins().fadd(av, bv),
                    FloatBinOp::Sub => self.builder.ins().fsub(av, bv),
                    FloatBinOp::Mul => self.builder.ins().fmul(av, bv),
                    FloatBinOp::Div => self.builder.ins().fdiv(av, bv),
                };
                self.write_reg(*dst, r);
            }
            Op::FNeg { w, dst, a } => {
                let av = match w {
                    FloatWidth::F64 => self.read_f64(*a),
                    FloatWidth::F32 => self.read_f32(*a),
                };
                let r = self.builder.ins().fneg(av);
                self.write_reg(*dst, r);
            }

            Op::ICmp { op, w, signed, dst, a, b } => {
                let (av, bv) = match w {
                    NumWidth::W64 => (self.read_reg(*a), self.read_reg(*b)),
                    NumWidth::W32 => (self.read_i32(*a), self.read_i32(*b)),
                };
                let cc = int_cc(*op, *signed);
                let r = self.builder.ins().icmp(cc, av, bv);
                let r64 = self.builder.ins().uextend(types::I64, r);
                self.write_reg(*dst, r64);
            }
            Op::FCmp { op, w, dst, a, b } => {
                let (av, bv) = match w {
                    FloatWidth::F64 => (self.read_f64(*a), self.read_f64(*b)),
                    FloatWidth::F32 => (self.read_f32(*a), self.read_f32(*b)),
                };
                let cc = float_cc(*op);
                let r = self.builder.ins().fcmp(cc, av, bv);
                let r64 = self.builder.ins().uextend(types::I64, r);
                self.write_reg(*dst, r64);
            }

            Op::I32ToI64 { dst, src } => {
                let lo = self.read_i32(*src);
                let v = self.builder.ins().sextend(types::I64, lo);
                self.write_reg(*dst, v);
            }
            Op::I64ToI32 { dst, src } => {
                let lo = self.read_i32(*src);
                let v = self.builder.ins().sextend(types::I64, lo);
                self.write_reg(*dst, v);
            }
            Op::I32ToF64 { dst, src } => {
                let lo = self.read_i32(*src);
                let f = self.builder.ins().fcvt_from_sint(types::F64, lo);
                self.write_reg(*dst, f);
            }
            Op::F64ToI32 { dst, src } => {
                let f = self.read_f64(*src);
                let i = self.builder.ins().fcvt_to_sint(types::I32, f);
                let v = self.builder.ins().sextend(types::I64, i);
                self.write_reg(*dst, v);
            }

            Op::Jump { target } => {
                let Some(&b) = self.blocks.get(target) else { return false };
                self.builder.ins().jump(b, &[]);
            }
            Op::JumpIf { cond, target, fallthrough } => {
                let cv = self.read_reg(*cond);
                let (Some(&tb), Some(&fb)) =
                    (self.blocks.get(target), self.blocks.get(fallthrough)) else {
                    return false;
                };
                self.builder.ins().brif(cv, tb, &[], fb, &[]);
            }
            Op::JumpIfNot { cond, target, fallthrough } => {
                let cv = self.read_reg(*cond);
                let (Some(&tb), Some(&fb)) =
                    (self.blocks.get(target), self.blocks.get(fallthrough)) else {
                    return false;
                };
                // brif jumps to first block when cond != 0; we want the
                // opposite, so swap.
                self.builder.ins().brif(cv, fb, &[], tb, &[]);
            }
            Op::Ret { src } => {
                let v = self.read_reg(*src);
                self.builder.ins().return_(&[v]);
            }
            Op::RetVoid => {
                let z = self.builder.ins().iconst(types::I64, 0);
                self.builder.ins().return_(&[z]);
            }

            Op::CallDirect { dst, fn_id, args } => {
                let Some(&func_ref) = self.callee_refs.get(fn_id) else {
                    // The target is interpreted-only — bail this whole
                    // function back to the interpreter for now. A future
                    // iteration adds a helper trampoline.
                    return false;
                };
                let n = args.len();
                let buf = self.alloca_u64_buf(n);
                for (i, r) in args.iter().enumerate() {
                    let v = self.read_reg(*r);
                    self.builder
                        .ins()
                        .store(MemFlags::trusted(), v, buf, (i * 8) as i32);
                }
                // M33: another JIT'd function may allocate transitively.
                // Publish before the call; pop after.
                self.m33_safepoint_enter();
                let call = self.builder.ins().call(func_ref, &[self.vm_ptr, buf]);
                self.m33_safepoint_leave();
                let result = self.builder.inst_results(call)[0];
                self.write_reg(*dst, result);
            }
            Op::CallNative { dst, native_id, args } => {
                let n = args.len();
                let buf = self.alloca_u64_buf(n);
                for (i, r) in args.iter().enumerate() {
                    let v = self.read_reg(*r);
                    self.builder
                        .ins()
                        .store(MemFlags::trusted(), v, buf, (i * 8) as i32);
                }
                let nid = self.builder.ins().iconst(types::I32, *native_id as i64);
                let n_args = self.builder.ins().iconst(types::I32, n as i64);
                // M33: the native trampoline re-enters the interpreter,
                // which may run arbitrary builtins (`alloc_*`, `str`, etc.).
                // Publish the register window before the call.
                self.m33_safepoint_enter();
                let call = self
                    .builder
                    .ins()
                    .call(self.trampoline_ref, &[self.vm_ptr, nid, buf, n_args]);
                self.m33_safepoint_leave();
                let result = self.builder.inst_results(call)[0];
                self.write_reg(*dst, result);
            }

            Op::ArrayLen { dst, list } => {
                // ListRepr: header(16) + length(8) + capacity(8) + data(8).
                let lp = self.read_reg(*list);
                let len = self
                    .builder
                    .ins()
                    .load(types::I64, MemFlags::trusted(), lp, 16);
                self.write_reg(*dst, len);
            }
            Op::ArrayGet { dst, list, idx, elem_tag } => {
                let lp = self.read_reg(*list);
                let data = self
                    .builder
                    .ins()
                    .load(types::I64, MemFlags::trusted(), lp, 32);
                let idx_v = self.read_reg(*idx);
                let off = self.builder.ins().imul_imm(idx_v, 8);
                let addr = self.builder.ins().iadd(data, off);
                // Skip bounds checks per spec ("v1 SKIP bounds checks").
                let v = if *elem_tag == strictpy_shared::TypeTag::F64 as u8 {
                    let f = self
                        .builder
                        .ins()
                        .load(types::F64, MemFlags::trusted(), addr, 0);
                    self.builder.ins().bitcast(types::I64, MemFlags::new(), f)
                } else {
                    self.builder
                        .ins()
                        .load(types::I64, MemFlags::trusted(), addr, 0)
                };
                self.write_reg(*dst, v);
            }

            Op::ArraySet { list, idx, src, elem_tag } => {
                // ListRepr layout: header(16) + length(8) + capacity(8) +
                // data(8). data is the *backing buffer* pointer; the M3
                // representation packs every element as a u64 slot.
                //
                // Per spec ("v1 SKIP bounds checks") we don't emit a guard.
                // The compiler's BoundsCheck stays as the M3 NullCheck
                // placeholder which the JIT already no-ops above.
                let lp = self.read_reg(*list);
                let data = self
                    .builder
                    .ins()
                    .load(types::I64, MemFlags::trusted(), lp, 32);
                let idx_v = self.read_reg(*idx);
                let off = self.builder.ins().imul_imm(idx_v, 8);
                let addr = self.builder.ins().iadd(data, off);
                let v = self.read_reg(*src);
                if *elem_tag == strictpy_shared::TypeTag::F64 as u8 {
                    let f = self.builder.ins().bitcast(types::F64, MemFlags::new(), v);
                    self.builder.ins().store(MemFlags::trusted(), f, addr, 0);
                } else {
                    // All other 8-byte tags (I64/U64/Ref/etc.) — store the
                    // raw register value as-is. The list is type-erased to
                    // u64 slots, so the byte pattern goes in verbatim.
                    self.builder.ins().store(MemFlags::trusted(), v, addr, 0);
                }
            }
            Op::ListPush { list, value } => {
                let lp = self.read_reg(*list);
                let vv = self.read_reg(*value);
                // M33: list push may grow the backing buffer through the
                // heap's `realloc_raw`. Publish before the call.
                self.m33_safepoint_enter();
                self.builder
                    .ins()
                    .call(self.rt_list_push_ref, &[self.vm_ptr, lp, vv]);
                self.m33_safepoint_leave();
            }
            Op::ListNew { dst, capacity } => {
                let elem_size = self.builder.ins().iconst(types::I32, 8);
                let cap = self.builder.ins().iconst(types::I32, *capacity as i64);
                self.m33_safepoint_enter();
                let call = self
                    .builder
                    .ins()
                    .call(self.rt_list_new_ref, &[self.vm_ptr, elem_size, cap]);
                self.m33_safepoint_leave();
                let result = self.builder.inst_results(call)[0];
                self.write_reg(*dst, result);
            }
            Op::ArrayNew { dst, length } => {
                let elem_size = self.builder.ins().iconst(types::I32, 8);
                let len_v = self.read_reg(*length);
                self.m33_safepoint_enter();
                let call = self
                    .builder
                    .ins()
                    .call(self.rt_array_new_ref, &[self.vm_ptr, elem_size, len_v]);
                self.m33_safepoint_leave();
                let result = self.builder.inst_results(call)[0];
                self.write_reg(*dst, result);
            }

            Op::New { dst, type_id } => {
                let tid = self.builder.ins().iconst(types::I32, *type_id as i64);
                // M33: `rt_alloc` is the prototypical "may collect" call;
                // it's the path the M26 btree(10k) workload exercises 10k
                // times in a tight recursive loop.
                self.m33_safepoint_enter();
                let call = self
                    .builder
                    .ins()
                    .call(self.rt_alloc_ref, &[self.vm_ptr, tid]);
                self.m33_safepoint_leave();
                let result = self.builder.inst_results(call)[0];
                self.write_reg(*dst, result);
            }
            Op::LoadField { dst, obj, offset, ty_tag } => {
                // Field offsets in M3 are relative to the start of fields,
                // i.e. they don't include the 16-byte header.
                const HDR: i32 = 16;
                let op = self.read_reg(*obj);
                let addr = self
                    .builder
                    .ins()
                    .iadd_imm(op, (HDR as i64) + (*offset as i64));
                let v = self.load_typed(*ty_tag, addr);
                self.write_reg(*dst, v);
            }
            Op::StoreField { obj, offset, src, ty_tag } => {
                const HDR: i32 = 16;
                let op = self.read_reg(*obj);
                let addr = self
                    .builder
                    .ins()
                    .iadd_imm(op, (HDR as i64) + (*offset as i64));
                let v = self.read_reg(*src);
                self.store_typed(*ty_tag, addr, v);
            }
            Op::VirtualCall { dst, recv, vtable_slot, args } => {
                // The compiler stores the receiver register separately
                // (`recv`) from the argument list (`args` is the *tail*,
                // i.e. everything after `self`). We need to prepend the
                // receiver into the marshalled buffer so the callee sees
                // `self` at register 0. See codegen.rs `IROp::VirtualCall`
                // and the interpreter's `op_call_virtual`.
                let n = 1 + args.len();
                let buf = self.alloca_u64_buf(n);
                let recv_v = self.read_reg(*recv);
                self.builder
                    .ins()
                    .store(MemFlags::trusted(), recv_v, buf, 0);
                for (i, r) in args.iter().enumerate() {
                    let v = self.read_reg(*r);
                    self.builder
                        .ins()
                        .store(MemFlags::trusted(), v, buf, ((i + 1) * 8) as i32);
                }
                let slot = self.builder.ins().iconst(types::I32, *vtable_slot as i64);
                let n_args = self.builder.ins().iconst(types::I32, n as i64);
                // M33: a virtual call dispatches through the interpreter's
                // invoke, which may allocate. Publish the window.
                self.m33_safepoint_enter();
                let call = self.builder.ins().call(
                    self.rt_virtual_call_ref,
                    &[self.vm_ptr, slot, buf, n_args],
                );
                self.m33_safepoint_leave();
                let result = self.builder.inst_results(call)[0];
                self.write_reg(*dst, result);
            }

            Op::ClosureNew { dst, fn_id, captures } => {
                // Marshal the capture registers into a stack buffer and hand
                // them to `rt_closure_new`, which allocates a `ClosureRepr`
                // with the captures stored inline. The captures are pointers
                // into the GC heap; publishing the register window before the
                // call keeps them rooted if the allocation collects.
                let n = captures.len();
                let buf = self.alloca_u64_buf(n);
                for (i, r) in captures.iter().enumerate() {
                    let v = self.read_reg(*r);
                    self.builder
                        .ins()
                        .store(MemFlags::trusted(), v, buf, (i * 8) as i32);
                }
                let fid = self.builder.ins().iconst(types::I32, *fn_id as i64);
                let n_cap = self.builder.ins().iconst(types::I32, n as i64);
                self.m33_safepoint_enter();
                let call = self.builder.ins().call(
                    self.rt_closure_new_ref,
                    &[self.vm_ptr, fid, buf, n_cap],
                );
                self.m33_safepoint_leave();
                let result = self.builder.inst_results(call)[0];
                self.write_reg(*dst, result);
            }
            Op::ClosureCall { dst, recv, args } => {
                // The closure pointer is `recv`; the explicit call arguments
                // follow (the inline captures are prepended on the Rust side
                // by `call_callable`, mirroring `Opcode::ClosureCall`). Only
                // the explicit args go in the marshalled buffer.
                let n = args.len();
                let buf = self.alloca_u64_buf(n);
                for (i, r) in args.iter().enumerate() {
                    let v = self.read_reg(*r);
                    self.builder
                        .ins()
                        .store(MemFlags::trusted(), v, buf, (i * 8) as i32);
                }
                let recv_v = self.read_reg(*recv);
                let n_args = self.builder.ins().iconst(types::I32, n as i64);
                self.m33_safepoint_enter();
                let call = self.builder.ins().call(
                    self.rt_closure_call_ref,
                    &[self.vm_ptr, recv_v, buf, n_args],
                );
                self.m33_safepoint_leave();
                let result = self.builder.inst_results(call)[0];
                self.write_reg(*dst, result);
            }

            Op::NullCheck { src } => {
                // BoundsCheck shares this opcode in the current compiler.
                // Suppress traps for now (the interpreter would have caught
                // the same condition during translation).
                let _ = src;
            }
        }
        true
    }

    /// Load a typed value of the width implied by `ty_tag` from `addr` and
    /// promote to a u64 register slot (matching the interpreter's
    /// `load_typed`). See spec §13.3.6 for the tag values.
    fn load_typed(&mut self, ty_tag: u8, addr: cranelift::codegen::ir::Value)
        -> cranelift::codegen::ir::Value
    {
        let tag = strictpy_shared::TypeTag::from_u8(ty_tag);
        let flags = MemFlags::trusted();
        match tag {
            Some(strictpy_shared::TypeTag::I8) => {
                let lo = self.builder.ins().load(types::I8, flags, addr, 0);
                self.builder.ins().sextend(types::I64, lo)
            }
            Some(strictpy_shared::TypeTag::U8) | Some(strictpy_shared::TypeTag::Bool) => {
                let lo = self.builder.ins().load(types::I8, flags, addr, 0);
                self.builder.ins().uextend(types::I64, lo)
            }
            Some(strictpy_shared::TypeTag::I16) => {
                let lo = self.builder.ins().load(types::I16, flags, addr, 0);
                self.builder.ins().sextend(types::I64, lo)
            }
            Some(strictpy_shared::TypeTag::U16) => {
                let lo = self.builder.ins().load(types::I16, flags, addr, 0);
                self.builder.ins().uextend(types::I64, lo)
            }
            Some(strictpy_shared::TypeTag::I32) => {
                let lo = self.builder.ins().load(types::I32, flags, addr, 0);
                self.builder.ins().sextend(types::I64, lo)
            }
            Some(strictpy_shared::TypeTag::U32) | Some(strictpy_shared::TypeTag::F32) => {
                let lo = self.builder.ins().load(types::I32, flags, addr, 0);
                self.builder.ins().uextend(types::I64, lo)
            }
            // I64 / U64 / F64 / Ref → straight 8-byte load.
            _ => self.builder.ins().load(types::I64, flags, addr, 0),
        }
    }

    /// Store the low bytes of `val` (a u64 register slot) into `addr` using
    /// the width implied by `ty_tag`.
    fn store_typed(
        &mut self,
        ty_tag: u8,
        addr: cranelift::codegen::ir::Value,
        val: cranelift::codegen::ir::Value,
    ) {
        let tag = strictpy_shared::TypeTag::from_u8(ty_tag);
        let flags = MemFlags::trusted();
        match tag {
            Some(strictpy_shared::TypeTag::I8)
            | Some(strictpy_shared::TypeTag::U8)
            | Some(strictpy_shared::TypeTag::Bool) => {
                let lo = self.builder.ins().ireduce(types::I8, val);
                self.builder.ins().store(flags, lo, addr, 0);
            }
            Some(strictpy_shared::TypeTag::I16) | Some(strictpy_shared::TypeTag::U16) => {
                let lo = self.builder.ins().ireduce(types::I16, val);
                self.builder.ins().store(flags, lo, addr, 0);
            }
            Some(strictpy_shared::TypeTag::I32)
            | Some(strictpy_shared::TypeTag::U32)
            | Some(strictpy_shared::TypeTag::F32) => {
                let lo = self.builder.ins().ireduce(types::I32, val);
                self.builder.ins().store(flags, lo, addr, 0);
            }
            _ => {
                self.builder.ins().store(flags, val, addr, 0);
            }
        }
    }
}

fn int_cc(op: IntCmp, signed: bool) -> IntCC {
    match (op, signed) {
        (IntCmp::Eq, _) => IntCC::Equal,
        (IntCmp::Ne, _) => IntCC::NotEqual,
        (IntCmp::Lt, true) => IntCC::SignedLessThan,
        (IntCmp::Le, true) => IntCC::SignedLessThanOrEqual,
        (IntCmp::Gt, true) => IntCC::SignedGreaterThan,
        (IntCmp::Ge, true) => IntCC::SignedGreaterThanOrEqual,
        (IntCmp::Lt, false) => IntCC::UnsignedLessThan,
        (IntCmp::Le, false) => IntCC::UnsignedLessThanOrEqual,
        (IntCmp::Gt, false) => IntCC::UnsignedGreaterThan,
        (IntCmp::Ge, false) => IntCC::UnsignedGreaterThanOrEqual,
    }
}

fn float_cc(op: FloatCmp) -> FloatCC {
    match op {
        FloatCmp::Eq => FloatCC::Equal,
        FloatCmp::Ne => FloatCC::NotEqual,
        FloatCmp::Lt => FloatCC::LessThan,
        FloatCmp::Le => FloatCC::LessThanOrEqual,
        FloatCmp::Gt => FloatCC::GreaterThan,
        FloatCmp::Ge => FloatCC::GreaterThanOrEqual,
    }
}

/// Helper the JIT'd code calls for `ConstStr` (allocates a string from
/// the module's string table).
///
/// # Safety
/// `vm` must be a valid `*mut crate::interp::Interpreter`. The index is
/// looked up in the loaded module's string table; out-of-range indices
/// return a null pointer.
#[no_mangle]
pub unsafe extern "C" fn strictpy_alloc_str_const(vm: *mut VmCtx, str_idx: u32) -> u64 {
    // SAFETY: caller contract.
    let interp = unsafe { &mut *(vm as *mut crate::interp::Interpreter) };
    // GC safepoint before minting a fresh heap string. A JIT'd loop that
    // materialises a `ConstStr` each iteration would otherwise leak every
    // copy; the caller's roots are published in the shadow window.
    interp.jit_safepoint();
    let s = match interp.shared.module.constants.get(str_idx as usize) {
        Some(crate::loader::Constant::String(sidx)) => interp
            .shared
            .module
            .strings
            .get(*sidx as usize)
            .cloned()
            .unwrap_or_default(),
        _ => match interp.shared.module.strings.get(str_idx as usize) {
            Some(s) => s.clone(),
            None => return 0,
        },
    };
    let p = interp.alloc_string(&s);
    p as u64
}

/// Trampoline the JIT'd code calls for `CALL_NATIVE`.
///
/// Re-enters the interpreter's existing `builtins::dispatch`, which knows
/// how to handle `println`, `str(x)`, allocations, etc. The vm pointer is
/// the `*mut Interpreter` the JIT entry was invoked with.
///
/// # Safety
///
/// - `vm` must be a valid `*mut crate::interp::Interpreter` for the
///   lifetime of this call.
/// - `args` must point to at least `n_args` u64 slots.
#[no_mangle]
pub unsafe extern "C" fn strictpy_native_trampoline(
    vm: *mut VmCtx,
    native_id: u32,
    args: *const u64,
    n_args: u32,
) -> u64 {
    // SAFETY: caller contract above; the interpreter cannot move while
    // we're inside this call because we're being driven from within its
    // own dispatch loop.
    let interp = unsafe { &mut *(vm as *mut crate::interp::Interpreter) };
    // GC safepoint before re-entering the interpreter's builtins, which run
    // arbitrary allocating code (`str(...)`, string ops, dict ops, …). This
    // is the path the reported `str()`-in-a-JIT-loop repro takes: the
    // temporaries are minted inside `dispatch`, so without a collect here a
    // fully-JIT'd loop grows the heap without bound. The caller's register
    // window is published in the shadow stack, so collecting is safe.
    interp.jit_safepoint();
    let slice: &[u64] = if args.is_null() || n_args == 0 {
        &[]
    } else {
        // SAFETY: caller promised the slot count.
        unsafe { std::slice::from_raw_parts(args, n_args as usize) }
    };
    match crate::builtins::dispatch(interp, native_id, slice) {
        Ok(v) => v,
        Err(_e) => {
            // JIT'd code can't propagate Rust errors; map traps to a 0
            // return for now. This loses the trap detail but matches what
            // would happen if the interpreter's match arm returned Err
            // and the caller ignored it. Future iteration: stash the
            // error on the interpreter for the next dispatch to surface.
            0
        }
    }
}

/// Public alias so `SharedVm::new_with_jit` can construct the JIT without
/// importing the no-mangle symbol directly.
#[allow(non_upper_case_globals)]
pub const native_trampoline: NativeTrampoline = strictpy_native_trampoline;

/// `Mutex`-wrapped `Jit` that the interpreter's shared state can hand out.
pub struct JitCell {
    pub inner: Mutex<Jit>,
}

impl JitCell {
    pub fn new(j: Jit) -> Self {
        Self { inner: Mutex::new(j) }
    }
    pub fn get(&self, fn_id: u32) -> Option<JitFn> {
        self.inner.lock().unwrap().get(fn_id)
    }
}

// SAFETY: JITModule contains pointers into mmap'd executable memory that
// are not themselves Send/Sync, but we keep the whole Jit behind a Mutex so
// nothing reaches across threads except the resolved function pointers
// (`unsafe extern "C" fn`, which are Send + Sync trivially).
unsafe impl Send for Jit {}
unsafe impl Sync for Jit {}
