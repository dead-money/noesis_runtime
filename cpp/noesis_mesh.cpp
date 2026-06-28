// Code-built MeshData + Mesh element (immediate-mode drawing).
//
// MeshData (NsGui/MeshData.h) is an Animatable holding CPU-side vertex / UV /
// index buffers plus an explicit bounding box. It is the low-level geometry
// payload consumed by DrawingContext::DrawMesh (cpp/noesis_drawing.cpp) and by
// the Mesh FrameworkElement (NsGui/Mesh.h). The buffers and bounds round-trip
// entirely on the CPU, so a headless test can prove values crossed the FFI by
// writing them through the setters and reading them back through GetVertices /
// GetUVs / GetIndices / GetBounds. There is no GetNum* getter in 3.2.13, so a
// count is proven by the buffer data that round-trips at that length.
//
// Both objects are handed out with a single owned +1 reference (handout() idiom
// shared with cpp/noesis_brushes.cpp); the Rust handle's Drop releases it.

#include "noesis_shim.h"

#include <NsCore/BaseComponent.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsDrawing/Point.h>
#include <NsDrawing/Rect.h>
#include <NsGui/Brush.h>
#include <NsGui/Mesh.h>
#include <NsGui/MeshData.h>

namespace {

void* handout(Noesis::BaseComponent* c) {
    if (!c) return nullptr;
    c->AddReference();
    return c;
}

template <class T>
T* cast(void* p) {
    if (!p) return nullptr;
    return Noesis::DynamicCast<T*>(static_cast<Noesis::BaseComponent*>(p));
}

}  // namespace

// ── MeshData ─────────────────────────────────────────────────────────────────

extern "C" void* noesis_mesh_data_create(void) {
    Noesis::Ptr<Noesis::MeshData> md = *new Noesis::MeshData();
    return handout(md.GetPtr());
}

// Set the vertex buffer from `count` interleaved (x, y) pairs (2*count floats).
// Resizes the buffer to `count` and notifies the change.
extern "C" bool noesis_mesh_data_set_vertices(void* mesh, const float* xy, uint32_t count) {
    auto* md = cast<Noesis::MeshData>(mesh);
    if (!md || (count != 0 && !xy)) return false;
    md->SetNumVertices(count);
    Noesis::Point* verts = md->GetVertices();
    for (uint32_t i = 0; i < count; ++i) {
        verts[i] = Noesis::Point(xy[2 * i], xy[2 * i + 1]);
    }
    md->Updated(Noesis::MeshData::Updated_Vertices);
    return true;
}

// Read `count` (x, y) pairs back from the vertex buffer into `out_xy`
// (2*count floats). The caller must pass the same count it set.
extern "C" bool noesis_mesh_data_get_vertices(void* mesh, float* out_xy, uint32_t count) {
    auto* md = cast<Noesis::MeshData>(mesh);
    if (!md || (count != 0 && !out_xy)) return false;
    const Noesis::Point* verts = md->GetVertices();
    for (uint32_t i = 0; i < count; ++i) {
        out_xy[2 * i] = verts[i].x;
        out_xy[2 * i + 1] = verts[i].y;
    }
    return true;
}

// Set the texture-coordinate buffer from `count` interleaved (u, v) pairs.
extern "C" bool noesis_mesh_data_set_uvs(void* mesh, const float* uv, uint32_t count) {
    auto* md = cast<Noesis::MeshData>(mesh);
    if (!md || (count != 0 && !uv)) return false;
    md->SetNumUVs(count);
    Noesis::Point* uvs = md->GetUVs();
    for (uint32_t i = 0; i < count; ++i) {
        uvs[i] = Noesis::Point(uv[2 * i], uv[2 * i + 1]);
    }
    md->Updated(Noesis::MeshData::Updated_UVs);
    return true;
}

extern "C" bool noesis_mesh_data_get_uvs(void* mesh, float* out_uv, uint32_t count) {
    auto* md = cast<Noesis::MeshData>(mesh);
    if (!md || (count != 0 && !out_uv)) return false;
    const Noesis::Point* uvs = md->GetUVs();
    for (uint32_t i = 0; i < count; ++i) {
        out_uv[2 * i] = uvs[i].x;
        out_uv[2 * i + 1] = uvs[i].y;
    }
    return true;
}

// Set the (16-bit) triangle index buffer from `count` indices.
extern "C" bool noesis_mesh_data_set_indices(void* mesh, const uint16_t* indices,
                                                uint32_t count) {
    auto* md = cast<Noesis::MeshData>(mesh);
    if (!md || (count != 0 && !indices)) return false;
    md->SetNumIndices(count);
    uint16_t* dst = md->GetIndices();
    for (uint32_t i = 0; i < count; ++i) {
        dst[i] = indices[i];
    }
    md->Updated(Noesis::MeshData::Updated_Indices);
    return true;
}

extern "C" bool noesis_mesh_data_get_indices(void* mesh, uint16_t* out_indices, uint32_t count) {
    auto* md = cast<Noesis::MeshData>(mesh);
    if (!md || (count != 0 && !out_indices)) return false;
    const uint16_t* src = md->GetIndices();
    for (uint32_t i = 0; i < count; ++i) {
        out_indices[i] = src[i];
    }
    return true;
}

extern "C" bool noesis_mesh_data_set_bounds(void* mesh, float x, float y, float w, float h) {
    auto* md = cast<Noesis::MeshData>(mesh);
    if (!md) return false;
    md->SetBounds(Noesis::Rect(x, y, x + w, y + h));
    return true;
}

// out = {x, y, w, h}
extern "C" bool noesis_mesh_data_get_bounds(void* mesh, float out[4]) {
    auto* md = cast<Noesis::MeshData>(mesh);
    if (!md || !out) return false;
    const Noesis::Rect& r = md->GetBounds();
    out[0] = r.x;
    out[1] = r.y;
    out[2] = r.width;
    out[3] = r.height;
    return true;
}

// ── Mesh element ─────────────────────────────────────────────────────────────

extern "C" void* noesis_mesh_create(void) {
    Noesis::Ptr<Noesis::Mesh> mesh = *new Noesis::Mesh();
    return handout(mesh.GetPtr());
}

extern "C" bool noesis_mesh_set_data(void* mesh, void* data) {
    auto* m = cast<Noesis::Mesh>(mesh);
    if (!m) return false;
    m->SetData(cast<Noesis::MeshData>(data));
    return true;
}

// Borrowed MeshData* (no +1 reference; do not release).
extern "C" void* noesis_mesh_get_data(void* mesh) {
    auto* m = cast<Noesis::Mesh>(mesh);
    if (!m) return nullptr;
    return m->GetData();
}

extern "C" bool noesis_mesh_set_brush(void* mesh, void* brush) {
    auto* m = cast<Noesis::Mesh>(mesh);
    if (!m) return false;
    m->SetBrush(cast<Noesis::Brush>(brush));
    return true;
}

// Borrowed Brush* (no +1 reference; do not release).
extern "C" void* noesis_mesh_get_brush(void* mesh) {
    auto* m = cast<Noesis::Mesh>(mesh);
    if (!m) return nullptr;
    return m->GetBrush();
}
