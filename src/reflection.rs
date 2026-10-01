//! Enums, routed events, and type metadata registered with Noesis reflection
//! at runtime, plus XAML's string-to-value conversion.
//!
//! These complement [`crate::classes`], which registers custom controls. XAML
//! and bindings find what you register here the same way they find types
//! built into Noesis.
//!
//! * [`register_enum`]: an enum usable for property values, `Style` setters
//!   and XAML enum strings.
//! * [`register_routed_event`] and [`raise_event`]: a routed event on a custom
//!   control type, raised from Rust and handled through
//!   [`crate::events::subscribe_event`].
//! * [`is_component_registered`], [`set_content_property`] and
//!   [`add_depends_on`]: inspect the factory and attach type metadata.
//! * [`convert_from_string`]: convert a string to a typed value the way the
//!   XAML parser does.
//!
//! Registrations are process-wide and last until [`crate::shutdown`]; they
//! cannot be removed.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface; explicit blocks add noise

use core::ffi::CStr;
use core::ptr::{self, NonNull};
use std::ffi::{CString, c_void};

use crate::ffi::{
    EnumValue, noesis_base_component_release, noesis_enum_name_from_value,
    noesis_enum_value_from_name, noesis_factory_is_registered, noesis_raise_routed_event,
    noesis_register_enum, noesis_register_routed_event, noesis_type_add_depends_on,
    noesis_type_converter_from_string, noesis_type_get_content_property,
    noesis_type_get_depends_on, noesis_type_set_content_property, noesis_unbox_bool,
    noesis_unbox_double, noesis_unbox_int32, noesis_unbox_string,
};
use crate::view::FrameworkElement;

/// An enum registered with [`register_enum`]. Dropping the handle does not
/// unregister the enum.
pub struct EnumType {
    name: CString,
}

/// Registers an enum named `name` with the given `(variant_name, value)`
/// members.
///
/// Returns `None` if `name` is empty or already registered, or if a variant
/// name contains an interior NUL byte.
///
/// # Panics
///
/// Panics if `name` contains an interior NUL byte.
#[must_use]
pub fn register_enum(name: &str, variants: &[(&str, i32)]) -> Option<EnumType> {
    let cname = CString::new(name).expect("enum name contained NUL");

    // Keep the variant name CStrings alive for the duration of the FFI call.
    let owned: Vec<CString> = variants
        .iter()
        .map(|(n, _)| CString::new(*n))
        .collect::<Result<_, _>>()
        .ok()?;
    let ffi_values: Vec<EnumValue> = owned
        .iter()
        .zip(variants.iter())
        .map(|(c, (_, v))| EnumValue {
            name: c.as_ptr(),
            value: *v,
        })
        .collect();

    // SAFETY: name + values point to live storage for the call; the C++ side
    // copies every name into an interned Symbol and the values into the
    // TypeEnum. The returned Type* is borrowed (owned by reflection).
    let ty = unsafe {
        noesis_register_enum(cname.as_ptr(), ffi_values.as_ptr(), ffi_values.len() as u32)
    };
    if ty.is_null() {
        return None;
    }
    Some(EnumType { name: cname })
}

