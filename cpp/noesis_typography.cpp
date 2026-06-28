// Typography & text properties: FontFamily, the TextElement attached
// font properties, a representative subset of the OpenType Typography attached
// DPs, and the IME CompositionUnderline list on a TextBox.
//
// Ownership: noesis_typography_font_family_create hands a FontFamily out
// across the C ABI with a single owned +1 reference (the handout() idiom shared
// with cpp/noesis_brushes.cpp). The Rust handle's Drop releases it via
// noesis_base_component_release. Assigning a FontFamily to an element (the
// attached TextElement.FontFamily DP) makes Noesis take its own reference.
//
// The TextElement and Typography accessors operate on a borrowed DependencyObject
// (any element). TextElement exposes static attached getters/setters directly;
// the Typography DPs are plain attached DependencyProperties, so we drive them
// through DependencyObject::SetValue/GetValue with the static DP pointers — the
// same path cpp/noesis_classes.cpp uses for code-set properties. Every setter has
// a getter that re-reads from the live object so a stubbed impl fails the tests.

#include "noesis_shim.h"

#include <NsCore/BaseComponent.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsGui/Brush.h>
#include <NsGui/CompositionUnderline.h>
#include <NsGui/DependencyObject.h>
#include <NsGui/FontFamily.h>
#include <NsGui/FontProperties.h>
#include <NsGui/TextBox.h>
#include <NsGui/TextElement.h>
#include <NsGui/Typography.h>

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

// ── FontFamily ───────────────────────────────────────────────────────────────

extern "C" void* noesis_typography_font_family_create(const char* source) {
    Noesis::Ptr<Noesis::FontFamily> family =
        source ? *new Noesis::FontFamily(source) : *new Noesis::FontFamily();
    return handout(family.GetPtr());
}

// Borrowed NUL-terminated UTF-8 source string, valid while the caller holds a
// reference to the FontFamily. NULL on type mismatch.
extern "C" const char* noesis_typography_font_family_get_source(void* family) {
    auto* f = cast<Noesis::FontFamily>(family);
    if (!f) return nullptr;
    return f->GetSource();
}

// Number of concrete fonts the family resolved to (depends on the registered
// font provider). 0 if `family` is not a FontFamily or nothing resolved.
extern "C" uint32_t noesis_typography_font_family_get_num_fonts(void* family) {
    auto* f = cast<Noesis::FontFamily>(family);
    if (!f) return 0;
    return f->GetNumFonts();
}

// Borrowed name of the font at `index`, or NULL if out of range / type mismatch.
extern "C" const char* noesis_typography_font_family_get_font_name(void* family,
                                                                      uint32_t index) {
    auto* f = cast<Noesis::FontFamily>(family);
    if (!f || index >= f->GetNumFonts()) return nullptr;
    return f->GetFontName(index);
}

// ── TextElement attached font properties ─────────────────────────────────────

extern "C" bool noesis_typography_text_element_set_font_size(void* element, float size) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d) return false;
    Noesis::TextElement::SetFontSize(d, size);
    return true;
}

extern "C" bool noesis_typography_text_element_get_font_size(void* element, float* out) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d || !out) return false;
    *out = Noesis::TextElement::GetFontSize(d);
    return true;
}

extern "C" bool noesis_typography_text_element_set_font_family(void* element, void* family) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d) return false;
    Noesis::TextElement::SetFontFamily(d, cast<Noesis::FontFamily>(family));
    return true;
}

// Borrowed FontFamily* currently set (no +1), or NULL.
extern "C" void* noesis_typography_text_element_get_font_family(void* element) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d) return nullptr;
    return Noesis::TextElement::GetFontFamily(d);
}

extern "C" bool noesis_typography_text_element_set_foreground(void* element, void* brush) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d) return false;
    Noesis::TextElement::SetForeground(d, cast<Noesis::Brush>(brush));
    return true;
}

// Borrowed Brush* currently set (no +1), or NULL.
extern "C" void* noesis_typography_text_element_get_foreground(void* element) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d) return nullptr;
    return Noesis::TextElement::GetForeground(d);
}

extern "C" bool noesis_typography_text_element_set_font_weight(void* element, int32_t weight) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d) return false;
    Noesis::TextElement::SetFontWeight(d, static_cast<Noesis::FontWeight>(weight));
    return true;
}

extern "C" bool noesis_typography_text_element_get_font_weight(void* element, int32_t* out) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d || !out) return false;
    *out = static_cast<int32_t>(Noesis::TextElement::GetFontWeight(d));
    return true;
}

extern "C" bool noesis_typography_text_element_set_font_style(void* element, int32_t style) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d) return false;
    Noesis::TextElement::SetFontStyle(d, static_cast<Noesis::FontStyle>(style));
    return true;
}

extern "C" bool noesis_typography_text_element_get_font_style(void* element, int32_t* out) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d || !out) return false;
    *out = static_cast<int32_t>(Noesis::TextElement::GetFontStyle(d));
    return true;
}

extern "C" bool noesis_typography_text_element_set_font_stretch(void* element, int32_t stretch) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d) return false;
    Noesis::TextElement::SetFontStretch(d, static_cast<Noesis::FontStretch>(stretch));
    return true;
}

extern "C" bool noesis_typography_text_element_get_font_stretch(void* element, int32_t* out) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d || !out) return false;
    *out = static_cast<int32_t>(Noesis::TextElement::GetFontStretch(d));
    return true;
}

