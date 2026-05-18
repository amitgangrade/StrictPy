//! Register-based bytecode interpreter. See spec §13 / §14.
//!
//! ## Frames
//!
//! Each function call pushes a [`Frame`] onto `Interpreter.frames`. A frame
//! owns its register file (`Vec<u64>`), tracks its program counter (byte
//! offset into the *module-global* code blob), and remembers where to
//! deliver its return value in the caller's register file.
//!
//! ## Dispatch
//!
//! A plain `match` over `Opcode::from_u8`. LLVM lowers this to a jump table
//! in release mode, which is fast enough for M4. Direct threading via
//! function pointers is M6.
//!
//! ## Conservative GC
//!
//! Registers don't carry type tags at runtime, so the GC scans every slot
//! conservatively. A value that happens to alias a heap pointer keeps the
//! object alive; this is safe (mark-sweep) but wastes a little memory.
//! Precise stack maps are M6 work.

use std::collections::HashMap;
use std::fs::File as FsFile;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Mutex};

use strictpy_shared::file_format::DISCARD_REG;
use strictpy_shared::{NativeFn, Opcode, TypeTag};

use crate::error::VmError;
use crate::gc::Heap;
use crate::loader::{self, Constant, Module};
use crate::object::{
    ChannelRepr, DictRepr, FileRepr, GcKind, ListRepr, ObjectHeader, RuntimeType, StringRepr,
    ThreadRepr,
};

/// Header size in bytes — every heap object starts with this many bytes
/// before its fields. Re-exported for clarity in arithmetic.
const HDR: usize = std::mem::size_of::<ObjectHeader>();

/// Max recursion depth, per spec §14.2 ("stack overflow is a clean trap,
/// never a Rust panic").
const MAX_FRAMES: usize = 1024;

/// One activation record. See spec §14.2.
pub struct Frame {
    /// Function-table index of the currently executing function.
    pub fn_id: u32,
    /// Program counter — byte offset into the module-global code blob.
    pub pc: usize,
    /// End-of-code for the current function (PC must stay below this).
    pub end_pc: usize,
    /// Register file; one 8-byte slot per register regardless of type.
    pub registers: Vec<u64>,
    /// Register in the *caller's* frame that receives our return value.
    /// `DISCARD_REG` (0xFFFF) means "throw the value away".
    pub return_reg: u16,
}

/// Output sink. Defaults to stdout; the integration tests inject a
/// `Vec<u8>` to capture stdout for assertions.
pub trait Stdout: Send {
    fn write_str(&mut self, s: &str);
}

struct RealStdout;
impl Stdout for RealStdout {
    fn write_str(&mut self, s: &str) {
        let stdout = std::io::stdout();
        let mut h = stdout.lock();
        let _ = h.write_all(s.as_bytes());
    }
}

/// One slot in the VM's open-file table. `None` means the handle has been
/// closed (and re-using it must trap).
pub struct FileSlot {
    pub file: Option<FsFile>,
}

/// One slot in the VM's channel table. After `ChannelClose`, `tx` becomes
/// `None`; `rx` stays so pending receives can drain.
///
/// The receiver lives inside its own `Arc<Mutex<...>>` so a thread blocked
/// on `Channel.recv()` can release the outer channel-table lock while it
/// waits — otherwise a producer trying to register a new channel would
/// deadlock on the table lock while the consumer holds it.
pub struct ChannelSlot {
    pub tx: Option<SyncSender<u64>>,
    pub rx: Arc<Mutex<Receiver<u64>>>,
}

/// One slot in the VM's thread table.
pub struct ThreadSlot {
    /// `None` if not yet started; `Some(jh)` while running; `None` again
    /// after `ThreadJoin` consumes the handle.
    pub(crate) join_handle: Option<std::thread::JoinHandle<Result<(), VmError>>>,
    /// Raw closure pointer captured at `Thread.__init__`. In M6 we extract
    /// the `(fn_id, captures)` pair from this pointer at `ThreadStart` time
    /// and ship a `SendableClosure` to the spawned worker — see
    /// `start_thread` in `builtins.rs`.
    pub(crate) target_closure: u64,
    /// Set once the worker has finished. Lets `Thread.is_alive` / future
    /// status queries report something useful without `.join()`ing.
    pub(crate) finished: bool,
}

/// One slot in the VM's dict table. M5 supports string-keyed dicts; the
/// keys are owned `String`s (cheap to clone from the heap-side `StringRepr`)
/// so the dict survives even if the source string objects move.
pub struct DictSlot {
    pub data: HashMap<String, u64>,
}

/// Bundle of [`RuntimeType`]s built once at module load and shared between
/// every interpreter (parent + spawned thread workers) that runs against
/// the same module. `Arc<TypeBundle>` is cheap to clone and free of locking.
pub struct TypeBundle {
    /// Map from file `type_id` → runtime type.
    pub types: HashMap<u32, Arc<RuntimeType>>,
    pub list_ty: Arc<RuntimeType>,
    pub str_ty: Arc<RuntimeType>,
    pub closure_ty: Arc<RuntimeType>,
    pub file_ty: Arc<RuntimeType>,
    pub channel_ty: Arc<RuntimeType>,
    pub thread_ty: Arc<RuntimeType>,
    pub dict_ty: Arc<RuntimeType>,
}

/// VM-global state shared between all interpreter instances running against
/// the same loaded module. Each spawned thread builds a fresh [`Interpreter`]
/// that points at the same `SharedVm`, so the heap and resource tables are
/// observed coherently across threads.
///
/// Locking strategy:
/// * `heap` — one big mutex. Allocations are infrequent compared to register
///   moves, so a single lock is fine for M6's "credible threading" goal.
/// * `files` / `channels` / `threads` / `dicts` — one mutex per table so a
///   long-running file read doesn't block a channel send.
/// * `module` and `types` are immutable after construction and live behind
///   `Arc` with no locking.
pub struct SharedVm {
    pub module: Arc<Module>,
    pub types: Arc<TypeBundle>,
    pub heap: Arc<Mutex<Heap>>,
    pub files: Arc<Mutex<Vec<Option<FileSlot>>>>,
    pub channels: Arc<Mutex<Vec<Option<ChannelSlot>>>>,
    pub threads: Arc<Mutex<Vec<Option<ThreadSlot>>>>,
    pub dicts: Arc<Mutex<Vec<Option<DictSlot>>>>,
    /// M7: shared stdout sink so spawned worker threads write to the same
    /// destination as the parent interpreter. Without this, the capture
    /// sink installed by `run_file_capture` only sees the parent's writes;
    /// producer.spy's `println("got ...")` runs on a worker and bypassed it.
    pub stdout: Arc<Mutex<Box<dyn Stdout>>>,
    /// AOT-at-load Cranelift JIT cache. `Some` when the `jit` feature is
    /// compiled in and the module loaded. Read-only after construction.
    #[cfg(feature = "jit")]
    pub jit: Option<Arc<crate::jit::JitCell>>,
    /// M9: depth counter of currently-running JIT'd frames. The conservative
    /// GC can't see heap pointers held in CPU registers by JIT'd code, so
    /// any collection that fires while a JIT frame is on the stack risks
    /// freeing reachable objects. As a temporary measure (until M10 lands
    /// precise stack maps), we skip collection whenever this counter is
    /// non-zero. JIT'd code itself never blocks here; only `maybe_collect`
    /// reads it.
    #[cfg(feature = "jit")]
    pub in_jit: AtomicUsize,
}

impl SharedVm {
    pub fn new(module: Module) -> Arc<Self> {
        let (types_map, list_ty, str_ty, closure_ty, file_ty, channel_ty, thread_ty, dict_ty) =
            build_type_registry(&module);
        let bundle = TypeBundle {
            types: types_map,
            list_ty,
            str_ty,
            closure_ty,
            file_ty,
            channel_ty,
            thread_ty,
            dict_ty,
        };
        Arc::new(Self {
            module: Arc::new(module),
            types: Arc::new(bundle),
            heap: Arc::new(Mutex::new(Heap::new())),
            // Slot 0 is reserved so any field that reads "handle == 0" can
            // be treated as "no resource attached".
            files: Arc::new(Mutex::new(vec![None])),
            channels: Arc::new(Mutex::new(vec![None])),
            threads: Arc::new(Mutex::new(vec![None])),
            dicts: Arc::new(Mutex::new(vec![None])),
            stdout: Arc::new(Mutex::new(Box::new(RealStdout))),
            #[cfg(feature = "jit")]
            jit: None,
            #[cfg(feature = "jit")]
            in_jit: AtomicUsize::new(0),
        })
    }

    /// Build a `SharedVm` and install a Cranelift JIT cache compiled against
    /// the same module. Falls back to the no-JIT constructor on platforms
    /// where Cranelift initialisation fails.
    #[cfg(feature = "jit")]
    pub fn new_with_jit(module: Module) -> Arc<Self> {
        let (types_map, list_ty, str_ty, closure_ty, file_ty, channel_ty, thread_ty, dict_ty) =
            build_type_registry(&module);
        let bundle = TypeBundle {
            types: types_map,
            list_ty,
            str_ty,
            closure_ty,
            file_ty,
            channel_ty,
            thread_ty,
            dict_ty,
        };
        // Build the JIT, compile every supported function, and stash the
        // cache on the shared VM so `op_call_direct` can dispatch to it.
        let mut jit = crate::jit::Jit::new(crate::jit::native_trampoline);
        let (_compiled, _total) = jit.compile_module(&module);
        let jit_cell = Arc::new(crate::jit::JitCell::new(jit));
        Arc::new(Self {
            module: Arc::new(module),
            types: Arc::new(bundle),
            heap: Arc::new(Mutex::new(Heap::new())),
            files: Arc::new(Mutex::new(vec![None])),
            channels: Arc::new(Mutex::new(vec![None])),
            threads: Arc::new(Mutex::new(vec![None])),
            dicts: Arc::new(Mutex::new(vec![None])),
            stdout: Arc::new(Mutex::new(Box::new(RealStdout))),
            jit: Some(jit_cell),
            in_jit: AtomicUsize::new(0),
        })
    }
}

/// M15: one entry in the interpreter's exception-handler stack. Pushed by
/// `EnterTry`, popped by `LeaveTry` (or by exception dispatch when the
/// matching handler is entered).
pub struct HandlerFrame {
    /// Call-frame depth at the time `EnterTry` was executed (i.e.
    /// `self.frames.len()` after the enter — the function that owns the
    /// `try`). On an exception we truncate `self.frames` to this depth.
    pub frame_depth: usize,
    /// Optional finally-block byte PC. Reached after the body completes
    /// normally OR after a handler runs OR (with `pending_exception` set)
    /// when the exception isn't caught by any arm of this try.
    pub finally_pc: Option<usize>,
    /// One arm per `except` clause.
    pub arms: Vec<HandlerArm>,
}

/// One arm of an `except` clause as seen by the interpreter.
#[derive(Clone)]
pub struct HandlerArm {
    /// Type-name string to match against the raised exception's
    /// `type_name`. The catch-all `"Exception"` matches any.
    pub filter: String,
    /// Byte PC of the handler body.
    pub handler_pc: usize,
    /// Register that receives the exception value when this arm fires.
    /// `DISCARD_REG` for arms without an `as e:` binding.
    pub bind_reg: u16,
}

