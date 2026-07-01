//! System integration callbacks from `NsGui/IntegrationAPI.h` (namespace
//! `Noesis::GUI`). These are **process-global** host hooks (not per-view)
//! that let the host react to engine requests: update the OS cursor, show or
//! hide the on-screen keyboard, open a URL, play a sound, and set the default
//! culture.
//!
//! # Pattern
//!
//! Each `set_*` registration boxes a Rust closure and hands a thin pointer to
//! it across the FFI as Noesis's `void* user`, alongside an `extern "C"`
//! trampoline. The C++ shim (`cpp/noesis_integration.cpp`) keeps a single
//! static `(user, callback)` slot per hook and registers its own translating
//! trampoline with Noesis (converting `Cursor*` → [`CursorType`], `const
//! Uri&` → `&str`). The returned `*Callback` guard owns the boxed closure and
//! clears the registration on `Drop`, mirroring the Registered-guard idiom in
//! [`crate::texture_provider`] / [`crate::font_provider`].
//!
//! # Single-slot, last-registration-wins
//!
//! These hooks are **process-global with exactly one slot per hook**. Both
//! Noesis itself and the C++ shim store a single `(user, callback)` pair. They
//! are therefore **not** independent, freely-stacked registrations: calling a
//! `set_*` again replaces the previous registration (last-registration-wins),
//! and the older guard is then logically dead even though it is still alive.
//!
//! To make `Drop` safe under that reality, each registration is tagged with a
//! unique generation id (see `next_reg_id`) and a per-hook atomic records the
//! id of the *currently active* registration. A guard's `Drop` clears the
//! global slot **only if it is still the active registration** (its id still
//! matches the per-hook atomic); otherwise it just frees its own boxed closure
//! and leaves the slot pointing at whoever overwrote it. Consequences:
//!
//!   - Dropping the **older** of two guards for the same hook is a no-op on the
//!     slot. The newer registration keeps firing (no clobber). Its box is
//!     still freed; the C++ slot never referenced it after the overwrite.
//!   - Dropping the **active** guard clears the slot and unregisters the Noesis
//!     callback. A previously-overwritten registration is **not** restored:
//!     once replaced, an older registration is gone for good.
//!
//! Each guard always frees exactly its own boxed closure, so there is no
//! double-free or use-after-free regardless of drop order.
//!
//! Registration publishes the new generation id *before* writing the FFI slot,
//! so a stale guard's `Drop` can never compare-exchange its way into clobbering
//! a newer registration. This relies on one invariant the caller must uphold:
//! **guard drops must be serialized with the view thread's callback dispatch**
//! (see "Lifetime" below). A guard freed concurrently with an in-flight callback
//! on the same hook is a data race regardless of the id bookkeeping.
//!
//! # Triggering
//!
//! [`open_url`] and [`play_audio`] invoke the registered callback
//! **synchronously**: they are genuine end-to-end round trips and are tested
//! as such. The cursor callback fires from input handling inside a live view's
//! event pump and *is* reachable headlessly: a mouse-move over an element with
//! a non-default `Cursor` drives it (proved in `tests/integration.rs`). The
//! software-keyboard callback fires only when a virtual-keyboard-enabled
//! element gains focus on a platform that requests it; that path can't be
//! synthesised headlessly, so for it we verify only that registration /
//! unregistration crosses the FFI cleanly.
//!
//! # Lifetime
//!
//! A guard must outlive every Noesis-internal reference that might call back
//! into the closure. Keep it alive until after [`crate::shutdown`] returns (or
//! until you explicitly drop it to unregister).

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface; explicit blocks add noise

use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};
use std::os::raw::c_char;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::ffi;

/// Monotonic source of per-registration ids, shared across every hook. `0` is
/// reserved as the "no active registration" sentinel, so ids start at `1`.
static NEXT_REG_ID: AtomicU64 = AtomicU64::new(1);

/// Allocate a fresh, never-`0` registration id.
fn next_reg_id() -> u64 {
    NEXT_REG_ID.fetch_add(1, Ordering::Relaxed)
}

/// Built-in cursor types, mirroring `Noesis::CursorType` in
/// `NsGui/Cursor.h`. The discriminants match the C++ enum exactly so the
/// value read back from a live `Cursor` round-trips.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(i32)]
#[non_exhaustive]
pub enum CursorType {
    None = 0,
    No = 1,
    Arrow = 2,
    AppStarting = 3,
    Cross = 4,
    Help = 5,
    IBeam = 6,
    SizeAll = 7,
    SizeNESW = 8,
    SizeNS = 9,
    SizeNWSE = 10,
    SizeWE = 11,
    UpArrow = 12,
    Wait = 13,
    Hand = 14,
    Pen = 15,
    ScrollNS = 16,
    ScrollWE = 17,
    ScrollAll = 18,
    ScrollN = 19,
    ScrollS = 20,
    ScrollW = 21,
    ScrollE = 22,
    ScrollNW = 23,
    ScrollNE = 24,
    ScrollSW = 25,
    ScrollSE = 26,
    ArrowCD = 27,
    Custom = 28,
}

