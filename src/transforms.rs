//! Build `Transform` objects from Rust and apply them as an element's
//! `RenderTransform`.
//!
//! Each type owns a freshly-created Noesis object holding a single `+1`
//! reference released on [`Drop`] (the [`crate::binding::Boxed`] pattern).
//! Assign one with
//! [`FrameworkElement::set_render_transform`](crate::view::FrameworkElement::set_render_transform);
//! Noesis takes its own reference, so the Rust handle may drop afterwards.
//!
//! Getters re-read field values from the live Noesis object, so they reflect
//! the object's current state rather than a Rust-side cache.
//!
//! 3D transforms ([`CompositeTransform3D`] / [`MatrixTransform3D`], both
//! implementing the [`Transform3D`] marker) are assigned to an element through
//! [`FrameworkElement::set_transform3d`](crate::view::FrameworkElement::set_transform3d)
//! (`UIElement::SetTransform3D`), NOT via `RenderTransform`.

use core::ptr::NonNull;
use std::ffi::c_void;

use crate::ffi::{
    noesis_base_component_release, noesis_composite_transform_create,
    noesis_composite_transform_get, noesis_composite_transform3d_create,
    noesis_composite_transform3d_get, noesis_composite_transform3d_set,
    noesis_matrix_transform_create, noesis_matrix_transform_get, noesis_matrix_transform_set,
    noesis_matrix_transform3d_create, noesis_matrix_transform3d_get, noesis_matrix_transform3d_set,
    noesis_rotate_transform_create, noesis_rotate_transform_get, noesis_rotate_transform_set_angle,
    noesis_scale_transform_create, noesis_scale_transform_get, noesis_scale_transform_set,
    noesis_skew_transform_create, noesis_skew_transform_get, noesis_transform_group_add_child,
    noesis_transform_group_child_count, noesis_transform_group_create,
    noesis_translate_transform_create, noesis_translate_transform_get,
    noesis_translate_transform_set,
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
        // SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
        unsafe impl Send for $name {}

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
                unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
            }
        }
    };
}

/// An owning, **type-erased** handle to a `Noesis::Transform` read back from an
/// element, e.g. via
/// [`FrameworkElement::render_transform`](crate::view::FrameworkElement::render_transform).
/// It doesn't expose the concrete transform kind, but it implements
/// [`Transform`], so it can be re-applied to another element through
/// [`FrameworkElement::set_render_transform`](crate::view::FrameworkElement::set_render_transform).
pub struct AnyTransform {
    ptr: NonNull<c_void>,
}

transform_handle!(AnyTransform);

