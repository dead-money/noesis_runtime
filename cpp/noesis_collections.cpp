// Rust data into XAML: ObservableCollection, string boxing, DataContext and
// ItemsSource, plus visual/logical tree traversal, hit testing, NameScope,
// alignment, thread affinity, and CollectionView current-item navigation.
//
// ObservableCollection<BaseComponent> raises CollectionChanged on every
// mutation, so a bound ItemsControl regenerates its containers. Items are
// BaseComponent*: boxed values or view-model instances from
// noesis_classes.cpp.
//
// Returned component pointers are +1 (release via
// noesis_base_component_release) unless marked borrowed.

#include "noesis_shim.h"

#include <NsCore/Boxing.h>
#include <NsCore/BaseComponent.h>
#include <NsCore/Delegate.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Ptr.h>
#include <NsCore/Symbol.h>
#include <NsCore/TypeClass.h>
#include <NsCore/TypeOf.h>
#include <NsCore/TypeProperty.h>
#include <NsGui/DependencyObject.h>
#include <NsGui/DependencyProperty.h>
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

// Returns `c` with a +1 owned by the caller. When `c` came from a local Ptr,
// that Ptr releases its own reference.
void* handout(Noesis::BaseComponent* c) {
    if (!c) return nullptr;
    c->AddReference();
    return c;
}

using ObsColl = Noesis::ObservableCollection<Noesis::BaseComponent>;

ObsColl* as_collection(void* p) {
    if (!p) return nullptr;
    return Noesis::DynamicCast<ObsColl*>(static_cast<Noesis::BaseComponent*>(p));
}

}  // namespace

// ── Boxing ──────────────────────────────────────────────────────────────────

extern "C" void* noesis_box_string(const char* text) {
    // Copies `text`; the caller may free it afterwards.
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

// Single NotifyCollectionChangedAction.Move (not Remove+Add) so a bound control
// keeps the moved container's selection / scroll state.
extern "C" bool noesis_observable_collection_move(
    void* collection, uint32_t old_index, uint32_t new_index) {
    ObsColl* coll = as_collection(collection);
    if (!coll) return false;
    uint32_t count = (uint32_t)coll->Count();
    if (old_index >= count || new_index >= count) return false;
    coll->Move(old_index, new_index);
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

// Borrowed; null when out of range.
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
    // Null clears.
    fe->SetDataContext(static_cast<Noesis::BaseComponent*>(context));
    return true;
}

// Borrowed.
extern "C" void* noesis_framework_element_get_data_context(void* element) {
    if (!element) return nullptr;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    return fe ? fe->GetDataContext() : nullptr;
}

// Reads a uint64 field off `element`'s DataContext, e.g. a row id from the
// borrowed source element of a routed event. Takes no references. Tries a
// uint64 DP (ClassBuilder instances), then a reflected property boxing a
// uint64 (plain view models). False, with `*out` untouched, if neither exists.
extern "C" bool noesis_element_datacontext_get_u64(
    void* element, const char* prop_name, uint64_t* out) {
    if (!element || !prop_name || !out) return false;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return false;
    Noesis::BaseComponent* dc = fe->GetDataContext();
    if (!dc) return false;

    Noesis::Symbol sym(prop_name, Noesis::Symbol::NullIfNotFound());
    if (sym.IsNull()) return false;

    if (auto* d = Noesis::DynamicCast<Noesis::DependencyObject*>(dc)) {
        const Noesis::DependencyProperty* dp =
            Noesis::FindDependencyProperty(d->GetClassType(), sym);
        if (dp && dp->GetType() == Noesis::TypeOf<uint64_t>()) {
            *out = d->GetValue<uint64_t>(dp);
            return true;
        }
    }

    // Unbox before `boxed` drops.
    if (const auto* tc = Noesis::DynamicCast<const Noesis::TypeClass*>(dc->GetClassType())) {
        if (const Noesis::TypeProperty* prop = tc->FindProperty(sym)) {
            Noesis::Ptr<Noesis::BaseComponent> boxed = prop->GetComponent(dc);
            if (boxed && Noesis::Boxing::CanUnbox<uint64_t>(boxed.GetPtr())) {
                *out = Noesis::Boxing::Unbox<uint64_t>(boxed.GetPtr());
                return true;
            }
        }
    }

    return false;
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

// Count of the control's Items view. -1 if `element` is not an ItemsControl.
extern "C" int32_t noesis_items_control_items_count(void* element) {
    if (!element) return -1;
    auto* ic = Noesis::DynamicCast<Noesis::ItemsControl*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!ic) return -1;
    Noesis::ItemCollection* items = ic->GetItems();
    return items ? items->Count() : 0;
}

// Count of realized item containers. Unlike items_count, this changes only
// after the generator regenerates, so it shows whether collection-change
// notification reached the control. -1 if `element` is not an ItemsControl.
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
// Visual children may not be FrameworkElements; they are returned unfiltered
// so indexed traversal has no holes.

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

// `x`/`y` in `element`-local DIPs. Topmost hit (+1), or null.
extern "C" void* noesis_visual_hit_test(void* element, float x, float y) {
    if (!element) return nullptr;
    auto* v = Noesis::DynamicCast<Noesis::Visual*>(static_cast<Noesis::BaseComponent*>(element));
    if (!v) return nullptr;
    Noesis::HitTestResult result = Noesis::VisualTreeHelper::HitTest(v, Noesis::Point(x, y));
    if (!result.visualHit) return nullptr;
    result.visualHit->AddReference();
    return static_cast<Noesis::BaseComponent*>(result.visualHit);
}

// `filter` picks which branches to descend; `result` sees each hit and may stop
// the walk. Callback visuals are borrowed for that call only. Return codes are
// raw HitTestFilterBehavior / HitTestResultBehavior values.
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
//
// Relative pivot in 0..1. The getter writes 0 and the setter returns false when
// `element` is not a UIElement.

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

extern "C" void* noesis_name_scope_create() {
    Noesis::Ptr<Noesis::NameScope> scope = Noesis::MakePtr<Noesis::NameScope>();
    return scope.GiveOwnership();
}

// Null if the element has none or is not a DependencyObject.
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

// Null `scope` clears. False if `element` is not a DependencyObject.
extern "C" bool noesis_name_scope_set(void* element, void* scope) {
    if (!element) return false;
    auto* d = Noesis::DynamicCast<Noesis::DependencyObject*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!d) return false;
    Noesis::NameScope::SetNameScope(d, static_cast<Noesis::NameScope*>(scope));
    return true;
}

