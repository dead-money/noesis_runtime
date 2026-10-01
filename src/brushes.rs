//! Brushes and effects built in code, for painting elements without XAML.
//!
//! Brushes: [`SolidColorBrush`], [`LinearGradientBrush`],
//! [`RadialGradientBrush`], [`ImageBrush`] and [`VisualBrush`]. Effects:
//! [`BlurEffect`] and [`DropShadowEffect`].
//!
//! Each type owns one reference to a new Noesis object and releases it on
//! drop. Assign one with the typed setters on
//! [`FrameworkElement`](crate::view::FrameworkElement)
//! ([`set_background`](crate::view::FrameworkElement::set_background),
//! [`set_foreground`](crate::view::FrameworkElement::set_foreground),
//! [`set_fill`](crate::view::FrameworkElement::set_fill),
//! [`set_stroke`](crate::view::FrameworkElement::set_stroke),
//! [`set_effect`](crate::view::FrameworkElement::set_effect)). The element
//! takes its own reference, so the Rust handle can be dropped after the call;
//! keep it if you want to change the brush later, since edits to a shared
//! brush show on every element that uses it.
//!
//! ```no_run
//! # use noesis_runtime::brushes::SolidColorBrush;
//! # use noesis_runtime::view::FrameworkElement;
//! # fn paint(element: &mut FrameworkElement) {
//! let brush = SolidColorBrush::new([1.0, 0.0, 0.0, 1.0]);
//! element.set_background(&brush);
//! # }
//! ```
//!
//! Colors are `[r, g, b, a]` floats in `0..=1`. Getters such as
//! [`SolidColorBrush::color`] read the live Noesis object, not a Rust-side copy.

use core::ptr::NonNull;
use std::ffi::c_void;

use crate::ffi::{
    noesis_base_component_release, noesis_blur_effect_create, noesis_blur_effect_get_radius,
    noesis_blur_effect_set_radius, noesis_drop_shadow_effect_create, noesis_drop_shadow_effect_get,
    noesis_drop_shadow_effect_set_blur_radius, noesis_drop_shadow_effect_set_color,
    noesis_drop_shadow_effect_set_direction, noesis_drop_shadow_effect_set_opacity,
    noesis_drop_shadow_effect_set_shadow_depth, noesis_gradient_brush_add_stop,
    noesis_gradient_brush_get_mapping_mode, noesis_gradient_brush_get_spread_method,
    noesis_gradient_brush_get_stop, noesis_gradient_brush_set_mapping_mode,
    noesis_gradient_brush_set_spread_method, noesis_gradient_brush_stop_count,
    noesis_image_brush_create, noesis_image_brush_get_image_source,
    noesis_image_brush_set_image_source, noesis_linear_gradient_brush_create,
    noesis_linear_gradient_brush_get_points, noesis_linear_gradient_brush_set_end_point,
    noesis_linear_gradient_brush_set_start_point, noesis_radial_gradient_brush_create,
    noesis_radial_gradient_brush_get_radius, noesis_radial_gradient_brush_set_center,
    noesis_radial_gradient_brush_set_gradient_origin, noesis_radial_gradient_brush_set_radius,
    noesis_solid_color_brush_create, noesis_solid_color_brush_get_color,
    noesis_solid_color_brush_set_color, noesis_tile_brush_get_alignment_x,
    noesis_tile_brush_get_alignment_y, noesis_tile_brush_get_stretch,
    noesis_tile_brush_get_tile_mode, noesis_tile_brush_get_viewbox,
    noesis_tile_brush_get_viewbox_units, noesis_tile_brush_get_viewport,
    noesis_tile_brush_get_viewport_units, noesis_tile_brush_set_alignment_x,
    noesis_tile_brush_set_alignment_y, noesis_tile_brush_set_stretch,
    noesis_tile_brush_set_tile_mode, noesis_tile_brush_set_viewbox,
    noesis_tile_brush_set_viewbox_units, noesis_tile_brush_set_viewport,
    noesis_tile_brush_set_viewport_units, noesis_visual_brush_create,
    noesis_visual_brush_get_visual, noesis_visual_brush_set_visual,
};

/// Any brush in this module. Element setters such as
/// [`FrameworkElement::set_background`](crate::view::FrameworkElement::set_background)
/// take `&impl Brush`, so they accept every brush type and nothing else.
pub trait Brush {
    /// Borrowed `Noesis::Brush*` (a `BaseComponent*`), valid for `self`'s
    /// lifetime. The element setters use it; you rarely need it directly.
    fn brush_raw(&self) -> *mut c_void;
}

/// Any effect in this module: a post-process applied to an element's rendered
/// output. Assign with
/// [`FrameworkElement::set_effect`](crate::view::FrameworkElement::set_effect).
pub trait Effect {
    /// Borrowed `Noesis::Effect*` (a `BaseComponent*`), valid for `self`'s
    /// lifetime.
    fn effect_raw(&self) -> *mut c_void;
}

