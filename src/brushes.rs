//! Code-built brushes and effects (TODO §11): construct `Brush` / `Effect`
//! objects from Rust and paint elements with them without authoring XAML.
//!
//! Each type here is an owning handle over a freshly-created Noesis object
//! holding a single `+1` reference, released on [`Drop`] — the same pattern as
//! [`crate::binding::Boxed`] / [`crate::binding::ObservableCollection`].
//! Assigning a brush/effect to an element makes Noesis take its own reference,
//! so the Rust handle may be dropped right after assignment. The ergonomic way
//! to assign is the typed sugar on [`FrameworkElement`](crate::view::FrameworkElement)
//! (`set_background` / `set_foreground` / `set_fill` / `set_effect`), which
//! routes through the generic `set_component` DP path.
//!
//! Read-back getters ([`SolidColorBrush::color`], [`BlurEffect::radius`], …)
//! re-read the value from the live Noesis object, so they prove a value crossed
//! the FFI rather than echoing a Rust-side cache.

use core::ptr::NonNull;
use std::ffi::c_void;

use crate::ffi::{
    dm_noesis_base_component_release, dm_noesis_blur_effect_create,
    dm_noesis_blur_effect_get_radius, dm_noesis_blur_effect_set_radius,
    dm_noesis_drop_shadow_effect_create, dm_noesis_drop_shadow_effect_get,
    dm_noesis_gradient_brush_add_stop, dm_noesis_gradient_brush_get_stop,
    dm_noesis_gradient_brush_stop_count, dm_noesis_image_brush_create,
    dm_noesis_image_brush_get_image_source, dm_noesis_image_brush_set_image_source,
    dm_noesis_linear_gradient_brush_create, dm_noesis_linear_gradient_brush_get_points,
    dm_noesis_linear_gradient_brush_set_end_point, dm_noesis_linear_gradient_brush_set_start_point,
    dm_noesis_radial_gradient_brush_create, dm_noesis_radial_gradient_brush_get_radius,
    dm_noesis_radial_gradient_brush_set_center,
    dm_noesis_radial_gradient_brush_set_gradient_origin,
    dm_noesis_radial_gradient_brush_set_radius, dm_noesis_solid_color_brush_create,
    dm_noesis_solid_color_brush_get_color, dm_noesis_solid_color_brush_set_color,
    dm_noesis_tile_brush_get_alignment_x, dm_noesis_tile_brush_get_alignment_y,
    dm_noesis_tile_brush_get_stretch, dm_noesis_tile_brush_get_tile_mode,
    dm_noesis_tile_brush_get_viewbox, dm_noesis_tile_brush_get_viewbox_units,
    dm_noesis_tile_brush_get_viewport, dm_noesis_tile_brush_get_viewport_units,
    dm_noesis_tile_brush_set_alignment_x, dm_noesis_tile_brush_set_alignment_y,
    dm_noesis_tile_brush_set_stretch, dm_noesis_tile_brush_set_tile_mode,
    dm_noesis_tile_brush_set_viewbox, dm_noesis_tile_brush_set_viewbox_units,
    dm_noesis_tile_brush_set_viewport, dm_noesis_tile_brush_set_viewport_units,
    dm_noesis_visual_brush_create, dm_noesis_visual_brush_get_visual,
    dm_noesis_visual_brush_set_visual,
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
        // SAFETY: a Noesis BaseComponent handle; same single-threaded-per-object
        // affinity as the other owning wrappers in this crate.
        unsafe impl Send for $name {}
        unsafe impl Sync for $name {}

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
                unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
            }
        }
    };
}

