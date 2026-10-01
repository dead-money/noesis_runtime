//! Process-global `Noesis::GUI` helpers: application-wide resource
//! dictionaries and loading XAML into an existing object.

use std::ffi::CString;
use std::os::raw::{c_char, c_void};

use crate::ffi::{
    noesis_gui_install_app_resources_chain, noesis_gui_load_application_resources,
    noesis_gui_load_component,
};

/// Loads a [`ResourceDictionary`] XAML through the installed XAML provider
/// and installs it as the process-global application resources, replacing any
/// previous dictionary. Every [`View`](crate::view::View) created afterwards
/// inherits its styles and brushes.
///
/// Returns `false` if the provider served nothing for `uri` or the XAML root
/// is not a `ResourceDictionary`.
///
/// [`ResourceDictionary`]: https://docs.noesisengine.com/gui/ResourceDictionary.html
///
/// # Panics
///
/// Panics if `uri` contains an interior NUL byte.
pub fn load_application_resources(uri: &str) -> bool {
    let c = CString::new(uri).expect("uri contained NUL");
    // SAFETY: c.as_ptr() lives for the duration of the call; the shim
    // only reads it.
    unsafe { noesis_gui_load_application_resources(c.as_ptr()) }
}

/// Installs application resources as a chain of leaf dictionaries, loaded one
/// at a time in the order given. Use this instead of
/// [`load_application_resources`] when leaves reference each other's keys.
///
/// `uris` are leaf `ResourceDictionary` URIs in dependency order: a leaf may
/// use `{StaticResource}` keys from earlier leaves, not later ones. A fresh,
/// empty parent dictionary is installed as the application resources first,
/// replacing any previous one. Each leaf is then added to the parent's
/// `MergedDictionaries` before its `Source` is set, so it parses with every
/// earlier sibling already in scope. Loading a single parent dictionary with
/// [`load_application_resources`] instead parses each merged child in
/// isolation, and cross-sibling `{StaticResource}` references resolve to null.
///
/// Returns `false` only when `uris` is empty. A leaf that fails to load is not
/// reported.
///
/// # Relative URIs inside a leaf
///
/// Relative URIs inside a leaf resolve against the leaf's own location. A
/// `Theme/Fonts.xaml` leaf declaring `<FontFamily>Fonts/#X</FontFamily>` looks
/// for family `X` in `Theme/Fonts/`, not the root `Fonts/`. Use a relative-up
/// URI (`../Fonts/#X`) or an absolute one (`/Assets/Fonts/#X`), or keep the leaf
/// at the same directory level as the assets it references.
///
/// # Panics
///
/// Panics if any URI contains an interior NUL byte.
#[must_use]
pub fn install_app_resources_chain<S: AsRef<str>>(uris: &[S]) -> bool {
    if uris.is_empty() {
        return false;
    }
    let cstrings: Vec<CString> = uris
        .iter()
        .map(|s| CString::new(s.as_ref()).expect("uri contained NUL"))
        .collect();
    let ptrs: Vec<*const c_char> = cstrings.iter().map(|c| c.as_ptr()).collect();
    // SAFETY: the C side reads `count` pointers, each valid for the
    // duration of the call; the parent dictionary it constructs holds
    // its own refs on the loaded children.
    unsafe { noesis_gui_install_app_resources_chain(ptrs.as_ptr(), ptrs.len() as u32) }
}

/// Loads the XAML at `uri` into an existing object (the code-behind /
/// `x:Class` pattern). Noesis populates the instance's children and named
/// fields in place instead of building a new tree the way
/// [`FrameworkElement::load`](crate::view::FrameworkElement::load) does.
///
/// The instance's reflected type must match the XAML root's `x:Class`. On a
/// mismatch Noesis logs a type error and leaves the instance untouched. To get
/// a matching type, register a class with [`crate::classes`] (say
/// `"Nz.LoadTarget"`), instantiate it, and load XAML whose root declares
/// `x:Class="Nz.LoadTarget"`. Keeping the two names in agreement is up to you.
///
/// Returns `false` if `component` is null. `true` means the load ran, not that
/// the tree was populated: an unresolvable `uri` or a type mismatch still
/// returns `true`.
///
/// # Safety
///
/// `component` must be null or a live `Noesis::BaseComponent*` (for example
/// [`ClassInstance::raw`](crate::classes::ClassInstance::raw)) that outlives the
/// call. It is borrowed; no reference is taken or released. Call this on the
/// thread driving the views; no thread check is performed.
///
/// # Panics
///
/// Panics if `uri` contains an interior NUL byte.
#[must_use]
pub unsafe fn load_component(component: *mut c_void, uri: &str) -> bool {
    if component.is_null() {
        return false;
    }
    let c = CString::new(uri).expect("uri contained NUL");
    // SAFETY: `component` is a caller-guaranteed live BaseComponent* (or was
    // null, handled above); `c` outlives the call. The shim borrows both.
    unsafe { noesis_gui_load_component(component, c.as_ptr()) }
}