macro_rules! base_component_handle {
    ($name:ident) => {
        // SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
        unsafe impl Send for $name {}

        impl $name {
            /// Raw `Noesis::BaseComponent*`. Borrowed for the lifetime of `self`.
            #[must_use]
            pub fn raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                // SAFETY: produced by a `*_create` entrypoint with a +1 ref that
                // we own; released exactly once here.
                unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
            }
        }
    };
}

/// Paints a single color.
pub struct SolidColorBrush {
    ptr: NonNull<c_void>,
}

base_component_handle!(SolidColorBrush);

impl SolidColorBrush {
    /// Creates a brush of `rgba` (`[r, g, b, a]`, each in `0..=1`).
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which does not happen after
    /// [`crate::init`].
    #[must_use]
    pub fn new(rgba: [f32; 4]) -> Self {
        // SAFETY: `rgba` outlives the call; the C side copies it into a Color.
        let ptr = unsafe { noesis_solid_color_brush_create(rgba.as_ptr()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_solid_color_brush_create returned null"),
        }
    }

    /// Replaces the color. Every element painted with this brush updates.
    pub fn set_color(&mut self, rgba: [f32; 4]) {
        // SAFETY: self.ptr is a live SolidColorBrush*; `rgba` outlives the call.
        unsafe {
            noesis_solid_color_brush_set_color(self.ptr.as_ptr(), rgba.as_ptr());
        }
    }

    /// The current color as `[r, g, b, a]`.
    #[must_use]
    pub fn color(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live SolidColorBrush*; `out` is a 4-float buffer.
        unsafe {
            noesis_solid_color_brush_get_color(self.ptr.as_ptr(), out.as_mut_ptr());
        }
        out
    }
}

impl Brush for SolidColorBrush {
    fn brush_raw(&self) -> *mut c_void {
        self.raw()
    }
}

/// A color at a position along a gradient.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GradientStop {
    /// Position of the stop along the gradient axis, `0..=1`.
    pub offset: f32,
    /// `[r, g, b, a]`, each `0..=1`.
    pub color: [f32; 4],
}

impl GradientStop {
    /// A stop of `color` at `offset`.
    #[must_use]
    pub fn new(offset: f32, color: [f32; 4]) -> Self {
        Self { offset, color }
    }
}

fn gradient_add_stop(ptr: *mut c_void, stop: GradientStop) -> Option<usize> {
    // SAFETY: `ptr` is a live GradientBrush*; `color` outlives the call.
    let idx = unsafe { noesis_gradient_brush_add_stop(ptr, stop.offset, stop.color.as_ptr()) };
    (idx >= 0).then_some(idx as usize)
}

fn gradient_stop_count(ptr: *mut c_void) -> usize {
    // SAFETY: `ptr` is a live GradientBrush*.
    let n = unsafe { noesis_gradient_brush_stop_count(ptr) };
    n.max(0) as usize
}

fn gradient_get_stop(ptr: *mut c_void, index: usize) -> Option<GradientStop> {
    let mut offset = 0.0f32;
    let mut color = [0.0f32; 4];
    // SAFETY: `ptr` is a live GradientBrush*; out params are valid buffers.
    let ok = unsafe {
        noesis_gradient_brush_get_stop(
            ptr,
            index as u32,
            &mut offset as *mut f32,
            color.as_mut_ptr(),
        )
    };
    ok.then_some(GradientStop { offset, color })
}

fn gradient_set_spread_method(ptr: *mut c_void, method: GradientSpreadMethod) -> bool {
    // SAFETY: `ptr` is a live GradientBrush*.
    unsafe { noesis_gradient_brush_set_spread_method(ptr, method as i32) }
}

fn gradient_spread_method(ptr: *mut c_void) -> Option<GradientSpreadMethod> {
    // SAFETY: `ptr` is a live GradientBrush*.
    GradientSpreadMethod::from_ordinal(unsafe { noesis_gradient_brush_get_spread_method(ptr) })
}

fn gradient_set_mapping_mode(ptr: *mut c_void, mode: BrushMappingMode) -> bool {
    // SAFETY: `ptr` is a live GradientBrush*.
    unsafe { noesis_gradient_brush_set_mapping_mode(ptr, mode as i32) }
}

fn gradient_mapping_mode(ptr: *mut c_void) -> Option<BrushMappingMode> {
    // SAFETY: `ptr` is a live GradientBrush*.
    BrushMappingMode::from_ordinal(unsafe { noesis_gradient_brush_get_mapping_mode(ptr) })
}

/// How a gradient paints the area outside its `0..=1` range.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GradientSpreadMethod {
    /// Fill the remaining space with the boundary colors (the XAML default).
    Pad = 0,
    /// Repeat the gradient in the reverse direction.
    Reflect = 1,
    /// Repeat the gradient in the original direction.
    Repeat = 2,
}

impl GradientSpreadMethod {
    fn from_ordinal(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Pad),
            1 => Some(Self::Reflect),
            2 => Some(Self::Repeat),
            _ => None,
        }
    }
}

/// Paints a gradient along the line from a start point to an end point.
///
/// Points default to `(0, 0)` and `(1, 1)` and, under the default
/// [`BrushMappingMode::RelativeToBoundingBox`], are fractions of the painted
/// area. A new brush has no stops; add them with [`add_stop`](Self::add_stop)
/// or build one with [`builder`](Self::builder).
pub struct LinearGradientBrush {
    ptr: NonNull<c_void>,
}

