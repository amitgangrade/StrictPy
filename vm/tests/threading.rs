//! End-to-end threading smoke test.
//!
//! Constructs a minimal module by hand (no compiler involvement), wires up
//! a worker function that calls `channel.send(value)` once, spawns a
//! `Thread` against it, and joins. We then drain the channel from the main
//! interpreter and assert we got the value the worker sent.
//!
//! This validates the M6 wiring end-to-end:
//!   * `Arc<SharedVm>` is observed coherently across threads.
//!   * `ChannelSlot` (with its `Arc<Mutex<Receiver<u64>>>`) survives being
//!     sent on by a worker.
//!   * `ThreadStart` extracts (fn_id, captures) from a real `ClosureRepr`.
//!   * `ThreadJoin` blocks on the worker's `JoinHandle`.

use strictpy_shared::file_format::{HEADER_SIZE, VERSION_MAJOR, VERSION_MINOR};
use strictpy_shared::{NativeFn, Opcode};

use strictpy_vm::builtins::dispatch;
use strictpy_vm::interp::Interpreter;
use strictpy_vm::loader::{Function, Header, Module};

/// Build a one-function module whose function `worker(channel_handle: u64)`
/// performs `ChannelSend(channel, 42)` and returns. The `ChannelRepr` pointer
/// is passed in register 0 (the closure capture).
fn build_worker_module() -> Module {
    // Layout:
    //   reg 0 = channel pointer  (capture)
    //   reg 1 = value to send    (42)
    //   reg 2 = discard
    //
    // CONST_I32 dst=1 val=42                   [op u8][dst u16][val u32]
    // CALL_NATIVE dst=2 id=ChannelSend argc=2 r=0 r=1
    //                                          [op u8][dst u16][id u32][argc u8][r u16]*2
    // RET_VOID                                 [op u8]
    let mut code: Vec<u8> = Vec::new();

    code.push(Opcode::ConstI32 as u8);
    code.extend_from_slice(&1u16.to_le_bytes());
    code.extend_from_slice(&42u32.to_le_bytes());

    code.push(Opcode::CallNative as u8);
    code.extend_from_slice(&2u16.to_le_bytes()); // dst
    code.extend_from_slice(&(NativeFn::ChannelSend as u32).to_le_bytes());
    code.push(2u8); // argc
    code.extend_from_slice(&0u16.to_le_bytes()); // arg 0 = channel pointer
    code.extend_from_slice(&1u16.to_le_bytes()); // arg 1 = value

    code.push(Opcode::RetVoid as u8);

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
            fn_id: 1,
            name_idx: 0,
            type_id: 0,
            code_offset: 0,
            code_length: code.len() as u32,
            num_params: 1, // takes the channel pointer
            num_locals: 0,
            num_registers: 3,
            flags: 0,
            exception_table: vec![],
            debug_info_offset: 0,
        }],
        code,
        strings: vec!["worker".into()],
    }
}

#[test]
fn spawned_thread_can_send_on_shared_channel() {
    let module = build_worker_module();
    let mut interp = Interpreter::new(module);

    // Create a bounded channel; main holds the receiver.
    let chan_ptr =
        dispatch(&mut interp, NativeFn::ChannelNew as u32, &[1u64]).expect("ChannelNew");
    assert!(chan_ptr != 0, "channel pointer must be non-zero");

    // Build a closure over fn_id=1 (worker) that captures the channel.
    let closure_ptr = interp.alloc_closure(1, &[chan_ptr]) as u64;
    assert!(closure_ptr != 0);

    // Wrap in a Thread, start it, join it.
    let thread_ptr = dispatch(&mut interp, NativeFn::ThreadNew as u32, &[closure_ptr])
        .expect("ThreadNew");
    dispatch(&mut interp, NativeFn::ThreadStart as u32, &[thread_ptr])
        .expect("ThreadStart");
    dispatch(&mut interp, NativeFn::ThreadJoin as u32, &[thread_ptr])
        .expect("ThreadJoin");

    // The worker should have pushed 42 onto the channel.
    let got = dispatch(&mut interp, NativeFn::ChannelRecv as u32, &[chan_ptr])
        .expect("ChannelRecv");
    assert_eq!(got, 42, "expected worker to send 42, got {got}");
}

#[test]
fn many_workers_share_the_same_channel() {
    // Spawn N workers, each sending one value. Main joins all of them and
    // then drains. Exercises that the shared channel table and the shared
    // heap survive concurrent allocators.
    const N: u32 = 8;

    let module = build_worker_module();
    let mut interp = Interpreter::new(module);

    let chan_ptr =
        dispatch(&mut interp, NativeFn::ChannelNew as u32, &[N as u64]).expect("ChannelNew");

    let mut threads: Vec<u64> = Vec::with_capacity(N as usize);
    for _ in 0..N {
        let closure_ptr = interp.alloc_closure(1, &[chan_ptr]) as u64;
        let tp = dispatch(&mut interp, NativeFn::ThreadNew as u32, &[closure_ptr])
            .expect("ThreadNew");
        dispatch(&mut interp, NativeFn::ThreadStart as u32, &[tp]).expect("ThreadStart");
        threads.push(tp);
    }
    for tp in &threads {
        dispatch(&mut interp, NativeFn::ThreadJoin as u32, &[*tp]).expect("ThreadJoin");
    }

    let mut total: u64 = 0;
    for _ in 0..N {
        total += dispatch(&mut interp, NativeFn::ChannelRecv as u32, &[chan_ptr])
            .expect("ChannelRecv");
    }
    assert_eq!(total, 42 * N as u64);
}
