//! Code-built brushes and effects: construct `Brush` / `Effect` objects from
//! Rust and paint elements with them without authoring XAML.
//!
//! Each type here is an owning handle over a freshly-created Noesis object
//! holding a single `+1` reference, released on [`Drop`]. Same pattern as
//! [`crate::binding::Boxed`] / [`crate::binding::ObservableCollection`].
//! Assigning a brush/effect to an element makes Noesis take its own reference,
//! so the Rust handle may be dropped right after assignment. The ergonomic way
//! to assign is the typed sugar on [`FrameworkElement`](crate::view::FrameworkElement)
//! (`set_background` / `set_foreground` / `set_fill` / `set_effect`), which
//! routes through the generic `set_component` DP path.
//!
//! Read-back getters ([`SolidColorBrush::color`], [`BlurEffect::radius`], ...)
//! re-read from the live Noesis object, so they reflect its current state rather
//! than a Rust-side cache.

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

/// A handle to a Noesis `Brush`. Implemented by every brush type in this module
/// so the typed element sugar (e.g.
/// [`FrameworkElement::set_background`](crate::view::FrameworkElement::set_background))
/// accepts any of them while keeping non-brush objects out.
pub trait Brush {
    /// Borrowed `Noesis::Brush*` (a `BaseComponent*`), valid for `self`'s
    /// lifetime. Used by the assignment sugar; not normally called directly.
    fn brush_raw(&self) -> *mut c_void;
}

/// A handle to a Noesis `Effect` (post-process applied to an element's visual).
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

/// A `SolidColorBrush` painting a flat `[r, g, b, a]` color (each `0..=1`).
pub struct SolidColorBrush {
    ptr: NonNull<c_void>,
}

base_component_handle!(SolidColorBrush);

