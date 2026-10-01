//! Value converters written in Rust, for data binding.
//!
//! A [`Converter`] is an `IValueConverter` that calls a Rust
//! [`ValueConverter`] to turn a source value into the value the target property
//! shows (bool to text, number formatting, enum mapping, ...) and, for two-way
//! bindings, back again.
//!
//! The converter receives the source value and the optional
//! `ConverterParameter` as [`ConvertArg`]s and decodes them with the typed
//! accessors ([`ConvertArg::as_bool`], [`as_i32`](ConvertArg::as_i32),
//! [`as_f64`](ConvertArg::as_f64), [`as_str`](ConvertArg::as_str)). It returns
//! a [`Converted`] value, or `None` to report `UnsetValue`, which makes the
//! binding use its `FallbackValue` or the property default.
//!
//! # Using a converter
//!
//! * In code: `Binding::new("Path").converter(&converter)`, then
//!   [`set_binding`](crate::binding::set_binding). See [`crate::binding`].
//! * From XAML: add it to an element's resources with
//!   [`add_resource`](crate::binding::add_resource) and write
//!   `{Binding Path, Converter={StaticResource Key}}`.
//!
//! # Lifetime
//!
//! [`Converter`] holds one reference, released on drop. A binding that uses the
//! converter holds its own, so the converter and its handler stay alive until
//! the last reference goes. The handler is freed exactly once, by the C++
//! destructor.
//!
//! # Threading
//!
//! Conversions run during Noesis's binding updates, on the thread that drives
//! the view. Keep them short.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface; explicit blocks add noise

use core::ptr::{self, NonNull};
use std::ffi::{CStr, CString, c_void};

use crate::ffi::{
    ValueConverterVTable, noesis_box_bool, noesis_box_double, noesis_box_int32, noesis_box_string,
    noesis_unbox_bool, noesis_unbox_double, noesis_unbox_int32, noesis_unbox_string,
    noesis_value_converter_create, noesis_value_converter_destroy,
};

/// A value or parameter passed to a [`ValueConverter`]: a borrowed, boxed
/// `Noesis::BaseComponent*`, valid only during the call. It is null when the
/// source value is null or no parameter was given. The typed accessors return
/// `None` when the boxed type doesn't match.
pub struct ConvertArg(Option<NonNull<c_void>>);

impl ConvertArg {
    pub(crate) fn new(raw: *mut c_void) -> Self {
        Self(NonNull::new(raw))
    }

    /// Whether the argument is null.
    #[must_use]
    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }

    /// The borrowed `Noesis::BaseComponent*`, or null.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.0.map_or(ptr::null_mut(), NonNull::as_ptr)
    }

    /// Unbox a `bool`. `None` on a type mismatch or null.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        let p = self.0?;
        let mut out = false;
        let ok = unsafe { noesis_unbox_bool(p.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Unbox an `i32`. `None` on a type mismatch or null.
    #[must_use]
    pub fn as_i32(&self) -> Option<i32> {
        let p = self.0?;
        let mut out = 0i32;
        let ok = unsafe { noesis_unbox_int32(p.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Unbox an `f64`. `None` on a type mismatch or null.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        let p = self.0?;
        let mut out = 0.0f64;
        let ok = unsafe { noesis_unbox_double(p.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Borrow a boxed string. `None` on a type mismatch, null, or invalid
    /// UTF-8. A literal `ConverterParameter` in XAML arrives as a string.
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

/// The result of a [`ValueConverter`], boxed for Noesis when returned.
#[derive(Debug, Clone)]
pub enum Converted {
    Bool(bool),
    Int32(i32),
    Double(f64),
    /// A string. Besides text properties, this works for enum targets such as
    /// `Visibility`, which Noesis parses from the string. An interior NUL byte
    /// makes the conversion fail, as if the converter had returned `None`.
    String(String),
    /// A null value. Returning `None` instead means `UnsetValue`.
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

/// The logic behind a [`Converter`]. Returning `None` from either method
/// reports `UnsetValue`.
///
/// The binding's target type is not passed in. Return a value that converts to
/// it; a [`Converted::String`] works for most targets.
pub trait ValueConverter: Send + 'static {
    /// Convert a source value for the target property.
    fn convert(&self, value: &ConvertArg, param: &ConvertArg) -> Option<Converted>;

    /// Convert a target value back to the source. Called only for `TwoWay` and
    /// `OneWayToSource` bindings. Defaults to `None`.
    fn convert_back(&self, _value: &ConvertArg, _param: &ConvertArg) -> Option<Converted> {
        None
    }
}

/// Any `Fn(&ConvertArg, &ConvertArg) -> Option<Converted>` closure is a one-way
/// converter.
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

/// An `IValueConverter` backed by a Rust [`ValueConverter`]. Use it with
/// [`Binding::converter`](crate::binding::Binding::converter), or add it to an
/// element's resources with [`add_resource`](crate::binding::add_resource).
/// Holds one reference, released on drop.
pub struct Converter {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for Converter {}

impl Converter {
    /// Create a converter from a [`ValueConverter`] or a closure.
    ///
    /// # Panics
    ///
    /// Never in practice: the C side returns null only for a null vtable.
    #[must_use]
    pub fn new<C: ValueConverter>(converter: C) -> Self {
        // Double box: `Box<dyn _>` is a fat pointer; the C ABI needs a thin one.
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
                // SAFETY: userdata came from Box::into_raw above; C++ never
                // stored it (null return = nothing took ownership).
                unsafe { drop(Box::from_raw(userdata)) };
                unreachable!("noesis_value_converter_create returned null for a non-null vtable");
            }
        }
    }

    /// Raw `Noesis::BaseComponent*` (an `IValueConverter`), valid while `self`
    /// is alive.
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
