//! Safe Rust bindings to the Noesis GUI native SDK, a XAML-based UI engine.
//!
//! The crate builds UI trees, drives input, and ticks layout and animation.
//! It does no drawing itself: Noesis renders through a
//! [`render_device::RenderDevice`] you implement for your GPU API. The sibling
//! crate `noesis_bevy` provides one for Bevy/wgpu.
//!
//! # Getting started
//!
//! Call [`init`] once at startup and [`shutdown`] once at exit, after every
//! Noesis handle has been dropped. In between:
//!
//! 1. Install an asset source with [`xaml_provider::set_xaml_provider`] (and,
//!    optionally, [`font_provider`] and [`texture_provider`] sources).
//! 2. Build a root element with [`view::FrameworkElement::load`] (from a URI)
//!    or [`view::FrameworkElement::parse`] (from a XAML string).
//! 3. Host it in a [`view::View`] with [`view::View::create`], then feed it the
//!    surface size, input events, and the current time each frame.
//! 4. Register your device with [`render_device::register`], bind it with
//!    [`view::Renderer::init`], and render each frame.
//!
//! ```no_run
//! use noesis_runtime::render_device::Registered;
//! use noesis_runtime::view::{FrameworkElement, View};
//!
//! fn run(device: &Registered, time_seconds: f64) {
//!     noesis_runtime::init();
//!
//!     let root = FrameworkElement::parse(
//!         r#"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">
//!                <TextBlock Text="Hello"/>
//!            </Grid>"#,
//!     )
//!     .expect("XAML parses");
//!     let mut view = View::create(root);
//!     view.set_size(1280, 720);
//!     view.renderer().init(device);
//!
//!     // Each frame:
//!     view.update(time_seconds);
//!     let mut renderer = view.renderer();
//!     renderer.update_render_tree();
//!     renderer.render_offscreen();
//!     renderer.render(false, true);
//!
//!     // Teardown: release device resources, drop every handle, then shut down.
//!     renderer.shutdown();
//!     drop(view);
//!     noesis_runtime::shutdown();
//! }
//! ```
//!
//! The [`prelude`] re-exports the types most code uses.
//!
//! # Setup
//!
//! Building requires the Noesis Native SDK 3.2.13. Set `NOESIS_SDK_DIR` to its
//! root (the directory containing `Include/` and `Bin/`). See `README.md`.
//!
//! # Features
//!
//! - `shim` (default): compiles the C++ shim and links the Noesis library.
//!   Everything except [`render_device`] needs it. Turn default features off to
//!   get only the render-device types and the
//!   [`RenderDevice`](render_device::RenderDevice) trait, with no SDK, no C++
//!   build and no link. That is for a render device used by a host that loads
//!   Noesis some other way, such as the managed (C#) SDK.
//! - `test-utils`: compiles the shim's test entry points and the tests that use
//!   them. Implies `shim`.
//!
//! # Thread affinity
//!
//! Noesis objects are thread-affine: every call on an object, and on the
//! [`view::View`] that owns it, must come from the one thread that drives that
//! view. Owning handles in this crate are therefore [`Send`] but not [`Sync`]:
//!
//! - `Send` is sound because Noesis reference counts are atomic, so the release
//!   in [`Drop`] is safe on any thread. You can move a handle to the view's
//!   thread and use it there.
//! - `Sync` would be unsound because many `&self` methods call into Noesis, and
//!   some mutate engine state lazily. Shared references across threads would
//!   race.
//!
//! The `unsafe impl Send` blocks throughout the crate rely on this contract.
//! [`render_device::RenderDevice`] is the exception: implementations must be
//! `Send + Sync` because Noesis may call them from a dedicated render thread.

#[cfg(feature = "shim")]
use std::ffi::{CStr, CString};

