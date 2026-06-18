//! IR-level optimization passes. See spec §10.7.
//!
//! Pipeline (M3 baseline — more in M6):
//!
//! 1. Constant folding & propagation                 — `opts::constant_fold`
//! 2. Dead code elimination                          — `opts::dead_code`
//! 3. Trivial copy propagation                       — `opts::copy_prop`
//! 4. Tuple scalar replacement (P1)                  — `opts::tuple_scalar`
//!
//! Deferred to M6: inlining, devirtualization, escape analysis, CSE, LICM,
//! bounds-check elimination, null-check elimination, register allocation,
//! peephole-on-bytecode.

use crate::ir::IRModule;
use crate::opts;

/// Run all enabled passes over `ir` to fixed point.
pub fn run(mut ir: IRModule) -> IRModule {
    // Safety cap: a malformed pass that flip-flops shouldn't hang the build.
    let mut iterations = 0;
    loop {
        let mut changed = false;
        changed |= opts::constant_fold::run(&mut ir);
        changed |= opts::tuple_scalar::run(&mut ir);
        changed |= opts::copy_prop::run(&mut ir);
        changed |= opts::dead_code::run(&mut ir);
        if !changed {
            break;
        }
        iterations += 1;
        if iterations > 32 {
            // Bail out — assume we've hit a non-monotonic interaction.
            break;
        }
    }
    ir
}
