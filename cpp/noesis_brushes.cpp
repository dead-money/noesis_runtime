// Code-built brushes, transforms, effects, and RenderOptions (TODO §11).
//
// These entrypoints construct presentation objects from Rust and hand them out
// across the C ABI with a single owned reference, mirroring the ownership
// idioms already used by cpp/noesis_binding.cpp (handout() + `*new T` adopt) and
// cpp/noesis_collections.cpp. The Rust side (src/brushes.rs / src/transforms.rs)
// wraps each pointer in an owning handle whose Drop calls
// noesis_base_component_release; assigning the object to an element (via the
// generic FrameworkElement::set_component) makes Noesis take its own reference,
// so the Rust builder handle can be dropped afterwards.
//
// Read-back getters (GetColor / GetRadius / GetAngle / …) exist so tests can
// prove a value actually crossed into the live Noesis object rather than being
// cached Rust-side: a stubbed constructor/setter fails the round-trip.

#include "noesis_shim.h"

#include <NsCore/BaseComponent.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsDrawing/Color.h>
#include <NsDrawing/Point.h>
#include <NsDrawing/Rect.h>
#include <NsGui/BlurEffect.h>
#include <NsGui/CompositeTransform.h>
#include <NsGui/CompositeTransform3D.h>
#include <NsGui/DependencyObject.h>
#include <NsGui/DropShadowEffect.h>
#include <NsGui/Effect.h>
#include <NsGui/Enums.h>  // BitmapScalingMode, AlignmentX/Y, Stretch, TileMode, BrushMappingMode
#include <NsGui/GradientBrush.h>
#include <NsGui/GradientStop.h>
#include <NsGui/GradientStopCollection.h>
#include <NsGui/ImageBrush.h>
#include <NsGui/ImageSource.h>
#include <NsGui/LinearGradientBrush.h>
#include <NsGui/MatrixTransform.h>
#include <NsGui/MatrixTransform3D.h>
#include <NsGui/RadialGradientBrush.h>
#include <NsGui/RenderOptions.h>
#include <NsGui/RotateTransform.h>
#include <NsGui/ScaleTransform.h>
#include <NsGui/SkewTransform.h>
#include <NsGui/SolidColorBrush.h>
#include <NsGui/TileBrush.h>
#include <NsGui/Transform.h>
#include <NsGui/Transform3D.h>
#include <NsGui/TransformGroup.h>
#include <NsGui/TranslateTransform.h>
#include <NsGui/UIElement.h>
#include <NsGui/Visual.h>
#include <NsGui/VisualBrush.h>
#include <NsMath/Transform.h>

