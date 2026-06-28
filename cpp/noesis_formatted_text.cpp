// Code-built FormattedText measurement / layout (TODO §13).
//
// FormattedText (NsGui/FormattedText.h) is a BaseComponent that computes glyph
// metrics and a text layout for a string + font properties at construction time
// (the public ctors call CalculateMetrics internally — there are no separate
// Set* layout mutators in 3.2.13, the constraints are constructor arguments).
//
// OWNERSHIP: this unit deliberately exposes NO public FontFamily entrypoint
// (the typography unit owns FontFamily). The create entrypoint takes the font
// family as a const char* NAME and builds the Noesis::FontFamily INTERNALLY,
// holding it in a local Ptr only for the duration of construction (the ctor
// consumes it while building font faces). The optional foreground is likewise a
// color built into a SolidColorBrush internally so callers never traffic in raw
// Brush*/FontFamily* pointers here.
//
// The returned FormattedText* is handed out with a single owned +1 reference
// (handout() idiom shared with cpp/noesis_brushes.cpp); the Rust handle's Drop
// calls noesis_base_component_release.
//
// Read-back getters (GetBounds / GetNumLines / GetLineInfo / Measure / …) let
// tests prove metrics genuinely crossed into the live Noesis object: a stub
// returning 0 fails the "width > 0 / longer string measures wider" assertions.

#include "noesis_shim.h"

#include <cfloat>

#include <NsCore/BaseComponent.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsDrawing/Color.h>
#include <NsDrawing/Rect.h>
#include <NsDrawing/Size.h>
#include <NsGui/FontFamily.h>
#include <NsGui/FontProperties.h>   // FontWeight / FontStyle / FontStretch
#include <NsGui/FormattedText.h>
#include <NsGui/SolidColorBrush.h>
#include <NsGui/TextProperties.h>   // TextAlignment / TextTrimming / TextWrapping / …

namespace {

void* handout(Noesis::BaseComponent* c) {
    if (!c) return nullptr;
    c->AddReference();
    return c;
}

Noesis::FormattedText* cast(void* p) {
    if (!p) return nullptr;
    return Noesis::DynamicCast<Noesis::FormattedText*>(static_cast<Noesis::BaseComponent*>(p));
}

}  // namespace

// Build a FormattedText for `text` in the family named `font_family` (resolved
// internally; pass a path-rooted name like "Fonts/#Bitter" or a bare family
// resolved through the font fallback chain). `weight`/`stretch`/`style` are the
// NsGui/FontProperties.h enum ordinals; `font_size` is in DIPs. The remaining
// arguments mirror the ctor constraints: `flow_direction` (FlowDirection),
// `max_width`/`max_height` (negative ⇒ unconstrained / FLT_MAX), `line_height`
// (0 ⇒ natural), `text_alignment` (TextAlignment), `text_trimming`
// (TextTrimming). `foreground` is an optional [r,g,b,a]; null ⇒ opaque black.
// Returns a +1 FormattedText* (release with noesis_base_component_release),
// or null on allocation failure.
extern "C" void* noesis_formatted_text_create(
    const char* text, const char* font_family, int32_t weight, int32_t stretch, int32_t style,
    float font_size, int32_t flow_direction, float max_width, float max_height, float line_height,
    int32_t text_alignment, int32_t text_trimming, const float foreground[4]) {
    // FontFamily is built here and lives only for the construction call.
    Noesis::Ptr<Noesis::FontFamily> family =
        *new Noesis::FontFamily(font_family ? font_family : "");

    Noesis::Color fg = foreground
                           ? Noesis::Color(foreground[0], foreground[1], foreground[2],
                                           foreground[3])
                           : Noesis::Color(0.0f, 0.0f, 0.0f, 1.0f);
    Noesis::Ptr<Noesis::SolidColorBrush> brush = *new Noesis::SolidColorBrush(fg);

    float mw = max_width < 0.0f ? FLT_MAX : max_width;
    float mh = max_height < 0.0f ? FLT_MAX : max_height;

    Noesis::Ptr<Noesis::FormattedText> ft = *new Noesis::FormattedText(
        text ? text : "", family.GetPtr(), static_cast<Noesis::FontWeight>(weight),
        static_cast<Noesis::FontStretch>(stretch), static_cast<Noesis::FontStyle>(style),
        font_size, brush.GetPtr(), static_cast<Noesis::FlowDirection>(flow_direction), mw, mh,
        line_height, static_cast<Noesis::TextAlignment>(text_alignment),
        static_cast<Noesis::TextTrimming>(text_trimming));

    return handout(ft.GetPtr());
}