/// The interpreter. Owns its call stack and stdout sink; everything else
/// (module bytecode, heap, resource tables) lives in [`SharedVm`] and may
/// be observed concurrently by other interpreter instances running on
/// other OS threads.
pub struct Interpreter {
    pub(crate) shared: Arc<SharedVm>,
    pub(crate) frames: Vec<Frame>,
    /// M15: stack of exception handlers active in the current call chain.
    /// Pushed by `Opcode::EnterTry`, popped by `Opcode::LeaveTry` or by
    /// `propagate_exception` on a match. Per-thread state — never shared.
    pub(crate) handler_frames: Vec<HandlerFrame>,
    /// M15: exception currently being propagated through a `finally` block.
    /// Set when `propagate_exception` dispatches to a finally without a
    /// matching except arm. Cleared on `EnterTry` re-entry into a nested
    /// try, on `LeaveTry` after the finally pops it (and we re-raise via
    /// `Rethrow`), or when a handler matches the pending type.
    pub(crate) pending_exception: Option<(String, String)>,

    /// M19: command-line args forwarded to `sys.argv`. Per Python
    /// convention `argv[0]` is the program path (the `.spyc` invoked)
    /// and `argv[1..]` are the trailing args the CLI received. Set by
    /// `Interpreter::set_argv` before `run_main`; defaults to a
    /// single-element list containing `"<unknown>"` for tests that
    /// instantiate the interp without going through `run_file_with_args`.
    pub(crate) argv: Vec<String>,

    /// M19: cached pointer to the heap-allocated `sys.argv` list.
    /// `SysArgv` materialises it on first read and stashes the pointer
    /// here so subsequent reads return the same object identity (so
    /// `sys.argv is sys.argv` is true, and so that any user-side
    /// mutation of the list — `sys.argv.pop()` — is visible).
    pub(crate) sys_argv_cache: Option<u64>,
}

impl Interpreter {
    pub fn new(module: Module) -> Self {
        #[cfg(feature = "jit")]
        {
            let shared = SharedVm::new_with_jit(module);
            return Self::from_shared(shared);
        }
        #[cfg(not(feature = "jit"))]
        {
            let shared = SharedVm::new(module);
            Self::from_shared(shared)
        }
    }

    /// Construct an interpreter that observes the given shared VM. Used by
    /// the threading runtime to spawn worker interpreters that share the
    /// heap, resource tables, and stdout sink with the parent.
    pub fn from_shared(shared: Arc<SharedVm>) -> Self {
        Self {
            shared,
            frames: Vec::with_capacity(64),
            handler_frames: Vec::new(),
            pending_exception: None,
            argv: Vec::new(),
            sys_argv_cache: None,
        }
    }

    /// M19: set the command-line args visible to `sys.argv`. Must be
    /// called *before* `run_main`; calling later won't affect the
    /// already-materialised list cached in `sys_argv_cache`.
    pub fn set_argv(&mut self, argv: Vec<String>) {
        self.argv = argv;
        self.sys_argv_cache = None;
    }

    /// Convenience accessor: the immutable module bytecode.
    pub(crate) fn module(&self) -> &Module {
        &self.shared.module
    }

    /// Replace the default stdout sink. Used by the integration tests to
    /// capture program output. M7: this replaces the *shared* sink so that
    /// any worker threads spawned later (or already running) also write
    /// into the capture buffer.
    pub fn set_stdout(&mut self, sink: Box<dyn Stdout>) {
        *self.shared.stdout.lock().unwrap() = sink;
    }

    /// Write to the program's stdout via the shared sink. Internal helper
    /// for builtins so they don't have to know about the Arc<Mutex<...>>.
    pub(crate) fn stdout_write(&self, s: &str) {
        self.shared.stdout.lock().unwrap().write_str(s);
    }

    /// Look up `main` in the module's function table and invoke it with no
    /// arguments. Returns the integer exit code.
    pub fn run_main(&mut self) -> Result<i32, VmError> {
        let main_fn_id = self.resolve_main()?;
        let ret = self.invoke(main_fn_id, &[])?;
        Ok(ret as i32)
    }

    fn resolve_main(&self) -> Result<u32, VmError> {
        for f in &self.shared.module.functions {
            let name = self
                .shared
                .module
                .strings
                .get(f.name_idx as usize)
                .map(|s| s.as_str())
                .unwrap_or("");
            if name == "main" {
                return Ok(f.fn_id);
            }
        }
        Err(VmError::LinkError("no `main` function in module".into()))
    }

    /// Invoke `fn_id` with `args` and return its raw 64-bit result.
    pub fn invoke(&mut self, fn_id: u32, args: &[u64]) -> Result<u64, VmError> {
        // Build the entry frame.
        let frame = self.build_frame(fn_id, args, DISCARD_REG)?;
        self.frames.push(frame);
        let frame_depth = self.frames.len();
        // Run until our frame pops.
        let result = self.run_until(frame_depth - 1)?;
        Ok(result)
    }

    /// Invoke a closure by `(fn_id, captures)`. The closure's parameters
    /// (if any) follow the captures. Used by the threading runtime to
    /// dispatch a `Thread`'s target on the worker thread.
    pub fn invoke_with_captures(
        &mut self,
        fn_id: u32,
        captures: &[u64],
        args: &[u64],
    ) -> Result<u64, VmError> {
        let mut full = Vec::with_capacity(captures.len() + args.len());
        full.extend_from_slice(captures);
        full.extend_from_slice(args);
        self.invoke(fn_id, &full)
    }

    /// Drive the interpreter until the call stack shrinks below
    /// `target_depth` (i.e. the frame we entered with returns).
    fn run_until(&mut self, target_depth: usize) -> Result<u64, VmError> {
        let mut last_return: u64 = 0;
        let mut steps: u64 = 0;
        // Runaway-loop guard. Was 50M when M3 had a loop-coalescing bug; M3.5
        // fixed that, so this is now just a long-running safety net.
        let step_cap: u64 = 10_000_000_000;
        loop {
            if self.frames.len() <= target_depth {
                return Ok(last_return);
            }
            steps += 1;
            if steps >= step_cap {
                return Err(VmError::Trap(format!(
                    "execution exceeded {step_cap} instructions"
                )));
            }
            let op = self.fetch_opcode()?;
            // M15: every step is wrapped in an exception-propagation check.
            // `step` itself never sees handler frames; on `Err(UncaughtException)`
            // we route the unwind through `propagate_exception` so any pending
            // try/except handler (in this frame or an ancestor call frame)
            // gets a chance to catch.
            match self.step(op) {
                Ok(StepOutcome::Continue) => {}
                Ok(StepOutcome::Returned(v)) => {
                    last_return = v;
                }
                Err(err) => {
                    if let VmError::UncaughtException { type_name, message } = err {
                        if self.propagate_exception(&type_name, &message, target_depth)? {
                            // Caught (or a finally has it). Resume.
                            continue;
                        }
                        // Unhandled — propagate up.
                        return Err(VmError::UncaughtException { type_name, message });
                    } else {
                        return Err(err);
                    }
                }
            }
        }
    }

