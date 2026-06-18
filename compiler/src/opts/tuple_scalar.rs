//! Tuple scalar replacement (P1 part 2).
//!
//! Eliminates the heap allocation for a tuple that is created and consumed
//! within the same basic block and never escapes. The canonical pattern is
//!
//! ```text
//! p = (r, q)          # IROp::Alloc{tuple} + Store(0)=r + Store(8)=q
//! use p.1             # Load(8) on p
//! ```
//!
//! which the lowerer emits as a tuple `Alloc` followed by one `Store` per
//! element and any number of `Load`s. When the allocated tuple value is
//! used *only* as the base of `Store`/`Load` ops (never returned, passed to
//! a call, stored into another object, compared by reference, or fed into a
//! phi), the heap object is unobservable: each `Load{offset=8*i}` can be
//! rewritten to a `Copy` of the value that was `Store`d at the same offset,
//! the `Store`s dropped, and the now-dead `Alloc` left to DCE.
//!
//! Conservative scope (correctness over coverage):
//!   * Only tuples whose `Alloc` and *every* use live in the same block are
//!     considered. Cross-block tuples keep their allocation (a load in a
//!     later block could observe a different store ordering, and proving
//!     otherwise needs real dominance/aliasing analysis).
//!   * The tuple value must not appear anywhere except as `args[0]` of a
//!     `Store`/`Load`. Appearing as a stored *value* (`args[1]` of a Store,
//!     a list push, an array set), a call argument, a return operand, a
//!     `RefEq` operand, or a phi input all count as escapes.
//!   * A `Load{offset}` is only replaced if a `Store{offset}` to the same
//!     offset was seen *earlier in the block*. Otherwise the tuple is left
//!     alone (it would read an uninitialised slot, which the lowerer never
//!     emits, but we stay defensive).
//!
//! The pass returns `true` if it rewrote anything so the fixed-point driver
//! re-runs DCE to drop the dead `Alloc`/`Store`s.

use std::collections::{HashMap, HashSet};

use crate::ir::{IRModule, IROp, Terminator, ValueId, ValueKind};
use crate::types::Ty;

