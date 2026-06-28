// Immediate-mode drawing: Pen + DrawingContext (TODO §10).
//
// Two surfaces live here:
//
//   1. A code-built `Pen` (NsGui/Pen.h) — constructed from Rust and handed out
//      with a single owned reference, exactly like the brushes / transforms in
//      cpp/noesis_brushes.cpp (handout() + `*new T` adopt). Several
//      DrawingContext draw calls need a Pen, and Noesis's Pen has read-back
//      getters (GetThickness / GetBrush / GetStartLineCap / …) so a test can
//      prove a value crossed into the live object rather than being cached.
//
//   2. The `DrawingContext` draw / push / pop commands. A DrawingContext has a
//      PRIVATE constructor in 3.2.13 (friend UIElement) and is delivered ONLY
//      to UIElement::OnRender — so these entrypoints take the BORROWED context
//      pointer the class render trampoline (cpp/noesis_classes.cpp) hands out,
//      DynamicCast it to a Noesis::DrawingContext*, and forward the call. They
//      are immediate-mode (no read-back): the test proves they reached Noesis
//      by driving a real render pass and observing the produced geometry.
//
// A minimal RectangleGeometry create is also exposed so the DrawGeometry /
// PushClip context entrypoints are reachable (a Geometry argument is otherwise
// unconstructable from this crate; full geometry construction is TODO §10).

#include "noesis_shim.h"

#include <NsCore/BaseComponent.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsDrawing/Point.h>
#include <NsDrawing/Rect.h>
#include <NsGui/Brush.h>
#include <NsGui/DrawingContext.h>
#include <NsGui/Enums.h>  // BlendingMode, PenLineCap, PenLineJoin
#include <NsGui/FormattedText.h>
#include <NsGui/Geometry.h>
#include <NsGui/ImageSource.h>
#include <NsGui/MeshData.h>
#include <NsGui/Pen.h>
#include <NsGui/RectangleGeometry.h>
#include <NsGui/Transform.h>

namespace {

// Hand a freshly-created (refcount-1) BaseComponent out across the C ABI with
// exactly one reference owned by the caller (mirrors cpp/noesis_brushes.cpp).
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

// Noesis::Rect's 4-arg constructor is (left, top, right, bottom).
Noesis::Rect rect_xywh(float x, float y, float w, float h) {
    return Noesis::Rect(x, y, x + w, y + h);
}

}  // namespace

// ── Pen ──────────────────────────────────────────────────────────────────────

extern "C" void* dm_noesis_pen_create(void* brush, float thickness) {
    Noesis::Ptr<Noesis::Pen> pen = *new Noesis::Pen();
    pen->SetThickness(thickness);
    if (auto* b = cast<Noesis::Brush>(brush)) pen->SetBrush(b);
    return handout(pen.GetPtr());
}

extern "C" bool dm_noesis_pen_set_brush(void* pen, void* brush) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p) return false;
    p->SetBrush(cast<Noesis::Brush>(brush));
    return true;
}

extern "C" void* dm_noesis_pen_get_brush(void* pen) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p) return nullptr;
    return p->GetBrush();
}

extern "C" bool dm_noesis_pen_set_thickness(void* pen, float thickness) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p) return false;
    p->SetThickness(thickness);
    return true;
}

extern "C" bool dm_noesis_pen_get_thickness(void* pen, float* out) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p || !out) return false;
    *out = p->GetThickness();
    return true;
}

extern "C" bool dm_noesis_pen_set_line_caps(void* pen, int32_t start_cap, int32_t end_cap,
                                            int32_t dash_cap) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p) return false;
    p->SetStartLineCap(static_cast<Noesis::PenLineCap>(start_cap));
    p->SetEndLineCap(static_cast<Noesis::PenLineCap>(end_cap));
    p->SetDashCap(static_cast<Noesis::PenLineCap>(dash_cap));
    return true;
}

extern "C" bool dm_noesis_pen_get_line_caps(void* pen, int32_t out[3]) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p || !out) return false;
    out[0] = static_cast<int32_t>(p->GetStartLineCap());
    out[1] = static_cast<int32_t>(p->GetEndLineCap());
    out[2] = static_cast<int32_t>(p->GetDashCap());
    return true;
}

extern "C" bool dm_noesis_pen_set_line_join(void* pen, int32_t join, float miter_limit) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p) return false;
    p->SetLineJoin(static_cast<Noesis::PenLineJoin>(join));
    p->SetMiterLimit(miter_limit);
    return true;
}

extern "C" bool dm_noesis_pen_get_line_join(void* pen, int32_t* out_join, float* out_miter_limit) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p) return false;
    if (out_join) *out_join = static_cast<int32_t>(p->GetLineJoin());
    if (out_miter_limit) *out_miter_limit = p->GetMiterLimit();
    return true;
}

// ── RectangleGeometry ────────────────────────────────────────────────────────

