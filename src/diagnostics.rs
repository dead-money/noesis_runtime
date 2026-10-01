//! Error and assert handlers, and Noesis memory counters.
//!
//! Everything here needs [`crate::init`] first.
//!
//! # Error and assert handlers
//!
//! Noesis reports errors and failed assertions through handlers you can
//! replace:
//!
//! - [`set_error_handler`]: the process-wide error handler. Receives `file`,
//!   `line`, `message`, and `fatal`.
//! - [`set_assert_handler`]: the process-wide assert handler. Receives `file`,
//!   `line`, and `expr`, and returns whether to request a debug break.
//! - [`set_thread_error_handler`]: an error handler for the calling thread. It
//!   takes priority over the process-wide one for errors raised on that thread
//!   and also receives an [`ErrorContext`] (document URI, line, column), which
//!   locates XAML parse errors.
//!
//! Each setter returns a guard. There is one slot per handler kind (one per
//! thread for the thread handler), and the last registration wins: installing
//! a new handler replaces the old one, and the old guard no longer controls the
//! slot. Dropping the active guard restores Noesis's default handler, not the
//! one it replaced. Dropping a replaced guard only frees its closure. Guards
//! can be dropped in any order.
//!
//! [`invoke_error`], [`invoke_error_with_context`], and [`invoke_assert`] run
//! the installed handler through Noesis's own dispatch, which is the way to
//! test a handler without causing a real error.
//!
//! # Fatal errors
//!
//! Pass `fatal = false` to the invokers. A fatal error aborts the process after
//! the handler returns. A real failed `NS_ASSERT` in a debug build of the SDK
//! may also break or abort.
//!
//! # Memory counters
//!
//! [`allocated_memory`], [`allocated_memory_accum`], and [`allocations_count`]
//! read Noesis's process-wide allocator counters. Compare values over time
//! rather than reading them as absolutes.

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

/// Registration ids, shared by every hook. Starts at `1`; `0` means "none".
static NEXT_REG_ID: AtomicU64 = AtomicU64::new(1);

fn next_reg_id() -> u64 {
    NEXT_REG_ID.fetch_add(1, Ordering::Relaxed)
}

/// Id of the global error hook's currently active registration (`0` = none).
static ERROR_ACTIVE: AtomicU64 = AtomicU64::new(0);

/// Id of the global assert hook's currently active registration (`0` = none).
static ASSERT_ACTIVE: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Id of this thread's active thread error registration (`0` = none). The
    /// guard is `!Send`, so install and drop run on the same thread.
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

/// Where an error occurred, passed to a [`set_thread_error_handler`] handler.
/// For a XAML parse error, `uri` names the document and `line` / `column` the
/// position in it.
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

/// Keeps a [`set_error_handler`] handler installed. Dropping it restores
/// Noesis's default handler, unless a newer handler has replaced this one. See
/// the [module docs](self).
#[must_use = "dropping the guard immediately uninstalls the handler"]
pub struct ErrorHandlerGuard {
    boxed: *mut ErrorClosure,
    id: u64,
}

