//! Constant folding & propagation. Spec §10.7 pass #1.
//!
//! Replaces a binary `Op` whose both operands are `Const(...)` with a fresh
//! `Const(...)` of the folded value. Idempotent and self-contained — does
//! NOT remove the now-dead source values; that's [`super::dead_code`]'s job.
//!
//! Only the simple integer cases are folded for M3 — float folding is left
//! to M6 once we have a clearer story on NaN / rounding semantics
//! (spec §7.3 leaves these implementation-defined).

use crate::ir::{IRConst, IRModule, IROp, Value, ValueId, ValueKind};

/// Returns `true` iff at least one value in `ir` was rewritten.
pub fn run(ir: &mut IRModule) -> bool {
    let mut changed = false;
    for f in &mut ir.functions {
        // Snapshot id → const for quick lookup.
        for bidx in 0..f.blocks.len() {
            // Read pass to compute folded replacements.
            let block_vals: Vec<Value> = f.blocks[bidx].values.clone();
            // Build a local cheap lookup: ValueId -> Const, sourced from
            // earlier values in the same block (SSA + linear scan is enough
            // for the toy programs).
            let lookup = |arg: ValueId| -> Option<IRConst> {
                for v in &block_vals {
                    if v.id == arg.0 {
                        if let ValueKind::Const(c) = &v.kind {
                            return Some(c.clone());
                        }
                    }
                }
                None
            };
            for v in f.blocks[bidx].values.iter_mut() {
                let (op, args) = match &v.kind {
                    ValueKind::Op { op, args } if args.len() == 2 => (op.clone(), args.clone()),
                    _ => continue,
                };
                let a = match lookup(args[0]) {
                    Some(c) => c,
                    None => continue,
                };
                let b = match lookup(args[1]) {
                    Some(c) => c,
                    None => continue,
                };
                if let Some(folded) = fold_binop(&op, &a, &b) {
                    v.kind = ValueKind::Const(folded);
                    changed = true;
                }
            }
        }
    }
    changed
}

fn fold_binop(op: &IROp, a: &IRConst, b: &IRConst) -> Option<IRConst> {
    match (op, a, b) {
        (IROp::IAdd, IRConst::I32(x), IRConst::I32(y)) => Some(IRConst::I32(x.wrapping_add(*y))),
        (IROp::IAdd, IRConst::I64(x), IRConst::I64(y)) => Some(IRConst::I64(x.wrapping_add(*y))),
        (IROp::ISub, IRConst::I32(x), IRConst::I32(y)) => Some(IRConst::I32(x.wrapping_sub(*y))),
        (IROp::ISub, IRConst::I64(x), IRConst::I64(y)) => Some(IRConst::I64(x.wrapping_sub(*y))),
        (IROp::IMul, IRConst::I32(x), IRConst::I32(y)) => Some(IRConst::I32(x.wrapping_mul(*y))),
        (IROp::IMul, IRConst::I64(x), IRConst::I64(y)) => Some(IRConst::I64(x.wrapping_mul(*y))),
        (IROp::IAnd, IRConst::I32(x), IRConst::I32(y)) => Some(IRConst::I32(x & y)),
        (IROp::IOr,  IRConst::I32(x), IRConst::I32(y)) => Some(IRConst::I32(x | y)),
        (IROp::IXor, IRConst::I32(x), IRConst::I32(y)) => Some(IRConst::I32(x ^ y)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BasicBlock, BlockId, FuncId, IRFunction, Terminator, Value};
    use crate::types::{PrimTy, Ty};

    fn one_block_fn(values: Vec<Value>) -> IRFunction {
        IRFunction {
            id: FuncId(0),
            name: "f".into(),
            params: vec![],
            ret: Ty::Primitive(PrimTy::I32),
            blocks: vec![BasicBlock {
                id: BlockId(0),
                values,
                terminator: Terminator::Ret { value: None },
            }],
        }
    }

    #[test]
    fn folds_iadd_i32() {
        let mut ir = IRModule::default();
        let v0 = Value { id: 0, ty: Ty::Primitive(PrimTy::I32), kind: ValueKind::Const(IRConst::I32(1)) };
        let v1 = Value { id: 1, ty: Ty::Primitive(PrimTy::I32), kind: ValueKind::Const(IRConst::I32(2)) };
        let v2 = Value {
            id: 2,
            ty: Ty::Primitive(PrimTy::I32),
            kind: ValueKind::Op { op: IROp::IAdd, args: vec![ValueId(0), ValueId(1)] },
        };
        ir.functions.push(one_block_fn(vec![v0, v1, v2]));
        assert!(run(&mut ir));
        let folded = &ir.functions[0].blocks[0].values[2];
        assert!(matches!(&folded.kind, ValueKind::Const(IRConst::I32(3))));
    }

    #[test]
    fn no_change_when_no_constants() {
        let mut ir = IRModule::default();
        let v0 = Value { id: 0, ty: Ty::Primitive(PrimTy::I32), kind: ValueKind::Param { idx: 0 } };
        let v1 = Value { id: 1, ty: Ty::Primitive(PrimTy::I32), kind: ValueKind::Param { idx: 1 } };
        let v2 = Value {
            id: 2,
            ty: Ty::Primitive(PrimTy::I32),
            kind: ValueKind::Op { op: IROp::IAdd, args: vec![ValueId(0), ValueId(1)] },
        };
        ir.functions.push(one_block_fn(vec![v0, v1, v2]));
        assert!(!run(&mut ir));
    }
}