extern "C" void* dm_noesis_drawing_rect_geometry_create(float x, float y, float w, float h, float rX,
                                                     float rY) {
    Noesis::Ptr<Noesis::RectangleGeometry> geo =
        *new Noesis::RectangleGeometry(rect_xywh(x, y, w, h), rX, rY);
    return handout(geo.GetPtr());
}

// out = {x, y, w, h}
extern "C" bool dm_noesis_rectangle_geometry_get_rect(void* geometry, float out[4]) {
    auto* g = cast<Noesis::RectangleGeometry>(geometry);
    if (!g || !out) return false;
    const Noesis::Rect& r = g->GetRect();
    out[0] = r.x;
    out[1] = r.y;
    out[2] = r.width;
    out[3] = r.height;
    return true;
}

// ── DrawingContext ───────────────────────────────────────────────────────────

extern "C" bool dm_noesis_drawing_draw_line(void* context, void* pen, float x0, float y0, float x1,
                                            float y1) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    if (!dc) return false;
    dc->DrawLine(cast<Noesis::Pen>(pen), Noesis::Point(x0, y0), Noesis::Point(x1, y1));
    return true;
}

extern "C" bool dm_noesis_drawing_draw_rectangle(void* context, void* brush, void* pen, float x,
                                                 float y, float w, float h) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    if (!dc) return false;
    dc->DrawRectangle(cast<Noesis::Brush>(brush), cast<Noesis::Pen>(pen), rect_xywh(x, y, w, h));
    return true;
}

extern "C" bool dm_noesis_drawing_draw_rounded_rectangle(void* context, void* brush, void* pen,
                                                         float x, float y, float w, float h,
                                                         float rX, float rY) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    if (!dc) return false;
    dc->DrawRoundedRectangle(cast<Noesis::Brush>(brush), cast<Noesis::Pen>(pen),
                             rect_xywh(x, y, w, h), rX, rY);
    return true;
}

extern "C" bool dm_noesis_drawing_draw_ellipse(void* context, void* brush, void* pen, float cx,
                                               float cy, float rX, float rY) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    if (!dc) return false;
    dc->DrawEllipse(cast<Noesis::Brush>(brush), cast<Noesis::Pen>(pen), Noesis::Point(cx, cy), rX,
                    rY);
    return true;
}

extern "C" bool dm_noesis_drawing_draw_geometry(void* context, void* brush, void* pen,
                                                void* geometry) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    if (!dc) return false;
    dc->DrawGeometry(cast<Noesis::Brush>(brush), cast<Noesis::Pen>(pen),
                     cast<Noesis::Geometry>(geometry));
    return true;
}

// Draw a FormattedText (NsGui/FormattedText.h, fully wrapped in
// cpp/noesis_formatted_text.cpp) into the bounds rect {x, y, w, h}. The text's
// foreground/brush is baked into the FormattedText itself, so DrawText takes no
// brush argument (see NsGui/DrawingContext.h: DrawText(FormattedText*, Rect)).
extern "C" bool dm_noesis_drawing_draw_text(void* context, void* formatted_text, float x, float y,
                                            float w, float h) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    auto* ft = cast<Noesis::FormattedText>(formatted_text);
    if (!dc || !ft) return false;
    dc->DrawText(ft, rect_xywh(x, y, w, h));
    return true;
}

// Fill a MeshData (NsGui/MeshData.h, built via dm_noesis_mesh_data_* in
// cpp/noesis_mesh.cpp) with `brush`. A null mesh is rejected; a null brush
// paints nothing (matching the other fill calls).
extern "C" bool dm_noesis_drawing_draw_mesh(void* context, void* brush, void* mesh) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    auto* md = cast<Noesis::MeshData>(mesh);
    if (!dc || !md) return false;
    dc->DrawMesh(cast<Noesis::Brush>(brush), md);
    return true;
}

extern "C" bool dm_noesis_drawing_draw_image(void* context, void* image_source, float x, float y,
                                             float w, float h) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    auto* img = cast<Noesis::ImageSource>(image_source);
    // DrawImage requires a real source; reject null rather than asserting inside
    // Noesis (building an ImageSource headless is TODO §12).
    if (!dc || !img) return false;
    dc->DrawImage(img, rect_xywh(x, y, w, h));
    return true;
}

extern "C" bool dm_noesis_drawing_pop(void* context) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    if (!dc) return false;
    dc->Pop();
    return true;
}

extern "C" bool dm_noesis_drawing_push_clip(void* context, void* geometry) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    auto* g = cast<Noesis::Geometry>(geometry);
    if (!dc || !g) return false;
    dc->PushClip(g);
    return true;
}

extern "C" bool dm_noesis_drawing_push_transform(void* context, void* transform) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    auto* t = cast<Noesis::Transform>(transform);
    if (!dc || !t) return false;
    dc->PushTransform(t);
    return true;
}

extern "C" bool dm_noesis_drawing_push_blending_mode(void* context, int32_t mode) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    if (!dc) return false;
    dc->PushBlendingMode(static_cast<Noesis::BlendingMode>(mode));
    return true;
}