impl CursorType {
    /// Map a raw `Noesis::CursorType` integer to a [`CursorType`]. Unknown
    /// values (including the `CursorType_Count` sentinel) map to
    /// [`CursorType::None`].
    #[must_use]
    pub fn from_raw(raw: i32) -> Self {
        match raw {
            1 => Self::No,
            2 => Self::Arrow,
            3 => Self::AppStarting,
            4 => Self::Cross,
            5 => Self::Help,
            6 => Self::IBeam,
            7 => Self::SizeAll,
            8 => Self::SizeNESW,
            9 => Self::SizeNS,
            10 => Self::SizeNWSE,
            11 => Self::SizeWE,
            12 => Self::UpArrow,
            13 => Self::Wait,
            14 => Self::Hand,
            15 => Self::Pen,
            16 => Self::ScrollNS,
            17 => Self::ScrollWE,
            18 => Self::ScrollAll,
            19 => Self::ScrollN,
            20 => Self::ScrollS,
            21 => Self::ScrollW,
            22 => Self::ScrollE,
            23 => Self::ScrollNW,
            24 => Self::ScrollNE,
            25 => Self::ScrollSW,
            26 => Self::ScrollSE,
            27 => Self::ArrowCD,
            28 => Self::Custom,
            _ => Self::None,
        }
    }
}

