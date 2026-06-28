//! Diagnostics: error / assert handlers (TODO §18) and memory-usage queries
//! (TODO §17).
//!
//! # Error & assert handlers
//!
//! Noesis routes recoverable errors (`NS_ERROR`), development checks
//! (`NS_CHECK`), and internal assertions (`NS_ASSERT`) through handler
//! functions you can override. There are three:
//!
//! - [`set_error_handler`] — the **global** error handler. Invoked for an error
//!   when no per-thread handler is installed. Receives `file`, `line`,
//!   `message`, `fatal`.
//! - [`set_assert_handler`] — the **global** assert handler. Receives `file`,
//!   `line`, `expr`; returns a `bool` requesting a debug break (`true`) or not.
//! - [`set_thread_error_handler`] — a **per-thread** error handler
//!   (`ErrorHandler2`). Takes priority over the global one for errors raised on
//!   the installing thread, and additionally receives an [`ErrorContext`]
//!   (`uri` / `line` / `column`) — valuable for XAML parse errors.
//!
//! `SetErrorHandler` / `SetAssertHandler` take a bare C function pointer with no
//! userdata, so the Rust closure lives in a process-global slot inside the shim;
//! a fixed trampoline forwards into it. The per-thread variant carries a
//! `void* user`, so its boxed closure threads straight through.
//!
//! Each setter returns an RAII guard. Dropping the guard restores the previous
//! handler (the house convention) and frees the boxed closure. Guards are
//! global state — drop them in LIFO order (nested scopes do this naturally).
//!
//! ## Driving the handlers
//!
//! [`invoke_error`] / [`invoke_error_with_context`] / [`invoke_assert`] call the
//! SDK's public invokers (`InvokeErrorHandler` / `InvokeAssertHandler`), running
//! the registered handler through Noesis's real dispatch path. These double as
//! the only way to exercise a handler deterministically without provoking a real
//! error.
//!
//! ## Safety
//!
//! **Only ever invoke with `fatal = false`.** A `fatal = true` error (like
//! `NS_FATAL`) aborts the process after the handler returns, and a real failed
//! `NS_ASSERT` in a Debug SDK build may break/abort. The invokers here let you
//! choose `fatal`; keep it `false` outside of deliberate crash testing.
//!
//! These are `NsCore` kernel functions: call [`crate::init`] first.
//!
//! # Memory queries
//!
//! [`allocated_memory`], [`allocated_memory_accum`], and [`allocations_count`]
//! expose Noesis's process-global allocator counters. The absolute values are
//! not meaningful across builds; reason about **deltas** and **monotonicity**.

use std::ffi::{CStr, CString, c_void};
use std::os::raw::c_char;

use crate::ffi::{
    AssertFn, Error2Fn, ErrorContext as FfiErrorContext, ErrorFn, dm_noesis_get_allocated_memory,
    dm_noesis_get_allocated_memory_accum, dm_noesis_get_allocations_count,
    dm_noesis_invoke_assert_handler, dm_noesis_invoke_error_handler, dm_noesis_set_assert_handler,
    dm_noesis_set_error_handler, dm_noesis_set_thread_error_handler,
};

/// A borrowed C string → owned `String`, empty on null.
unsafe fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        // SAFETY: caller guarantees `p` is a valid NUL-terminated C string for
        // the duration of the call; we copy it immediately.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

/// A borrowed C string → owned `String`, `None` on null.
unsafe fn cstr_opt(p: *const c_char) -> Option<String> {
    if p.is_null() {
        None
    } else {
        // SAFETY: as `cstr`.
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }
}

/// Context for an error raised with location info — surfaced to a per-thread
/// handler ([`set_thread_error_handler`]). For XAML parse errors `uri` names the
/// offending document and `line` / `column` the position within it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ErrorContext {
    /// The document URI the error refers to (e.g. a XAML file), if any.
    pub uri: Option<String>,
    /// 1-based line within the document, or 0 when unknown.
    pub line: u32,
    /// 1-based column within the document, or 0 when unknown.
    pub column: u32,
}

// ── Global error handler ─────────────────────────────────────────────────────

type ErrorClosure = Box<dyn FnMut(&str, u32, &str, bool) + Send + 'static>;

