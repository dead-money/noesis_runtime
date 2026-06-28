//! Code-built transforms (TODO §11): construct `Transform` objects from Rust
//! and apply them as an element's `RenderTransform`.
//!
//! Each type owns a freshly-created Noesis object holding a single `+1`
//! reference released on [`Drop`] (the [`crate::binding::Boxed`] pattern).
//! Assign one with
//! [`FrameworkElement::set_render_transform`](crate::view::FrameworkElement::set_render_transform);
//! Noesis takes its own reference, so the Rust handle may drop afterwards.
//!
//! Getters re-read field values from the live Noesis object, proving they
//! crossed the FFI rather than echoing a Rust cache.
//!
//! 3D transforms (`Transform3D` / `CompositeTransform3D` / `MatrixTransform3D`)
//! are deferred — they are rarely needed for 2D UI and the 2D set covers the
//! `RenderTransform` surface.

use core::ptr::NonNull;
use std::ffi::c_void;

use crate::ffi::{
    dm_noesis_base_component_release, dm_noesis_composite_transform_create,
    dm_noesis_composite_transform_get, dm_noesis_matrix_transform_create,
    dm_noesis_matrix_transform_get, dm_noesis_matrix_transform_set,
    dm_noesis_rotate_transform_create, dm_noesis_rotate_transform_get,
    dm_noesis_rotate_transform_set_angle, dm_noesis_scale_transform_create,
    dm_noesis_scale_transform_get, dm_noesis_scale_transform_set, dm_noesis_skew_transform_create,
    dm_noesis_skew_transform_get, dm_noesis_transform_group_add_child,
    dm_noesis_transform_group_child_count, dm_noesis_transform_group_create,
    dm_noesis_translate_transform_create, dm_noesis_translate_transform_get,
    dm_noesis_translate_transform_set,
};

/// A handle to a Noesis `Transform`. Implemented by every transform type here so
/// [`FrameworkElement::set_render_transform`](crate::view::FrameworkElement::set_render_transform)
/// and [`TransformGroup::add_child`] accept any of them.
pub trait Transform {
    /// Borrowed `Noesis::Transform*` (a `BaseComponent*`), valid for `self`'s
    /// lifetime.
    fn transform_raw(&self) -> *mut c_void;
}

macro_rules! transform_handle {
    ($name:ident) => {
        // SAFETY: a Noesis BaseComponent handle; same per-object affinity as the
        // other owning wrappers in this crate.
        unsafe impl Send for $name {}
        unsafe impl Sync for $name {}

        impl $name {
            /// Raw `Noesis::BaseComponent*`. Borrowed for the lifetime of `self`.
            #[must_use]
            pub fn raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }

        impl Transform for $name {
            fn transform_raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                // SAFETY: produced by a `*_create` entrypoint with a +1 ref we
                // own; released exactly once here.
                unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
            }
        }
    };
}

// ── TranslateTransform ───────────────────────────────────────────────────────

/// Offsets an element by `(X, Y)`.
pub struct TranslateTransform {
    ptr: NonNull<c_void>,
}

transform_handle!(TranslateTransform);

impl TranslateTransform {
    /// Create a translate transform of `(x, y)`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the transform.
    #[must_use]
    pub fn new(x: f32, y: f32) -> Self {
        let ptr = unsafe { dm_noesis_translate_transform_create(x, y) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_translate_transform_create returned null"),
        }
    }

    /// Set the translation offset.
    pub fn set(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live TranslateTransform*.
        unsafe { dm_noesis_translate_transform_set(self.ptr.as_ptr(), x, y) };
    }

    /// Read `(x, y)` back from the live object.
    #[must_use]
    pub fn get(&self) -> (f32, f32) {
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        // SAFETY: self.ptr is a live TranslateTransform*; out params valid.
        unsafe {
            dm_noesis_translate_transform_get(
                self.ptr.as_ptr(),
                &mut x as *mut f32,
                &mut y as *mut f32,
            )
        };
        (x, y)
    }
}

// ── ScaleTransform ───────────────────────────────────────────────────────────

/// Scales an element by `(ScaleX, ScaleY)` about a center `(CenterX, CenterY)`.
pub struct ScaleTransform {
    ptr: NonNull<c_void>,
}

transform_handle!(ScaleTransform);

