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
#include <NsDrawing/Point.h>
#include <NsGui/DispatcherObject.h>
#include <NsGui/Enums.h>
#include <NsGui/FrameworkElement.h>
#include <NsGui/ItemCollection.h>
#include <NsGui/ItemContainerGenerator.h>
#include <NsGui/ItemsControl.h>
#include <NsGui/LogicalTreeHelper.h>
#include <NsGui/ObservableCollection.h>
#include <NsGui/Visual.h>
#include <NsGui/VisualTreeHelper.h>

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

// ── Visual / logical tree traversal (TODO §2.A) ─────────────────────────────
//
// VisualTreeHelper operates on `Visual*` — children may be plain Visuals, not
// FrameworkElements, so these return raw +1 BaseComponent* handles without
// null-filtering non-FE nodes (filtering would punch holes in indexed
// traversal). The Rust `FrameworkElement` handle is just an owned
// BaseComponent* whose FE-specific methods DynamicCast internally, so handing
// back a Visual* is fine. All owning returns AddReference() once for the
// caller (matching `find_name`); the Rust drop releases.

extern "C" uint32_t dm_noesis_visual_children_count(void* element) {
    if (!element) return 0;
    auto* v = Noesis::DynamicCast<Noesis::Visual*>(static_cast<Noesis::BaseComponent*>(element));
    if (!v) return 0;
    return Noesis::VisualTreeHelper::GetChildrenCount(v);
}

extern "C" void* dm_noesis_visual_child(void* element, uint32_t index) {
    if (!element) return nullptr;
    auto* v = Noesis::DynamicCast<Noesis::Visual*>(static_cast<Noesis::BaseComponent*>(element));
    if (!v || index >= Noesis::VisualTreeHelper::GetChildrenCount(v)) return nullptr;
    Noesis::Visual* child = Noesis::VisualTreeHelper::GetChild(v, index);
    if (!child) return nullptr;
    child->AddReference();
    return static_cast<Noesis::BaseComponent*>(child);
}

extern "C" void* dm_noesis_visual_parent(void* element) {
    if (!element) return nullptr;
    auto* v = Noesis::DynamicCast<Noesis::Visual*>(static_cast<Noesis::BaseComponent*>(element));
    if (!v) return nullptr;
    Noesis::Visual* parent = Noesis::VisualTreeHelper::GetParent(v);
    if (!parent) return nullptr;
    parent->AddReference();
    return static_cast<Noesis::BaseComponent*>(parent);
}

// Hit-test a single point in `element`-local DIPs. Returns the topmost hit
// Visual* (+1) or null when nothing was hit / `element` is not a Visual.
extern "C" void* dm_noesis_visual_hit_test(void* element, float x, float y) {
    if (!element) return nullptr;
    auto* v = Noesis::DynamicCast<Noesis::Visual*>(static_cast<Noesis::BaseComponent*>(element));
    if (!v) return nullptr;
    Noesis::HitTestResult result = Noesis::VisualTreeHelper::HitTest(v, Noesis::Point(x, y));
    if (!result.visualHit) return nullptr;
    result.visualHit->AddReference();
    return static_cast<Noesis::BaseComponent*>(result.visualHit);
}

extern "C" void* dm_noesis_framework_element_logical_parent(void* element) {
    if (!element) return nullptr;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return nullptr;
    Noesis::FrameworkElement* parent = fe->GetParent();
    if (!parent) return nullptr;
    parent->AddReference();
    return static_cast<Noesis::BaseComponent*>(parent);
}

extern "C" uint32_t dm_noesis_logical_children_count(void* element) {
    if (!element) return 0;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return 0;
    return Noesis::LogicalTreeHelper::GetChildrenCount(fe);
}

extern "C" void* dm_noesis_logical_child(void* element, uint32_t index) {
    if (!element) return nullptr;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe || index >= Noesis::LogicalTreeHelper::GetChildrenCount(fe)) return nullptr;
    // GetChild returns a Ptr<BaseComponent> already at +1. The local Ptr
    // releases at scope end, so AddReference() the raw pointer here to leave
    // the caller a net +1 after the Ptr destructs.
    Noesis::Ptr<Noesis::BaseComponent> child = Noesis::LogicalTreeHelper::GetChild(fe, index);
    if (!child) return nullptr;
    child->AddReference();
    return child.GetPtr();
}

extern "C" void* dm_noesis_framework_element_template_child(void* element, const char* name) {
    if (!element || !name) return nullptr;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return nullptr;
    // GetTemplateChild returns a non-owning raw pointer — AddReference() to
    // hand the caller a +1, matching the rest of this surface.
    Noesis::BaseComponent* child = fe->GetTemplateChild(name);
    if (!child) return nullptr;
    child->AddReference();
    return child;
}

// ── HorizontalAlignment / VerticalAlignment (TODO §2.E) ─────────────────────
//
// A bespoke path: the generic INT32 tag won't match the enum's reflected Type,
// so go through the FrameworkElement accessors directly. Values mirror
// `Noesis::HorizontalAlignment` / `VerticalAlignment` (Left/Center/Right/
// Stretch, Top/Center/Bottom/Stretch — 0..=3). Getters return -1 if `element`
// is not a FrameworkElement; setters no-op.

extern "C" void dm_noesis_framework_element_set_halign(void* element, int32_t value) {
    if (!element) return;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return;
    fe->SetHorizontalAlignment(static_cast<Noesis::HorizontalAlignment>(value));
}

extern "C" void dm_noesis_framework_element_set_valign(void* element, int32_t value) {
    if (!element) return;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return;
    fe->SetVerticalAlignment(static_cast<Noesis::VerticalAlignment>(value));
}

extern "C" int32_t dm_noesis_framework_element_get_halign(void* element) {
    if (!element) return -1;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return -1;
    return static_cast<int32_t>(fe->GetHorizontalAlignment());
}

extern "C" int32_t dm_noesis_framework_element_get_valign(void* element) {
    if (!element) return -1;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return -1;
    return static_cast<int32_t>(fe->GetVerticalAlignment());
}

// ── Thread affinity / DispatcherObject (TODO §2.G) ──────────────────────────
//
// Only the affinity queries are exposed: NsGui has no public BeginInvoke
// surface (cross-thread marshalling would need IView timers — TODO §1).

extern "C" bool dm_noesis_dependency_object_check_access(void* obj) {
    if (!obj) return false;
    auto* d = Noesis::DynamicCast<Noesis::DispatcherObject*>(
        static_cast<Noesis::BaseComponent*>(obj));
    if (!d) return false;
    return d->CheckAccess();
}

extern "C" uint32_t dm_noesis_dependency_object_thread_id(void* obj) {
    if (!obj) return UINT32_MAX;
    auto* d = Noesis::DynamicCast<Noesis::DispatcherObject*>(
        static_cast<Noesis::BaseComponent*>(obj));
    if (!d) return UINT32_MAX;
    return d->GetThreadId();
}
