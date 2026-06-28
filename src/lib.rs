//! FFI to the Noesis GUI Native SDK.
//!
//! Renderer-agnostic — Bevy/wgpu integration lives in the sibling crate
//! `noesis_bevy`. See `../noesis_bevy/CLAUDE.md` for the phase plan.
//!
//! Currently at Phase 0: lifecycle only.
//!
//! # Setup
//!
//! Set `NOESIS_SDK_DIR` to the extracted Noesis Native SDK 3.2.13 root (the
//! directory containing `Include/` and `Bin/`). See `README.md`.
//!
//! # Thread affinity
//!
//! Noesis objects are **thread-affine**: the engine expects every method on a
//! given object (and on the [`view::View`] that owns it) to be called from the
//! one thread that drives that view. The owning handle types in this crate
//! (geometries, brushes, transforms, commands, bindings, the RAII registration
//! guards, etc.) therefore implement [`Send`] but deliberately **not** [`Sync`]:
//!
//! - **`Send` is sound.** `Noesis::BaseRefCounted::mRefCount` is an
//!   `AtomicInteger`, so the `-1` release performed by [`Drop`] is safe on any
//!   thread. Ownership of a handle may be *moved* to whichever thread owns the
//!   Noesis view, after which all calls happen on that thread.
//! - **`Sync` is unsound.** Many `&self` methods call into Noesis (FFI reads,
//!   and some lazily mutate engine state — e.g. resource-dictionary or
//!   collection getters). Sharing a `&handle` across threads would let two
//!   threads invoke those concurrently on a thread-affine engine, which races.
//!
//! Callers must invoke Noesis methods only from the view's thread. SAFETY
//! comments on the individual `unsafe impl Send` blocks point back here rather
//! than restating this contract. (The render-device *trait* is the exception:
//! a user [`render_device::RenderDevice`] impl must be `Send + Sync` because
//! Noesis may call its trampolines from a dedicated render thread — that bound
//! is on the trait, not on these owning handles.)

use std::ffi::{CStr, CString};

pub mod animation;
pub mod binding;
pub mod brushes;
pub mod classes;
pub mod collection_view;
pub mod commands;
pub mod converters;
pub mod diagnostics;
pub mod drawing;
pub mod element_tree;
pub mod events;
// Not part of the stable API; no semver guarantees.
#[doc(hidden)]
pub mod ffi;
pub mod font_provider;
pub mod formatted_text;
pub mod geometry;
pub mod gui;
pub mod imaging;
pub mod input;
pub mod integration;
pub mod markup;
pub mod mesh;
pub mod multi_binding;
pub mod name_scope;
pub(crate) mod panic_guard;
pub mod plain_vm;
pub mod reflection;
pub mod render_device;
pub mod resources;
pub mod shapes;
pub mod styles;
pub mod svg;
pub mod text_inlines;
pub mod texture_provider;
pub mod transforms;
pub mod typography;
pub mod view;
pub mod xaml;
pub mod xaml_provider;

/// Optional. Apply Indie license credentials before [`init`] to suppress the
/// trial watermark. Pass empty strings (or skip the call) to run in trial mode.
///
/// # Panics
///
/// Panics if `name` or `key` contain interior NUL bytes.
pub fn set_license(name: &str, key: &str) {
    let n = CString::new(name).expect("license name contained NUL");
    let k = CString::new(key).expect("license key contained NUL");
    // SAFETY: pointers live for the duration of the call; the shim copies into Noesis.
    unsafe { ffi::noesis_set_license(n.as_ptr(), k.as_ptr()) }
}

/// Disable the Hot Reload feature before [`init`]. Hot Reload is on by default
/// in Debug/Profile SDK builds and costs a little extra memory; disabling it is
/// purely an optimization. No-op once [`init`] has run, and a no-op on a
/// Release dylib where the feature is compiled out.
///
/// Part of the inspector / hot-reload control surface (see
/// [`disable_inspector`], [`disable_socket_init`], [`is_inspector_connected`],
/// [`update_inspector`]). There is intentionally no `enable_*` counterpart:
/// these features default on in instrumented SDK builds, so we only expose the
/// off switches plus the runtime queries.
///
/// Must be called **before** [`init`].
pub fn disable_hot_reload() {
    // SAFETY: a pre-init GUI:: free call with no arguments or preconditions
    // beyond "call before Init", which is the caller's contract.
    unsafe { ffi::noesis_disable_hot_reload() }
}

/// Skip the Inspector's socket initialization (e.g. `WSAStartup` on Windows)
/// before [`init`]. Use this only when the host process has already initialized
/// sockets itself, to avoid a double init. No-op after [`init`] / on a Release
/// dylib.
///
/// Must be called **before** [`init`].
pub fn disable_socket_init() {
    // SAFETY: pre-init GUI:: free call; see `disable_hot_reload`.
    unsafe { ffi::noesis_disable_socket_init() }
}

