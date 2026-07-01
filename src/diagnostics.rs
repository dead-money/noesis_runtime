//! Diagnostics: error / assert handlers and memory-usage queries.
//!
//! # Error & assert handlers
//!
//! Noesis routes recoverable errors (`NS_ERROR`), development checks
//! (`NS_CHECK`), and internal assertions (`NS_ASSERT`) through handler
//! functions you can override. There are three:
//!
//! - [`set_error_handler`]: the **global** error handler. Invoked for an error
//!   when no per-thread handler is installed. Receives `file`, `line`,
//!   `message`, `fatal`.
//! - [`set_assert_handler`]: the **global** assert handler. Receives `file`,
//!   `line`, `expr`; returns a `bool` requesting a debug break (`true`) or not.
//! - [`set_thread_error_handler`]: a **per-thread** error handler
//!   (`ErrorHandler2`). Takes priority over the global one for errors raised on
//!   the installing thread, and additionally receives an [`ErrorContext`]
//!   (`uri` / `line` / `column`), valuable for XAML parse errors.
//!
//! `SetErrorHandler` / `SetAssertHandler` take a bare C function pointer with no
//! userdata, so the Rust closure lives in a process-global slot inside the shim;
//! a fixed trampoline forwards into it. The per-thread variant carries a
//! `void* user`, so its boxed closure threads straight through.
//!
//! Each setter returns an RAII guard. The error and assert handlers are
//! **process-global with a single slot**; the per-thread handler owns one slot
//! per thread. All three are **last-registration-wins**: installing a second
//! handler replaces the first, and the older guard is then logically dead even
//! though it is still alive. To make `Drop` safe under that reality, each
//! registration carries a unique generation id and the active-registration id is
//! recorded per hook. A guard's `Drop` clears its slot (restoring Noesis's own
//! default handler) **only if it is still the active registration**; otherwise
//! it just frees its own boxed closure and leaves the slot pointing at whoever
//! overwrote it. Replacing a handler therefore *drops* the old one rather than
//! stacking it: once replaced, a predecessor is gone for good and is **not**
//! restored when its successor is dropped. Each guard always frees exactly its
//! own box, so drop order can never cause a double-free or use-after-free.
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

use std::cell::Cell;
use std::ffi::{CStr, CString, c_void};
use std::os::raw::c_char;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::ffi::{
    ErrorContext as FfiErrorContext, noesis_get_allocated_memory,
    noesis_get_allocated_memory_accum, noesis_get_allocations_count, noesis_invoke_assert_handler,
    noesis_invoke_error_handler, noesis_set_assert_handler, noesis_set_error_handler,
    noesis_set_thread_error_handler,
};

/// Monotonic source of per-registration ids, shared across every hook. `0` is
/// reserved as the "no active registration" sentinel, so ids start at `1`.
static NEXT_REG_ID: AtomicU64 = AtomicU64::new(1);

/// Allocate a fresh, never-`0` registration id.
fn next_reg_id() -> u64 {
    NEXT_REG_ID.fetch_add(1, Ordering::Relaxed)
}

/// Id of the global error hook's currently active registration (`0` = none).
static ERROR_ACTIVE: AtomicU64 = AtomicU64::new(0);

/// Id of the global assert hook's currently active registration (`0` = none).
static ASSERT_ACTIVE: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Id of *this thread's* currently active per-thread error registration
    /// (`0` = none). Thread-local because the handler is thread-scoped and the
    /// guard is `!Send`, so install and drop always run on the same thread.
    static THREAD_ERROR_ACTIVE: Cell<u64> = const { Cell::new(0) };
}

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

/// Context for an error raised with location info, surfaced to a per-thread
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

type ErrorClosure = Box<dyn Fn(&str, u32, &str, bool) + Send + 'static>;

/// SAFETY: `userdata` is the `Box<ErrorClosure>` leaked in [`set_error_handler`]
/// and kept alive by the live [`ErrorHandlerGuard`].
unsafe extern "C" fn error_trampoline(
    userdata: *mut c_void,
    file: *const c_char,
    line: u32,
    message: *const c_char,
    fatal: bool,
) {
    crate::panic_guard::guard(|| {
        // SAFETY: userdata is the leaked Box<ErrorClosure>; the guard guarantees it
        // outlives every dispatch. Shared `&`: the handler is `Fn`, so a
        // re-entrant invoke that materialises a second reference is sound.
        let closure = unsafe { &*userdata.cast::<ErrorClosure>() };
        let file = unsafe { cstr(file) };
        let message = unsafe { cstr(message) };
        closure(&file, line, &message, fatal);
    })
}

/// RAII guard for the global error handler. On drop, if this is still the active
/// registration, it restores Noesis's default handler; it always frees its
/// boxed closure. See the module docs for the last-registration-wins semantics.
#[must_use = "dropping the guard immediately uninstalls the handler"]
pub struct ErrorHandlerGuard {
    boxed: *mut ErrorClosure,
    id: u64,
}

