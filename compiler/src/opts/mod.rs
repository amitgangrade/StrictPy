//! Individual optimization passes. See spec §10.7.
//!
//! Each submodule exposes a `pub fn run(ir: &mut IRModule) -> bool` that
//! returns `true` iff the pass changed the IR. The fixed-point driver in
//! [`super::run`](crate::optimize::run) keeps re-running enabled passes
//! until none of them report a change.

pub mod constant_fold;
pub mod copy_prop;
pub mod dead_code;
