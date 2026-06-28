//! Register Rust-backed XAML `MarkupExtension`s (Phase 5.D).
//!
//! Mirrors the C# / Unity binding's approach: a per-base C++ trampoline
//! ([`MarkupExtension`](https://docs.noesisengine.com/...) on the C++ side)
//! plus a synthetic per-name `TypeClassBuilder` so XAML can resolve
//! `{myns:Foo positional_arg}` to a Rust callback.
//!
//! AoR's `LocalizeExtension` is the motivating example —
//! `{aor:Localize menu.main_menu.new_game}` resolves the key through a
//! locale table and substitutes the result.
//!
//! # v1 scope
//!
//! * Single positional `Key` argument (the bit between `{name ` and `}`).
//! * Callback returns either a `&str` (most common) or a borrowed
//!   `BaseComponent*`.
//! * No reactive bindings — the callback runs at XAML parse time and the
//!   returned value is substituted statically. Locale switching requires
//!   re-loading the XAML. Reactive bindings (locale switch updates the UI
//!   in place) follow in a separate PR.
//!
//! # Threading
//!
//! Callbacks fire from inside Noesis's XAML parser, on whichever thread
//! triggered the load. In a Bevy app that's the render thread (which
//! drives the View). The handler is `Send`; mutations to Bevy ECS state
//! should be queued and processed on the main thread.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface — explicit blocks add noise

use core::ffi::CStr;
use core::ptr::NonNull;
use std::ffi::{CString, c_char, c_void};
use std::sync::Mutex;

use crate::ffi::{dm_noesis_markup_extension_register, dm_noesis_markup_extension_unregister};

/// Value returned from a [`MarkupExtensionHandler::provide_value`] callback.
/// `String` is the common case; `Component` covers handlers that resolve
/// to an existing Noesis object (e.g. a resource from the application
/// resource dictionary).
pub enum MarkupValue<'a> {
    String(&'a str),
    /// Borrowed `Noesis::BaseComponent*`. Caller does not consume a ref;
    /// the C++ trampoline adds one when constructing the returned `Ptr`.
    Component(NonNull<c_void>),
    /// Signals "no value" — Noesis substitutes `BaseComponent::GetUnsetValue()`
    /// (which the parser interprets as "leave the property at its default").
    Unset,
}

/// Per-extension callback. Receives the positional `key` argument the XAML
/// parser populated and returns a [`MarkupValue`] for Noesis to substitute.
pub trait MarkupExtensionHandler: Send + 'static {
    fn provide_value(&mut self, key: &str) -> MarkupValue<'_>;
}

/// Convenience: closures that return `String` work as handlers. The
/// returned `String` is held by a per-handler scratch slot for the
/// duration of the callback (Noesis copies the bytes immediately).
impl<F> MarkupExtensionHandler for ClosureHandler<F>
where
    F: FnMut(&str) -> Option<String> + Send + 'static,
{
    fn provide_value(&mut self, key: &str) -> MarkupValue<'_> {
        match (self.f)(key) {
            Some(s) => {
                self.scratch = s;
                MarkupValue::String(&self.scratch)
            }
            None => MarkupValue::Unset,
        }
    }
}

/// Adapter newtype so `FnMut(&str) -> Option<String>` can satisfy the
/// trait without colliding with future blanket impls. Construct via
/// [`MarkupExtensionRegistration::from_closure`].
pub struct ClosureHandler<F: FnMut(&str) -> Option<String> + Send + 'static> {
    f: F,
    scratch: String,
}

/// RAII handle for a registered MarkupExtension. Drop unregisters from
/// the Factory + Reflection registries — preventing new instances from
/// being parsed — but the underlying MarkupClassData and the boxed
/// handler survive as long as live extension instances remain. Same
/// intrusive-refcount contract as [`crate::classes::ClassRegistration`].
pub struct MarkupExtensionRegistration {
    token: NonNull<c_void>,
    _name: CString,
}

// SAFETY: the boxed handler is Send; the C++ side serializes registry
// access via its own mutex.
unsafe impl Send for MarkupExtensionRegistration {}
unsafe impl Sync for MarkupExtensionRegistration {}

impl MarkupExtensionRegistration {
    /// Register a Rust-backed MarkupExtension. `name` is the XAML-visible
    /// type (e.g. `"AOR.Localize"`); the namespace mapping
    /// (`xmlns:aor="clr-namespace:AOR"`) lives in the XAML.
    ///
    /// Returns `None` when the C++ side rejects (most commonly: name
    /// already registered).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL.
    pub fn new<H: MarkupExtensionHandler>(name: &str, handler: H) -> Option<Self> {
        let cname = CString::new(name).expect("extension name contained NUL");
        let boxed: Box<Box<dyn MarkupExtensionHandler>> = Box::new(Box::new(handler));
        let userdata = Box::into_raw(boxed);

        let token = unsafe {
            dm_noesis_markup_extension_register(
                cname.as_ptr(),
                provide_trampoline,
                userdata.cast(),
                markup_handler_free_trampoline,
            )
        };
        let Some(token) = NonNull::new(token) else {
            // Registration rejected before C++ took ownership of the box.
            unsafe { drop(Box::from_raw(userdata)) };
            return None;
        };

        Some(Self {
            token,
            _name: cname,
        })
    }