// ── SolidColorBrush ──────────────────────────────────────────────────────────

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
        let ptr = unsafe { dm_noesis_solid_color_brush_create(rgba.as_ptr()) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_solid_color_brush_create returned null"),
        }
    }

    /// Replace the brush color with `rgba`.
    pub fn set_color(&mut self, rgba: [f32; 4]) {
        // SAFETY: self.ptr is a live SolidColorBrush*; `rgba` outlives the call.
        unsafe {
            dm_noesis_solid_color_brush_set_color(self.ptr.as_ptr(), rgba.as_ptr());
        }
    }

    /// Read the brush color back from the live Noesis object as `[r, g, b, a]`.
    #[must_use]
    pub fn color(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live SolidColorBrush*; `out` is a 4-float buffer.
        unsafe {
            dm_noesis_solid_color_brush_get_color(self.ptr.as_ptr(), out.as_mut_ptr());
        }
        out
    }
}

impl Brush for SolidColorBrush {
    fn brush_raw(&self) -> *mut c_void {
        self.raw()
    }
}

// ── Gradient stop (a plain value, not a handle) ──────────────────────────────

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
    let idx = unsafe { dm_noesis_gradient_brush_add_stop(ptr, stop.offset, stop.color.as_ptr()) };
    (idx >= 0).then_some(idx as usize)
}

fn gradient_stop_count(ptr: *mut c_void) -> usize {
    // SAFETY: `ptr` is a live GradientBrush*.
    let n = unsafe { dm_noesis_gradient_brush_stop_count(ptr) };
    n.max(0) as usize
}

fn gradient_get_stop(ptr: *mut c_void, index: usize) -> Option<GradientStop> {
    let mut offset = 0.0f32;
    let mut color = [0.0f32; 4];
    // SAFETY: `ptr` is a live GradientBrush*; out params are valid buffers.
    let ok = unsafe {
        dm_noesis_gradient_brush_get_stop(
            ptr,
            index as u32,
            &mut offset as *mut f32,
            color.as_mut_ptr(),
        )
    };
    ok.then_some(GradientStop { offset, color })
}

// ── LinearGradientBrush ──────────────────────────────────────────────────────

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
        let ptr = unsafe { dm_noesis_linear_gradient_brush_create() };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_linear_gradient_brush_create returned null"),
        }
    }

    /// Set the gradient start point (relative coordinates by default).
    pub fn set_start_point(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live LinearGradientBrush*.
        unsafe { dm_noesis_linear_gradient_brush_set_start_point(self.ptr.as_ptr(), x, y) };
    }

    /// Set the gradient end point.
    pub fn set_end_point(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live LinearGradientBrush*.
        unsafe { dm_noesis_linear_gradient_brush_set_end_point(self.ptr.as_ptr(), x, y) };
    }

    /// Read `({startX, startY}, {endX, endY})` back from the live object.
    #[must_use]
    pub fn points(&self) -> ([f32; 2], [f32; 2]) {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live LinearGradientBrush*; `out` is 4 floats.
        unsafe { dm_noesis_linear_gradient_brush_get_points(self.ptr.as_ptr(), out.as_mut_ptr()) };
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
}

impl Brush for LinearGradientBrush {
    fn brush_raw(&self) -> *mut c_void {
        self.raw()
    }
}