impl Drop for ErrorHandlerGuard {
    fn drop(&mut self) {
        // A newer registration owns the slot otherwise and must keep firing.
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

/// Install the process-wide error handler. It receives `file`, `line`,
/// `message`, and `fatal` for every error on a thread without a
/// [`set_thread_error_handler`] handler. The handler is installed while the
/// guard lives.
///
/// The closure is `Fn` because an error raised inside it calls it again; use
/// interior mutability for state. A later call replaces this handler; see the
/// [module docs](self) for how guards interact.
pub fn set_error_handler<F>(handler: F) -> ErrorHandlerGuard
where
    F: Fn(&str, u32, &str, bool) + Send + 'static,
{
    let boxed: *mut ErrorClosure = Box::into_raw(Box::new(Box::new(handler)));
    let id = next_reg_id();
    // Claim the id before writing the slot so a stale guard's drop can't clear
    // this registration.
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
    crate::panic_guard::guard(|| {
        // SAFETY: see `error_trampoline`; shared `&` because the handler is `Fn`.
        let closure = unsafe { &*userdata.cast::<AssertClosure>() };
        let file = unsafe { cstr(file) };
        let expr = unsafe { cstr(expr) };
        closure(&file, line, &expr)
    })
}

/// Keeps a [`set_assert_handler`] handler installed. Dropping it restores
/// Noesis's default handler, unless a newer handler has replaced this one. See
/// the [module docs](self).
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

/// Install the process-wide assert handler. It receives `file`, `line`, and
/// `expr`, and returns `true` to request a debug break. A panic in the handler
/// counts as `false`. The handler is installed while the guard lives.
///
/// The closure is `Fn` because an assertion raised inside it calls it again;
/// use interior mutability for state. A later call replaces this handler; see
/// the [module docs](self).
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

/// Keeps a [`set_thread_error_handler`] handler installed on this thread.
/// Dropping it removes the thread handler, so errors go to the process-wide
/// handler again, unless a newer thread handler has replaced this one. Not
/// `Send`: it must be dropped on the thread that installed it.
#[must_use = "dropping the guard immediately uninstalls the handler"]
pub struct ThreadErrorHandlerGuard {
    boxed: *mut Error2Closure,
    id: u64,
}

impl Drop for ThreadErrorHandlerGuard {
    fn drop(&mut self) {
        // A newer registration on this thread owns the slot otherwise.
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

/// Install an error handler for the calling thread. It takes priority over
/// [`set_error_handler`] for errors raised on this thread. It receives `file`,
/// `line`, `message`, `fatal`, and an [`ErrorContext`] when the error carries a
/// location (e.g. a XAML parse error). The handler is installed while the guard
/// lives.
///
/// The closure is `Fn` because an error raised inside it calls it again; use
/// interior mutability for state. A later call on the same thread replaces this
/// handler; see the [module docs](self).
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

/// Raise an error through Noesis's dispatch, with no [`ErrorContext`]. It goes
/// to this thread's handler if one is installed, otherwise the process-wide
/// one.
///
/// Pass `fatal = false`: `true` aborts the process after the handler returns.
///
/// # Panics
///
/// Panics if `file` or `message` contains an interior NUL byte.
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

/// Like [`invoke_error`], with an [`ErrorContext`] built from `uri`,
/// `ctx_line`, and `ctx_col`. Only a [`set_thread_error_handler`] handler sees
/// the context.
///
/// Pass `fatal = false` (see [`invoke_error`]).
///
/// # Panics
///
/// Panics if `file`, `uri`, or `message` contains an interior NUL byte.
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

/// Report a failed assertion through Noesis's dispatch and return the handler's
/// answer (`true` requests a debug break). Without a [`set_assert_handler`]
/// handler, Noesis's default runs, which may break or abort in a debug SDK
/// build.
///
/// # Panics
///
/// Panics if `file` or `expr` contains an interior NUL byte.
#[must_use]
pub fn invoke_assert(file: &str, line: u32, expr: &str) -> bool {
    let cf = CString::new(file).expect("file contained interior NUL");
    let ce = CString::new(expr).expect("expr contained interior NUL");
    // SAFETY: both C strings live for the call.
    unsafe { noesis_invoke_assert_handler(cf.as_ptr(), line, ce.as_ptr()) }
}

/// Bytes currently allocated by Noesis. Rises and falls as objects are created
/// and freed.
#[must_use]
pub fn allocated_memory() -> u32 {
    // SAFETY: a process-global counter read; safe any time after init.
    unsafe { noesis_get_allocated_memory() }
}

/// Total bytes Noesis has allocated since startup. Never decreases.
#[must_use]
pub fn allocated_memory_accum() -> u32 {
    // SAFETY: as `allocated_memory`.
    unsafe { noesis_get_allocated_memory_accum() }
}

/// Number of live Noesis allocations.
#[must_use]
pub fn allocations_count() -> u32 {
    // SAFETY: as `allocated_memory`.
    unsafe { noesis_get_allocations_count() }
}
