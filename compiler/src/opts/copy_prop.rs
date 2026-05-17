//! Trivial copy propagation. Spec §10.7 (subset of pass #1).
//!
//! A `Copy { src }` whose `src` is itself another `Copy { src: src' }` can
//! be folded to `Copy { src: src' }`. Chains collapse in one fixed-point
//! iteration.

use std::collections::HashMap;

use crate::ir::{IRModule, IROp, ValueId, ValueKind};

pub fn run(ir: &mut IRModule) -> bool {
    let mut changed = false;
    for f in &mut ir.functions {
        // Build a per-function map: id → its copy source (only when value
        // is itself a Copy with a single arg).
        let mut copy_src: HashMap<u32, u32> = HashMap::new();
        for b in &f.blocks {
            for v in &b.values {
                if let ValueKind::Op { op: IROp::Copy, args } = &v.kind {
                    if args.len() == 1 {
                        copy_src.insert(v.id, args[0].0);
                    }
                }
            }
        }
        if copy_src.is_empty() {
            continue;
        }
        // Resolve transitively.
        let resolve = |mut id: u32| -> u32 {
            let mut hops = 0;
            while let Some(src) = copy_src.get(&id).copied() {
                if src == id || hops > 64 {
                    break;
                }
                id = src;
                hops += 1;
            }
            id
        };
        for b in &mut f.blocks {
            for v in &mut b.values {
                match &mut v.kind {
                    ValueKind::Op { args, .. } => {
                        for a in args {
                            let r = resolve(a.0);
                            if r != a.0 {
                                *a = ValueId(r);
                                changed = true;
                            }
                        }
                    }
                    ValueKind::Phi { incoming } => {
                        for (_, a) in incoming {
                            let r = resolve(a.0);
                            if r != a.0 {
                                *a = ValueId(r);
                                changed = true;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    changed
}
