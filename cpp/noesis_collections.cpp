// Data-binding bridge: ObservableCollection + boxing + DataContext / ItemsSource
// wiring.
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
//     most common item is a string; `noesis_box_string` wraps a C string
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
#include <NsCore/Delegate.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Ptr.h>
#include <NsDrawing/Point.h>
#include <NsGui/CollectionView.h>
#include <NsGui/CollectionViewSource.h>
#include <NsGui/DispatcherObject.h>
#include <NsGui/Enums.h>
#include <NsGui/Events.h>
#include <NsGui/FrameworkElement.h>
#include <NsGui/ICollectionView.h>
#include <NsGui/IList.h>
#include <NsGui/ItemCollection.h>
#include <NsGui/ItemContainerGenerator.h>
#include <NsGui/ItemsControl.h>
#include <NsGui/LogicalTreeHelper.h>
#include <NsGui/ObservableCollection.h>
#include <NsGui/INameScope.h>
#include <NsGui/NameScope.h>
#include <NsGui/UIElement.h>
#include <NsGui/Visual.h>
#include <NsGui/VisualTreeHelper.h>

namespace {

// Hand a freshly-created (or borrowed) BaseComponent out across the C ABI with
// exactly one reference owned by the caller, balanced by
// `noesis_base_component_release`. Safe to call on a refcount-0 `new`'d
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

extern "C" void* noesis_box_string(const char* text) {
    // Box(const char*) copies the bytes into a Noesis::String inside a
    // BoxedValue<String>; the caller's `text` can go away after this call.
    Noesis::Ptr<Noesis::BoxedValue> boxed = Noesis::Boxing::Box(text ? text : "");
    return handout(boxed.GetPtr());
}

// ── ObservableCollection<BaseComponent> ─────────────────────────────────────

extern "C" void* noesis_observable_collection_create(void) {
    Noesis::Ptr<ObsColl> coll = *new ObsColl();
    return handout(coll.GetPtr());
}

extern "C" int32_t noesis_observable_collection_add(void* collection, void* item) {
    ObsColl* coll = as_collection(collection);
    if (!coll) return -1;
    return coll->Add(static_cast<Noesis::BaseComponent*>(item));
}

extern "C" bool noesis_observable_collection_insert(
    void* collection, uint32_t index, void* item) {
    ObsColl* coll = as_collection(collection);
    if (!coll || index > (uint32_t)coll->Count()) return false;
    coll->Insert(index, static_cast<Noesis::BaseComponent*>(item));
    return true;
}

extern "C" bool noesis_observable_collection_set(
    void* collection, uint32_t index, void* item) {
    ObsColl* coll = as_collection(collection);
    if (!coll || index >= (uint32_t)coll->Count()) return false;
    coll->Set(index, static_cast<Noesis::BaseComponent*>(item));
    return true;
}

extern "C" bool noesis_observable_collection_remove_at(void* collection, uint32_t index) {
    ObsColl* coll = as_collection(collection);
    if (!coll || index >= (uint32_t)coll->Count()) return false;
    coll->RemoveAt(index);
    return true;
}

extern "C" void noesis_observable_collection_clear(void* collection) {
    ObsColl* coll = as_collection(collection);
    if (coll) coll->Clear();
}

extern "C" int32_t noesis_observable_collection_count(void* collection) {
    ObsColl* coll = as_collection(collection);
    return coll ? coll->Count() : -1;
}

// Borrowed (no +1) pointer to the item at `index`, or null. The collection
// owns the reference; copy / AddReference if you need to keep it.
extern "C" void* noesis_observable_collection_get(void* collection, uint32_t index) {
    ObsColl* coll = as_collection(collection);
    if (!coll || index >= (uint32_t)coll->Count()) return nullptr;
    return coll->Get(index);
}

// ── DataContext ─────────────────────────────────────────────────────────────

extern "C" bool noesis_framework_element_set_data_context(void* element, void* context) {
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
extern "C" void* noesis_framework_element_get_data_context(void* element) {
    if (!element) return nullptr;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    return fe ? fe->GetDataContext() : nullptr;
}

// ── ItemsControl.ItemsSource + container introspection ──────────────────────

extern "C" bool noesis_items_control_set_items_source(void* element, void* items) {
    if (!element) return false;
    auto* ic = Noesis::DynamicCast<Noesis::ItemsControl*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!ic) return false;
    ic->SetItemsSource(static_cast<Noesis::BaseComponent*>(items));
    return true;
}

// Number of items the ItemsControl currently sees (its `Items` view over the
// bound ItemsSource). -1 if `element` is not an ItemsControl.
extern "C" int32_t noesis_items_control_items_count(void* element) {
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
extern "C" int32_t noesis_items_control_realized_count(void* element) {
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

// ── Visual / logical tree traversal ─────────────────────────────────────────
//
// VisualTreeHelper operates on `Visual*` — children may be plain Visuals, not
// FrameworkElements, so these return raw +1 BaseComponent* handles without
// null-filtering non-FE nodes (filtering would punch holes in indexed
// traversal). The Rust `FrameworkElement` handle is just an owned
// BaseComponent* whose FE-specific methods DynamicCast internally, so handing
// back a Visual* is fine. All owning returns AddReference() once for the
// caller (matching `find_name`); the Rust drop releases.

extern "C" uint32_t noesis_visual_children_count(void* element) {
    if (!element) return 0;
    auto* v = Noesis::DynamicCast<Noesis::Visual*>(static_cast<Noesis::BaseComponent*>(element));
    if (!v) return 0;
    return Noesis::VisualTreeHelper::GetChildrenCount(v);
}

extern "C" void* noesis_visual_child(void* element, uint32_t index) {
    if (!element) return nullptr;
    auto* v = Noesis::DynamicCast<Noesis::Visual*>(static_cast<Noesis::BaseComponent*>(element));
    if (!v || index >= Noesis::VisualTreeHelper::GetChildrenCount(v)) return nullptr;
    Noesis::Visual* child = Noesis::VisualTreeHelper::GetChild(v, index);
    if (!child) return nullptr;
    child->AddReference();
    return static_cast<Noesis::BaseComponent*>(child);
}

extern "C" void* noesis_visual_parent(void* element) {
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
extern "C" void* noesis_visual_hit_test(void* element, float x, float y) {
    if (!element) return nullptr;
    auto* v = Noesis::DynamicCast<Noesis::Visual*>(static_cast<Noesis::BaseComponent*>(element));
    if (!v) return nullptr;
    Noesis::HitTestResult result = Noesis::VisualTreeHelper::HitTest(v, Noesis::Point(x, y));
    if (!result.visualHit) return nullptr;
    result.visualHit->AddReference();
    return static_cast<Noesis::BaseComponent*>(result.visualHit);
}

// Filtered hit test — the callback overload of VisualTreeHelper::HitTest. As the
// tree is walked, `filter` is invoked for each visual (its return selects which
// branches to descend), and `result` for each hit (its return continues or
// stops the walk). The visual pointers handed to the callbacks are BORROWED and
// valid only for that call; Rust AddRef's (via base_component_add_reference) if
// it wants to keep one. Return codes are the raw Noesis enum values.
namespace {
struct HitTestBridge {
    noesis_hit_filter_fn filter;
    noesis_hit_result_fn result;
    void* userdata;

    Noesis::HitTestFilterBehavior OnFilter(Noesis::Visual* target) {
        if (!filter) return Noesis::HitTestFilterBehavior_Continue;
        return static_cast<Noesis::HitTestFilterBehavior>(
            filter(userdata, static_cast<Noesis::BaseComponent*>(target)));
    }
    Noesis::HitTestResultBehavior OnResult(const Noesis::HitTestResult& r) {
        if (!result) return Noesis::HitTestResultBehavior_Continue;
        return static_cast<Noesis::HitTestResultBehavior>(
            result(userdata, static_cast<Noesis::BaseComponent*>(r.visualHit)));
    }
};
}  // namespace

extern "C" void noesis_visual_hit_test_filtered(
    void* element, float x, float y, noesis_hit_filter_fn filter,
    noesis_hit_result_fn result, void* userdata)
{
    if (!element || !result) return;
    auto* v = Noesis::DynamicCast<Noesis::Visual*>(static_cast<Noesis::BaseComponent*>(element));
    if (!v) return;
    HitTestBridge bridge{filter, result, userdata};
    Noesis::VisualTreeHelper::HitTest(
        v, Noesis::Point(x, y),
        Noesis::MakeDelegate(&bridge, &HitTestBridge::OnFilter),
        Noesis::MakeDelegate(&bridge, &HitTestBridge::OnResult));
}

extern "C" void* noesis_framework_element_logical_parent(void* element) {
    if (!element) return nullptr;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return nullptr;
    Noesis::FrameworkElement* parent = fe->GetParent();
    if (!parent) return nullptr;
    parent->AddReference();
    return static_cast<Noesis::BaseComponent*>(parent);
}

// ── RenderTransform origin ──────────────────────────────────────────────────
// UIElement::Get/SetRenderTransformOrigin — the (0..1, 0..1) relative pivot the
// RenderTransform rotates/scales around. `out_x`/`out_y` are written 0 when the
// element is not a UIElement; the setter is a no-op then.

extern "C" void noesis_ui_element_get_render_transform_origin(
    void* element, float* out_x, float* out_y)
{
    if (out_x) *out_x = 0.0f;
    if (out_y) *out_y = 0.0f;
    if (!element) return;
    auto* ui = Noesis::DynamicCast<Noesis::UIElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!ui) return;
    const Noesis::Point& p = ui->GetRenderTransformOrigin();
    if (out_x) *out_x = p.x;
    if (out_y) *out_y = p.y;
}

extern "C" bool noesis_ui_element_set_render_transform_origin(
    void* element, float x, float y)
{
    if (!element) return false;
    auto* ui = Noesis::DynamicCast<Noesis::UIElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!ui) return false;
    ui->SetRenderTransformOrigin(Noesis::Point(x, y));
    return true;
}

// ── Standalone NameScope ────────────────────────────────────────────────────
// The freestanding NameScope object, distinct from the per-FrameworkElement
// RegisterName path. All component pointers handed back are +1 (release via
// noesis_base_component_release).

// Create an empty NameScope (+1).
extern "C" void* noesis_name_scope_create() {
    Noesis::Ptr<Noesis::NameScope> scope = Noesis::MakePtr<Noesis::NameScope>();
    return scope.GiveOwnership();
}

// Attached NameScope on `element` (NameScope::GetNameScope), +1, or NULL if the
// element carries none / is not a DependencyObject.
extern "C" void* noesis_name_scope_get(void* element) {
    if (!element) return nullptr;
    auto* d = Noesis::DynamicCast<Noesis::DependencyObject*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!d) return nullptr;
    Noesis::NameScope* scope = Noesis::NameScope::GetNameScope(d);
    if (!scope) return nullptr;
    scope->AddReference();
    return static_cast<Noesis::BaseComponent*>(scope);
}

// Attach `scope` (may be NULL to clear) as `element`'s NameScope. Returns false
// if `element` is not a DependencyObject.
extern "C" bool noesis_name_scope_set(void* element, void* scope) {
    if (!element) return false;
    auto* d = Noesis::DynamicCast<Noesis::DependencyObject*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!d) return false;
    Noesis::NameScope::SetNameScope(d, static_cast<Noesis::NameScope*>(scope));
    return true;
}