impl ScaleTransform {
    /// Create a scale transform with the given factors and center.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the transform.
    #[must_use]
    pub fn new(scale_x: f32, scale_y: f32, center_x: f32, center_y: f32) -> Self {
        let ptr = unsafe { dm_noesis_scale_transform_create(scale_x, scale_y, center_x, center_y) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_scale_transform_create returned null"),
        }
    }

    /// Set scale factors and center.
    pub fn set(&mut self, scale_x: f32, scale_y: f32, center_x: f32, center_y: f32) {
        // SAFETY: self.ptr is a live ScaleTransform*.
        unsafe {
            dm_noesis_scale_transform_set(self.ptr.as_ptr(), scale_x, scale_y, center_x, center_y)
        };
    }

    /// Read `[scaleX, scaleY, centerX, centerY]` back from the live object.
    #[must_use]
    pub fn get(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live ScaleTransform*; `out` is 4 floats.
        unsafe { dm_noesis_scale_transform_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }
}

// ── RotateTransform ──────────────────────────────────────────────────────────

/// Rotates an element by `Angle` (degrees) about `(CenterX, CenterY)`.
pub struct RotateTransform {
    ptr: NonNull<c_void>,
}

transform_handle!(RotateTransform);

impl RotateTransform {
    /// Create a rotate transform with the given angle (degrees) and center.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the transform.
    #[must_use]
    pub fn new(angle: f32, center_x: f32, center_y: f32) -> Self {
        let ptr = unsafe { dm_noesis_rotate_transform_create(angle, center_x, center_y) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_rotate_transform_create returned null"),
        }
    }

    /// Set the rotation angle (degrees).
    pub fn set_angle(&mut self, angle: f32) {
        // SAFETY: self.ptr is a live RotateTransform*.
        unsafe { dm_noesis_rotate_transform_set_angle(self.ptr.as_ptr(), angle) };
    }

    /// Read `[angle, centerX, centerY]` back from the live object.
    #[must_use]
    pub fn get(&self) -> [f32; 3] {
        let mut out = [0.0f32; 3];
        // SAFETY: self.ptr is a live RotateTransform*; `out` is 3 floats.
        unsafe { dm_noesis_rotate_transform_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }

    /// Convenience reader for just the angle (degrees).
    #[must_use]
    pub fn angle(&self) -> f32 {
        self.get()[0]
    }
}

// ── SkewTransform ────────────────────────────────────────────────────────────

/// Skews an element by `(AngleX, AngleY)` (degrees) about a center.
pub struct SkewTransform {
    ptr: NonNull<c_void>,
}

transform_handle!(SkewTransform);

impl SkewTransform {
    /// Create a skew transform with the given angles (degrees) and center.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the transform.
    #[must_use]
    pub fn new(angle_x: f32, angle_y: f32, center_x: f32, center_y: f32) -> Self {
        let ptr = unsafe { dm_noesis_skew_transform_create(angle_x, angle_y, center_x, center_y) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_skew_transform_create returned null"),
        }
    }

    /// Read `[angleX, angleY, centerX, centerY]` back from the live object.
    #[must_use]
    pub fn get(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live SkewTransform*; `out` is 4 floats.
        unsafe { dm_noesis_skew_transform_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }
}

// ── MatrixTransform ──────────────────────────────────────────────────────────

/// Applies an arbitrary affine `Matrix` (`[m00, m01, m10, m11, m20, m21]`).
pub struct MatrixTransform {
    ptr: NonNull<c_void>,
}

transform_handle!(MatrixTransform);

impl MatrixTransform {
    /// Create a matrix transform from the 6 affine coefficients
    /// `[m00, m01, m10, m11, m20, m21]` (Noesis `Transform2` row-major layout).
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the transform.
    #[must_use]
    pub fn new(matrix: [f32; 6]) -> Self {
        // SAFETY: `matrix` outlives the call; the C side copies it.
        let ptr = unsafe { dm_noesis_matrix_transform_create(matrix.as_ptr()) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_matrix_transform_create returned null"),
        }
    }

    /// Replace the matrix.
    pub fn set(&mut self, matrix: [f32; 6]) {
        // SAFETY: self.ptr is a live MatrixTransform*; `matrix` outlives call.
        unsafe { dm_noesis_matrix_transform_set(self.ptr.as_ptr(), matrix.as_ptr()) };
    }