// ── Typography attached DPs (representative subset) ───────────────────────────
//
// These are plain attached DependencyProperties whose static DP pointers live on
// Noesis::Typography. We set/read them through DependencyObject::SetValue/GetValue
// with the right value type (enum or bool). The remaining ~30 Typography DPs
// (CapitalSpacing, ContextualAlternates, the 20 StylisticSet* flags, swash/
// alternate indices, …) follow this identical pattern.

extern "C" bool noesis_typography_set_capitals(void* element, int32_t value) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d) return false;
    d->SetValue<Noesis::FontCapitals>(Noesis::Typography::CapitalsProperty,
                                      static_cast<Noesis::FontCapitals>(value));
    return true;
}

extern "C" bool noesis_typography_get_capitals(void* element, int32_t* out) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d || !out) return false;
    *out = static_cast<int32_t>(d->GetValue<Noesis::FontCapitals>(Noesis::Typography::CapitalsProperty));
    return true;
}

extern "C" bool noesis_typography_set_numeral_style(void* element, int32_t value) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d) return false;
    d->SetValue<Noesis::FontNumeralStyle>(Noesis::Typography::NumeralStyleProperty,
                                          static_cast<Noesis::FontNumeralStyle>(value));
    return true;
}

extern "C" bool noesis_typography_get_numeral_style(void* element, int32_t* out) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d || !out) return false;
    *out = static_cast<int32_t>(
        d->GetValue<Noesis::FontNumeralStyle>(Noesis::Typography::NumeralStyleProperty));
    return true;
}

extern "C" bool noesis_typography_set_fraction(void* element, int32_t value) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d) return false;
    d->SetValue<Noesis::FontFraction>(Noesis::Typography::FractionProperty,
                                      static_cast<Noesis::FontFraction>(value));
    return true;
}

extern "C" bool noesis_typography_get_fraction(void* element, int32_t* out) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d || !out) return false;
    *out = static_cast<int32_t>(d->GetValue<Noesis::FontFraction>(Noesis::Typography::FractionProperty));
    return true;
}

extern "C" bool noesis_typography_set_variants(void* element, int32_t value) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d) return false;
    d->SetValue<Noesis::FontVariants>(Noesis::Typography::VariantsProperty,
                                      static_cast<Noesis::FontVariants>(value));
    return true;
}

extern "C" bool noesis_typography_get_variants(void* element, int32_t* out) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d || !out) return false;
    *out = static_cast<int32_t>(d->GetValue<Noesis::FontVariants>(Noesis::Typography::VariantsProperty));
    return true;
}

extern "C" bool noesis_typography_set_standard_ligatures(void* element, bool value) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d) return false;
    d->SetValue<bool>(Noesis::Typography::StandardLigaturesProperty, value);
    return true;
}

extern "C" bool noesis_typography_get_standard_ligatures(void* element, bool* out) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d || !out) return false;
    *out = d->GetValue<bool>(Noesis::Typography::StandardLigaturesProperty);
    return true;
}

extern "C" bool noesis_typography_set_kerning(void* element, bool value) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d) return false;
    d->SetValue<bool>(Noesis::Typography::KerningProperty, value);
    return true;
}

extern "C" bool noesis_typography_get_kerning(void* element, bool* out) {
    auto* d = cast<Noesis::DependencyObject>(element);
    if (!d || !out) return false;
    *out = d->GetValue<bool>(Noesis::Typography::KerningProperty);
    return true;
}

// ── CompositionUnderline (IME) on a TextBox ──────────────────────────────────

extern "C" bool noesis_typography_text_box_add_composition_underline(void* element,
                                                                        uint32_t start,
                                                                        uint32_t end, int32_t style,
                                                                        bool bold) {
    auto* tb = cast<Noesis::TextBox>(element);
    if (!tb) return false;
    Noesis::CompositionUnderline u;
    u.start = start;
    u.end = end;
    u.style = static_cast<Noesis::CompositionLineStyle>(style);
    u.bold = bold;
    tb->AddCompositionUnderline(u);
    return true;
}

// Number of IME composition underlines, or -1 if `element` is not a TextBox.
extern "C" int32_t noesis_typography_text_box_num_composition_underlines(void* element) {
    auto* tb = cast<Noesis::TextBox>(element);
    if (!tb) return -1;
    return static_cast<int32_t>(tb->GetNumCompositionUnderlines());
}

extern "C" bool noesis_typography_text_box_get_composition_underline(
    void* element, uint32_t index, uint32_t* out_start, uint32_t* out_end, int32_t* out_style,
    bool* out_bold) {
    auto* tb = cast<Noesis::TextBox>(element);
    if (!tb || index >= tb->GetNumCompositionUnderlines()) return false;
    const Noesis::CompositionUnderline& u = tb->GetCompositionUnderline(index);
    if (out_start) *out_start = u.start;
    if (out_end) *out_end = u.end;
    if (out_style) *out_style = static_cast<int32_t>(u.style);
    if (out_bold) *out_bold = u.bold;
    return true;
}

extern "C" bool noesis_typography_text_box_clear_composition_underlines(void* element) {
    auto* tb = cast<Noesis::TextBox>(element);
    if (!tb) return false;
    tb->ClearCompositionUnderlines();
    return true;
}