base_component_handle!(LinearGradientBrush);

impl Default for LinearGradientBrush {
    fn default() -> Self {
        Self::new()
    }
}

impl LinearGradientBrush {
    /// Creates a brush with no stops.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the brush.
    #[must_use]
    pub fn new() -> Self {
        let ptr = unsafe { noesis_linear_gradient_brush_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_linear_gradient_brush_create returned null"),
        }
    }

    /// Sets the start point, in the units chosen by
    /// [`set_mapping_mode`](Self::set_mapping_mode).
    pub fn set_start_point(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live LinearGradientBrush*.
        unsafe { noesis_linear_gradient_brush_set_start_point(self.ptr.as_ptr(), x, y) };
    }

    /// Sets the end point, in the units chosen by
    /// [`set_mapping_mode`](Self::set_mapping_mode).
    pub fn set_end_point(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live LinearGradientBrush*.
        unsafe { noesis_linear_gradient_brush_set_end_point(self.ptr.as_ptr(), x, y) };
    }

    /// The current `([start_x, start_y], [end_x, end_y])`.
    #[must_use]
    pub fn points(&self) -> ([f32; 2], [f32; 2]) {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live LinearGradientBrush*; `out` is 4 floats.
        unsafe { noesis_linear_gradient_brush_get_points(self.ptr.as_ptr(), out.as_mut_ptr()) };
        ([out[0], out[1]], [out[2], out[3]])
    }

    /// Appends a stop and returns its index. `None` only if Noesis rejects
    /// the call.
    pub fn add_stop(&mut self, stop: GradientStop) -> Option<usize> {
        gradient_add_stop(self.ptr.as_ptr(), stop)
    }

    /// Number of stops on the brush.
    #[must_use]
    pub fn stop_count(&self) -> usize {
        gradient_stop_count(self.ptr.as_ptr())
    }

    /// The stop at `index` (in insertion order), or `None` if out of range.
    #[must_use]
    pub fn stop(&self, index: usize) -> Option<GradientStop> {
        gradient_get_stop(self.ptr.as_ptr(), index)
    }

    /// Sets how the gradient paints outside its `0..=1` range. Default
    /// [`GradientSpreadMethod::Pad`]. Returns `true` on success.
    pub fn set_spread_method(&mut self, method: GradientSpreadMethod) -> bool {
        gradient_set_spread_method(self.ptr.as_ptr(), method)
    }

    /// The current spread method.
    #[must_use]
    pub fn spread_method(&self) -> Option<GradientSpreadMethod> {
        gradient_spread_method(self.ptr.as_ptr())
    }

    /// Sets whether the start and end points are absolute or fractions of the
    /// painted area. Default [`BrushMappingMode::RelativeToBoundingBox`].
    /// Returns `true` on success.
    pub fn set_mapping_mode(&mut self, mode: BrushMappingMode) -> bool {
        gradient_set_mapping_mode(self.ptr.as_ptr(), mode)
    }

    /// The current mapping mode.
    #[must_use]
    pub fn mapping_mode(&self) -> Option<BrushMappingMode> {
        gradient_mapping_mode(self.ptr.as_ptr())
    }

    /// Starts a [`LinearGradientBrushBuilder`].
    pub fn builder() -> LinearGradientBrushBuilder {
        LinearGradientBrushBuilder {
            brush: LinearGradientBrush::new(),
        }
    }
}

/// Chained construction of a [`LinearGradientBrush`]. Each method calls the
/// matching setter on the brush; finish with [`build`](Self::build).
///
/// ```no_run
/// # use noesis_runtime::brushes::{LinearGradientBrush, GradientSpreadMethod, BrushMappingMode};
/// let brush = LinearGradientBrush::builder()
///     .start(0.0, 0.0)
///     .end(1.0, 1.0)
///     .spread_method(GradientSpreadMethod::Reflect)
///     .mapping_mode(BrushMappingMode::RelativeToBoundingBox)
///     .stop(0.0, [1.0, 0.0, 0.0, 1.0])
///     .stop(1.0, [0.0, 0.0, 1.0, 1.0])
///     .build();
/// ```
#[must_use]
pub struct LinearGradientBrushBuilder {
    brush: LinearGradientBrush,
}

impl LinearGradientBrushBuilder {
    /// See [`LinearGradientBrush::set_start_point`].
    pub fn start(mut self, x: f32, y: f32) -> Self {
        self.brush.set_start_point(x, y);
        self
    }

    /// See [`LinearGradientBrush::set_end_point`].
    pub fn end(mut self, x: f32, y: f32) -> Self {
        self.brush.set_end_point(x, y);
        self
    }

    /// See [`GradientSpreadMethod`].
    pub fn spread_method(mut self, method: GradientSpreadMethod) -> Self {
        self.brush.set_spread_method(method);
        self
    }

    /// See [`BrushMappingMode`].
    pub fn mapping_mode(mut self, mode: BrushMappingMode) -> Self {
        self.brush.set_mapping_mode(mode);
        self
    }