impl AnyTransform {
    /// Wrap a raw `Noesis::Transform*` that already carries a `+1` reference
    /// this handle takes ownership of (released on drop). Crate-internal: used
    /// by the render-transform getter, which `AddRef`s the borrowed pointer.
    pub(crate) unsafe fn from_owned(ptr: NonNull<c_void>) -> Self {
        Self { ptr }
    }
}

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
        let ptr = unsafe { noesis_translate_transform_create(x, y) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_translate_transform_create returned null"),
        }
    }

    /// Set the translation offset.
    pub fn set(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live TranslateTransform*.
        unsafe { noesis_translate_transform_set(self.ptr.as_ptr(), x, y) };
    }

    /// Read `(x, y)` back from the live object.
    #[must_use]
    pub fn get(&self) -> (f32, f32) {
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        // SAFETY: self.ptr is a live TranslateTransform*; out params valid.
        unsafe {
            noesis_translate_transform_get(
                self.ptr.as_ptr(),
                &mut x as *mut f32,
                &mut y as *mut f32,
            )
        };
        (x, y)
    }
}

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
        let ptr = unsafe { noesis_scale_transform_create(scale_x, scale_y, center_x, center_y) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_scale_transform_create returned null"),
        }
    }

    /// Set scale factors and center.
    pub fn set(&mut self, scale_x: f32, scale_y: f32, center_x: f32, center_y: f32) {
        // SAFETY: self.ptr is a live ScaleTransform*.
        unsafe {
            noesis_scale_transform_set(self.ptr.as_ptr(), scale_x, scale_y, center_x, center_y)
        };
    }

    /// Read `[scaleX, scaleY, centerX, centerY]` back from the live object.
    #[must_use]
    pub fn get(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live ScaleTransform*; `out` is 4 floats.
        unsafe { noesis_scale_transform_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }
}

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
        let ptr = unsafe { noesis_rotate_transform_create(angle, center_x, center_y) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_rotate_transform_create returned null"),
        }
    }

    /// Set the rotation angle (degrees).
    pub fn set_angle(&mut self, angle: f32) {
        // SAFETY: self.ptr is a live RotateTransform*.
        unsafe { noesis_rotate_transform_set_angle(self.ptr.as_ptr(), angle) };
    }

    /// Read `[angle, centerX, centerY]` back from the live object.
    #[must_use]
    pub fn get(&self) -> [f32; 3] {
        let mut out = [0.0f32; 3];
        // SAFETY: self.ptr is a live RotateTransform*; `out` is 3 floats.
        unsafe { noesis_rotate_transform_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }

    /// Convenience reader for just the angle (degrees).
    #[must_use]
    pub fn angle(&self) -> f32 {
        self.get()[0]
    }
}

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
        let ptr = unsafe { noesis_skew_transform_create(angle_x, angle_y, center_x, center_y) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_skew_transform_create returned null"),
        }
    }

    /// Read `[angleX, angleY, centerX, centerY]` back from the live object.
    #[must_use]
    pub fn get(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live SkewTransform*; `out` is 4 floats.
        unsafe { noesis_skew_transform_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }
}

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
        let ptr = unsafe { noesis_matrix_transform_create(matrix.as_ptr()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_matrix_transform_create returned null"),
        }
    }

    /// Replace the matrix.
    pub fn set(&mut self, matrix: [f32; 6]) {
        // SAFETY: self.ptr is a live MatrixTransform*; `matrix` outlives call.
        unsafe { noesis_matrix_transform_set(self.ptr.as_ptr(), matrix.as_ptr()) };
    }

    /// Read the 6 matrix coefficients back from the live object.
    #[must_use]
    pub fn get(&self) -> [f32; 6] {
        let mut out = [0.0f32; 6];
        // SAFETY: self.ptr is a live MatrixTransform*; `out` is 6 floats.
        unsafe { noesis_matrix_transform_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }
}

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
        let ptr = unsafe { noesis_transform_group_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_transform_group_create returned null"),
        }
    }

    /// Append a child transform. The group takes its own reference, so `child`
    /// may be dropped afterwards. Returns `false` if `child` is not a transform.
    pub fn add_child<T: Transform>(&mut self, child: &T) -> bool {
        // SAFETY: self.ptr is a live TransformGroup*; child.transform_raw() is a
        // live Transform* borrowed for the duration of the call.
        unsafe { noesis_transform_group_add_child(self.ptr.as_ptr(), child.transform_raw()) }
    }

    /// Number of child transforms in the group.
    #[must_use]
    pub fn child_count(&self) -> usize {
        // SAFETY: self.ptr is a live TransformGroup*.
        let n = unsafe { noesis_transform_group_child_count(self.ptr.as_ptr()) };
        n.max(0) as usize
    }
}

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
        let ptr = unsafe { noesis_composite_transform_create(arr.as_ptr()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_composite_transform_create returned null"),
        }
    }

    /// Read all fields back from the live object.
    #[must_use]
    pub fn get(&self) -> CompositeFields {
        let mut out = [0.0f32; 9];
        // SAFETY: self.ptr is a live CompositeTransform*; `out` is 9 floats.
        unsafe { noesis_composite_transform_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        CompositeFields::from_array(out)
    }
}

/// A handle to a Noesis `Transform3D`. Implemented by every 3D transform type
/// here so
/// [`FrameworkElement::set_transform3d`](crate::view::FrameworkElement::set_transform3d)
/// accepts any of them while keeping 2D transforms out.
pub trait Transform3D {
    /// Borrowed `Noesis::Transform3D*` (a `BaseComponent*`), valid for `self`'s
    /// lifetime. Used by the element assignment sugar.
    fn transform3d_raw(&self) -> *mut c_void;
}

macro_rules! transform3d_handle {
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

        impl Transform3D for $name {
            fn transform3d_raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                // SAFETY: produced by a `*_create` entrypoint with a +1 ref we
                // own; released exactly once here.
                unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
            }
        }
    };
}

/// An owning, **type-erased** handle to a `Noesis::Transform3D` read back from an
/// element, e.g. via
/// [`FrameworkElement::transform3d`](crate::view::FrameworkElement::transform3d).
/// It implements [`Transform3D`], so it can be re-applied to another element via
/// [`FrameworkElement::set_transform3d`](crate::view::FrameworkElement::set_transform3d).
pub struct AnyTransform3D {
    ptr: NonNull<c_void>,
}

transform3d_handle!(AnyTransform3D);

impl AnyTransform3D {
    /// Wrap a raw `Noesis::Transform3D*` that already carries a `+1` reference
    /// this handle takes ownership of (released on drop). Crate-internal: used
    /// by the element getter, which `AddRef`s the borrowed pointer.
    pub(crate) unsafe fn from_owned(ptr: NonNull<c_void>) -> Self {
        Self { ptr }
    }
}

