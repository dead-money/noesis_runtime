// Code-side element-tree construction (Phase 1): build and mutate panel trees,
// Border/Decorator children, and Grid row/column definitions from Rust. The
// relevant collections (Panel::Children, Grid::Row/ColumnDefinitions) and the
// Decorator Child are NOT DependencyProperties, so they cannot be reached by the
// generic by-name DP setters — this unit wraps the typed C++ accessors instead.
//
// Ownership mirrors cpp/noesis_text_inlines.cpp / cpp/noesis_collections.cpp:
//
//   * GetChildren / GetRowDefinitions / GetColumnDefinitions return the live
//     collection owned by its host element; we hand it out at +1 (handout) so
//     Rust holds an owning view that keeps it alive for the handle's lifetime,
//     released via dm_noesis_base_component_release.
//
//   * RowDefinition / ColumnDefinition _create hand out a freshly-`new`'d object
//     at +1; adding it to a DefinitionCollection makes the collection take its
//     own reference, so the Rust builder handle may be dropped afterwards.
//
//   * Decorator::GetChild and the collection Get* accessors return BORROWED
//     (no +1) pointers owned by the host; the address matches the BaseComponent
//     subobject of the element set, so callers can compare it for identity.
//
// Read-back getters (Decorator child, collection counts/gets, Grid definition
// lengths) re-read from the live Noesis object so a stubbed constructor/setter
// fails the round-trip.

#include "noesis_shim.h"

#include <NsCore/BaseComponent.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsGui/BaseCollection.h>
#include <NsGui/ColumnDefinition.h>
#include <NsGui/Decorator.h>
#include <NsGui/Grid.h>
#include <NsGui/GridLength.h>
#include <NsGui/Panel.h>
#include <NsGui/RowDefinition.h>
#include <NsGui/UICollection.h>
#include <NsGui/UIElement.h>
#include <NsGui/UIElementCollection.h>

// GridUnitType ordinals the Rust side mirrors (NsGui/GridLength.h). Note the
// WPF-unusual order: Auto precedes Pixel.
static_assert(Noesis::GridUnitType_Auto == 0, "GridUnitType ordinal drift");
static_assert(Noesis::GridUnitType_Pixel == 1, "GridUnitType ordinal drift");
static_assert(Noesis::GridUnitType_Star == 2, "GridUnitType ordinal drift");

namespace {

// Hand a freshly-created (or borrowed) BaseComponent out across the C ABI with
// exactly one reference owned by the caller, balanced by
// dm_noesis_base_component_release.
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

using UIElemColl = Noesis::UICollection<Noesis::UIElement>;

UIElemColl* as_children(void* p) {
    if (!p) return nullptr;
    return Noesis::DynamicCast<UIElemColl*>(static_cast<Noesis::BaseComponent*>(p));
}

Noesis::BaseCollection* as_defs(void* p) {
    if (!p) return nullptr;
    return Noesis::DynamicCast<Noesis::BaseCollection*>(static_cast<Noesis::BaseComponent*>(p));
}

}  // namespace

// ── Decorator / Border Child ─────────────────────────────────────────────────

// Set the single Child of a Decorator (e.g. Border). The Decorator takes its own
// reference; pass NULL to clear. Returns false if `decorator` is not a Decorator
// or `child` is non-null but not a UIElement.
extern "C" bool dm_noesis_decorator_set_child(void* decorator, void* child) {
    auto* d = cast<Noesis::Decorator>(decorator);
    if (!d) return false;
    if (!child) {
        d->SetChild(nullptr);
        return true;
    }
    auto* ui = cast<Noesis::UIElement>(child);
    if (!ui) return false;
    d->SetChild(ui);
    return true;
}

// Borrowed (no +1) BaseComponent* of the Decorator's Child, or NULL. The address
// matches the BaseComponent subobject of the UIElement that was set.
extern "C" void* dm_noesis_decorator_get_child(void* decorator) {
    auto* d = cast<Noesis::Decorator>(decorator);
    if (!d) return nullptr;
    return static_cast<Noesis::BaseComponent*>(d->GetChild());
}

// ── Panel Children (UICollection<UIElement>) ─────────────────────────────────

