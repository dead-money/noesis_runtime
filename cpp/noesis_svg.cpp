// SVG / SVGPath parsing and geometry queries (TODO §12 "SVG").
//
// Two real 3.2.13 surfaces are wrapped here, both fully CPU/headless — no GPU
// RenderDevice or render pass is required to exercise either one:
//
//   * NsDrawing/SVGPath.h — Noesis::SVGPath. We own a heap SVGPath built either
//     by parsing an SVG path string (SVGPath::TryParse) or via the path-builder
//     statics (MoveTo / LineTo / Close / AddRect / …). The path's `commands`
//     member is a Vector<uint32_t> (a BaseVector<uint32_t>) that we feed to the
//     static query API: CalculateBounds(ArrayRef<uint32_t>) -> Rect,
//     FillContains(ArrayRef<uint32_t>, Point, Fill) -> bool, and
//     StrokeContains(ArrayRef<uint32_t>, Point, Pen) -> bool. We expose the
//     command count + the query results to Rust; the raw command ints stay C++
//     side (Rust never needs them).
//
//   * NsGui/SVG.h — Noesis::SVG::Parse(const char* svg, Image& image), the free
//     function that parses a whole <svg> document into a Noesis::SVG::Image (a
//     plain struct: width, height, Vector<Shape> shapes). We own that Image and
//     expose its width/height + shape count + per-shape fill type so a test can
//     prove a document actually parsed.
//
// NEITHER Noesis::SVGPath NOR Noesis::SVG::Image is a BaseComponent, so these
// handles are owned with plain new/delete and released through the dedicated
// *_destroy entrypoints below (NOT dm_noesis_base_component_release).

#include "noesis_shim.h"

#include <NsCore/ArrayRef.h>
#include <NsCore/Vector.h>
#include <NsDrawing/Point.h>
#include <NsDrawing/Rect.h>
#include <NsDrawing/SVGPath.h>
#include <NsGui/SVG.h>
#include <NsMath/Transform.h>

namespace {

Noesis::SVGPath* as_path(void* p) { return static_cast<Noesis::SVGPath*>(p); }
Noesis::SVG::Image* as_image(void* p) { return static_cast<Noesis::SVG::Image*>(p); }

// A borrowed ArrayRef over a path's command buffer, valid for the call.
Noesis::ArrayRef<uint32_t> commands_of(Noesis::SVGPath* path) {
    return Noesis::ArrayRef<uint32_t>(path->commands.Data(), path->commands.Size());
}

}  // namespace

// ── SVGPath: parse / build ───────────────────────────────────────────────────

// Parse an SVG path data string (e.g. "M0 0 L100 0 L100 50 Z") into a freshly
// owned SVGPath. Returns null if the string fails to parse. The returned pointer
// must be released with dm_noesis_svg_path_destroy.
extern "C" void* dm_noesis_svg_path_parse(const char* str) {
    if (!str) return nullptr;
    Noesis::SVGPath* path = new Noesis::SVGPath();
    if (!Noesis::SVGPath::TryParse(str, *path)) {
        delete path;
        return nullptr;
    }
    return path;
}

// Create an empty SVGPath to be populated with the builder entrypoints below.
extern "C" void* dm_noesis_svg_path_create() { return new Noesis::SVGPath(); }

extern "C" void dm_noesis_svg_path_destroy(void* path) { delete as_path(path); }

// Number of uint32 entries in the path's command buffer. A parsed/built path is
// non-empty; this is the cheap "did anything cross the FFI" discriminator.
extern "C" uint32_t dm_noesis_svg_path_command_count(void* path) {
    Noesis::SVGPath* p = as_path(path);
    return p ? p->commands.Size() : 0u;
}

// ── SVGPath builder statics (append to the owned command buffer) ─────────────

extern "C" void dm_noesis_svg_path_move_to(void* path, float x, float y) {
    Noesis::SVGPath* p = as_path(path);
    if (p) Noesis::SVGPath::MoveTo(p->commands, x, y);
}

extern "C" void dm_noesis_svg_path_line_to(void* path, float x, float y) {
    Noesis::SVGPath* p = as_path(path);
    if (p) Noesis::SVGPath::LineTo(p->commands, x, y);
}

extern "C" void dm_noesis_svg_path_close(void* path) {
    Noesis::SVGPath* p = as_path(path);
    if (p) Noesis::SVGPath::Close(p->commands);
}

