//! System integration callbacks from `NsGui/IntegrationAPI.h` (namespace
//! `Noesis::GUI`). These are **process-global** host hooks — not per-view —
//! that let the host react to engine requests: update the OS cursor, show or
//! hide the on-screen keyboard, open a URL, play a sound, and set the default
//! culture.
//!
//! # Pattern
//!
//! Each `set_*` registration boxes a Rust closure and hands a thin pointer to
//! it across the FFI as Noesis's `void* user`, alongside an `extern "C"`
//! trampoline. The C++ shim ([`cpp/noesis_integration.cpp`]) keeps a single
//! static `(user, callback)` slot per hook and registers its own translating
//! trampoline with Noesis (converting `Cursor*` → [`CursorType`], `const
//! Uri&` → `&str`). The returned `*Callback` guard owns the boxed closure and
//! clears the registration on `Drop`, mirroring the Registered-guard idiom in
//! [`crate::texture_provider`] / [`crate::font_provider`].
//!
//! # Triggering
//!
//! [`open_url`] and [`play_audio`] invoke the registered callback
//! **synchronously** — they are genuine end-to-end round trips and are tested
//! as such. The cursor and software-keyboard callbacks fire from input /
//! focus handling deep inside a live view's event pump and are not reachable
//! headlessly (see the note in `tests/integration.rs` and "Known SDK
//! limitations"); we still verify their registration / unregistration FFI
//! crossing is sound.
//!
//! # Lifetime
//!
//! A guard must outlive every Noesis-internal reference that might call back
//! into the closure. Keep it alive until after [`crate::shutdown`] returns (or
//! until you explicitly drop it to unregister).

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface — explicit blocks add noise

use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};
use std::os::raw::c_char;

use crate::ffi;

// ────────────────────────────────────────────────────────────────────────────
// CursorType
// ────────────────────────────────────────────────────────────────────────────

/// Built-in cursor types, mirroring `Noesis::CursorType` in
/// `NsGui/Cursor.h`. The discriminants match the C++ enum exactly so the
/// value read back from a live `Cursor` round-trips.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(i32)]
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

// ────────────────────────────────────────────────────────────────────────────
// Shared helpers
// ────────────────────────────────────────────────────────────────────────────

fn cstr_to_str<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(p) }
            .to_str()
            .expect("noesis passed non-UTF-8 string to an integration callback")
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Cursor callback
// ────────────────────────────────────────────────────────────────────────────

type CursorClosure = Box<dyn FnMut(*mut c_void, CursorType) + Send>;

unsafe extern "C" fn cursor_tramp(user: *mut c_void, view: *mut c_void, cursor_type: i32) {
    let cb = &mut *user.cast::<CursorClosure>();
    cb(view, CursorType::from_raw(cursor_type));
}

/// Guard for a registered cursor callback. Drop unregisters and frees the
/// boxed closure.
pub struct CursorCallback {
    user: NonNull<CursorClosure>,
}

// SAFETY: the boxed closure is `Send`; Noesis serialises callback dispatch
// per global hook. Mirrors the provider `Registered` guards.
unsafe impl Send for CursorCallback {}
unsafe impl Sync for CursorCallback {}

impl Drop for CursorCallback {
    fn drop(&mut self) {
        unsafe {
            ffi::dm_noesis_set_cursor_callback(core::ptr::null_mut(), None);
            drop(Box::from_raw(self.user.as_ptr()));
        }
    }
}