extern "C" void* noesis_name_scope_find_name(void* scope, const char* name) {
    if (!scope || !name) return nullptr;
    auto* s = Noesis::DynamicCast<Noesis::NameScope*>(
        static_cast<Noesis::BaseComponent*>(scope));
    if (!s) return nullptr;
    Noesis::BaseComponent* obj = s->FindName(name);
    if (!obj) return nullptr;
    obj->AddReference();
    return obj;
}

extern "C" void noesis_name_scope_register_name(void* scope, const char* name, void* obj) {
    if (!scope || !name || !obj) return;
    auto* s = Noesis::DynamicCast<Noesis::NameScope*>(
        static_cast<Noesis::BaseComponent*>(scope));
    if (!s) return;
    s->RegisterName(name, static_cast<Noesis::BaseComponent*>(obj));
}

extern "C" void noesis_name_scope_unregister_name(void* scope, const char* name) {
    if (!scope || !name) return;
    auto* s = Noesis::DynamicCast<Noesis::NameScope*>(
        static_cast<Noesis::BaseComponent*>(scope));
    if (!s) return;
    s->UnregisterName(name);
}

extern "C" void noesis_name_scope_update_name(void* scope, const char* name, void* obj) {
    if (!scope || !name || !obj) return;
    auto* s = Noesis::DynamicCast<Noesis::NameScope*>(
        static_cast<Noesis::BaseComponent*>(scope));
    if (!s) return;
    s->UpdateName(name, static_cast<Noesis::BaseComponent*>(obj));
}

// Borrowed from the scope; copy it before mutating the scope.
extern "C" const char* noesis_name_scope_find_object(void* scope, void* obj) {
    if (!scope || !obj) return nullptr;
    auto* s = Noesis::DynamicCast<Noesis::NameScope*>(
        static_cast<Noesis::BaseComponent*>(scope));
    if (!s) return nullptr;
    return s->FindObject(static_cast<Noesis::BaseComponent*>(obj));
}