namespace {

// Hand a freshly-created (refcount-1) BaseComponent out across the C ABI with
// exactly one reference owned by the caller. The local Ptr that produced the
// object releases its own reference on scope exit, leaving the caller's +1.
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

// ── SolidColorBrush ──────────────────────────────────────────────────────────

extern "C" void* noesis_solid_color_brush_create(const float color[4]) {
    Noesis::Color c = color ? Noesis::Color(color[0], color[1], color[2], color[3])
                            : Noesis::Color(0.0f, 0.0f, 0.0f, 1.0f);
    Noesis::Ptr<Noesis::SolidColorBrush> brush = *new Noesis::SolidColorBrush(c);
    return handout(brush.GetPtr());
}

extern "C" bool noesis_solid_color_brush_set_color(void* brush, const float color[4]) {
    auto* b = cast<Noesis::SolidColorBrush>(brush);
    if (!b || !color) return false;
    b->SetColor(Noesis::Color(color[0], color[1], color[2], color[3]));
    return true;
}

extern "C" bool noesis_solid_color_brush_get_color(void* brush, float out[4]) {
    auto* b = cast<Noesis::SolidColorBrush>(brush);
    if (!b || !out) return false;
    const Noesis::Color& c = b->GetColor();
    out[0] = c.r;
    out[1] = c.g;
    out[2] = c.b;
    out[3] = c.a;
    return true;
}

// ── Gradient brushes ─────────────────────────────────────────────────────────

extern "C" void* noesis_linear_gradient_brush_create() {
    Noesis::Ptr<Noesis::LinearGradientBrush> brush = *new Noesis::LinearGradientBrush();
    return handout(brush.GetPtr());
}

extern "C" bool noesis_linear_gradient_brush_set_start_point(void* brush, float x, float y) {
    auto* b = cast<Noesis::LinearGradientBrush>(brush);
    if (!b) return false;
    b->SetStartPoint(Noesis::Point(x, y));
    return true;
}

extern "C" bool noesis_linear_gradient_brush_set_end_point(void* brush, float x, float y) {
    auto* b = cast<Noesis::LinearGradientBrush>(brush);
    if (!b) return false;
    b->SetEndPoint(Noesis::Point(x, y));
    return true;
}

extern "C" bool noesis_linear_gradient_brush_get_points(void* brush, float out[4]) {
    auto* b = cast<Noesis::LinearGradientBrush>(brush);
    if (!b || !out) return false;
    const Noesis::Point& s = b->GetStartPoint();
    const Noesis::Point& e = b->GetEndPoint();
    out[0] = s.x;
    out[1] = s.y;
    out[2] = e.x;
    out[3] = e.y;
    return true;
}

extern "C" void* noesis_radial_gradient_brush_create() {
    Noesis::Ptr<Noesis::RadialGradientBrush> brush = *new Noesis::RadialGradientBrush();
    return handout(brush.GetPtr());
}

extern "C" bool noesis_radial_gradient_brush_set_center(void* brush, float x, float y) {
    auto* b = cast<Noesis::RadialGradientBrush>(brush);
    if (!b) return false;
    b->SetCenter(Noesis::Point(x, y));
    return true;
}

extern "C" bool noesis_radial_gradient_brush_set_gradient_origin(void* brush, float x, float y) {
    auto* b = cast<Noesis::RadialGradientBrush>(brush);
    if (!b) return false;
    b->SetGradientOrigin(Noesis::Point(x, y));
    return true;
}

extern "C" bool noesis_radial_gradient_brush_set_radius(void* brush, float rx, float ry) {
    auto* b = cast<Noesis::RadialGradientBrush>(brush);
    if (!b) return false;
    b->SetRadiusX(rx);
    b->SetRadiusY(ry);
    return true;
}

extern "C" bool noesis_radial_gradient_brush_get_radius(void* brush, float* rx, float* ry) {
    auto* b = cast<Noesis::RadialGradientBrush>(brush);
    if (!b || !rx || !ry) return false;
    *rx = b->GetRadiusX();
    *ry = b->GetRadiusY();
    return true;
}

// Add a gradient stop (offset in 0..=1, color rgba) to any GradientBrush. The
// brush owns a GradientStopCollection by default; we create one if it is null.
// Returns the new stop's index, or -1 on failure.
extern "C" int32_t noesis_gradient_brush_add_stop(void* brush, float offset,
                                                     const float color[4]) {
    auto* b = cast<Noesis::GradientBrush>(brush);
    if (!b || !color) return -1;

    Noesis::GradientStopCollection* stops = b->GetGradientStops();
    if (!stops) {
        Noesis::Ptr<Noesis::GradientStopCollection> created =
            *new Noesis::GradientStopCollection();
        b->SetGradientStops(created.GetPtr());
        stops = created.GetPtr();
    }

    Noesis::Ptr<Noesis::GradientStop> stop = *new Noesis::GradientStop();
    stop->SetOffset(offset);
    stop->SetColor(Noesis::Color(color[0], color[1], color[2], color[3]));
    return stops->Add(stop.GetPtr());
}

extern "C" int32_t noesis_gradient_brush_stop_count(void* brush) {
    auto* b = cast<Noesis::GradientBrush>(brush);
    if (!b) return -1;
    Noesis::GradientStopCollection* stops = b->GetGradientStops();
    return stops ? stops->Count() : 0;
}

extern "C" bool noesis_gradient_brush_get_stop(void* brush, uint32_t index, float* out_offset,
                                                  float out_color[4]) {
    auto* b = cast<Noesis::GradientBrush>(brush);
    if (!b) return false;
    Noesis::GradientStopCollection* stops = b->GetGradientStops();
    if (!stops || index >= static_cast<uint32_t>(stops->Count())) return false;
    Noesis::GradientStop* stop = stops->Get(index);
    if (!stop) return false;
    if (out_offset) *out_offset = stop->GetOffset();
    if (out_color) {
        const Noesis::Color& c = stop->GetColor();
        out_color[0] = c.r;
        out_color[1] = c.g;
        out_color[2] = c.b;
        out_color[3] = c.a;
    }
    return true;
}

// GradientBrush SpreadMethod / MappingMode (works on any LinearGradientBrush /
// RadialGradientBrush). Ordinals match Noesis::GradientSpreadMethod and
// Noesis::BrushMappingMode; getters return -1 on a non-GradientBrush pointer.
static_assert(Noesis::GradientSpreadMethod_Pad == 0, "GradientSpreadMethod ordinal drift");
static_assert(Noesis::GradientSpreadMethod_Reflect == 1, "GradientSpreadMethod ordinal drift");
static_assert(Noesis::GradientSpreadMethod_Repeat == 2, "GradientSpreadMethod ordinal drift");
static_assert(Noesis::BrushMappingMode_Absolute == 0, "BrushMappingMode ordinal drift");
static_assert(Noesis::BrushMappingMode_RelativeToBoundingBox == 1, "BrushMappingMode ordinal drift");

extern "C" bool noesis_gradient_brush_set_spread_method(void* brush, int32_t method) {
    auto* b = cast<Noesis::GradientBrush>(brush);
    if (!b) return false;
    b->SetSpreadMethod(static_cast<Noesis::GradientSpreadMethod>(method));
    return true;
}

extern "C" int32_t noesis_gradient_brush_get_spread_method(void* brush) {
    auto* b = cast<Noesis::GradientBrush>(brush);
    if (!b) return -1;
    return static_cast<int32_t>(b->GetSpreadMethod());
}

extern "C" bool noesis_gradient_brush_set_mapping_mode(void* brush, int32_t mode) {
    auto* b = cast<Noesis::GradientBrush>(brush);
    if (!b) return false;
    b->SetMappingMode(static_cast<Noesis::BrushMappingMode>(mode));
    return true;
}

extern "C" int32_t noesis_gradient_brush_get_mapping_mode(void* brush) {
    auto* b = cast<Noesis::GradientBrush>(brush);
    if (!b) return -1;
    return static_cast<int32_t>(b->GetMappingMode());
}

// ── ImageBrush ───────────────────────────────────────────────────────────────

// Create an ImageBrush, optionally pointing at a borrowed ImageSource (e.g. a
// pointer from FrameworkElement::get_component on a loaded image, or null to
// wire the source later). Noesis takes its own reference to the source.
extern "C" void* noesis_image_brush_create(void* image_source) {
    auto* src = cast<Noesis::ImageSource>(image_source);
    Noesis::Ptr<Noesis::ImageBrush> brush =
        src ? *new Noesis::ImageBrush(src) : *new Noesis::ImageBrush();
    return handout(brush.GetPtr());
}

extern "C" bool noesis_image_brush_set_image_source(void* brush, void* image_source) {
    auto* b = cast<Noesis::ImageBrush>(brush);
    if (!b) return false;
    b->SetImageSource(cast<Noesis::ImageSource>(image_source));
    return true;
}

// Borrowed (no +1) ImageSource currently set on the brush, or null.
extern "C" void* noesis_image_brush_get_image_source(void* brush) {
    auto* b = cast<Noesis::ImageBrush>(brush);
    if (!b) return nullptr;
    return b->GetImageSource();
}

// ── VisualBrush ──────────────────────────────────────────────────────────────

// Create a VisualBrush, optionally pointing at a borrowed Visual (any element is
// a Visual; pass null to wire the source later). Noesis takes its own reference
// to the visual. NOTE: VisualBrush only renders when the visual is part of the
// logical tree (header comment), but the property assignment + read-back is
// fully headless-verifiable via GetVisual pointer identity.
extern "C" void* noesis_visual_brush_create(void* visual) {
    Noesis::Ptr<Noesis::VisualBrush> brush = *new Noesis::VisualBrush();
    if (visual) {
        brush->SetVisual(cast<Noesis::Visual>(visual));
    }
    return handout(brush.GetPtr());
}

extern "C" bool noesis_visual_brush_set_visual(void* brush, void* visual) {
    auto* b = cast<Noesis::VisualBrush>(brush);
    if (!b) return false;
    b->SetVisual(cast<Noesis::Visual>(visual));
    return true;
}

// Borrowed (no +1) Visual currently set on the brush, or null.
extern "C" void* noesis_visual_brush_get_visual(void* brush) {
    auto* b = cast<Noesis::VisualBrush>(brush);
    if (!b) return nullptr;
    return b->GetVisual();
}

// ── TileBrush tiling knobs (base of ImageBrush AND VisualBrush) ───────────────
//
// AlignmentX/Y, Stretch, TileMode and the Viewport/Viewbox *Units use the
// integer enum ordinals from NsGui/Enums.h. The getters return the ordinal or
// -1 if `brush` is not a TileBrush (every enum's minimum ordinal is 0).
// Viewport/Viewbox are Rects passed as {x, y, width, height}.

extern "C" bool noesis_tile_brush_set_alignment_x(void* brush, int32_t value) {
    auto* b = cast<Noesis::TileBrush>(brush);
    if (!b) return false;
    b->SetAlignmentX(static_cast<Noesis::AlignmentX>(value));
    return true;
}

extern "C" int32_t noesis_tile_brush_get_alignment_x(void* brush) {
    auto* b = cast<Noesis::TileBrush>(brush);
    if (!b) return -1;
    return static_cast<int32_t>(b->GetAlignmentX());
}

extern "C" bool noesis_tile_brush_set_alignment_y(void* brush, int32_t value) {
    auto* b = cast<Noesis::TileBrush>(brush);
    if (!b) return false;
    b->SetAlignmentY(static_cast<Noesis::AlignmentY>(value));
    return true;
}

extern "C" int32_t noesis_tile_brush_get_alignment_y(void* brush) {
    auto* b = cast<Noesis::TileBrush>(brush);
    if (!b) return -1;
    return static_cast<int32_t>(b->GetAlignmentY());
}

extern "C" bool noesis_tile_brush_set_stretch(void* brush, int32_t value) {
    auto* b = cast<Noesis::TileBrush>(brush);
    if (!b) return false;
    b->SetStretch(static_cast<Noesis::Stretch>(value));
    return true;
}

extern "C" int32_t noesis_tile_brush_get_stretch(void* brush) {
    auto* b = cast<Noesis::TileBrush>(brush);
    if (!b) return -1;
    return static_cast<int32_t>(b->GetStretch());
}

extern "C" bool noesis_tile_brush_set_tile_mode(void* brush, int32_t value) {
    auto* b = cast<Noesis::TileBrush>(brush);
    if (!b) return false;
    b->SetTileMode(static_cast<Noesis::TileMode>(value));
    return true;
}

extern "C" int32_t noesis_tile_brush_get_tile_mode(void* brush) {
    auto* b = cast<Noesis::TileBrush>(brush);
    if (!b) return -1;
    return static_cast<int32_t>(b->GetTileMode());
}

extern "C" bool noesis_tile_brush_set_viewport_units(void* brush, int32_t value) {
    auto* b = cast<Noesis::TileBrush>(brush);
    if (!b) return false;
    b->SetViewportUnits(static_cast<Noesis::BrushMappingMode>(value));
    return true;
}

extern "C" int32_t noesis_tile_brush_get_viewport_units(void* brush) {
    auto* b = cast<Noesis::TileBrush>(brush);
    if (!b) return -1;
    return static_cast<int32_t>(b->GetViewportUnits());
}

extern "C" bool noesis_tile_brush_set_viewbox_units(void* brush, int32_t value) {
    auto* b = cast<Noesis::TileBrush>(brush);
    if (!b) return false;
    b->SetViewboxUnits(static_cast<Noesis::BrushMappingMode>(value));
    return true;
}

extern "C" int32_t noesis_tile_brush_get_viewbox_units(void* brush) {
    auto* b = cast<Noesis::TileBrush>(brush);
    if (!b) return -1;
    return static_cast<int32_t>(b->GetViewboxUnits());
}

// rect = {x, y, width, height}
extern "C" bool noesis_tile_brush_set_viewport(void* brush, float x, float y, float w, float h) {
    auto* b = cast<Noesis::TileBrush>(brush);
    if (!b) return false;
    Noesis::Rect r;
    r.x = x;
    r.y = y;
    r.width = w;
    r.height = h;
    b->SetViewport(r);
    return true;
}

extern "C" bool noesis_tile_brush_get_viewport(void* brush, float out[4]) {
    auto* b = cast<Noesis::TileBrush>(brush);
    if (!b || !out) return false;
    const Noesis::Rect& r = b->GetViewport();
    out[0] = r.x;
    out[1] = r.y;
    out[2] = r.width;
    out[3] = r.height;
    return true;
}

extern "C" bool noesis_tile_brush_set_viewbox(void* brush, float x, float y, float w, float h) {
    auto* b = cast<Noesis::TileBrush>(brush);
    if (!b) return false;
    Noesis::Rect r;
    r.x = x;
    r.y = y;
    r.width = w;
    r.height = h;
    b->SetViewbox(r);
    return true;
}

extern "C" bool noesis_tile_brush_get_viewbox(void* brush, float out[4]) {
    auto* b = cast<Noesis::TileBrush>(brush);
    if (!b || !out) return false;
    const Noesis::Rect& r = b->GetViewbox();
    out[0] = r.x;
    out[1] = r.y;
    out[2] = r.width;
    out[3] = r.height;
    return true;
}

// ── Transforms ───────────────────────────────────────────────────────────────

extern "C" void* noesis_translate_transform_create(float x, float y) {
    Noesis::Ptr<Noesis::TranslateTransform> t = *new Noesis::TranslateTransform(x, y);
    return handout(t.GetPtr());
}

extern "C" bool noesis_translate_transform_set(void* transform, float x, float y) {
    auto* t = cast<Noesis::TranslateTransform>(transform);
    if (!t) return false;
    t->SetX(x);
    t->SetY(y);
    return true;
}

extern "C" bool noesis_translate_transform_get(void* transform, float* x, float* y) {
    auto* t = cast<Noesis::TranslateTransform>(transform);
    if (!t || !x || !y) return false;
    *x = t->GetX();
    *y = t->GetY();
    return true;
}

extern "C" void* noesis_scale_transform_create(float sx, float sy, float cx, float cy) {
    Noesis::Ptr<Noesis::ScaleTransform> t = *new Noesis::ScaleTransform(sx, sy);
    t->SetCenterX(cx);
    t->SetCenterY(cy);
    return handout(t.GetPtr());
}

extern "C" bool noesis_scale_transform_set(void* transform, float sx, float sy, float cx,
                                              float cy) {
    auto* t = cast<Noesis::ScaleTransform>(transform);
    if (!t) return false;
    t->SetScaleX(sx);
    t->SetScaleY(sy);
    t->SetCenterX(cx);
    t->SetCenterY(cy);
    return true;
}

// out = [scaleX, scaleY, centerX, centerY]
extern "C" bool noesis_scale_transform_get(void* transform, float out[4]) {
    auto* t = cast<Noesis::ScaleTransform>(transform);
    if (!t || !out) return false;
    out[0] = t->GetScaleX();
    out[1] = t->GetScaleY();
    out[2] = t->GetCenterX();
    out[3] = t->GetCenterY();
    return true;
}

extern "C" void* noesis_rotate_transform_create(float angle, float cx, float cy) {
    Noesis::Ptr<Noesis::RotateTransform> t = *new Noesis::RotateTransform(angle);
    t->SetCenterX(cx);
    t->SetCenterY(cy);
    return handout(t.GetPtr());
}

extern "C" bool noesis_rotate_transform_set_angle(void* transform, float angle) {
    auto* t = cast<Noesis::RotateTransform>(transform);
    if (!t) return false;
    t->SetAngle(angle);
    return true;
}

// out = [angle, centerX, centerY]
extern "C" bool noesis_rotate_transform_get(void* transform, float out[3]) {
    auto* t = cast<Noesis::RotateTransform>(transform);
    if (!t || !out) return false;
    out[0] = t->GetAngle();
    out[1] = t->GetCenterX();
    out[2] = t->GetCenterY();
    return true;
}

extern "C" void* noesis_skew_transform_create(float ax, float ay, float cx, float cy) {
    Noesis::Ptr<Noesis::SkewTransform> t = *new Noesis::SkewTransform(ax, ay);
    t->SetCenterX(cx);
    t->SetCenterY(cy);
    return handout(t.GetPtr());
}

// out = [angleX, angleY, centerX, centerY]
extern "C" bool noesis_skew_transform_get(void* transform, float out[4]) {
    auto* t = cast<Noesis::SkewTransform>(transform);
    if (!t || !out) return false;
    out[0] = t->GetAngleX();
    out[1] = t->GetAngleY();
    out[2] = t->GetCenterX();
    out[3] = t->GetCenterY();
    return true;
}

// matrix = [m00, m01, m10, m11, m20, m21] (Transform2 row-major layout).
extern "C" void* noesis_matrix_transform_create(const float matrix[6]) {
    Noesis::Transform2 m = matrix ? Noesis::Transform2(matrix) : Noesis::Transform2();
    Noesis::Ptr<Noesis::MatrixTransform> t = *new Noesis::MatrixTransform(m);
    return handout(t.GetPtr());
}

extern "C" bool noesis_matrix_transform_set(void* transform, const float matrix[6]) {
    auto* t = cast<Noesis::MatrixTransform>(transform);
    if (!t || !matrix) return false;
    t->SetMatrix(Noesis::Transform2(matrix));
    return true;
}

extern "C" bool noesis_matrix_transform_get(void* transform, float out[6]) {
    auto* t = cast<Noesis::MatrixTransform>(transform);
    if (!t || !out) return false;
    const Noesis::Transform2& m = t->GetMatrix();
    const float* data = m.GetData();
    for (int i = 0; i < 6; ++i) out[i] = data[i];
    return true;
}

extern "C" void* noesis_transform_group_create() {
    Noesis::Ptr<Noesis::TransformGroup> g = *new Noesis::TransformGroup();
    // Ensure a children collection exists so add_child never has to create one.
    if (!g->GetChildren()) {
        Noesis::Ptr<Noesis::TransformCollection> children = *new Noesis::TransformCollection();
        g->SetChildren(children.GetPtr());
    }
    return handout(g.GetPtr());
}

// Append a child transform to a TransformGroup. The group's collection takes its
// own reference; the caller keeps ownership of `child`. Returns false if `group`
// is not a TransformGroup or `child` is not a Transform.
extern "C" bool noesis_transform_group_add_child(void* group, void* child) {
    auto* g = cast<Noesis::TransformGroup>(group);
    auto* c = cast<Noesis::Transform>(child);
    if (!g || !c) return false;
    Noesis::TransformCollection* children = g->GetChildren();
    if (!children) {
        Noesis::Ptr<Noesis::TransformCollection> created = *new Noesis::TransformCollection();
        g->SetChildren(created.GetPtr());
        children = created.GetPtr();
    }
    children->Add(c);
    return true;
}

extern "C" int32_t noesis_transform_group_child_count(void* group) {
    auto* g = cast<Noesis::TransformGroup>(group);
    if (!g) return -1;
    return static_cast<int32_t>(g->GetNumChildren());
}

// fields = [centerX, centerY, scaleX, scaleY, skewX, skewY, rotation,
//           translateX, translateY]
extern "C" void* noesis_composite_transform_create(const float fields[9]) {
    Noesis::Ptr<Noesis::CompositeTransform> t = *new Noesis::CompositeTransform();
    if (fields) {
        t->SetCenterX(fields[0]);
        t->SetCenterY(fields[1]);
        t->SetScaleX(fields[2]);
        t->SetScaleY(fields[3]);
        t->SetSkewX(fields[4]);
        t->SetSkewY(fields[5]);
        t->SetRotation(fields[6]);
        t->SetTranslateX(fields[7]);
        t->SetTranslateY(fields[8]);
    }
    return handout(t.GetPtr());
}

extern "C" bool noesis_composite_transform_get(void* transform, float out[9]) {
    auto* t = cast<Noesis::CompositeTransform>(transform);
    if (!t || !out) return false;
    out[0] = t->GetCenterX();
    out[1] = t->GetCenterY();
    out[2] = t->GetScaleX();
    out[3] = t->GetScaleY();
    out[4] = t->GetSkewX();
    out[5] = t->GetSkewY();
    out[6] = t->GetRotation();
    out[7] = t->GetTranslateX();
    out[8] = t->GetTranslateY();
    return true;
}

// ── 3D transforms ──────────────────────────────────────────────────────────
//
// Transform3D objects are assigned to an element via UIElement::SetTransform3D
// (the Transform3DProperty), NOT via RenderTransform. See the element accessors
// noesis_element_set_transform3d / _get_transform3d below.

// fields = {centerX, centerY, centerZ, rotationX, rotationY, rotationZ,
//           scaleX, scaleY, scaleZ, translateX, translateY, translateZ}
extern "C" void* noesis_composite_transform3d_create(const float fields[12]) {
    Noesis::Ptr<Noesis::CompositeTransform3D> t = *new Noesis::CompositeTransform3D();
    if (fields) {
        t->SetCenterX(fields[0]);
        t->SetCenterY(fields[1]);
        t->SetCenterZ(fields[2]);
        t->SetRotationX(fields[3]);
        t->SetRotationY(fields[4]);
        t->SetRotationZ(fields[5]);
        t->SetScaleX(fields[6]);
        t->SetScaleY(fields[7]);
        t->SetScaleZ(fields[8]);
        t->SetTranslateX(fields[9]);
        t->SetTranslateY(fields[10]);
        t->SetTranslateZ(fields[11]);
    }
    return handout(t.GetPtr());
}

extern "C" bool noesis_composite_transform3d_set(void* transform, const float fields[12]) {
    auto* t = cast<Noesis::CompositeTransform3D>(transform);
    if (!t || !fields) return false;
    t->SetCenterX(fields[0]);
    t->SetCenterY(fields[1]);
    t->SetCenterZ(fields[2]);
    t->SetRotationX(fields[3]);
    t->SetRotationY(fields[4]);
    t->SetRotationZ(fields[5]);
    t->SetScaleX(fields[6]);
    t->SetScaleY(fields[7]);
    t->SetScaleZ(fields[8]);
    t->SetTranslateX(fields[9]);
    t->SetTranslateY(fields[10]);
    t->SetTranslateZ(fields[11]);
    return true;
}

extern "C" bool noesis_composite_transform3d_get(void* transform, float out[12]) {
    auto* t = cast<Noesis::CompositeTransform3D>(transform);
    if (!t || !out) return false;
    out[0] = t->GetCenterX();
    out[1] = t->GetCenterY();
    out[2] = t->GetCenterZ();
    out[3] = t->GetRotationX();
    out[4] = t->GetRotationY();
    out[5] = t->GetRotationZ();
    out[6] = t->GetScaleX();
    out[7] = t->GetScaleY();
    out[8] = t->GetScaleZ();
    out[9] = t->GetTranslateX();
    out[10] = t->GetTranslateY();
    out[11] = t->GetTranslateZ();
    return true;
}

// matrix = 12 floats = Noesis::Transform3 (4 rows of Vector3, row-major).
extern "C" void* noesis_matrix_transform3d_create(const float matrix[12]) {
    Noesis::Transform3 m = matrix ? Noesis::Transform3(matrix) : Noesis::Transform3::Identity();
    Noesis::Ptr<Noesis::MatrixTransform3D> t = *new Noesis::MatrixTransform3D(m);
    return handout(t.GetPtr());
}

extern "C" bool noesis_matrix_transform3d_set(void* transform, const float matrix[12]) {
    auto* t = cast<Noesis::MatrixTransform3D>(transform);
    if (!t || !matrix) return false;
    t->SetMatrix(Noesis::Transform3(matrix));
    return true;
}

extern "C" bool noesis_matrix_transform3d_get(void* transform, float out[12]) {
    auto* t = cast<Noesis::MatrixTransform3D>(transform);
    if (!t || !out) return false;
    const Noesis::Transform3& m = t->GetMatrix();
    const float* data = m.GetData();
    for (int i = 0; i < 12; ++i) out[i] = data[i];
    return true;
}

// Assign a Transform3D to an element (UIElement::SetTransform3D). `transform` is
// a borrowed Transform3D* (or null to clear); Noesis takes its own reference.
// Returns false if `element` is not a UIElement or `transform` is non-null but
// not a Transform3D.
extern "C" bool noesis_element_set_transform3d(void* element, void* transform) {
    auto* e = cast<Noesis::UIElement>(element);
    if (!e) return false;
    if (transform) {
        auto* t = cast<Noesis::Transform3D>(transform);
        if (!t) return false;
        e->SetTransform3D(t);
    } else {
        e->SetTransform3D(nullptr);
    }
    return true;
}

// Borrowed (no +1) Transform3D currently set on the element, or null.
extern "C" void* noesis_element_get_transform3d(void* element) {
    auto* e = cast<Noesis::UIElement>(element);
    if (!e) return nullptr;
    return e->GetTransform3D();
}

// ── Effects ──────────────────────────────────────────────────────────────────

extern "C" void* noesis_blur_effect_create(float radius) {
    Noesis::Ptr<Noesis::BlurEffect> e = *new Noesis::BlurEffect();
    e->SetRadius(radius);
    return handout(e.GetPtr());
}

extern "C" bool noesis_blur_effect_set_radius(void* effect, float radius) {
    auto* e = cast<Noesis::BlurEffect>(effect);
    if (!e) return false;
    e->SetRadius(radius);
    return true;
}

extern "C" bool noesis_blur_effect_get_radius(void* effect, float* out) {
    auto* e = cast<Noesis::BlurEffect>(effect);
    if (!e || !out) return false;
    *out = e->GetRadius();
    return true;
}

extern "C" void* noesis_drop_shadow_effect_create(const float color[4], float blur_radius,
                                                     float direction, float shadow_depth,
                                                     float opacity) {
    Noesis::Ptr<Noesis::DropShadowEffect> e = *new Noesis::DropShadowEffect();
    if (color) e->SetColor(Noesis::Color(color[0], color[1], color[2], color[3]));
    e->SetBlurRadius(blur_radius);
    e->SetDirection(direction);
    e->SetShadowDepth(shadow_depth);
    e->SetOpacity(opacity);
    return handout(e.GetPtr());
}

// out_color = [r,g,b,a]; any out pointer may be null to skip that field.
extern "C" bool noesis_drop_shadow_effect_get(void* effect, float out_color[4], float* out_blur,
                                                 float* out_direction, float* out_shadow_depth,
                                                 float* out_opacity) {
    auto* e = cast<Noesis::DropShadowEffect>(effect);
    if (!e) return false;
    if (out_color) {
        const Noesis::Color& c = e->GetColor();
        out_color[0] = c.r;
        out_color[1] = c.g;
        out_color[2] = c.b;
        out_color[3] = c.a;
    }
    if (out_blur) *out_blur = e->GetBlurRadius();
    if (out_direction) *out_direction = e->GetDirection();
    if (out_shadow_depth) *out_shadow_depth = e->GetShadowDepth();
    if (out_opacity) *out_opacity = e->GetOpacity();
    return true;
}

extern "C" bool noesis_drop_shadow_effect_set_color(void* effect, const float color[4]) {
    auto* e = cast<Noesis::DropShadowEffect>(effect);
    if (!e || !color) return false;
    e->SetColor(Noesis::Color(color[0], color[1], color[2], color[3]));
    return true;
}

extern "C" bool noesis_drop_shadow_effect_set_blur_radius(void* effect, float blur_radius) {
    auto* e = cast<Noesis::DropShadowEffect>(effect);
    if (!e) return false;
    e->SetBlurRadius(blur_radius);
    return true;
}

extern "C" bool noesis_drop_shadow_effect_set_direction(void* effect, float direction) {
    auto* e = cast<Noesis::DropShadowEffect>(effect);
    if (!e) return false;
    e->SetDirection(direction);
    return true;
}

extern "C" bool noesis_drop_shadow_effect_set_shadow_depth(void* effect, float shadow_depth) {
    auto* e = cast<Noesis::DropShadowEffect>(effect);
    if (!e) return false;
    e->SetShadowDepth(shadow_depth);
    return true;
}

extern "C" bool noesis_drop_shadow_effect_set_opacity(void* effect, float opacity) {
    auto* e = cast<Noesis::DropShadowEffect>(effect);
    if (!e) return false;
    e->SetOpacity(opacity);
    return true;
}

// ── RenderOptions (attached property) ────────────────────────────────────────
//
// RenderOptions.BitmapScalingMode is an attached DP whose value type is the enum
// BitmapScalingMode, so it can't go through the generic Int32 attached-property
// path (whose type check demands TypeOf<int32_t>). These wrap the static
// accessors directly. `mode` ordinals match Noesis::BitmapScalingMode.

extern "C" bool noesis_render_options_set_bitmap_scaling_mode(void* obj, int32_t mode) {
    auto* d = cast<Noesis::DependencyObject>(obj);
    if (!d) return false;
    Noesis::RenderOptions::SetBitmapScalingMode(d, static_cast<Noesis::BitmapScalingMode>(mode));
    return true;
}

// Returns the BitmapScalingMode ordinal, or -1 if `obj` is not a DependencyObject.
extern "C" int32_t noesis_render_options_get_bitmap_scaling_mode(void* obj) {
    auto* d = cast<Noesis::DependencyObject>(obj);
    if (!d) return -1;
    return static_cast<int32_t>(Noesis::RenderOptions::GetBitmapScalingMode(d));
}
