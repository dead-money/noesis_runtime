//! FFI to the Noesis GUI Native SDK.
//!
//! Renderer-agnostic — Bevy/wgpu integration lives in the sibling crate
//! `dm_noesis_bevy`. See `../dm_noesis_bevy/CLAUDE.md` for the phase plan.
//!
//! Currently at Phase 0: lifecycle only.
//!
//! # Setup
//!
//! Set `NOESIS_SDK_DIR` to the extracted Noesis Native SDK 3.2.13 root (the
//! directory containing `Include/` and `Bin/`). See `README.md`.

use std::ffi::{CStr, CString};

pub mod animation;
pub mod binding;
pub mod brushes;
pub mod classes;
pub mod commands;
pub mod converters;
pub mod diagnostics;
pub mod drawing;
pub mod events;
pub mod ffi;
pub mod font_provider;
pub mod formatted_text;
pub mod geometry;
pub mod gui;
pub mod imaging;
pub mod input;
pub mod integration;
pub mod markup;
pub mod multi_binding;
pub mod name_scope;
pub mod plain_vm;
pub mod reflection;
pub mod render_device;
pub mod resources;
pub mod shapes;
pub mod styles;
pub mod svg;
pub mod text_inlines;
pub mod texture_provider;
pub mod transforms;
pub mod typography;
pub mod view;
pub mod xaml;
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

/// Disable the Hot Reload feature before [`init`]. Hot Reload is on by default
/// in Debug/Profile SDK builds and costs a little extra memory; disabling it is
/// purely an optimization. No-op once [`init`] has run, and a no-op on a
/// Release dylib where the feature is compiled out.
///
/// Part of the inspector / hot-reload control surface (see
/// [`disable_inspector`], [`disable_socket_init`], [`is_inspector_connected`],
/// [`update_inspector`]). There is intentionally no `enable_*` counterpart:
/// these features default on in instrumented SDK builds, so we only expose the
/// off switches plus the runtime queries.
///
/// Must be called **before** [`init`].
pub fn disable_hot_reload() {
    // SAFETY: a pre-init GUI:: free call with no arguments or preconditions
    // beyond "call before Init", which is the caller's contract.
    unsafe { ffi::dm_noesis_disable_hot_reload() }
}

/// Skip the Inspector's socket initialization (e.g. `WSAStartup` on Windows)
/// before [`init`]. Use this only when the host process has already initialized
/// sockets itself, to avoid a double init. No-op after [`init`] / on a Release
/// dylib.
///
/// Must be called **before** [`init`].
pub fn disable_socket_init() {
    // SAFETY: pre-init GUI:: free call; see `disable_hot_reload`.
    unsafe { ffi::dm_noesis_disable_socket_init() }
}

/// Disable all remote Inspector connections before [`init`]. The Inspector is
/// enabled by default in Debug/Profile SDK builds (it opens a socket and waits
/// for the remote tool); call this to keep it off. No-op after [`init`] / on a
/// Release dylib where the Inspector is compiled out.
///
/// Must be called **before** [`init`].
pub fn disable_inspector() {
    // SAFETY: pre-init GUI:: free call; see `disable_hot_reload`.
    unsafe { ffi::dm_noesis_disable_inspector() }
}

/// Returns whether a remote Inspector is currently connected.
///
/// Always `false` when nothing is attached, and always `false` on a Release
/// dylib (the Inspector is compiled out of Release SDK builds). The value of
/// exposing it is the query itself plus the [`update_inspector`] pump for hosts
/// running an instrumented build.
#[must_use]
pub fn is_inspector_connected() -> bool {
    // SAFETY: runtime GUI:: query; safe to call any time, returns false if the
    // Inspector subsystem is absent.
    unsafe { ffi::dm_noesis_is_inspector_connected() }
}

/// Keep the Inspector connection alive. [`crate::view::View`] updates call this
/// internally, so it is only needed when the Inspector connects before any view
/// exists. No-op on a Release dylib.
pub fn update_inspector() {
    // SAFETY: runtime GUI:: call; safe to call any time (no-op without an
    // active Inspector connection).
    unsafe { ffi::dm_noesis_update_inspector() }
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

/// Returns the Noesis runtime build version (e.g. `"3.2.13"`).
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