// `cb` receives pointers borrowed for that call only.
extern "C" void noesis_name_scope_enum(
    void* scope, noesis_name_scope_enum_fn cb, void* userdata)
{
    if (!scope || !cb) return;
    auto* s = Noesis::DynamicCast<Noesis::NameScope*>(
        static_cast<Noesis::BaseComponent*>(scope));
    if (!s) return;
    struct Ctx {
        noesis_name_scope_enum_fn cb;
        void* userdata;
    } ctx{cb, userdata};
    s->EnumNamedObjects(
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
    // The local Ptr releases at scope end; AddReference leaves the caller +1.
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
    Noesis::BaseComponent* child = fe->GetTemplateChild(name);
    if (!child) return nullptr;
    child->AddReference();
    return child;
}

// ── HorizontalAlignment / VerticalAlignment ─────────────────────────────────
//
// Enum-typed DPs, so the generic Int32 path can't set them. Values are
// Left/Center/Right/Stretch and Top/Center/Bottom/Stretch (0..=3). Getters
// return -1 if `element` is not a FrameworkElement; setters no-op.

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
// Queries only: NsGui has no public BeginInvoke.

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
// Only current-item navigation and Refresh: the SDK has no programmatic
// SortDescription or Filter delegate.

namespace {

Noesis::CollectionView* as_collection_view(void* p) {
    if (!p) return nullptr;
    return Noesis::DynamicCast<Noesis::CollectionView*>(static_cast<Noesis::BaseComponent*>(p));
}

// Holds a +1 on the view and owns the Rust userdata box (freed in the
// destructor). Deletion is deferred while a callback is on the stack, so Rust
// may unsubscribe from inside its own callback. View thread only, no atomics.
class RustCurrentChangedHandler {
public:
    RustCurrentChangedHandler(noesis_collection_view_changed_fn cb, void* userdata,
                              noesis_subscription_free_fn free, Noesis::CollectionView* view)
        : mCb(cb), mUserdata(userdata), mFree(free), mView(view) {
        if (mView) mView->AddReference();
    }

    ~RustCurrentChangedHandler() {
        if (mView) mView->Release();
        if (mFree && mUserdata) mFree(mUserdata);
    }

    RustCurrentChangedHandler(const RustCurrentChangedHandler&) = delete;
    RustCurrentChangedHandler& operator=(const RustCurrentChangedHandler&) = delete;

    void OnChanged(Noesis::BaseComponent* /*sender*/, const Noesis::EventArgs& /*args*/) {
        mDispatchDepth++;
        if (mCb) mCb(mUserdata);
        // Outermost frame only: the callback may re-raise CurrentChanged.
        if (--mDispatchDepth == 0 && mPendingDelete) {
            delete this;
        }
    }

    // True: OnChanged's outermost frame deletes; the caller must not.
    bool deferDeleteIfDispatching() {
        if (mDispatchDepth > 0) {
            mPendingDelete = true;
            return true;
        }
        return false;
    }

    Noesis::CollectionView* view() const { return mView; }

private:
    noesis_collection_view_changed_fn mCb;
    void* mUserdata;
    noesis_subscription_free_fn mFree;
    Noesis::CollectionView* mView;
    uint32_t mDispatchDepth = 0;
    bool mPendingDelete = false;
};

}  // namespace

extern "C" void* noesis_collection_view_source_create(void) {
    Noesis::Ptr<Noesis::CollectionViewSource> cvs = *new Noesis::CollectionViewSource();
    return handout(cvs.GetPtr());
}

// `source` is borrowed; null clears.
extern "C" bool noesis_collection_view_source_set_source(void* cvs, void* source) {
    auto* s = Noesis::DynamicCast<Noesis::CollectionViewSource*>(
        static_cast<Noesis::BaseComponent*>(cvs));
    if (!s) return false;
    s->SetSource(static_cast<Noesis::BaseComponent*>(source));
    return true;
}

// Null if `cvs` has no list source. A code-built CollectionViewSource that was
// never hosted in a tree leaves GetView() null, so a CollectionView is built
// over the source list instead; navigation behaves the same.
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

// -1 if `view` is not a CollectionView.
extern "C" int32_t noesis_collection_view_count(void* view) {
    Noesis::CollectionView* cv = as_collection_view(view);
    return cv ? cv->Count() : -1;
}

// -1 is before first, Count is after last; INT32_MIN if not a CollectionView.
extern "C" int32_t noesis_collection_view_current_position(void* view) {
    Noesis::CollectionView* cv = as_collection_view(view);
    return cv ? cv->CurrentPosition() : INT32_MIN;
}

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

extern "C" void noesis_collection_view_refresh(void* view) {
    Noesis::CollectionView* cv = as_collection_view(view);
    if (cv) cv->Refresh();
}

// Returns a token for noesis_collection_view_unsubscribe_current_changed, or
// null if `view` is not a CollectionView or `cb` is null.
extern "C" void* noesis_collection_view_subscribe_current_changed(
    void* view, noesis_collection_view_changed_fn cb, void* userdata,
    noesis_subscription_free_fn free_handler) {
    Noesis::CollectionView* cv = as_collection_view(view);
    if (!cv || !cb) return nullptr;
    auto* handler = new RustCurrentChangedHandler(cb, userdata, free_handler, cv);
    cv->CurrentChanged() += Noesis::MakeDelegate(handler, &RustCurrentChangedHandler::OnChanged);
    return handler;
}

extern "C" void noesis_collection_view_unsubscribe_current_changed(void* token) {
    if (!token) return;
    auto* handler = static_cast<RustCurrentChangedHandler*>(token);
    if (auto* cv = handler->view()) {
        cv->CurrentChanged() -= Noesis::MakeDelegate(handler, &RustCurrentChangedHandler::OnChanged);
    }
    if (handler->deferDeleteIfDispatching()) return;
    delete handler;
}
