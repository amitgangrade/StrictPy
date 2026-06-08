//! Dead code elimination. Spec §10.7 pass #2.
//!
//! Drops SSA values that have no use AND no side effects. Side-effecting
//! ops (stores, calls, allocs, checks, terminators' operands) are
//! considered "live" regardless of use count.

use std::collections::HashSet;

use crate::ir::{IRModule, IROp, Terminator, ValueId, ValueKind};

pub fn run(ir: &mut IRModule) -> bool {
    let mut changed = false;
    for f in &mut ir.functions {
        // Collect every used ValueId across all blocks.
        let mut used: HashSet<u32> = HashSet::new();
        for b in &f.blocks {
            for v in &b.values {
                match &v.kind {
                    ValueKind::Op { args, .. } => {
                        for a in args {
                            used.insert(a.0);
                        }
                    }
                    ValueKind::Phi { incoming } => {
                        for (_, a) in incoming {
                            used.insert(a.0);
                        }
                    }
                    _ => {}
                }
            }
            match &b.terminator {
                Terminator::CondBranch { cond: ValueId(c), .. } => {
                    used.insert(*c);
                }
                Terminator::Ret { value: Some(ValueId(v)) } => {
                    used.insert(*v);
                }
                Terminator::Throw { exc: ValueId(e) } => {
                    used.insert(*e);
                }
                _ => {}
            }
        }
        // Now drop values that are unused AND pure.
        for b in &mut f.blocks {
            let before = b.values.len();
            b.values.retain(|v| {
                if used.contains(&v.id) {
                    return true;
                }
                if has_side_effect(&v.kind) {
                    return true;
                }
                false
            });
            if b.values.len() != before {
                changed = true;
            }
        }
    }
    changed
}

fn has_side_effect(k: &ValueKind) -> bool {
    match k {
        ValueKind::Op { op, .. } => matches!(
            op,
            IROp::Alloc { .. }
                | IROp::Store { .. }
                | IROp::ArraySet
                | IROp::ListPush
                | IROp::WriteLocal { .. }
                | IROp::DirectCall { .. }
                | IROp::VirtualCall { .. }
                | IROp::IfaceCall { .. }
                | IROp::IndirectCall
                | IROp::NativeCall { .. }
                | IROp::BoundsCheck
                | IROp::NullCheck
                | IROp::TypeCheck
                | IROp::ClosureNew { .. }
                | IROp::ClosureCall
                | IROp::TryEnter { .. }
                | IROp::TryLeave
                | IROp::EndFinally
                // M62b: generators. `Yield` and `GenNext` mutate the
                // suspended-frame state machine and must never be eliminated
                // even when their result is unused; `MakeGen` allocates.
                | IROp::MakeGen { .. }
                | IROp::Yield
                | IROp::GenNext { .. }
        ),
        // Constants/params/phis are side-effect-free but might still be live;
        // the `used` set already handles that. If a phi has no uses we let DCE
        // drop it.
        _ => false,
    }
}
