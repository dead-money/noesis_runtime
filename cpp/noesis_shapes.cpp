// Code-built Shape elements (TODO §10): construct Rectangle / Ellipse / Line
// from Rust and set their drawing properties without authoring XAML.
//
// These entrypoints construct shape elements and hand them out across the C ABI
// with a single owned reference, mirroring cpp/noesis_brushes.cpp (handout() +
// `*new T` adopt). The Rust side (src/shapes.rs) wraps each pointer in an
// owning handle whose Drop calls dm_noesis_base_component_release; assigning the
// shape into an element tree (or assigning a brush into the shape) makes Noesis
// take its own reference, so the Rust builder handle can be dropped afterwards.
//
// Fill/Stroke reuse the existing brush wrappers: the setters accept any Brush*
// (a BaseComponent*) and the getters return the live Brush* (borrowed, no +1) so
// a test can prove the brush crossed into the Noesis object by pointer identity.
//
// Read-back getters (GetRadiusX / GetStrokeThickness / GetX1 / …) re-read from
// the live Noesis object so tests prove a value actually crossed the FFI rather
// than echoing a Rust-side cache: a stubbed setter fails the round-trip.

#include "noesis_shim.h"

#include <NsCore/BaseComponent.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsGui/Brush.h>
#include <NsGui/Ellipse.h>
#include <NsGui/Enums.h>  // PenLineCap, PenLineJoin, Stretch
#include <NsGui/Line.h>
#include <NsGui/Rectangle.h>
#include <NsGui/Shape.h>

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

// ── Shape element constructors ───────────────────────────────────────────────

extern "C" void* dm_noesis_rectangle_create(void) {
    Noesis::Ptr<Noesis::Rectangle> r = *new Noesis::Rectangle();
    return handout(r.GetPtr());
}

extern "C" void* dm_noesis_ellipse_create(void) {
    Noesis::Ptr<Noesis::Ellipse> e = *new Noesis::Ellipse();
    return handout(e.GetPtr());
}

extern "C" void* dm_noesis_line_create(void) {
    Noesis::Ptr<Noesis::Line> l = *new Noesis::Line();
    return handout(l.GetPtr());
}

// ── FrameworkElement Width/Height (Shape derives from FrameworkElement; the
// shape's own size comes from these inherited DPs, not from the Shape class) ──

extern "C" bool dm_noesis_shape_set_width(void* shape, float width) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return false;
    s->SetWidth(width);
    return true;
}

extern "C" bool dm_noesis_shape_get_width(void* shape, float* out) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s || !out) return false;
    *out = s->GetWidth();
    return true;
}

extern "C" bool dm_noesis_shape_set_height(void* shape, float height) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return false;
    s->SetHeight(height);
    return true;
}

extern "C" bool dm_noesis_shape_get_height(void* shape, float* out) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s || !out) return false;
    *out = s->GetHeight();
    return true;
}

// ── Shape::Fill / Shape::Stroke (reuse brush wrappers) ───────────────────────

extern "C" bool dm_noesis_shape_set_fill(void* shape, void* brush) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return false;
    s->SetFill(cast<Noesis::Brush>(brush));  // null clears the fill
    return true;
}

extern "C" void* dm_noesis_shape_get_fill(void* shape) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return nullptr;
    return s->GetFill();  // borrowed, no +1
}

extern "C" bool dm_noesis_shape_set_stroke(void* shape, void* brush) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return false;
    s->SetStroke(cast<Noesis::Brush>(brush));  // null clears the stroke
    return true;
}

extern "C" void* dm_noesis_shape_get_stroke(void* shape) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return nullptr;
    return s->GetStroke();  // borrowed, no +1
}

// ── Shape stroke scalar properties ───────────────────────────────────────────

extern "C" bool dm_noesis_shape_set_stroke_thickness(void* shape, float value) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return false;
    s->SetStrokeThickness(value);
    return true;
}

extern "C" bool dm_noesis_shape_get_stroke_thickness(void* shape, float* out) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s || !out) return false;
    *out = s->GetStrokeThickness();
    return true;
}

extern "C" bool dm_noesis_shape_set_stroke_miter_limit(void* shape, float value) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return false;
    s->SetStrokeMiterLimit(value);
    return true;
}

extern "C" bool dm_noesis_shape_get_stroke_miter_limit(void* shape, float* out) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s || !out) return false;
    *out = s->GetStrokeMiterLimit();
    return true;
}

extern "C" bool dm_noesis_shape_set_stroke_dash_offset(void* shape, float value) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return false;
    s->SetStrokeDashOffset(value);
    return true;
}

extern "C" bool dm_noesis_shape_get_stroke_dash_offset(void* shape, float* out) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s || !out) return false;
    *out = s->GetStrokeDashOffset();
    return true;
}

extern "C" bool dm_noesis_shape_set_trim_start(void* shape, float value) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return false;
    s->SetTrimStart(value);
    return true;
}

extern "C" bool dm_noesis_shape_get_trim_start(void* shape, float* out) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s || !out) return false;
    *out = s->GetTrimStart();
    return true;
}