    /// M15: try to deliver `(type_name, message)` to the nearest matching
    /// handler frame whose owning call frame is at depth `> target_depth`.
    ///
    /// Returns `Ok(true)` if a handler (or finally) was reached and execution
    /// can resume; `Ok(false)` if no handler matched and the exception should
    /// propagate up the caller chain. `Err(_)` is only used for VM-internal
    /// trap propagation while materialising the exception value.
    fn propagate_exception(
        &mut self,
        type_name: &str,
        message: &str,
        target_depth: usize,
    ) -> Result<bool, VmError> {
        loop {
            // Snapshot the top handler frame's data so we can release the
            // borrow before mutating `self`.
            let (frame_depth, matched, finally_pc) = match self.handler_frames.last() {
                None => break,
                Some(hf) => {
                    if hf.frame_depth <= target_depth {
                        break;
                    }
                    let matched = hf
                        .arms
                        .iter()
                        .find(|arm| {
                            arm.filter == "Exception"
                                || arm.filter == type_name
                                || exception_name_alias(&arm.filter) == type_name
                        })
                        .cloned();
                    (hf.frame_depth, matched, hf.finally_pc)
                }
            };
            if let Some(arm) = matched {
                // Pop call frames above this handler's owning frame.
                while self.frames.len() > frame_depth {
                    self.frames.pop();
                }
                // Pop this handler frame plus anything pushed inside the
                // try-body (none can outlive it in a well-formed lowering,
                // but be defensive).
                while let Some(top) = self.handler_frames.last() {
                    if top.frame_depth >= frame_depth {
                        let was_match = top.frame_depth == frame_depth;
                        self.handler_frames.pop();
                        if was_match {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                // Materialise the exception value into the bind register
                // (if the handler asked for one).
                if arm.bind_reg != DISCARD_REG {
                    let exc_ptr = self.materialise_exception(type_name, message)?;
                    self.write_reg(arm.bind_reg, exc_ptr);
                }
                // Reset pending-exception state: this exception is now handled.
                self.pending_exception = None;
                // Jump to the handler.
                if let Some(f) = self.frames.last_mut() {
                    f.pc = arm.handler_pc;
                }
                return Ok(true);
            }
            // No arm matched. If the try has a finally, route there and
            // stash the pending exception so EndFinally re-raises it.
            if let Some(fpc) = finally_pc {
                while self.frames.len() > frame_depth {
                    self.frames.pop();
                }
                self.pending_exception = Some((type_name.to_string(), message.to_string()));
                // Pop this handler frame (and anything pushed inside it).
                while let Some(top) = self.handler_frames.last() {
                    if top.frame_depth >= frame_depth {
                        let was_match = top.frame_depth == frame_depth;
                        self.handler_frames.pop();
                        if was_match {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                if let Some(f) = self.frames.last_mut() {
                    f.pc = fpc;
                }
                return Ok(true);
            }
            // Neither matched nor has finally — drop this frame and keep
            // searching up.
            self.handler_frames.pop();
        }
        Ok(false)
    }

    /// M15: lazily allocate a heap exception object with `type_name` and
    /// `message` fields. Layout matches `__Exception__`-style classes
    /// registered by the resolver (two `str` fields at offsets 0 and 8 past
    /// the object header). The type pointer is left null — the GC scans
    /// only the two 8-byte slots conservatively, which is fine because both
    /// are heap pointers.
    fn materialise_exception(
        &mut self,
        type_name: &str,
        message: &str,
    ) -> Result<u64, VmError> {
        let tn_ptr = self.alloc_string(type_name) as u64;
        let msg_ptr = self.alloc_string(message) as u64;
        // Allocate a plain object: 16-byte header + 16 bytes payload.
        // We don't have a runtime type for "Exception" wired through the
        // type bundle; use a null type pointer. The GC scans `GcKind::Class`
        // as a sequence of 8-byte slots, which is what we want.
        let size = HDR + 16;
        let p = self
            .shared
            .heap
            .lock()
            .unwrap()
            .alloc(size, std::ptr::null(), GcKind::Class);
        unsafe {
            let base = p.add(HDR);
            std::ptr::write_unaligned(base as *mut u64, tn_ptr);
            std::ptr::write_unaligned(base.add(8) as *mut u64, msg_ptr);
        }
        Ok(p as u64)
    }

    fn build_frame(
        &self,
        fn_id: u32,
        args: &[u64],
        return_reg: u16,
    ) -> Result<Frame, VmError> {
        let f = self
            .shared
            .module
            .functions
            .iter()
            .find(|f| f.fn_id == fn_id)
            .ok_or_else(|| VmError::LinkError(format!("function id {fn_id} not found")))?;
        let nregs = f.num_registers as usize;
        let mut registers = vec![0u64; nregs.max(args.len())];
        for (i, &a) in args.iter().enumerate() {
            if i < registers.len() {
                registers[i] = a;
            }
        }
        let pc = f.code_offset as usize;
        let end_pc = pc + f.code_length as usize;
        Ok(Frame {
            fn_id,
            pc,
            end_pc,
            registers,
            return_reg,
        })
    }

    fn fetch_opcode(&mut self) -> Result<Opcode, VmError> {
        let frame = self
            .frames
            .last_mut()
            .ok_or_else(|| VmError::Trap("fetch with empty call stack".into()))?;
        if frame.pc >= frame.end_pc {
            return Err(VmError::Trap(format!(
                "pc {} past end of function {}",
                frame.pc, frame.fn_id
            )));
        }
        let byte = self.shared.module.code[frame.pc];
        frame.pc += 1;
        Opcode::from_u8(byte).ok_or(VmError::BadOpcode(byte))
    }

    // ── Frame accessors ─────────────────────────────────────────────────

    fn frame(&self) -> &Frame {
        self.frames.last().expect("active frame")
    }
    fn frame_mut(&mut self) -> &mut Frame {
        self.frames.last_mut().expect("active frame")
    }

    fn read_u8(&mut self) -> Result<u8, VmError> {
        let pc = self.frame().pc;
        let v = self.shared.module.code[pc];
        self.frame_mut().pc = pc + 1;
        Ok(v)
    }
    fn read_u16(&mut self) -> Result<u16, VmError> {
        let pc = self.frame().pc;
        let b0 = self.shared.module.code[pc] as u16;
        let b1 = self.shared.module.code[pc + 1] as u16;
        self.frame_mut().pc = pc + 2;
        Ok(b0 | (b1 << 8))
    }
    fn read_u32(&mut self) -> Result<u32, VmError> {
        let pc = self.frame().pc;
        let b0 = self.shared.module.code[pc] as u32;
        let b1 = self.shared.module.code[pc + 1] as u32;
        let b2 = self.shared.module.code[pc + 2] as u32;
        let b3 = self.shared.module.code[pc + 3] as u32;
        self.frame_mut().pc = pc + 4;
        Ok(b0 | (b1 << 8) | (b2 << 16) | (b3 << 24))
    }
    fn read_i32(&mut self) -> Result<i32, VmError> {
        self.read_u32().map(|v| v as i32)
    }

    fn read_reg(&self, r: u16) -> u64 {
        self.frame().registers.get(r as usize).copied().unwrap_or(0)
    }
    fn write_reg(&mut self, r: u16, v: u64) {
        if r == DISCARD_REG {
            return;
        }
        let f = self.frame_mut();
        if (r as usize) >= f.registers.len() {
            f.registers.resize(r as usize + 1, 0);
        }
        f.registers[r as usize] = v;
    }

    // ── Dispatch ────────────────────────────────────────────────────────

    fn step(&mut self, op: Opcode) -> Result<StepOutcome, VmError> {
        match op {
            Opcode::ConstI32 => self.op_const_i32()?,
            Opcode::ConstI64 => self.op_const_i64()?,
            Opcode::ConstF32 => self.op_const_f32()?,
            Opcode::ConstF64 => self.op_const_f64()?,
            Opcode::ConstStr => self.op_const_str()?,
            Opcode::ConstTrue => self.op_const_true()?,
            Opcode::ConstFalse => self.op_const_false()?,
            Opcode::ConstNone => self.op_const_none()?,
            Opcode::Move => self.op_move()?,

            Opcode::IAddI32 => self.bin_i32(|a, b| a.wrapping_add(b))?,
            Opcode::ISubI32 => self.bin_i32(|a, b| a.wrapping_sub(b))?,
            Opcode::IMulI32 => self.bin_i32(|a, b| a.wrapping_mul(b))?,
            Opcode::IDivI32 => self.bin_i32_chk(|a, b| a.wrapping_div(b))?,
            Opcode::IRemI32 => self.bin_i32_chk(|a, b| a.wrapping_rem(b))?,
            Opcode::INegI32 => self.un_i32(|a| a.wrapping_neg())?,
            Opcode::IAndI32 => self.bin_i32(|a, b| a & b)?,
            Opcode::IOrI32 => self.bin_i32(|a, b| a | b)?,
            Opcode::IXorI32 => self.bin_i32(|a, b| a ^ b)?,
            Opcode::IShlI32 => self.bin_i32(|a, b| a.wrapping_shl(b as u32))?,
            Opcode::IShrI32 => self.bin_i32(|a, b| a.wrapping_shr(b as u32))?,
            Opcode::INotI32 => self.un_i32(|a| !a)?,

            Opcode::IAddI64 => self.bin_i64(|a, b| a.wrapping_add(b))?,
            Opcode::ISubI64 => self.bin_i64(|a, b| a.wrapping_sub(b))?,
            Opcode::IMulI64 => self.bin_i64(|a, b| a.wrapping_mul(b))?,
            Opcode::IDivI64 => self.bin_i64_chk(|a, b| a.wrapping_div(b))?,
            Opcode::IRemI64 => self.bin_i64_chk(|a, b| a.wrapping_rem(b))?,
            Opcode::INegI64 => self.un_i64(|a| a.wrapping_neg())?,
            Opcode::IAndI64 => self.bin_i64(|a, b| a & b)?,
            Opcode::IOrI64 => self.bin_i64(|a, b| a | b)?,
            Opcode::IXorI64 => self.bin_i64(|a, b| a ^ b)?,
            Opcode::IShlI64 => self.bin_i64(|a, b| a.wrapping_shl(b as u32))?,
            Opcode::IShrI64 => self.bin_i64(|a, b| a.wrapping_shr(b as u32))?,
            Opcode::INotI64 => self.un_i64(|a| !a)?,

            Opcode::UAddU32 => self.bin_u32(|a, b| a.wrapping_add(b))?,
            Opcode::USubU32 => self.bin_u32(|a, b| a.wrapping_sub(b))?,
            Opcode::UMulU32 => self.bin_u32(|a, b| a.wrapping_mul(b))?,
            Opcode::UDivU32 => self.bin_u32_chk(|a, b| a.wrapping_div(b))?,
            Opcode::URemU32 => self.bin_u32_chk(|a, b| a.wrapping_rem(b))?,
            Opcode::UNegU32 => self.un_u32(|a| a.wrapping_neg())?,
            Opcode::UAndU32 => self.bin_u32(|a, b| a & b)?,
            Opcode::UOrU32 => self.bin_u32(|a, b| a | b)?,
            Opcode::UXorU32 => self.bin_u32(|a, b| a ^ b)?,
            Opcode::UShlU32 => self.bin_u32(|a, b| a.wrapping_shl(b))?,
            Opcode::UShrU32 => self.bin_u32(|a, b| a.wrapping_shr(b))?,
            Opcode::UNotU32 => self.un_u32(|a| !a)?,

            Opcode::UAddU64 => self.bin_u64(|a, b| a.wrapping_add(b))?,
            Opcode::USubU64 => self.bin_u64(|a, b| a.wrapping_sub(b))?,
            Opcode::UMulU64 => self.bin_u64(|a, b| a.wrapping_mul(b))?,
            Opcode::UDivU64 => self.bin_u64_chk(|a, b| a.wrapping_div(b))?,
            Opcode::URemU64 => self.bin_u64_chk(|a, b| a.wrapping_rem(b))?,
            Opcode::UNegU64 => self.un_u64(|a| a.wrapping_neg())?,
            Opcode::UAndU64 => self.bin_u64(|a, b| a & b)?,
            Opcode::UOrU64 => self.bin_u64(|a, b| a | b)?,
            Opcode::UXorU64 => self.bin_u64(|a, b| a ^ b)?,
            Opcode::UShlU64 => self.bin_u64(|a, b| a.wrapping_shl(b as u32))?,
            Opcode::UShrU64 => self.bin_u64(|a, b| a.wrapping_shr(b as u32))?,
            Opcode::UNotU64 => self.un_u64(|a| !a)?,

            Opcode::FAddF32 => self.bin_f32(|a, b| a + b)?,
            Opcode::FSubF32 => self.bin_f32(|a, b| a - b)?,
            Opcode::FMulF32 => self.bin_f32(|a, b| a * b)?,
            Opcode::FDivF32 => self.bin_f32(|a, b| a / b)?,
            Opcode::FNegF32 => self.un_f32(|a| -a)?,
            Opcode::FAddF64 => self.bin_f64(|a, b| a + b)?,
            Opcode::FSubF64 => self.bin_f64(|a, b| a - b)?,
            Opcode::FMulF64 => self.bin_f64(|a, b| a * b)?,
            Opcode::FDivF64 => self.bin_f64(|a, b| a / b)?,
            Opcode::FNegF64 => self.un_f64(|a| -a)?,

            Opcode::IEqI32 => self.cmp_i32(|a, b| a == b)?,
            Opcode::INeI32 => self.cmp_i32(|a, b| a != b)?,
            Opcode::ILtI32 => self.cmp_i32(|a, b| a < b)?,
            Opcode::ILeI32 => self.cmp_i32(|a, b| a <= b)?,
            Opcode::IGtI32 => self.cmp_i32(|a, b| a > b)?,
            Opcode::IGeI32 => self.cmp_i32(|a, b| a >= b)?,
            Opcode::IEqI64 => self.cmp_i64(|a, b| a == b)?,
            Opcode::INeI64 => self.cmp_i64(|a, b| a != b)?,
            Opcode::ILtI64 => self.cmp_i64(|a, b| a < b)?,
            Opcode::ILeI64 => self.cmp_i64(|a, b| a <= b)?,
            Opcode::IGtI64 => self.cmp_i64(|a, b| a > b)?,
            Opcode::IGeI64 => self.cmp_i64(|a, b| a >= b)?,
            Opcode::UEqU32 => self.cmp_u32(|a, b| a == b)?,
            Opcode::UNeU32 => self.cmp_u32(|a, b| a != b)?,
            Opcode::ULtU32 => self.cmp_u32(|a, b| a < b)?,
            Opcode::ULeU32 => self.cmp_u32(|a, b| a <= b)?,
            Opcode::UGtU32 => self.cmp_u32(|a, b| a > b)?,
            Opcode::UGeU32 => self.cmp_u32(|a, b| a >= b)?,
            Opcode::UEqU64 => self.cmp_u64(|a, b| a == b)?,
            Opcode::UNeU64 => self.cmp_u64(|a, b| a != b)?,
            Opcode::ULtU64 => self.cmp_u64(|a, b| a < b)?,
            Opcode::ULeU64 => self.cmp_u64(|a, b| a <= b)?,
            Opcode::UGtU64 => self.cmp_u64(|a, b| a > b)?,
            Opcode::UGeU64 => self.cmp_u64(|a, b| a >= b)?,
            Opcode::FEqF32 => self.cmp_f32(|a, b| a == b)?,
            Opcode::FNeF32 => self.cmp_f32(|a, b| a != b)?,
            Opcode::FLtF32 => self.cmp_f32(|a, b| a < b)?,
            Opcode::FLeF32 => self.cmp_f32(|a, b| a <= b)?,
            Opcode::FGtF32 => self.cmp_f32(|a, b| a > b)?,
            Opcode::FGeF32 => self.cmp_f32(|a, b| a >= b)?,
            Opcode::FEqF64 => self.cmp_f64(|a, b| a == b)?,
            Opcode::FNeF64 => self.cmp_f64(|a, b| a != b)?,
            Opcode::FLtF64 => self.cmp_f64(|a, b| a < b)?,
            Opcode::FLeF64 => self.cmp_f64(|a, b| a <= b)?,
            Opcode::FGtF64 => self.cmp_f64(|a, b| a > b)?,
            Opcode::FGeF64 => self.cmp_f64(|a, b| a >= b)?,
            Opcode::StrEq => self.op_str_eq()?,
            Opcode::RefEq => {
                let dst = self.read_u16()?;
                let a = self.read_u16()?;
                let b = self.read_u16()?;
                let av = self.read_reg(a);
                let bv = self.read_reg(b);
                self.write_reg(dst, (av == bv) as u64);
            }

            Opcode::I32ToI64 => self.un_i64(|a| a)?, // already sign-extended in register
            Opcode::I64ToI32 => self.un_i64(|a| a as i32 as i64)?,
            Opcode::I32ToF64 => {
                let dst = self.read_u16()?;
                let src = self.read_u16()?;
                let v = self.read_reg(src) as i32;
                self.write_reg(dst, (v as f64).to_bits());
            }
            Opcode::F64ToI32 => {
                let dst = self.read_u16()?;
                let src = self.read_u16()?;
                let v = f64::from_bits(self.read_reg(src));
                self.write_reg(dst, ((v as i32) as i64) as u64);
            }
            Opcode::F32ToF64 => {
                let dst = self.read_u16()?;
                let src = self.read_u16()?;
                let v = f32::from_bits(self.read_reg(src) as u32);
                self.write_reg(dst, (v as f64).to_bits());
            }
            Opcode::F64ToF32 => {
                let dst = self.read_u16()?;
                let src = self.read_u16()?;
                let v = f64::from_bits(self.read_reg(src));
                self.write_reg(dst, (v as f32).to_bits() as u64);
            }
            Opcode::I32ToBigInt => {
                return Err(VmError::Trap("I32_TO_BIGINT not implemented (M5)".into()));
            }
            Opcode::BoolToI32 => {
                let dst = self.read_u16()?;
                let src = self.read_u16()?;
                let v = self.read_reg(src) & 1;
                self.write_reg(dst, v);
            }

            Opcode::New => self.op_new()?,
            Opcode::NewInit => {
                return Err(VmError::Trap("NEW_INIT not implemented (M5)".into()));
            }
            Opcode::LoadField => self.op_load_field()?,
            Opcode::StoreField => self.op_store_field()?,
            Opcode::LoadVtable => {
                return Err(VmError::Trap("LOAD_VTABLE not implemented (M5)".into()));
            }
            Opcode::IsInstance => self.op_is_instance()?,
            Opcode::CastChecked => {
                return Err(VmError::Trap("CAST_CHECKED not implemented (M5)".into()));
            }
            Opcode::NullCheck => self.op_null_check()?,

            Opcode::ArrayNew => self.op_array_new()?,
            Opcode::ArrayLen => self.op_array_len()?,
            Opcode::ArrayGet => self.op_array_get()?,
            Opcode::ArraySet => self.op_array_set()?,
            Opcode::ArrayGetUnchecked => self.op_array_get()?,
            Opcode::ArraySetUnchecked => self.op_array_set()?,

            Opcode::CallDirect => return self.op_call_direct(),
            Opcode::CallVirtual => return self.op_call_virtual(),
            Opcode::CallIface => {
                return Err(VmError::Trap("CALL_IFACE not implemented (M5)".into()));
            }
            Opcode::CallIndirect => return self.op_call_indirect(),
            Opcode::TailCallDirect => {
                return Err(VmError::Trap("TAIL_CALL_DIRECT not implemented (M5)".into()));
            }
            Opcode::CallNative => return self.op_call_native(),

            Opcode::Jump => self.op_jump()?,
            Opcode::JumpIf => self.op_jump_if()?,
            Opcode::JumpIfNot => self.op_jump_if_not()?,
            Opcode::Ret => return self.op_ret(),
            Opcode::RetVoid => return self.op_ret_void(),
            Opcode::Throw => self.op_throw()?,
            Opcode::EnterTry => self.op_enter_try()?,
            Opcode::LeaveTry => self.op_leave_try()?,
            Opcode::Rethrow => self.op_end_finally()?,
            Opcode::Switch => {
                return Err(VmError::Trap("SWITCH not implemented (M5)".into()));
            }

            Opcode::ListNew => self.op_list_new()?,
            Opcode::ListPush => self.op_list_push()?,
            Opcode::ListPop => self.op_list_pop()?,
            Opcode::DictNew | Opcode::DictGet | Opcode::DictSet | Opcode::DictHas => {
                return Err(VmError::Trap(format!("{op:?} not implemented (M5)")));
            }
            Opcode::StrConcat => self.op_str_concat()?,
            Opcode::StrLen => self.op_str_len()?,
            Opcode::StrCharAt => self.op_str_char_at()?,

            Opcode::ClosureNew => self.op_closure_new()?,
            Opcode::ClosureCall => return self.op_closure_call(),

            Opcode::GcSafepoint => self.maybe_collect(),
            Opcode::GcWriteBarrier => {
                // TODO(M7 generational GC): when the heap splits into a
                // young/old region (spec §15), this opcode must record the
                // target object in a remembered set whenever an old-gen
                // object is mutated to hold a young-gen pointer. M6 keeps
                // the single-generation mark-sweep collector, so the
                // barrier is still a clean nop. The M6-B agent attempted
                // to wire two-region split + remembered set but reverted
                // (the conservative root scan in `Heap::collect` over the
                // worker-thread register file would need precise stack
                // maps to be safe under generational promotion, and that
                // is M7 work).
            }
            Opcode::DebugNop => { /* nop */ }
            Opcode::Halt => return Err(VmError::Trap("HALT executed".into())),
        }
        Ok(StepOutcome::Continue)
    }

    // ── Arithmetic helpers ──────────────────────────────────────────────

    fn bin_i32(&mut self, f: impl Fn(i32, i32) -> i32) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        let av = self.read_reg(a) as i32;
        let bv = self.read_reg(b) as i32;
        let r = f(av, bv);
        self.write_reg(dst, r as i64 as u64);
        Ok(())
    }
    fn bin_i32_chk(&mut self, f: impl Fn(i32, i32) -> i32) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        let av = self.read_reg(a) as i32;
        let bv = self.read_reg(b) as i32;
        if bv == 0 {
            return Err(VmError::UncaughtException {
                type_name: "ZeroDivisionError".into(),
                message: "division by zero".into(),
            });
        }
        let r = f(av, bv);
        self.write_reg(dst, r as i64 as u64);
        Ok(())
    }
    fn un_i32(&mut self, f: impl Fn(i32) -> i32) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let av = self.read_reg(a) as i32;
        self.write_reg(dst, f(av) as i64 as u64);
        Ok(())
    }
    fn bin_i64(&mut self, f: impl Fn(i64, i64) -> i64) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        let av = self.read_reg(a) as i64;
        let bv = self.read_reg(b) as i64;
        self.write_reg(dst, f(av, bv) as u64);
        Ok(())
    }
    fn bin_i64_chk(&mut self, f: impl Fn(i64, i64) -> i64) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        let av = self.read_reg(a) as i64;
        let bv = self.read_reg(b) as i64;
        if bv == 0 {
            return Err(VmError::UncaughtException {
                type_name: "ZeroDivisionError".into(),
                message: "division by zero".into(),
            });
        }
        self.write_reg(dst, f(av, bv) as u64);
        Ok(())
    }
    fn un_i64(&mut self, f: impl Fn(i64) -> i64) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let av = self.read_reg(a) as i64;
        self.write_reg(dst, f(av) as u64);
        Ok(())
    }
    fn bin_u32(&mut self, f: impl Fn(u32, u32) -> u32) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        let av = self.read_reg(a) as u32;
        let bv = self.read_reg(b) as u32;
        self.write_reg(dst, f(av, bv) as u64);
        Ok(())
    }
    fn bin_u32_chk(&mut self, f: impl Fn(u32, u32) -> u32) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        let av = self.read_reg(a) as u32;
        let bv = self.read_reg(b) as u32;
        if bv == 0 {
            return Err(VmError::UncaughtException {
                type_name: "ZeroDivisionError".into(),
                message: "division by zero".into(),
            });
        }
        self.write_reg(dst, f(av, bv) as u64);
        Ok(())
    }
    fn un_u32(&mut self, f: impl Fn(u32) -> u32) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        self.write_reg(dst, f(self.read_reg(a) as u32) as u64);
        Ok(())
    }
    fn bin_u64(&mut self, f: impl Fn(u64, u64) -> u64) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        let av = self.read_reg(a);
        let bv = self.read_reg(b);
        self.write_reg(dst, f(av, bv));
        Ok(())
    }
    fn bin_u64_chk(&mut self, f: impl Fn(u64, u64) -> u64) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        let av = self.read_reg(a);
        let bv = self.read_reg(b);
        if bv == 0 {
            return Err(VmError::UncaughtException {
                type_name: "ZeroDivisionError".into(),
                message: "division by zero".into(),
            });
        }
        self.write_reg(dst, f(av, bv));
        Ok(())
    }
    fn un_u64(&mut self, f: impl Fn(u64) -> u64) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        self.write_reg(dst, f(self.read_reg(a)));
        Ok(())
    }
    fn bin_f32(&mut self, f: impl Fn(f32, f32) -> f32) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        let av = f32::from_bits(self.read_reg(a) as u32);
        let bv = f32::from_bits(self.read_reg(b) as u32);
        self.write_reg(dst, f(av, bv).to_bits() as u64);
        Ok(())
    }
    fn un_f32(&mut self, f: impl Fn(f32) -> f32) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let av = f32::from_bits(self.read_reg(a) as u32);
        self.write_reg(dst, f(av).to_bits() as u64);
        Ok(())
    }
    fn bin_f64(&mut self, f: impl Fn(f64, f64) -> f64) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        let av = f64::from_bits(self.read_reg(a));
        let bv = f64::from_bits(self.read_reg(b));
        self.write_reg(dst, f(av, bv).to_bits());
        Ok(())
    }
    fn un_f64(&mut self, f: impl Fn(f64) -> f64) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let av = f64::from_bits(self.read_reg(a));
        self.write_reg(dst, f(av).to_bits());
        Ok(())
    }
    fn cmp_i32(&mut self, f: impl Fn(i32, i32) -> bool) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        let av = self.read_reg(a) as i32;
        let bv = self.read_reg(b) as i32;
        self.write_reg(dst, f(av, bv) as u64);
        Ok(())
    }
    fn cmp_i64(&mut self, f: impl Fn(i64, i64) -> bool) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        let av = self.read_reg(a) as i64;
        let bv = self.read_reg(b) as i64;
        self.write_reg(dst, f(av, bv) as u64);
        Ok(())
    }
    fn cmp_u32(&mut self, f: impl Fn(u32, u32) -> bool) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        self.write_reg(dst, f(self.read_reg(a) as u32, self.read_reg(b) as u32) as u64);
        Ok(())
    }
    fn cmp_u64(&mut self, f: impl Fn(u64, u64) -> bool) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        self.write_reg(dst, f(self.read_reg(a), self.read_reg(b)) as u64);
        Ok(())
    }
    fn cmp_f32(&mut self, f: impl Fn(f32, f32) -> bool) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        let av = f32::from_bits(self.read_reg(a) as u32);
        let bv = f32::from_bits(self.read_reg(b) as u32);
        self.write_reg(dst, f(av, bv) as u64);
        Ok(())
    }
    fn cmp_f64(&mut self, f: impl Fn(f64, f64) -> bool) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        let av = f64::from_bits(self.read_reg(a));
        let bv = f64::from_bits(self.read_reg(b));
        self.write_reg(dst, f(av, bv) as u64);
        Ok(())
    }

    // ── Constants & moves ───────────────────────────────────────────────

    fn op_const_i32(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let v = self.read_u32()? as i32;
        self.write_reg(dst, v as i64 as u64);
        Ok(())
    }
    fn op_const_i64(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let idx = self.read_u32()?;
        let v = match self.shared.module.constants.get(idx as usize) {
            Some(Constant::I64(v)) => *v as u64,
            Some(Constant::I32(v)) => *v as i64 as u64,
            Some(Constant::U64(v)) => *v,
            Some(Constant::U32(v)) => *v as u64,
            Some(other) => {
                return Err(VmError::Trap(format!("CONST_I64 expected i64, got {other:?}")))
            }
            None => return Err(VmError::Trap(format!("const idx {idx} out of range"))),
        };
        self.write_reg(dst, v);
        Ok(())
    }
    fn op_const_f32(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let bits = self.read_u32()?;
        self.write_reg(dst, bits as u64);
        Ok(())
    }
    fn op_const_f64(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let idx = self.read_u32()?;
        let bits = match self.shared.module.constants.get(idx as usize) {
            Some(Constant::F64(v)) => v.to_bits(),
            Some(Constant::F32(v)) => (*v as f64).to_bits(),
            Some(Constant::I64(v)) => (*v as f64).to_bits(),
            Some(other) => {
                return Err(VmError::Trap(format!("CONST_F64 expected f64, got {other:?}")))
            }
            None => return Err(VmError::Trap(format!("const idx {idx} out of range"))),
        };
        self.write_reg(dst, bits);
        Ok(())
    }
    fn op_const_str(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let str_idx = self.read_u32()? as usize;
        // The compiler interns each unique string in the string table; the
        // ConstStr instruction's operand is the string-table index directly
        // (matches what `codegen::emit_const` writes — `sink.intern_string`
        // returns a pool index, but the string-table index *equals* the pool
        // index by construction in M3). For correctness we look up via the
        // constant pool first if that index is in range.
        let s = if let Some(Constant::String(sidx)) = self.shared.module.constants.get(str_idx) {
            self.shared
                .module
                .strings
                .get(*sidx as usize)
                .cloned()
                .unwrap_or_default()
        } else {
            self.shared
                .module
                .strings
                .get(str_idx)
                .cloned()
                .unwrap_or_default()
        };
        let p = self.alloc_string(&s);
        self.write_reg(dst, p as u64);
        Ok(())
    }
    fn op_const_true(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        self.write_reg(dst, 1);
        Ok(())
    }
    fn op_const_false(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        self.write_reg(dst, 0);
        Ok(())
    }
    fn op_const_none(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        // M7: the `none` literal must be the same bit pattern as the
        // sentinel returned by NativeFn::ChannelTryRecv / DictGet when the
        // value is absent (see vm/src/builtins.rs::NONE_SENTINEL). Loading
        // 0 here makes `v is none` always false for empty try_recv → the
        // consumer in producer.spy never breaks out of its drain loop.
        self.write_reg(dst, crate::builtins::NONE_SENTINEL);
        Ok(())
    }
    fn op_move(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let src = self.read_u16()?;
        let v = self.read_reg(src);
        self.write_reg(dst, v);
        Ok(())
    }

    // ── Object / heap ───────────────────────────────────────────────────

    fn op_new(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let operand = self.read_u32()?;
        // M3 quirk: the compiler emits the *class_id* here, not the
        // type-table type_id. Try a direct lookup first (correct case),
        // then fall back to interpreting the operand as a class_id index
        // into the type table (M3 emits classes in class_id-sorted order,
        // which is also their type-table position).
        let ty = match self.shared.types.types.get(&operand) {
            Some(t) => t.clone(),
            None => {
                let idx = operand as usize;
                if let Some(entry) = self.shared.module.types.get(idx) {
                    self.shared
                        .types
                        .types
                        .get(&entry.type_id)
                        .cloned()
                        .ok_or_else(|| {
                            VmError::LinkError(format!(
                                "type id {} (class_id {operand}) not found",
                                entry.type_id
                            ))
                        })?
                } else {
                    return Err(VmError::LinkError(format!(
                        "NEW: class_id {operand} out of range (no type-table entry)"
                    )));
                }
            }
        };
        let size = (ty.size as usize).max(HDR);
        let ty_ptr: *const RuntimeType = Arc::as_ptr(&ty);
        let p = self.shared.heap.lock().unwrap().alloc(size, ty_ptr, GcKind::Class);
        self.write_reg(dst, p as u64);
        Ok(())
    }
    fn op_load_field(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let obj = self.read_u16()?;
        let offset = self.read_u32()? as usize;
        let tag = self.read_u8()?;
        let tt = TypeTag::from_u8(tag).ok_or_else(|| VmError::Trap(format!("bad type tag {tag}")))?;
        let p = self.read_reg(obj) as *mut u8;
        if p.is_null() {
            return Err(VmError::UncaughtException {
                type_name: "NullPointerError".into(),
                message: "load from null object".into(),
            });
        }
        // Field offsets in M3 don't include the object header — adjust.
        let addr = unsafe { p.add(HDR + offset) };
        let v = unsafe { load_typed(addr, tt) };
        self.write_reg(dst, v);
        Ok(())
    }
    fn op_store_field(&mut self) -> Result<(), VmError> {
        let obj = self.read_u16()?;
        let offset = self.read_u32()? as usize;
        let src = self.read_u16()?;
        let tag = self.read_u8()?;
        let tt = TypeTag::from_u8(tag).ok_or_else(|| VmError::Trap(format!("bad type tag {tag}")))?;
        let p = self.read_reg(obj) as *mut u8;
        if p.is_null() {
            return Err(VmError::UncaughtException {
                type_name: "NullPointerError".into(),
                message: "store to null object".into(),
            });
        }
        let val = self.read_reg(src);
        let addr = unsafe { p.add(HDR + offset) };
        unsafe { store_typed(addr, val, tt) };
        Ok(())
    }
    fn op_is_instance(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let src = self.read_u16()?;
        let target = self.read_u32()?;
        // M16: real isinstance. Sentinel target u32::MAX (legacy from the
        // pre-M16 IROp::TypeCheck path) still answers true for back-compat.
        if target == u32::MAX {
            self.write_reg(dst, 1);
            return Ok(());
        }
        let p = self.read_reg(src) as *const u8;
        if p.is_null() {
            // isinstance(none, T) is false.
            self.write_reg(dst, 0);
            return Ok(());
        }
        // Load the runtime type pointer from the header and read its
        // type_id. Then walk the base_type chain via the module's type
        // table until we find `target` or hit NO_BASE_TYPE.
        let hdr = p as *const ObjectHeader;
        let rt = unsafe { (*hdr).vtable };
        if rt.is_null() {
            self.write_reg(dst, 0);
            return Ok(());
        }
        let start_id = unsafe { (*rt).type_id };
        let mut cur = start_id;
        let mut matched = false;
        // Each user class produces one entry in `module.types`; the slot
        // index equals the type_id. Built-in pseudo-types (list/str/etc.)
        // use u32::MAX-N sentinels and never appear in the table — they
        // simply have no parent.
        const NO_BASE: u32 = strictpy_shared::file_format::NO_BASE_TYPE;
        // Cap the walk: defensive bound against malformed type tables.
        for _ in 0..32 {
            if cur == target {
                matched = true;
                break;
            }
            // Find the type-table entry whose type_id == cur.
            let entry = self
                .shared
                .module
                .types
                .iter()
                .find(|t| t.type_id == cur);
            let next = match entry {
                Some(e) => e.base_type,
                None => NO_BASE,
            };
            if next == NO_BASE || next == cur {
                break;
            }
            cur = next;
        }
        self.write_reg(dst, if matched { 1 } else { 0 });
        Ok(())
    }
    fn op_null_check(&mut self) -> Result<(), VmError> {
        let r = self.read_u16()?;
        let p = self.read_reg(r);
        if p == 0 {
            return Err(VmError::UncaughtException {
                type_name: "NullPointerError".into(),
                message: "null check failed".into(),
            });
        }
        Ok(())
    }

    // ── Lists ────────────────────────────────────────────────────────────

    fn op_list_new(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let _elem_type_id = self.read_u32()?;
        let capacity = self.read_u32()? as usize;
        let p = self.alloc_list(capacity);
        self.write_reg(dst, p as u64);
        Ok(())
    }
    fn op_list_push(&mut self) -> Result<(), VmError> {
        // LIST_PUSH lst:r16, value:r16  (3 bytes args)
        let lst_r = self.read_u16()?;
        let val_r = self.read_u16()?;
        let lst = self.read_reg(lst_r) as *mut ListRepr;
        let v = self.read_reg(val_r);
        if lst.is_null() {
            return Err(VmError::UncaughtException {
                type_name: "NullPointerError".into(),
                message: "list push on null".into(),
            });
        }
        unsafe { self.list_push(lst, v) };
        Ok(())
    }
    fn op_list_pop(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let lst_r = self.read_u16()?;
        let lst = self.read_reg(lst_r) as *mut ListRepr;
        if lst.is_null() {
            return Err(VmError::UncaughtException {
                type_name: "NullPointerError".into(),
                message: "list pop on null".into(),
            });
        }
        let v = unsafe {
            let length = (*lst).length;
            if length == 0 {
                return Err(VmError::UncaughtException {
                    type_name: "IndexError".into(),
                    message: "pop from empty list".into(),
                });
            }
            let data = (*lst).data as *const u64;
            let v = std::ptr::read_unaligned(data.add(length - 1));
            (*lst).length = length - 1;
            v
        };
        self.write_reg(dst, v);
        Ok(())
    }
    fn op_array_new(&mut self) -> Result<(), VmError> {
        // ARRAY_NEW: dst:r16, elem_type_id:u32, length:r16  (per spec §13.3.7)
        let dst = self.read_u16()?;
        let _elem_type_id = self.read_u32()?;
        let length_r = self.read_u16()?;
        let length = self.read_reg(length_r) as usize;
        let p = self.alloc_list(length);
        // Set length = capacity (filled with zeros).
        unsafe {
            (*(p as *mut ListRepr)).length = length;
        }
        self.write_reg(dst, p as u64);
        Ok(())
    }
    fn op_array_len(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let arr_r = self.read_u16()?;
        let arr = self.read_reg(arr_r) as *const ListRepr;
        if arr.is_null() {
            self.write_reg(dst, 0);
            return Ok(());
        }
        let len = unsafe { (*arr).length };
        self.write_reg(dst, len as u64);
        Ok(())
    }
    fn op_array_get(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let arr_r = self.read_u16()?;
        let idx_r = self.read_u16()?;
        let _tag = self.read_u8()?;
        let arr = self.read_reg(arr_r) as *const ListRepr;
        let idx = self.read_reg(idx_r) as i64;
        if arr.is_null() {
            return Err(VmError::UncaughtException {
                type_name: "NullPointerError".into(),
                message: "index into null".into(),
            });
        }
        let len = unsafe { (*arr).length } as i64;
        if idx < 0 || idx >= len {
            return Err(VmError::UncaughtException {
                type_name: "IndexError".into(),
                message: format!("index {idx} out of range for length {len}"),
            });
        }
        let v = unsafe {
            let data = (*arr).data as *const u64;
            std::ptr::read_unaligned(data.add(idx as usize))
        };
        self.write_reg(dst, v);
        Ok(())
    }
    fn op_array_set(&mut self) -> Result<(), VmError> {
        let arr_r = self.read_u16()?;
        let idx_r = self.read_u16()?;
        let val_r = self.read_u16()?;
        let _tag = self.read_u8()?;
        let arr = self.read_reg(arr_r) as *mut ListRepr;
        let idx = self.read_reg(idx_r) as i64;
        let val = self.read_reg(val_r);
        if arr.is_null() {
            return Err(VmError::UncaughtException {
                type_name: "NullPointerError".into(),
                message: "set on null".into(),
            });
        }
        let len = unsafe { (*arr).length } as i64;
        if idx < 0 || idx >= len {
            return Err(VmError::UncaughtException {
                type_name: "IndexError".into(),
                message: format!("index {idx} out of range for length {len}"),
            });
        }
        unsafe {
            let data = (*arr).data as *mut u64;
            std::ptr::write_unaligned(data.add(idx as usize), val);
        }
        Ok(())
    }

    // ── Strings ─────────────────────────────────────────────────────────

    fn op_str_concat(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        let ap = self.read_reg(a) as *const StringRepr;
        let bp = self.read_reg(b) as *const StringRepr;
        let s = unsafe { format!("{}{}", read_str(ap), read_str(bp)) };
        let p = self.alloc_string(&s);
        self.write_reg(dst, p as u64);
        Ok(())
    }
    fn op_str_len(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let s = self.read_u16()?;
        let p = self.read_reg(s) as *const StringRepr;
        let n = if p.is_null() { 0 } else { unsafe { (*p).length } };
        self.write_reg(dst, n as u64);
        Ok(())
    }
    fn op_str_char_at(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let s = self.read_u16()?;
        let i = self.read_u16()?;
        let p = self.read_reg(s) as *const StringRepr;
        let idx = self.read_reg(i) as usize;
        let s = unsafe { read_str(p) };
        let ch = s.chars().nth(idx).unwrap_or('\0');
        self.write_reg(dst, ch as u32 as u64);
        Ok(())
    }
    fn op_str_eq(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let a = self.read_u16()?;
        let b = self.read_u16()?;
        let ap = self.read_reg(a) as *const StringRepr;
        let bp = self.read_reg(b) as *const StringRepr;
        let eq = unsafe { read_str(ap) == read_str(bp) };
        self.write_reg(dst, eq as u64);
        Ok(())
    }

    // ── Closures ────────────────────────────────────────────────────────

    fn op_closure_new(&mut self) -> Result<(), VmError> {
        let dst = self.read_u16()?;
        let fn_id = self.read_u32()?;
        let n_cap = self.read_u8()?;
        let mut caps: Vec<u64> = Vec::with_capacity(n_cap as usize);
        for _ in 0..n_cap {
            let r = self.read_u16()?;
            caps.push(self.read_reg(r));
        }
        if fn_id == u32::MAX {
            return Err(VmError::Trap(
                "closure body not lowered yet (M3 limitation): cannot create lambda closure"
                    .into(),
            ));
        }
        let p = self.alloc_closure(fn_id, &caps);
        self.write_reg(dst, p as u64);
        Ok(())
    }
    fn op_closure_call(&mut self) -> Result<StepOutcome, VmError> {
        let dst = self.read_u16()?;
        let recv = self.read_u16()?;
        let argc = self.read_u8()?;
        let mut args: Vec<u64> = Vec::with_capacity(argc as usize);
        for _ in 0..argc {
            let r = self.read_u16()?;
            args.push(self.read_reg(r));
        }
        let cp = self.read_reg(recv) as *const crate::object::ClosureRepr;
        if cp.is_null() {
            return Err(VmError::UncaughtException {
                type_name: "NullPointerError".into(),
                message: "call on null closure".into(),
            });
        }
        let (fn_id, n_cap) = unsafe { ((*cp).fn_id, (*cp).capture_n) };
        // Prepend captures.
        let mut full: Vec<u64> = Vec::with_capacity(n_cap as usize + args.len());
        for i in 0..n_cap as usize {
            let cap_ptr = unsafe {
                (cp as *const u8)
                    .add(std::mem::size_of::<crate::object::ClosureRepr>())
                    as *const u64
            };
            let v = unsafe { std::ptr::read_unaligned(cap_ptr.add(i)) };
            full.push(v);
        }
        full.extend_from_slice(&args);
        self.do_call(fn_id, &full, dst)?;
        Ok(StepOutcome::Continue)
    }

    // ── Calls ───────────────────────────────────────────────────────────

    fn op_call_direct(&mut self) -> Result<StepOutcome, VmError> {
        let dst = self.read_u16()?;
        let fn_id = self.read_u32()?;
        let argc = self.read_u8()?;
        let mut args: Vec<u64> = Vec::with_capacity(argc as usize);
        for _ in 0..argc {
            let r = self.read_u16()?;
            args.push(self.read_reg(r));
        }
        // Fast path: if the JIT compiled this function, dispatch directly
        // to the native entry point. The JIT shares the interpreter's
        // shared-VM heap and stdout via the `*mut VmCtx` we hand it.
        #[cfg(feature = "jit")]
        if let Some(jf) = self.shared.jit.as_ref().and_then(|j| j.get(fn_id)) {
            let vm_ptr = self as *mut Self as *mut crate::jit::VmCtx;
            let args_ptr: *const u64 = if args.is_empty() {
                std::ptr::null()
            } else {
                args.as_ptr()
            };
            // SAFETY: `jf` is a function pointer produced by Cranelift that
            // matches the unified `(vm, args) -> u64` ABI. `vm_ptr` is a
            // valid `*mut Interpreter` live for the duration of the call;
            // `args` is owned by this stack frame and stays alive across
            // the call.
            //
            // M9: bracket the call with `in_jit` ++ / -- so any allocation
            // inside the JIT'd code (or its `rt_*` helpers) suppresses GC.
            // The counter handles re-entry: a JIT'd function that calls
            // another JIT'd function bumps the depth and only the outermost
            // exit re-enables collection.
            self.shared.in_jit.fetch_add(1, Ordering::AcqRel);
            let ret = unsafe { jf(vm_ptr, args_ptr) };
            self.shared.in_jit.fetch_sub(1, Ordering::AcqRel);
            self.write_reg(dst, ret);
            return Ok(StepOutcome::Continue);
        }
        self.do_call(fn_id, &args, dst)?;
        Ok(StepOutcome::Continue)
    }
    fn op_call_virtual(&mut self) -> Result<StepOutcome, VmError> {
        let dst = self.read_u16()?;
        let recv_r = self.read_u16()?;
        let slot = self.read_u16()? as usize;
        let argc = self.read_u8()?;
        let mut args: Vec<u64> = Vec::with_capacity(1 + argc as usize);
        let recv = self.read_reg(recv_r);
        args.push(recv);
        for _ in 0..argc {
            let r = self.read_u16()?;
            args.push(self.read_reg(r));
        }
        if recv == 0 {
            return Err(VmError::UncaughtException {
                type_name: "NullPointerError".into(),
                message: "virtual call on null receiver".into(),
            });
        }
        let ty_ptr = unsafe { (*(recv as *const ObjectHeader)).vtable };
        if ty_ptr.is_null() {
            return Err(VmError::Trap("receiver has null vtable".into()));
        }
        let fn_id = unsafe {
            let ty = &*ty_ptr;
            ty.vtable
                .get(slot)
                .copied()
                .ok_or_else(|| VmError::Trap(format!("vtable slot {slot} out of range")))?
        };
        if fn_id == u32::MAX {
            return Err(VmError::Trap(format!("unresolved vtable slot {slot}")));
        }
        self.do_call(fn_id, &args, dst)?;
        Ok(StepOutcome::Continue)
    }
    fn op_call_indirect(&mut self) -> Result<StepOutcome, VmError> {
        let dst = self.read_u16()?;
        let recv_r = self.read_u16()?;
        let argc = self.read_u8()?;
        let mut args: Vec<u64> = Vec::with_capacity(argc as usize);
        for _ in 0..argc {
            let r = self.read_u16()?;
            args.push(self.read_reg(r));
        }
        // Receiver is expected to be a closure pointer.
        let cp = self.read_reg(recv_r) as *const crate::object::ClosureRepr;
        if cp.is_null() {
            return Err(VmError::UncaughtException {
                type_name: "NullPointerError".into(),
                message: "indirect call on null".into(),
            });
        }
        let (fn_id, n_cap) = unsafe { ((*cp).fn_id, (*cp).capture_n) };
        let mut full: Vec<u64> = Vec::with_capacity(n_cap as usize + args.len());
        for i in 0..n_cap as usize {
            let cap_ptr = unsafe {
                (cp as *const u8)
                    .add(std::mem::size_of::<crate::object::ClosureRepr>())
                    as *const u64
            };
            let v = unsafe { std::ptr::read_unaligned(cap_ptr.add(i)) };
            full.push(v);
        }
        full.extend_from_slice(&args);
        self.do_call(fn_id, &full, dst)?;
        Ok(StepOutcome::Continue)
    }
    fn op_call_native(&mut self) -> Result<StepOutcome, VmError> {
        let dst = self.read_u16()?;
        let native_id = self.read_u32()?;
        let argc = self.read_u8()?;
        let mut args: Vec<u64> = Vec::with_capacity(argc as usize);
        for _ in 0..argc {
            let r = self.read_u16()?;
            args.push(self.read_reg(r));
        }
        let result = crate::builtins::dispatch(self, native_id, &args)?;
        self.write_reg(dst, result);
        Ok(StepOutcome::Continue)
    }

    fn do_call(&mut self, fn_id: u32, args: &[u64], return_reg: u16) -> Result<(), VmError> {
        if self.frames.len() >= MAX_FRAMES {
            return Err(VmError::Trap("stack overflow (max frame depth)".into()));
        }
        let frame = self.build_frame(fn_id, args, return_reg)?;
        self.frames.push(frame);
        Ok(())
    }

    // ── Control flow ────────────────────────────────────────────────────

    fn op_jump(&mut self) -> Result<(), VmError> {
        let offset = self.read_i32()?;
        let f = self.frame_mut();
        // Per spec §13.3.9: offset is relative to the byte after the offset
        // slot (which is now `f.pc`).
        f.pc = (f.pc as i64 + offset as i64) as usize;
        Ok(())
    }
    fn op_jump_if(&mut self) -> Result<(), VmError> {
        let cond = self.read_u16()?;
        let offset = self.read_i32()?;
        let v = self.read_reg(cond);
        let f = self.frame_mut();
        if v != 0 {
            f.pc = (f.pc as i64 + offset as i64) as usize;
        }
        Ok(())
    }
    fn op_jump_if_not(&mut self) -> Result<(), VmError> {
        let cond = self.read_u16()?;
        let offset = self.read_i32()?;
        let v = self.read_reg(cond);
        let f = self.frame_mut();
        if v == 0 {
            f.pc = (f.pc as i64 + offset as i64) as usize;
        }
        Ok(())
    }
    // ── M15 try/except ─────────────────────────────────────────────────

    /// `Throw r:u16` — interpret `r` as a heap pointer to an exception
    /// object whose `type_name` field is at offset 0 and `message` field is
    /// at offset 8 (past the object header). Read both, return a
    /// `VmError::UncaughtException` so the `run_until` loop's catch wrapper
    /// can deliver it to a handler.
    fn op_throw(&mut self) -> Result<(), VmError> {
        let r = self.read_u16()?;
        let p = self.read_reg(r) as *mut u8;
        let (type_name, message) = if p.is_null() {
            ("Exception".to_string(), String::new())
        } else {
            unsafe {
                let base = p.add(HDR);
                let tn_p = std::ptr::read_unaligned(base as *const u64) as *const StringRepr;
                let msg_p = std::ptr::read_unaligned(base.add(8) as *const u64) as *const StringRepr;
                let tn = if tn_p.is_null() { String::new() } else { read_str(tn_p).to_string() };
                let msg = if msg_p.is_null() { String::new() } else { read_str(msg_p).to_string() };
                (if tn.is_empty() { "Exception".to_string() } else { tn }, msg)
            }
        };
        Err(VmError::UncaughtException { type_name, message })
    }

    /// `EnterTry` — push a handler frame onto `self.handler_frames`. Layout
    /// of the operand stream is documented in
    /// `compiler/src/codegen.rs::emit_op` (search for `IROp::TryEnter`).
    fn op_enter_try(&mut self) -> Result<(), VmError> {
        let finally_rel = self.read_i32()?;
        // The relative offset is `target_block_offset - (pos + 4)`. Compute
        // the absolute pc; -1 sentinel = no finally.
        let cur_pc_after = self.frame().pc; // already past the i32 we just read.
        let finally_pc = if finally_rel == -1 {
            None
        } else {
            Some((cur_pc_after as i64 + finally_rel as i64) as usize)
        };
        let n_arms = self.read_u8()? as usize;
        let mut arms: Vec<HandlerArm> = Vec::with_capacity(n_arms);
        for _ in 0..n_arms {
            let filter_idx = self.read_u32()?;
            let handler_rel = self.read_i32()?;
            let handler_after = self.frame().pc;
            let handler_pc = (handler_after as i64 + handler_rel as i64) as usize;
            let bind_reg = self.read_u16()?;
            let filter = self
                .shared
                .module
                .strings
                .get(filter_idx as usize)
                .cloned()
                .unwrap_or_else(|| "Exception".to_string());
            arms.push(HandlerArm { filter, handler_pc, bind_reg });
        }
        let frame_depth = self.frames.len();
        self.handler_frames.push(HandlerFrame {
            frame_depth,
            finally_pc,
            arms,
        });
        Ok(())
    }

    /// `LeaveTry` — pop the topmost handler frame. The current function's
    /// `try` body completed normally; no handler should fire after this.
    /// If a pending exception is stored (we're at the end of a try-body
    /// that immediately preceded a finally), do NOT clear it — the finally
    /// block's `EndFinally` will pick it up.
    fn op_leave_try(&mut self) -> Result<(), VmError> {
        // Pop only the matching frame for this call frame's `try`. There
        // should be one per body in a well-formed lowering.
        if let Some(top) = self.handler_frames.last() {
            if top.frame_depth == self.frames.len() {
                self.handler_frames.pop();
            }
        }
        Ok(())
    }

    /// `Rethrow` (alias for END_FINALLY) — if `self.pending_exception` is
    /// `Some`, re-raise it so any outer handler can catch. Otherwise
    /// no-op. Emitted at the bottom of a `finally` block.
    fn op_end_finally(&mut self) -> Result<(), VmError> {
        if let Some((tn, msg)) = self.pending_exception.take() {
            return Err(VmError::UncaughtException {
                type_name: tn,
                message: msg,
            });
        }
        Ok(())
    }

    fn op_ret(&mut self) -> Result<StepOutcome, VmError> {
        let src = self.read_u16()?;
        let v = self.read_reg(src);
        let popped = self.frames.pop().expect("active frame");
        // M15: drop any handler frames that were active in the returning
        // function. They reference a frame depth that no longer exists.
        let new_depth = self.frames.len();
        while let Some(top) = self.handler_frames.last() {
            if top.frame_depth > new_depth {
                self.handler_frames.pop();
            } else {
                break;
            }
        }
        if let Some(caller) = self.frames.last_mut() {
            if popped.return_reg != DISCARD_REG {
                let r = popped.return_reg as usize;
                if r >= caller.registers.len() {
                    caller.registers.resize(r + 1, 0);
                }
                caller.registers[r] = v;
            }
        }
        Ok(StepOutcome::Returned(v))
    }
    fn op_ret_void(&mut self) -> Result<StepOutcome, VmError> {
        let _ = self.frames.pop();
        let new_depth = self.frames.len();
        while let Some(top) = self.handler_frames.last() {
            if top.frame_depth > new_depth {
                self.handler_frames.pop();
            } else {
                break;
            }
        }
        Ok(StepOutcome::Returned(0))
    }

    // ── Heap helpers ────────────────────────────────────────────────────

    pub(crate) fn alloc_string(&mut self, s: &str) -> *mut StringRepr {
        let size = std::mem::size_of::<StringRepr>();
        let ty: *const RuntimeType = Arc::as_ptr(&self.shared.types.str_ty);
        let p = self.shared.heap.lock().unwrap().alloc(size, ty, GcKind::Str);
        let bytes = s.as_bytes();
        let data = self.shared.heap.lock().unwrap().alloc_raw(bytes.len(), 1);
        if !data.is_null() && !bytes.is_empty() {
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
            }
        }
        let sp = p as *mut StringRepr;
        unsafe {
            (*sp).length = s.chars().count();
            (*sp).byte_len = bytes.len();
            (*sp).data = data;
            (*sp).flags = if s.is_ascii() { 0b10 } else { 0 };
        }
        sp
    }

    pub(crate) fn alloc_list(&mut self, capacity: usize) -> *mut ListRepr {
        let size = std::mem::size_of::<ListRepr>();
        let ty: *const RuntimeType = Arc::as_ptr(&self.shared.types.list_ty);
        let p = self.shared.heap.lock().unwrap().alloc(size, ty, GcKind::List);
        let cap = capacity.max(4);
        let data = self.shared.heap.lock().unwrap().alloc_raw(cap * 8, 8);
        let lp = p as *mut ListRepr;
        unsafe {
            (*lp).length = 0;
            (*lp).capacity = cap;
            (*lp).data = data;
        }
        lp
    }

    pub fn alloc_closure(&mut self, fn_id: u32, caps: &[u64]) -> *mut crate::object::ClosureRepr {
        let base = std::mem::size_of::<crate::object::ClosureRepr>();
        let size = base + caps.len() * 8;
        let ty: *const RuntimeType = Arc::as_ptr(&self.shared.types.closure_ty);
        let p = self.shared.heap.lock().unwrap().alloc(size, ty, GcKind::Closure);
        let cp = p as *mut crate::object::ClosureRepr;
        unsafe {
            (*cp).fn_id = fn_id;
            (*cp).capture_n = caps.len() as u32;
            let cap_ptr = (cp as *mut u8).add(base) as *mut u64;
            for (i, &v) in caps.iter().enumerate() {
                std::ptr::write_unaligned(cap_ptr.add(i), v);
            }
        }
        cp
    }

    /// Allocate a `FileRepr` wrapping the given OS file handle. `mode_flags`
    /// uses bit 0 = read, bit 1 = write.
    pub(crate) fn alloc_file(&mut self, file: FsFile, mode_flags: u32) -> *mut FileRepr {
        let handle = {
            let mut files = self.shared.files.lock().unwrap();
            let h = files.len() as u64;
            files.push(Some(FileSlot { file: Some(file) }));
            h
        };
        let size = std::mem::size_of::<FileRepr>();
        let ty: *const RuntimeType = Arc::as_ptr(&self.shared.types.file_ty);
        let p = self.shared.heap.lock().unwrap().alloc(size, ty, GcKind::File);
        let fp = p as *mut FileRepr;
        // SAFETY: `p` was just allocated with `size` bytes for a FileRepr.
        unsafe {
            (*fp).handle = handle;
            (*fp).mode_flags = mode_flags;
            (*fp)._padding = 0;
        }
        fp
    }

    /// Allocate a `ChannelRepr` wrapping the given (tx, rx) pair.
    pub(crate) fn alloc_channel(
        &mut self,
        tx: SyncSender<u64>,
        rx: Receiver<u64>,
    ) -> *mut ChannelRepr {
        let handle = {
            let mut chans = self.shared.channels.lock().unwrap();
            let h = chans.len() as u64;
            chans.push(Some(ChannelSlot {
                tx: Some(tx),
                rx: Arc::new(Mutex::new(rx)),
            }));
            h
        };
        let size = std::mem::size_of::<ChannelRepr>();
        let ty: *const RuntimeType = Arc::as_ptr(&self.shared.types.channel_ty);
        let p = self.shared.heap.lock().unwrap().alloc(size, ty, GcKind::Channel);
        let cp = p as *mut ChannelRepr;
        // SAFETY: just allocated.
        unsafe {
            (*cp).handle = handle;
        }
        cp
    }

    /// Allocate a `ThreadRepr` recording the closure pointer it will invoke.
    pub(crate) fn alloc_thread(&mut self, closure_ptr: u64) -> *mut ThreadRepr {
        let handle = {
            let mut threads = self.shared.threads.lock().unwrap();
            let h = threads.len() as u64;
            threads.push(Some(ThreadSlot {
                join_handle: None,
                target_closure: closure_ptr,
                finished: false,
            }));
            h
        };
        let size = std::mem::size_of::<ThreadRepr>();
        let ty: *const RuntimeType = Arc::as_ptr(&self.shared.types.thread_ty);
        let p = self.shared.heap.lock().unwrap().alloc(size, ty, GcKind::Thread);
        let tp = p as *mut ThreadRepr;
        // SAFETY: just allocated.
        unsafe {
            (*tp).handle = handle;
            (*tp).started = 0;
            (*tp).finished = 0;
        }
        tp
    }

    /// M20a: allocate an N-slot tuple-shaped object on the heap.  Used by
    /// native functions whose declared return type is `Ty::Tuple(_)` (e.g.
    /// `path.splitext`).  The IR-side tuple lowering uses
    /// `Alloc(class_id)` because it knows the registered tuple type id at
    /// compile time; native code can't see that table, so we allocate with
    /// a null type pointer and `GcKind::Class` — the GC scans the payload
    /// as a uniform sequence of 8-byte slots, which is exactly what every
    /// tuple needs.  Returns the raw object pointer.
    pub(crate) fn alloc_tuple_obj(&mut self, slots: &[u64]) -> *mut u8 {
        let size = HDR + slots.len() * 8;
        let p = self
            .shared
            .heap
            .lock()
            .unwrap()
            .alloc(size, std::ptr::null(), GcKind::Class);
        unsafe {
            let base = p.add(HDR);
            for (i, v) in slots.iter().enumerate() {
                std::ptr::write_unaligned(base.add(i * 8) as *mut u64, *v);
            }
        }
        p
    }

    /// Allocate a `DictRepr` backed by an empty side-table HashMap.
    pub(crate) fn alloc_dict(&mut self, key_kind: u32) -> *mut DictRepr {
        let handle = {
            let mut dicts = self.shared.dicts.lock().unwrap();
            let h = dicts.len() as u64;
            dicts.push(Some(DictSlot {
                data: HashMap::new(),
            }));
            h
        };
        let size = std::mem::size_of::<DictRepr>();
        let ty: *const RuntimeType = Arc::as_ptr(&self.shared.types.dict_ty);
        let p = self.shared.heap.lock().unwrap().alloc(size, ty, GcKind::Dict);
        let dp = p as *mut DictRepr;
        // SAFETY: just allocated.
        unsafe {
            (*dp).handle = handle;
            (*dp).key_kind = key_kind;
            (*dp)._padding = 0;
        }
        dp
    }

    pub(crate) unsafe fn list_push(&mut self, lst: *mut ListRepr, val: u64) {
        let length = (*lst).length;
        let capacity = (*lst).capacity;
        if length >= capacity {
            let new_cap = (capacity.max(4)) * 2;
            let new_data = self.shared.heap.lock().unwrap().realloc_raw(
                (*lst).data,
                capacity * 8,
                new_cap * 8,
                8,
                length * 8,
            );
            (*lst).data = new_data;
            (*lst).capacity = new_cap;
        }
        let data = (*lst).data as *mut u64;
        std::ptr::write_unaligned(data.add(length), val);
        (*lst).length = length + 1;
    }

    /// True if `addr` (treated as a candidate pointer) names an object
    /// whose runtime type is the string type. Used by the StrFromAny
    /// heuristic in `builtins.rs`.
    pub(crate) fn is_known_string_ptr(&self, addr: u64) -> bool {
        if addr == 0 || addr < 0x1000 {
            return false;
        }
        // SAFETY: we deliberately treat `addr` as a maybe-pointer; reading
        // the vtable field is undefined if it doesn't point at a header.
        // Mitigation: we sanity-check alignment and only deref if the
        // resulting vtable equals our known str_ty pointer.
        if addr & 0x7 != 0 {
            return false;
        }
        let our_str_ty: *const RuntimeType = Arc::as_ptr(&self.shared.types.str_ty);
        unsafe {
            let hdr = addr as *const ObjectHeader;
            // Read the vtable field. If the address isn't really an object
            // header this is UB; in practice on a 64-bit platform it'll
            // either be a non-matching pointer or segfault before us.
            // Compare with our singleton str_ty pointer.
            std::ptr::read_unaligned(hdr as *const usize) == our_str_ty as usize
        }
    }

    fn maybe_collect(&mut self) {
        // M9: skip GC entirely while any JIT'd frame is running. JIT'd
        // code holds heap pointers in CPU registers that the conservative
        // scanner can't see, so collecting now could free reachable
        // objects. M10 will replace this with precise stack maps.
        #[cfg(feature = "jit")]
        if self.shared.in_jit.load(Ordering::Acquire) > 0 {
            return;
        }
        let mut heap = self.shared.heap.lock().unwrap();
        if !heap.should_collect() {
            return;
        }
        // Build root windows from every active frame's register file.
        let mut roots: Vec<&[u64]> = self
            .frames
            .iter()
            .map(|f| f.registers.as_slice())
            .collect();
        // Also root every value living in a dict side-table — those values
        // might be StringRepr pointers or other heap objects with no other
        // root. (Channel payloads do not need rooting: every payload was
        // produced by a thread that already had it in registers, and once
        // sent it lives only inside the OS-level mpsc queue which the GC
        // does not touch. Dropping the channel does not free its messages
        // via the GC.)
        //
        // M6 caveat: we deliberately do NOT scan other threads' register
        // files here. A spawned worker that's busy in bytecode may be
        // holding the only reference to a heap object, and a concurrent
        // collection on the parent thread would free it. The escape valve
        // for now is that the GC also can't run concurrently — the worker
        // would also be trying to acquire `shared.heap` to allocate, so
        // the worst case is that a collection misses a worker-only object
        // and that object survives until the worker exits and the parent's
        // next safepoint covers it (worker registers drop with the
        // interpreter). For producer.spy this is fine because the
        // workers' working sets are tiny i32s and channel-pointer
        // captures which are also held in main's registers.
        let dict_roots: Vec<Vec<u64>> = self
            .shared
            .dicts
            .lock()
            .unwrap()
            .iter()
            .filter_map(|s| s.as_ref())
            .map(|slot| slot.data.values().copied().collect())
            .collect();
        for v in &dict_roots {
            roots.push(v.as_slice());
        }
        heap.collect(&roots);
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Helpers
// ─────────────────────────────────────────────────────────────────────────

enum StepOutcome {
    Continue,
    /// A `Ret`/`RetVoid` just popped a frame; payload is the raw return value.
    Returned(u64),
}

/// Read a typed value from `addr` using the byte width implied by `tag`.
///
/// # Safety
/// `addr` must point at least `tag.size_bytes()` bytes of valid memory.
unsafe fn load_typed(addr: *const u8, tag: TypeTag) -> u64 {
    match tag {
        TypeTag::I8 => std::ptr::read_unaligned(addr as *const i8) as i64 as u64,
        TypeTag::U8 | TypeTag::Bool => std::ptr::read_unaligned(addr) as u64,
        TypeTag::I16 => std::ptr::read_unaligned(addr as *const i16) as i64 as u64,
        TypeTag::U16 => std::ptr::read_unaligned(addr as *const u16) as u64,
        TypeTag::I32 => std::ptr::read_unaligned(addr as *const i32) as i64 as u64,
        TypeTag::U32 | TypeTag::F32 => std::ptr::read_unaligned(addr as *const u32) as u64,
        TypeTag::I64 | TypeTag::U64 | TypeTag::F64 | TypeTag::Ref => {
            std::ptr::read_unaligned(addr as *const u64)
        }
    }
}

/// Store a typed value to `addr` truncating to the byte width implied by `tag`.
///
/// # Safety
/// `addr` must point at least `tag.size_bytes()` bytes of valid memory.
unsafe fn store_typed(addr: *mut u8, val: u64, tag: TypeTag) {
    match tag {
        TypeTag::I8 | TypeTag::U8 | TypeTag::Bool => {
            std::ptr::write_unaligned(addr, val as u8);
        }
        TypeTag::I16 | TypeTag::U16 => {
            std::ptr::write_unaligned(addr as *mut u16, val as u16);
        }
        TypeTag::I32 | TypeTag::U32 | TypeTag::F32 => {
            std::ptr::write_unaligned(addr as *mut u32, val as u32);
        }
        TypeTag::I64 | TypeTag::U64 | TypeTag::F64 | TypeTag::Ref => {
            std::ptr::write_unaligned(addr as *mut u64, val);
        }
    }
}

/// Read a UTF-8 string from a [`StringRepr`] pointer. Returns "" if the
/// pointer is null.
///
/// # Safety
/// `p` must point at a valid `StringRepr` allocated on our heap (or be null).
pub(crate) unsafe fn read_str(p: *const StringRepr) -> String {
    if p.is_null() {
        return String::new();
    }
    let data = (*p).data;
    let len = (*p).byte_len;
    if data.is_null() || len == 0 {
        return String::new();
    }
    let slice = std::slice::from_raw_parts(data, len);
    match std::str::from_utf8(slice) {
        Ok(s) => s.to_string(),
        Err(_) => String::from_utf8_lossy(slice).into_owned(),
    }
}

fn build_type_registry(
    module: &Module,
) -> (
    HashMap<u32, Arc<RuntimeType>>,
    Arc<RuntimeType>,
    Arc<RuntimeType>,
    Arc<RuntimeType>,
    Arc<RuntimeType>,
    Arc<RuntimeType>,
    Arc<RuntimeType>,
    Arc<RuntimeType>,
) {
    let mut map = HashMap::new();
    for t in &module.types {
        let name = module
            .strings
            .get(t.name_idx as usize)
            .cloned()
            .unwrap_or_else(|| format!("type#{}", t.type_id));
        let mut field_offsets = Vec::with_capacity(t.fields.len());
        let mut field_is_ref = Vec::with_capacity(t.fields.len());
        for f in &t.fields {
            field_offsets.push(f.offset);
            // Conservatively assume any field could be a ref. M6 will tighten.
            field_is_ref.push(true);
        }
        let rt = Arc::new(RuntimeType {
            type_id: t.type_id,
            name,
            size: t.size,
            field_offsets,
            field_is_ref,
            vtable: t.vtable.clone(),
            kind: t.kind,
            gc_kind: GcKind::Class,
        });
        map.insert(t.type_id, rt);
    }
    let list_ty = Arc::new(RuntimeType {
        type_id: u32::MAX - 1,
        name: "list".into(),
        size: std::mem::size_of::<ListRepr>() as u32,
        field_offsets: vec![],
        field_is_ref: vec![],
        vtable: vec![],
        kind: 1,
        gc_kind: GcKind::List,
    });
    let str_ty = Arc::new(RuntimeType {
        type_id: u32::MAX - 2,
        name: "str".into(),
        size: std::mem::size_of::<StringRepr>() as u32,
        field_offsets: vec![],
        field_is_ref: vec![],
        vtable: vec![],
        kind: 1,
        gc_kind: GcKind::Str,
    });
    let closure_ty = Arc::new(RuntimeType {
        type_id: u32::MAX - 3,
        name: "closure".into(),
        size: std::mem::size_of::<crate::object::ClosureRepr>() as u32,
        field_offsets: vec![],
        field_is_ref: vec![],
        vtable: vec![],
        kind: 1,
        gc_kind: GcKind::Closure,
    });
    let file_ty = Arc::new(RuntimeType {
        type_id: u32::MAX - 4,
        name: "File".into(),
        size: std::mem::size_of::<FileRepr>() as u32,
        field_offsets: vec![],
        field_is_ref: vec![],
        vtable: vec![],
        kind: 1,
        gc_kind: GcKind::File,
    });
    let channel_ty = Arc::new(RuntimeType {
        type_id: u32::MAX - 5,
        name: "Channel".into(),
        size: std::mem::size_of::<ChannelRepr>() as u32,
        field_offsets: vec![],
        field_is_ref: vec![],
        vtable: vec![],
        kind: 1,
        gc_kind: GcKind::Channel,
    });
    let thread_ty = Arc::new(RuntimeType {
        type_id: u32::MAX - 6,
        name: "Thread".into(),
        size: std::mem::size_of::<ThreadRepr>() as u32,
        field_offsets: vec![],
        field_is_ref: vec![],
        vtable: vec![],
        kind: 1,
        gc_kind: GcKind::Thread,
    });
    let dict_ty = Arc::new(RuntimeType {
        type_id: u32::MAX - 7,
        name: "dict".into(),
        size: std::mem::size_of::<DictRepr>() as u32,
        field_offsets: vec![],
        field_is_ref: vec![],
        vtable: vec![],
        kind: 1,
        gc_kind: GcKind::Dict,
    });
    (map, list_ty, str_ty, closure_ty, file_ty, channel_ty, thread_ty, dict_ty)
}

// Keep `NativeFn` / `loader::Module` imports live for now even if unused.
#[allow(dead_code)]
fn _keep_alive(_: NativeFn, _: &loader::Module) {}

/// Map a handler-arm filter name to its canonical alias for exception-match
/// purposes. M18 fix (BUG-036): runtime emits `ZeroDivisionError` as the
/// canonical (Python-compatible) name, but historical code and the M15
/// implementation accepted `DivisionByZeroError` as a synonym. Keep both
/// working: if a user writes `except DivisionByZeroError`, this returns
/// `"ZeroDivisionError"`, which then matches the emitted type_name.
fn exception_name_alias(filter: &str) -> &str {
    match filter {
        "DivisionByZeroError" => "ZeroDivisionError",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::{Function, Header, Module};
    use strictpy_shared::file_format::{HEADER_SIZE, VERSION_MAJOR, VERSION_MINOR};

    fn make_module(code: Vec<u8>, num_registers: u16) -> Module {
        Module {
            header: Header {
                version_major: VERSION_MAJOR,
                version_minor: VERSION_MINOR,
                flags: 0,
                const_pool_offset: 0,
                type_table_offset: 0,
                function_table_offset: 0,
                code_offset: HEADER_SIZE as u32,
                string_table_offset: 0,
            },
            constants: vec![],
            types: vec![],
            functions: vec![Function {
                fn_id: 0,
                name_idx: 0,
                type_id: 0,
                code_offset: 0,
                code_length: code.len() as u32,
                num_params: 0,
                num_locals: 0,
                num_registers,
                flags: 0,
                exception_table: vec![],
                debug_info_offset: 0,
            }],
            code,
            strings: vec!["main".into()],
        }
    }

    #[test]
    fn const_i32_then_ret_returns_value() {
        // CONST_I32 dst=0 val=42 ; RET src=0
        let mut code = vec![Opcode::ConstI32 as u8];
        code.extend_from_slice(&0u16.to_le_bytes());
        code.extend_from_slice(&42u32.to_le_bytes());
        code.push(Opcode::Ret as u8);
        code.extend_from_slice(&0u16.to_le_bytes());

        let m = make_module(code, 1);
        let mut i = Interpreter::new(m);
        let r = i.invoke(0, &[]).unwrap();
        assert_eq!(r, 42);
    }

    #[test]
    fn add_two_i32_args() {
        // IADD_I32 dst=2 a=0 b=1 ; RET src=2
        let mut code = vec![Opcode::IAddI32 as u8];
        code.extend_from_slice(&2u16.to_le_bytes());
        code.extend_from_slice(&0u16.to_le_bytes());
        code.extend_from_slice(&1u16.to_le_bytes());
        code.push(Opcode::Ret as u8);
        code.extend_from_slice(&2u16.to_le_bytes());

        let m = make_module(code, 3);
        let mut i = Interpreter::new(m);
        let r = i.invoke(0, &[3, 4]).unwrap();
        assert_eq!(r as i32, 7);
    }
}
