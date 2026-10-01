//! Bindings that combine several sources into one value.
//!
//! A [`MultiBinding`] feeds the values of several child [`Binding`]s to a
//! [`MultiConverter`], which returns one value for the target property. It is
//! the code equivalent of a XAML `<MultiBinding>` with a `Converter`.
//!
//! ```no_run
//! use noesis_runtime::binding::Binding;
//! use noesis_runtime::converters::{ConvertArg, Converted};
//! use noesis_runtime::multi_binding::{MultiBinding, MultiConverter};
//! use noesis_runtime::view::FrameworkElement;
//!
//! fn bind_full_name(label: &FrameworkElement) {
//!     let conv = MultiConverter::new(|values: &[ConvertArg], _param: &ConvertArg| {
//!         let first = values.first().and_then(ConvertArg::as_str).unwrap_or_default();
//!         let last = values.get(1).and_then(ConvertArg::as_str).unwrap_or_default();
//!         Some(Converted::String(format!("{first} {last}")))
//!     });
//!     let mb = MultiBinding::new()
//!         .converter(&conv)
//!         .add_binding(Binding::new("First"))
//!         .add_binding(Binding::new("Last"));
//!     assert!(mb.set_on(label, "Text"));
//! }
//! ```
//!
//! Both handles own a reference released on drop. Once wired, Noesis holds its
//! own references, so the handles can be dropped; the converter's closure is
//! freed after the last reference goes away.
//!
//! The converter runs during binding updates, on the thread that drives the
//! view.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface; explicit blocks add noise

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

/// Combines the values of a [`MultiBinding`]'s children into one target value.
/// Closures of the same signature implement it.
pub trait MultiValueConverter: Send + 'static {
    /// `values` holds one value per child [`Binding`], in the order they were
    /// [added](MultiBinding::add_binding). `param` is the converter parameter;
    /// [`ConvertArg::is_none`] if none was set. Returning `None` makes the binding use its
    /// fallback value or the property default.
    fn convert(&self, values: &[ConvertArg], param: &ConvertArg) -> Option<Converted>;
}

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
/// `MultiConverter::new`, alive until the free trampoline runs. `values`
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

/// SAFETY: `userdata` was produced by `MultiConverter::new` and C++ owns it;
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

/// A Noesis `IMultiValueConverter` that calls a Rust [`MultiValueConverter`].
/// Attach it with [`MultiBinding::converter`].
pub struct MultiConverter {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for MultiConverter {}

impl MultiConverter {
    /// Wraps `converter`, which may be a
    /// `Fn(&[ConvertArg], &ConvertArg) -> Option<Converted>` closure.
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

    /// The underlying `Noesis::BaseComponent*`, valid while `self` is alive.
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

/// A `Noesis::MultiBinding` built in code. Add children with
/// [`add_binding`](Self::add_binding), set a [`MultiConverter`] with
/// [`converter`](Self::converter), then attach it to a property with
/// [`set_on`](Self::set_on).
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
    /// Creates a multi-binding with no children and no converter.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate it.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: no preconditions; returns a +1-owned MultiBinding*.
        let ptr = unsafe { noesis_multi_binding_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_multi_binding_create returned null"),
        }
    }

    /// Appends a child binding. Its value appears at the matching index of the
    /// `values` passed to [`MultiValueConverter::convert`].
    #[must_use]
    pub fn add_binding(self, binding: Binding) -> Self {
        // SAFETY: both pointers are live; the MultiBinding takes its own ref.
        unsafe { noesis_multi_binding_add_binding(self.ptr.as_ptr(), binding.raw()) };
        self
    }

    /// Sets the converter. The binding holds its own reference, so `converter`
    /// can be dropped afterwards.
    #[must_use]
    pub fn converter(self, converter: &MultiConverter) -> Self {
        // SAFETY: both pointers are live; the binding stores its own ref.
        unsafe { noesis_multi_binding_set_converter(self.ptr.as_ptr(), converter.raw()) };
        self
    }

    /// Sets the `param` value passed to every converter call.
    #[must_use]
    pub fn converter_parameter(self, parameter: &crate::binding::Boxed) -> Self {
        // SAFETY: both pointers are live; the binding stores its own ref.
        unsafe { noesis_multi_binding_set_converter_parameter(self.ptr.as_ptr(), parameter.raw()) };
        self
    }

    /// Sets the binding direction.
    #[must_use]
    pub fn mode(self, mode: BindingMode) -> Self {
        // SAFETY: ptr is live.
        unsafe { noesis_multi_binding_set_mode(self.ptr.as_ptr(), mode as i32) };
        self
    }

    /// The underlying `Noesis::MultiBinding*`, valid while `self` is alive.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Binds `element`'s dependency property `dp_name` to this multi-binding.
    /// Noesis holds its own reference afterwards. Returns `false` if `element`
    /// has no dependency property named `dp_name`.
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
