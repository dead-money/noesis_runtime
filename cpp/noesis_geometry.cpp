// Code-built Geometry object model (TODO §10).
//
// These entrypoints construct Geometry objects (and their PathFigure / PathSegment
// building blocks) from Rust and hand them out across the C ABI with a single
// owned reference, mirroring the ownership idioms already used by
// cpp/noesis_brushes.cpp (handout() + `*new T` adopt) and cpp/noesis_collections.cpp.
// The Rust side (src/geometry.rs) wraps each pointer in an owning handle whose
// Drop calls noesis_base_component_release; assigning a finished geometry to a
// Path's Data (via the generic FrameworkElement::set_component path) makes Noesis
// take its own reference, so the Rust builder handle can be dropped afterwards.
//
// Read-back getters re-read from the LIVE Noesis object — GetBounds() /
// GetRenderBounds() prove a real path was built (a no-op constructor yields empty
// bounds), figure/segment/child counts prove collection wiring crossed the FFI,
// and CombinedGeometry mode / FillRule prove enum round-trips. A stubbed
// implementation fails the tests in tests/geometry.rs.

#include "noesis_shim.h"

#include <vector>

#include <NsCore/BaseComponent.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsDrawing/Point.h>
#include <NsDrawing/Rect.h>
#include <NsDrawing/Size.h>
#include <NsGui/ArcSegment.h>
#include <NsGui/BezierSegment.h>
#include <NsGui/BoxedFreezableCollection.h>
#include <NsGui/CombinedGeometry.h>
#include <NsGui/EllipseGeometry.h>
#include <NsGui/Enums.h>
#include <NsGui/FreezableCollection.h>
#include <NsGui/Geometry.h>
#include <NsGui/GeometryGroup.h>
#include <NsGui/LineGeometry.h>
#include <NsGui/LineSegment.h>
#include <NsGui/PathFigure.h>
#include <NsGui/PathGeometry.h>
#include <NsGui/PathSegment.h>
#include <NsGui/PolyBezierSegment.h>
#include <NsGui/PolyLineSegment.h>
#include <NsGui/PolyQuadraticBezierSegment.h>
#include <NsGui/QuadraticBezierSegment.h>
#include <NsGui/RectangleGeometry.h>
#include <NsGui/StreamGeometry.h>
#include <NsGui/StreamGeometryContext.h>
#include <NsGui/Transform.h>

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

void store_rect(const Noesis::Rect& r, float out[4]) {
    out[0] = r.x;
    out[1] = r.y;
    out[2] = r.width;
    out[3] = r.height;
}

}  // namespace

// ── Geometry base ────────────────────────────────────────────────────────────

extern "C" bool noesis_geometry_get_bounds(void* geometry, float out[4]) {
    auto* g = cast<Noesis::Geometry>(geometry);
    if (!g || !out) return false;
    store_rect(g->GetBounds(), out);
    return true;
}

extern "C" bool noesis_geometry_get_render_bounds(void* geometry, float out[4]) {
    auto* g = cast<Noesis::Geometry>(geometry);
    if (!g || !out) return false;
    store_rect(g->GetRenderBounds(nullptr), out);
    return true;
}

extern "C" int32_t noesis_geometry_is_empty(void* geometry) {
    auto* g = cast<Noesis::Geometry>(geometry);
    if (!g) return -1;
    return g->IsEmpty() ? 1 : 0;
}

extern "C" bool noesis_geometry_set_transform(void* geometry, void* transform) {
    auto* g = cast<Noesis::Geometry>(geometry);
    if (!g) return false;
    g->SetTransform(cast<Noesis::Transform>(transform));
    return true;
}

extern "C" void* noesis_geometry_get_transform(void* geometry) {
    auto* g = cast<Noesis::Geometry>(geometry);
    if (!g) return nullptr;
    return g->GetTransform();
}

// ── StreamGeometry + StreamGeometryContext ───────────────────────────────────

extern "C" void* noesis_stream_geometry_create(void) {
    Noesis::Ptr<Noesis::StreamGeometry> g = *new Noesis::StreamGeometry();
    return handout(g.GetPtr());
}

extern "C" void* noesis_stream_geometry_create_from_data(const char* data) {
    Noesis::Ptr<Noesis::StreamGeometry> g =
        data ? *new Noesis::StreamGeometry(data) : *new Noesis::StreamGeometry();
    return handout(g.GetPtr());
}

