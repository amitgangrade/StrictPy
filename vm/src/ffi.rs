//! Foreign function interface. See spec §17.
//!
//! StrictPy programs declare extern functions with the `@extern("c", "libname")`
//! decorator. At link time the loader resolves each extern symbol against the
//! named dynamic library (or against a built-in registry for the libc and
//! libm bindings shipped with the runtime).
//!
//! Pointer operations (`*T`, `*const T`) are unsafe and require the calling
//! function to be marked `@unsafe`. The default calling convention is `"c"`
//! (System V AMD64 on Unix, Windows x64 on Windows). Strings cross the FFI
//! boundary as `(*u8, usize)` pairs — no implicit null-termination.
//!
//! The runtime guarantees 16-byte stack alignment at the call site so that
//! SSE/AVX moves inside native code are well-defined.
//!
//! This module is a stub; the real FFI bridge lands in a later milestone.
