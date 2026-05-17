//! TEMPORARY (M3.5 agent diagnostic): run dot.spy and capture the crash
//! point. This test is ignored because dot.spy itself currently crashes
//! the VM due to an M3.5 codegen bug; running this diagnostic would also
//! crash. Re-enable with `--ignored` once dot.spy is unbroken.

use std::fs;
use std::path::PathBuf;

use strictpy_compiler::compile_source;
use strictpy_shared::Opcode;
use strictpy_vm::{loader, run_file_capture};

#[test]
#[ignore = "diagnostic for the known M3.5 dot.spy crash; ignore until that is fixed"]
fn debug_dot_capture() {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.push("examples");
    p.push("dot.spy");
    let src = fs::read_to_string(&p).expect("read");
    let bytes = compile_source(p.display().to_string(), &src).unwrap();
    let mut out_path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    out_path.push("debug_dot.spyc");
    fs::write(&out_path, &bytes).unwrap();

    let m = loader::load(&bytes).unwrap();
    for f in &m.functions {
        eprintln!("=== fn id={} code_off={} len={}", f.fn_id, f.code_offset, f.code_length);
        let mut pc = f.code_offset as usize;
        let end = (f.code_offset + f.code_length) as usize;
        while pc < end {
            let op_byte = m.code[pc];
            let op = Opcode::from_u8(op_byte);
            eprint!("    [{:05}] {:02x}", pc, op_byte);
            pc += 1;
            match op {
                Some(o) => {
                    eprint!(" {:?}", o);
                    let size = op_operand_size(o, &m.code, pc);
                    let bytes = &m.code[pc..pc + size];
                    eprint!(" {:02x?}", bytes);
                    pc += size;
                }
                None => eprint!(" <unknown>"),
            }
            eprintln!();
        }
    }

    eprintln!("compiled size = {}", bytes.len());
    match run_file_capture(&out_path) {
        Ok((code, out)) => eprintln!("OK exit={} stdout={:?}", code, out),
        Err(e) => eprintln!("ERR: {:?}", e),
    }
}

fn op_operand_size(op: Opcode, code: &[u8], pc: usize) -> usize {
    use Opcode::*;
    match op {
        ConstI32 | ConstStr | ConstF32 => 2 + 4,
        ConstI64 | ConstF64 => 2 + 4,
        ConstTrue | ConstFalse | ConstNone => 2,
        Move => 2 + 2,
        IAddI32 | ISubI32 | IMulI32 | IDivI32 | IRemI32
        | IAndI32 | IOrI32 | IXorI32 | IShlI32 | IShrI32
        | IAddI64 | ISubI64 | IMulI64 | IDivI64 | IRemI64
        | IAndI64 | IOrI64 | IXorI64 | IShlI64 | IShrI64
        | UAddU32 | USubU32 | UMulU32 | UDivU32 | UAddU64 | USubU64 | UMulU64 | UDivU64
        | FAddF64 | FSubF64 | FMulF64 | FDivF64
        | FAddF32 | FSubF32 | FMulF32 | FDivF32
        | IEqI32 | INeI32 | ILtI32 | ILeI32 | IGtI32 | IGeI32
        | IEqI64 | INeI64 | ILtI64 | ILeI64 | IGtI64 | IGeI64
        | FEqF64 | FNeF64 | FLtF64 | FLeF64 | FGtF64 | FGeF64
        | StrEq | RefEq => 2 + 2 + 2,
        INegI32 | INegI64 | FNegF64 | FNegF32 | INotI32 | INotI64 | BoolToI32
        | I32ToI64 | I64ToI32 | I32ToF64 | F64ToI32 | F32ToF64 | F64ToF32 => 2 + 2,
        New => 2 + 4,
        LoadField => 2 + 2 + 4 + 1,
        StoreField => 2 + 4 + 2 + 1,
        ListNew => 2 + 4 + 4,
        ListPush => 2 + 2,
        ListPop => 2 + 2,
        ArrayGet => 2 + 2 + 2 + 1,
        ArraySet => 2 + 2 + 2 + 1,
        ArrayLen => 2 + 2,
        CallDirect | TailCallDirect => {
            // dst:u16, fn_id:u32, argc:u8, args:u16*argc
            let argc = code[pc + 2 + 4] as usize;
            2 + 4 + 1 + 2 * argc
        }
        CallVirtual | CallIface => {
            // dst:u16, recv:u16, slot:u16 (or u32+u16), argc:u8, args:u16*argc
            let header = match op { CallIface => 2 + 2 + 4 + 2, _ => 2 + 2 + 2 };
            let argc = code[pc + header] as usize;
            header + 1 + 2 * argc
        }
        CallIndirect => {
            // dst:u16, recv:u16, argc:u8, args:u16*argc
            let argc = code[pc + 2 + 2] as usize;
            2 + 2 + 1 + 2 * argc
        }
        CallNative => {
            // dst:u16, native_id:u32, argc:u8, args:u16*argc
            let argc = code[pc + 2 + 4] as usize;
            2 + 4 + 1 + 2 * argc
        }
        Jump => 4,
        JumpIf | JumpIfNot => 2 + 4,
        Ret => 2,
        RetVoid | Halt => 0,
        Throw => 2,
        NullCheck => 2,
        ClosureNew => {
            let n = code[pc + 2 + 4] as usize;
            2 + 4 + 1 + 2 * n
        }
        ClosureCall => {
            let argc = code[pc + 2 + 2] as usize;
            2 + 2 + 1 + 2 * argc
        }
        IsInstance => 2 + 2 + 4,
        _ => 0,
    }
}