impl SolidColorBrush {
    /// Create a solid brush of `rgba` (`{r, g, b, a}`, each in `0..=1`).
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the brush (not expected after
    /// [`crate::init`]).
    #[must_use]
    pub fn new(rgba: [f32; 4]) -> Self {
        // SAFETY: `rgba` outlives the call; the C side copies it into a Color.
        let ptr = unsafe { noesis_solid_color_brush_create(rgba.as_ptr()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_solid_color_brush_create returned null"),
        }
    }

    /// Replace the brush color with `rgba`.
    pub fn set_color(&mut self, rgba: [f32; 4]) {
        // SAFETY: self.ptr is a live SolidColorBrush*; `rgba` outlives the call.
        unsafe {
            noesis_solid_color_brush_set_color(self.ptr.as_ptr(), rgba.as_ptr());
        }
    }

    /// Read the brush color back from the live Noesis object as `[r, g, b, a]`.
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

/// A single gradient stop: a color at a normalized `offset` (`0..=1`).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GradientStop {
    /// Position of the stop along the gradient axis, `0..=1`.
    pub offset: f32,
    /// Stop color `{r, g, b, a}`, each `0..=1`.
    pub color: [f32; 4],
}

impl GradientStop {
    /// Construct a stop from an `offset` and `rgba` color.
    #[must_use]
    pub fn new(offset: f32, color: [f32; 4]) -> Self {
        Self { offset, color }
    }
}

// Shared gradient-stop helpers (LinearGradientBrush / RadialGradientBrush both
// derive from GradientBrush, so the stop FFI is type-agnostic).
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

/// `Noesis::GradientSpreadMethod`: how a gradient paints the
/// area outside its `[0, 1]` gradient vector. Ordinals match the C++ enum.
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

/// A `LinearGradientBrush` painting a gradient along the line from `StartPoint`
/// to `EndPoint` (default `(0,0)`..`(1,1)`, relative to the painted area).
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
    /// Create an empty linear gradient brush (no stops yet).
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

    /// Set the gradient start point (relative coordinates by default).
    pub fn set_start_point(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live LinearGradientBrush*.
        unsafe { noesis_linear_gradient_brush_set_start_point(self.ptr.as_ptr(), x, y) };
    }

    /// Set the gradient end point.
    pub fn set_end_point(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live LinearGradientBrush*.
        unsafe { noesis_linear_gradient_brush_set_end_point(self.ptr.as_ptr(), x, y) };
    }

    /// Read `({startX, startY}, {endX, endY})` back from the live object.
    #[must_use]
    pub fn points(&self) -> ([f32; 2], [f32; 2]) {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live LinearGradientBrush*; `out` is 4 floats.
        unsafe { noesis_linear_gradient_brush_get_points(self.ptr.as_ptr(), out.as_mut_ptr()) };
        ([out[0], out[1]], [out[2], out[3]])
    }

    /// Append a gradient stop. Returns its index, or `None` on failure.
    pub fn add_stop(&mut self, stop: GradientStop) -> Option<usize> {
        gradient_add_stop(self.ptr.as_ptr(), stop)
    }

    /// Number of gradient stops currently on the brush.
    #[must_use]
    pub fn stop_count(&self) -> usize {
        gradient_stop_count(self.ptr.as_ptr())
    }

    /// Read back the stop at `index`, or `None` if out of range.
    #[must_use]
    pub fn stop(&self, index: usize) -> Option<GradientStop> {
        gradient_get_stop(self.ptr.as_ptr(), index)
    }

    /// Set how the gradient paints outside its `[0, 1]` vector.
    pub fn set_spread_method(&mut self, method: GradientSpreadMethod) -> bool {
        gradient_set_spread_method(self.ptr.as_ptr(), method)
    }

    /// Read the spread method back from the live object.
    #[must_use]
    pub fn spread_method(&self) -> Option<GradientSpreadMethod> {
        gradient_spread_method(self.ptr.as_ptr())
    }

    /// Set whether start/end points are absolute or relative to the bounding box.
    pub fn set_mapping_mode(&mut self, mode: BrushMappingMode) -> bool {
        gradient_set_mapping_mode(self.ptr.as_ptr(), mode)
    }

    /// Read the mapping mode back from the live object.
    #[must_use]
    pub fn mapping_mode(&self) -> Option<BrushMappingMode> {
        gradient_mapping_mode(self.ptr.as_ptr())
    }

    /// Start a [`LinearGradientBrushBuilder`] for fluent construction.
    pub fn builder() -> LinearGradientBrushBuilder {
        LinearGradientBrushBuilder {
            brush: LinearGradientBrush::new(),
        }
    }
}

/// Fluent builder for a [`LinearGradientBrush`]: set the start/end points, spread
/// method and mapping mode, append `.stop(..)`s, then [`build`](Self::build).
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
    /// Set the gradient start point.
    pub fn start(mut self, x: f32, y: f32) -> Self {
        self.brush.set_start_point(x, y);
        self
    }

    /// Set the gradient end point.
    pub fn end(mut self, x: f32, y: f32) -> Self {
        self.brush.set_end_point(x, y);
        self
    }

    /// Set the spread method.
    pub fn spread_method(mut self, method: GradientSpreadMethod) -> Self {
        self.brush.set_spread_method(method);
        self
    }

    /// Set the mapping mode.
    pub fn mapping_mode(mut self, mode: BrushMappingMode) -> Self {
        self.brush.set_mapping_mode(mode);
        self
    }

    /// Append a gradient stop of `color` at `offset` (`0..=1`).
    pub fn stop(mut self, offset: f32, color: [f32; 4]) -> Self {
        self.brush.add_stop(GradientStop { offset, color });
        self
    }

    /// Finish and return the built brush.
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

/// A `RadialGradientBrush` painting a gradient from a focal `GradientOrigin`
/// outward to the circle defined by `Center` + `RadiusX`/`RadiusY`.
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
    /// Create an empty radial gradient brush (no stops yet).
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

    /// Set the center of the outermost circle.
    pub fn set_center(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live RadialGradientBrush*.
        unsafe { noesis_radial_gradient_brush_set_center(self.ptr.as_ptr(), x, y) };
    }

    /// Set the focal point where the gradient begins.
    pub fn set_gradient_origin(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live RadialGradientBrush*.
        unsafe { noesis_radial_gradient_brush_set_gradient_origin(self.ptr.as_ptr(), x, y) };
    }

