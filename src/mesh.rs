//! Triangle meshes built in code.
//!
//! [`MeshData`] holds raw geometry that Noesis sends to the GPU as is: `(x, y)`
//! vertices, optional `(u, v)` texture coordinates, 16-bit triangle indices,
//! and a bounding box. Draw it with
//! [`DrawingContext::draw_mesh`](crate::drawing::DrawingContext::draw_mesh), or
//! place it in the element tree inside a [`Mesh`] element.
//!
//! ```no_run
//! use noesis_runtime::brushes::SolidColorBrush;
//! use noesis_runtime::mesh::{Mesh, MeshData};
//!
//! let mut data = MeshData::new();
//! data.set_vertices(&[[0.0, 0.0], [100.0, 0.0], [0.0, 100.0]]);
//! data.set_indices(&[0, 1, 2]);
//! data.set_bounds([0.0, 0.0, 100.0, 100.0]);
//!
//! let mut mesh = Mesh::new();
//! let _ = mesh.set_data(&data);
//! let _ = mesh.set_brush(&SolidColorBrush::new([1.0, 0.0, 0.0, 1.0]));
//! ```
//!
//! Each handle owns one reference to its Noesis object and releases it on drop.
//! Noesis has no element-count getters, so [`MeshData`] remembers the counts it
//! last set and its getters read back that many elements.

use core::ptr::NonNull;
use std::ffi::c_void;

use crate::brushes::Brush;
use crate::ffi::{
    noesis_base_component_release, noesis_mesh_create, noesis_mesh_data_create,
    noesis_mesh_data_get_bounds, noesis_mesh_data_get_indices, noesis_mesh_data_get_uvs,
    noesis_mesh_data_get_vertices, noesis_mesh_data_set_bounds, noesis_mesh_data_set_indices,
    noesis_mesh_data_set_uvs, noesis_mesh_data_set_vertices, noesis_mesh_get_brush,
    noesis_mesh_get_data, noesis_mesh_set_brush, noesis_mesh_set_data,
};

/// Vertex, texture-coordinate and index buffers plus a bounding box. Draw it
/// with [`DrawingContext::draw_mesh`](crate::drawing::DrawingContext::draw_mesh)
/// or host it in a [`Mesh`].
pub struct MeshData {
    ptr: NonNull<c_void>,
    num_vertices: u32,
    num_uvs: u32,
    num_indices: u32,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for MeshData {}

impl MeshData {
    /// Creates an empty mesh with zero bounds.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate (not expected after [`crate::init`]).
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: returns a +1-owned MeshData* this handle releases on Drop.
        let ptr = unsafe { noesis_mesh_data_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_mesh_data_create returned null"),
            num_vertices: 0,
            num_uvs: 0,
            num_indices: 0,
        }
    }

    /// The underlying `Noesis::MeshData*`, valid while `self` is alive.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Replaces the vertex buffer with `vertices`, as `(x, y)` positions in
    /// device-independent pixels.
    ///
    /// # Panics
    ///
    /// Panics if there are more than `u32::MAX` vertices.
    pub fn set_vertices(&mut self, vertices: &[[f32; 2]]) {
        let count = u32::try_from(vertices.len()).expect("vertex count exceeds u32");
        // SAFETY: live MeshData*; `vertices` is `2 * count` contiguous floats
        // ([f32; 2] is layout-compatible with two consecutive f32s); the C side
        // only reads `count` pairs.
        unsafe {
            noesis_mesh_data_set_vertices(self.ptr.as_ptr(), vertices.as_ptr().cast(), count);
        }
        self.num_vertices = count;
    }

    /// Reads back the vertices last set with [`Self::set_vertices`].
    #[must_use]
    pub fn vertices(&self) -> Vec<[f32; 2]> {
        let mut out = vec![[0.0f32; 2]; self.num_vertices as usize];
        // SAFETY: live MeshData*; `out` holds `2 * num_vertices` floats and the
        // C side reads exactly `num_vertices` pairs back from the buffer.
        unsafe {
            noesis_mesh_data_get_vertices(
                self.ptr.as_ptr(),
                out.as_mut_ptr().cast(),
                self.num_vertices,
            );
        }
        out
    }

    /// Number of vertices last set via [`Self::set_vertices`].
    #[must_use]
    pub fn num_vertices(&self) -> u32 {
        self.num_vertices
    }

    /// Replaces the texture-coordinate buffer with `uvs`, one `(u, v)` pair
    /// per vertex.
    ///
    /// # Panics
    ///
    /// Panics if there are more than `u32::MAX` coordinates.
    pub fn set_uvs(&mut self, uvs: &[[f32; 2]]) {
        let count = u32::try_from(uvs.len()).expect("uv count exceeds u32");
        // SAFETY: as `set_vertices`.
        unsafe {
            noesis_mesh_data_set_uvs(self.ptr.as_ptr(), uvs.as_ptr().cast(), count);
        }
        self.num_uvs = count;
    }

