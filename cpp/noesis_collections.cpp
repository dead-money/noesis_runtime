// Data-binding bridge: ObservableCollection + boxing + DataContext / ItemsSource
// wiring (TODO §3).
//
// This is the "drive XAML from Rust data" surface. Three cooperating pieces:
//
//   * ObservableCollection<BaseComponent> — Noesis's concrete observable list.
//     It already implements INotifyCollectionChanged + INotifyPropertyChanged,
//     so once it's bound to an ItemsControl.ItemsSource, every Add/Insert/
//     Remove/Clear from Rust raises CollectionChanged and the control
//     regenerates its containers. We just expose CRUD over the C ABI.
//
//   * Boxing — list items and DataContext values are `BaseComponent*`. The
//     most common item is a string; `dm_noesis_box_string` wraps a C string
//     in a `BoxedValue<String>` so a `DataTemplate` with `{Binding}` (the whole
//     item) renders it. Reference-typed view models (the synthetic classes from
//     noesis_classes.cpp) are passed through directly.
//
//   * DataContext / ItemsSource setters — the two DependencyObject hooks a
//     binding-driven workflow needs: point an element's DataContext at a Rust
//     view model, or an ItemsControl's ItemsSource at an ObservableCollection.
//
// Bindings themselves are authored in XAML (`{Binding Path}`); this bridge is
// the runtime plumbing that makes those bindings resolve against Rust-owned
// data. A synthetic-class instance (a DependencyObject with registered DPs)
// used as a DataContext is a fully functional binding source: writing a DP
// from Rust raises the DependencyObject change notification the binding engine
// observes, so the bound element updates on the next View::Update.

#include "noesis_shim.h"

#include <NsCore/Boxing.h>
#include <NsCore/BaseComponent.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Ptr.h>
#include <NsGui/FrameworkElement.h>
#include <NsGui/ItemCollection.h>
#include <NsGui/ItemContainerGenerator.h>
#include <NsGui/ItemsControl.h>
#include <NsGui/ObservableCollection.h>

namespace {

// Hand a freshly-created (or borrowed) BaseComponent out across the C ABI with
// exactly one reference owned by the caller, balanced by
// `dm_noesis_base_component_release`. Safe to call on a refcount-0 `new`'d
// object (bumps 0→1) or on a live borrowed object (bumps N→N+1).
void* handout(Noesis::BaseComponent* c) {
    if (!c) return nullptr;
    c->AddReference();
    return c;
}

using ObsColl = Noesis::ObservableCollection<Noesis::BaseComponent>;

ObsColl* as_collection(void* p) {
    if (!p) return nullptr;
    // The collection is created by us as an ObservableCollection<BaseComponent>;
    // a plain static_cast through BaseComponent is correct, but DynamicCast
    // keeps us honest if a caller passes the wrong pointer.
    return Noesis::DynamicCast<ObsColl*>(static_cast<Noesis::BaseComponent*>(p));
}

}  // namespace

// ── Boxing ──────────────────────────────────────────────────────────────────

extern "C" void* dm_noesis_box_string(const char* text) {
    // Box(const char*) copies the bytes into a Noesis::String inside a
    // BoxedValue<String>; the caller's `text` can go away after this call.
    Noesis::Ptr<Noesis::BoxedValue> boxed = Noesis::Boxing::Box(text ? text : "");
    return handout(boxed.GetPtr());
}

// ── ObservableCollection<BaseComponent> ─────────────────────────────────────

extern "C" void* dm_noesis_observable_collection_create(void) {
    Noesis::Ptr<ObsColl> coll = *new ObsColl();
    return handout(coll.GetPtr());
}

extern "C" int32_t dm_noesis_observable_collection_add(void* collection, void* item) {
    ObsColl* coll = as_collection(collection);
    if (!coll) return -1;
    return coll->Add(static_cast<Noesis::BaseComponent*>(item));
}