#[cfg(feature = "shim")]
pub mod animation;
#[cfg(feature = "shim")]
pub mod binding;
#[cfg(feature = "shim")]
pub mod brushes;
#[cfg(feature = "shim")]
pub mod classes;
#[cfg(feature = "shim")]
pub mod collection_view;
#[cfg(feature = "shim")]
pub mod commands;
#[cfg(feature = "shim")]
pub mod converters;
#[cfg(feature = "shim")]
pub mod diagnostics;
#[cfg(feature = "shim")]
pub mod drawing;
#[cfg(feature = "shim")]
pub mod element_tree;
#[cfg(feature = "shim")]
pub mod events;
// Not part of the stable API; no semver guarantees.
#[cfg(feature = "shim")]
#[doc(hidden)]
pub mod ffi;
#[cfg(feature = "shim")]
pub mod font_provider;
#[cfg(feature = "shim")]
pub mod formatted_text;
#[cfg(feature = "shim")]
pub mod geometry;
#[cfg(feature = "shim")]
pub mod gui;
#[cfg(feature = "shim")]
pub mod imaging;
#[cfg(feature = "shim")]
pub mod input;
#[cfg(feature = "shim")]
pub mod integration;
#[cfg(feature = "shim")]
pub mod markup;
#[cfg(feature = "shim")]
pub mod mesh;
#[cfg(feature = "shim")]
pub mod multi_binding;
#[cfg(feature = "shim")]
pub mod name_scope;
#[cfg(feature = "shim")]
pub(crate) mod panic_guard;
#[cfg(feature = "shim")]
pub mod plain_vm;
#[cfg(feature = "shim")]
pub mod reflection;
pub mod render_device;
#[cfg(feature = "shim")]
pub mod resources;
#[cfg(feature = "shim")]
pub mod shapes;
#[cfg(feature = "shim")]
pub mod styles;
#[cfg(feature = "shim")]
pub mod svg;
#[cfg(feature = "shim")]
pub mod text_inlines;
#[cfg(feature = "shim")]
pub mod texture_provider;
#[cfg(feature = "shim")]
pub mod transforms;
#[cfg(feature = "shim")]
pub mod typography;
#[cfg(feature = "shim")]
pub mod view;
#[cfg(feature = "shim")]
pub mod xaml;
#[cfg(feature = "shim")]
pub mod xaml_provider;

/// Applies Noesis license credentials. Call before [`init`]; without a license
/// Noesis runs in trial mode and draws a watermark.
///
/// # Panics
///
/// Panics if `name` or `key` contain interior NUL bytes.
#[cfg(feature = "shim")]
pub fn set_license(name: &str, key: &str) {
    let n = CString::new(name).expect("license name contained NUL");
    let k = CString::new(key).expect("license key contained NUL");
    // SAFETY: pointers live for the duration of the call; the shim copies into Noesis.
    unsafe { ffi::noesis_set_license(n.as_ptr(), k.as_ptr()) }
}

/// Disables Hot Reload. Call before [`init`].
///
/// Hot Reload is on by default in Debug and Profile SDK builds and costs some
/// memory. This is a no-op after [`init`] and on a Release SDK build, where the
/// feature is compiled out.
#[cfg(feature = "shim")]
pub fn disable_hot_reload() {
    // SAFETY: a pre-init GUI:: free call with no arguments or preconditions
    // beyond "call before Init", which is the caller's contract.
    unsafe { ffi::noesis_disable_hot_reload() }
}

/// Skips the Inspector's socket initialization (`WSAStartup` on Windows). Call
/// before [`init`], and only when the host has already initialized sockets.
///
/// No-op after [`init`] and on a Release SDK build.
#[cfg(feature = "shim")]
pub fn disable_socket_init() {
    // SAFETY: pre-init GUI:: free call; see `disable_hot_reload`.
    unsafe { ffi::noesis_disable_socket_init() }
}

/// Disables remote Inspector connections. Call before [`init`].
///
/// Debug and Profile SDK builds open a socket for the Inspector by default.
/// No-op after [`init`] and on a Release SDK build, where the Inspector is
/// compiled out.
#[cfg(feature = "shim")]
pub fn disable_inspector() {
    // SAFETY: pre-init GUI:: free call; see `disable_hot_reload`.
    unsafe { ffi::noesis_disable_inspector() }
}

