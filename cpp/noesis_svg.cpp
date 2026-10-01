// SVG path data (Noesis::SVGPath) and whole-document parsing (Noesis::SVG::Image).
// Both run on the CPU; no RenderDevice is needed.
//
// Neither type is a BaseComponent: handles are plain new/delete objects and
// must be released with their *_destroy entrypoint, never
// noesis_base_component_release.

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

// Borrows the command buffer; invalidated by any builder call on the path.
Noesis::ArrayRef<uint32_t> commands_of(Noesis::SVGPath* path) {
    return Noesis::ArrayRef<uint32_t>(path->commands.Data(), path->commands.Size());
}

}  // namespace

// Null on a null or unparseable string. Release with noesis_svg_path_destroy.
extern "C" void* noesis_svg_path_parse(const char* str) {
    if (!str) return nullptr;
    Noesis::SVGPath* path = new Noesis::SVGPath();
    if (!Noesis::SVGPath::TryParse(str, *path)) {
        delete path;
        return nullptr;
    }
    return path;
}

extern "C" void* noesis_svg_path_create() { return new Noesis::SVGPath(); }

extern "C" void noesis_svg_path_destroy(void* path) { delete as_path(path); }

extern "C" uint32_t noesis_svg_path_command_count(void* path) {
    Noesis::SVGPath* p = as_path(path);
    return p ? p->commands.Size() : 0u;
}

extern "C" void noesis_svg_path_move_to(void* path, float x, float y) {
    Noesis::SVGPath* p = as_path(path);
    if (p) Noesis::SVGPath::MoveTo(p->commands, x, y);
}

extern "C" void noesis_svg_path_line_to(void* path, float x, float y) {
    Noesis::SVGPath* p = as_path(path);
    if (p) Noesis::SVGPath::LineTo(p->commands, x, y);
}

extern "C" void noesis_svg_path_close(void* path) {
    Noesis::SVGPath* p = as_path(path);
    if (p) Noesis::SVGPath::Close(p->commands);
}

extern "C" void noesis_svg_path_add_rect(void* path, float x, float y, float width,
                                            float height) {
    Noesis::SVGPath* p = as_path(path);
    if (p) Noesis::SVGPath::AddRect(p->commands, x, y, width, height);
}

extern "C" void noesis_svg_path_add_ellipse(void* path, float x, float y, float rx, float ry) {
    Noesis::SVGPath* p = as_path(path);
    if (p) Noesis::SVGPath::AddEllipse(p->commands, x, y, rx, ry);
}

// out = [x, y, width, height].
extern "C" bool noesis_svg_path_calculate_bounds(void* path, float out[4]) {
    Noesis::SVGPath* p = as_path(path);
    if (!p || !out) return false;
    Noesis::Rect r = Noesis::SVGPath::CalculateBounds(commands_of(p));
    out[0] = r.x;
    out[1] = r.y;
    out[2] = r.width;
    out[3] = r.height;
    return true;
}

// `fill_rule`: 0 EvenOdd, 1 NonZero (Noesis::SVGPath::Fill ordinals).
extern "C" bool noesis_svg_path_fill_contains(void* path, float x, float y, int32_t fill_rule) {
    Noesis::SVGPath* p = as_path(path);
    if (!p) return false;
    return Noesis::SVGPath::FillContains(commands_of(p), Noesis::Point(x, y),
                                         static_cast<Noesis::SVGPath::Fill>(fill_rule));
}

// `join` / `start_cap` / `end_cap` are SVGPath::StrokeJoinStyle /
// StrokeCapStyle ordinals.
extern "C" bool noesis_svg_path_stroke_contains(void* path, float x, float y, float width,
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

// Null only for a null `svg`; a malformed document yields an image with zero
// shapes. Release with noesis_svg_image_destroy.
extern "C" void* noesis_svg_image_parse(const char* svg) {
    if (!svg) return nullptr;
    Noesis::SVG::Image* image = new Noesis::SVG::Image();
    Noesis::SVG::Parse(svg, *image);
    return image;
}

extern "C" void noesis_svg_image_destroy(void* image) { delete as_image(image); }

extern "C" bool noesis_svg_image_get_size(void* image, float* width, float* height) {
    Noesis::SVG::Image* img = as_image(image);
    if (!img) return false;
    if (width) *width = img->width;
    if (height) *height = img->height;
    return true;
}

extern "C" uint32_t noesis_svg_image_shape_count(void* image) {
    Noesis::SVG::Image* img = as_image(image);
    return img ? img->shapes.Size() : 0u;
}

// SVG::Brush::Type ordinal (0 None, 1 Solid, 2 Linear, 3 Radial), or -1 if
// `index` is out of range.
extern "C" int32_t noesis_svg_image_shape_fill_type(void* image, uint32_t index) {
    Noesis::SVG::Image* img = as_image(image);
    if (!img || index >= img->shapes.Size()) return -1;
    return static_cast<int32_t>(img->shapes[index].fill.type);
}