/// The 12 fields of a [`CompositeTransform3D`], in their FFI order.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Composite3DFields {
    /// Center X of the transformation, in pixels.
    pub center_x: f32,
    /// Center Y of the transformation, in pixels.
    pub center_y: f32,
    /// Center Z of the transformation, in pixels.
    pub center_z: f32,
    /// Degrees of rotation around the X axis.
    pub rotation_x: f32,
    /// Degrees of rotation around the Y axis.
    pub rotation_y: f32,
    /// Degrees of rotation around the Z axis.
    pub rotation_z: f32,
    /// X-axis scale factor.
    pub scale_x: f32,
    /// Y-axis scale factor.
    pub scale_y: f32,
    /// Z-axis scale factor.
    pub scale_z: f32,
    /// Distance to translate along the X axis, in pixels.
    pub translate_x: f32,
    /// Distance to translate along the Y axis, in pixels.
    pub translate_y: f32,
    /// Distance to translate along the Z axis, in pixels.
    pub translate_z: f32,
}

impl Default for Composite3DFields {
    /// Identity composite: unit scale, no rotation/translation, origin center.
    fn default() -> Self {
        Self {
            center_x: 0.0,
            center_y: 0.0,
            center_z: 0.0,
            rotation_x: 0.0,
            rotation_y: 0.0,
            rotation_z: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            scale_z: 1.0,
            translate_x: 0.0,
            translate_y: 0.0,
            translate_z: 0.0,
        }
    }
}

impl Composite3DFields {
    fn to_array(self) -> [f32; 12] {
        [
            self.center_x,
            self.center_y,
            self.center_z,
            self.rotation_x,
            self.rotation_y,
            self.rotation_z,
            self.scale_x,
            self.scale_y,
            self.scale_z,
            self.translate_x,
            self.translate_y,
            self.translate_z,
        ]
    }

    fn from_array(a: [f32; 12]) -> Self {
        Self {
            center_x: a[0],
            center_y: a[1],
            center_z: a[2],
            rotation_x: a[3],
            rotation_y: a[4],
            rotation_z: a[5],
            scale_x: a[6],
            scale_y: a[7],
            scale_z: a[8],
            translate_x: a[9],
            translate_y: a[10],
            translate_z: a[11],
        }
    }
}

/// The 3D scale/rotation/translation transform (XAML `CompositeTransform3D`),
/// with 12 float dependency properties. Assign via
/// [`FrameworkElement::set_transform3d`](crate::view::FrameworkElement::set_transform3d).
pub struct CompositeTransform3D {
    ptr: NonNull<c_void>,
}

transform3d_handle!(CompositeTransform3D);

impl CompositeTransform3D {
    /// Create a 3D composite transform from all of its fields.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the transform.
    #[must_use]
    pub fn new(fields: Composite3DFields) -> Self {
        let arr = fields.to_array();
        // SAFETY: `arr` outlives the call; the C side reads 12 floats.
        let ptr = unsafe { noesis_composite_transform3d_create(arr.as_ptr()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_composite_transform3d_create returned null"),
        }
    }

    /// Replace all fields.
    pub fn set(&mut self, fields: Composite3DFields) {
        let arr = fields.to_array();
        // SAFETY: self.ptr is a live CompositeTransform3D*; `arr` is 12 floats.
        unsafe { noesis_composite_transform3d_set(self.ptr.as_ptr(), arr.as_ptr()) };
    }

    /// Read all fields back from the live object.
    #[must_use]
    pub fn get(&self) -> Composite3DFields {
        let mut out = [0.0f32; 12];
        // SAFETY: self.ptr is a live CompositeTransform3D*; `out` is 12 floats.
        unsafe { noesis_composite_transform3d_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        Composite3DFields::from_array(out)
    }
}

/// Applies an arbitrary 3D matrix (XAML `MatrixTransform3D`). The matrix is a
/// Noesis `Transform3`: 12 floats laid out as 4 rows of a `Vector3`
/// (`[row0(xyz), row1(xyz), row2(xyz), row3(xyz)]`, row 3 being translation).
pub struct MatrixTransform3D {
    ptr: NonNull<c_void>,
}

transform3d_handle!(MatrixTransform3D);

impl MatrixTransform3D {
    /// Create a 3D matrix transform from the 12 `Transform3` coefficients.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the transform.
    #[must_use]
    pub fn new(matrix: [f32; 12]) -> Self {
        // SAFETY: `matrix` outlives the call; the C side copies 12 floats.
        let ptr = unsafe { noesis_matrix_transform3d_create(matrix.as_ptr()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_matrix_transform3d_create returned null"),
        }
    }

    /// Replace the matrix.
    pub fn set(&mut self, matrix: [f32; 12]) {
        // SAFETY: self.ptr is a live MatrixTransform3D*; `matrix` is 12 floats.
        unsafe { noesis_matrix_transform3d_set(self.ptr.as_ptr(), matrix.as_ptr()) };
    }

    /// Read the 12 matrix coefficients back from the live object.
    #[must_use]
    pub fn get(&self) -> [f32; 12] {
        let mut out = [0.0f32; 12];
        // SAFETY: self.ptr is a live MatrixTransform3D*; `out` is 12 floats.
        unsafe { noesis_matrix_transform3d_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }
}