// Live UIElementCollection of a Panel's children, handed out at +1 (release via
// dm_noesis_base_component_release). The collection is also owned by the Panel;
// the +1 keeps it alive for the handle's lifetime. NULL when `panel` is not a
// Panel.
extern "C" void* dm_noesis_panel_children_get(void* panel) {
    auto* p = cast<Noesis::Panel>(panel);
    if (!p) return nullptr;
    return handout(p->GetChildren());
}

// Append `child` (a borrowed UIElement*; the collection takes its own reference).
// Returns the insertion index, or -1 if `coll` is not a UIElementCollection or
// `child` is not a UIElement.
extern "C" int32_t dm_noesis_panel_children_add(void* coll, void* child) {
    UIElemColl* c = as_children(coll);
    auto* ui = cast<Noesis::UIElement>(child);
    if (!c || !ui) return -1;
    return c->Add(ui);
}

// Insert `child` at `index` (allows index == count). Returns false if `coll` is
// not a UIElementCollection, `child` is not a UIElement, or `index` is out of
// range.
extern "C" bool dm_noesis_panel_children_insert(void* coll, uint32_t index, void* child) {
    UIElemColl* c = as_children(coll);
    auto* ui = cast<Noesis::UIElement>(child);
    if (!c || !ui || index > (uint32_t)c->Count()) return false;
    c->Insert(index, ui);
    return true;
}

// Remove the child at `index`. Returns false on non-collection / out-of-range.
extern "C" bool dm_noesis_panel_children_remove_at(void* coll, uint32_t index) {
    UIElemColl* c = as_children(coll);
    if (!c || index >= (uint32_t)c->Count()) return false;
    c->RemoveAt(index);
    return true;
}

// Remove every child. Returns false if `coll` is not a UIElementCollection.
extern "C" bool dm_noesis_panel_children_clear(void* coll) {
    UIElemColl* c = as_children(coll);
    if (!c) return false;
    c->Clear();
    return true;
}

// Child count, or -1 if `coll` is not a UIElementCollection.
extern "C" int32_t dm_noesis_panel_children_count(void* coll) {
    UIElemColl* c = as_children(coll);
    return c ? c->Count() : -1;
}

// Borrowed (no +1) UIElement* at `index`, or NULL on null/non-collection/
// out-of-range. The collection owns the reference.
extern "C" void* dm_noesis_panel_children_get_at(void* coll, uint32_t index) {
    UIElemColl* c = as_children(coll);
    if (!c || index >= (uint32_t)c->Count()) return nullptr;
    return static_cast<Noesis::BaseComponent*>(c->Get(index));
}

// ── Grid RowDefinition / ColumnDefinition ────────────────────────────────────

// Create a RowDefinition / ColumnDefinition at +1 (release via
// dm_noesis_base_component_release). Defaults to a 1* height/width.
extern "C" void* dm_noesis_grid_row_definition_create(void) {
    Noesis::Ptr<Noesis::RowDefinition> d = *new Noesis::RowDefinition();
    return handout(d.GetPtr());
}

extern "C" void* dm_noesis_grid_column_definition_create(void) {
    Noesis::Ptr<Noesis::ColumnDefinition> d = *new Noesis::ColumnDefinition();
    return handout(d.GetPtr());
}

// Set a RowDefinition's Height from a marshalled GridLength (`value`, `unit` is a
// GridUnitType ordinal: 0 Auto, 1 Pixel, 2 Star). Returns false if `def` is not
// a RowDefinition or `unit` is out of range.
extern "C" bool dm_noesis_grid_row_definition_set_height(void* def, float value, int32_t unit) {
    auto* d = cast<Noesis::RowDefinition>(def);
    if (!d || unit < 0 || unit > 2) return false;
    d->SetHeight(Noesis::GridLength(value, static_cast<Noesis::GridUnitType>(unit)));
    return true;
}

// Read a RowDefinition's Height back into `*out_value` / `*out_unit`. Returns
// false (leaving the out-params untouched) if `def` is not a RowDefinition.
extern "C" bool dm_noesis_grid_row_definition_get_height(
    void* def, float* out_value, int32_t* out_unit) {
    auto* d = cast<Noesis::RowDefinition>(def);
    if (!d) return false;
    const Noesis::GridLength& gl = d->GetHeight();
    if (out_value) *out_value = gl.GetValue();
    if (out_unit) *out_unit = static_cast<int32_t>(gl.GetGridUnitType());
    return true;
}