extern "C" bool dm_noesis_shape_set_trim_end(void* shape, float value) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return false;
    s->SetTrimEnd(value);
    return true;
}

extern "C" bool dm_noesis_shape_get_trim_end(void* shape, float* out) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s || !out) return false;
    *out = s->GetTrimEnd();
    return true;
}

extern "C" bool dm_noesis_shape_set_trim_offset(void* shape, float value) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return false;
    s->SetTrimOffset(value);
    return true;
}

extern "C" bool dm_noesis_shape_get_trim_offset(void* shape, float* out) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s || !out) return false;
    *out = s->GetTrimOffset();
    return true;
}

// ── Shape stroke enum properties (ordinals match the Noesis enums) ───────────

extern "C" bool dm_noesis_shape_set_stroke_dash_cap(void* shape, int32_t value) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return false;
    s->SetStrokeDashCap(static_cast<Noesis::PenLineCap>(value));
    return true;
}

extern "C" int32_t dm_noesis_shape_get_stroke_dash_cap(void* shape) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return -1;
    return static_cast<int32_t>(s->GetStrokeDashCap());
}

extern "C" bool dm_noesis_shape_set_stroke_start_line_cap(void* shape, int32_t value) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return false;
    s->SetStrokeStartLineCap(static_cast<Noesis::PenLineCap>(value));
    return true;
}

extern "C" int32_t dm_noesis_shape_get_stroke_start_line_cap(void* shape) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return -1;
    return static_cast<int32_t>(s->GetStrokeStartLineCap());
}

extern "C" bool dm_noesis_shape_set_stroke_end_line_cap(void* shape, int32_t value) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return false;
    s->SetStrokeEndLineCap(static_cast<Noesis::PenLineCap>(value));
    return true;
}

extern "C" int32_t dm_noesis_shape_get_stroke_end_line_cap(void* shape) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return -1;
    return static_cast<int32_t>(s->GetStrokeEndLineCap());
}

extern "C" bool dm_noesis_shape_set_stroke_line_join(void* shape, int32_t value) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return false;
    s->SetStrokeLineJoin(static_cast<Noesis::PenLineJoin>(value));
    return true;
}

extern "C" int32_t dm_noesis_shape_get_stroke_line_join(void* shape) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return -1;
    return static_cast<int32_t>(s->GetStrokeLineJoin());
}

extern "C" bool dm_noesis_shape_set_stretch(void* shape, int32_t value) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return false;
    s->SetStretch(static_cast<Noesis::Stretch>(value));
    return true;
}

extern "C" int32_t dm_noesis_shape_get_stretch(void* shape) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return -1;
    return static_cast<int32_t>(s->GetStretch());
}

// ── Shape::StrokeDashArray (exposed by Noesis as a string) ───────────────────

extern "C" bool dm_noesis_shape_set_stroke_dash_array(void* shape, const char* dashes) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return false;
    s->SetStrokeDashArray(dashes);
    return true;
}

// Returns a borrowed pointer owned by the Noesis Shape; valid until the shape is
// mutated or released. The Rust side copies it immediately into an owned String.
extern "C" const char* dm_noesis_shape_get_stroke_dash_array(void* shape) {
    auto* s = cast<Noesis::Shape>(shape);
    if (!s) return nullptr;
    return s->GetStrokeDashArray();
}

// ── Rectangle::RadiusX / RadiusY ─────────────────────────────────────────────

extern "C" bool dm_noesis_rectangle_set_radius_x(void* shape, float value) {
    auto* r = cast<Noesis::Rectangle>(shape);
    if (!r) return false;
    r->SetRadiusX(value);
    return true;
}

extern "C" bool dm_noesis_rectangle_get_radius_x(void* shape, float* out) {
    auto* r = cast<Noesis::Rectangle>(shape);
    if (!r || !out) return false;
    *out = r->GetRadiusX();
    return true;
}

extern "C" bool dm_noesis_rectangle_set_radius_y(void* shape, float value) {
    auto* r = cast<Noesis::Rectangle>(shape);
    if (!r) return false;
    r->SetRadiusY(value);
    return true;
}

extern "C" bool dm_noesis_rectangle_get_radius_y(void* shape, float* out) {
    auto* r = cast<Noesis::Rectangle>(shape);
    if (!r || !out) return false;
    *out = r->GetRadiusY();
    return true;
}

// ── Line::X1/Y1/X2/Y2 (set/get all four at once) ─────────────────────────────

extern "C" bool dm_noesis_line_set(void* shape, float x1, float y1, float x2, float y2) {
    auto* l = cast<Noesis::Line>(shape);
    if (!l) return false;
    l->SetX1(x1);
    l->SetY1(y1);
    l->SetX2(x2);
    l->SetY2(y2);
    return true;
}

// out = {x1, y1, x2, y2}
extern "C" bool dm_noesis_line_get(void* shape, float out[4]) {
    auto* l = cast<Noesis::Line>(shape);
    if (!l || !out) return false;
    out[0] = l->GetX1();
    out[1] = l->GetY1();
    out[2] = l->GetX2();
    out[3] = l->GetY2();
    return true;
}
