//! Rust value converters for data binding.
//!
//! A [`Converter`] wraps a `Noesis::BaseValueConverter` subclass whose
//! `TryConvert` / `TryConvertBack` forward into a Rust [`ValueConverter`]. This
//! is what real bindings need: mapping a source value to a target-shaped value
//! (bool → text/Visibility, number formatting, enum mapping, ...) as the data
//! crosses the binding.
//!
//! Binding values cross the FFI as boxed `Noesis::BaseComponent*`. The callback
//! receives the source [`value`](ValueConverter::convert) and the optional
//! `ConverterParameter` as [`ConvertArg`]s (unbox them with
//! [`ConvertArg::as_i32`] / [`as_bool`](ConvertArg::as_bool) /
//! [`as_f64`](ConvertArg::as_f64) / [`as_str`](ConvertArg::as_str)), and returns
//! a [`Converted`] the trampoline re-boxes for Noesis (or `None` to signal
//! `UnsetValue`, so the binding falls back to its `FallbackValue` / the
//! property default).
//!
//! # Reaching a binding
//!
//! Two ways, both wired the same on the C++ side:
//!
//! * **Code-built binding**, the primary path for Rust integration:
//!   `Binding::new("Path").converter(&converter)` then
//!   [`set_binding`](crate::binding::set_binding). See [`crate::binding`].
//! * **XAML resource**: insert the converter into an element's
//!   `ResourceDictionary` with [`add_resource`](crate::binding::add_resource)
//!   and author `{Binding Path, Converter={StaticResource Key}}` in XAML.
//!
//! # Lifetime
//!
//! [`Converter`] holds the caller's `+1` reference, released on drop. If a
//! binding still references the converter (the common case while it's wired
//! onto a live element), the underlying object (and the boxed handler) stay
//! alive until that reference also drops. The handler is freed exactly once, by
//! the C++ destructor, after the last reference goes away.
//!
//! # Threading
//!
//! `convert` / `convert_back` fire from inside Noesis's binding pump on whatever
//! thread drives the view. The handler is stored behind `Send`; keep the work
//! small.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface; explicit blocks add noise

use core::ptr::{self, NonNull};
use std::ffi::{CStr, CString, c_void};

use crate::ffi::{
    ValueConverterVTable, noesis_box_bool, noesis_box_double, noesis_box_int32, noesis_box_string,
    noesis_unbox_bool, noesis_unbox_double, noesis_unbox_int32, noesis_unbox_string,
    noesis_value_converter_create, noesis_value_converter_destroy,
};

/// A borrowed, boxed binding value handed to a [`ValueConverter`]. `None`-valued
/// (the bound source produced null / the control supplied no parameter) reports
/// [`is_none`](Self::is_none); the typed accessors return `None` when the boxed
/// runtime type doesn't match.
pub struct ConvertArg(Option<NonNull<c_void>>);

impl ConvertArg {
    pub(crate) fn new(raw: *mut c_void) -> Self {
        Self(NonNull::new(raw))
    }