// Set a ColumnDefinition's Width from a marshalled GridLength. As above.
extern "C" bool dm_noesis_grid_column_definition_set_width(void* def, float value, int32_t unit) {
    auto* d = cast<Noesis::ColumnDefinition>(def);
    if (!d || unit < 0 || unit > 2) return false;
    d->SetWidth(Noesis::GridLength(value, static_cast<Noesis::GridUnitType>(unit)));
    return true;
}

// Read a ColumnDefinition's Width back. As above.
extern "C" bool dm_noesis_grid_column_definition_get_width(
    void* def, float* out_value, int32_t* out_unit) {
    auto* d = cast<Noesis::ColumnDefinition>(def);
    if (!d) return false;
    const Noesis::GridLength& gl = d->GetWidth();
    if (out_value) *out_value = gl.GetValue();
    if (out_unit) *out_unit = static_cast<int32_t>(gl.GetGridUnitType());
    return true;
}

// ── Grid definition collections ──────────────────────────────────────────────

// Live Row/ColumnDefinitionCollection of a Grid, handed out at +1. The
// collection is also owned by the Grid. NULL when `grid` is not a Grid.
extern "C" void* dm_noesis_grid_get_row_definitions(void* grid) {
    auto* g = cast<Noesis::Grid>(grid);
    if (!g) return nullptr;
    return handout(g->GetRowDefinitions());
}

extern "C" void* dm_noesis_grid_get_column_definitions(void* grid) {
    auto* g = cast<Noesis::Grid>(grid);
    if (!g) return nullptr;
    return handout(g->GetColumnDefinitions());
}

// Append `def` (a borrowed Row/ColumnDefinition*; the collection takes its own
// reference and validates the element type). Returns the insertion index, or -1
// if `coll` is not a definition collection or `def` is not a BaseDefinition.
extern "C" int32_t dm_noesis_definition_collection_add(void* coll, void* def) {
    Noesis::BaseCollection* c = as_defs(coll);
    auto* d = cast<Noesis::BaseDefinition>(def);
    if (!c || !d) return -1;
    return c->AddComponent(static_cast<Noesis::BaseComponent*>(d));
}

// Insert `def` at `index` (allows index == count). Returns false on
// non-collection / non-definition / out-of-range.
extern "C" bool dm_noesis_definition_collection_insert(void* coll, uint32_t index, void* def) {
    Noesis::BaseCollection* c = as_defs(coll);
    auto* d = cast<Noesis::BaseDefinition>(def);
    if (!c || !d || index > (uint32_t)c->Count()) return false;
    c->InsertComponent(index, static_cast<Noesis::BaseComponent*>(d));
    return true;
}

// Remove the definition at `index`. Returns false on non-collection /
// out-of-range.
extern "C" bool dm_noesis_definition_collection_remove_at(void* coll, uint32_t index) {
    Noesis::BaseCollection* c = as_defs(coll);
    if (!c || index >= (uint32_t)c->Count()) return false;
    c->RemoveAt(index);
    return true;
}

// Remove every definition. Returns false if `coll` is not a definition
// collection.
extern "C" bool dm_noesis_definition_collection_clear(void* coll) {
    Noesis::BaseCollection* c = as_defs(coll);
    if (!c) return false;
    c->Clear();
    return true;
}

// Definition count, or -1 if `coll` is not a definition collection.
extern "C" int32_t dm_noesis_definition_collection_count(void* coll) {
    Noesis::BaseCollection* c = as_defs(coll);
    return c ? c->Count() : -1;
}

// Borrowed (no +1) Row/ColumnDefinition* at `index`, or NULL on null/
// non-collection/out-of-range. The collection owns the reference.
extern "C" void* dm_noesis_definition_collection_get(void* coll, uint32_t index) {
    Noesis::BaseCollection* c = as_defs(coll);
    if (!c || index >= (uint32_t)c->Count()) return nullptr;
    // GetComponent hands back a +1 Ptr; the collection still owns its own
    // reference, so the object outlives the temporary's release and the bare
    // pointer is a valid borrow.
    return c->GetComponent(index).GetPtr();
}