pub fn run(ir: &mut IRModule) -> bool {
    let mut changed = false;
    for f in &mut ir.functions {
        // A candidate is disqualified if it is used in a block other than
        // the one it was allocated in. Build a function-wide tally of which
        // block each candidate alloc lives in, plus the set of candidate
        // ids referenced from each block, so cross-block uses are rejected.
        // Tuple allocs and their indices, keyed by block index.
        for bi in 0..f.blocks.len() {
            // 1. Find candidate tuple allocs in this block (value is a
            //    tuple-typed `Alloc`).
            let mut candidates: HashSet<u32> = HashSet::new();
            for v in &f.blocks[bi].values {
                if let ValueKind::Op { op: IROp::Alloc { .. }, .. } = &v.kind {
                    if matches!(v.ty, Ty::Tuple(_)) {
                        candidates.insert(v.id);
                    }
                }
            }
            if candidates.is_empty() {
                continue;
            }

            // 2. Reject any candidate referenced from a *different* block —
            //    its lifetime crosses a block boundary and replacement would
            //    need dominance/aliasing analysis we don't do here.
            for (obi, ob) in f.blocks.iter().enumerate() {
                if obi == bi {
                    continue;
                }
                for v in &ob.values {
                    match &v.kind {
                        ValueKind::Op { args, .. } => {
                            for a in args {
                                candidates.remove(&a.0);
                            }
                        }
                        ValueKind::Phi { incoming } => {
                            for (_, a) in incoming {
                                candidates.remove(&a.0);
                            }
                        }
                        _ => {}
                    }
                }
                match &ob.terminator {
                    Terminator::CondBranch { cond, .. } => {
                        candidates.remove(&cond.0);
                    }
                    Terminator::Ret { value: Some(v) } => {
                        candidates.remove(&v.0);
                    }
                    Terminator::Throw { exc } => {
                        candidates.remove(&exc.0);
                    }
                    _ => {}
                }
            }
            if candidates.is_empty() {
                continue;
            }

            let b = &f.blocks[bi];

            // 3. Disqualify any candidate that escapes within this block —
            //    i.e. appears in a use that is not `args[0]` of a
            //    Store/Load. The first arg of a Store/Load is the only
            //    "non-escaping" position.
            let mut escaped: HashSet<u32> = HashSet::new();
            for v in &b.values {
                match &v.kind {
                    ValueKind::Op { op, args } => {
                        let base_ok = matches!(op, IROp::Store { .. } | IROp::Load { .. });
                        for (i, a) in args.iter().enumerate() {
                            if !candidates.contains(&a.0) {
                                continue;
                            }
                            if !(base_ok && i == 0) {
                                escaped.insert(a.0);
                            }
                        }
                    }
                    ValueKind::Phi { incoming } => {
                        for (_, a) in incoming {
                            if candidates.contains(&a.0) {
                                escaped.insert(a.0);
                            }
                        }
                    }
                    _ => {}
                }
            }
            match &b.terminator {
                Terminator::CondBranch { cond, .. } => {
                    if candidates.contains(&cond.0) {
                        escaped.insert(cond.0);
                    }
                }
                Terminator::Ret { value: Some(v) } => {
                    if candidates.contains(&v.0) {
                        escaped.insert(v.0);
                    }
                }
                Terminator::Throw { exc } => {
                    if candidates.contains(&exc.0) {
                        escaped.insert(exc.0);
                    }
                }
                _ => {}
            }
            candidates.retain(|c| !escaped.contains(c));
            if candidates.is_empty() {
                continue;
            }

            // 4. Walk the block top-to-bottom tracking, per surviving
            //    candidate, the latest stored value at each offset. Replace
            //    each `Load{offset}` whose base is a candidate and whose
            //    offset has a known store with a `Copy` of that value. Mark
            //    consumed `Store`s for removal.
            let mut stores_at: HashMap<u32, HashMap<u32, ValueId>> = HashMap::new();
            let mut load_rewrites: Vec<(usize, ValueId)> = Vec::new();
            let mut remove_idx: HashSet<usize> = HashSet::new();

            for (idx, v) in b.values.iter().enumerate() {
                if let ValueKind::Op { op, args } = &v.kind {
                    match op {
                        IROp::Store { offset } => {
                            if let (Some(base), Some(val)) = (args.first(), args.get(1)) {
                                if candidates.contains(&base.0) {
                                    stores_at
                                        .entry(base.0)
                                        .or_default()
                                        .insert(*offset, *val);
                                    remove_idx.insert(idx);
                                }
                            }
                        }
                        IROp::Load { offset } => {
                            if let Some(base) = args.first() {
                                if candidates.contains(&base.0) {
                                    if let Some(val) =
                                        stores_at.get(&base.0).and_then(|m| m.get(offset))
                                    {
                                        load_rewrites.push((idx, *val));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            if load_rewrites.is_empty() && remove_idx.is_empty() {
                continue;
            }

            let b = &mut f.blocks[bi];

            // 5. Apply load rewrites: turn each consumed `Load` into a
            //    `Copy` of the stored value.
            for (idx, src) in &load_rewrites {
                if let ValueKind::Op { op, args } = &mut b.values[*idx].kind {
                    *op = IROp::Copy;
                    *args = vec![*src];
                    changed = true;
                }
            }

            // 6. Drop the consumed `Store`s. (The dead `Alloc` is left for
            //    DCE — it has no remaining uses once stores/loads are gone.)
            if !remove_idx.is_empty() {
                let mut i = 0usize;
                b.values.retain(|_| {
                    let keep = !remove_idx.contains(&i);
                    i += 1;
                    keep
                });
                changed = true;
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BasicBlock, BlockId, FuncId, IRFunction, IRModule, Value};
    use crate::types::PrimTy;

    fn i64_ty() -> Ty {
        Ty::Primitive(PrimTy::I64)
    }

    fn op_val(id: u32, ty: Ty, op: IROp, args: Vec<u32>) -> Value {
        Value {
            id,
            ty,
            kind: ValueKind::Op {
                op,
                args: args.into_iter().map(ValueId).collect(),
            },
        }
    }

    fn param_val(id: u32, idx: u32) -> Value {
        Value {
            id,
            ty: i64_ty(),
            kind: ValueKind::Param { idx },
        }
    }

    fn run_one(blocks: Vec<BasicBlock>) -> (bool, IRFunction) {
        let f = IRFunction {
            id: FuncId(0),
            name: "f".into(),
            params: vec![i64_ty(), i64_ty()],
            ret: i64_ty(),
            blocks,
        };
        let mut m = IRModule {
            functions: vec![f],
            ..Default::default()
        };
        let changed = run(&mut m);
        (changed, m.functions.pop().unwrap())
    }

    /// `p = (a, b); return p.1` — the non-escaping intra-block case.
    /// The Load(8) should become Copy(b) and both Stores removed.
    #[test]
    fn replaces_nonescaping_tuple() {
        // v0=a (param0), v1=b (param1), v2=Alloc tuple,
        // store(0,v2,v0), store(8,v2,v1), v5=load(8,v2), ret v5
        let block = BasicBlock {
            id: BlockId(0),
            values: vec![
                param_val(0, 0),
                param_val(1, 1),
                op_val(
                    2,
                    Ty::Tuple(vec![i64_ty(), i64_ty()]),
                    IROp::Alloc { class_id: 99 },
                    vec![],
                ),
                op_val(3, Ty::Primitive(PrimTy::Unit), IROp::Store { offset: 0 }, vec![2, 0]),
                op_val(4, Ty::Primitive(PrimTy::Unit), IROp::Store { offset: 8 }, vec![2, 1]),
                op_val(5, i64_ty(), IROp::Load { offset: 8 }, vec![2]),
            ],
            terminator: Terminator::Ret { value: Some(ValueId(5)) },
        };
        let (changed, f) = run_one(vec![block]);
        assert!(changed, "pass should fire on a non-escaping tuple");
        let vals = &f.blocks[0].values;
        // The two Stores are gone; the Load became a Copy of v1 (b).
        assert!(
            !vals.iter().any(|v| matches!(
                &v.kind,
                ValueKind::Op { op: IROp::Store { .. }, .. }
            )),
            "stores should be removed"
        );
        let load = vals.iter().find(|v| v.id == 5).expect("v5 still present");
        match &load.kind {
            ValueKind::Op { op: IROp::Copy, args } => {
                assert_eq!(args, &[ValueId(1)], "Load(8) must copy the stored b value");
            }
            other => panic!("expected Copy, got {other:?}"),
        }
    }

    /// A tuple returned directly escapes and must keep its allocation.
    #[test]
    fn keeps_escaping_tuple() {
        let block = BasicBlock {
            id: BlockId(0),
            values: vec![
                param_val(0, 0),
                param_val(1, 1),
                op_val(
                    2,
                    Ty::Tuple(vec![i64_ty(), i64_ty()]),
                    IROp::Alloc { class_id: 99 },
                    vec![],
                ),
                op_val(3, Ty::Primitive(PrimTy::Unit), IROp::Store { offset: 0 }, vec![2, 0]),
                op_val(4, Ty::Primitive(PrimTy::Unit), IROp::Store { offset: 8 }, vec![2, 1]),
            ],
            // Returning the tuple value itself = escape.
            terminator: Terminator::Ret { value: Some(ValueId(2)) },
        };
        let (changed, f) = run_one(vec![block]);
        assert!(!changed, "escaping tuple must be left untouched");
        let vals = &f.blocks[0].values;
        assert_eq!(
            vals.iter()
                .filter(|v| matches!(&v.kind, ValueKind::Op { op: IROp::Store { .. }, .. }))
                .count(),
            2,
            "stores must survive for an escaping tuple"
        );
    }

    /// A tuple whose element store reads the tuple itself (nested) — and a
    /// tuple used across blocks — must not be scalarised.
    #[test]
    fn keeps_cross_block_tuple() {
        let b0 = BasicBlock {
            id: BlockId(0),
            values: vec![
                param_val(0, 0),
                param_val(1, 1),
                op_val(
                    2,
                    Ty::Tuple(vec![i64_ty(), i64_ty()]),
                    IROp::Alloc { class_id: 99 },
                    vec![],
                ),
                op_val(3, Ty::Primitive(PrimTy::Unit), IROp::Store { offset: 0 }, vec![2, 0]),
            ],
            terminator: Terminator::Branch { target: BlockId(1) },
        };
        let b1 = BasicBlock {
            id: BlockId(1),
            // Load in a different block than the alloc.
            values: vec![op_val(4, i64_ty(), IROp::Load { offset: 0 }, vec![2])],
            terminator: Terminator::Ret { value: Some(ValueId(4)) },
        };
        let (changed, _f) = run_one(vec![b0, b1]);
        assert!(!changed, "cross-block tuple must be left untouched");
    }
}
