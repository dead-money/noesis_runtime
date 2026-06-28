//! Thin wrappers around the top-level `Noesis::GUI::*` helpers that don't
//! fit into the provider / view / render-device modules.

use std::ffi::CString;
use std::os::raw::c_char;

use crate::ffi::{
    dm_noesis_gui_install_app_resources_chain, dm_noesis_gui_load_application_resources,
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
    unsafe { dm_noesis_gui_load_application_resources(c.as_ptr()) }
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
/// directory level as the assets it references. AoR's original
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
    unsafe { dm_noesis_gui_install_app_resources_chain(ptrs.as_ptr(), ptrs.len() as u32) }
}