extern "C" bool dm_noesis_observable_collection_insert(
    void* collection, uint32_t index, void* item) {
    ObsColl* coll = as_collection(collection);
    if (!coll || index > (uint32_t)coll->Count()) return false;
    coll->Insert(index, static_cast<Noesis::BaseComponent*>(item));
    return true;
}

extern "C" bool dm_noesis_observable_collection_set(
    void* collection, uint32_t index, void* item) {
    ObsColl* coll = as_collection(collection);
    if (!coll || index >= (uint32_t)coll->Count()) return false;
    coll->Set(index, static_cast<Noesis::BaseComponent*>(item));
    return true;
}

extern "C" bool dm_noesis_observable_collection_remove_at(void* collection, uint32_t index) {
    ObsColl* coll = as_collection(collection);
    if (!coll || index >= (uint32_t)coll->Count()) return false;
    coll->RemoveAt(index);
    return true;
}

extern "C" void dm_noesis_observable_collection_clear(void* collection) {
    ObsColl* coll = as_collection(collection);
    if (coll) coll->Clear();
}

extern "C" int32_t dm_noesis_observable_collection_count(void* collection) {
    ObsColl* coll = as_collection(collection);
    return coll ? coll->Count() : -1;
}

// Borrowed (no +1) pointer to the item at `index`, or null. The collection
// owns the reference; copy / AddReference if you need to keep it.
extern "C" void* dm_noesis_observable_collection_get(void* collection, uint32_t index) {
    ObsColl* coll = as_collection(collection);
    if (!coll || index >= (uint32_t)coll->Count()) return nullptr;
    return coll->Get(index);
}

// ── DataContext ─────────────────────────────────────────────────────────────

extern "C" bool dm_noesis_framework_element_set_data_context(void* element, void* context) {
    if (!element) return false;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return false;
    // SetDataContext takes a borrowed pointer and stores its own ref; passing
    // null clears it.
    fe->SetDataContext(static_cast<Noesis::BaseComponent*>(context));
    return true;
}

// Borrowed (no +1) pointer to the element's current DataContext, or null.
extern "C" void* dm_noesis_framework_element_get_data_context(void* element) {
    if (!element) return nullptr;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    return fe ? fe->GetDataContext() : nullptr;
}

// ── ItemsControl.ItemsSource + container introspection ──────────────────────

extern "C" bool dm_noesis_items_control_set_items_source(void* element, void* items) {
    if (!element) return false;
    auto* ic = Noesis::DynamicCast<Noesis::ItemsControl*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!ic) return false;
    ic->SetItemsSource(static_cast<Noesis::BaseComponent*>(items));
    return true;
}

// Number of items the ItemsControl currently sees (its `Items` view over the
// bound ItemsSource). -1 if `element` is not an ItemsControl.
extern "C" int32_t dm_noesis_items_control_items_count(void* element) {
    if (!element) return -1;
    auto* ic = Noesis::DynamicCast<Noesis::ItemsControl*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!ic) return -1;
    Noesis::ItemCollection* items = ic->GetItems();
    return items ? items->Count() : 0;
}

// Number of *realized* item containers the generator has materialized. Unlike
// `items_count` (a live passthrough to the source), this only grows when the
// generator actually regenerates — which, for a source mutated after the first
// layout, requires INotifyCollectionChanged to have fired and invalidated
// measure. So a realized count that tracks post-mutation collection size is a
// genuine proof that change notification reached the control. -1 if `element`
// is not an ItemsControl.
extern "C" int32_t dm_noesis_items_control_realized_count(void* element) {
    if (!element) return -1;
    auto* ic = Noesis::DynamicCast<Noesis::ItemsControl*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!ic) return -1;
    Noesis::ItemContainerGenerator* gen = ic->GetItemContainerGenerator();
    Noesis::ItemCollection* items = ic->GetItems();
    if (!gen || !items) return 0;
    int n = items->Count();
    int realized = 0;
    for (int i = 0; i < n; ++i) {
        if (gen->ContainerFromIndex(i) != nullptr) ++realized;
    }
    return realized;
}
