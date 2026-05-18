# Design decision: StrictPy cannot use NumPy or pandas

**Status**: structural limitation, not a planned feature
**Trade-off**: ecosystem incompatibility vs every other architectural choice the project is built on

## The question

A user asked: "Make sure that StrictPy can use NumPy and pandas." The honest
answer is **no, and getting there isn't a small fix.**

## Why it can't work in current StrictPy

StrictPy isn't Python. It's a different runtime that happens to share Python's
source syntax (with modifications). Concretely:

- StrictPy has its own bytecode format (`.spyc`) executed by `spy.exe`, not
  CPython.
- Object layout is custom — 16-byte `ObjectHeader` + flat fields, no
  `PyObject*`, no refcounts.
- No Python C API surface — no `PyArg_ParseTuple`, no `tp_dealloc`, no buffer
  protocol.
- No GIL, no `Py_INCREF`, no shared memory model with CPython.

NumPy and pandas are **CPython extension modules**. They link against
`libpython`, allocate via Python's memory pool, implement the Python C API,
and use refcount-based lifetime management. There is no way to import them
into StrictPy without reimplementing the entire CPython C API — which would
defeat every architectural choice the project is built on.

## Three theoretical paths (none planned)

1. **Embed CPython in the VM.** `import numpy` becomes "spin up a hosted
   CPython interpreter, import numpy in *that*, marshal data across an FFI
   boundary." Plausible but ~5K+ lines of CPython embedding work, and
   performance reverts to CPython's at the boundary. Projects like
   RustPython have explored this; it's not trivial.

2. **C-level FFI to numpy's C library.** Call numpy's C functions directly
   via FFI. Still need to manage `PyObject*` lifetimes, refcounts, and the
   GIL. Almost as much work as option 1.

3. **Implement numpy/pandas natively in StrictPy.** `Array[f64]` with SIMD
   ops, a typed-column DataFrame, etc. Years of work. Doesn't exist anywhere
   outside numpy itself.

## What StrictPy already does that might be relevant

- `List[f64]` is **already a contiguous f64 buffer** (per the M3 type-erased
  list layout). The benchmark proves it: dot product over `List[f64]` of
  1M elements beats CPython 4× without any vectorization.
- A `numpy.ndarray` view is one cast away — if someone wanted to interop at
  the buffer level rather than the API level, that path is open.
- Future work on SIMD intrinsics for `List[f64]` operations would target the
  same workloads numpy serves.

## When to revisit

If StrictPy ever acquires a non-trivial user base that wants data-science
workloads, option 1 (embed CPython) is the most pragmatic. The "import
numpy" call would block during CPython startup (~50-100ms one-time cost),
numpy code would run at CPython's speed, but the StrictPy parts of the
program would still benefit from the JIT.

Until then, this is a documented structural limitation. Programs that need
numpy belong in CPython, not StrictPy.

## Reference

- Conversation source: orchestrator's response to the user's "make sure
  StrictPy can use numpy and pandas" instruction, prior to the M10 round.
- Related: `methodology.md` "What this archive does and doesn't claim" —
  the project explicitly does not support claims about StrictPy as a
  general data-science runtime.