    /// Reads back the texture coordinates last set with [`Self::set_uvs`].
    #[must_use]
    pub fn uvs(&self) -> Vec<[f32; 2]> {
        let mut out = vec![[0.0f32; 2]; self.num_uvs as usize];
        // SAFETY: as `vertices`.
        unsafe {
            noesis_mesh_data_get_uvs(self.ptr.as_ptr(), out.as_mut_ptr().cast(), self.num_uvs);
        }
        out
    }

    /// Number of texture coordinates last set via [`Self::set_uvs`].
    #[must_use]
    pub fn num_uvs(&self) -> u32 {
        self.num_uvs
    }

    /// Replaces the index buffer with `indices`, three per triangle.
    ///
    /// # Panics
    ///
    /// Panics if there are more than `u32::MAX` indices.
    pub fn set_indices(&mut self, indices: &[u16]) {
        let count = u32::try_from(indices.len()).expect("index count exceeds u32");
        // SAFETY: live MeshData*; `indices` is `count` contiguous u16s the C
        // side only reads.
        unsafe {
            noesis_mesh_data_set_indices(self.ptr.as_ptr(), indices.as_ptr(), count);
        }
        self.num_indices = count;
    }

    /// Reads back the indices last set with [`Self::set_indices`].
    #[must_use]
    pub fn indices(&self) -> Vec<u16> {
        let mut out = vec![0u16; self.num_indices as usize];
        // SAFETY: live MeshData*; `out` holds `num_indices` u16s.
        unsafe {
            noesis_mesh_data_get_indices(self.ptr.as_ptr(), out.as_mut_ptr(), self.num_indices);
        }
        out
    }

    /// Number of indices last set via [`Self::set_indices`].
    #[must_use]
    pub fn num_indices(&self) -> u32 {
        self.num_indices
    }

    /// Sets the bounding box as `[x, y, width, height]` in device-independent
    /// pixels. Noesis does not compute it from the vertices.
    pub fn set_bounds(&mut self, bounds: [f32; 4]) {
        // SAFETY: live MeshData*.
        unsafe {
            noesis_mesh_data_set_bounds(
                self.ptr.as_ptr(),
                bounds[0],
                bounds[1],
                bounds[2],
                bounds[3],
            );
        }
    }

    /// The bounding box as `[x, y, width, height]`.
    #[must_use]
    pub fn bounds(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: live MeshData*; `out` is a 4-float buffer.
        unsafe {
            noesis_mesh_data_get_bounds(self.ptr.as_ptr(), out.as_mut_ptr());
        }
        out
    }
}

impl Default for MeshData {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MeshData {
    fn drop(&mut self) {
        // SAFETY: produced by noesis_mesh_data_create with a +1 ref we own.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A [`FrameworkElement`] that draws a [`MeshData`] filled with a [`Brush`].
/// Once its [`raw`](Mesh::raw) pointer is in the element tree, Noesis holds its
/// own reference and this handle can be dropped.
///
/// [`FrameworkElement`]: crate::view::FrameworkElement
pub struct Mesh {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for Mesh {}

impl Mesh {
    /// Creates a mesh element with no data or brush.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate (not expected after [`crate::init`]).
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: returns a +1-owned Mesh* this handle releases on Drop.
        let ptr = unsafe { noesis_mesh_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_mesh_create returned null"),
        }
    }

    /// The underlying `Noesis::Mesh*` (also a `FrameworkElement*`), valid
    /// while `self` is alive.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Sets the geometry to draw. Noesis takes its own reference, so `data` can
    /// be dropped afterwards. Returns `false` if the data was not set.
    #[must_use = "a false return means the data was not set (the Mesh pointer was null or not a Noesis::Mesh)"]
    pub fn set_data(&mut self, data: &MeshData) -> bool {
        // SAFETY: self.ptr is a live Mesh*; data.raw() is a live MeshData*.
        unsafe { noesis_mesh_set_data(self.ptr.as_ptr(), data.raw()) }
    }

    /// The current `Noesis::MeshData*`, or `None`. Borrowed: do not release it.
    #[must_use]
    pub fn data(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live Mesh*; the returned pointer is borrowed.
        NonNull::new(unsafe { noesis_mesh_get_data(self.ptr.as_ptr()) })
    }

    /// Sets the fill brush. Noesis takes its own reference. Returns `false` if
    /// the brush was not set.
    #[must_use = "a false return means the brush was not set (the Mesh pointer was null or not a Noesis::Mesh)"]
    pub fn set_brush(&mut self, brush: &dyn Brush) -> bool {
        // SAFETY: self.ptr is a live Mesh*; brush_raw() is a live Brush*.
        unsafe { noesis_mesh_set_brush(self.ptr.as_ptr(), brush.brush_raw()) }
    }

    /// The current `Noesis::Brush*`, or `None`. Borrowed: do not release it.
    #[must_use]
    pub fn brush(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live Mesh*; the returned pointer is borrowed.
        NonNull::new(unsafe { noesis_mesh_get_brush(self.ptr.as_ptr()) })
    }
}

impl Default for Mesh {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Mesh {
    fn drop(&mut self) {
        // SAFETY: produced by noesis_mesh_create with a +1 ref we own.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}