extern "C" bool noesis_stream_geometry_set_data(void* geometry, const char* data) {
    auto* g = cast<Noesis::StreamGeometry>(geometry);
    if (!g || !data) return false;
    g->SetData(data);
    return true;
}

extern "C" bool noesis_stream_geometry_set_fill_rule(void* geometry, int32_t rule) {
    auto* g = cast<Noesis::StreamGeometry>(geometry);
    if (!g) return false;
    g->SetFillRule(static_cast<Noesis::FillRule>(rule));
    return true;
}

extern "C" int32_t noesis_stream_geometry_get_fill_rule(void* geometry) {
    auto* g = cast<Noesis::StreamGeometry>(geometry);
    if (!g) return -1;
    return static_cast<int32_t>(g->GetFillRule());
}

// Open() returns a StreamGeometryContext by value that keeps a Ptr to the
// geometry alive; copy it onto the heap so the Rust handle can drive it across
// the ABI, then flush (Close) or free (destroy) it.
extern "C" void* noesis_stream_geometry_open(void* geometry) {
    auto* g = cast<Noesis::StreamGeometry>(geometry);
    if (!g) return nullptr;
    return new Noesis::StreamGeometryContext(g->Open());
}

extern "C" bool noesis_stream_geometry_context_begin_figure(void* ctx, float x, float y,
                                                               bool is_closed) {
    if (!ctx) return false;
    static_cast<Noesis::StreamGeometryContext*>(ctx)->BeginFigure(Noesis::Point(x, y), is_closed);
    return true;
}

extern "C" bool noesis_stream_geometry_context_line_to(void* ctx, float x, float y) {
    if (!ctx) return false;
    static_cast<Noesis::StreamGeometryContext*>(ctx)->LineTo(Noesis::Point(x, y));
    return true;
}

extern "C" bool noesis_stream_geometry_context_cubic_to(void* ctx, float x1, float y1, float x2,
                                                           float y2, float x3, float y3) {
    if (!ctx) return false;
    static_cast<Noesis::StreamGeometryContext*>(ctx)->CubicTo(
        Noesis::Point(x1, y1), Noesis::Point(x2, y2), Noesis::Point(x3, y3));
    return true;
}

extern "C" bool noesis_stream_geometry_context_quadratic_to(void* ctx, float x1, float y1,
                                                               float x2, float y2) {
    if (!ctx) return false;
    static_cast<Noesis::StreamGeometryContext*>(ctx)->QuadraticTo(Noesis::Point(x1, y1),
                                                                  Noesis::Point(x2, y2));
    return true;
}

extern "C" bool noesis_stream_geometry_context_arc_to(void* ctx, float x, float y, float width,
                                                         float height, float rotation_deg,
                                                         bool is_large_arc,
                                                         int32_t sweep_direction) {
    if (!ctx) return false;
    static_cast<Noesis::StreamGeometryContext*>(ctx)->ArcTo(
        Noesis::Point(x, y), Noesis::Size(width, height), rotation_deg, is_large_arc,
        static_cast<Noesis::SweepDirection>(sweep_direction));
    return true;
}

extern "C" bool noesis_stream_geometry_context_set_is_closed(void* ctx, bool is_closed) {
    if (!ctx) return false;
    static_cast<Noesis::StreamGeometryContext*>(ctx)->SetIsClosed(is_closed);
    return true;
}

extern "C" bool noesis_stream_geometry_context_close(void* ctx) {
    if (!ctx) return false;
    auto* c = static_cast<Noesis::StreamGeometryContext*>(ctx);
    c->Close();
    delete c;
    return true;
}

extern "C" void noesis_stream_geometry_context_destroy(void* ctx) {
    delete static_cast<Noesis::StreamGeometryContext*>(ctx);
}

// ── PathGeometry + PathFigure ────────────────────────────────────────────────

extern "C" void* noesis_path_geometry_create(void) {
    Noesis::Ptr<Noesis::PathGeometry> g = *new Noesis::PathGeometry();
    if (!g->GetFigures()) {
        Noesis::Ptr<Noesis::PathFigureCollection> figures = *new Noesis::PathFigureCollection();
        g->SetFigures(figures.GetPtr());
    }
    return handout(g.GetPtr());
}

