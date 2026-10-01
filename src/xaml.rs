//! XAML loading beyond [`FrameworkElement::load`](crate::view::FrameworkElement::load).
//!
//! - [`get_xaml_dependencies`] lists the resources a XAML buffer references
//!   (other XAMLs, textures, audio, fonts, `UserControl` nodes, the root type)
//!   without building the object tree. Use it for asset preloading and
//!   dependency analysis.
//! - [`load_xaml_component`] loads a XAML root of any type, including ones that
//!   are not a `FrameworkElement` such as a bare `ResourceDictionary`, and
//!   returns it as a [`LoadedComponent`].

use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};
use std::os::raw::c_char;

use crate::ffi::{
    noesis_base_component_release, noesis_base_component_type_name, noesis_get_xaml_dependencies,
    noesis_gui_load_xaml_component,
};

/// Classifies a dependency reported by [`get_xaml_dependencies`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum XamlDependencyKind {
    /// Xamls, audio, textures, and `Uri` properties (e.g. `Image Source`).
    Filename,
    /// `FontFamily` properties and resources.
    Font,
    /// A `UserControl` referenced as a prefixed node (e.g. `local:ColorPicker`).
    UserControl,
    /// The type of the root node (e.g. `ResourceDictionary`, `UserControl`).
    Root,
}

impl XamlDependencyKind {
    fn from_raw(kind: i32) -> Option<Self> {
        match kind {
            0 => Some(Self::Filename),
            1 => Some(Self::Font),
            2 => Some(Self::UserControl),
            3 => Some(Self::Root),
            _ => None,
        }
    }
}

/// One dependency reported by [`get_xaml_dependencies`].
///
/// `uri` is the referenced URI, or a type name for
/// [`XamlDependencyKind::Root`] and [`XamlDependencyKind::UserControl`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XamlDependency {
    pub uri: String,
    pub kind: XamlDependencyKind,
}

// SAFETY: called synchronously from `noesis_get_xaml_dependencies`, once per
// dependency; `user` is the `&mut Vec<XamlDependency>` passed to that call.
unsafe extern "C" fn collect(user: *mut c_void, uri: *const c_char, kind: i32) {
    crate::panic_guard::guard(|| {
        // SAFETY: `user` is the &mut Vec we handed to the FFI; the borrow is scoped
        // to this synchronous callback and never aliased on the Rust side.
        let out = unsafe { &mut *user.cast::<Vec<XamlDependency>>() };
        let Some(kind) = XamlDependencyKind::from_raw(kind) else {
            return;
        };
        let uri = if uri.is_null() {
            String::new()
        } else {
            // SAFETY: `uri` is a NUL-terminated string valid for this call.
            unsafe { CStr::from_ptr(uri) }
                .to_string_lossy()
                .into_owned()
        };
        out.push(XamlDependency { uri, kind });
    })
}

/// Lists the resources `xaml` references, without instantiating its object
/// tree.
///
/// `base_uri` is the URI the XAML is treated as living at, used to resolve
/// relative references; pass `""` if there is none. Requires [`crate::init`].
///
/// Returns an empty `Vec` when the XAML references nothing or is malformed
/// (the parse error goes to the log handler).
///
/// # Panics
///
/// Panics if `base_uri` contains an interior NUL byte, or if `xaml` is larger
/// than 4 GiB.
#[must_use]
pub fn get_xaml_dependencies(xaml: &[u8], base_uri: &str) -> Vec<XamlDependency> {
    let base = CString::new(base_uri).expect("base_uri contained interior NUL");
    let mut out: Vec<XamlDependency> = Vec::new();
    let out_ptr: *mut Vec<XamlDependency> = &mut out;
    // SAFETY: `xaml` outlives the synchronous call (Noesis wraps it in a
    // MemoryStream and reads it before returning). `collect` only touches
    // `out` through `out_ptr`, which stays valid for the duration of the call.
    unsafe {
        noesis_get_xaml_dependencies(
            xaml.as_ptr(),
            u32::try_from(xaml.len()).expect("XAML > 4 GiB"),
            base.as_ptr(),
            out_ptr.cast(),
            collect,
        );
    }
    out
}

/// A XAML root of any type, returned by [`load_xaml_component`].
///
/// Holds a reference to the Noesis object, released on drop.
pub struct LoadedComponent {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for LoadedComponent {}

impl LoadedComponent {
    /// Raw `Noesis::BaseComponent*`, valid while `self` is alive. No reference
    /// is added.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Reflected class name of the root, such as `"ResourceDictionary"` or
    /// `"Grid"`. Empty if Noesis reports no class type.
    #[must_use]
    pub fn type_name(&self) -> String {
        // SAFETY: `self.ptr` is a live BaseComponent* for the lifetime of self.
        let name = unsafe { noesis_base_component_type_name(self.ptr.as_ptr()) };
        if name.is_null() {
            String::new()
        } else {
            // SAFETY: non-null NUL-terminated string owned by Noesis.
            unsafe { CStr::from_ptr(name) }
                .to_string_lossy()
                .into_owned()
        }
    }
}

impl Drop for LoadedComponent {
    fn drop(&mut self) {
        // SAFETY: ptr carries the +1 ref handed out by load_xaml_component;
        // released exactly once here.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) };
    }
}

/// Loads XAML by `uri` through the installed [`crate::xaml_provider`],
/// whatever the root's type.
///
/// [`FrameworkElement::load`](crate::view::FrameworkElement::load) returns
/// `None` for roots that are not a `FrameworkElement`; this keeps them. Use
/// [`LoadedComponent::type_name`] to see what was loaded.
///
/// Returns `None` when no provider knows the URI or the XAML is malformed.
///
/// # Panics
///
/// Panics if `uri` contains an interior NUL byte.
#[must_use]
pub fn load_xaml_component(uri: &str) -> Option<LoadedComponent> {
    let c = CString::new(uri).expect("uri contained interior NUL");
    // SAFETY: c.as_ptr() is valid for the call; the result is a fresh +1
    // BaseComponent* (or null), which LoadedComponent's Drop releases.
    let ptr = unsafe { noesis_gui_load_xaml_component(c.as_ptr()) };
    NonNull::new(ptr).map(|ptr| LoadedComponent { ptr })
}
