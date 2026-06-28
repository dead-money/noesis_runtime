//! Panic containment for FFI trampolines.
//!
//! Noesis invokes the crate's exported `extern "C" fn` trampolines across the C
//! ABI, sometimes from a dedicated render thread. If Rust code unwinds across
//! that boundary the behavior is undefined (an immediate abort at best). Every
//! trampoline body therefore runs inside [`guard`] (or [`guard_or`] when the
//! return type has no meaningful [`Default`], e.g. a raw pointer). A caught
//! panic is swallowed and replaced with a value the C side treats as the
//! least-harmful "do nothing"/failure outcome (`()`, `false`, `0`, null, ...).
//!
//! User code panicking inside one of these callbacks is contained here rather
//! than tearing down the process; the panic message is still printed by the
//! default panic hook.

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Run `f`, containing any panic that escapes it.
///
/// On a caught panic, returns `R::default()`, chosen so each trampoline's
/// substitute value is the engine's "no-op"/failure case (`()` for void
/// callbacks, `false` for predicates, `0`/`0.0` for numeric results, ...).
#[inline]
pub(crate) fn guard<R: Default>(f: impl FnOnce() -> R) -> R {
    guard_or(R::default(), f)
}

/// Run `f`, containing any panic that escapes it, returning `default` on panic.
///
/// Use this for trampolines whose return type has no suitable [`Default`]
/// (typically a raw pointer, where the safe default is `std::ptr::null_mut()`).
#[inline]
pub(crate) fn guard_or<R>(default: R, f: impl FnOnce() -> R) -> R {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => default,
    }
}