impl Drop for ErrorHandlerGuard {
    fn drop(&mut self) {
        // Clear the global slot only if it still names THIS registration; a newer
        // set_error_handler otherwise owns it and must keep firing.
        if ERROR_ACTIVE
            .compare_exchange(self.id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            // SAFETY: a null cb tells the shim to restore Noesis's saved default;
            // no previous (cb, user) requested.
            unsafe {
                noesis_set_error_handler(
                    None,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
        }
        // SAFETY: reclaim our leaked closure box exactly once.
        unsafe { drop(Box::from_raw(self.boxed)) };
    }
}

/// Install a process-global error handler. The closure receives `file`, `line`,
/// `message`, and `fatal` for every error not claimed by a per-thread handler
/// ([`set_thread_error_handler`]). It must be `Fn` because an error raised from
/// inside the handler re-enters it synchronously; use interior mutability for
/// handler state. Keep the returned guard alive for as long as the handler
/// should be active.
///
/// This hook is **process-global with a single slot**: a later
/// `set_error_handler` replaces this one (last-registration-wins), after which
/// dropping this now-older guard no longer touches the slot; dropping the active
/// guard restores Noesis's default handler (not a predecessor). See the module
/// docs for the full semantics.
///
/// Requires [`crate::init`]. Drive it with [`invoke_error`] (always
/// `fatal = false`; see the module safety note).
pub fn set_error_handler<F>(handler: F) -> ErrorHandlerGuard
where
    F: Fn(&str, u32, &str, bool) + Send + 'static,
{
    let boxed: *mut ErrorClosure = Box::into_raw(Box::new(Box::new(handler)));
    let id = next_reg_id();
    // Claim the id before writing the slot so a stale guard's drop (which
    // compare-exchanges against the active id) can never clobber this fresh
    // registration.
    ERROR_ACTIVE.store(id, Ordering::Release);
    // SAFETY: trampoline is extern "C"; `boxed` is freshly leaked and kept alive
    // by the guard; no previous (cb, user) requested.
    unsafe {
        noesis_set_error_handler(
            Some(error_trampoline),
            boxed.cast(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }
    ErrorHandlerGuard { boxed, id }
}

type AssertClosure = Box<dyn Fn(&str, u32, &str) -> bool + Send + 'static>;

/// SAFETY: `userdata` is the `Box<AssertClosure>` leaked in
/// [`set_assert_handler`], kept alive by the live [`AssertHandlerGuard`].
unsafe extern "C" fn assert_trampoline(
    userdata: *mut c_void,
    file: *const c_char,
    line: u32,
    expr: *const c_char,
) -> bool {
    // A panicking handler is contained and reported as `false` (no debug break).
    crate::panic_guard::guard(|| {
        // SAFETY: see `error_trampoline`; shared `&` because the handler is `Fn`.
        let closure = unsafe { &*userdata.cast::<AssertClosure>() };
        let file = unsafe { cstr(file) };
        let expr = unsafe { cstr(expr) };
        closure(&file, line, &expr)
    })
}

/// RAII guard for the global assert handler. On drop, if still the active
/// registration, restores Noesis's default handler; always frees its box. See
/// the module docs for the last-registration-wins semantics.
#[must_use = "dropping the guard immediately uninstalls the handler"]
pub struct AssertHandlerGuard {
    boxed: *mut AssertClosure,
    id: u64,
}

impl Drop for AssertHandlerGuard {
    fn drop(&mut self) {
        // See `ErrorHandlerGuard::drop`.
        if ASSERT_ACTIVE
            .compare_exchange(self.id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            // SAFETY: a null cb restores Noesis's saved default assert handler.
            unsafe {
                noesis_set_assert_handler(
                    None,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
        }
        // SAFETY: reclaim our leaked closure box exactly once.
        unsafe { drop(Box::from_raw(self.boxed)) };
    }
}

/// Install a process-global assert handler. The closure receives `file`,
/// `line`, `expr` and returns whether Noesis should request a debug break
/// (`true`) for that assertion. It must be `Fn` because an assertion raised from
/// inside the handler re-enters it synchronously; use interior mutability for
/// handler state. Keep the guard alive while active.
///
/// Process-global, single-slot, last-registration-wins; dropping the active
/// guard restores Noesis's default (not a predecessor). See the module docs.
///
/// Requires [`crate::init`]. Drive it with [`invoke_assert`]. Do **not** let a
/// real failed `NS_ASSERT` reach it (it may abort in a Debug SDK build).
pub fn set_assert_handler<F>(handler: F) -> AssertHandlerGuard
where
    F: Fn(&str, u32, &str) -> bool + Send + 'static,
{
    let boxed: *mut AssertClosure = Box::into_raw(Box::new(Box::new(handler)));
    let id = next_reg_id();
    // Claim the id before writing the slot (see `set_error_handler`).
    ASSERT_ACTIVE.store(id, Ordering::Release);
    // SAFETY: as `set_error_handler`.
    unsafe {
        noesis_set_assert_handler(
            Some(assert_trampoline),
            boxed.cast(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }
    AssertHandlerGuard { boxed, id }
}

type Error2Closure = Box<dyn Fn(&str, u32, &str, bool, Option<&ErrorContext>) + Send + 'static>;

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
    crate::panic_guard::guard(|| {
        // SAFETY: see `error_trampoline`; userdata is our leaked Box<Error2Closure>.
        // Shared `&` because the handler is `Fn` (re-entrant-safe).
        let closure = unsafe { &*userdata.cast::<Error2Closure>() };
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
    })
}

/// RAII guard for the per-thread error handler. On drop, if still this thread's
/// active registration, it clears the thread handler; it always frees its box.
/// Because the handler is thread-scoped (and this guard is `!Send`), install and
/// drop it on the same thread. Last-registration-wins per the module docs.
#[must_use = "dropping the guard immediately uninstalls the handler"]
pub struct ThreadErrorHandlerGuard {
    boxed: *mut Error2Closure,
    id: u64,
}

impl Drop for ThreadErrorHandlerGuard {
    fn drop(&mut self) {
        // Clear this thread's slot only if it still names THIS registration; a
        // newer set_thread_error_handler on this thread otherwise owns it.
        let still_active = THREAD_ERROR_ACTIVE.with(|c| {
            if c.get() == self.id {
                c.set(0);
                true
            } else {
                false
            }
        });
        if still_active {
            // SAFETY: a null handler clears this thread's error handler; no
            // previous (handler, user) requested.
            unsafe {
                noesis_set_thread_error_handler(
                    None,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
        }
        // SAFETY: reclaim our leaked closure box exactly once.
        unsafe { drop(Box::from_raw(self.boxed)) };
    }
}

/// Install a per-thread error handler. Unlike [`set_error_handler`], it takes
/// priority for errors raised on this thread and additionally receives an
/// [`ErrorContext`] (the offending `uri` / `line` / `column`, e.g. for a XAML
/// parse error). The closure receives `file`, `line`, `message`, `fatal`, and
/// `Some(&ErrorContext)` when location info was supplied (else `None`). It must
/// be `Fn` because an error raised from inside the handler re-enters it
/// synchronously; use interior mutability for handler state.
///
/// One slot per thread, last-registration-wins: a later
/// `set_thread_error_handler` on this thread replaces this one, after which
/// dropping this now-older guard no longer touches the slot; dropping the active
/// guard clears the thread handler (no predecessor is restored). See the module
/// docs.
///
/// Requires [`crate::init`]. Drive it with [`invoke_error_with_context`]
/// (always `fatal = false`).
pub fn set_thread_error_handler<F>(handler: F) -> ThreadErrorHandlerGuard
where
    F: Fn(&str, u32, &str, bool, Option<&ErrorContext>) + Send + 'static,
{
    let boxed: *mut Error2Closure = Box::into_raw(Box::new(Box::new(handler)));
    let id = next_reg_id();
    // Claim the id before writing the slot (see `set_error_handler`).
    THREAD_ERROR_ACTIVE.with(|c| c.set(id));
    // SAFETY: trampoline is extern "C"; `boxed` is the threaded userdata kept
    // alive by the guard; no previous (handler, user) requested.
    unsafe {
        noesis_set_thread_error_handler(
            Some(error2_trampoline),
            boxed.cast(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }
    ThreadErrorHandlerGuard { boxed, id }
}

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
        noesis_invoke_error_handler(
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
        noesis_invoke_error_handler(
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
    unsafe { noesis_invoke_assert_handler(cf.as_ptr(), line, ce.as_ptr()) }
}

/// Bytes currently allocated through Noesis's allocator
/// (`Noesis::GetAllocatedMemory`). Rises and falls as objects are created and
/// freed; reason about deltas, not the absolute value.
#[must_use]
pub fn allocated_memory() -> u32 {
    // SAFETY: a process-global counter read; safe any time after init.
    unsafe { noesis_get_allocated_memory() }
}

/// Cumulative bytes ever allocated through Noesis's allocator
/// (`Noesis::GetAllocatedMemoryAccum`). Monotonic non-decreasing.
#[must_use]
pub fn allocated_memory_accum() -> u32 {
    // SAFETY: as `allocated_memory`.
    unsafe { noesis_get_allocated_memory_accum() }
}

/// Number of live allocations through Noesis's allocator
/// (`Noesis::GetAllocationsCount`). Rises and falls with object lifetimes.
#[must_use]
pub fn allocations_count() -> u32 {
    // SAFETY: as `allocated_memory`.
    unsafe { noesis_get_allocations_count() }
}
