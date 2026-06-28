// TextBlock inline content model (TODO §13): the Inline element family that
// ships in 3.2.13 — Run, Span, Bold, Italic, Underline, Hyperlink, LineBreak,
// and InlineUIContainer — plus the InlineCollection (UICollection<Inline>) that
// TextBlock and Span expose so inlines can be assembled from Rust.
//
// Ownership mirrors cpp/noesis_brushes.cpp / cpp/noesis_collections.cpp:
//
//   * Each *_create hands a freshly-`new`'d Inline out across the C ABI with a
//     single owned +1 reference (handout()), balanced by the Rust handle's Drop
//     calling noesis_base_component_release. Adding the Inline to an
//     InlineCollection makes the collection take its own reference, so the Rust
//     builder handle may be dropped afterwards.
//
//   * GetInlines (on TextBlock / Span) returns the live InlineCollection at +1
//     (handout) so Rust holds an owning handle over it; the collection is also
//     owned by its host element, and the +1 keeps it alive for the handle's
//     lifetime. The collection's add/count/get entrypoints mirror the
//     UICollection<Inline> surface (Add/Count/Get) the same way
//     cpp/noesis_collections.cpp wraps ObservableCollection.
//
// Read-back getters (Run text, Hyperlink NavigateUri, collection Count/Get,
// InlineUIContainer Child, Inline TextDecorations) re-read from the live Noesis
// object so a stubbed constructor/setter fails the round-trip.

#include "noesis_shim.h"

#include <NsCore/BaseComponent.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsGui/Bold.h>
#include <NsGui/Enums.h>  // TextDecorations
#include <NsGui/Hyperlink.h>
#include <NsGui/Inline.h>
#include <NsGui/InlineUIContainer.h>
#include <NsGui/Italic.h>
#include <NsGui/LineBreak.h>
#include <NsGui/Run.h>
#include <NsGui/Span.h>
#include <NsGui/TextBlock.h>
#include <NsGui/UICollection.h>
#include <NsGui/UIElement.h>
#include <NsGui/Underline.h>

namespace {

// Hand a freshly-created (or borrowed) BaseComponent out across the C ABI with
// exactly one reference owned by the caller, balanced by
// noesis_base_component_release. Safe on a refcount-0 `new`'d object
// (bumps 0->1) or a live borrowed object (bumps N->N+1).
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

using InlineColl = Noesis::UICollection<Noesis::Inline>;

InlineColl* as_inlines(void* p) {
    if (!p) return nullptr;
    return Noesis::DynamicCast<InlineColl*>(static_cast<Noesis::BaseComponent*>(p));
}

}  // namespace

// ── Inline constructors ─────────────────────────────────────────────────────

extern "C" void* noesis_text_inlines_run_create(const char* text) {
    Noesis::Ptr<Noesis::Run> run = text ? *new Noesis::Run(text) : *new Noesis::Run();
    return handout(run.GetPtr());
}

extern "C" void* noesis_text_inlines_span_create(void) {
    Noesis::Ptr<Noesis::Span> span = *new Noesis::Span();
    return handout(span.GetPtr());
}

extern "C" void* noesis_text_inlines_bold_create(void) {
    Noesis::Ptr<Noesis::Bold> bold = *new Noesis::Bold();
    return handout(bold.GetPtr());
}

extern "C" void* noesis_text_inlines_italic_create(void) {
    Noesis::Ptr<Noesis::Italic> italic = *new Noesis::Italic();
    return handout(italic.GetPtr());
}

extern "C" void* noesis_text_inlines_underline_create(void) {
    Noesis::Ptr<Noesis::Underline> underline = *new Noesis::Underline();
    return handout(underline.GetPtr());
}

extern "C" void* noesis_text_inlines_hyperlink_create(void) {
    Noesis::Ptr<Noesis::Hyperlink> link = *new Noesis::Hyperlink();
    return handout(link.GetPtr());
}

extern "C" void* noesis_text_inlines_line_break_create(void) {
    Noesis::Ptr<Noesis::LineBreak> br = *new Noesis::LineBreak();
    return handout(br.GetPtr());
}

extern "C" void* noesis_text_inlines_ui_container_create(void) {
    Noesis::Ptr<Noesis::InlineUIContainer> c = *new Noesis::InlineUIContainer();
    return handout(c.GetPtr());
}

// ── Run text ────────────────────────────────────────────────────────────────

extern "C" bool noesis_text_inlines_run_set_text(void* run, const char* text) {
    auto* r = cast<Noesis::Run>(run);
    if (!r) return false;
    // Run::SetText copies into the Run's own storage; `text` need not outlive
    // the call. A null pointer clears the run to the empty string.
    r->SetText(text ? text : "");
    return true;
}

// Borrowed (no +1) pointer into the Run's own UTF-8 storage, or NULL when `run`
// is not a Run. Valid until the Run's text is next mutated; copy if you need to
// keep it.
extern "C" const char* noesis_text_inlines_run_get_text(void* run) {
    auto* r = cast<Noesis::Run>(run);
    if (!r) return nullptr;
    return r->GetText();
}

