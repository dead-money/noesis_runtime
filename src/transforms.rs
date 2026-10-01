//! Build 2D and 3D transforms from Rust and apply them to elements.
//!
//! 2D transforms ([`TranslateTransform`], [`ScaleTransform`],
//! [`RotateTransform`], [`SkewTransform`], [`MatrixTransform`],
//! [`CompositeTransform`], and [`TransformGroup`] to combine them) implement
//! [`Transform`]. Assign one as an element's `RenderTransform` with
//! [`FrameworkElement::set_render_transform`](crate::view::FrameworkElement::set_render_transform).
//!
//! 3D transforms ([`CompositeTransform3D`], [`MatrixTransform3D`]) implement
//! [`Transform3D`] and go through
//! [`FrameworkElement::set_transform3d`](crate::view::FrameworkElement::set_transform3d)
//! instead; they are not a `RenderTransform`.
//!
//! Each handle owns one reference to its Noesis object and releases it on
//! [`Drop`]. Assigning it to an element makes Noesis take its own reference, so
//! the handle may be dropped afterwards. Getters read from the live object, so
//! they also see changes made by animations or XAML. Angles are in degrees.

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

/// An owning handle to a transform of unknown kind, returned by
/// [`FrameworkElement::render_transform`](crate::view::FrameworkElement::render_transform).
/// It doesn't expose the concrete type, but it implements [`Transform`], so you
/// can assign it to another element with
/// [`FrameworkElement::set_render_transform`](crate::view::FrameworkElement::set_render_transform).
pub struct AnyTransform {
    ptr: NonNull<c_void>,
}

transform_handle!(AnyTransform);

impl AnyTransform {
    /// Takes ownership of one reference on `ptr`, a live `Noesis::Transform*`.
    pub(crate) unsafe fn from_owned(ptr: NonNull<c_void>) -> Self {
        Self { ptr }
    }
}

/// Offsets an element by `(x, y)`.
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

    /// The current `(x, y)` offset.
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

/// Scales an element by `(scale_x, scale_y)` about the point
/// `(center_x, center_y)`.
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

    /// The current `[scale_x, scale_y, center_x, center_y]`.
    #[must_use]
    pub fn get(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live ScaleTransform*; `out` is 4 floats.
        unsafe { noesis_scale_transform_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }
}

/// Rotates an element by an angle in degrees about `(center_x, center_y)`.
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

    /// Set the rotation angle in degrees. The center is unchanged.
    pub fn set_angle(&mut self, angle: f32) {
        // SAFETY: self.ptr is a live RotateTransform*.
        unsafe { noesis_rotate_transform_set_angle(self.ptr.as_ptr(), angle) };
    }

    /// The current `[angle, center_x, center_y]`.
    #[must_use]
    pub fn get(&self) -> [f32; 3] {
        let mut out = [0.0f32; 3];
        // SAFETY: self.ptr is a live RotateTransform*; `out` is 3 floats.
        unsafe { noesis_rotate_transform_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }

    /// The current angle in degrees.
    #[must_use]
    pub fn angle(&self) -> f32 {
        self.get()[0]
    }
}

/// Skews an element by `(angle_x, angle_y)` degrees about a center point.
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

    /// The current `[angle_x, angle_y, center_x, center_y]`.
    #[must_use]
    pub fn get(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live SkewTransform*; `out` is 4 floats.
        unsafe { noesis_skew_transform_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }
}

/// Applies an arbitrary 2D affine matrix.
///
/// The matrix is six floats `[m00, m01, m10, m11, m20, m21]`: three rows of
/// two, with the last row holding the translation.
pub struct MatrixTransform {
    ptr: NonNull<c_void>,
}

transform_handle!(MatrixTransform);

impl MatrixTransform {
    /// Create a matrix transform from the six coefficients described on
    /// [`MatrixTransform`].
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

    /// The current six matrix coefficients.
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
    /// may be dropped afterwards. Returns `false` if Noesis rejects `child` as
    /// not a `Transform`, which doesn't happen for this module's types.
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

/// Scale, skew, rotation, and translation in one transform, applied in that
/// order. Scale, skew, and rotation share one center point.
pub struct CompositeTransform {
    ptr: NonNull<c_void>,
}

transform_handle!(CompositeTransform);

/// The fields of a [`CompositeTransform`]. [`Default`] is the identity.
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

    /// The current field values.
    #[must_use]
    pub fn get(&self) -> CompositeFields {
        let mut out = [0.0f32; 9];
        // SAFETY: self.ptr is a live CompositeTransform*; `out` is 9 floats.
        unsafe { noesis_composite_transform_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        CompositeFields::from_array(out)
    }
}

/// A handle to a Noesis `Transform3D`. Every 3D transform type here implements
/// it, so
/// [`FrameworkElement::set_transform3d`](crate::view::FrameworkElement::set_transform3d)
/// accepts any of them and rejects 2D transforms at compile time.
pub trait Transform3D {
    /// Borrowed `Noesis::Transform3D*` (a `BaseComponent*`), valid for `self`'s
    /// lifetime.
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

/// An owning handle to a 3D transform of unknown kind, returned by
/// [`FrameworkElement::transform3d`](crate::view::FrameworkElement::transform3d).
/// It implements [`Transform3D`], so you can assign it to another element with
/// [`FrameworkElement::set_transform3d`](crate::view::FrameworkElement::set_transform3d).
pub struct AnyTransform3D {
    ptr: NonNull<c_void>,
}

transform3d_handle!(AnyTransform3D);

impl AnyTransform3D {
    /// Takes ownership of one reference on `ptr`, a live `Noesis::Transform3D*`.
    pub(crate) unsafe fn from_owned(ptr: NonNull<c_void>) -> Self {
        Self { ptr }
    }
}

/// The fields of a [`CompositeTransform3D`]. [`Default`] is the identity.
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

/// Scale, rotation, and translation in 3D, about a center point. Assign it with
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

    /// The current field values.
    #[must_use]
    pub fn get(&self) -> Composite3DFields {
        let mut out = [0.0f32; 12];
        // SAFETY: self.ptr is a live CompositeTransform3D*; `out` is 12 floats.
        unsafe { noesis_composite_transform3d_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        Composite3DFields::from_array(out)
    }
}

/// Applies an arbitrary 3D affine matrix.
///
/// The matrix is 12 floats: four rows of `xyz`, with the last row holding the
/// translation.
pub struct MatrixTransform3D {
    ptr: NonNull<c_void>,
}

transform3d_handle!(MatrixTransform3D);

impl MatrixTransform3D {
    /// Create a 3D matrix transform from the 12 coefficients described on
    /// [`MatrixTransform3D`].
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

    /// The current 12 matrix coefficients.
    #[must_use]
    pub fn get(&self) -> [f32; 12] {
        let mut out = [0.0f32; 12];
        // SAFETY: self.ptr is a live MatrixTransform3D*; `out` is 12 floats.
        unsafe { noesis_matrix_transform3d_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }
}