    /// Set the horizontal/vertical radii of the outermost circle.
    pub fn set_radius(&mut self, rx: f32, ry: f32) {
        // SAFETY: self.ptr is a live RadialGradientBrush*.
        unsafe { noesis_radial_gradient_brush_set_radius(self.ptr.as_ptr(), rx, ry) };
    }

    /// Read `(radiusX, radiusY)` back from the live object.
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

    /// Append a gradient stop. Returns its index, or `None` on failure.
    pub fn add_stop(&mut self, stop: GradientStop) -> Option<usize> {
        gradient_add_stop(self.ptr.as_ptr(), stop)
    }

    /// Number of gradient stops currently on the brush.
    #[must_use]
    pub fn stop_count(&self) -> usize {
        gradient_stop_count(self.ptr.as_ptr())
    }

    /// Read back the stop at `index`, or `None` if out of range.
    #[must_use]
    pub fn stop(&self, index: usize) -> Option<GradientStop> {
        gradient_get_stop(self.ptr.as_ptr(), index)
    }

    /// Set how the gradient paints outside its `[0, 1]` vector.
    pub fn set_spread_method(&mut self, method: GradientSpreadMethod) -> bool {
        gradient_set_spread_method(self.ptr.as_ptr(), method)
    }

    /// Read the spread method back from the live object.
    #[must_use]
    pub fn spread_method(&self) -> Option<GradientSpreadMethod> {
        gradient_spread_method(self.ptr.as_ptr())
    }

    /// Set whether center/radius/origin are absolute or relative to the box.
    pub fn set_mapping_mode(&mut self, mode: BrushMappingMode) -> bool {
        gradient_set_mapping_mode(self.ptr.as_ptr(), mode)
    }

    /// Read the mapping mode back from the live object.
    #[must_use]
    pub fn mapping_mode(&self) -> Option<BrushMappingMode> {
        gradient_mapping_mode(self.ptr.as_ptr())
    }

    /// Start a [`RadialGradientBrushBuilder`] for fluent construction.
    pub fn builder() -> RadialGradientBrushBuilder {
        RadialGradientBrushBuilder {
            brush: RadialGradientBrush::new(),
        }
    }
}

/// Fluent builder for a [`RadialGradientBrush`]: set the center, gradient origin,
/// radii, spread method and mapping mode, append `.stop(..)`s, then
/// [`build`](Self::build).
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
    /// Set the center of the outermost circle.
    pub fn center(mut self, x: f32, y: f32) -> Self {
        self.brush.set_center(x, y);
        self
    }

    /// Set the focal point where the gradient begins.
    pub fn gradient_origin(mut self, x: f32, y: f32) -> Self {
        self.brush.set_gradient_origin(x, y);
        self
    }

    /// Set the horizontal/vertical radii of the outermost circle.
    pub fn radius(mut self, rx: f32, ry: f32) -> Self {
        self.brush.set_radius(rx, ry);
        self
    }

    /// Set the spread method.
    pub fn spread_method(mut self, method: GradientSpreadMethod) -> Self {
        self.brush.set_spread_method(method);
        self
    }

    /// Set the mapping mode.
    pub fn mapping_mode(mut self, mode: BrushMappingMode) -> Self {
        self.brush.set_mapping_mode(mode);
        self
    }

    /// Append a gradient stop of `color` at `offset` (`0..=1`).
    pub fn stop(mut self, offset: f32, color: [f32; 4]) -> Self {
        self.brush.add_stop(GradientStop { offset, color });
        self
    }

    /// Finish and return the built brush.
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

/// `Noesis::AlignmentX`: horizontal alignment of a tile's
/// content within its base tile. Ordinals match the C++ enum.
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

/// `Noesis::AlignmentY`: vertical alignment of a tile's content
/// within its base tile. Ordinals match the C++ enum.
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

/// `Noesis::Stretch`: how content is resized to fill its
/// allocated space. Ordinals match the C++ enum.
///
/// This is the crate's single `Stretch` type, used both by tile brushes here
/// and by [shapes](crate::shapes::Shape::set_stretch), which re-exports it.
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