    /// Appends a stop of `color` (`[r, g, b, a]`) at `offset` (`0..=1`).
    pub fn stop(mut self, offset: f32, color: [f32; 4]) -> Self {
        self.brush.add_stop(GradientStop { offset, color });
        self
    }

    /// Returns the brush.
    #[must_use]
    pub fn build(self) -> LinearGradientBrush {
        self.brush
    }
}

impl Brush for LinearGradientBrush {
    fn brush_raw(&self) -> *mut c_void {
        self.raw()
    }
}

/// Paints a gradient from a focal point outward to an ellipse.
///
/// The ellipse is set by [`set_center`](Self::set_center) and
/// [`set_radius`](Self::set_radius); the gradient's `0` offset sits at
/// [`set_gradient_origin`](Self::set_gradient_origin). Under the default
/// [`BrushMappingMode::RelativeToBoundingBox`] all three are fractions of the
/// painted area (center and origin default to `(0.5, 0.5)`, radii to `0.5`).
pub struct RadialGradientBrush {
    ptr: NonNull<c_void>,
}

base_component_handle!(RadialGradientBrush);

impl Default for RadialGradientBrush {
    fn default() -> Self {
        Self::new()
    }
}

impl RadialGradientBrush {
    /// Creates a brush with no stops.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the brush.
    #[must_use]
    pub fn new() -> Self {
        let ptr = unsafe { noesis_radial_gradient_brush_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_radial_gradient_brush_create returned null"),
        }
    }

    /// Sets the center of the outer ellipse.
    pub fn set_center(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live RadialGradientBrush*.
        unsafe { noesis_radial_gradient_brush_set_center(self.ptr.as_ptr(), x, y) };
    }

    /// Sets the focal point where the gradient starts.
    pub fn set_gradient_origin(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live RadialGradientBrush*.
        unsafe { noesis_radial_gradient_brush_set_gradient_origin(self.ptr.as_ptr(), x, y) };
    }

    /// Sets the horizontal and vertical radii of the outer ellipse.
    pub fn set_radius(&mut self, rx: f32, ry: f32) {
        // SAFETY: self.ptr is a live RadialGradientBrush*.
        unsafe { noesis_radial_gradient_brush_set_radius(self.ptr.as_ptr(), rx, ry) };
    }

    /// The current `(radius_x, radius_y)`.
    #[must_use]
    pub fn radius(&self) -> (f32, f32) {
        let mut rx = 0.0f32;
        let mut ry = 0.0f32;
        // SAFETY: self.ptr is a live RadialGradientBrush*; out params valid.
        unsafe {
            noesis_radial_gradient_brush_get_radius(
                self.ptr.as_ptr(),
                &mut rx as *mut f32,
                &mut ry as *mut f32,
            )
        };
        (rx, ry)
    }

    /// Appends a stop and returns its index. `None` only if Noesis rejects
    /// the call.
    pub fn add_stop(&mut self, stop: GradientStop) -> Option<usize> {
        gradient_add_stop(self.ptr.as_ptr(), stop)
    }

    /// Number of stops on the brush.
    #[must_use]
    pub fn stop_count(&self) -> usize {
        gradient_stop_count(self.ptr.as_ptr())
    }

    /// The stop at `index` (in insertion order), or `None` if out of range.
    #[must_use]
    pub fn stop(&self, index: usize) -> Option<GradientStop> {
        gradient_get_stop(self.ptr.as_ptr(), index)
    }

    /// Sets how the gradient paints outside its `0..=1` range. Default
    /// [`GradientSpreadMethod::Pad`]. Returns `true` on success.
    pub fn set_spread_method(&mut self, method: GradientSpreadMethod) -> bool {
        gradient_set_spread_method(self.ptr.as_ptr(), method)
    }

    /// The current spread method.
    #[must_use]
    pub fn spread_method(&self) -> Option<GradientSpreadMethod> {
        gradient_spread_method(self.ptr.as_ptr())
    }

    /// Sets whether center, origin and radii are absolute or fractions of the
    /// painted area. Default [`BrushMappingMode::RelativeToBoundingBox`].
    /// Returns `true` on success.
    pub fn set_mapping_mode(&mut self, mode: BrushMappingMode) -> bool {
        gradient_set_mapping_mode(self.ptr.as_ptr(), mode)
    }

    /// The current mapping mode.
    #[must_use]
    pub fn mapping_mode(&self) -> Option<BrushMappingMode> {
        gradient_mapping_mode(self.ptr.as_ptr())
    }

    /// Starts a [`RadialGradientBrushBuilder`].
    pub fn builder() -> RadialGradientBrushBuilder {
        RadialGradientBrushBuilder {
            brush: RadialGradientBrush::new(),
        }
    }
}

/// Chained construction of a [`RadialGradientBrush`]. Each method calls the
/// matching setter on the brush; finish with [`build`](Self::build).
///
/// ```no_run
/// # use noesis_runtime::brushes::{RadialGradientBrush, GradientSpreadMethod};
/// let brush = RadialGradientBrush::builder()
///     .center(0.5, 0.5)
///     .gradient_origin(0.5, 0.5)
///     .radius(0.5, 0.5)
///     .spread_method(GradientSpreadMethod::Pad)
///     .stop(0.0, [1.0, 1.0, 1.0, 1.0])
///     .stop(1.0, [0.0, 0.0, 0.0, 1.0])
///     .build();
/// ```
#[must_use]
pub struct RadialGradientBrushBuilder {
    brush: RadialGradientBrush,
}

