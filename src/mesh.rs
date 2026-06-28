//! Code-built [`MeshData`] + [`Mesh`] element for immediate-mode drawing.
//!
//! [`MeshData`] is the low-level CPU geometry payload Noesis submits straight to
//! the GPU: interleaved `(x, y)` vertices, optional `(u, v)` texture
//! coordinates, a 16-bit triangle index buffer, and an explicit bounding box. It
//! is consumed two ways: by
//! [`DrawingContext::draw_mesh`](crate::drawing::DrawingContext::draw_mesh) in an
//! `OnRender` callback, or hosted in a [`Mesh`] [`FrameworkElement`](crate::view::FrameworkElement) in the
//! element tree.
//!
//! Both handles own a freshly-created Noesis object holding a single `+1`
//! reference released on [`Drop`], the same ownership idiom as
//! [`crate::brushes`] / [`crate::shapes`].
//!
//! # Read-back
//!
//! The buffers live on the CPU, so the setters round-trip headlessly: write a
//! buffer with [`MeshData::set_vertices`] / [`set_uvs`](MeshData::set_uvs) /
//! [`set_indices`](MeshData::set_indices) and read the same values back with the
//! matching getter, and the bounds round-trip through [`MeshData::bounds`].
//! Noesis 3.2.13 exposes no `GetNumVertices`/`...` getter, so the element count is
//! proven by the buffer data that reads back at that length (the handle tracks
//! the count it last set so the getters know how many elements to read).

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

/// An owning handle to a Noesis `MeshData`: the CPU vertex / UV / index buffers
/// plus a bounding box. Build it, then draw it via
/// [`DrawingContext::draw_mesh`](crate::drawing::DrawingContext::draw_mesh) or
/// host it in a [`Mesh`].
pub struct MeshData {
    ptr: NonNull<c_void>,
    num_vertices: u32,
    num_uvs: u32,
    num_indices: u32,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for MeshData {}

impl MeshData {
    /// Create an empty `MeshData` (no vertices / UVs / indices, zero bounds).
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

    /// Raw `Noesis::MeshData*` (a `BaseComponent*`), borrowed for `self`'s
    /// lifetime.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Replace the vertex buffer with `vertices` (`(x, y)` pairs in DIPs),
    /// resizing it to `vertices.len()`.
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

    /// Read the vertex buffer back from the live object (length == the count
    /// last set via [`Self::set_vertices`]).
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

    /// Replace the texture-coordinate buffer with `uvs` (`(u, v)` pairs),
    /// resizing it to `uvs.len()`.
    pub fn set_uvs(&mut self, uvs: &[[f32; 2]]) {
        let count = u32::try_from(uvs.len()).expect("uv count exceeds u32");
        // SAFETY: as `set_vertices`.
        unsafe {
            noesis_mesh_data_set_uvs(self.ptr.as_ptr(), uvs.as_ptr().cast(), count);
        }
        self.num_uvs = count;
    }

    /// Read the texture-coordinate buffer back from the live object.
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

    /// Replace the 16-bit triangle index buffer with `indices`, resizing it to
    /// `indices.len()`.
    pub fn set_indices(&mut self, indices: &[u16]) {
        let count = u32::try_from(indices.len()).expect("index count exceeds u32");
        // SAFETY: live MeshData*; `indices` is `count` contiguous u16s the C
        // side only reads.
        unsafe {
            noesis_mesh_data_set_indices(self.ptr.as_ptr(), indices.as_ptr(), count);
        }
        self.num_indices = count;
    }

    /// Read the index buffer back from the live object.
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

    /// Set the bounding box `[x, y, w, h]` in DIPs.
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

    /// Read the bounding box `[x, y, w, h]` back from the live object.
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

/// An owning handle to a Noesis `Mesh`: a [`FrameworkElement`] that renders a
/// [`MeshData`] filled with a [`Brush`]. Hand its [`raw`](Mesh::raw) pointer to
/// the element tree (Noesis takes its own reference) and the handle may then be
/// dropped.
///
/// [`FrameworkElement`]: crate::view::FrameworkElement
pub struct Mesh {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for Mesh {}

impl Mesh {
    /// Create an empty `Mesh` element (no data / brush set yet).
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

    /// Raw `Noesis::Mesh*` (also a `FrameworkElement*` / `BaseComponent*`),
    /// borrowed for `self`'s lifetime.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Assign the [`MeshData`] to render (Noesis takes its own reference, so the
    /// `MeshData` handle may be dropped afterwards).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_data(&mut self, data: &MeshData) -> bool {
        // SAFETY: self.ptr is a live Mesh*; data.raw() is a live MeshData*.
        unsafe { noesis_mesh_set_data(self.ptr.as_ptr(), data.raw()) }
    }

    /// Borrowed `Noesis::MeshData*` currently set, or `None`. The pointer has no
    /// `+1` reference; do not release it.
    #[must_use]
    pub fn data(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live Mesh*; the returned pointer is borrowed.
        NonNull::new(unsafe { noesis_mesh_get_data(self.ptr.as_ptr()) })
    }

    /// Set the fill [`Brush`] (Noesis takes its own reference).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_brush(&mut self, brush: &dyn Brush) -> bool {
        // SAFETY: self.ptr is a live Mesh*; brush_raw() is a live Brush*.
        unsafe { noesis_mesh_set_brush(self.ptr.as_ptr(), brush.brush_raw()) }
    }

    /// Borrowed `Noesis::Brush*` currently set, or `None`. The pointer has no
    /// `+1` reference; do not release it.
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