    /// Convenience: register an extension whose body is a single closure
    /// returning the localized / resolved string for a key.
    /// Returning `None` from the closure produces a `MarkupValue::Unset`.
    pub fn from_closure<F>(name: &str, f: F) -> Option<Self>
    where
        F: FnMut(&str) -> Option<String> + Send + 'static,
    {
        let handler = ClosureHandler {
            f,
            scratch: String::new(),
        };
        Self::new(name, handler)
    }

    /// Internal token (a `void*` to the C++-side ClassData).
    pub fn token(&self) -> NonNull<c_void> {
        self.token
    }
}

impl Drop for MarkupExtensionRegistration {
    fn drop(&mut self) {
        // The C++ side owns the boxed handler. `dm_noesis_markup_extension_unregister`
        // releases the Rust caller's MarkupClassData ref — if no extension
        // instances are alive, ClassData self-destructs immediately and
        // `markup_handler_free_trampoline` runs to drop the handler box;
        // otherwise the deferred free runs when the last instance dies.
        //
        // SAFETY: `self.token` was produced by `new` and is freed exactly
        // once here.
        unsafe { dm_noesis_markup_extension_unregister(self.token.as_ptr()) };
    }
}

/// Free trampoline matching [`crate::ffi::MarkupFreeFn`]. Drops the
/// `Box<Box<dyn MarkupExtensionHandler>>` whose ownership was transferred
/// to C++ at register time, plus the per-handler scratch slot.
unsafe extern "C" fn markup_handler_free_trampoline(userdata: *mut c_void) {
    if userdata.is_null() {
        return;
    }
    forget_string_scratch(userdata);
    // SAFETY: `userdata` is the `Box::into_raw` from `new`. Single-owner
    // contract; C++ calls this exactly once.
    drop(Box::from_raw(
        userdata as *mut Box<dyn MarkupExtensionHandler>,
    ));
}

// ── Trampoline ─────────────────────────────────────────────────────────────

unsafe extern "C" fn provide_trampoline(
    userdata: *mut c_void,
    key: *const c_char,
    out_string: *mut *const c_char,
    out_component: *mut *mut c_void,
) -> bool {
    let handler = &mut *userdata.cast::<Box<dyn MarkupExtensionHandler>>();
    let key_str = if key.is_null() {
        ""
    } else {
        CStr::from_ptr(key).to_str().unwrap_or("")
    };

    *out_string = core::ptr::null();
    *out_component = core::ptr::null_mut();

    match handler.provide_value(key_str) {
        MarkupValue::Unset => false,
        MarkupValue::Component(ptr) => {
            *out_component = ptr.as_ptr();
            true
        }
        MarkupValue::String(s) => {
            // The borrowed &str must remain valid for the C++ side to copy
            // the bytes into Noesis's String storage. The string returned by
            // `provide_value` borrows from the handler — typically scratch
            // storage on the handler itself, which the C++ side copies before
            // the next callback or any further handler mutation. Stash a
            // CString in a per-handler slot so the trailing NUL is in place.
            let cstring = CString::new(s.as_bytes()).unwrap_or_default();
            let key = userdata as usize;
            let mut table = STRING_SCRATCH.lock().expect("STRING_SCRATCH poisoned");
            // Replace any prior scratch for this handler — the C++ side
            // copies bytes synchronously, so the previous slot can go.
            let slot = table.iter_mut().find(|(k, _)| *k == key);
            let cstr_ptr = match slot {
                Some(slot) => {
                    slot.1 = cstring;
                    slot.1.as_ptr()
                }
                None => {
                    table.push((key, cstring));
                    table.last().expect("just pushed").1.as_ptr()
                }
            };
            *out_string = cstr_ptr;
            true
        }
    }
}

// Per-handler CString scratch. Handlers borrow &str into Rust-owned data;
// we need a stable C-string slot for the bytes Noesis sees so the trailing
// NUL is in place. Keyed by handler userdata pointer (unique per
// registration); cleaned up when the registration drops.
static STRING_SCRATCH: Mutex<Vec<(usize, CString)>> = Mutex::new(Vec::new());

fn forget_string_scratch(userdata: *mut c_void) {
    let key = userdata as usize;
    let mut table = STRING_SCRATCH.lock().expect("STRING_SCRATCH poisoned");
    table.retain(|(k, _)| *k != key);
}