    /// Whether the argument carried no value (a null `BaseComponent*`).
    #[must_use]
    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }

    /// Raw borrowed `Noesis::BaseComponent*` (the boxed value), or null.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.0.map_or(ptr::null_mut(), NonNull::as_ptr)
    }

    /// Unbox a `bool` (a `BoxedValue<bool>`), or `None` on type mismatch / null.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        let p = self.0?;
        let mut out = false;
        let ok = unsafe { noesis_unbox_bool(p.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Unbox an `i32` (a `BoxedValue<int>`), or `None` on type mismatch / null.
    #[must_use]
    pub fn as_i32(&self) -> Option<i32> {
        let p = self.0?;
        let mut out = 0i32;
        let ok = unsafe { noesis_unbox_int32(p.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Unbox an `f64` (a `BoxedValue<double>`), or `None` on type mismatch / null.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        let p = self.0?;
        let mut out = 0.0f64;
        let ok = unsafe { noesis_unbox_double(p.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Borrowed view of a boxed string (a `BoxedValue<String>`), valid for the
    /// callback. `None` on type mismatch / null / non-UTF-8.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        let p = self.0?;
        let s = unsafe { noesis_unbox_string(p.as_ptr()) };
        if s.is_null() {
            return None;
        }
        unsafe { CStr::from_ptr(s) }.to_str().ok()
    }
}

/// A value a [`ValueConverter`] produces. The trampoline re-boxes it into the
/// `Noesis::BaseComponent*` the binding engine expects.
#[derive(Debug, Clone)]
pub enum Converted {
    Bool(bool),
    Int32(i32),
    Double(f64),
    /// A string, the most common converted target (e.g. a `TextBlock`'s `Text`,
    /// or a value coerced to an enum like `Visibility` via Noesis's
    /// string→enum type converter). Must not contain an interior NUL byte;
    /// boxing one panics (caught by the trampoline as a clean conversion
    /// failure).
    String(String),
    /// An explicit null value (distinct from returning `None`, which signals
    /// `UnsetValue` / fallback).
    Null,
}

impl Converted {
    /// Box into a `+1`-owned `BaseComponent*` (ownership transfers to C++), or
    /// null for [`Converted::Null`].
    pub(crate) fn into_boxed(self) -> *mut c_void {
        match self {
            Converted::Bool(b) => unsafe { noesis_box_bool(b) },
            Converted::Int32(i) => unsafe { noesis_box_int32(i) },
            Converted::Double(d) => unsafe { noesis_box_double(d) },
            Converted::String(s) => {
                let cs = CString::new(s).expect("converted string contained NUL");
                unsafe { noesis_box_string(cs.as_ptr()) }
            }
            Converted::Null => ptr::null_mut(),
        }
    }
}

/// Rust-side conversion logic. `convert` maps a binding source value to the
/// target; `convert_back` (only reached on a `TwoWay` / `OneWayToSource`
/// binding) maps a target value back to the source. Returning `None` signals
/// `UnsetValue`.
pub trait ValueConverter: Send + 'static {
    /// Source → target. Called when the binding propagates from source to
    /// target.
    fn convert(&self, value: &ConvertArg, param: &ConvertArg) -> Option<Converted>;

    /// Target → source. Default: `None` (no back-conversion). Only invoked for
    /// `TwoWay` / `OneWayToSource` bindings.
    fn convert_back(&self, _value: &ConvertArg, _param: &ConvertArg) -> Option<Converted> {
        None
    }
}

/// Adapter so a bare `Fn(&ConvertArg, &ConvertArg) -> Option<Converted>` closure
/// is a one-way [`ValueConverter`] (`convert_back` returns `None`).
impl<F> ValueConverter for F
where
    F: Fn(&ConvertArg, &ConvertArg) -> Option<Converted> + Send + 'static,
{
    fn convert(&self, value: &ConvertArg, param: &ConvertArg) -> Option<Converted> {
        self(value, param)
    }
}

static CONVERTER_VTABLE: ValueConverterVTable = ValueConverterVTable {
    convert: convert_trampoline,
    convert_back: convert_back_trampoline,
};

/// SAFETY: `userdata` is the `Box<Box<dyn ValueConverter>>` leaked in
/// [`Converter::new`], alive until the free trampoline runs.
unsafe extern "C" fn convert_trampoline(
    userdata: *mut c_void,
    value: *mut c_void,
    _target_type: *const c_void,
    parameter: *mut c_void,
    out_result: *mut *mut c_void,
) -> bool {
    crate::panic_guard::guard(|| {
        let handler = &*userdata.cast::<Box<dyn ValueConverter>>();
        let v = ConvertArg::new(value);
        let p = ConvertArg::new(parameter);
        match handler.convert(&v, &p) {
            Some(result) => {
                if !out_result.is_null() {
                    *out_result = result.into_boxed();
                }
                true
            }
            None => false,
        }
    })
}

/// SAFETY: see [`convert_trampoline`].
unsafe extern "C" fn convert_back_trampoline(
    userdata: *mut c_void,
    value: *mut c_void,
    _target_type: *const c_void,
    parameter: *mut c_void,
    out_result: *mut *mut c_void,
) -> bool {
    crate::panic_guard::guard(|| {
        let handler = &*userdata.cast::<Box<dyn ValueConverter>>();
        let v = ConvertArg::new(value);
        let p = ConvertArg::new(parameter);
        match handler.convert_back(&v, &p) {
            Some(result) => {
                if !out_result.is_null() {
                    *out_result = result.into_boxed();
                }
                true
            }
            None => false,
        }
    })
}

/// SAFETY: `userdata` was produced by [`Converter::new`] and C++ owns it; this
/// is the matching `Box::from_raw` that ends that ownership, run exactly once.
unsafe extern "C" fn converter_free_trampoline(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        drop(Box::from_raw(userdata.cast::<Box<dyn ValueConverter>>()));
    })
}

/// A Rust-backed `IValueConverter`. Owns a `+1` reference released on drop.
/// Attach it to a binding with
/// [`Binding::converter`](crate::binding::Binding::converter), or insert it into
/// an element's resources with [`add_resource`](crate::binding::add_resource).
pub struct Converter {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for Converter {}

impl Converter {
    /// Build a converter from a [`ValueConverter`]. A bare
    /// `Fn(&ConvertArg, &ConvertArg) -> Option<Converted>` closure also works
    /// (one-way: its `convert_back` returns `None`).
    ///
    /// # Panics
    ///
    /// Panics only on an impossible internal invariant (the C side returning
    /// null for a valid vtable, which it never does).
    #[must_use]
    pub fn new<C: ValueConverter>(converter: C) -> Self {
        // Double-Box for a stable thin pointer across the C ABI.
        let boxed: Box<Box<dyn ValueConverter>> = Box::new(Box::new(converter));
        let userdata = Box::into_raw(boxed);

        // SAFETY: vtable is 'static + valid; userdata ownership transfers to
        // C++; the free trampoline is extern "C".
        let ptr = unsafe {
            noesis_value_converter_create(
                &CONVERTER_VTABLE,
                userdata.cast(),
                converter_free_trampoline,
            )
        };

        match NonNull::new(ptr) {
            Some(ptr) => Converter { ptr },
            None => {
                // The C side only returns null for a null vtable, which we
                // never pass. Reclaim the leaked box defensively.
                // SAFETY: userdata came from Box::into_raw above; C++ never
                // stored it (null return = nothing took ownership).
                unsafe { drop(Box::from_raw(userdata)) };
                unreachable!("noesis_value_converter_create returned null for a non-null vtable");
            }
        }
    }

    /// Raw `Noesis::BaseComponent*` (an `IValueConverter`), for handing to a
    /// binding or a resource dictionary. Borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl Drop for Converter {
    fn drop(&mut self) {
        // SAFETY: produced by noesis_value_converter_create with +1 ref; this
        // releases exactly that ref. The handler box is freed by the C++
        // destructor once the last reference (possibly a binding) drops.
        unsafe { noesis_value_converter_destroy(self.ptr.as_ptr()) }
    }
}