impl RadialGradientBrushBuilder {
    /// See [`RadialGradientBrush::set_center`].
    pub fn center(mut self, x: f32, y: f32) -> Self {
        self.brush.set_center(x, y);
        self
    }

    /// See [`RadialGradientBrush::set_gradient_origin`].
    pub fn gradient_origin(mut self, x: f32, y: f32) -> Self {
        self.brush.set_gradient_origin(x, y);
        self
    }

    /// See [`RadialGradientBrush::set_radius`].
    pub fn radius(mut self, rx: f32, ry: f32) -> Self {
        self.brush.set_radius(rx, ry);
        self
    }

    /// See [`GradientSpreadMethod`].
    pub fn spread_method(mut self, method: GradientSpreadMethod) -> Self {
        self.brush.set_spread_method(method);
        self
    }

    /// See [`BrushMappingMode`].
    pub fn mapping_mode(mut self, mode: BrushMappingMode) -> Self {
        self.brush.set_mapping_mode(mode);
        self
    }

    /// Appends a stop of `color` (`[r, g, b, a]`) at `offset` (`0..=1`).
    pub fn stop(mut self, offset: f32, color: [f32; 4]) -> Self {
        self.brush.add_stop(GradientStop { offset, color });
        self
    }

    /// Returns the brush.
    #[must_use]
    pub fn build(self) -> RadialGradientBrush {
        self.brush
    }
}

impl Brush for RadialGradientBrush {
    fn brush_raw(&self) -> *mut c_void {
        self.raw()
    }
}

/// Horizontal placement of a tile brush's content within its tile.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AlignmentX {
    /// Align toward the left edge.
    Left = 0,
    /// Align toward the center.
    Center = 1,
    /// Align toward the right edge.
    Right = 2,
}

impl AlignmentX {
    fn from_ordinal(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Left),
            1 => Some(Self::Center),
            2 => Some(Self::Right),
            _ => None,
        }
    }
}

/// Vertical placement of a tile brush's content within its tile.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AlignmentY {
    /// Align toward the upper edge.
    Top = 0,
    /// Align toward the center.
    Center = 1,
    /// Align toward the lower edge.
    Bottom = 2,
}

impl AlignmentY {
    fn from_ordinal(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Top),
            1 => Some(Self::Center),
            2 => Some(Self::Bottom),
            _ => None,
        }
    }
}

/// How content is resized to fill its space. Used by [`TileBrush`] and by
/// [`Shape::set_stretch`](crate::shapes::Shape::set_stretch).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Stretch {
    /// Preserve original size.
    None = 0,
    /// Resize to fill, ignoring aspect ratio.
    Fill = 1,
    /// Resize to fit while preserving aspect ratio.
    Uniform = 2,
    /// Resize to fill while preserving aspect ratio (clips overflow).
    UniformToFill = 3,
}

impl Stretch {
    pub(crate) fn from_ordinal(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Fill),
            2 => Some(Self::Uniform),
            3 => Some(Self::UniformToFill),
            _ => None,
        }
    }
}

/// How a tile brush repeats its tile to fill the painted area.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TileMode {
    /// Draw the base tile once; the rest is transparent.
    None = 0,
    /// Repeat the base tile edge-to-edge.
    Tile = 1,
    /// Like [`Tile`](Self::Tile), flipping alternate columns horizontally.
    FlipX = 2,
    /// Like [`Tile`](Self::Tile), flipping alternate rows vertically.
    FlipY = 3,
    /// [`FlipX`](Self::FlipX) and [`FlipY`](Self::FlipY) together.
    FlipXY = 4,
}

impl TileMode {
    fn from_ordinal(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Tile),
            2 => Some(Self::FlipX),
            3 => Some(Self::FlipY),
            4 => Some(Self::FlipXY),
            _ => None,
        }
    }
}

/// Whether brush coordinates (gradient points, tile viewport and viewbox) are
/// absolute or fractions of the painted area.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BrushMappingMode {
    /// Absolute, in the painted element's local space.
    Absolute = 0,
    /// Fractions of the painted area's bounding box, `0..=1`.
    RelativeToBoundingBox = 1,
}

impl BrushMappingMode {
    fn from_ordinal(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Absolute),
            1 => Some(Self::RelativeToBoundingBox),
            _ => None,
        }
    }
}