extern "C" bool noesis_path_geometry_set_fill_rule(void* geometry, int32_t rule) {
    auto* g = cast<Noesis::PathGeometry>(geometry);
    if (!g) return false;
    g->SetFillRule(static_cast<Noesis::FillRule>(rule));
    return true;
}

extern "C" int32_t noesis_path_geometry_get_fill_rule(void* geometry) {
    auto* g = cast<Noesis::PathGeometry>(geometry);
    if (!g) return -1;
    return static_cast<int32_t>(g->GetFillRule());
}

extern "C" int32_t noesis_path_geometry_add_figure(void* geometry, void* figure) {
    auto* g = cast<Noesis::PathGeometry>(geometry);
    auto* f = cast<Noesis::PathFigure>(figure);
    if (!g || !f) return -1;
    Noesis::PathFigureCollection* figures = g->GetFigures();
    if (!figures) {
        Noesis::Ptr<Noesis::PathFigureCollection> created = *new Noesis::PathFigureCollection();
        g->SetFigures(created.GetPtr());
        figures = created.GetPtr();
    }
    return figures->Add(f);
}

extern "C" int32_t noesis_path_geometry_figure_count(void* geometry) {
    auto* g = cast<Noesis::PathGeometry>(geometry);
    if (!g) return -1;
    Noesis::PathFigureCollection* figures = g->GetFigures();
    return figures ? figures->Count() : 0;
}

extern "C" void* noesis_path_figure_create(void) {
    Noesis::Ptr<Noesis::PathFigure> f = *new Noesis::PathFigure();
    if (!f->GetSegments()) {
        Noesis::Ptr<Noesis::PathSegmentCollection> segments = *new Noesis::PathSegmentCollection();
        f->SetSegments(segments.GetPtr());
    }
    return handout(f.GetPtr());
}

extern "C" bool noesis_path_figure_set_start_point(void* figure, float x, float y) {
    auto* f = cast<Noesis::PathFigure>(figure);
    if (!f) return false;
    f->SetStartPoint(Noesis::Point(x, y));
    return true;
}

extern "C" bool noesis_path_figure_get_start_point(void* figure, float out[2]) {
    auto* f = cast<Noesis::PathFigure>(figure);
    if (!f || !out) return false;
    const Noesis::Point& p = f->GetStartPoint();
    out[0] = p.x;
    out[1] = p.y;
    return true;
}

extern "C" bool noesis_path_figure_set_is_closed(void* figure, bool is_closed) {
    auto* f = cast<Noesis::PathFigure>(figure);
    if (!f) return false;
    f->SetIsClosed(is_closed);
    return true;
}

extern "C" bool noesis_path_figure_set_is_filled(void* figure, bool is_filled) {
    auto* f = cast<Noesis::PathFigure>(figure);
    if (!f) return false;
    f->SetIsFilled(is_filled);
    return true;
}

extern "C" int32_t noesis_path_figure_get_is_closed(void* figure) {
    auto* f = cast<Noesis::PathFigure>(figure);
    if (!f) return -1;
    return f->GetIsClosed() ? 1 : 0;
}

extern "C" int32_t noesis_path_figure_get_is_filled(void* figure) {
    auto* f = cast<Noesis::PathFigure>(figure);
    if (!f) return -1;
    return f->GetIsFilled() ? 1 : 0;
}

extern "C" int32_t noesis_path_figure_add_segment(void* figure, void* segment) {
    auto* f = cast<Noesis::PathFigure>(figure);
    auto* s = cast<Noesis::PathSegment>(segment);
    if (!f || !s) return -1;
    Noesis::PathSegmentCollection* segments = f->GetSegments();
    if (!segments) {
        Noesis::Ptr<Noesis::PathSegmentCollection> created = *new Noesis::PathSegmentCollection();
        f->SetSegments(created.GetPtr());
        segments = created.GetPtr();
    }
    return segments->Add(s);
}

extern "C" int32_t noesis_path_figure_segment_count(void* figure) {
    auto* f = cast<Noesis::PathFigure>(figure);
    if (!f) return -1;
    Noesis::PathSegmentCollection* segments = f->GetSegments();
    return segments ? segments->Count() : 0;
}

// ── Path segments ────────────────────────────────────────────────────────────