impl EnumType {
    /// The name this enum was registered under.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.to_str().unwrap_or_default()
    }

    /// The value of member `variant_name`, or `None` if there is no such
    /// member.
    #[must_use]
    pub fn value_from_name(&self, variant_name: &str) -> Option<i32> {
        let cn = CString::new(variant_name).ok()?;
        let mut out = 0i32;
        // SAFETY: both pointers are valid for the call; out is written only on success.
        let ok = unsafe { noesis_enum_value_from_name(self.name.as_ptr(), cn.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// The name of the member with `value`, or `None` if no member has it.
    #[must_use]
    pub fn name_from_value(&self, value: i32) -> Option<String> {
        let mut out: *const core::ffi::c_char = ptr::null();
        // SAFETY: name ptr valid for the call; out receives a borrowed
        // interned Symbol string (valid while Noesis lives) which we copy.
        let ok = unsafe { noesis_enum_name_from_value(self.name.as_ptr(), value, &mut out) };
        if !ok || out.is_null() {
            return None;
        }
        // SAFETY: out is a non-null NUL-terminated interned Symbol string.
        Some(
            unsafe { CStr::from_ptr(out) }
                .to_string_lossy()
                .into_owned(),
        )
    }
}

/// How a routed event travels through the element tree.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RoutingStrategy {
    /// From the root down to the source, as preview events do.
    Tunnel = 0,
    /// From the source up to the root.
    Bubble = 1,
    /// To the source element only.
    Direct = 2,
}

/// Registers a routed event named `event_name` on the element type
/// `type_name`, typically a custom control from [`crate::classes`]. Instances
/// then accept handlers through [`crate::events::subscribe_event`], and
/// [`raise_event`] raises it.
///
/// Returns `false` if the type is unknown or not an element type, or if it
/// already has an event with that name.
///
/// # Panics
///
/// Panics if `type_name` / `event_name` contain an interior NUL byte.
#[must_use]
pub fn register_routed_event(type_name: &str, event_name: &str, strategy: RoutingStrategy) -> bool {
    let ct = CString::new(type_name).expect("type name contained NUL");
    let ce = CString::new(event_name).expect("event name contained NUL");
    // SAFETY: both pointers are valid for the call; the C++ side registers the
    // event on the type's UIElementData metadata.
    unsafe { noesis_register_routed_event(ct.as_ptr(), ce.as_ptr(), strategy as i32) }
}

/// Raises the routed event `event_name` with `element` as its source, routed
/// by the event's [`RoutingStrategy`].
///
/// Returns `false` if `element` is not a `UIElement` or neither its type nor
/// its base types define the event.
///
/// # Panics
///
/// Panics if `event_name` contains an interior NUL byte.
#[must_use]
pub fn raise_event(element: &FrameworkElement, event_name: &str) -> bool {
    let ce = CString::new(event_name).expect("event name contained NUL");
    // SAFETY: element.raw() is a live UIElement* for the borrow; the name ptr
    // is valid for the call.
    unsafe { noesis_raise_routed_event(element.raw(), ce.as_ptr()) }
}

/// Whether the XAML parser can create a component named `name`.
/// [`ClassBuilder::register`](crate::classes::ClassBuilder::register) makes a
/// custom class creatable.
#[must_use]
pub fn is_component_registered(name: &str) -> bool {
    let Ok(c) = CString::new(name) else {
        return false;
    };
    // SAFETY: c.as_ptr() valid for the call; queries Factory::IsComponentRegistered.
    unsafe { noesis_factory_is_registered(c.as_ptr()) }
}

/// Makes `prop_name` the content property of the type `type_name`, so child
/// content in XAML (`<ns:Thing><Child/></ns:Thing>`) goes into that property.
/// Use it on types you registered. Returns `false` if the type is unknown.
///
/// # Panics
///
/// Panics if `type_name` / `prop_name` contain an interior NUL byte.
#[must_use]
pub fn set_content_property(type_name: &str, prop_name: &str) -> bool {
    let ct = CString::new(type_name).expect("type name contained NUL");
    let cp = CString::new(prop_name).expect("prop name contained NUL");
    // SAFETY: both pointers valid for the call; appends ContentPropertyMetaData.
    unsafe { noesis_type_set_content_property(ct.as_ptr(), cp.as_ptr()) }
}

/// The content property recorded on `type_name`, such as one set with
/// [`set_content_property`]. `None` if the type is unknown or has none.
///
/// # Panics
///
/// Panics if `type_name` contains an interior NUL byte.
#[must_use]
pub fn get_content_property(type_name: &str) -> Option<String> {
    let ct = CString::new(type_name).expect("type name contained NUL");
    let mut out: *const core::ffi::c_char = ptr::null();
    // SAFETY: type_name ptr valid for the call; out receives a borrowed interned
    // Symbol string (valid while Noesis lives) which we copy on success.
    let ok = unsafe { noesis_type_get_content_property(ct.as_ptr(), &mut out) };
    if !ok || out.is_null() {
        return None;
    }
    // SAFETY: out is a non-null NUL-terminated interned Symbol string.
    Some(
        unsafe { CStr::from_ptr(out) }
            .to_string_lossy()
            .into_owned(),
    )
}

/// Attaches `DependsOn` metadata naming `prop_name` to the type `type_name`.
/// Returns `false` if the type is unknown.
///
/// Unlike WPF, Noesis attaches `DependsOn` to the type, not to a property.
/// Only the first record on a type can be read back with [`get_depends_on`].
/// It does not interfere with [`set_content_property`].
///
/// # Panics
///
/// Panics if `type_name` / `prop_name` contain an interior NUL byte.
#[must_use]
pub fn add_depends_on(type_name: &str, prop_name: &str) -> bool {
    let ct = CString::new(type_name).expect("type name contained NUL");
    let cp = CString::new(prop_name).expect("prop name contained NUL");
    // SAFETY: both pointers valid for the call; appends DependsOnMetaData.
    unsafe { noesis_type_add_depends_on(ct.as_ptr(), cp.as_ptr()) }
}

/// The property named by the first `DependsOn` record on `type_name`. `None`
/// if the type is unknown or has none.
///
/// # Panics
///
/// Panics if `type_name` contains an interior NUL byte.
#[must_use]
pub fn get_depends_on(type_name: &str) -> Option<String> {
    let ct = CString::new(type_name).expect("type name contained NUL");
    let mut out: *const core::ffi::c_char = ptr::null();
    // SAFETY: type_name ptr valid for the call; out receives a borrowed interned
    // Symbol string (valid while Noesis lives) which we copy on success.
    let ok = unsafe { noesis_type_get_depends_on(ct.as_ptr(), &mut out) };
    if !ok || out.is_null() {
        return None;
    }
    // SAFETY: out is a non-null NUL-terminated interned Symbol string.
    Some(
        unsafe { CStr::from_ptr(out) }
            .to_string_lossy()
            .into_owned(),
    )
}

// No custom TypeConverter registration: in SDK 3.2.13 `TypeConverter::Get`
// ignores runtime-registered converter types (see LIMITATIONS.md).

/// A value produced by [`convert_from_string`]. Read it with the typed
/// accessors; each returns `None` if the value is another type. Holds one
/// reference, released on drop.
pub struct BoxedValue {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for BoxedValue {}

impl BoxedValue {
    /// The underlying `Noesis::BaseComponent*`, valid while `self` is alive.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// The value as an `i32`.
    #[must_use]
    pub fn as_i32(&self) -> Option<i32> {
        let mut out = 0i32;
        let ok = unsafe { noesis_unbox_int32(self.ptr.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// The value as a `bool`.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        let mut out = false;
        let ok = unsafe { noesis_unbox_bool(self.ptr.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// The value as an `f64`.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        let mut out = 0.0f64;
        let ok = unsafe { noesis_unbox_double(self.ptr.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// The value as a string. `None` also for invalid UTF-8.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        let s = unsafe { noesis_unbox_string(self.ptr.as_ptr()) };
        if s.is_null() {
            return None;
        }
        unsafe { CStr::from_ptr(s) }.to_str().ok()
    }
}

impl Drop for BoxedValue {
    fn drop(&mut self) {
        // SAFETY: produced by noesis_type_converter_from_string with +1 ref.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// Converts `s` to a value of the type named `type_name`, the way the XAML
/// parser converts attribute strings. Returns `None` if the type is unknown,
/// has no converter, or `s` does not parse.
///
/// Works for built-in types with a converter, such as `Bool`, `Int32`,
/// `Single`, `Color` and `Thickness`. Enums from [`register_enum`] have no
/// converter; use [`EnumType::value_from_name`] for them.
///
/// # Panics
///
/// Panics if `type_name` / `s` contain an interior NUL byte.
#[must_use]
pub fn convert_from_string(type_name: &str, s: &str) -> Option<BoxedValue> {
    let ct = CString::new(type_name).expect("type name contained NUL");
    let cs = CString::new(s).expect("string contained NUL");
    let mut out: *mut c_void = ptr::null_mut();
    // SAFETY: pointers valid for the call; out receives a +1-owned boxed
    // component (BoxedValue::drop releases it) or stays null on failure.
    let ok = unsafe { noesis_type_converter_from_string(ct.as_ptr(), cs.as_ptr(), &mut out) };
    if !ok {
        return None;
    }
    NonNull::new(out).map(|ptr| BoxedValue { ptr })
}