extern "C" void dm_noesis_svg_path_add_rect(void* path, float x, float y, float width,
                                            float height) {
    Noesis::SVGPath* p = as_path(path);
    if (p) Noesis::SVGPath::AddRect(p->commands, x, y, width, height);
}

extern "C" void dm_noesis_svg_path_add_ellipse(void* path, float x, float y, float rx, float ry) {
    Noesis::SVGPath* p = as_path(path);
    if (p) Noesis::SVGPath::AddEllipse(p->commands, x, y, rx, ry);
}

// ── SVGPath queries (static command-buffer API) ──────────────────────────────

// Axis-aligned bounding box of the path geometry. out = [x, y, width, height].
extern "C" bool dm_noesis_svg_path_calculate_bounds(void* path, float out[4]) {
    Noesis::SVGPath* p = as_path(path);
    if (!p || !out) return false;
    Noesis::Rect r = Noesis::SVGPath::CalculateBounds(commands_of(p));
    out[0] = r.x;
    out[1] = r.y;
    out[2] = r.width;
    out[3] = r.height;
    return true;
}

// True if (x, y) lies inside the filled region. `fill_rule` selects the winding
// rule: 0 = EvenOdd, 1 = NonZero (Noesis::SVGPath::Fill ordinals).
extern "C" bool dm_noesis_svg_path_fill_contains(void* path, float x, float y, int32_t fill_rule) {
    Noesis::SVGPath* p = as_path(path);
    if (!p) return false;
    return Noesis::SVGPath::FillContains(commands_of(p), Noesis::Point(x, y),
                                         static_cast<Noesis::SVGPath::Fill>(fill_rule));
}

// True if (x, y) lies within the stroked outline of the path for the given pen.
// `join` is a Noesis::SVGPath::StrokeJoinStyle ordinal; `start_cap`/`end_cap`
// are Noesis::SVGPath::StrokeCapStyle ordinals.
extern "C" bool dm_noesis_svg_path_stroke_contains(void* path, float x, float y, float width,
                                                   int32_t join, int32_t start_cap, int32_t end_cap,
                                                   float miter_limit) {
    Noesis::SVGPath* p = as_path(path);
    if (!p) return false;
    Noesis::SVGPath::Pen pen;
    pen.width = width;
    pen.join = static_cast<Noesis::SVGPath::StrokeJoinStyle>(join);
    pen.startCap = static_cast<Noesis::SVGPath::StrokeCapStyle>(start_cap);
    pen.endCap = static_cast<Noesis::SVGPath::StrokeCapStyle>(end_cap);
    pen.miterLimit = miter_limit;
    return Noesis::SVGPath::StrokeContains(commands_of(p), Noesis::Point(x, y), pen);
}

// ── SVG document parsing (NsGui/SVG.h free function) ─────────────────────────

// Parse a full <svg> document string into a freshly owned Noesis::SVG::Image.
// Always returns a non-null handle (Parse populates it in place); release with
// dm_noesis_svg_image_destroy. A malformed document yields an image with zero
// shapes, observable via dm_noesis_svg_image_shape_count.
extern "C" void* dm_noesis_svg_image_parse(const char* svg) {
    if (!svg) return nullptr;
    Noesis::SVG::Image* image = new Noesis::SVG::Image();
    Noesis::SVG::Parse(svg, *image);
    return image;
}

extern "C" void dm_noesis_svg_image_destroy(void* image) { delete as_image(image); }

// Parsed document size (the <svg> width/height). Returns false if `image` null.
extern "C" bool dm_noesis_svg_image_get_size(void* image, float* width, float* height) {
    Noesis::SVG::Image* img = as_image(image);
    if (!img) return false;
    if (width) *width = img->width;
    if (height) *height = img->height;
    return true;
}

// Number of parsed shapes (paths) in the document.
extern "C" uint32_t dm_noesis_svg_image_shape_count(void* image) {
    Noesis::SVG::Image* img = as_image(image);
    return img ? img->shapes.Size() : 0u;
}

// Fill-brush type of shape `index` (Noesis::SVG::Brush::Type ordinal: 0 None,
// 1 Solid, 2 Linear, 3 Radial), or -1 if the index is out of range.
extern "C" int32_t dm_noesis_svg_image_shape_fill_type(void* image, uint32_t index) {
    Noesis::SVG::Image* img = as_image(image);
    if (!img || index >= img->shapes.Size()) return -1;
    return static_cast<int32_t>(img->shapes[index].fill.type);
}