extern "C" void* noesis_line_segment_create(float x, float y) {
    Noesis::Ptr<Noesis::LineSegment> s = *new Noesis::LineSegment(Noesis::Point(x, y), true);
    return handout(s.GetPtr());
}

extern "C" bool noesis_line_segment_get_point(void* segment, float out[2]) {
    auto* s = cast<Noesis::LineSegment>(segment);
    if (!s || !out) return false;
    const Noesis::Point& p = s->GetPoint();
    out[0] = p.x;
    out[1] = p.y;
    return true;
}

extern "C" void* noesis_bezier_segment_create(float x1, float y1, float x2, float y2, float x3,
                                                 float y3) {
    Noesis::Ptr<Noesis::BezierSegment> s = *new Noesis::BezierSegment(
        Noesis::Point(x1, y1), Noesis::Point(x2, y2), Noesis::Point(x3, y3), true);
    return handout(s.GetPtr());
}

extern "C" bool noesis_bezier_segment_get(void* segment, float out[6]) {
    auto* s = cast<Noesis::BezierSegment>(segment);
    if (!s || !out) return false;
    const Noesis::Point& p1 = s->GetPoint1();
    const Noesis::Point& p2 = s->GetPoint2();
    const Noesis::Point& p3 = s->GetPoint3();
    out[0] = p1.x;
    out[1] = p1.y;
    out[2] = p2.x;
    out[3] = p2.y;
    out[4] = p3.x;
    out[5] = p3.y;
    return true;
}

extern "C" void* noesis_quadratic_bezier_segment_create(float x1, float y1, float x2, float y2) {
    Noesis::Ptr<Noesis::QuadraticBezierSegment> s =
        *new Noesis::QuadraticBezierSegment(Noesis::Point(x1, y1), Noesis::Point(x2, y2), true);
    return handout(s.GetPtr());
}

extern "C" bool noesis_quadratic_bezier_segment_get(void* segment, float out[4]) {
    auto* s = cast<Noesis::QuadraticBezierSegment>(segment);
    if (!s || !out) return false;
    const Noesis::Point& p1 = s->GetPoint1();
    const Noesis::Point& p2 = s->GetPoint2();
    out[0] = p1.x;
    out[1] = p1.y;
    out[2] = p2.x;
    out[3] = p2.y;
    return true;
}

extern "C" void* noesis_arc_segment_create(float x, float y, float width, float height,
                                              float rotation_deg, bool is_large_arc,
                                              int32_t sweep_direction) {
    Noesis::Ptr<Noesis::ArcSegment> s = *new Noesis::ArcSegment(
        Noesis::Point(x, y), Noesis::Size(width, height), rotation_deg, is_large_arc,
        static_cast<Noesis::SweepDirection>(sweep_direction), true);
    return handout(s.GetPtr());
}

extern "C" bool noesis_arc_segment_get(void* segment, float out_point[2], float out_size[2],
                                          float* out_rotation_deg, bool* out_is_large_arc,
                                          int32_t* out_sweep_direction) {
    auto* s = cast<Noesis::ArcSegment>(segment);
    if (!s) return false;
    if (out_point) {
        const Noesis::Point& p = s->GetPoint();
        out_point[0] = p.x;
        out_point[1] = p.y;
    }
    if (out_size) {
        const Noesis::Size& sz = s->GetSize();
        out_size[0] = sz.width;
        out_size[1] = sz.height;
    }
    if (out_rotation_deg) *out_rotation_deg = s->GetRotationAngle();
    if (out_is_large_arc) *out_is_large_arc = s->GetIsLargeArc();
    if (out_sweep_direction) *out_sweep_direction = static_cast<int32_t>(s->GetSweepDirection());
    return true;
}

namespace {

// Build a Noesis::Point vector from a flat (x, y) float array.
std::vector<Noesis::Point> make_points(const float* points, uint32_t num_points) {
    std::vector<Noesis::Point> pts;
    if (points && num_points) {
        pts.reserve(num_points);
        for (uint32_t i = 0; i < num_points; ++i) {
            pts.emplace_back(points[2 * i], points[2 * i + 1]);
        }
    }
    return pts;
}

// Borrow the PointCollection from any of the three poly segment types, or null.
Noesis::PointCollection* poly_points(void* segment) {
    if (auto* s = cast<Noesis::PolyLineSegment>(segment)) return s->GetPoints();
    if (auto* s = cast<Noesis::PolyBezierSegment>(segment)) return s->GetPoints();
    if (auto* s = cast<Noesis::PolyQuadraticBezierSegment>(segment)) return s->GetPoints();
    return nullptr;
}

}  // namespace

