// Immediate-mode drawing: Pen and DrawingContext.
//
//   * `Pen`: created here and handed out with one reference owned by the
//     caller, like the brushes in cpp/noesis_brushes.cpp.
//
//   * `DrawingContext` draw / push / pop commands. DrawingContext has a private
//     constructor in 3.2.13 and is delivered only to UIElement::OnRender, so
//     these take the borrowed context pointer that the class render trampoline
//     (cpp/noesis_classes.cpp) hands out. They have no read-back.
//
// noesis_drawing_rect_geometry_create builds a RectangleGeometry for
// DrawGeometry / PushClip.

#include "noesis_shim.h"

#include <clocale>
#include <cstdio>
#include <string>

#include <NsCore/BaseComponent.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsDrawing/Point.h>
#include <NsDrawing/Rect.h>
#include <NsGui/Brush.h>
#include <NsGui/DashStyle.h>
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

// Adds the caller's reference; the creating Ptr releases its own at scope exit.
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

extern "C" void* noesis_pen_create(void* brush, float thickness) {
    Noesis::Ptr<Noesis::Pen> pen = *new Noesis::Pen();
    pen->SetThickness(thickness);
    if (auto* b = cast<Noesis::Brush>(brush)) pen->SetBrush(b);
    return handout(pen.GetPtr());
}

extern "C" bool noesis_pen_set_brush(void* pen, void* brush) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p) return false;
    p->SetBrush(cast<Noesis::Brush>(brush));
    return true;
}

extern "C" void* noesis_pen_get_brush(void* pen) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p) return nullptr;
    return p->GetBrush();
}

extern "C" bool noesis_pen_set_thickness(void* pen, float thickness) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p) return false;
    p->SetThickness(thickness);
    return true;
}

extern "C" bool noesis_pen_get_thickness(void* pen, float* out) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p || !out) return false;
    *out = p->GetThickness();
    return true;
}

extern "C" bool noesis_pen_set_line_caps(void* pen, int32_t start_cap, int32_t end_cap,
                                            int32_t dash_cap) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p) return false;
    p->SetStartLineCap(static_cast<Noesis::PenLineCap>(start_cap));
    p->SetEndLineCap(static_cast<Noesis::PenLineCap>(end_cap));
    p->SetDashCap(static_cast<Noesis::PenLineCap>(dash_cap));
    return true;
}

extern "C" bool noesis_pen_get_line_caps(void* pen, int32_t out[3]) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p || !out) return false;
    out[0] = static_cast<int32_t>(p->GetStartLineCap());
    out[1] = static_cast<int32_t>(p->GetEndLineCap());
    out[2] = static_cast<int32_t>(p->GetDashCap());
    return true;
}

extern "C" bool noesis_pen_set_line_join(void* pen, int32_t join, float miter_limit) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p) return false;
    p->SetLineJoin(static_cast<Noesis::PenLineJoin>(join));
    p->SetMiterLimit(miter_limit);
    return true;
}

extern "C" bool noesis_pen_get_line_join(void* pen, int32_t* out_join, float* out_miter_limit) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p) return false;
    if (out_join) *out_join = static_cast<int32_t>(p->GetLineJoin());
    if (out_miter_limit) *out_miter_limit = p->GetMiterLimit();
    return true;
}

// ── Pen dash style (immediate-mode dashed strokes) ───────────────────────────
//
// Builds a DashStyle from `dashes` and assigns it to the Pen. Noesis takes
// DashStyle.Dashes as a space-separated string, so the array is formatted here.
// `count == 0` clears the dash style (solid stroke). Dash lengths are multiples
// of the pen thickness.

extern "C" bool noesis_pen_set_dash_style(
    void* pen, const float* dashes, uint32_t count, float offset) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p) return false;
    if (count == 0) {
        p->SetDashStyle(nullptr);
        return true;
    }
    if (!dashes) return false;
    Noesis::Ptr<Noesis::DashStyle> style = *new Noesis::DashStyle();
    std::string text;
    char buf[32];
    // %g honors LC_NUMERIC; a de_DE-style "1,5" is rejected by Noesis's parser.
    const char sep = std::localeconv()->decimal_point[0];
    for (uint32_t i = 0; i < count; ++i) {
        if (i != 0) text.push_back(' ');
        std::snprintf(buf, sizeof(buf), "%g", static_cast<double>(dashes[i]));
        if (sep != '.') {
            for (char* c = buf; *c; ++c) {
                if (*c == sep) *c = '.';
            }
        }
        text += buf;
    }
    style->SetDashes(text.c_str());
    style->SetOffset(offset);
    p->SetDashStyle(style.GetPtr());
    return true;
}

// Read the Pen's DashStyle.Offset into *out. False (output untouched) if the
// Pen has no dash style set or `pen` is not a Pen.
extern "C" bool noesis_pen_get_dash_offset(void* pen, float* out) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p || !out) return false;
    Noesis::DashStyle* style = p->GetDashStyle();
    if (!style) return false;
    *out = style->GetOffset();
    return true;
}

