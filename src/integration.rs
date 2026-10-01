//! Process-global host hooks from `NsGui/IntegrationAPI.h`: Noesis calls back
//! into your app to change the OS cursor, show or hide the on-screen keyboard,
//! open a URL, or play a sound. The default culture is set here too
//! ([`set_culture`]).
//!
//! Each `set_*` function boxes your closure, registers it with Noesis, and
//! returns a guard. Dropping the guard unregisters the hook.
//!
//! # One slot per hook
//!
//! Each hook has exactly one process-wide slot. Calling a `set_*` function
//! again replaces the previous registration, and the older guard goes inert:
//!
//! - Dropping an older, replaced guard leaves the slot alone, so the newer
//!   registration keeps firing.
//! - Dropping the active guard clears the slot. A previously replaced
//!   registration is not restored.
//!
//! Each guard frees only its own closure, so any drop order is safe.
//!
//! # When callbacks fire
//!
//! [`open_url`] and [`play_audio`] call the registered closure synchronously.
//! The cursor callback fires from a view's input handling, for example when
//! the mouse moves over an element with a non-default `Cursor`. The
//! software-keyboard callback fires when an element that wants a virtual
//! keyboard gains or loses focus, on platforms that request one.
//!
//! # Lifetime and threading
//!
//! Keep a guard alive until after [`crate::shutdown`] returns, or drop it
//! earlier to unregister. Drop guards on the thread that drives the views, or
//! otherwise serialize the drop with callback dispatch: freeing a closure
//! while a callback for the same hook is running is a data race.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface; explicit blocks add noise

use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};
use std::os::raw::c_char;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::ffi;

// Ids start at 1; 0 means "no active registration" in the per-hook atomics.
// A guard's Drop clears the FFI slot only while its id is still the active one.
static NEXT_REG_ID: AtomicU64 = AtomicU64::new(1);

fn next_reg_id() -> u64 {
    NEXT_REG_ID.fetch_add(1, Ordering::Relaxed)
}

/// Built-in cursor types (`Noesis::CursorType` in `NsGui/Cursor.h`), passed to
/// the [`set_cursor_callback`] closure. Discriminants match the C++ enum.
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
    /// Maps a raw `Noesis::CursorType` value. Unknown values, including the
    /// `CursorType_Count` sentinel, map to [`CursorType::None`].
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

