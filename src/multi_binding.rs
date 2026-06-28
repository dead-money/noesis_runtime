//! `MultiBinding` + `IMultiValueConverter` from Rust.
//!
//! A [`MultiBinding`] combines N child [`Binding`]s through a Rust
//! [`MultiValueConverter`] into a single target value — the code-built
//! equivalent of authoring a `<MultiBinding>` with several `<Binding>` children
//! and a `Converter` in XAML. The converter receives the source values as an
//! array of boxed arguments (one per child binding, in order) and returns a
//! single combined value.
//!
//! ```ignore
//! let conv = MultiConverter::new(|values: &[ConvertArg], _p: &ConvertArg| {
//!     let a = values.first().and_then(ConvertArg::as_str).unwrap_or_default();
//!     let b = values.get(1).and_then(ConvertArg::as_str).unwrap_or_default();
//!     Some(Converted::String(format!("{a} {b}")))
//! });
//! let mb = MultiBinding::new()
//!     .converter(&conv)
//!     .add_binding(Binding::new("First"))
//!     .add_binding(Binding::new("Last"));
//! mb.set_on(&label, "Text");
//! ```
//!
//! # Lifetime
//!
//! Both [`MultiConverter`] and [`MultiBinding`] own a `+1` reference released on
//! drop. [`MultiBinding::set_on`] makes Noesis take its own reference, so the
//! handle may be dropped after wiring; the converter stays alive while the
//! binding references it. The converter's handler box is freed exactly once, by
//! the C++ destructor, after the last reference drops — modelled on
//! [`crate::converters::Converter`].
//!
//! # Threading
//!
//! `convert` fires from inside Noesis's binding pump on whatever thread drives
//! the view. The handler is stored behind `Send`; keep the work small.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface — explicit blocks add noise

use core::ptr::NonNull;
use std::ffi::{CString, c_void};

use crate::binding::Binding;
use crate::converters::{ConvertArg, Converted};
use crate::ffi::{
    MultiValueConverterVTable, noesis_multi_binding_add_binding, noesis_multi_binding_create,
    noesis_multi_binding_destroy, noesis_multi_binding_set_converter,
    noesis_multi_binding_set_converter_parameter, noesis_multi_binding_set_mode,
    noesis_multi_value_converter_create, noesis_multi_value_converter_destroy,
    noesis_set_multi_binding,
};
use crate::view::FrameworkElement;

pub use crate::binding::BindingMode;

/// Rust-side multi-value conversion logic: combine the source values of the
/// child bindings into one target value. Returning `None` signals `UnsetValue`
/// (the binding falls back to its `FallbackValue` / the property default).
pub trait MultiValueConverter: Send + 'static {
    /// `values` holds one borrowed boxed argument per child [`Binding`], in the
    /// order they were [`added`](MultiBinding::add_binding). `param` is the
    /// optional converter parameter.
    fn convert(&self, values: &[ConvertArg], param: &ConvertArg) -> Option<Converted>;
}

/// A bare closure is a [`MultiValueConverter`].
impl<F> MultiValueConverter for F
where
    F: Fn(&[ConvertArg], &ConvertArg) -> Option<Converted> + Send + 'static,
{
    fn convert(&self, values: &[ConvertArg], param: &ConvertArg) -> Option<Converted> {
        self(values, param)
    }
}

static MULTI_CONVERTER_VTABLE: MultiValueConverterVTable = MultiValueConverterVTable {
    convert: multi_convert_trampoline,
};

