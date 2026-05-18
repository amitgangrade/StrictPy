# Design decision: `is_native` flag on ClassLayout

**Milestone introduced**: M7
**Status**: in production
**Trade-off**: extensibility vs forcing every stdlib type through the dynamic-dispatch path

## The problem

StrictPy has two kinds of "classes":

1. **User classes** — declared with `open`/`final`/`sealed`. Real vtables.
   Real fields at offsets. Method calls use `VirtualCall { vtable_slot }`
   (or `DirectCall` after devirtualization).

2. **Stdlib runtime classes** — `Channel`, `File`, `Dict`, `Thread`, `str`.
   These are *types* in the language but their instances are handle-backed
   — `ChannelRepr { handle: u64 }` is just an index into a VM-side table.
   Method calls on them must dispatch to Rust functions in `vm/src/builtins.rs`
   via `NativeFn` ids, not through a vtable.

The IR lowerer originally emitted `VirtualCall` for any method call. Stdlib
runtime classes don't HAVE vtables, so `ch.send(i)` would trap at runtime
with `"vtable slot 0 out of range"`. This blocked producer.spy and
wordcount.spy from running end-to-end through M5.

## The choice

Add a `pub is_native: bool` field to `ClassLayout` (`compiler/src/types.rs`).

- Resolver sets `is_native: true` when registering prelude runtime classes:
  Channel, File, Dict, Thread, str.
- IR lowerer's `lower_method_call` checks the flag *before* attempting
  vtable dispatch. If `is_native`, skip the vtable path and fall through to
  the existing `NativeCall { native_id: NativeFn::from_name(method) }` path.
- User classes (tree.spy, sealed AST hierarchies in JSON, etc.) keep
  `is_native: false` and dispatch through real vtables.

## Alternatives considered

1. **Separate `Ty::Runtime(RuntimeKind)` variant** that's disjoint from
   `Ty::Class(ClassId)`. Cleaner type-theoretically, but would require
   touching every site that pattern-matches on `Ty` — a much bigger diff
   and more migration risk.

2. **Register every stdlib method as a separate `Function` with `NativeFn`
   body** and have the method call resolve through normal function dispatch.
   Would require synthesizing function declarations for each prelude method,
   and reproduces the "method call vs free call" distinction the language
   doesn't otherwise need.

3. **Two parallel ClassLayout types** — `UserClassLayout` and
   `NativeClassLayout`. Requires resolver to know at registration time
   which kind a class is, and forks downstream consumers.

The `is_native` flag won because it's a single-line change to the existing
type and a one-conditional addition to the existing dispatch. Two LOC of
new state; the rest is existing code paths.

## Trade-offs

- **For**: minimal disruption; existing code keeps working; new runtime
  classes (future Atomic[T], Channel[T] variants, etc.) just set the flag.
- **Against**: the flag is opaque about WHAT kind of native dispatch to
  use. `Channel.send` and `File.read` both have `is_native: true` and the
  decision of which NativeFn to call is buried in `NativeFn::from_name`.
  This makes the dispatch table implicitly typed-by-string instead of
  explicitly by class.
- **Mitigation**: the M7 fix added `resolve_native_method` to
  disambiguate overloaded method names (e.g., `close` on Channel vs File).

## When to revisit

If the project grows runtime classes with overlapping method names, the
string-based dispatch in `NativeFn::from_name` will need replacement by a
proper per-class dispatch table. The signal: any time a method has the
same name on two `is_native` classes and they want different behavior.

## Reference

- Code: `compiler/src/types.rs::ClassLayout`,
  `compiler/src/resolver.rs::register_prelude_class`,
  `compiler/src/ir.rs::lower_method_call`
- Related: M7 brief and report in `agent_reports/`.