/// `Noesis::TileMode`: how a base tile repeats to fill the
/// painted area. Ordinals match the C++ enum.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TileMode {
    /// Draw the base tile once; the rest is transparent.
    None = 0,
    /// Repeat the base tile edge-to-edge.
    Tile = 1,
    /// Like `Tile`, flipping alternate columns horizontally.
    FlipX = 2,
    /// Like `Tile`, flipping alternate rows vertically.
    FlipY = 3,
    /// Combination of `FlipX` and `FlipY`.
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

/// `Noesis::BrushMappingMode`: whether a `Viewport`/`Viewbox`
/// Rect is in absolute coordinates or relative to the bounding box. Ordinals
/// match the C++ enum.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BrushMappingMode {
    /// Coordinates are absolute (local space).
    Absolute = 0,
    /// Coordinates are relative to the bounding box (`0..=1`).
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

/// The tiling knobs common to every `TileBrush` (the base of both
/// [`ImageBrush`] and [`VisualBrush`]): content alignment, stretch, tile mode,
/// and the `Viewport`/`Viewbox` Rects with their mapping units.
///
/// Each `Viewport`/`Viewbox` Rect is expressed as `[x, y, width, height]`.
/// Getters re-read from the live Noesis object, so they reflect its current
/// state rather than a Rust cache.
pub trait TileBrush: Brush {
    /// Set the horizontal alignment of content within the base tile.
    fn set_alignment_x(&mut self, value: AlignmentX) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { noesis_tile_brush_set_alignment_x(self.brush_raw(), value as i32) };
    }
    /// Read the horizontal alignment back from the live object.
    fn alignment_x(&self) -> Option<AlignmentX> {
        // SAFETY: brush_raw() is a live TileBrush*.
        AlignmentX::from_ordinal(unsafe { noesis_tile_brush_get_alignment_x(self.brush_raw()) })
    }

    /// Set the vertical alignment of content within the base tile.
    fn set_alignment_y(&mut self, value: AlignmentY) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { noesis_tile_brush_set_alignment_y(self.brush_raw(), value as i32) };
    }
    /// Read the vertical alignment back from the live object.
    fn alignment_y(&self) -> Option<AlignmentY> {
        // SAFETY: brush_raw() is a live TileBrush*.
        AlignmentY::from_ordinal(unsafe { noesis_tile_brush_get_alignment_y(self.brush_raw()) })
    }

    /// Set how content stretches to fit its tile.
    fn set_stretch(&mut self, value: Stretch) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { noesis_tile_brush_set_stretch(self.brush_raw(), value as i32) };
    }
    /// Read the stretch mode back from the live object.
    fn stretch(&self) -> Option<Stretch> {
        // SAFETY: brush_raw() is a live TileBrush*.
        Stretch::from_ordinal(unsafe { noesis_tile_brush_get_stretch(self.brush_raw()) })
    }

    /// Set how the base tile repeats to fill the painted area.
    fn set_tile_mode(&mut self, value: TileMode) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { noesis_tile_brush_set_tile_mode(self.brush_raw(), value as i32) };
    }
    /// Read the tile mode back from the live object.
    fn tile_mode(&self) -> Option<TileMode> {
        // SAFETY: brush_raw() is a live TileBrush*.
        TileMode::from_ordinal(unsafe { noesis_tile_brush_get_tile_mode(self.brush_raw()) })
    }

    /// Set the base-tile rectangle as `[x, y, width, height]`.
    fn set_viewport(&mut self, rect: [f32; 4]) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe {
            noesis_tile_brush_set_viewport(self.brush_raw(), rect[0], rect[1], rect[2], rect[3])
        };
    }
    /// Read the `Viewport` Rect back as `[x, y, width, height]`.
    fn viewport(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: brush_raw() is a live TileBrush*; `out` is 4 floats.
        unsafe { noesis_tile_brush_get_viewport(self.brush_raw(), out.as_mut_ptr()) };
        out
    }

    /// Set the mapping mode of the `Viewport` Rect.
    fn set_viewport_units(&mut self, value: BrushMappingMode) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { noesis_tile_brush_set_viewport_units(self.brush_raw(), value as i32) };
    }
    /// Read the `Viewport` mapping mode back from the live object.
    fn viewport_units(&self) -> Option<BrushMappingMode> {
        // SAFETY: brush_raw() is a live TileBrush*.
        BrushMappingMode::from_ordinal(unsafe {
            noesis_tile_brush_get_viewport_units(self.brush_raw())
        })
    }

    /// Set the content rectangle as `[x, y, width, height]`.
    fn set_viewbox(&mut self, rect: [f32; 4]) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe {
            noesis_tile_brush_set_viewbox(self.brush_raw(), rect[0], rect[1], rect[2], rect[3])
        };
    }
    /// Read the `Viewbox` Rect back as `[x, y, width, height]`.
    fn viewbox(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: brush_raw() is a live TileBrush*; `out` is 4 floats.
        unsafe { noesis_tile_brush_get_viewbox(self.brush_raw(), out.as_mut_ptr()) };
        out
    }

    /// Set the mapping mode of the `Viewbox` Rect.
    fn set_viewbox_units(&mut self, value: BrushMappingMode) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { noesis_tile_brush_set_viewbox_units(self.brush_raw(), value as i32) };
    }
    /// Read the `Viewbox` mapping mode back from the live object.
    fn viewbox_units(&self) -> Option<BrushMappingMode> {
        // SAFETY: brush_raw() is a live TileBrush*.
        BrushMappingMode::from_ordinal(unsafe {
            noesis_tile_brush_get_viewbox_units(self.brush_raw())
        })
    }
}