// ── Hyperlink NavigateUri ───────────────────────────────────────────────────

extern "C" bool noesis_text_inlines_hyperlink_set_navigate_uri(
    void* link, const char* uri) {
    auto* h = cast<Noesis::Hyperlink>(link);
    if (!h) return false;
    h->SetNavigateUri(uri ? uri : "");
    return true;
}

// Borrowed (no +1) pointer into the Hyperlink's NavigateUri storage, or NULL
// when `link` is not a Hyperlink.
extern "C" const char* noesis_text_inlines_hyperlink_get_navigate_uri(void* link) {
    auto* h = cast<Noesis::Hyperlink>(link);
    if (!h) return nullptr;
    return h->GetNavigateUri();
}

// ── Inline base: TextDecorations ────────────────────────────────────────────

// `decorations` is a Noesis::TextDecorations enum value (0 None, 1 OverLine,
// 2 Baseline, 3 Underline, 4 Strikethrough). Returns false if `inl` is not an
// Inline.
extern "C" bool noesis_text_inlines_inline_set_text_decorations(
    void* inl, int32_t decorations) {
    auto* i = cast<Noesis::Inline>(inl);
    if (!i) return false;
    i->SetTextDecorations(static_cast<Noesis::TextDecorations>(decorations));
    return true;
}

// Reads TextDecorations back from the live Inline. Returns -1 if `inl` is not
// an Inline.
extern "C" int32_t noesis_text_inlines_inline_get_text_decorations(void* inl) {
    auto* i = cast<Noesis::Inline>(inl);
    if (!i) return -1;
    return static_cast<int32_t>(i->GetTextDecorations());
}

// ── InlineUIContainer Child ─────────────────────────────────────────────────

// Host `child` (a UIElement*, e.g. a Button) in the container. The container
// takes its own reference; pass NULL to clear. Returns false if `container` is
// not an InlineUIContainer or `child` is non-null but not a UIElement.
extern "C" bool noesis_text_inlines_ui_container_set_child(
    void* container, void* child) {
    auto* c = cast<Noesis::InlineUIContainer>(container);
    if (!c) return false;
    if (!child) {
        c->SetChild(nullptr);
        return true;
    }
    auto* ui = cast<Noesis::UIElement>(child);
    if (!ui) return false;
    c->SetChild(ui);
    return true;
}

// Borrowed (no +1) BaseComponent* of the container's hosted Child, or NULL.
// The address matches the BaseComponent subobject of the UIElement that was
// set, so callers can compare it for identity against the element they passed.
extern "C" void* noesis_text_inlines_ui_container_get_child(void* container) {
    auto* c = cast<Noesis::InlineUIContainer>(container);
    if (!c) return nullptr;
    Noesis::UIElement* child = c->GetChild();
    return static_cast<Noesis::BaseComponent*>(child);
}

// ── InlineCollection (UICollection<Inline>) ─────────────────────────────────

// Live InlineCollection of a TextBlock's top-level inlines, handed out at +1
// (release via noesis_base_component_release). The collection is also owned
// by the TextBlock; the +1 keeps it alive for the handle's lifetime. NULL when
// `text_block` is not a TextBlock.
extern "C" void* noesis_text_inlines_text_block_get_inlines(void* text_block) {
    auto* tb = cast<Noesis::TextBlock>(text_block);
    if (!tb) return nullptr;
    return handout(tb->GetInlines());
}

// Live nested InlineCollection of a Span (or Span subclass: Bold/Italic/
// Underline/Hyperlink), handed out at +1. NULL when `span` is not a Span.
extern "C" void* noesis_text_inlines_span_get_inlines(void* span) {
    auto* s = cast<Noesis::Span>(span);
    if (!s) return nullptr;
    return handout(s->GetInlines());
}

// Append `inl` (a borrowed Inline*; the collection takes its own reference).
// Returns the insertion index, or -1 if `collection` is not an InlineCollection
// or `inl` is not an Inline.
extern "C" int32_t noesis_text_inlines_collection_add(void* collection, void* inl) {
    InlineColl* coll = as_inlines(collection);
    auto* i = cast<Noesis::Inline>(inl);
    if (!coll || !i) return -1;
    return coll->Add(i);
}

// Item count, or -1 if `collection` is not an InlineCollection.
extern "C" int32_t noesis_text_inlines_collection_count(void* collection) {
    InlineColl* coll = as_inlines(collection);
    return coll ? coll->Count() : -1;
}

// Borrowed (no +1) Inline* at `index`, or NULL on null/non-collection/
// out-of-range. The collection owns the reference.
extern "C" void* noesis_text_inlines_collection_get(void* collection, uint32_t index) {
    InlineColl* coll = as_inlines(collection);
    if (!coll || index >= (uint32_t)coll->Count()) return nullptr;
    return static_cast<Noesis::BaseComponent*>(coll->Get(index));
}