// ── RadialGradientBrush ──────────────────────────────────────────────────────

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
        let ptr = unsafe { dm_noesis_radial_gradient_brush_create() };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_radial_gradient_brush_create returned null"),
        }
    }

    /// Set the center of the outermost circle.
    pub fn set_center(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live RadialGradientBrush*.
        unsafe { dm_noesis_radial_gradient_brush_set_center(self.ptr.as_ptr(), x, y) };
    }

    /// Set the focal point where the gradient begins.
    pub fn set_gradient_origin(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live RadialGradientBrush*.
        unsafe { dm_noesis_radial_gradient_brush_set_gradient_origin(self.ptr.as_ptr(), x, y) };
    }

    /// Set the horizontal/vertical radii of the outermost circle.
    pub fn set_radius(&mut self, rx: f32, ry: f32) {
        // SAFETY: self.ptr is a live RadialGradientBrush*.
        unsafe { dm_noesis_radial_gradient_brush_set_radius(self.ptr.as_ptr(), rx, ry) };
    }

    /// Read `(radiusX, radiusY)` back from the live object.
    #[must_use]
    pub fn radius(&self) -> (f32, f32) {
        let mut rx = 0.0f32;
        let mut ry = 0.0f32;
        // SAFETY: self.ptr is a live RadialGradientBrush*; out params valid.
        unsafe {
            dm_noesis_radial_gradient_brush_get_radius(
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
}

impl Brush for RadialGradientBrush {
    fn brush_raw(&self) -> *mut c_void {
        self.raw()
    }
}

// ── TileBrush tiling knobs (shared by ImageBrush + VisualBrush) ──────────────

/// `Noesis::AlignmentX` (`NsGui/Enums.h`): horizontal alignment of a tile's
/// content within its base tile. Ordinals match the C++ enum.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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

/// `Noesis::AlignmentY` (`NsGui/Enums.h`): vertical alignment of a tile's content
/// within its base tile. Ordinals match the C++ enum.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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

/// `Noesis::Stretch` (`NsGui/Enums.h`): how a tile's content is resized to fill
/// its tile. Ordinals match the C++ enum.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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
    fn from_ordinal(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Fill),
            2 => Some(Self::Uniform),
            3 => Some(Self::UniformToFill),
            _ => None,
        }
    }
}

/// `Noesis::TileMode` (`NsGui/Enums.h`): how a base tile repeats to fill the
/// painted area. Ordinals match the C++ enum.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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

/// `Noesis::BrushMappingMode` (`NsGui/Enums.h`): whether a `Viewport`/`Viewbox`
/// Rect is in absolute coordinates or relative to the bounding box. Ordinals
/// match the C++ enum.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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
/// Getters re-read from the live Noesis object so they prove a value crossed the
/// FFI rather than echoing a Rust cache.
pub trait TileBrush: Brush {
    /// Set the horizontal alignment of content within the base tile.
    fn set_alignment_x(&mut self, value: AlignmentX) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { dm_noesis_tile_brush_set_alignment_x(self.brush_raw(), value as i32) };
    }
    /// Read the horizontal alignment back from the live object.
    fn alignment_x(&self) -> Option<AlignmentX> {
        // SAFETY: brush_raw() is a live TileBrush*.
        AlignmentX::from_ordinal(unsafe { dm_noesis_tile_brush_get_alignment_x(self.brush_raw()) })
    }

    /// Set the vertical alignment of content within the base tile.
    fn set_alignment_y(&mut self, value: AlignmentY) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { dm_noesis_tile_brush_set_alignment_y(self.brush_raw(), value as i32) };
    }
    /// Read the vertical alignment back from the live object.
    fn alignment_y(&self) -> Option<AlignmentY> {
        // SAFETY: brush_raw() is a live TileBrush*.
        AlignmentY::from_ordinal(unsafe { dm_noesis_tile_brush_get_alignment_y(self.brush_raw()) })
    }

    /// Set how content stretches to fit its tile.
    fn set_stretch(&mut self, value: Stretch) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { dm_noesis_tile_brush_set_stretch(self.brush_raw(), value as i32) };
    }
    /// Read the stretch mode back from the live object.
    fn stretch(&self) -> Option<Stretch> {
        // SAFETY: brush_raw() is a live TileBrush*.
        Stretch::from_ordinal(unsafe { dm_noesis_tile_brush_get_stretch(self.brush_raw()) })
    }

    /// Set how the base tile repeats to fill the painted area.
    fn set_tile_mode(&mut self, value: TileMode) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { dm_noesis_tile_brush_set_tile_mode(self.brush_raw(), value as i32) };
    }
    /// Read the tile mode back from the live object.
    fn tile_mode(&self) -> Option<TileMode> {
        // SAFETY: brush_raw() is a live TileBrush*.
        TileMode::from_ordinal(unsafe { dm_noesis_tile_brush_get_tile_mode(self.brush_raw()) })
    }

    /// Set the base-tile rectangle as `[x, y, width, height]`.
    fn set_viewport(&mut self, rect: [f32; 4]) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe {
            dm_noesis_tile_brush_set_viewport(self.brush_raw(), rect[0], rect[1], rect[2], rect[3])
        };
    }
    /// Read the `Viewport` Rect back as `[x, y, width, height]`.
    fn viewport(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: brush_raw() is a live TileBrush*; `out` is 4 floats.
        unsafe { dm_noesis_tile_brush_get_viewport(self.brush_raw(), out.as_mut_ptr()) };
        out
    }

    /// Set the mapping mode of the `Viewport` Rect.
    fn set_viewport_units(&mut self, value: BrushMappingMode) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { dm_noesis_tile_brush_set_viewport_units(self.brush_raw(), value as i32) };
    }
    /// Read the `Viewport` mapping mode back from the live object.
    fn viewport_units(&self) -> Option<BrushMappingMode> {
        // SAFETY: brush_raw() is a live TileBrush*.
        BrushMappingMode::from_ordinal(unsafe {
            dm_noesis_tile_brush_get_viewport_units(self.brush_raw())
        })
    }

    /// Set the content rectangle as `[x, y, width, height]`.
    fn set_viewbox(&mut self, rect: [f32; 4]) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe {
            dm_noesis_tile_brush_set_viewbox(self.brush_raw(), rect[0], rect[1], rect[2], rect[3])
        };
    }
    /// Read the `Viewbox` Rect back as `[x, y, width, height]`.
    fn viewbox(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: brush_raw() is a live TileBrush*; `out` is 4 floats.
        unsafe { dm_noesis_tile_brush_get_viewbox(self.brush_raw(), out.as_mut_ptr()) };
        out
    }

    /// Set the mapping mode of the `Viewbox` Rect.
    fn set_viewbox_units(&mut self, value: BrushMappingMode) {
        // SAFETY: brush_raw() is a live TileBrush*.
        unsafe { dm_noesis_tile_brush_set_viewbox_units(self.brush_raw(), value as i32) };
    }
    /// Read the `Viewbox` mapping mode back from the live object.
    fn viewbox_units(&self) -> Option<BrushMappingMode> {
        // SAFETY: brush_raw() is a live TileBrush*.
        BrushMappingMode::from_ordinal(unsafe {
            dm_noesis_tile_brush_get_viewbox_units(self.brush_raw())
        })
    }
}