extern "C" void* noesis_poly_line_segment_create(const float* points, uint32_t num_points) {
    std::vector<Noesis::Point> pts = make_points(points, num_points);
    Noesis::Ptr<Noesis::PolyLineSegment> s =
        *new Noesis::PolyLineSegment(pts.data(), static_cast<uint32_t>(pts.size()), true);
    return handout(s.GetPtr());
}

extern "C" void* noesis_poly_bezier_segment_create(const float* points, uint32_t num_points) {
    std::vector<Noesis::Point> pts = make_points(points, num_points);
    Noesis::Ptr<Noesis::PolyBezierSegment> s =
        *new Noesis::PolyBezierSegment(pts.data(), static_cast<uint32_t>(pts.size()), true);
    return handout(s.GetPtr());
}

extern "C" void* noesis_poly_quadratic_bezier_segment_create(const float* points,
                                                                uint32_t num_points) {
    std::vector<Noesis::Point> pts = make_points(points, num_points);
    Noesis::Ptr<Noesis::PolyQuadraticBezierSegment> s = *new Noesis::PolyQuadraticBezierSegment(
        pts.data(), static_cast<uint32_t>(pts.size()), true);
    return handout(s.GetPtr());
}

extern "C" int32_t noesis_poly_segment_point_count(void* segment) {
    Noesis::PointCollection* pts = poly_points(segment);
    if (!pts) return cast<Noesis::PathSegment>(segment) ? 0 : -1;
    return pts->Count();
}

extern "C" bool noesis_poly_segment_get_point(void* segment, uint32_t index, float out[2]) {
    Noesis::PointCollection* pts = poly_points(segment);
    if (!pts || !out || index >= static_cast<uint32_t>(pts->Count())) return false;
    const Noesis::Point& p = pts->Get(index);
    out[0] = p.x;
    out[1] = p.y;
    return true;
}

// ── EllipseGeometry / RectangleGeometry / LineGeometry ───────────────────────

extern "C" void* noesis_ellipse_geometry_create(float cx, float cy, float rx, float ry) {
    Noesis::Ptr<Noesis::EllipseGeometry> g =
        *new Noesis::EllipseGeometry(Noesis::Point(cx, cy), rx, ry);
    return handout(g.GetPtr());
}

extern "C" bool noesis_ellipse_geometry_get(void* geometry, float out[4]) {
    auto* g = cast<Noesis::EllipseGeometry>(geometry);
    if (!g || !out) return false;
    const Noesis::Point& c = g->GetCenter();
    out[0] = c.x;
    out[1] = c.y;
    out[2] = g->GetRadiusX();
    out[3] = g->GetRadiusY();
    return true;
}

extern "C" void* noesis_rectangle_geometry_create(float x, float y, float width, float height,
                                                     float rx, float ry) {
    Noesis::Ptr<Noesis::RectangleGeometry> g = *new Noesis::RectangleGeometry(
        Noesis::Rect(Noesis::Point(x, y), Noesis::Size(width, height)), rx, ry);
    return handout(g.GetPtr());
}

extern "C" bool noesis_rectangle_geometry_get(void* geometry, float out_rect[4],
                                                 float out_radii[2]) {
    auto* g = cast<Noesis::RectangleGeometry>(geometry);
    if (!g) return false;
    if (out_rect) store_rect(g->GetRect(), out_rect);
    if (out_radii) {
        out_radii[0] = g->GetRadiusX();
        out_radii[1] = g->GetRadiusY();
    }
    return true;
}

extern "C" void* noesis_line_geometry_create(float x1, float y1, float x2, float y2) {
    Noesis::Ptr<Noesis::LineGeometry> g =
        *new Noesis::LineGeometry(Noesis::Point(x1, y1), Noesis::Point(x2, y2));
    return handout(g.GetPtr());
}

