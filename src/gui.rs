//! Thin wrappers around the top-level `Noesis::GUI::*` helpers that don't
//! fit into the provider / view / render-device modules.

use std::ffi::CString;
use std::os::raw::{c_char, c_void};

use crate::ffi::{
    noesis_gui_install_app_resources_chain, noesis_gui_load_application_resources,
    noesis_gui_load_component,
};

/// Load a [`ResourceDictionary`] XAML via the installed XAML provider and
/// install it as the process-global application resources — every
/// [`crate::view::View`] created afterwards inherits these styles and
/// brushes. Replaces any previously-installed dictionary.
///
/// Returns `true` when the URI resolved to a valid
/// `ResourceDictionary`; `false` when the provider didn't serve bytes or
/// when the XAML parsed to a different root element.
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

/// Install application resources by building the merged-dictionary
/// chain manually, leaf by leaf. `uris` are the leaf
/// `ResourceDictionary` URIs in dependency order — earlier entries
/// must be loadable without referencing later entries.
///
/// Sidesteps a Noesis behaviour where a top-level `LoadXaml` of a
/// parent dictionary parses its `MergedDictionaries` children in
/// isolation, leaving cross-sibling `{StaticResource SiblingKey}`
/// references inside child bodies null-resolved at parse time.
///
/// Each leaf is created empty, added to the parent's
/// `MergedDictionaries` collection (so the parent scope is wired in
/// before parsing starts), then loaded by assigning its `Source`
/// property — at which point the parent already contains every
/// previously-loaded sibling.
///
/// # Relative URIs in installed leaves
///
/// Each leaf is loaded via `ResourceDictionary::SetSource(Uri)`,
/// which means relative URIs *inside* a leaf — most notably
/// `<FontFamily>Folder/#Family</FontFamily>` resources — resolve
/// against the leaf's own location. A `Theme/Fonts.xaml` leaf
/// declaring `<FontFamily>Fonts/#X</FontFamily>` will look for
/// family `X` in folder `Theme/Fonts/`, not the project-root
/// `Fonts/`. If your font provider's `register_font` calls register
/// under `Fonts/`, the corresponding leaf needs to use a relative-up
/// URI (`../Fonts/#X`) — or the leaf needs to live at the same
/// directory level as the assets it references. `AoR`'s original
/// theme uses absolute `/Assets/Fonts/...` URIs to sidestep this;
/// in our setup the equivalent is the relative-up form.
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

/// Load the XAML at `uri` into an existing component instance — the
/// code-behind / `x:Class` pattern, where the root object already exists and
/// `GUI::LoadComponent` populates its children and named fields in place
/// (instead of constructing a fresh tree the way [`crate::view::FrameworkElement::load`]
/// does).
///
/// Returns `false` when `component` is null or `uri` is empty/unresolvable.
/// A `true` return means the call linked and ran; it does **not** by itself
/// guarantee the tree was populated.
///
/// # Reflection requirement / limitation
///
/// For `LoadComponent` to actually graft the parsed tree onto `component`, the
/// instance's reflected type must match the XAML root's `x:Class`. Noesis maps
/// the root element back onto the supplied instance by type identity; a
/// mismatch leaves the instance untouched (and Noesis logs a type error).
/// The custom-class registration surface ([`crate::classes`]) supplies exactly
/// such a type: register a class as `"DM.LoadTarget"`, instantiate it, and load
/// XAML whose root carries `x:Class="DM.LoadTarget"` — the parsed children and
/// named fields are grafted onto that instance (verified by `tests/parse_xaml`,
/// which asserts a named child becomes resolvable through the instance only
/// after this call). The caller is responsible for ensuring the registered type
/// name and the XAML `x:Class` agree; this entry point does not synthesize that
/// pairing on its own.
///
/// # Safety
///
/// `component` must be a live `Noesis::BaseComponent*` (for example a
/// [`crate::classes::ClassInstance::raw`] value) that outlives the call, or
/// null. The pointer is borrowed — ownership is not taken and the caller's
/// reference is unaffected. Runs on the view-driving thread; no `VerifyAccess`
/// is performed.
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