/// SAFETY: `userdata` is the `Box<ErrorClosure>` leaked in [`set_error_handler`]
/// and kept alive by the live [`ErrorHandlerGuard`].
unsafe extern "C" fn error_trampoline(
    userdata: *mut c_void,
    file: *const c_char,
    line: u32,
    message: *const c_char,
    fatal: bool,
) {
    // SAFETY: userdata is the leaked Box<ErrorClosure>; the guard guarantees it
    // outlives every dispatch.
    let closure = unsafe { &mut *userdata.cast::<ErrorClosure>() };
    let file = unsafe { cstr(file) };
    let message = unsafe { cstr(message) };
    closure(&file, line, &message, fatal);
}

/// RAII guard for the global error handler. On drop, restores the previously
/// installed handler and frees the boxed closure.
#[must_use = "dropping the guard immediately uninstalls the handler"]
pub struct ErrorHandlerGuard {
    boxed: *mut ErrorClosure,
    prev_cb: Option<ErrorFn>,
    prev_user: *mut c_void,
}

impl Drop for ErrorHandlerGuard {
    fn drop(&mut self) {
        // SAFETY: restore the predecessor (which the C shim re-points its global
        // slot at), then reclaim our leaked closure box exactly once.
        unsafe {
            dm_noesis_set_error_handler(
                self.prev_cb,
                self.prev_user,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            drop(Box::from_raw(self.boxed));
        }
    }
}

/// Install a process-global error handler. The closure receives `file`, `line`,
/// `message`, and `fatal` for every error not claimed by a per-thread handler
/// ([`set_thread_error_handler`]). Keep the returned guard alive for as long as
/// the handler should be active; dropping it restores the previous handler.
///
/// Requires [`crate::init`]. Drive it with [`invoke_error`] (always
/// `fatal = false` — see the module safety note).
pub fn set_error_handler<F>(handler: F) -> ErrorHandlerGuard
where
    F: FnMut(&str, u32, &str, bool) + Send + 'static,
{
    let boxed: *mut ErrorClosure = Box::into_raw(Box::new(Box::new(handler)));
    let mut prev_cb: Option<ErrorFn> = None;
    let mut prev_user: *mut c_void = std::ptr::null_mut();
    // SAFETY: trampoline is extern "C"; `boxed` is freshly leaked and kept alive
    // by the guard; the out-params receive the previous (cb, user).
    unsafe {
        dm_noesis_set_error_handler(
            Some(error_trampoline),
            boxed.cast(),
            &mut prev_cb,
            &mut prev_user,
        );
    }
    ErrorHandlerGuard {
        boxed,
        prev_cb,
        prev_user,
    }
}

// ── Global assert handler ─────────────────────────────────────────────────────

type AssertClosure = Box<dyn FnMut(&str, u32, &str) -> bool + Send + 'static>;

/// SAFETY: `userdata` is the `Box<AssertClosure>` leaked in
/// [`set_assert_handler`], kept alive by the live [`AssertHandlerGuard`].
unsafe extern "C" fn assert_trampoline(
    userdata: *mut c_void,
    file: *const c_char,
    line: u32,
    expr: *const c_char,
) -> bool {
    // SAFETY: see `error_trampoline`.
    let closure = unsafe { &mut *userdata.cast::<AssertClosure>() };
    let file = unsafe { cstr(file) };
    let expr = unsafe { cstr(expr) };
    closure(&file, line, &expr)
}

/// RAII guard for the global assert handler; restores the predecessor on drop.
#[must_use = "dropping the guard immediately uninstalls the handler"]
pub struct AssertHandlerGuard {
    boxed: *mut AssertClosure,
    prev_cb: Option<AssertFn>,
    prev_user: *mut c_void,
}

impl Drop for AssertHandlerGuard {
    fn drop(&mut self) {
        // SAFETY: see `ErrorHandlerGuard::drop`.
        unsafe {
            dm_noesis_set_assert_handler(
                self.prev_cb,
                self.prev_user,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            drop(Box::from_raw(self.boxed));
        }
    }
}

/// Install a process-global assert handler. The closure receives `file`,
/// `line`, `expr` and returns whether Noesis should request a debug break
/// (`true`) for that assertion. Keep the guard alive while active.
///
/// Requires [`crate::init`]. Drive it with [`invoke_assert`] — do **not** let a
/// real failed `NS_ASSERT` reach it (it may abort in a Debug SDK build).
pub fn set_assert_handler<F>(handler: F) -> AssertHandlerGuard
where
    F: FnMut(&str, u32, &str) -> bool + Send + 'static,
{
    let boxed: *mut AssertClosure = Box::into_raw(Box::new(Box::new(handler)));
    let mut prev_cb: Option<AssertFn> = None;
    let mut prev_user: *mut c_void = std::ptr::null_mut();
    // SAFETY: as `set_error_handler`.
    unsafe {
        dm_noesis_set_assert_handler(
            Some(assert_trampoline),
            boxed.cast(),
            &mut prev_cb,
            &mut prev_user,
        );
    }
    AssertHandlerGuard {
        boxed,
        prev_cb,
        prev_user,
    }
}

// ── Per-thread error handler (ErrorHandler2, carries an ErrorContext) ─────────

type Error2Closure = Box<dyn FnMut(&str, u32, &str, bool, Option<&ErrorContext>) + Send + 'static>;

/// SAFETY: `userdata` is the `Box<Error2Closure>` leaked in
/// [`set_thread_error_handler`], kept alive by the live guard. `context` is null
/// or a valid `ErrorContext*` for the duration of the call.
unsafe extern "C" fn error2_trampoline(
    file: *const c_char,
    line: u32,
    message: *const c_char,
    fatal: bool,
    context: *mut FfiErrorContext,
    userdata: *mut c_void,
) {
    // SAFETY: see `error_trampoline`; userdata is our leaked Box<Error2Closure>.
    let closure = unsafe { &mut *userdata.cast::<Error2Closure>() };
    let file = unsafe { cstr(file) };
    let message = unsafe { cstr(message) };
    let ctx = if context.is_null() {
        None
    } else {
        // SAFETY: non-null context is a valid ErrorContext for this call.
        let c = unsafe { &*context };
        Some(ErrorContext {
            uri: unsafe { cstr_opt(c.uri) },
            line: c.line,
            column: c.column,
        })
    };
    closure(&file, line, &message, fatal, ctx.as_ref());
}

/// RAII guard for the per-thread error handler; restores the predecessor on
/// drop. Because the handler is thread-scoped, install and drop it on the same
/// thread.
#[must_use = "dropping the guard immediately uninstalls the handler"]
pub struct ThreadErrorHandlerGuard {
    boxed: *mut Error2Closure,
    prev_handler: Option<Error2Fn>,
    prev_user: *mut c_void,
}

impl Drop for ThreadErrorHandlerGuard {
    fn drop(&mut self) {
        // SAFETY: restore the predecessor handler+user, then reclaim our box.
        unsafe {
            dm_noesis_set_thread_error_handler(
                self.prev_handler,
                self.prev_user,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            drop(Box::from_raw(self.boxed));
        }
    }
}

/// Install a per-thread error handler. Unlike [`set_error_handler`], it takes
/// priority for errors raised on this thread and additionally receives an
/// [`ErrorContext`] (the offending `uri` / `line` / `column`, e.g. for a XAML
/// parse error). The closure receives `file`, `line`, `message`, `fatal`, and
/// `Some(&ErrorContext)` when location info was supplied (else `None`).
///
/// Requires [`crate::init`]. Drive it with [`invoke_error_with_context`]
/// (always `fatal = false`).
pub fn set_thread_error_handler<F>(handler: F) -> ThreadErrorHandlerGuard
where
    F: FnMut(&str, u32, &str, bool, Option<&ErrorContext>) + Send + 'static,
{
    let boxed: *mut Error2Closure = Box::into_raw(Box::new(Box::new(handler)));
    let mut prev_handler: Option<Error2Fn> = None;
    let mut prev_user: *mut c_void = std::ptr::null_mut();
    // SAFETY: trampoline is extern "C"; `boxed` is the threaded userdata kept
    // alive by the guard; the out-params receive the previous (handler, user).
    unsafe {
        dm_noesis_set_thread_error_handler(
            Some(error2_trampoline),
            boxed.cast(),
            &mut prev_handler,
            &mut prev_user,
        );
    }
    ThreadErrorHandlerGuard {
        boxed,
        prev_handler,
        prev_user,
    }
}

// ── Invokers ──────────────────────────────────────────────────────────────────

/// Run the registered error handler through Noesis's real dispatch
/// (`InvokeErrorHandler`) with no [`ErrorContext`]. Routes to the per-thread
/// handler if one is installed on this thread, else the global one.
///
/// Keep `fatal = false`: a `fatal = true` invocation aborts the process after
/// the handler returns.
///
/// # Panics
///
/// Panics if `file` or `message` contain an interior NUL byte.
pub fn invoke_error(file: &str, line: u32, fatal: bool, message: &str) {
    let cf = CString::new(file).expect("file contained interior NUL");
    let cm = CString::new(message).expect("message contained interior NUL");
    // SAFETY: both C strings live for the call; has_context=false so the uri
    // pointer is ignored.
    unsafe {
        dm_noesis_invoke_error_handler(
            cf.as_ptr(),
            line,
            fatal,
            false,
            std::ptr::null(),
            0,
            0,
            cm.as_ptr(),
        );
    }
}

/// Like [`invoke_error`], but supplies a non-null [`ErrorContext`] built from
/// `uri` / `ctx_line` / `ctx_col`. Only a per-thread handler
/// ([`set_thread_error_handler`]) observes the context; the global handler does
/// not receive it.
///
/// Keep `fatal = false` (see [`invoke_error`]).
///
/// # Panics
///
/// Panics if `file`, `uri`, or `message` contain an interior NUL byte.
pub fn invoke_error_with_context(
    file: &str,
    line: u32,
    fatal: bool,
    uri: &str,
    ctx_line: u32,
    ctx_col: u32,
    message: &str,
) {
    let cf = CString::new(file).expect("file contained interior NUL");
    let cu = CString::new(uri).expect("uri contained interior NUL");
    let cm = CString::new(message).expect("message contained interior NUL");
    // SAFETY: all three C strings live for the call; has_context=true.
    unsafe {
        dm_noesis_invoke_error_handler(
            cf.as_ptr(),
            line,
            fatal,
            true,
            cu.as_ptr(),
            ctx_line,
            ctx_col,
            cm.as_ptr(),
        );
    }
}

/// Run the registered assert handler through Noesis (`InvokeAssertHandler`),
/// returning whatever the handler returned (a debug-break request). With no
/// custom handler this hits the Noesis default.
///
/// # Panics
///
/// Panics if `file` or `expr` contain an interior NUL byte.
#[must_use]
pub fn invoke_assert(file: &str, line: u32, expr: &str) -> bool {
    let cf = CString::new(file).expect("file contained interior NUL");
    let ce = CString::new(expr).expect("expr contained interior NUL");
    // SAFETY: both C strings live for the call.
    unsafe { dm_noesis_invoke_assert_handler(cf.as_ptr(), line, ce.as_ptr()) }
}

// ── Memory queries (TODO §17) ─────────────────────────────────────────────────

/// Bytes currently allocated through Noesis's allocator
/// (`Noesis::GetAllocatedMemory`). Rises and falls as objects are created and
/// freed; reason about deltas, not the absolute value.
#[must_use]
pub fn allocated_memory() -> u32 {
    // SAFETY: a process-global counter read; safe any time after init.
    unsafe { dm_noesis_get_allocated_memory() }
}

/// Cumulative bytes ever allocated through Noesis's allocator
/// (`Noesis::GetAllocatedMemoryAccum`). Monotonic non-decreasing.
#[must_use]
pub fn allocated_memory_accum() -> u32 {
    // SAFETY: as `allocated_memory`.
    unsafe { dm_noesis_get_allocated_memory_accum() }
}

/// Number of live allocations through Noesis's allocator
/// (`Noesis::GetAllocationsCount`). Rises and falls with object lifetimes.
#[must_use]
pub fn allocations_count() -> u32 {
    // SAFETY: as `allocated_memory`.
    unsafe { dm_noesis_get_allocations_count() }
}