// INameScope operations on a NameScope*. find_name returns +1 or NULL.
extern "C" void* noesis_name_scope_find_name(void* scope, const char* name) {
    if (!scope || !name) return nullptr;
    auto* s = static_cast<Noesis::NameScope*>(scope);
    Noesis::BaseComponent* obj = s->FindName(name);
    if (!obj) return nullptr;
    obj->AddReference();
    return obj;
}

extern "C" void noesis_name_scope_register_name(void* scope, const char* name, void* obj) {
    if (!scope || !name || !obj) return;
    static_cast<Noesis::NameScope*>(scope)->RegisterName(
        name, static_cast<Noesis::BaseComponent*>(obj));
}

extern "C" void noesis_name_scope_unregister_name(void* scope, const char* name) {
    if (!scope || !name) return;
    static_cast<Noesis::NameScope*>(scope)->UnregisterName(name);
}

extern "C" void noesis_name_scope_update_name(void* scope, const char* name, void* obj) {
    if (!scope || !name || !obj) return;
    static_cast<Noesis::NameScope*>(scope)->UpdateName(
        name, static_cast<Noesis::BaseComponent*>(obj));
}

// Reverse lookup: the registered name of `obj`, or NULL. The returned pointer is
// owned by the NameScope (borrowed); copy it out before mutating the scope.
extern "C" const char* noesis_name_scope_find_object(void* scope, void* obj) {
    if (!scope || !obj) return nullptr;
    return static_cast<Noesis::NameScope*>(scope)->FindObject(
        static_cast<Noesis::BaseComponent*>(obj));
}

