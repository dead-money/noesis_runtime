//! Custom XAML markup extensions backed by Rust callbacks.
//!
//! A registered extension lets XAML resolve a value through your code while it
//! parses, written as `{prefix:Name argument}`. Localization is the typical use:
//!
//! ```no_run
//! use noesis_runtime::markup::MarkupExtensionRegistration;
//!
//! // XAML: xmlns:l="clr-namespace:MyGame"  ...  Text="{l:Localize menu.new_game}"
//! let _localize = MarkupExtensionRegistration::from_closure("MyGame.Localize", |key| {
//!     Some(format!("[{key}]"))
//! })
//! .expect("name not yet registered");
//! ```
//!
//! # Limitations
//!
//! * One positional string argument, the text between the extension name and
//!   the closing `}`.
//! * The callback returns a string or an existing Noesis object.
//! * The value is resolved once, at parse time. Nothing updates afterwards; to
//!   switch locale, reload the XAML.
//!
//! # Threading
//!
//! Callbacks run inside the XAML parser, on whichever thread started the load.
//! The handler must be `Send`.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface; explicit blocks add noise

use core::ffi::CStr;
use core::ptr::NonNull;
use std::ffi::{CString, c_char, c_void};
use std::sync::Mutex;

use crate::ffi::{noesis_markup_extension_register, noesis_markup_extension_unregister};

/// The value a [`MarkupExtensionHandler`] provides for one use of the
/// extension.
pub enum MarkupValue<'a> {
    /// A string. Noesis copies it, so it only has to live until
    /// `provide_value` returns. A string with an interior NUL byte is treated as
    /// [`Unset`](Self::Unset).
    String(&'a str),
    /// A borrowed `Noesis::BaseComponent*`, such as an existing resource.
    /// Noesis takes its own reference; yours is not consumed.
    Component(NonNull<c_void>),
    /// No value; the target property keeps its default.
    Unset,
}

/// The logic behind a markup extension. For a closure, use
/// [`MarkupExtensionRegistration::from_closure`] instead.
///
/// # Re-entrancy
///
/// `provide_value` takes `&mut self` so a returned string can borrow from the
/// handler. Do not start a nested XAML load that uses this same extension from
/// inside `provide_value` (for example through
/// [`crate::xaml::load_xaml_component`]); that would alias the handler. Several
/// uses in one document are resolved one after another, which is fine.
pub trait MarkupExtensionHandler: Send + 'static {
    /// Returns the value for one use of the extension. `key` is the positional
    /// argument, or `""` if it was absent or not valid UTF-8.
    fn provide_value(&mut self, key: &str) -> MarkupValue<'_>;
}

/// `None` from the closure becomes [`MarkupValue::Unset`].
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

/// A closure adapted to [`MarkupExtensionHandler`]. Created by
/// [`MarkupExtensionRegistration::from_closure`].
pub struct ClosureHandler<F: FnMut(&str) -> Option<String> + Send + 'static> {
    f: F,
    scratch: String,
}

/// Keeps a markup extension registered. Dropping it stops XAML parsed
/// afterwards from using the extension; the handler is freed once no extension
/// instance remains alive. The type name stays registered with Noesis
/// reflection, so the same name cannot be registered again in this process.
#[must_use = "dropping the guard immediately clears the registration"]
pub struct MarkupExtensionRegistration {
    token: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for MarkupExtensionRegistration {}

impl MarkupExtensionRegistration {
    /// Registers `handler` as the extension type `name`, such as
    /// `"MyGame.Localize"`. XAML reaches it through a namespace mapping like
    /// `xmlns:l="clr-namespace:MyGame"`.
    ///
    /// Returns `None` if a type named `name` is already registered.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL.
    pub fn new<H: MarkupExtensionHandler>(name: &str, handler: H) -> Option<Self> {
        let cname = CString::new(name).expect("extension name contained NUL");
        let boxed: Box<Box<dyn MarkupExtensionHandler>> = Box::new(Box::new(handler));
        let userdata = Box::into_raw(boxed);

        let token = unsafe {
            noesis_markup_extension_register(
                cname.as_ptr(),
                provide_trampoline,
                userdata.cast(),
                markup_handler_free_trampoline,
            )
        };
        let Some(token) = NonNull::new(token) else {
            // A null token means C++ rejected the registration and, per the
            // contract, left ownership of the box with us. Free it here.
            unsafe { drop(Box::from_raw(userdata)) };
            return None;
        };

        Some(Self { token })
    }

    /// Registers a closure that maps the positional argument to a string.
    /// Returning `None` leaves the target property at its default. Fails and
    /// panics under the same conditions as [`Self::new`].
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

    /// Opaque registration token used by the C shim.
    pub fn token(&self) -> NonNull<c_void> {
        self.token
    }
}

impl Drop for MarkupExtensionRegistration {
    fn drop(&mut self) {
        // Releases only this guard's ref; live extension instances keep the
        // handler box alive until the last one dies.
        //
        // SAFETY: `self.token` was produced by `new` and is freed exactly
        // once here.
        unsafe { noesis_markup_extension_unregister(self.token.as_ptr()) };
    }
}

/// Drops the handler box handed to C++ at registration, and its string
/// scratch slot.
unsafe extern "C" fn markup_handler_free_trampoline(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        forget_string_scratch(userdata);
        // SAFETY: `userdata` is the `Box::into_raw` from `new`. Single-owner
        // contract; C++ calls this exactly once.
        drop(Box::from_raw(
            userdata.cast::<Box<dyn MarkupExtensionHandler>>(),
        ));
    })
}

unsafe extern "C" fn provide_trampoline(
    userdata: *mut c_void,
    key: *const c_char,
    out_string: *mut *const c_char,
    out_component: *mut *mut c_void,
) -> bool {
    crate::panic_guard::guard(|| {
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
                // Noesis copies the string after this returns, so park it in a
                // per-handler slot; an interior NUL reports no value rather than truncating.
                let Ok(cstring) = CString::new(s.as_bytes()) else {
                    return false;
                };
                let key = userdata as usize;
                let mut table = STRING_SCRATCH.lock().expect("STRING_SCRATCH poisoned");
                // Prior scratch for this handler is already copied, so reuse the slot.
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
    })
}

// Keyed by handler userdata pointer; entries are removed when the handler is freed.
static STRING_SCRATCH: Mutex<Vec<(usize, CString)>> = Mutex::new(Vec::new());

fn forget_string_scratch(userdata: *mut c_void) {
    let key = userdata as usize;
    let mut table = STRING_SCRATCH.lock().expect("STRING_SCRATCH poisoned");
    table.retain(|(k, _)| *k != key);
}
