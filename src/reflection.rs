//! Runtime registration of "other reflected entities" (TODO §9): custom enums,
//! custom routed events on Rust-backed types, factory/metadata introspection,
//! and Rust-backed reflection [`TypeConverter`]s.
//!
//! These complement [`crate::classes`] (Rust-backed XAML classes) and
//! [`crate::converters`] (binding `IValueConverter`s). Everything here registers
//! against Noesis's reflection database so the XAML parser / bindings resolve
//! the entity the same way they resolve a compile-time `NS_REGISTER_*` macro.
//!
//! # What each piece is for
//!
//! * [`register_enum`] — a named enum usable as a dependency-property value /
//!   `Style` setter / XAML enum string. Verify the registered string<->int
//!   pairs with [`EnumType::value_from_name`] / [`EnumType::name_from_value`].
//! * [`register_routed_event`] + [`raise_event`] — a [`RoutingStrategy`] routed
//!   event on a Rust-backed [`crate::classes`] type, raised from Rust and
//!   observed with [`crate::events::subscribe_event`].
//! * [`is_component_registered`] / [`set_content_property`] — `Factory`
//!   introspection and `ContentProperty` attribution for Rust-backed types.
//! * [`convert_from_string`] — drive `TypeConverter::Get` +
//!   `TryConvertFromString` (the XAML-parse string→value coercion path) for any
//!   built-in / reflected type. Custom converter *registration* is deferred — an
//!   SDK limitation; see the section comment by [`convert_from_string`].
//!
//! # Lifetime
//!
//! Registrations are process-global and live until [`crate::shutdown`]; there is
//! no unregister (mirroring how compile-time reflection works).

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface — explicit blocks add noise

use core::ffi::CStr;
use core::ptr::{self, NonNull};
use std::ffi::{CString, c_void};

use crate::ffi::{
    EnumValue, dm_noesis_base_component_release, dm_noesis_enum_name_from_value,
    dm_noesis_enum_value_from_name, dm_noesis_factory_is_registered, dm_noesis_raise_routed_event,
    dm_noesis_register_enum, dm_noesis_register_routed_event, dm_noesis_type_converter_from_string,
    dm_noesis_type_set_content_property, dm_noesis_unbox_bool, dm_noesis_unbox_double,
    dm_noesis_unbox_int32, dm_noesis_unbox_string,
};
use crate::view::FrameworkElement;

// ── (A) Custom enums ──────────────────────────────────────────────────────────

/// A handle to a registered runtime enum. Holds the reflected name so the
/// query helpers can resolve the `Noesis::TypeEnum` on demand. The enum itself
/// is owned by Noesis's reflection registry and lives until [`crate::shutdown`].
pub struct EnumType {
    name: CString,
}

/// Register a named runtime enum with the given `(variant_name, value)` pairs,
/// so it is reachable by reflection name (XAML enum-typed properties, `Style`
/// setters, the `EnumConverter` path).
///
/// Returns `None` if `name` is empty / already registered, or any variant name
/// contains an interior NUL.
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
        dm_noesis_register_enum(cname.as_ptr(), ffi_values.as_ptr(), ffi_values.len() as u32)
    };
    if ty.is_null() {
        return None;
    }
    Some(EnumType { name: cname })
}

impl EnumType {
    /// The reflected name this enum was registered under.
    #[must_use]
    pub fn name(&self) -> &str {
        self.name.to_str().unwrap_or_default()
    }