// Lossy: a panic here would cross the C ABI. Owned because the pointer is only
// valid during the callback.
fn cstr_to_str(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

type CursorClosure = Box<dyn Fn(*mut c_void, CursorType) + Send>;

static CURSOR_ACTIVE: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn cursor_tramp(user: *mut c_void, view: *mut c_void, cursor_type: i32) {
    crate::panic_guard::guard(|| {
        // Shared `&`: the closure is `Fn`, so a callback that re-enters Noesis
        // (materialising a second reference) is sound.
        let cb = &*user.cast::<CursorClosure>();
        cb(view, CursorType::from_raw(cursor_type));
    })
}

/// Guard returned by [`set_cursor_callback`]. Dropping it unregisters the
/// callback if it is still the active one (see [one slot per
/// hook](self#one-slot-per-hook)).
pub struct CursorCallback {
    user: NonNull<CursorClosure>,
    id: u64,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for CursorCallback {}

impl Drop for CursorCallback {
    fn drop(&mut self) {
        // A newer registration may own the slot; leave it firing.
        if CURSOR_ACTIVE
            .compare_exchange(self.id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            unsafe { ffi::noesis_set_cursor_callback(core::ptr::null_mut(), None) };
        }
        unsafe { drop(Box::from_raw(self.user.as_ptr())) };
    }
}

/// Registers `f` as the cursor-change callback, replacing any previous one.
/// Drop the returned guard to unregister.
///
/// `f` receives the borrowed `Noesis::IView*` asking for the change and the
/// requested [`CursorType`]. It runs synchronously during input dispatch and
/// may re-enter, so it is `Fn`; keep mutable state behind interior
/// mutability. See [one slot per hook](self#one-slot-per-hook).
pub fn set_cursor_callback<F>(f: F) -> CursorCallback
where
    F: Fn(*mut c_void, CursorType) + Send + 'static,
{
    let boxed: Box<CursorClosure> = Box::new(Box::new(f));
    let user = Box::into_raw(boxed);
    let id = next_reg_id();
    // Publish the id before writing the FFI slot so a stale guard's drop can't
    // clear this registration. Assumes drops are serialized with dispatch.
    CURSOR_ACTIVE.store(id, Ordering::Release);
    // SAFETY: `user` is freshly leaked; trampoline is 'static.
    unsafe { ffi::noesis_set_cursor_callback(user.cast(), Some(cursor_tramp)) };
    CursorCallback {
        user: NonNull::new(user).expect("Box::into_raw returned null"),
        id,
    }
}

type KeyboardClosure = Box<dyn Fn(*mut c_void, bool) + Send>;

static KEYBOARD_ACTIVE: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn keyboard_tramp(user: *mut c_void, focused: *mut c_void, open: bool) {
    crate::panic_guard::guard(|| {
        // Shared `&`: the closure is `Fn` (re-entrant-safe).
        let cb = &*user.cast::<KeyboardClosure>();
        cb(focused, open);
    })
}

/// Guard returned by [`set_software_keyboard_callback`]. Dropping it
/// unregisters the callback if it is still the active one (see [one slot per
/// hook](self#one-slot-per-hook)).
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

/// Registers `f` as the on-screen-keyboard callback, replacing any previous
/// one. Drop the returned guard to unregister.
///
/// `f` receives the borrowed `Noesis::UIElement*` that has focus and `true` to
/// open the keyboard or `false` to close it. It may re-enter, so it is `Fn`.
/// See [one slot per hook](self#one-slot-per-hook).
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

static OPEN_URL_ACTIVE: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn open_url_tramp(user: *mut c_void, url: *const c_char) {
    crate::panic_guard::guard(|| {
        // Shared `&`: the closure is `Fn`. `open_url` dispatches synchronously,
        // so a callback that re-enters must not alias a `&mut`.
        let cb = &*user.cast::<OpenUrlClosure>();
        cb(&cstr_to_str(url));
    })
}

/// Guard returned by [`set_open_url_callback`]. Dropping it unregisters the
/// callback if it is still the active one (see [one slot per
/// hook](self#one-slot-per-hook)).
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

/// Registers `f` as the open-URL callback, replacing any previous one. Drop
/// the returned guard to unregister.
///
/// `f` receives the URL the host should open, for example in a browser.
/// [`open_url`] calls it synchronously and it may re-enter, so it is `Fn`.
/// Non-UTF-8 bytes are replaced with U+FFFD. See [one slot per
/// hook](self#one-slot-per-hook).
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

/// Asks Noesis to open `url`, which calls the registered open-URL callback
/// synchronously. Does nothing if no callback is registered.
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

static PLAY_AUDIO_ACTIVE: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn play_audio_tramp(user: *mut c_void, uri: *const c_char, volume: f32) {
    crate::panic_guard::guard(|| {
        // Shared `&`: the closure is `Fn`. `play_audio` dispatches synchronously,
        // so a callback that re-enters must not alias a `&mut`.
        let cb = &*user.cast::<PlayAudioClosure>();
        cb(&cstr_to_str(uri), volume);
    })
}

/// Guard returned by [`set_play_audio_callback`]. Dropping it unregisters the
/// callback if it is still the active one (see [one slot per
/// hook](self#one-slot-per-hook)).
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

/// Registers `f` as the play-audio callback, replacing any previous one. Drop
/// the returned guard to unregister.
///
/// `f` receives the sound's URI and a volume in `[0.0, 1.0]`. [`play_audio`]
/// calls it synchronously and it may re-enter, so it is `Fn`. See [one slot per
/// hook](self#one-slot-per-hook).
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

/// Asks Noesis to play the sound at `uri` at `volume`, which calls the
/// registered play-audio callback synchronously. Does nothing if no callback
/// is registered.
///
/// # Panics
///
/// Panics if `uri` contains an interior NUL byte.
pub fn play_audio(uri: &str, volume: f32) {
    let c = CString::new(uri).expect("uri contained interior NUL");
    // SAFETY: pointer is valid for the duration of the synchronous call.
    unsafe { ffi::noesis_play_audio(c.as_ptr(), volume) };
}

/// Sets the default culture by BCP-47 name (e.g. `"en-US"`, `"fr-FR"`), read
/// back by [`get_culture`]. Only the name changes; the number format keeps
/// Noesis's defaults.
///
/// # Panics
///
/// Panics if `name` contains an interior NUL byte.
pub fn set_culture(name: &str) {
    let c = CString::new(name).expect("culture name contained interior NUL");
    // SAFETY: the shim copies the name into process-static storage.
    unsafe { ffi::noesis_set_culture(c.as_ptr()) };
}

/// The default culture's BCP-47 name; `"en-US"` until [`set_culture`] is
/// called.
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