/// An `ImageBrush` tiling/stretching an `ImageSource` over the painted area.
///
/// Source wiring is via a borrowed `ImageSource*`, typically one obtained from
/// [`FrameworkElement::get_component`](crate::view::FrameworkElement::get_component)
/// on an element with a loaded image, since this crate does not yet build
/// `ImageSource`s from raw pixels (that needs the imaging surface).
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
    /// Create an image brush with no source set.
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

    /// Create an image brush pointing at a borrowed `ImageSource*`. Noesis takes
    /// its own reference to the source. Returns `None` only if allocation fails;
    /// a non-null pointer that isn't an `ImageSource` yields a brush with no
    /// source set (as if constructed with null).
    ///
    /// # Safety
    ///
    /// `image_source` must be a valid live `Noesis::ImageSource*` (e.g. from
    /// [`FrameworkElement::get_component`](crate::view::FrameworkElement::get_component))
    /// or null.
    #[must_use]
    pub unsafe fn with_source(image_source: *mut c_void) -> Option<Self> {
        // SAFETY: per the contract, `image_source` is a live ImageSource* or null.
        let ptr = unsafe { noesis_image_brush_create(image_source) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Point the brush at a borrowed `ImageSource*` (or null to clear). Noesis
    /// takes its own reference.
    ///
    /// # Safety
    ///
    /// `image_source` must be a valid live `Noesis::ImageSource*` or null.
    pub unsafe fn set_image_source(&mut self, image_source: *mut c_void) -> bool {
        // SAFETY: self.ptr is a live ImageBrush*; `image_source` per contract.
        unsafe { noesis_image_brush_set_image_source(self.ptr.as_ptr(), image_source) }
    }

    /// Borrowed `ImageSource*` currently set on the brush, or `None`. The
    /// pointer has no `+1` reference; do not release it.
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

/// A `VisualBrush` painting an area with a `Visual`. Any element (a
/// [`FrameworkElement`](crate::view::FrameworkElement)) is a Visual.
///
/// It derives from [`TileBrush`], so the tiling knobs (alignment / stretch /
/// tile mode / viewport / viewbox) apply here too.
///
/// A `VisualBrush` only *renders* when its visual is part of the live element
/// tree. The property still round-trips ([`VisualBrush::visual`] reads the
/// pointer straight back from the brush), but nothing paints until the visual
/// is parented.
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
    /// Create a visual brush with no source set.
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

    /// Create a visual brush painting `element` (its content). Noesis takes its
    /// own reference to the visual.
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

    /// Point the brush at `element` as its visual source. Noesis takes its own
    /// reference, so `element` may outlive or be dropped after the call (the
    /// brush holds the live element alive). Returns `false` only if `self` is
    /// somehow not a `VisualBrush` (not expected).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_visual(&mut self, element: &crate::view::FrameworkElement) -> bool {
        // SAFETY: self.ptr is a live VisualBrush*; element.raw() is a live
        // Visual* borrowed for the call.
        unsafe { noesis_visual_brush_set_visual(self.ptr.as_ptr(), element.raw()) }
    }

    /// Clear the brush's visual source.
    pub fn clear_visual(&mut self) -> bool {
        // SAFETY: self.ptr is a live VisualBrush*; null clears the source.
        unsafe { noesis_visual_brush_set_visual(self.ptr.as_ptr(), core::ptr::null_mut()) }
    }

    /// Borrowed `Visual*` currently set on the brush, or `None`. The pointer has
    /// no `+1` reference; do not release it.
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

/// A `BlurEffect` blurring an element's visual by a `Radius` (in DIPs).
pub struct BlurEffect {
    ptr: NonNull<c_void>,
}

base_component_handle!(BlurEffect);

impl BlurEffect {
    /// Create a blur effect with the given `radius`.
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

    /// Change the blur radius.
    pub fn set_radius(&mut self, radius: f32) {
        // SAFETY: self.ptr is a live BlurEffect*.
        unsafe { noesis_blur_effect_set_radius(self.ptr.as_ptr(), radius) };
    }

    /// Read the blur radius back from the live object.
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

/// A `DropShadowEffect` casting a colored shadow behind an element's visual.
pub struct DropShadowEffect {
    ptr: NonNull<c_void>,
}

base_component_handle!(DropShadowEffect);

/// Read-back of a [`DropShadowEffect`]'s parameters.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DropShadowParams {
    /// Shadow color `{r, g, b, a}`.
    pub color: [f32; 4],
    /// Blur radius of the shadow edge.
    pub blur_radius: f32,
    /// Angle of the shadow, in degrees.
    pub direction: f32,
    /// Distance of the shadow from the content.
    pub shadow_depth: f32,
    /// Shadow opacity, `0..=1`.
    pub opacity: f32,
}

impl Default for DropShadowParams {
    /// Noesis's `DropShadowEffect` defaults: black, `5` blur, `315°` direction,
    /// `5` depth, fully opaque.
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
    /// Create a drop-shadow effect from a [`DropShadowParams`] struct (the
    /// ergonomic alternative to the 5-positional-argument [`new`](Self::new)).
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

    /// Create a drop-shadow effect with all parameters specified.
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

    /// Read all shadow parameters back from the live object.
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

    /// Replace all shadow parameters from a [`DropShadowParams`] struct.
    pub fn set_params(&mut self, params: DropShadowParams) {
        self.set_color(params.color);
        self.set_blur_radius(params.blur_radius);
        self.set_direction(params.direction);
        self.set_shadow_depth(params.shadow_depth);
        self.set_opacity(params.opacity);
    }

    /// Set the shadow color `{r, g, b, a}`.
    pub fn set_color(&mut self, rgba: [f32; 4]) {
        // SAFETY: self.ptr is a live DropShadowEffect*; `rgba` outlives the call.
        unsafe { noesis_drop_shadow_effect_set_color(self.ptr.as_ptr(), rgba.as_ptr()) };
    }

    /// Set the blur radius of the shadow edge.
    pub fn set_blur_radius(&mut self, blur_radius: f32) {
        // SAFETY: self.ptr is a live DropShadowEffect*.
        unsafe { noesis_drop_shadow_effect_set_blur_radius(self.ptr.as_ptr(), blur_radius) };
    }

    /// Set the shadow direction, in degrees.
    pub fn set_direction(&mut self, direction: f32) {
        // SAFETY: self.ptr is a live DropShadowEffect*.
        unsafe { noesis_drop_shadow_effect_set_direction(self.ptr.as_ptr(), direction) };
    }

    /// Set the distance of the shadow from the content.
    pub fn set_shadow_depth(&mut self, shadow_depth: f32) {
        // SAFETY: self.ptr is a live DropShadowEffect*.
        unsafe { noesis_drop_shadow_effect_set_shadow_depth(self.ptr.as_ptr(), shadow_depth) };
    }

    /// Set the shadow opacity, `0..=1`.
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