    /// Integer value of the member named `variant_name`, read straight through
    /// `Noesis::TypeEnum::HasName`. `None` if the name is not a member.
    #[must_use]
    pub fn value_from_name(&self, variant_name: &str) -> Option<i32> {
        let cn = CString::new(variant_name).ok()?;
        let mut out = 0i32;
        // SAFETY: both pointers are valid for the call; out is written only on success.
        let ok =
            unsafe { dm_noesis_enum_value_from_name(self.name.as_ptr(), cn.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Member name for an integer `value`, via `Noesis::TypeEnum::HasValue`.
    /// `None` if no member maps to that value.
    #[must_use]
    pub fn name_from_value(&self, value: i32) -> Option<String> {
        let mut out: *const core::ffi::c_char = ptr::null();
        // SAFETY: name ptr valid for the call; out receives a borrowed
        // interned Symbol string (valid while Noesis lives) which we copy.
        let ok = unsafe { dm_noesis_enum_name_from_value(self.name.as_ptr(), value, &mut out) };
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

// ── (B) Custom routed events ──────────────────────────────────────────────────

/// Routing strategy for a custom routed event (mirrors `Noesis::RoutingStrategy`).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RoutingStrategy {
    /// Top-of-tree → source (preview events).
    Tunnel = 0,
    /// Source → top-of-tree (the WPF default).
    Bubble = 1,
    /// Delivered only to the originating element.
    Direct = 2,
}

/// Register a routed event named `event_name` on the registered Rust-backed
/// type `type_name` (a [`crate::classes`] `ContentControl`). After this,
/// instances of that type accept handlers via
/// [`crate::events::subscribe_event`] and the event can be raised with
/// [`raise_event`].
///
/// Returns `false` if the type is unknown, is not a Rust-backed
/// element type (no `UIElementData`), or the event name is already registered.
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
    unsafe { dm_noesis_register_routed_event(ct.as_ptr(), ce.as_ptr(), strategy as i32) }
}

/// Raise the routed event `event_name` from `element`, dispatched per the
/// event's registered [`RoutingStrategy`] (`Noesis::UIElement::RaiseEvent`).
/// Handlers wired with [`crate::events::subscribe_event`] observe it.
///
/// Returns `false` if `element` is not a `UIElement` or the event is not found
/// in its class hierarchy.
///
/// # Panics
///
/// Panics if `event_name` contains an interior NUL byte.
#[must_use]
pub fn raise_event(element: &FrameworkElement, event_name: &str) -> bool {
    let ce = CString::new(event_name).expect("event name contained NUL");
    // SAFETY: element.raw() is a live UIElement* for the borrow; the name ptr
    // is valid for the call.
    unsafe { dm_noesis_raise_routed_event(element.raw(), ce.as_ptr()) }
}

// ── (C) Factory / component metadata ──────────────────────────────────────────

/// Whether a component named `name` is registered in `Noesis::Factory` — i.e.
/// `<ns:name/>` can be instantiated by the XAML parser. Rust-backed classes
/// register their factory creator in [`crate::classes::ClassBuilder::register`].
#[must_use]
pub fn is_component_registered(name: &str) -> bool {
    let Ok(c) = CString::new(name) else {
        return false;
    };
    // SAFETY: c.as_ptr() valid for the call; queries Factory::IsComponentRegistered.
    unsafe { dm_noesis_factory_is_registered(c.as_ptr()) }
}

/// Attach a `ContentProperty` to the registered type `type_name`, so XAML child
/// content (`<ns:Thing><Child/></ns:Thing>`) is routed into `prop_name` instead
/// of the inherited content property. Returns `false` if the type is unknown.
///
/// # Panics
///
/// Panics if `type_name` / `prop_name` contain an interior NUL byte.
#[must_use]
pub fn set_content_property(type_name: &str, prop_name: &str) -> bool {
    let ct = CString::new(type_name).expect("type name contained NUL");
    let cp = CString::new(prop_name).expect("prop name contained NUL");
    // SAFETY: both pointers valid for the call; appends ContentPropertyMetaData.
    unsafe { dm_noesis_type_set_content_property(ct.as_ptr(), cp.as_ptr()) }
}

// ── (D) String → value conversion via the reflection TypeConverter ────────────
//
// Custom (Rust-backed) reflection `TypeConverter` *registration* is DEFERRED:
// `TypeConverter::Get` resolves converters through an internal registry that
// `TypeConverterMetaData` + `Factory::RegisterComponent` do not drive at runtime
// in 3.2.13 (a synthetic converter type registers in the Factory yet `Get` still
// returns null). See TODO.md "Known SDK limitations".
//
// The *consumption* side below is fully exposed: [`convert_from_string`] drives
// `TypeConverter::Get` + `TryConvertFromString`, the exact string→value path the
// XAML parser uses, for any built-in / reflected type.

/// An owned, boxed value returned by [`convert_from_string`]. Holds a `+1`
/// reference released on drop. Unbox it with [`BoxedValue::as_i32`] etc.
pub struct BoxedValue {
    ptr: NonNull<c_void>,
}

// SAFETY: a Noesis BaseComponent handle; same threading rationale as the other
// owning wrappers in this crate.
unsafe impl Send for BoxedValue {}
unsafe impl Sync for BoxedValue {}

impl BoxedValue {
    /// Raw borrowed `Noesis::BaseComponent*`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Unbox an `i32` (`BoxedValue<int>`), or `None` on type mismatch.
    #[must_use]
    pub fn as_i32(&self) -> Option<i32> {
        let mut out = 0i32;
        let ok = unsafe { dm_noesis_unbox_int32(self.ptr.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Unbox a `bool`, or `None` on type mismatch.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        let mut out = false;
        let ok = unsafe { dm_noesis_unbox_bool(self.ptr.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Unbox an `f64`, or `None` on type mismatch.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        let mut out = 0.0f64;
        let ok = unsafe { dm_noesis_unbox_double(self.ptr.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Borrowed view of a boxed string, valid while `self` is alive. `None` on
    /// type mismatch / non-UTF-8.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        let s = unsafe { dm_noesis_unbox_string(self.ptr.as_ptr()) };
        if s.is_null() {
            return None;
        }
        unsafe { CStr::from_ptr(s) }.to_str().ok()
    }
}

impl Drop for BoxedValue {
    fn drop(&mut self) {
        // SAFETY: produced by dm_noesis_type_converter_from_string with +1 ref.
        unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// Resolve the `TypeConverter` registered for `type_name` (`TypeConverter::Get`)
/// and convert `s` to a boxed value via `TryConvertFromString` — the exact
/// string→value path the XAML parser drives for a typed property. Returns
/// `None` if the type / converter is unknown or the string does not convert.
///
/// Works for any built-in / reflected type that has a converter (e.g. `Bool`,
/// `Int32`, `Single`, `Color`, `Thickness`). Note: a custom [`register_enum`]'d
/// enum does NOT get an auto-resolved converter here (`TypeConverter::Get`
/// returns null for runtime-registered types) — query enum members via
/// [`EnumType::value_from_name`] instead.
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
    let ok = unsafe { dm_noesis_type_converter_from_string(ct.as_ptr(), cs.as_ptr(), &mut out) };
    if !ok {
        return None;
    }
    NonNull::new(out).map(|ptr| BoxedValue { ptr })
}