/// Disable all remote Inspector connections before [`init`]. The Inspector is
/// enabled by default in Debug/Profile SDK builds (it opens a socket and waits
/// for the remote tool); call this to keep it off. No-op after [`init`] / on a
/// Release dylib where the Inspector is compiled out.
///
/// Must be called **before** [`init`].
pub fn disable_inspector() {
    // SAFETY: pre-init GUI:: free call; see `disable_hot_reload`.
    unsafe { ffi::noesis_disable_inspector() }
}

/// Returns whether a remote Inspector is currently connected.
///
/// Always `false` when nothing is attached, and always `false` on a Release
/// dylib (the Inspector is compiled out of Release SDK builds). The value of
/// exposing it is the query itself plus the [`update_inspector`] pump for hosts
/// running an instrumented build.
#[must_use]
pub fn is_inspector_connected() -> bool {
    // SAFETY: runtime GUI:: query; safe to call any time, returns false if the
    // Inspector subsystem is absent.
    unsafe { ffi::noesis_is_inspector_connected() }
}

/// Keep the Inspector connection alive. [`crate::view::View`] updates call this
/// internally, so it is only needed when the Inspector connects before any view
/// exists. No-op on a Release dylib.
pub fn update_inspector() {
    // SAFETY: runtime GUI:: call; safe to call any time (no-op without an
    // active Inspector connection).
    unsafe { ffi::noesis_update_inspector() }
}

/// Initialize Noesis subsystems. Call exactly once per process; Noesis does
/// not support re-init after [`shutdown`].
pub fn init() {
    // SAFETY: no preconditions other than "call once" — documented by Noesis.
    unsafe { ffi::noesis_init() }
}

/// Shut Noesis down. Call once at process exit, after all Noesis-owned objects
/// have been released.
pub fn shutdown() {
    // SAFETY: caller responsibility per docs.
    unsafe { ffi::noesis_shutdown() }
}

/// Curated re-exports of the items most code reaches for. Glob-import it
/// (`use noesis_runtime::prelude::*;`) to pull in the core view/element
/// handles, the brush/transform/geometry traits and their common concrete
/// types, data-binding and collection types, the custom-control and
/// markup-extension surface, the provider traits, the lifecycle free functions,
/// and the most-used enums.
///
/// This is a convenience surface, not the full API — anything not listed here is
/// still reachable through its owning module (`crate::animation`,
/// `crate::input`, `crate::diagnostics`, …).
///
/// Compiled (so the names stay honest) but not executed: Noesis [`init`] runs
/// once per process, and `cargo test` merges all doctests into one binary.
///
/// ```no_run
/// use noesis_runtime::prelude::*;
///
/// noesis_runtime::init();
/// // Freestanding objects round-trip through the FFI without a live view.
/// let mut items = ObservableCollection::new();
/// assert!(items.is_empty());
/// items.push_string("first");
/// items.push_string("second");
/// assert_eq!(items.len(), 2);
///
/// // Build a brush and read its color back across the FFI boundary.
/// let mut brush = SolidColorBrush::new([1.0, 0.0, 0.0, 1.0]);
/// brush.set_color([0.0, 1.0, 0.0, 1.0]);
/// assert_eq!(brush.color(), [0.0, 1.0, 0.0, 1.0]);
/// noesis_runtime::shutdown();
/// ```
pub mod prelude {
    // Lifecycle & runtime free functions.
    pub use crate::{init, set_license, shutdown, version};

    // Core view / element handles.
    pub use crate::view::{FrameworkElement, View};

    // Data binding & collections.
    pub use crate::binding::{Binding, BindingMode, ObservableCollection, UpdateSourceTrigger};

    // Brushes & effects (traits + common concrete types).
    pub use crate::brushes::{
        Brush, Effect, GradientStop, ImageBrush, LinearGradientBrush, RadialGradientBrush,
        SolidColorBrush, Stretch,
    };

    // Transforms (trait + common concrete types).
    pub use crate::transforms::{
        RotateTransform, ScaleTransform, Transform, TransformGroup, TranslateTransform,
    };

    // Geometry (trait + common concrete types).
    pub use crate::geometry::{
        EllipseGeometry, FillRule, Geometry, LineGeometry, PathGeometry, Rect, RectangleGeometry,
    };

    // Resources, styles, templates.
    pub use crate::resources::ResourceDictionary;
    pub use crate::styles::{ControlTemplate, Style};

    // Custom controls & markup extensions.
    pub use crate::classes::{
        ClassBuilder, ClassRegistration, PropertyChangeHandler, PropertyValue,
    };
    pub use crate::ffi::{ClassBase, PropType};
    pub use crate::markup::MarkupExtensionRegistration;

    // Asset providers (traits + the XAML installer).
    pub use crate::font_provider::FontProvider;
    pub use crate::texture_provider::TextureProvider;
    pub use crate::xaml_provider::{XamlProvider, set_xaml_provider};

    // Most-used enums.
    pub use crate::view::{HAlign, Key, MouseButton, VAlign};
}

/// Returns the Noesis runtime build version (e.g. `"3.2.13"`).
#[must_use]
pub fn version() -> String {
    // SAFETY: version string is owned by the Noesis runtime and stays valid for
    // the lifetime of the process; we copy it into an owned String.
    let p = unsafe { ffi::noesis_version() };
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}