/// Tiling settings shared by [`ImageBrush`] and [`VisualBrush`].
///
/// The *viewbox* selects the part of the content to show; the *viewport* is
/// the base tile it is drawn into, which [`TileMode`] then repeats. Both are
/// `[x, y, width, height]`, in the units set by
/// [`set_viewbox_units`](Self::set_viewbox_units) and
/// [`set_viewport_units`](Self::set_viewport_units).
///
/// Getters return `None` only if Noesis reports a value outside the Rust enum.
pub trait TileBrush: Brush {
    /// Sets the horizontal placement of content within the tile.
    fn set_alignment_x(&mut self, value: AlignmentX) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { noesis_tile_brush_set_alignment_x(self.brush_raw(), value as i32) };
    }
    /// The current horizontal placement.
    fn alignment_x(&self) -> Option<AlignmentX> {
        // SAFETY: brush_raw() is a live TileBrush*.
        AlignmentX::from_ordinal(unsafe { noesis_tile_brush_get_alignment_x(self.brush_raw()) })
    }

    /// Sets the vertical placement of content within the tile.
    fn set_alignment_y(&mut self, value: AlignmentY) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { noesis_tile_brush_set_alignment_y(self.brush_raw(), value as i32) };
    }
    /// The current vertical placement.
    fn alignment_y(&self) -> Option<AlignmentY> {
        // SAFETY: brush_raw() is a live TileBrush*.
        AlignmentY::from_ordinal(unsafe { noesis_tile_brush_get_alignment_y(self.brush_raw()) })
    }

    /// Sets how content is resized to fit the tile.
    fn set_stretch(&mut self, value: Stretch) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { noesis_tile_brush_set_stretch(self.brush_raw(), value as i32) };
    }
    /// The current stretch mode.
    fn stretch(&self) -> Option<Stretch> {
        // SAFETY: brush_raw() is a live TileBrush*.
        Stretch::from_ordinal(unsafe { noesis_tile_brush_get_stretch(self.brush_raw()) })
    }

    /// Sets how the tile repeats to fill the painted area.
    fn set_tile_mode(&mut self, value: TileMode) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { noesis_tile_brush_set_tile_mode(self.brush_raw(), value as i32) };
    }
    /// The current tile mode.
    fn tile_mode(&self) -> Option<TileMode> {
        // SAFETY: brush_raw() is a live TileBrush*.
        TileMode::from_ordinal(unsafe { noesis_tile_brush_get_tile_mode(self.brush_raw()) })
    }

    /// Sets the tile rectangle, `[x, y, width, height]`.
    fn set_viewport(&mut self, rect: [f32; 4]) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe {
            noesis_tile_brush_set_viewport(self.brush_raw(), rect[0], rect[1], rect[2], rect[3])
        };
    }
    /// The current tile rectangle, `[x, y, width, height]`.
    fn viewport(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: brush_raw() is a live TileBrush*; `out` is 4 floats.
        unsafe { noesis_tile_brush_get_viewport(self.brush_raw(), out.as_mut_ptr()) };
        out
    }

    /// Sets the units of the viewport.
    fn set_viewport_units(&mut self, value: BrushMappingMode) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { noesis_tile_brush_set_viewport_units(self.brush_raw(), value as i32) };
    }
    /// The current viewport units.
    fn viewport_units(&self) -> Option<BrushMappingMode> {
        // SAFETY: brush_raw() is a live TileBrush*.
        BrushMappingMode::from_ordinal(unsafe {
            noesis_tile_brush_get_viewport_units(self.brush_raw())
        })
    }

    /// Sets the part of the content to show, `[x, y, width, height]`.
    fn set_viewbox(&mut self, rect: [f32; 4]) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe {
            noesis_tile_brush_set_viewbox(self.brush_raw(), rect[0], rect[1], rect[2], rect[3])
        };
    }
    /// The current viewbox, `[x, y, width, height]`.
    fn viewbox(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: brush_raw() is a live TileBrush*; `out` is 4 floats.
        unsafe { noesis_tile_brush_get_viewbox(self.brush_raw(), out.as_mut_ptr()) };
        out
    }

    /// Sets the units of the viewbox.
    fn set_viewbox_units(&mut self, value: BrushMappingMode) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { noesis_tile_brush_set_viewbox_units(self.brush_raw(), value as i32) };
    }
    /// The current viewbox units.
    fn viewbox_units(&self) -> Option<BrushMappingMode> {
        // SAFETY: brush_raw() is a live TileBrush*.
        BrushMappingMode::from_ordinal(unsafe {
            noesis_tile_brush_get_viewbox_units(self.brush_raw())
        })
    }
}

/// Paints an image, stretched or tiled per the [`TileBrush`] settings.
///
/// The source is a raw `ImageSource*`: the `raw()` pointer of an
/// [`imaging`](crate::imaging) type such as
/// [`BitmapImage`](crate::imaging::BitmapImage), or one read off an existing
/// element with
/// [`FrameworkElement::get_component`](crate::view::FrameworkElement::get_component).
pub struct ImageBrush {
    ptr: NonNull<c_void>,
}

base_component_handle!(ImageBrush);

impl Default for ImageBrush {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageBrush {
    /// Creates a brush with no image; it paints nothing until a source is set.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the brush.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: null source is allowed (created without an image).
        let ptr = unsafe { noesis_image_brush_create(core::ptr::null_mut()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_image_brush_create returned null"),
        }
    }