// ── ImageBrush ───────────────────────────────────────────────────────────────

/// An `ImageBrush` tiling/stretching an `ImageSource` over the painted area.
///
/// Source wiring is via a borrowed `ImageSource*` — typically one obtained from
/// [`FrameworkElement::get_component`](crate::view::FrameworkElement::get_component)
/// on an element with a loaded image, since this crate does not yet build
/// `ImageSource`s from raw pixels (that needs the imaging surface, TODO §12).
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
        let ptr = unsafe { dm_noesis_image_brush_create(core::ptr::null_mut()) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_image_brush_create returned null"),
        }
    }

    /// Create an image brush pointing at a borrowed `ImageSource*`. Noesis takes
    /// its own reference to the source. Returns `None` only if allocation fails.
    ///
    /// # Safety
    ///
    /// `image_source` must be a valid live `Noesis::ImageSource*` (e.g. from
    /// [`FrameworkElement::get_component`](crate::view::FrameworkElement::get_component))
    /// or null.
    #[must_use]
    pub unsafe fn with_source(image_source: *mut c_void) -> Option<Self> {
        // SAFETY: per the contract, `image_source` is a live ImageSource* or null.
        let ptr = unsafe { dm_noesis_image_brush_create(image_source) };
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
        unsafe { dm_noesis_image_brush_set_image_source(self.ptr.as_ptr(), image_source) }
    }

    /// Borrowed `ImageSource*` currently set on the brush, or `None`. The
    /// pointer has no `+1` reference; do not release it.
    #[must_use]
    pub fn image_source(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live ImageBrush*; the returned pointer is borrowed.
        let p = unsafe { dm_noesis_image_brush_get_image_source(self.ptr.as_ptr()) };
        NonNull::new(p)
    }
}