/// Returns whether a remote Inspector is connected. Always `false` on a
/// Release SDK build.
#[cfg(feature = "shim")]
#[must_use]
pub fn is_inspector_connected() -> bool {
    // SAFETY: runtime GUI:: query; safe to call any time, returns false if the
    // Inspector subsystem is absent.
    unsafe { ffi::noesis_is_inspector_connected() }
}

/// Keeps the Inspector connection alive. [`View::update`](view::View::update)
/// does this internally, so you only need it while no view exists. No-op on a
/// Release SDK build.
#[cfg(feature = "shim")]
pub fn update_inspector() {
    // SAFETY: runtime GUI:: call; safe to call any time (no-op without an
    // active Inspector connection).
    unsafe { ffi::noesis_update_inspector() }
}

/// Initializes Noesis. Call exactly once per process, before creating any
/// Noesis object. Noesis cannot be initialized again after [`shutdown`].
#[cfg(feature = "shim")]
pub fn init() {
    // SAFETY: no preconditions other than "call once", documented by Noesis.
    unsafe { ffi::noesis_init() }
}

/// Shuts Noesis down. Call once at exit, after every handle from this crate
/// has been dropped; dropping one afterwards touches freed engine state.
#[cfg(feature = "shim")]
pub fn shutdown() {
    // SAFETY: caller responsibility per docs.
    unsafe { ffi::noesis_shutdown() }
}

/// Re-exports of the most-used items: views and elements, brushes,
/// transforms, geometry, data binding, custom classes and markup extensions,
/// the asset-provider traits, the lifecycle functions, and common enums.
///
/// Everything else is reached through its module ([`animation`], [`input`],
/// [`diagnostics`], ...).
///
/// ```no_run
/// use noesis_runtime::prelude::*;
///
/// noesis_runtime::init();
/// let mut items = ObservableCollection::new();
/// assert!(items.is_empty());
/// items.push_string("first");
/// items.push_string("second");
/// assert_eq!(items.len(), 2);
///
/// let mut brush = SolidColorBrush::new([1.0, 0.0, 0.0, 1.0]);
/// brush.set_color([0.0, 1.0, 0.0, 1.0]);
/// assert_eq!(brush.color(), [0.0, 1.0, 0.0, 1.0]);
/// noesis_runtime::shutdown();
/// ```
#[cfg(feature = "shim")]
pub mod prelude {
    pub use crate::{init, set_license, shutdown, version};

    pub use crate::view::{FrameworkElement, View};

    pub use crate::binding::{Binding, BindingMode, ObservableCollection, UpdateSourceTrigger};

    pub use crate::brushes::{
        Brush, Effect, GradientStop, ImageBrush, LinearGradientBrush, RadialGradientBrush,
        SolidColorBrush, Stretch,
    };

    pub use crate::transforms::{
        RotateTransform, ScaleTransform, Transform, TransformGroup, TranslateTransform,
    };

    pub use crate::geometry::{
        EllipseGeometry, FillRule, Geometry, LineGeometry, PathGeometry, Rect, RectangleGeometry,
    };

    pub use crate::resources::ResourceDictionary;
    pub use crate::styles::{ControlTemplate, Style};

    pub use crate::classes::{
        ClassBuilder, ClassRegistration, PropertyChangeHandler, PropertyValue,
    };
    pub use crate::ffi::{ClassBase, PropType};
    pub use crate::markup::MarkupExtensionRegistration;

    pub use crate::font_provider::FontProvider;
    pub use crate::texture_provider::TextureProvider;
    pub use crate::xaml_provider::{XamlProvider, set_xaml_provider};

    pub use crate::view::{HAlign, Key, MouseButton, VAlign};
}

/// Returns the version of the linked Noesis runtime, such as `"3.2.13"`, or an
/// empty string if Noesis reports none.
#[cfg(feature = "shim")]
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