    /// Creates a brush painting `image_source`. The brush takes its own
    /// reference, so the caller keeps ownership of the source. A pointer to a
    /// non-`ImageSource` object gives a brush with no image. Returns `None`
    /// only if Noesis returns null.
    ///
    /// # Safety
    ///
    /// `image_source` must be null or a live `Noesis::BaseComponent*`.
    #[must_use]
    pub unsafe fn with_source(image_source: *mut c_void) -> Option<Self> {
        // SAFETY: per the contract, `image_source` is a live ImageSource* or null.
        let ptr = unsafe { noesis_image_brush_create(image_source) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Replaces the image. The brush takes its own reference. Null, or a
    /// pointer to a non-`ImageSource` object, clears it. Returns `true` on
    /// success.
    ///
    /// # Safety
    ///
    /// `image_source` must be null or a live `Noesis::BaseComponent*`.
    pub unsafe fn set_image_source(&mut self, image_source: *mut c_void) -> bool {
        // SAFETY: self.ptr is a live ImageBrush*; `image_source` per contract.
        unsafe { noesis_image_brush_set_image_source(self.ptr.as_ptr(), image_source) }
    }

    /// The current `ImageSource*`, or `None`. Borrowed: do not release it, and
    /// do not use it after the brush's source changes or the brush is dropped.
    #[must_use]
    pub fn image_source(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live ImageBrush*; the returned pointer is borrowed.
        let p = unsafe { noesis_image_brush_get_image_source(self.ptr.as_ptr()) };
        NonNull::new(p)
    }
}

impl Brush for ImageBrush {
    fn brush_raw(&self) -> *mut c_void {
        self.raw()
    }
}

impl TileBrush for ImageBrush {}

/// Paints an area with the rendered content of another element, stretched or
/// tiled per the [`TileBrush`] settings.
///
/// The brush paints only while its source element is in a live element tree;
/// an unparented element can be set and read back but draws nothing.
pub struct VisualBrush {
    ptr: NonNull<c_void>,
}

base_component_handle!(VisualBrush);

impl Default for VisualBrush {
    fn default() -> Self {
        Self::new()
    }
}

impl VisualBrush {
    /// Creates a brush with no source; it paints nothing until one is set.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the brush.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: null visual is allowed (source wired later).
        let ptr = unsafe { noesis_visual_brush_create(core::ptr::null_mut()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_visual_brush_create returned null"),
        }
    }

    /// Creates a brush painting `element`. The brush takes its own reference
    /// to the element.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the brush.
    #[must_use]
    pub fn from_element(element: &crate::view::FrameworkElement) -> Self {
        // SAFETY: element.raw() is a live Visual* (every element is a Visual),
        // borrowed for the call; Noesis stores its own reference.
        let ptr = unsafe { noesis_visual_brush_create(element.raw()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_visual_brush_create returned null"),
        }
    }

    /// Replaces the source element. The brush takes its own reference, so the
    /// element stays alive while the brush uses it even if your handle is
    /// dropped. Always returns `true` for a live brush.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_visual(&mut self, element: &crate::view::FrameworkElement) -> bool {
        // SAFETY: self.ptr is a live VisualBrush*; element.raw() is a live
        // Visual* borrowed for the call.
        unsafe { noesis_visual_brush_set_visual(self.ptr.as_ptr(), element.raw()) }
    }

    /// Removes the source element. Always returns `true` for a live brush.
    pub fn clear_visual(&mut self) -> bool {
        // SAFETY: self.ptr is a live VisualBrush*; null clears the source.
        unsafe { noesis_visual_brush_set_visual(self.ptr.as_ptr(), core::ptr::null_mut()) }
    }

    /// The current source as a `Visual*`, or `None`. Borrowed: do not release
    /// it, and do not use it after the source changes or the brush is dropped.
    #[must_use]
    pub fn visual(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live VisualBrush*; the returned pointer is borrowed.
        let p = unsafe { noesis_visual_brush_get_visual(self.ptr.as_ptr()) };
        NonNull::new(p)
    }
}

impl Brush for VisualBrush {
    fn brush_raw(&self) -> *mut c_void {
        self.raw()
    }
}

impl TileBrush for VisualBrush {}

/// Blurs an element's rendered output.
pub struct BlurEffect {
    ptr: NonNull<c_void>,
}

base_component_handle!(BlurEffect);

impl BlurEffect {
    /// Creates a blur of `radius`, in device-independent pixels.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the effect.
    #[must_use]
    pub fn new(radius: f32) -> Self {
        let ptr = unsafe { noesis_blur_effect_create(radius) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_blur_effect_create returned null"),
        }
    }

    /// Sets the radius, in device-independent pixels.
    pub fn set_radius(&mut self, radius: f32) {
        // SAFETY: self.ptr is a live BlurEffect*.
        unsafe { noesis_blur_effect_set_radius(self.ptr.as_ptr(), radius) };
    }

    /// The current radius.
    #[must_use]
    pub fn radius(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: self.ptr is a live BlurEffect*; `out` is a valid float.
        unsafe { noesis_blur_effect_get_radius(self.ptr.as_ptr(), &mut out as *mut f32) };
        out
    }
}

impl Effect for BlurEffect {
    fn effect_raw(&self) -> *mut c_void {
        self.raw()
    }
}

/// Draws a shadow behind an element's rendered output.
///
/// Build one with [`from_params`](Self::from_params) and
/// [`DropShadowParams::default`], overriding the fields you need.
pub struct DropShadowEffect {
    ptr: NonNull<c_void>,
}

base_component_handle!(DropShadowEffect);

/// All parameters of a [`DropShadowEffect`]. [`Default`] gives Noesis's
/// defaults.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DropShadowParams {
    /// `[r, g, b, a]`, each `0..=1`.
    pub color: [f32; 4],
    /// Softness of the shadow edge, in device-independent pixels.
    pub blur_radius: f32,
    /// Direction the shadow is cast, in degrees counterclockwise from the
    /// positive x-axis (`315` is down and to the right).
    pub direction: f32,
    /// Offset of the shadow from the content, in device-independent pixels.
    pub shadow_depth: f32,
    /// Shadow opacity, `0..=1`.
    pub opacity: f32,
}

impl Default for DropShadowParams {
    /// Black, blur radius `5`, direction `315`, depth `5`, opacity `1`.
    fn default() -> Self {
        Self {
            color: [0.0, 0.0, 0.0, 1.0],
            blur_radius: 5.0,
            direction: 315.0,
            shadow_depth: 5.0,
            opacity: 1.0,
        }
    }
}

impl DropShadowEffect {
    /// Creates a shadow from `params`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the effect.
    #[must_use]
    pub fn from_params(params: DropShadowParams) -> Self {
        Self::new(
            params.color,
            params.blur_radius,
            params.direction,
            params.shadow_depth,
            params.opacity,
        )
    }

