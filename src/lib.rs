//! FFI to the Noesis GUI Native SDK.
//!
//! Renderer-agnostic — Bevy/wgpu integration lives in the sibling crate
//! `dm_noesis_bevy`. See `../dm_noesis_bevy/CLAUDE.md` for the phase plan.
//!
//! Currently at Phase 0: lifecycle only.
//!
//! # Setup
//!
//! Set `NOESIS_SDK_DIR` to the extracted Noesis Native SDK 3.2.12 root (the
//! directory containing `Include/` and `Bin/`). See `README.md`.

use std::ffi::{CStr, CString};

pub mod classes;
pub mod events;
pub mod ffi;
pub mod font_provider;
pub mod gui;
pub mod markup;
pub mod render_device;
pub mod texture_provider;
pub mod view;
pub mod xaml_provider;

/// Optional. Apply Indie license credentials before [`init`] to suppress the
/// trial watermark. Pass empty strings (or skip the call) to run in trial mode.
///
/// # Panics
///
/// Panics if `name` or `key` contain interior NUL bytes.
pub fn set_license(name: &str, key: &str) {
    let n = CString::new(name).expect("license name contained NUL");
    let k = CString::new(key).expect("license key contained NUL");
    // SAFETY: pointers live for the duration of the call; the shim copies into Noesis.
    unsafe { ffi::dm_noesis_set_license(n.as_ptr(), k.as_ptr()) }
}

/// Initialize Noesis subsystems. Call exactly once per process; Noesis does
/// not support re-init after [`shutdown`].
pub fn init() {
    // SAFETY: no preconditions other than "call once" — documented by Noesis.
    unsafe { ffi::dm_noesis_init() }
}

/// Shut Noesis down. Call once at process exit, after all Noesis-owned objects
/// have been released.
pub fn shutdown() {
    // SAFETY: caller responsibility per docs.
    unsafe { ffi::dm_noesis_shutdown() }
}

/// Returns the Noesis runtime build version (e.g. `"3.2.12"`).
#[must_use]
pub fn version() -> String {
    // SAFETY: version string is owned by the Noesis runtime and stays valid for
    // the lifetime of the process; we copy it into an owned String.
    let p = unsafe { ffi::dm_noesis_version() };
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}
