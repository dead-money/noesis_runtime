//! XAML loading variants.
//!
//! Two surfaces that complement the [`crate::view::FrameworkElement`] load /
//! parse path:
//!
//! - [`get_xaml_dependencies`]: statically walk an in-memory XAML buffer's
//!   referenced resources (other XAMLs / textures / audio, fonts, prefixed
//!   `UserControl` nodes, and the root node's type) *without* instantiating
//!   the object tree. Backs `Noesis::GUI::GetXamlDependencies`. Use it for
//!   asset preloading and dependency analysis.
//!
//! - [`load_xaml_component`] / [`LoadedComponent`]: load a XAML root that is
//!   *not* a `FrameworkElement` (e.g. a bare `ResourceDictionary`), reporting
//!   success plus the reflected class-type name. The
//!   [`crate::view::FrameworkElement::load`] path returns `None` for such
//!   roots; this keeps them.

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

/// A single dependency found inside a XAML buffer: the referenced `uri` (or
/// type name, for [`XamlDependencyKind::Root`] / [`XamlDependencyKind::UserControl`])
/// together with its [`XamlDependencyKind`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XamlDependency {
    pub uri: String,
    pub kind: XamlDependencyKind,
}

/// Trampoline target for the C callback. `user` points at the `Vec` being
/// filled. SAFETY: invoked synchronously from inside
/// `noesis_get_xaml_dependencies`, once per dependency; `user` is the
/// `&mut Vec<XamlDependency>` we passed in, valid for that whole call.
unsafe extern "C" fn collect(user: *mut c_void, uri: *const c_char, kind: i32) {
    crate::panic_guard::guard(|| {
        // SAFETY: `user` is the &mut Vec we handed to the FFI; the borrow is scoped
        // to this synchronous callback and never aliased on the Rust side.
        let out = unsafe { &mut *user.cast::<Vec<XamlDependency>>() };
        let Some(kind) = XamlDependencyKind::from_raw(kind) else {
            return;
        };
        // Borrowed string from Noesis; copy into an owned String immediately.
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

/// Walk `xaml`'s referenced resources without instantiating the object tree,
/// returning every dependency in document order. `base_uri` is the URI the
/// XAML is treated as living at (used to resolve relative dependency paths);
/// pass `""` if there is no meaningful base.
///
/// Requires [`crate::init`] to have run. The returned `Vec` is empty when the
/// XAML is malformed (Noesis routes the parse error through the log handler
/// and reports no dependencies) or genuinely references nothing.
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

/// An owning handle to a XAML root loaded via [`load_xaml_component`], whatever
/// its concrete type. Holds a `+1` reference released on [`Drop`]. Unlike
/// [`crate::view::FrameworkElement`], the root need not be a `FrameworkElement`.
/// This is how a bare `ResourceDictionary` (or any other `BaseComponent`
/// root) is loaded and inspected.
pub struct LoadedComponent {
    ptr: NonNull<c_void>,
}

impl LoadedComponent {
    /// Raw `Noesis::BaseComponent*`, borrowed for the lifetime of `self`. Hand
    /// to other Noesis APIs that take a `BaseComponent*`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// The reflected class-type name of the loaded root (e.g.
    /// `"ResourceDictionary"`), via `BaseObject::GetClassType()->GetName()`.
    /// Returns an empty string only if Noesis reports no class type (not
    /// expected for a successfully-loaded root).
    #[must_use]
    pub fn type_name(&self) -> String {
        // SAFETY: `self.ptr` is a live BaseComponent* for the lifetime of self.
        let name = unsafe { noesis_base_component_type_name(self.ptr.as_ptr()) };
        if name.is_null() {
            String::new()
        } else {
            // Interned, process-stable string; copy it immediately anyway.
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

/// Load XAML by `uri` through the installed [`crate::xaml_provider`], keeping
/// the root whatever its type. Returns `None` when the URI is unknown to the
/// provider or the XAML is malformed.
///
/// This is the typed sibling of [`crate::view::FrameworkElement::load`], which
/// narrows the root to `FrameworkElement` and so returns `None` for roots like
/// `ResourceDictionary`. Inspect the loaded type via
/// [`LoadedComponent::type_name`].
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