// Text bounds from the last layout: out = {x, y, width, height} in DIPs.
extern "C" bool noesis_formatted_text_get_bounds(void* ft, float out[4]) {
    auto* f = cast(ft);
    if (!f || !out) return false;
    Noesis::Rect r = f->GetBounds();
    out[0] = r.x;
    out[1] = r.y;
    out[2] = r.width;
    out[3] = r.height;
    return true;
}

// Number of laid-out lines, or -1 if `ft` is not a FormattedText.
extern "C" int32_t noesis_formatted_text_get_num_lines(void* ft) {
    auto* f = cast(ft);
    if (!f) return -1;
    return static_cast<int32_t>(f->GetNumLines());
}

// Per-line metrics for `index` (< GetNumLines): glyph count, height, baseline.
// Any out pointer may be null. Returns false on null/not-a-FormattedText or an
// out-of-range index.
extern "C" bool noesis_formatted_text_get_line_info(void* ft, uint32_t index,
                                                       uint32_t* out_num_glyphs, float* out_height,
                                                       float* out_baseline) {
    auto* f = cast(ft);
    if (!f) return false;
    if (index >= f->GetNumLines()) return false;
    const Noesis::LineInfo& info = f->GetLineInfo(index);
    if (out_num_glyphs) *out_num_glyphs = info.numGlyphs;
    if (out_height) *out_height = info.height;
    if (out_baseline) *out_baseline = info.baseline;
    return true;
}

// Whether the FormattedText holds no text. Writes the flag to `out`; returns
// false (and leaves `out` untouched) if `ft` is not a FormattedText.
extern "C" bool noesis_formatted_text_is_empty(void* ft, bool* out) {
    auto* f = cast(ft);
    if (!f || !out) return false;
    *out = f->IsEmpty();
    return true;
}

// Whether the FormattedText paints with any VisualBrush. Same out/return
// contract as is_empty.
extern "C" bool noesis_formatted_text_has_visual_brush(void* ft, bool* out) {
    auto* f = cast(ft);
    if (!f || !out) return false;
    *out = f->HasVisualBrush();
    return true;
}

// Re-measure the stored runs under fresh constraints, returning the resulting
// Size to out_w/out_h (DIPs). `alignment`/`wrapping`/`trimming`/`line_stacking`/
// `flow_direction` are the matching enum ordinals; negative max_* ⇒ FLT_MAX.
// This is an independent read-back of the same metrics the ctor computes.
extern "C" bool noesis_formatted_text_measure(void* ft, int32_t alignment, int32_t wrapping,
                                                 int32_t trimming, float max_width, float max_height,
                                                 float line_height, int32_t line_stacking,
                                                 int32_t flow_direction, float* out_w,
                                                 float* out_h) {
    auto* f = cast(ft);
    if (!f) return false;
    float mw = max_width < 0.0f ? FLT_MAX : max_width;
    float mh = max_height < 0.0f ? FLT_MAX : max_height;
    Noesis::Size s = f->Measure(
        static_cast<Noesis::TextAlignment>(alignment), static_cast<Noesis::TextWrapping>(wrapping),
        static_cast<Noesis::TextTrimming>(trimming), mw, mh, line_height,
        static_cast<Noesis::LineStackingStrategy>(line_stacking),
        static_cast<Noesis::FlowDirection>(flow_direction));
    if (out_w) *out_w = s.width;
    if (out_h) *out_h = s.height;
    return true;
}

// x/y position of the glyph at character index `ch_index` (after the char when
// `after_char`). Noesis returns -10/-10 when the index is outside layout limits.
extern "C" bool noesis_formatted_text_get_glyph_position(void* ft, uint32_t ch_index,
                                                            bool after_char, float* out_x,
                                                            float* out_y) {
    auto* f = cast(ft);
    if (!f) return false;
    float x = 0.0f;
    float y = 0.0f;
    f->GetGlyphPosition(ch_index, after_char, x, y);
    if (out_x) *out_x = x;
    if (out_y) *out_y = y;
    return true;
}

// Glyph index under the point (x, y) in layout DIPs. `out_is_inside` /
// `out_is_trailing` (either may be null) report whether the point fell inside a
// glyph and on its trailing half. Returns false on null/not-a-FormattedText.
extern "C" bool noesis_formatted_text_hit_test(void* ft, float x, float y, uint32_t* out_index,
                                                  bool* out_is_inside, bool* out_is_trailing) {
    auto* f = cast(ft);
    if (!f) return false;
    bool isInside = false;
    bool isTrailing = false;
    uint32_t index = f->HitTest(x, y, isInside, isTrailing);
    if (out_index) *out_index = index;
    if (out_is_inside) *out_is_inside = isInside;
    if (out_is_trailing) *out_is_trailing = isTrailing;
    return true;
}