/// SAFETY: `userdata` is the `Box<Box<dyn MultiValueConverter>>` leaked in
/// [`MultiConverter::new`], alive until the free trampoline runs. `values`
/// points at `count` borrowed boxed `BaseComponent*`.
unsafe extern "C" fn multi_convert_trampoline(
    userdata: *mut c_void,
    values: *const *mut c_void,
    count: u32,
    _target_type: *const c_void,
    parameter: *mut c_void,
    out_result: *mut *mut c_void,
) -> bool {
    crate::panic_guard::guard(|| {
        let handler = &*userdata.cast::<Box<dyn MultiValueConverter>>();

        let args: Vec<ConvertArg> = if values.is_null() || count == 0 {
            Vec::new()
        } else {
            let slice = core::slice::from_raw_parts(values, count as usize);
            slice.iter().map(|&p| ConvertArg::new(p)).collect()
        };
        let param = ConvertArg::new(parameter);

        match handler.convert(&args, &param) {
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

/// SAFETY: `userdata` was produced by [`MultiConverter::new`] and C++ owns it;
/// this is the matching `Box::from_raw`, run exactly once on last release.
unsafe extern "C" fn multi_converter_free_trampoline(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        drop(Box::from_raw(
            userdata.cast::<Box<dyn MultiValueConverter>>(),
        ));
    })
}

/// A Rust-backed `IMultiValueConverter`. Owns a `+1` reference released on drop.
/// Attach it to a [`MultiBinding`] with [`MultiBinding::converter`].
pub struct MultiConverter {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for MultiConverter {}

impl MultiConverter {
    /// Build a multi-value converter from a [`MultiValueConverter`]. A bare
    /// `Fn(&[ConvertArg], &ConvertArg) -> Option<Converted>` closure also works.
    ///
    /// # Panics
    ///
    /// Panics only on an impossible internal invariant (the C side returning
    /// null for a valid vtable).
    #[must_use]
    pub fn new<C: MultiValueConverter>(converter: C) -> Self {
        let boxed: Box<Box<dyn MultiValueConverter>> = Box::new(Box::new(converter));
        let userdata = Box::into_raw(boxed);

        // SAFETY: vtable is 'static + valid; userdata ownership transfers to
        // C++; the free trampoline is extern "C".
        let ptr = unsafe {
            noesis_multi_value_converter_create(
                &MULTI_CONVERTER_VTABLE,
                userdata.cast(),
                multi_converter_free_trampoline,
            )
        };

        match NonNull::new(ptr) {
            Some(ptr) => MultiConverter { ptr },
            None => {
                // SAFETY: userdata came from Box::into_raw; C++ took no
                // ownership on a null return.
                unsafe { drop(Box::from_raw(userdata)) };
                unreachable!(
                    "noesis_multi_value_converter_create returned null for a non-null vtable"
                );
            }
        }
    }

    /// Raw `Noesis::BaseComponent*` (an `IMultiValueConverter`). Borrowed for the
    /// lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl Drop for MultiConverter {
    fn drop(&mut self) {
        // SAFETY: produced by create with +1; releases exactly that ref. The
        // handler box is freed by the C++ destructor once the last reference
        // (possibly a binding) drops.
        unsafe { noesis_multi_value_converter_destroy(self.ptr.as_ptr()) }
    }
}

/// A code-built `Noesis::MultiBinding`. Add child [`Binding`]s with
/// [`add_binding`](Self::add_binding), attach a [`MultiConverter`] with
/// [`converter`](Self::converter), then wire it onto a target DP with
/// [`set_on`](Self::set_on).
///
/// Owns a `+1` reference released on drop. [`set_on`](Self::set_on) makes Noesis
/// take its own reference.
pub struct MultiBinding {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for MultiBinding {}

impl Default for MultiBinding {
    fn default() -> Self {
        Self::new()
    }
}

impl MultiBinding {
    /// Create an empty `MultiBinding`.
    ///
    /// # Panics
    ///
    /// Panics if the Noesis allocation fails.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: no preconditions; returns a +1-owned MultiBinding*.
        let ptr = unsafe { noesis_multi_binding_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_multi_binding_create returned null"),
        }
    }

    /// Append a child [`Binding`]. Order matters: it determines the index of
    /// this binding's value in the [`MultiValueConverter::convert`] `values`
    /// slice. The `MultiBinding` takes its own reference to the child, so the
    /// passed [`Binding`] is consumed (dropped after wiring). Chainable.
    #[must_use]
    pub fn add_binding(self, binding: Binding) -> Self {
        // SAFETY: both pointers are live; the MultiBinding takes its own ref.
        unsafe { noesis_multi_binding_add_binding(self.ptr.as_ptr(), binding.raw()) };
        self
    }

    /// Attach a Rust [`MultiConverter`]. The binding takes its own reference, so
    /// the handle may be dropped afterwards. Chainable.
    #[must_use]
    pub fn converter(self, converter: &MultiConverter) -> Self {
        // SAFETY: both pointers are live; the binding stores its own ref.
        unsafe { noesis_multi_binding_set_converter(self.ptr.as_ptr(), converter.raw()) };
        self
    }

    /// Set the converter parameter (a boxed value passed to the converter on
    /// every call). The binding stores its own reference. Chainable.
    #[must_use]
    pub fn converter_parameter(self, parameter: &crate::binding::Boxed) -> Self {
        // SAFETY: both pointers are live; the binding stores its own ref.
        unsafe { noesis_multi_binding_set_converter_parameter(self.ptr.as_ptr(), parameter.raw()) };
        self
    }

    /// Set the [`BindingMode`]. Chainable.
    #[must_use]
    pub fn mode(self, mode: BindingMode) -> Self {
        // SAFETY: ptr is live.
        unsafe { noesis_multi_binding_set_mode(self.ptr.as_ptr(), mode as i32) };
        self
    }

    /// Raw `Noesis::MultiBinding*` (a `BaseComponent*`). Borrowed for the
    /// lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Wire this `MultiBinding` onto `element`'s `dp_name` dependency property.
    /// Returns `false` if `element` is not a `DependencyObject` or `dp_name` is
    /// unknown on its type.
    ///
    /// # Panics
    ///
    /// Panics if `dp_name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_on(&self, element: &FrameworkElement, dp_name: &str) -> bool {
        let c = CString::new(dp_name).expect("dp name contained interior NUL");
        // SAFETY: element + self are live; c is valid for the call.
        unsafe { noesis_set_multi_binding(element.raw(), c.as_ptr(), self.ptr.as_ptr()) }
    }
}

impl Drop for MultiBinding {
    fn drop(&mut self) {
        // SAFETY: produced by create with +1; releases exactly that ref.
        unsafe { noesis_multi_binding_destroy(self.ptr.as_ptr()) }
    }
}