// Borrowed space-separated dash string from the Pen's DashStyle
// (DashStyle::GetDashes), or null if no dash style is set / not a Pen. Valid
// until the Pen (or its DashStyle) is next mutated; copy it out immediately.
extern "C" const char* noesis_pen_get_dashes(void* pen) {
    auto* p = cast<Noesis::Pen>(pen);
    if (!p) return nullptr;
    Noesis::DashStyle* style = p->GetDashStyle();
    return style ? style->GetDashes() : nullptr;
}

// ── RectangleGeometry ────────────────────────────────────────────────────────

extern "C" void* noesis_drawing_rect_geometry_create(float x, float y, float w, float h, float rX,
                                                     float rY) {
    Noesis::Ptr<Noesis::RectangleGeometry> geo =
        *new Noesis::RectangleGeometry(rect_xywh(x, y, w, h), rX, rY);
    return handout(geo.GetPtr());
}

// out = {x, y, w, h}
extern "C" bool noesis_rectangle_geometry_get_rect(void* geometry, float out[4]) {
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

extern "C" bool noesis_drawing_draw_line(void* context, void* pen, float x0, float y0, float x1,
                                            float y1) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    if (!dc) return false;
    dc->DrawLine(cast<Noesis::Pen>(pen), Noesis::Point(x0, y0), Noesis::Point(x1, y1));
    return true;
}

extern "C" bool noesis_drawing_draw_rectangle(void* context, void* brush, void* pen, float x,
                                                 float y, float w, float h) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    if (!dc) return false;
    dc->DrawRectangle(cast<Noesis::Brush>(brush), cast<Noesis::Pen>(pen), rect_xywh(x, y, w, h));
    return true;
}

extern "C" bool noesis_drawing_draw_rounded_rectangle(void* context, void* brush, void* pen,
                                                         float x, float y, float w, float h,
                                                         float rX, float rY) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    if (!dc) return false;
    dc->DrawRoundedRectangle(cast<Noesis::Brush>(brush), cast<Noesis::Pen>(pen),
                             rect_xywh(x, y, w, h), rX, rY);
    return true;
}

extern "C" bool noesis_drawing_draw_ellipse(void* context, void* brush, void* pen, float cx,
                                               float cy, float rX, float rY) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    if (!dc) return false;
    dc->DrawEllipse(cast<Noesis::Brush>(brush), cast<Noesis::Pen>(pen), Noesis::Point(cx, cy), rX,
                    rY);
    return true;
}

extern "C" bool noesis_drawing_draw_geometry(void* context, void* brush, void* pen,
                                                void* geometry) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    if (!dc) return false;
    dc->DrawGeometry(cast<Noesis::Brush>(brush), cast<Noesis::Pen>(pen),
                     cast<Noesis::Geometry>(geometry));
    return true;
}

// Draws a FormattedText (cpp/noesis_formatted_text.cpp) into the bounds
// {x, y, w, h}. No brush argument: the foreground is baked into the
// FormattedText.
extern "C" bool noesis_drawing_draw_text(void* context, void* formatted_text, float x, float y,
                                            float w, float h) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    auto* ft = cast<Noesis::FormattedText>(formatted_text);
    if (!dc || !ft) return false;
    dc->DrawText(ft, rect_xywh(x, y, w, h));
    return true;
}

// Fills a MeshData (cpp/noesis_mesh.cpp) with `brush`. A null mesh is
// rejected; a null brush paints nothing.
extern "C" bool noesis_drawing_draw_mesh(void* context, void* brush, void* mesh) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    auto* md = cast<Noesis::MeshData>(mesh);
    if (!dc || !md) return false;
    dc->DrawMesh(cast<Noesis::Brush>(brush), md);
    return true;
}

extern "C" bool noesis_drawing_draw_image(void* context, void* image_source, float x, float y,
                                             float w, float h) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    auto* img = cast<Noesis::ImageSource>(image_source);
    // null source asserts inside Noesis
    if (!dc || !img) return false;
    dc->DrawImage(img, rect_xywh(x, y, w, h));
    return true;
}

extern "C" bool noesis_drawing_pop(void* context) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    if (!dc) return false;
    dc->Pop();
    return true;
}

extern "C" bool noesis_drawing_push_clip(void* context, void* geometry) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    auto* g = cast<Noesis::Geometry>(geometry);
    if (!dc || !g) return false;
    dc->PushClip(g);
    return true;
}

extern "C" bool noesis_drawing_push_transform(void* context, void* transform) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    auto* t = cast<Noesis::Transform>(transform);
    if (!dc || !t) return false;
    dc->PushTransform(t);
    return true;
}

extern "C" bool noesis_drawing_push_blending_mode(void* context, int32_t mode) {
    auto* dc = cast<Noesis::DrawingContext>(context);
    if (!dc) return false;
    dc->PushBlendingMode(static_cast<Noesis::BlendingMode>(mode));
    return true;
}