    /// Creates a shadow from positional parameters, in the order and units of
    /// [`DropShadowParams`].
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the effect.
    #[must_use]
    pub fn new(
        color: [f32; 4],
        blur_radius: f32,
        direction: f32,
        shadow_depth: f32,
        opacity: f32,
    ) -> Self {
        // SAFETY: `color` outlives the call; the C side copies it into a Color.
        let ptr = unsafe {
            noesis_drop_shadow_effect_create(
                color.as_ptr(),
                blur_radius,
                direction,
                shadow_depth,
                opacity,
            )
        };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_drop_shadow_effect_create returned null"),
        }
    }

    /// The current parameters.
    #[must_use]
    pub fn params(&self) -> DropShadowParams {
        let mut color = [0.0f32; 4];
        let mut blur_radius = 0.0f32;
        let mut direction = 0.0f32;
        let mut shadow_depth = 0.0f32;
        let mut opacity = 0.0f32;
        // SAFETY: self.ptr is a live DropShadowEffect*; all out params valid.
        unsafe {
            noesis_drop_shadow_effect_get(
                self.ptr.as_ptr(),
                color.as_mut_ptr(),
                &mut blur_radius as *mut f32,
                &mut direction as *mut f32,
                &mut shadow_depth as *mut f32,
                &mut opacity as *mut f32,
            )
        };
        DropShadowParams {
            color,
            blur_radius,
            direction,
            shadow_depth,
            opacity,
        }
    }

    /// Replaces every parameter.
    pub fn set_params(&mut self, params: DropShadowParams) {
        self.set_color(params.color);
        self.set_blur_radius(params.blur_radius);
        self.set_direction(params.direction);
        self.set_shadow_depth(params.shadow_depth);
        self.set_opacity(params.opacity);
    }

    /// Sets the color, `[r, g, b, a]`.
    pub fn set_color(&mut self, rgba: [f32; 4]) {
        // SAFETY: self.ptr is a live DropShadowEffect*; `rgba` outlives the call.
        unsafe { noesis_drop_shadow_effect_set_color(self.ptr.as_ptr(), rgba.as_ptr()) };
    }

    /// Sets the edge softness, in device-independent pixels.
    pub fn set_blur_radius(&mut self, blur_radius: f32) {
        // SAFETY: self.ptr is a live DropShadowEffect*.
        unsafe { noesis_drop_shadow_effect_set_blur_radius(self.ptr.as_ptr(), blur_radius) };
    }

    /// Sets the direction, in degrees counterclockwise from the positive x-axis.
    pub fn set_direction(&mut self, direction: f32) {
        // SAFETY: self.ptr is a live DropShadowEffect*.
        unsafe { noesis_drop_shadow_effect_set_direction(self.ptr.as_ptr(), direction) };
    }

    /// Sets the offset from the content, in device-independent pixels.
    pub fn set_shadow_depth(&mut self, shadow_depth: f32) {
        // SAFETY: self.ptr is a live DropShadowEffect*.
        unsafe { noesis_drop_shadow_effect_set_shadow_depth(self.ptr.as_ptr(), shadow_depth) };
    }

    /// Sets the opacity, `0..=1`.
    pub fn set_opacity(&mut self, opacity: f32) {
        // SAFETY: self.ptr is a live DropShadowEffect*.
        unsafe { noesis_drop_shadow_effect_set_opacity(self.ptr.as_ptr(), opacity) };
    }
}

impl Effect for DropShadowEffect {
    fn effect_raw(&self) -> *mut c_void {
        self.raw()
    }
}