// Enumerate every (name, object) pair. `cb` receives borrowed pointers valid
// only for that call. No-op on NULL scope/cb.
extern "C" void noesis_name_scope_enum(
    void* scope, noesis_name_scope_enum_fn cb, void* userdata)
{
    if (!scope || !cb) return;
    struct Ctx {
        noesis_name_scope_enum_fn cb;
        void* userdata;
    } ctx{cb, userdata};
    static_cast<Noesis::NameScope*>(scope)->EnumNamedObjects(
        [](const char* name, Noesis::BaseComponent* obj, void* ud) {
            auto* c = static_cast<Ctx*>(ud);
            c->cb(c->userdata, name, obj);
        },
        &ctx);
}

extern "C" uint32_t noesis_logical_children_count(void* element) {
    if (!element) return 0;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return 0;
    return Noesis::LogicalTreeHelper::GetChildrenCount(fe);
}

extern "C" void* noesis_logical_child(void* element, uint32_t index) {
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

extern "C" void* noesis_framework_element_template_child(void* element, const char* name) {
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

// ── HorizontalAlignment / VerticalAlignment ─────────────────────────────────
//
// A bespoke path: the generic INT32 tag won't match the enum's reflected Type,
// so go through the FrameworkElement accessors directly. Values mirror
// `Noesis::HorizontalAlignment` / `VerticalAlignment` (Left/Center/Right/
// Stretch, Top/Center/Bottom/Stretch — 0..=3). Getters return -1 if `element`
// is not a FrameworkElement; setters no-op.

extern "C" void noesis_framework_element_set_halign(void* element, int32_t value) {
    if (!element) return;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return;
    fe->SetHorizontalAlignment(static_cast<Noesis::HorizontalAlignment>(value));
}

extern "C" void noesis_framework_element_set_valign(void* element, int32_t value) {
    if (!element) return;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return;
    fe->SetVerticalAlignment(static_cast<Noesis::VerticalAlignment>(value));
}

extern "C" int32_t noesis_framework_element_get_halign(void* element) {
    if (!element) return -1;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return -1;
    return static_cast<int32_t>(fe->GetHorizontalAlignment());
}

extern "C" int32_t noesis_framework_element_get_valign(void* element) {
    if (!element) return -1;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return -1;
    return static_cast<int32_t>(fe->GetVerticalAlignment());
}

// ── Thread affinity / DispatcherObject ──────────────────────────────────────
//
// Only the affinity queries are exposed: NsGui has no public BeginInvoke
// surface (cross-thread marshalling would need IView timers).

extern "C" bool noesis_dependency_object_check_access(void* obj) {
    if (!obj) return false;
    auto* d = Noesis::DynamicCast<Noesis::DispatcherObject*>(
        static_cast<Noesis::BaseComponent*>(obj));
    if (!d) return false;
    return d->CheckAccess();
}

extern "C" uint32_t noesis_dependency_object_thread_id(void* obj) {
    if (!obj) return UINT32_MAX;
    auto* d = Noesis::DynamicCast<Noesis::DispatcherObject*>(
        static_cast<Noesis::BaseComponent*>(obj));
    if (!d) return UINT32_MAX;
    return d->GetThreadId();
}

// ── ICollectionView current-item navigation ──────────────────────────────────
//
// A CollectionViewSource wraps a source list and lazily produces a
// CollectionView (an ICollectionView) over it. The view tracks a *current item*
// — the record-management surface WPF/Noesis controls (Selector etc.) bind to.
// Sort/filter/group remain a real SDK limitation (no programmatic SortDescription
// /Filter delegate), so only current-item navigation + Refresh are exposed.

namespace {

Noesis::CollectionView* as_collection_view(void* p) {
    if (!p) return nullptr;
    return Noesis::DynamicCast<Noesis::CollectionView*>(static_cast<Noesis::BaseComponent*>(p));
}

// Adapter between CollectionView::CurrentChanged() (an EventHandler, i.e.
// Delegate<void(BaseComponent*, const EventArgs&)>) and the C ABI callback.
// Holds a +1 ref on the view so the subscription stays valid; `+=` in subscribe
// is balanced by `-=` in unsubscribe.
class RustCurrentChangedHandler {
public:
    RustCurrentChangedHandler(noesis_collection_view_changed_fn cb, void* userdata,
                              Noesis::CollectionView* view)
        : mCb(cb), mUserdata(userdata), mView(view) {
        if (mView) mView->AddReference();
    }

    ~RustCurrentChangedHandler() {
        if (mView) mView->Release();
    }

    RustCurrentChangedHandler(const RustCurrentChangedHandler&) = delete;
    RustCurrentChangedHandler& operator=(const RustCurrentChangedHandler&) = delete;

    void OnChanged(Noesis::BaseComponent* /*sender*/, const Noesis::EventArgs& /*args*/) {
        if (mCb) mCb(mUserdata);
    }

    Noesis::CollectionView* view() const { return mView; }

private:
    noesis_collection_view_changed_fn mCb;
    void* mUserdata;
    Noesis::CollectionView* mView;  // raw + manual AddRef/Release — see ctor/dtor.
};

}  // namespace

// Create an empty CollectionViewSource (+1 ref for the caller).
extern "C" void* noesis_collection_view_source_create(void) {
    Noesis::Ptr<Noesis::CollectionViewSource> cvs = *new Noesis::CollectionViewSource();
    return handout(cvs.GetPtr());
}

// Point the source at `source` (a borrowed list, e.g. an ObservableCollection);
// the CollectionViewSource (re)builds its view. Pass null to clear. false if
// `cvs` is not a CollectionViewSource.
extern "C" bool noesis_collection_view_source_set_source(void* cvs, void* source) {
    auto* s = Noesis::DynamicCast<Noesis::CollectionViewSource*>(
        static_cast<Noesis::BaseComponent*>(cvs));
    if (!s) return false;
    s->SetSource(static_cast<Noesis::BaseComponent*>(source));
    return true;
}

// +1-owned (AddRef'd) CollectionView currently associated with `cvs`
// (CollectionViewSource::GetView), or null if `cvs` is not a CollectionViewSource
// / has no source. Set a Source first.
//
// A CollectionViewSource only eagerly materializes its ViewProperty once it is
// hosted (XAML-parsed / initialized in a tree); a standalone code-built one
// leaves GetView() null. So when GetView() is null we build a CollectionView
// directly over the source list (which is exactly what the hosted path would
// produce) — the current-item navigation surface is identical either way.
extern "C" void* noesis_collection_view_source_get_view(void* cvs) {
    auto* s = Noesis::DynamicCast<Noesis::CollectionViewSource*>(
        static_cast<Noesis::BaseComponent*>(cvs));
    if (!s) return nullptr;
    if (Noesis::CollectionView* v = s->GetView()) return handout(v);
    auto* list = Noesis::DynamicCast<Noesis::IList*>(s->GetSource());
    if (!list) return nullptr;
    Noesis::Ptr<Noesis::CollectionView> cv = *new Noesis::CollectionView(list);
    return cv.GiveOwnership();
}

// Number of records in the view, or -1 if `view` is not a CollectionView.
extern "C" int32_t noesis_collection_view_count(void* view) {
    Noesis::CollectionView* cv = as_collection_view(view);
    return cv ? cv->Count() : -1;
}

// Ordinal position of the CurrentItem, or INT32_MIN if not a CollectionView.
// (Noesis uses -1 for "before first" and Count for "after last".)
extern "C" int32_t noesis_collection_view_current_position(void* view) {
    Noesis::CollectionView* cv = as_collection_view(view);
    return cv ? cv->CurrentPosition() : INT32_MIN;
}

// +1-owned (AddRef'd) CurrentItem, or null if there is none / not a view.
extern "C" void* noesis_collection_view_current_item(void* view) {
    Noesis::CollectionView* cv = as_collection_view(view);
    if (!cv) return nullptr;
    Noesis::Ptr<Noesis::BaseComponent> item = cv->CurrentItem();
    return handout(item.GetPtr());
}

extern "C" bool noesis_collection_view_is_current_before_first(void* view) {
    Noesis::CollectionView* cv = as_collection_view(view);
    return cv ? cv->IsCurrentBeforeFirst() : false;
}

extern "C" bool noesis_collection_view_is_current_after_last(void* view) {
    Noesis::CollectionView* cv = as_collection_view(view);
    return cv ? cv->IsCurrentAfterLast() : false;
}

extern "C" bool noesis_collection_view_move_current_to_first(void* view) {
    Noesis::CollectionView* cv = as_collection_view(view);
    return cv ? cv->MoveCurrentToFirst() : false;
}

extern "C" bool noesis_collection_view_move_current_to_last(void* view) {
    Noesis::CollectionView* cv = as_collection_view(view);
    return cv ? cv->MoveCurrentToLast() : false;
}

extern "C" bool noesis_collection_view_move_current_to_next(void* view) {
    Noesis::CollectionView* cv = as_collection_view(view);
    return cv ? cv->MoveCurrentToNext() : false;
}

extern "C" bool noesis_collection_view_move_current_to_previous(void* view) {
    Noesis::CollectionView* cv = as_collection_view(view);
    return cv ? cv->MoveCurrentToPrevious() : false;
}

extern "C" bool noesis_collection_view_move_current_to_position(void* view, int32_t position) {
    Noesis::CollectionView* cv = as_collection_view(view);
    return cv ? cv->MoveCurrentToPosition(position) : false;
}

// Recreate the view (ICollectionView::Refresh).
extern "C" void noesis_collection_view_refresh(void* view) {
    Noesis::CollectionView* cv = as_collection_view(view);
    if (cv) cv->Refresh();
}

// Subscribe `cb` to the view's CurrentChanged event. Returns an opaque handler
// token (release via noesis_collection_view_unsubscribe_current_changed), or
// null on a non-CollectionView handle / null cb.
extern "C" void* noesis_collection_view_subscribe_current_changed(
    void* view, noesis_collection_view_changed_fn cb, void* userdata) {
    Noesis::CollectionView* cv = as_collection_view(view);
    if (!cv || !cb) return nullptr;
    auto* handler = new RustCurrentChangedHandler(cb, userdata, cv);
    cv->CurrentChanged() += Noesis::MakeDelegate(handler, &RustCurrentChangedHandler::OnChanged);
    return handler;
}

extern "C" void noesis_collection_view_unsubscribe_current_changed(void* token) {
    if (!token) return;
    auto* handler = static_cast<RustCurrentChangedHandler*>(token);
    if (auto* cv = handler->view()) {
        cv->CurrentChanged() -= Noesis::MakeDelegate(handler, &RustCurrentChangedHandler::OnChanged);
    }
    delete handler;
}