impl Brush for ImageBrush {
    fn brush_raw(&self) -> *mut c_void {
        self.raw()
    }
}

impl TileBrush for ImageBrush {}

// ── VisualBrush ──────────────────────────────────────────────────────────────

/// A `VisualBrush` (NsGui/VisualBrush.h) painting an area with a `Visual` — any
/// element (a [`FrameworkElement`](crate::view::FrameworkElement)) is a Visual.
///
/// It derives from [`TileBrush`], so the tiling knobs (alignment / stretch /
/// tile mode / viewport / viewbox) apply here too.
///
/// NOTE (SDK): the Noesis header states a `VisualBrush` only *renders* when its
/// visual is part of the logical tree. The property assignment is nonetheless
/// fully headless-verifiable: [`VisualBrush::visual`] reads the visual pointer
/// back from the live brush, and assigning the brush to an element round-trips
/// via `get_component` pointer identity.
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
        let ptr = unsafe { dm_noesis_visual_brush_create(core::ptr::null_mut()) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_visual_brush_create returned null"),
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
        let ptr = unsafe { dm_noesis_visual_brush_create(element.raw()) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_visual_brush_create returned null"),
        }
    }

    /// Point the brush at `element` as its visual source. Noesis takes its own
    /// reference, so `element` may outlive or be dropped after the call (the
    /// brush holds the live element alive). Returns `false` only if `self` is
    /// somehow not a `VisualBrush` (not expected).
    pub fn set_visual(&mut self, element: &crate::view::FrameworkElement) -> bool {
        // SAFETY: self.ptr is a live VisualBrush*; element.raw() is a live
        // Visual* borrowed for the call.
        unsafe { dm_noesis_visual_brush_set_visual(self.ptr.as_ptr(), element.raw()) }
    }

    /// Clear the brush's visual source.
    pub fn clear_visual(&mut self) -> bool {
        // SAFETY: self.ptr is a live VisualBrush*; null clears the source.
        unsafe { dm_noesis_visual_brush_set_visual(self.ptr.as_ptr(), core::ptr::null_mut()) }
    }

    /// Borrowed `Visual*` currently set on the brush, or `None`. The pointer has
    /// no `+1` reference; do not release it.
    #[must_use]
    pub fn visual(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live VisualBrush*; the returned pointer is borrowed.
        let p = unsafe { dm_noesis_visual_brush_get_visual(self.ptr.as_ptr()) };
        NonNull::new(p)
    }
}

impl Brush for VisualBrush {
    fn brush_raw(&self) -> *mut c_void {
        self.raw()
    }
}

impl TileBrush for VisualBrush {}

// ── BlurEffect ───────────────────────────────────────────────────────────────

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
        let ptr = unsafe { dm_noesis_blur_effect_create(radius) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_blur_effect_create returned null"),
        }
    }

    /// Change the blur radius.
    pub fn set_radius(&mut self, radius: f32) {
        // SAFETY: self.ptr is a live BlurEffect*.
        unsafe { dm_noesis_blur_effect_set_radius(self.ptr.as_ptr(), radius) };
    }

    /// Read the blur radius back from the live object.
    #[must_use]
    pub fn radius(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: self.ptr is a live BlurEffect*; `out` is a valid float.
        unsafe { dm_noesis_blur_effect_get_radius(self.ptr.as_ptr(), &mut out as *mut f32) };
        out
    }
}

impl Effect for BlurEffect {
    fn effect_raw(&self) -> *mut c_void {
        self.raw()
    }
}

// ── DropShadowEffect ─────────────────────────────────────────────────────────

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

impl DropShadowEffect {
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
            dm_noesis_drop_shadow_effect_create(
                color.as_ptr(),
                blur_radius,
                direction,
                shadow_depth,
                opacity,
            )
        };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_drop_shadow_effect_create returned null"),
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
            dm_noesis_drop_shadow_effect_get(
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
}

impl Effect for DropShadowEffect {
    fn effect_raw(&self) -> *mut c_void {
        self.raw()
    }
}