/// Register `f` as the global cursor-update callback. `f` receives the
/// borrowed `Noesis::IView*` (opaque) requesting the change and the desired
/// [`CursorType`]. Returns a guard; drop it to unregister.
pub fn set_cursor_callback<F>(f: F) -> CursorCallback
where
    F: FnMut(*mut c_void, CursorType) + Send + 'static,
{
    let boxed: Box<CursorClosure> = Box::new(Box::new(f));
    let user = Box::into_raw(boxed);
    // SAFETY: `user` is freshly leaked; trampoline is 'static.
    unsafe { ffi::dm_noesis_set_cursor_callback(user.cast(), Some(cursor_tramp)) };
    CursorCallback {
        user: NonNull::new(user).expect("Box::into_raw returned null"),
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Software keyboard callback
// ────────────────────────────────────────────────────────────────────────────

type KeyboardClosure = Box<dyn FnMut(*mut c_void, bool) + Send>;

unsafe extern "C" fn keyboard_tramp(user: *mut c_void, focused: *mut c_void, open: bool) {
    let cb = &mut *user.cast::<KeyboardClosure>();
    cb(focused, open);
}

/// Guard for a registered software-keyboard callback.
pub struct SoftwareKeyboardCallback {
    user: NonNull<KeyboardClosure>,
}

// SAFETY: see [`CursorCallback`].
unsafe impl Send for SoftwareKeyboardCallback {}
unsafe impl Sync for SoftwareKeyboardCallback {}

impl Drop for SoftwareKeyboardCallback {
    fn drop(&mut self) {
        unsafe {
            ffi::dm_noesis_set_software_keyboard_callback(core::ptr::null_mut(), None);
            drop(Box::from_raw(self.user.as_ptr()));
        }
    }
}

/// Register `f` as the global on-screen-keyboard callback. `f` receives the
/// borrowed `Noesis::UIElement*` (opaque) that has focus and a `bool` that is
/// `true` to open the keyboard, `false` to close it.
pub fn set_software_keyboard_callback<F>(f: F) -> SoftwareKeyboardCallback
where
    F: FnMut(*mut c_void, bool) + Send + 'static,
{
    let boxed: Box<KeyboardClosure> = Box::new(Box::new(f));
    let user = Box::into_raw(boxed);
    // SAFETY: `user` is freshly leaked; trampoline is 'static.
    unsafe { ffi::dm_noesis_set_software_keyboard_callback(user.cast(), Some(keyboard_tramp)) };
    SoftwareKeyboardCallback {
        user: NonNull::new(user).expect("Box::into_raw returned null"),
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Open-URL callback + trigger
// ────────────────────────────────────────────────────────────────────────────

type OpenUrlClosure = Box<dyn FnMut(&str) + Send>;

unsafe extern "C" fn open_url_tramp(user: *mut c_void, url: *const c_char) {
    let cb = &mut *user.cast::<OpenUrlClosure>();
    cb(cstr_to_str(url));
}

/// Guard for a registered open-URL callback.
pub struct OpenUrlCallback {
    user: NonNull<OpenUrlClosure>,
}

// SAFETY: see [`CursorCallback`].
unsafe impl Send for OpenUrlCallback {}
unsafe impl Sync for OpenUrlCallback {}

impl Drop for OpenUrlCallback {
    fn drop(&mut self) {
        unsafe {
            ffi::dm_noesis_set_open_url_callback(core::ptr::null_mut(), None);
            drop(Box::from_raw(self.user.as_ptr()));
        }
    }
}

/// Register `f` as the global open-URL callback. `f` receives the URL string
/// whenever the host should open it in a browser. [`open_url`] triggers this
/// synchronously.
pub fn set_open_url_callback<F>(f: F) -> OpenUrlCallback
where
    F: FnMut(&str) + Send + 'static,
{
    let boxed: Box<OpenUrlClosure> = Box::new(Box::new(f));
    let user = Box::into_raw(boxed);
    // SAFETY: `user` is freshly leaked; trampoline is 'static.
    unsafe { ffi::dm_noesis_set_open_url_callback(user.cast(), Some(open_url_tramp)) };
    OpenUrlCallback {
        user: NonNull::new(user).expect("Box::into_raw returned null"),
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
    unsafe { ffi::dm_noesis_open_url(c.as_ptr()) };
}

// ────────────────────────────────────────────────────────────────────────────
// Play-audio callback + trigger
// ────────────────────────────────────────────────────────────────────────────

type PlayAudioClosure = Box<dyn FnMut(&str, f32) + Send>;

unsafe extern "C" fn play_audio_tramp(user: *mut c_void, uri: *const c_char, volume: f32) {
    let cb = &mut *user.cast::<PlayAudioClosure>();
    cb(cstr_to_str(uri), volume);
}

/// Guard for a registered play-audio callback.
pub struct PlayAudioCallback {
    user: NonNull<PlayAudioClosure>,
}

// SAFETY: see [`CursorCallback`].
unsafe impl Send for PlayAudioCallback {}
unsafe impl Sync for PlayAudioCallback {}

impl Drop for PlayAudioCallback {
    fn drop(&mut self) {
        unsafe {
            ffi::dm_noesis_set_play_audio_callback(core::ptr::null_mut(), None);
            drop(Box::from_raw(self.user.as_ptr()));
        }
    }
}

/// Register `f` as the global play-audio callback. `f` receives the
/// canonicalized URI string of the sound and a volume in `[0.0, 1.0]`.
/// [`play_audio`] triggers this synchronously.
pub fn set_play_audio_callback<F>(f: F) -> PlayAudioCallback
where
    F: FnMut(&str, f32) + Send + 'static,
{
    let boxed: Box<PlayAudioClosure> = Box::new(Box::new(f));
    let user = Box::into_raw(boxed);
    // SAFETY: `user` is freshly leaked; trampoline is 'static.
    unsafe { ffi::dm_noesis_set_play_audio_callback(user.cast(), Some(play_audio_tramp)) };
    PlayAudioCallback {
        user: NonNull::new(user).expect("Box::into_raw returned null"),
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
    unsafe { ffi::dm_noesis_play_audio(c.as_ptr(), volume) };
}

// ────────────────────────────────────────────────────────────────────────────
// Culture
// ────────────────────────────────────────────────────────────────────────────

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
    unsafe { ffi::dm_noesis_set_culture(c.as_ptr()) };
}

/// Return the active default culture's BCP-47 name. Defaults to `"en-US"`
/// before any [`set_culture`] call.
#[must_use]
pub fn get_culture() -> String {
    // SAFETY: the returned pointer is borrowed and stays valid; we copy out.
    let p = unsafe { ffi::dm_noesis_get_culture() };
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}