/// Decode a Noesis-supplied string lossily into an owned `String`. Odd/non-UTF-8
/// engine input must not panic across the C ABI, so invalid bytes become U+FFFD
/// rather than aborting. Returns an owned copy (not a borrow) because the source
/// pointer is only valid for the duration of the callback.
fn cstr_to_str(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

type CursorClosure = Box<dyn Fn(*mut c_void, CursorType) + Send>;

/// Id of the cursor hook's currently active registration (`0` = none).
static CURSOR_ACTIVE: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn cursor_tramp(user: *mut c_void, view: *mut c_void, cursor_type: i32) {
    crate::panic_guard::guard(|| {
        // Shared `&`: the closure is `Fn`, so a callback that re-enters Noesis
        // (materialising a second reference) is sound.
        let cb = &*user.cast::<CursorClosure>();
        cb(view, CursorType::from_raw(cursor_type));
    })
}

/// Guard for a registered cursor callback. This is a **process-global,
/// single-slot, last-registration-wins** hook (see the module docs): dropping
/// the guard clears the slot only while it is still the active registration,
/// and always frees its own boxed closure.
pub struct CursorCallback {
    user: NonNull<CursorClosure>,
    id: u64,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for CursorCallback {}

impl Drop for CursorCallback {
    fn drop(&mut self) {
        // Only clear the global slot if it still points at THIS registration;
        // otherwise a newer `set_cursor_callback` owns it and must keep firing.
        if CURSOR_ACTIVE
            .compare_exchange(self.id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            unsafe { ffi::noesis_set_cursor_callback(core::ptr::null_mut(), None) };
        }
        unsafe { drop(Box::from_raw(self.user.as_ptr())) };
    }
}

/// Register `f` as the global cursor-update callback. `f` receives the
/// borrowed `Noesis::IView*` (opaque) requesting the change and the desired
/// [`CursorType`]. It must be `Fn` because the callback fires synchronously from
/// input dispatch and may re-enter; use interior mutability for state. Returns a
/// guard; drop it to unregister.
///
/// This hook is **process-global with a single slot**: a later
/// `set_cursor_callback` replaces this one (last-registration-wins), after
/// which dropping this (now older) guard no longer touches the slot. See the
/// module docs for the full semantics.
pub fn set_cursor_callback<F>(f: F) -> CursorCallback
where
    F: Fn(*mut c_void, CursorType) + Send + 'static,
{
    let boxed: Box<CursorClosure> = Box::new(Box::new(f));
    let user = Box::into_raw(boxed);
    let id = next_reg_id();
    // Claim the active id BEFORE writing the FFI slot: a stale guard's drop
    // compare-exchanges against CURSOR_ACTIVE, so publishing the new id first
    // means it can never match (and thus never clobber) this fresh registration.
    // This assumes guard drops are serialized with view-thread callback dispatch
    // (see the module "Lifetime" note).
    CURSOR_ACTIVE.store(id, Ordering::Release);
    // SAFETY: `user` is freshly leaked; trampoline is 'static.
    unsafe { ffi::noesis_set_cursor_callback(user.cast(), Some(cursor_tramp)) };
    CursorCallback {
        user: NonNull::new(user).expect("Box::into_raw returned null"),
        id,
    }
}

type KeyboardClosure = Box<dyn Fn(*mut c_void, bool) + Send>;

/// Id of the keyboard hook's currently active registration (`0` = none).
static KEYBOARD_ACTIVE: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn keyboard_tramp(user: *mut c_void, focused: *mut c_void, open: bool) {
    crate::panic_guard::guard(|| {
        // Shared `&`: the closure is `Fn` (re-entrant-safe).
        let cb = &*user.cast::<KeyboardClosure>();
        cb(focused, open);
    })
}

/// Guard for a registered software-keyboard callback. Process-global,
/// single-slot, last-registration-wins; see [`CursorCallback`] and the module
/// docs.
pub struct SoftwareKeyboardCallback {
    user: NonNull<KeyboardClosure>,
    id: u64,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for SoftwareKeyboardCallback {}

impl Drop for SoftwareKeyboardCallback {
    fn drop(&mut self) {
        if KEYBOARD_ACTIVE
            .compare_exchange(self.id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            unsafe { ffi::noesis_set_software_keyboard_callback(core::ptr::null_mut(), None) };
        }
        unsafe { drop(Box::from_raw(self.user.as_ptr())) };
    }
}

/// Register `f` as the global on-screen-keyboard callback. `f` receives the
/// borrowed `Noesis::UIElement*` (opaque) that has focus and a `bool` that is
/// `true` to open the keyboard, `false` to close it. It must be `Fn` (the
/// callback may re-enter; use interior mutability for state).
///
/// Process-global, single-slot, last-registration-wins; see
/// [`set_cursor_callback`] and the module docs.
pub fn set_software_keyboard_callback<F>(f: F) -> SoftwareKeyboardCallback
where
    F: Fn(*mut c_void, bool) + Send + 'static,
{
    let boxed: Box<KeyboardClosure> = Box::new(Box::new(f));
    let user = Box::into_raw(boxed);
    let id = next_reg_id();
    // Claim the active id before writing the FFI slot (see `set_cursor_callback`).
    KEYBOARD_ACTIVE.store(id, Ordering::Release);
    // SAFETY: `user` is freshly leaked; trampoline is 'static.
    unsafe { ffi::noesis_set_software_keyboard_callback(user.cast(), Some(keyboard_tramp)) };
    SoftwareKeyboardCallback {
        user: NonNull::new(user).expect("Box::into_raw returned null"),
        id,
    }
}

type OpenUrlClosure = Box<dyn Fn(&str) + Send>;

/// Id of the open-URL hook's currently active registration (`0` = none).
static OPEN_URL_ACTIVE: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn open_url_tramp(user: *mut c_void, url: *const c_char) {
    crate::panic_guard::guard(|| {
        // Shared `&`: the closure is `Fn`. `open_url` dispatches synchronously,
        // so a callback that re-enters must not alias a `&mut`.
        let cb = &*user.cast::<OpenUrlClosure>();
        cb(&cstr_to_str(url));
    })
}

/// Guard for a registered open-URL callback. Process-global, single-slot,
/// last-registration-wins; see [`CursorCallback`] and the module docs.
pub struct OpenUrlCallback {
    user: NonNull<OpenUrlClosure>,
    id: u64,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for OpenUrlCallback {}

impl Drop for OpenUrlCallback {
    fn drop(&mut self) {
        if OPEN_URL_ACTIVE
            .compare_exchange(self.id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            unsafe { ffi::noesis_set_open_url_callback(core::ptr::null_mut(), None) };
        }
        unsafe { drop(Box::from_raw(self.user.as_ptr())) };
    }
}

/// Register `f` as the global open-URL callback. `f` receives the URL string
/// whenever the host should open it in a browser. [`open_url`] triggers this
/// synchronously, so `f` must be `Fn` (a re-entrant call must not alias handler
/// state; use interior mutability).
///
/// Process-global, single-slot, last-registration-wins; see
/// [`set_cursor_callback`] and the module docs.
pub fn set_open_url_callback<F>(f: F) -> OpenUrlCallback
where
    F: Fn(&str) + Send + 'static,
{
    let boxed: Box<OpenUrlClosure> = Box::new(Box::new(f));
    let user = Box::into_raw(boxed);
    let id = next_reg_id();
    // Claim the active id before writing the FFI slot (see `set_cursor_callback`).
    OPEN_URL_ACTIVE.store(id, Ordering::Release);
    // SAFETY: `user` is freshly leaked; trampoline is 'static.
    unsafe { ffi::noesis_set_open_url_callback(user.cast(), Some(open_url_tramp)) };
    OpenUrlCallback {
        user: NonNull::new(user).expect("Box::into_raw returned null"),
        id,
    }
}

/// Ask Noesis to open `url`. Invokes the registered open-URL callback
/// synchronously; a no-op if none is registered.
///
/// # Panics
///
/// Panics if `url` contains an interior NUL byte.
pub fn open_url(url: &str) {
    let c = CString::new(url).expect("url contained interior NUL");
    // SAFETY: pointer is valid for the duration of the synchronous call.
    unsafe { ffi::noesis_open_url(c.as_ptr()) };
}

type PlayAudioClosure = Box<dyn Fn(&str, f32) + Send>;

/// Id of the play-audio hook's currently active registration (`0` = none).
static PLAY_AUDIO_ACTIVE: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn play_audio_tramp(user: *mut c_void, uri: *const c_char, volume: f32) {
    crate::panic_guard::guard(|| {
        // Shared `&`: the closure is `Fn`. `play_audio` dispatches synchronously,
        // so a callback that re-enters must not alias a `&mut`.
        let cb = &*user.cast::<PlayAudioClosure>();
        cb(&cstr_to_str(uri), volume);
    })
}

/// Guard for a registered play-audio callback. Process-global, single-slot,
/// last-registration-wins; see [`CursorCallback`] and the module docs.
pub struct PlayAudioCallback {
    user: NonNull<PlayAudioClosure>,
    id: u64,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for PlayAudioCallback {}

impl Drop for PlayAudioCallback {
    fn drop(&mut self) {
        if PLAY_AUDIO_ACTIVE
            .compare_exchange(self.id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            unsafe { ffi::noesis_set_play_audio_callback(core::ptr::null_mut(), None) };
        }
        unsafe { drop(Box::from_raw(self.user.as_ptr())) };
    }
}

/// Register `f` as the global play-audio callback. `f` receives the
/// canonicalized URI string of the sound and a volume in `[0.0, 1.0]`.
/// [`play_audio`] triggers this synchronously, so `f` must be `Fn` (a re-entrant
/// call must not alias handler state; use interior mutability).
///
/// Process-global, single-slot, last-registration-wins; see
/// [`set_cursor_callback`] and the module docs.
pub fn set_play_audio_callback<F>(f: F) -> PlayAudioCallback
where
    F: Fn(&str, f32) + Send + 'static,
{
    let boxed: Box<PlayAudioClosure> = Box::new(Box::new(f));
    let user = Box::into_raw(boxed);
    let id = next_reg_id();
    // Claim the active id before writing the FFI slot (see `set_cursor_callback`).
    PLAY_AUDIO_ACTIVE.store(id, Ordering::Release);
    // SAFETY: `user` is freshly leaked; trampoline is 'static.
    unsafe { ffi::noesis_set_play_audio_callback(user.cast(), Some(play_audio_tramp)) };
    PlayAudioCallback {
        user: NonNull::new(user).expect("Box::into_raw returned null"),
        id,
    }
}

/// Ask Noesis to play the sound at `uri` at `volume`. Invokes the registered
/// play-audio callback synchronously; a no-op if none is registered.
///
/// # Panics
///
/// Panics if `uri` contains an interior NUL byte.
pub fn play_audio(uri: &str, volume: f32) {
    let c = CString::new(uri).expect("uri contained interior NUL");
    // SAFETY: pointer is valid for the duration of the synchronous call.
    unsafe { ffi::noesis_play_audio(c.as_ptr(), volume) };
}

/// Set the default culture by BCP-47 name (e.g. `"en-US"`, `"fr-FR"`). The
/// name backs Noesis's number / currency / date formatting. Round-trips
/// through [`get_culture`]. The C++ shim keeps the name alive in a static
/// buffer for the process lifetime.
///
/// # Panics
///
/// Panics if `name` contains an interior NUL byte.
pub fn set_culture(name: &str) {
    let c = CString::new(name).expect("culture name contained interior NUL");
    // SAFETY: the shim copies the name into process-static storage.
    unsafe { ffi::noesis_set_culture(c.as_ptr()) };
}

/// Return the active default culture's BCP-47 name. Defaults to `"en-US"`
/// before any [`set_culture`] call.
#[must_use]
pub fn get_culture() -> String {
    // SAFETY: the returned pointer is borrowed and stays valid; we copy out.
    let p = unsafe { ffi::noesis_get_culture() };
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}