extern "C" bool noesis_line_geometry_get(void* geometry, float out[4]) {
    auto* g = cast<Noesis::LineGeometry>(geometry);
    if (!g || !out) return false;
    const Noesis::Point& s = g->GetStartPoint();
    const Noesis::Point& e = g->GetEndPoint();
    out[0] = s.x;
    out[1] = s.y;
    out[2] = e.x;
    out[3] = e.y;
    return true;
}

// ── CombinedGeometry ─────────────────────────────────────────────────────────

extern "C" void* noesis_combined_geometry_create(int32_t mode, void* geometry1,
                                                    void* geometry2) {
    Noesis::Ptr<Noesis::CombinedGeometry> g =
        *new Noesis::CombinedGeometry(cast<Noesis::Geometry>(geometry1),
                                      cast<Noesis::Geometry>(geometry2),
                                      static_cast<Noesis::GeometryCombineMode>(mode));
    return handout(g.GetPtr());
}

extern "C" bool noesis_combined_geometry_set_geometry1(void* geometry, void* g1) {
    auto* g = cast<Noesis::CombinedGeometry>(geometry);
    if (!g) return false;
    g->SetGeometry1(cast<Noesis::Geometry>(g1));
    return true;
}

extern "C" bool noesis_combined_geometry_set_geometry2(void* geometry, void* g2) {
    auto* g = cast<Noesis::CombinedGeometry>(geometry);
    if (!g) return false;
    g->SetGeometry2(cast<Noesis::Geometry>(g2));
    return true;
}

extern "C" void* noesis_combined_geometry_get_geometry1(void* geometry) {
    auto* g = cast<Noesis::CombinedGeometry>(geometry);
    if (!g) return nullptr;
    return g->GetGeometry1();
}

extern "C" void* noesis_combined_geometry_get_geometry2(void* geometry) {
    auto* g = cast<Noesis::CombinedGeometry>(geometry);
    if (!g) return nullptr;
    return g->GetGeometry2();
}

extern "C" bool noesis_combined_geometry_set_mode(void* geometry, int32_t mode) {
    auto* g = cast<Noesis::CombinedGeometry>(geometry);
    if (!g) return false;
    g->SetGeometryCombineMode(static_cast<Noesis::GeometryCombineMode>(mode));
    return true;
}

extern "C" int32_t noesis_combined_geometry_get_mode(void* geometry) {
    auto* g = cast<Noesis::CombinedGeometry>(geometry);
    if (!g) return -1;
    return static_cast<int32_t>(g->GetGeometryCombineMode());
}

// ── GeometryGroup ────────────────────────────────────────────────────────────

extern "C" void* noesis_geometry_group_create(void) {
    Noesis::Ptr<Noesis::GeometryGroup> g = *new Noesis::GeometryGroup();
    if (!g->GetChildren()) {
        Noesis::Ptr<Noesis::GeometryCollection> children = *new Noesis::GeometryCollection();
        g->SetChildren(children.GetPtr());
    }
    return handout(g.GetPtr());
}

extern "C" bool noesis_geometry_group_set_fill_rule(void* geometry, int32_t rule) {
    auto* g = cast<Noesis::GeometryGroup>(geometry);
    if (!g) return false;
    g->SetFillRule(static_cast<Noesis::FillRule>(rule));
    return true;
}

extern "C" int32_t noesis_geometry_group_get_fill_rule(void* geometry) {
    auto* g = cast<Noesis::GeometryGroup>(geometry);
    if (!g) return -1;
    return static_cast<int32_t>(g->GetFillRule());
}

extern "C" int32_t noesis_geometry_group_add_child(void* geometry, void* child) {
    auto* g = cast<Noesis::GeometryGroup>(geometry);
    auto* c = cast<Noesis::Geometry>(child);
    if (!g || !c) return -1;
    Noesis::GeometryCollection* children = g->GetChildren();
    if (!children) {
        Noesis::Ptr<Noesis::GeometryCollection> created = *new Noesis::GeometryCollection();
        g->SetChildren(created.GetPtr());
        children = created.GetPtr();
    }
    return children->Add(c);
}

extern "C" int32_t noesis_geometry_group_child_count(void* geometry) {
    auto* g = cast<Noesis::GeometryGroup>(geometry);
    if (!g) return -1;
    Noesis::GeometryCollection* children = g->GetChildren();
    return children ? children->Count() : 0;
}