    /// Read the 6 matrix coefficients back from the live object.
    #[must_use]
    pub fn get(&self) -> [f32; 6] {
        let mut out = [0.0f32; 6];
        // SAFETY: self.ptr is a live MatrixTransform*; `out` is 6 floats.
        unsafe { dm_noesis_matrix_transform_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }
}

// ── TransformGroup ───────────────────────────────────────────────────────────

/// Composes several child transforms, applied in order.
pub struct TransformGroup {
    ptr: NonNull<c_void>,
}

transform_handle!(TransformGroup);

impl Default for TransformGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl TransformGroup {
    /// Create an empty transform group.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the group.
    #[must_use]
    pub fn new() -> Self {
        let ptr = unsafe { dm_noesis_transform_group_create() };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_transform_group_create returned null"),
        }
    }

    /// Append a child transform. The group takes its own reference, so `child`
    /// may be dropped afterwards. Returns `false` if `child` is not a transform.
    pub fn add_child<T: Transform>(&mut self, child: &T) -> bool {
        // SAFETY: self.ptr is a live TransformGroup*; child.transform_raw() is a
        // live Transform* borrowed for the duration of the call.
        unsafe { dm_noesis_transform_group_add_child(self.ptr.as_ptr(), child.transform_raw()) }
    }

    /// Number of child transforms in the group.
    #[must_use]
    pub fn child_count(&self) -> usize {
        // SAFETY: self.ptr is a live TransformGroup*.
        let n = unsafe { dm_noesis_transform_group_child_count(self.ptr.as_ptr()) };
        n.max(0) as usize
    }
}

// ── CompositeTransform ───────────────────────────────────────────────────────

/// The combined scale/skew/rotate/translate transform (the XAML
/// `CompositeTransform`), applied in that canonical order about a center.
pub struct CompositeTransform {
    ptr: NonNull<c_void>,
}

transform_handle!(CompositeTransform);

/// The 9 fields of a [`CompositeTransform`], in their FFI order.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CompositeFields {
    /// Center X for scale/skew/rotate.
    pub center_x: f32,
    /// Center Y for scale/skew/rotate.
    pub center_y: f32,
    /// Horizontal scale factor.
    pub scale_x: f32,
    /// Vertical scale factor.
    pub scale_y: f32,
    /// Horizontal skew angle (degrees).
    pub skew_x: f32,
    /// Vertical skew angle (degrees).
    pub skew_y: f32,
    /// Rotation angle (degrees).
    pub rotation: f32,
    /// Horizontal translation.
    pub translate_x: f32,
    /// Vertical translation.
    pub translate_y: f32,
}

impl Default for CompositeFields {
    /// Identity composite: unit scale, no skew/rotation/translation.
    fn default() -> Self {
        Self {
            center_x: 0.0,
            center_y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            skew_x: 0.0,
            skew_y: 0.0,
            rotation: 0.0,
            translate_x: 0.0,
            translate_y: 0.0,
        }
    }
}

impl CompositeFields {
    fn to_array(self) -> [f32; 9] {
        [
            self.center_x,
            self.center_y,
            self.scale_x,
            self.scale_y,
            self.skew_x,
            self.skew_y,
            self.rotation,
            self.translate_x,
            self.translate_y,
        ]
    }

    fn from_array(a: [f32; 9]) -> Self {
        Self {
            center_x: a[0],
            center_y: a[1],
            scale_x: a[2],
            scale_y: a[3],
            skew_x: a[4],
            skew_y: a[5],
            rotation: a[6],
            translate_x: a[7],
            translate_y: a[8],
        }
    }
}

impl CompositeTransform {
    /// Create a composite transform from all of its fields.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the transform.
    #[must_use]
    pub fn new(fields: CompositeFields) -> Self {
        let arr = fields.to_array();
        // SAFETY: `arr` outlives the call; the C side reads 9 floats.
        let ptr = unsafe { dm_noesis_composite_transform_create(arr.as_ptr()) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_composite_transform_create returned null"),
        }
    }

    /// Read all fields back from the live object.
    #[must_use]
    pub fn get(&self) -> CompositeFields {
        let mut out = [0.0f32; 9];
        // SAFETY: self.ptr is a live CompositeTransform*; `out` is 9 floats.
        unsafe { dm_noesis_composite_transform_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        CompositeFields::from_array(out)
    }
}
