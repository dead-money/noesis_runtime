//! Panic containment for FFI trampolines.
//!
//! Noesis calls the crate's `extern "C"` trampolines, sometimes from a render
//! thread. A panic reaching that boundary aborts the process, so every
//! trampoline body runs inside [`guard`] (or [`guard_or`] when the return type
//! has no suitable [`Default`], such as a raw pointer). A caught panic returns
//! the value the C side treats as "do nothing" or failure (`()`, `false`, `0`,
//! null). The default panic hook still prints the message.

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs `f`, returning `R::default()` if it panics.
#[inline]
pub(crate) fn guard<R: Default>(f: impl FnOnce() -> R) -> R {
    guard_or(R::default(), f)
}

/// Runs `f`, returning `default` if it panics. For return types without a
/// suitable [`Default`], such as raw pointers.
#[inline]
pub(crate) fn guard_or<R>(default: R, f: impl FnOnce() -> R) -> R {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => default,
    }
}
