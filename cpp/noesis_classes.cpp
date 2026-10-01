// Custom XAML classes defined from Rust, plus generic DependencyProperty
// access by name.
//
// C++ types can't be created at runtime, so each registered class gets a
// synthetic TypeClassBuilder and a Factory creator that instantiates a
// per-base trampoline subclass whose GetClassType() reports the synthetic
// class.
//
//   * Trampolines (RustContentControl ... RustDecorator via RUST_TRAMPOLINE,
//     plus RustFreezable) forward DP changes, layout, and OnRender to Rust.
//   * RustClassInstance is mixed into every trampoline; g_instance_map maps
//     the canonical BaseComponent* to it across the multiple-inheritance
//     pointer adjustment.
//   * ClassData holds per-class state: TypeClass, DP metadata, registered
//     properties, and the donated Rust callbacks.
//   * g_class_registry maps a Symbol to ClassData for the Factory creator.

#include "noesis_shim.h"

#include <NsCore/Noesis.h>
#include <NsCore/Boxing.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Factory.h>
#include <NsCore/HashMap.h>
#include <NsCore/Ptr.h>
#include <NsCore/Reflection.h>
#include <NsCore/ReflectionImplement.h>
#include <NsCore/String.h>
#include <NsCore/Symbol.h>
#include <NsCore/TypeClass.h>
#include <NsCore/TypeClassBuilder.h>
#include <NsCore/TypeEnum.h>
#include <NsCore/TypeOf.h>
#include <NsDrawing/Color.h>
#include <NsDrawing/Point.h>
#include <NsDrawing/Rect.h>
#include <NsDrawing/Size.h>
#include <NsDrawing/Thickness.h>
#include <NsMath/Vector.h>
#include <NsGui/ContentControl.h>
#include <NsGui/Control.h>
#include <NsGui/Decorator.h>
#include <NsGui/DependencyData.h>
#include <NsGui/DependencyObject.h>
#include <NsGui/DrawingContext.h>
#include <NsGui/DependencyProperty.h>
#include <NsGui/Freezable.h>
#include <NsGui/FrameworkElement.h>
#include <NsGui/FrameworkPropertyMetadata.h>
#include <NsGui/ImageSource.h>
#include <NsGui/Panel.h>
#include <NsGui/PropertyMetadata.h>
#include <NsGui/UIElement.h>
#include <NsGui/UIElementData.h>
#include <NsGui/UserControl.h>

#include <array>
#include <atomic>
#include <cstring>
#include <mutex>
#include <unordered_map>
#include <utility>
#include <vector>

namespace {

struct PropEntry {
    const Noesis::DependencyProperty* dp;
    noesis_prop_type type;
    bool read_only;
};

// Refcounted: the Rust registration holds the initial 1, each live instance
// holds one more. At zero the donated Rust boxes (property, coerce, layout,
// render) are freed, so callbacks fired during instance destruction still see
// live userdata. ClassData, typeClass, and uiData are never freed: the
// destructor chain of the last instance walks typeClass after Release returns.
struct ClassData {
    Noesis::String                      name;
    Noesis::Symbol                      sym;
    noesis_class_base                base;
    // Owned by the Reflection registry.
    Noesis::TypeClassBuilder*           typeClass;
    // UIElementData for UIElement bases, plain DependencyData for Freezable.
    Noesis::Ptr<Noesis::DependencyData>  uiData;
    std::vector<PropEntry>              properties;
    noesis_prop_changed_fn           cb;
    void*                               userdata;
    noesis_class_free_fn             free_handler;
    // Applies only to DPs registered with coerce enabled.
    noesis_coerce_fn                 coerce_cb;
    void*                               coerce_userdata;
    noesis_class_free_fn             coerce_free;
    bool                                has_layout;
    noesis_layout_vtable             layout;
    void*                               layout_userdata;
    noesis_layout_free_fn            layout_free;
    noesis_render_fn                 render_cb;
    void*                               render_userdata;
    noesis_render_free_fn            render_free;
    std::atomic<int>                    ref_count;

    ClassData()
        : base(NOESIS_BASE_CONTENT_CONTROL)
        , typeClass(nullptr)
        , cb(nullptr)
        , userdata(nullptr)
        , free_handler(nullptr)
        , coerce_cb(nullptr)
        , coerce_userdata(nullptr)
        , coerce_free(nullptr)
        , has_layout(false)
        , layout{}
        , layout_userdata(nullptr)
        , layout_free(nullptr)
        , render_cb(nullptr)
        , render_userdata(nullptr)
        , render_free(nullptr)
        , ref_count(1) {}

    void AddRef() noexcept {
        ref_count.fetch_add(1, std::memory_order_relaxed);
    }

    // Nulls each pointer before freeing so a second call is a no-op.
    void FreeDonated() {
        void* ud = userdata;
        userdata = nullptr;
        if (free_handler && ud) free_handler(ud);

        void* cu = coerce_userdata;
        coerce_userdata = nullptr;
        if (coerce_free && cu) coerce_free(cu);

        void* lu = layout_userdata;
        layout_userdata = nullptr;
        if (layout_free && lu) layout_free(lu);

        void* ru = render_userdata;
        render_userdata = nullptr;
        if (render_free && ru) render_free(ru);
    }

    void Release() {
        if (ref_count.fetch_sub(1, std::memory_order_acq_rel) == 1) {
            std::atomic_thread_fence(std::memory_order_acquire);
            // Never delete typeClass, uiData, or `this` here: this runs from
            // ~RustClassInstance, and the base destructors that follow still
            // walk typeClass. Bounded leak of one ClassData per class.
            FreeDonated();
        }
    }
};

std::mutex                                       g_registry_mutex;
std::unordered_map<uint32_t, ClassData*>         g_class_registry;

// Every ClassData ever registered, for the shutdown sweep.
std::mutex                                       g_all_class_data_mutex;
std::vector<ClassData*>                          g_all_class_data;

void track_class_data(ClassData* cd) {
    std::lock_guard<std::mutex> lock(g_all_class_data_mutex);
    g_all_class_data.push_back(cd);
}

ClassData* registry_find(Noesis::Symbol sym) {
    std::lock_guard<std::mutex> lock(g_registry_mutex);
    auto it = g_class_registry.find((uint32_t)sym);
    return it == g_class_registry.end() ? nullptr : it->second;
}

bool registry_insert(Noesis::Symbol sym, ClassData* cd) {
    std::lock_guard<std::mutex> lock(g_registry_mutex);
    return g_class_registry.emplace((uint32_t)sym, cd).second;
}

void registry_erase(Noesis::Symbol sym) {
    std::lock_guard<std::mutex> lock(g_registry_mutex);
    g_class_registry.erase((uint32_t)sym);
}

// ── Instance map ────────────────────────────────────────────────────────────
//
// The canonical BaseComponent* (the FFI address; the Noesis base is the first
// base of every trampoline) to its RustClassInstance subobject, which sits at
// a non-zero offset.

struct RustClassInstance;

std::mutex                                       g_instance_mutex;
std::unordered_map<void*, RustClassInstance*>    g_instance_map;

void instance_map_insert(void* canonical, RustClassInstance* iface) {
    std::lock_guard<std::mutex> lock(g_instance_mutex);
    g_instance_map[canonical] = iface;
}

void instance_map_erase(void* canonical) {
    std::lock_guard<std::mutex> lock(g_instance_mutex);
    g_instance_map.erase(canonical);
}

RustClassInstance* instance_lookup(void* canonical) {
    std::lock_guard<std::mutex> lock(g_instance_mutex);
    auto it = g_instance_map.find(canonical);
    return it == g_instance_map.end() ? nullptr : it->second;
}

// ── RustClassInstance interface ─────────────────────────────────────────────
//
// Second base of every trampoline, after the Noesis base. RustSetReadOnly is
// virtual because protected SetReadOnlyProperty is reachable only from the
// concrete class.
struct RustClassInstance {
    virtual ~RustClassInstance() {
        if (mCanonical) {
            instance_map_erase(mCanonical);
            mCanonical = nullptr;
        }
        if (mClassData) {
            mClassData->Release();
            mClassData = nullptr;
        }
    }

    virtual void RustSetReadOnly(const Noesis::DependencyProperty* dp,
        noesis_prop_type type, const void* value_ptr) = 0;

    // Once per instance, from make_trampoline. The ClassData ref taken here is
    // released in the destructor.
    void BindClassData(ClassData* cd, Noesis::BaseComponent* canonical,
                       Noesis::DependencyObject* dobj) {
        mClassData = cd;
        mCanonical = canonical;
        mDO = dobj;
        if (cd) cd->AddRef();
        if (canonical) instance_map_insert(canonical, this);
    }

    ClassData* GetClassData() const { return mClassData; }
    Noesis::DependencyObject* GetDO() const { return mDO; }
    void* GetCanonical() const { return mCanonical; }

protected:
    ClassData* mClassData = nullptr;
    Noesis::BaseComponent* mCanonical = nullptr;
    Noesis::DependencyObject* mDO = nullptr;
};

// ── Marshaling helpers ─────────────────────────────────────────────────────

void invoke_cb(ClassData* cd, void* instance, uint32_t idx,
               noesis_prop_type ty, const void* raw) {
    if (!cd->cb) return;

    switch (ty) {
        case NOESIS_PROP_INT32:
        case NOESIS_PROP_UINT32:
        case NOESIS_PROP_UINT64:
        case NOESIS_PROP_FLOAT:
        case NOESIS_PROP_DOUBLE:
        case NOESIS_PROP_BOOL:
        case NOESIS_PROP_THICKNESS:
        case NOESIS_PROP_COLOR:
        case NOESIS_PROP_RECT:
        case NOESIS_PROP_POINT:
        case NOESIS_PROP_SIZE:
        case NOESIS_PROP_VECTOR:
        case NOESIS_PROP_ENUM:
            // `raw` already points at the value (POINT/SIZE/VECTOR: float[2]).
            cd->cb(cd->userdata, instance, idx, raw);
            return;

        case NOESIS_PROP_STRING: {
            const auto* s = static_cast<const Noesis::String*>(raw);
            const char* c = s ? s->Str() : nullptr;
            cd->cb(cd->userdata, instance, idx, &c);
            return;
        }

        case NOESIS_PROP_IMAGE_SOURCE:
        case NOESIS_PROP_BASE_COMPONENT: {
            // `raw` points at a Ptr<BaseComponent>; pass the borrowed pointer.
            const auto* p = static_cast<const Noesis::Ptr<Noesis::BaseComponent>*>(raw);
            Noesis::BaseComponent* b = p ? p->GetPtr() : nullptr;
            cd->cb(cd->userdata, instance, idx, &b);
            return;
        }
    }
}

// Templated on the trampoline so the protected SetReadOnlyProperty (re-exposed
// with `using`) is reachable. Object and ImageSource tags no-op: there is no
// boxed SetReadOnly form.
template<class Obj>
void apply_set_readonly(Obj* obj, const Noesis::DependencyProperty* dp,
                        noesis_prop_type type, const void* value_ptr) {
    using namespace Noesis;
    switch (type) {
        case NOESIS_PROP_INT32:
            obj->template SetReadOnlyProperty<int32_t>(
                dp, value_ptr ? *static_cast<const int32_t*>(value_ptr) : 0);
            return;
        case NOESIS_PROP_UINT32:
            obj->template SetReadOnlyProperty<uint32_t>(
                dp, value_ptr ? *static_cast<const uint32_t*>(value_ptr) : 0);
            return;
        case NOESIS_PROP_UINT64:
            obj->template SetReadOnlyProperty<uint64_t>(
                dp, value_ptr ? *static_cast<const uint64_t*>(value_ptr) : 0);
            return;
        case NOESIS_PROP_FLOAT:
            obj->template SetReadOnlyProperty<float>(
                dp, value_ptr ? *static_cast<const float*>(value_ptr) : 0.0f);
            return;
        case NOESIS_PROP_DOUBLE:
            obj->template SetReadOnlyProperty<double>(
                dp, value_ptr ? *static_cast<const double*>(value_ptr) : 0.0);
            return;
        case NOESIS_PROP_BOOL:
            obj->template SetReadOnlyProperty<bool>(
                dp, value_ptr ? *static_cast<const bool*>(value_ptr) : false);
            return;
        case NOESIS_PROP_STRING: {
            const char* s = value_ptr ? *static_cast<const char* const*>(value_ptr) : nullptr;
            obj->template SetReadOnlyProperty<String>(dp, s ? s : "");
            return;
        }
        case NOESIS_PROP_THICKNESS: {
            Thickness t;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                t = Thickness(f[0], f[1], f[2], f[3]);
            }
            obj->template SetReadOnlyProperty<Thickness>(dp, t);
            return;
        }
        case NOESIS_PROP_COLOR: {
            Color c;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                c = Color(f[0], f[1], f[2], f[3]);
            }
            obj->template SetReadOnlyProperty<Color>(dp, c);
            return;
        }
        case NOESIS_PROP_RECT: {
            Rect r;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                r = Rect(f[0], f[1], f[0] + f[2], f[1] + f[3]);
            }
            obj->template SetReadOnlyProperty<Rect>(dp, r);
            return;
        }
        case NOESIS_PROP_POINT: {
            Point p;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                p = Point(f[0], f[1]);
            }
            obj->template SetReadOnlyProperty<Point>(dp, p);
            return;
        }
        case NOESIS_PROP_SIZE: {
            Size s;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                s = Size(f[0], f[1]);
            }
            obj->template SetReadOnlyProperty<Size>(dp, s);
            return;
        }
        case NOESIS_PROP_VECTOR: {
            Vector2 v;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                v = Vector2(f[0], f[1]);
            }
            obj->template SetReadOnlyProperty<Vector2>(dp, v);
            return;
        }
        case NOESIS_PROP_ENUM:
            obj->template SetReadOnlyProperty<int32_t>(
                dp, value_ptr ? *static_cast<const int32_t*>(value_ptr) : 0);
            return;
        case NOESIS_PROP_IMAGE_SOURCE:
        case NOESIS_PROP_BASE_COMPONENT:
            return;
    }
}

void rust_forward_change(RustClassInstance* self,
                         const Noesis::DependencyPropertyChangedEventArgs& args) {
    ClassData* cd = self->GetClassData();
    if (!cd) return;
    for (uint32_t i = 0; i < cd->properties.size(); ++i) {
        const auto& pe = cd->properties[i];
        if (pe.dp == args.prop) {
            invoke_cb(cd, self->GetCanonical(), i, pe.type, args.newValue);
            return;
        }
    }
}

// False when no handler is installed, so the trampoline uses the base layout.
bool rust_run_measure(RustClassInstance* self, const Noesis::Size& avail, Noesis::Size* out) {
    ClassData* cd = self->GetClassData();
    if (!cd || !cd->has_layout || !cd->layout.measure) return false;
    float w = 0.0f, h = 0.0f;
    cd->layout.measure(cd->layout_userdata, self->GetCanonical(),
                       avail.width, avail.height, &w, &h);
    *out = Noesis::Size(w, h);
    return true;
}

bool rust_run_arrange(RustClassInstance* self, const Noesis::Size& final_, Noesis::Size* out) {
    ClassData* cd = self->GetClassData();
    if (!cd || !cd->has_layout || !cd->layout.arrange) return false;
    float w = 0.0f, h = 0.0f;
    cd->layout.arrange(cd->layout_userdata, self->GetCanonical(),
                       final_.width, final_.height, &w, &h);
    *out = Noesis::Size(w, h);
    return true;
}

// The DrawingContext is passed borrowed; Rust drives it through
// noesis_drawing_*.
void rust_run_render(RustClassInstance* self, Noesis::DrawingContext* dc) {
    ClassData* cd = self->GetClassData();
    if (!cd || !cd->render_cb) return;
    cd->render_cb(cd->render_userdata, self->GetCanonical(),
                  static_cast<Noesis::BaseComponent*>(dc));
}

// ── Trampoline subclasses ───────────────────────────────────────────────────
//
// Hand-rolled reflection: NS_IMPLEMENT_REFLECTION's GetClassType() always
// returns the static type, but instances must report their synthetic
// TypeClass for `Style TargetType="my:Foo"` and bindings to work. Reuses
// TypeClassCreator::Create / Fill so TypeOf<>, RegisterType and
// IsAssignableFrom behave normally.
//
// Every RUST_TRAMPOLINE base derives from FrameworkElement.

#define RUST_TRAMPOLINE(ClassName, NsBase, ReflName)                                       \
class ClassName final : public NsBase, public RustClassInstance {                              \
public:                                                                                        \
    ClassName() = default;                                                                     \
    /* Re-expose the protected read-only setter so apply_set_readonly reaches it. */           \
    using Noesis::DependencyObject::SetReadOnlyProperty;                                       \
    void RustSetReadOnly(const Noesis::DependencyProperty* dp,                                 \
        noesis_prop_type type, const void* value_ptr) override {                            \
        apply_set_readonly(this, dp, type, value_ptr);                                         \
    }                                                                                          \
    static const Noesis::TypeClass* StaticGetClassType(Noesis::TypeTag<ClassName>*);           \
    const Noesis::TypeClass* GetClassType() const override {                                   \
        if (mClassData && mClassData->typeClass)                                               \
            return static_cast<const Noesis::TypeClass*>(mClassData->typeClass);               \
        return StaticGetClassType((Noesis::TypeTag<ClassName>*)nullptr);                       \
    }                                                                                          \
protected:                                                                                     \
    bool OnPropertyChanged(const Noesis::DependencyPropertyChangedEventArgs& args) override {  \
        bool processed = NsBase::OnPropertyChanged(args);                                      \
        rust_forward_change(this, args);                                                       \
        return processed;                                                                      \
    }                                                                                          \
    Noesis::Size MeasureOverride(const Noesis::Size& availableSize) override {                 \
        Noesis::Size out;                                                                      \
        if (rust_run_measure(this, availableSize, &out)) return out;                           \
        return NsBase::MeasureOverride(availableSize);                                         \
    }                                                                                          \
    Noesis::Size ArrangeOverride(const Noesis::Size& finalSize) override {                     \
        Noesis::Size out;                                                                      \
        if (rust_run_arrange(this, finalSize, &out)) return out;                               \
        return NsBase::ArrangeOverride(finalSize);                                             \
    }                                                                                          \
    void OnRender(Noesis::DrawingContext* drawingContext) override {                            \
        NsBase::OnRender(drawingContext);                                                      \
        rust_run_render(this, drawingContext);                                                \
    }                                                                                          \
private:                                                                                       \
    typedef ClassName SelfClass;                                                               \
    typedef NsBase ParentClass;                                                                \
    friend class Noesis::TypeClassCreator;                                                     \
    static void StaticFillClassType(Noesis::TypeClassCreator&) {}                              \
};                                                                                             \
const Noesis::TypeClass* ClassName::StaticGetClassType(Noesis::TypeTag<ClassName>*) {           \
    static const Noesis::TypeClass* type;                                                      \
    if (NS_UNLIKELY(type == 0)) {                                                              \
        type = static_cast<const Noesis::TypeClass*>(Noesis::Reflection::RegisterType(         \
            ReflName, Noesis::TypeClassCreator::Create<ClassName>,                             \
            Noesis::TypeClassCreator::Fill<ClassName, NsBase>));                               \
    }                                                                                          \
    return type;                                                                               \
}

RUST_TRAMPOLINE(RustContentControl, Noesis::ContentControl, "DmNoesis.RustContentControl")
RUST_TRAMPOLINE(RustControl, Noesis::Control, "DmNoesis.RustControl")
RUST_TRAMPOLINE(RustFrameworkElement, Noesis::FrameworkElement, "DmNoesis.RustFrameworkElement")
RUST_TRAMPOLINE(RustUserControl, Noesis::UserControl, "DmNoesis.RustUserControl")
RUST_TRAMPOLINE(RustPanel, Noesis::Panel, "DmNoesis.RustPanel")
RUST_TRAMPOLINE(RustDecorator, Noesis::Decorator, "DmNoesis.RustDecorator")

#undef RUST_TRAMPOLINE

Noesis::BaseComponent* make_trampoline(ClassData* cd);

// Not a UIElement: DP change forwarding only, plus CreateInstanceCore. Brush,
// Geometry, Transform and Effect add pure IRenderProxyCreator virtuals and
// can't be subclassed this way (see LIMITATIONS.md).
class RustFreezable final : public Noesis::Freezable, public RustClassInstance {
public:
    RustFreezable() = default;
    using Noesis::DependencyObject::SetReadOnlyProperty;
    void RustSetReadOnly(const Noesis::DependencyProperty* dp,
        noesis_prop_type type, const void* value_ptr) override {
        apply_set_readonly(this, dp, type, value_ptr);
    }
    static const Noesis::TypeClass* StaticGetClassType(Noesis::TypeTag<RustFreezable>*);
    const Noesis::TypeClass* GetClassType() const override {
        if (mClassData && mClassData->typeClass)
            return static_cast<const Noesis::TypeClass*>(mClassData->typeClass);
        return StaticGetClassType((Noesis::TypeTag<RustFreezable>*)nullptr);
    }

protected:
    bool OnPropertyChanged(const Noesis::DependencyPropertyChangedEventArgs& args) override {
        bool processed = Freezable::OnPropertyChanged(args);
        rust_forward_change(this, args);
        return processed;
    }
    // Used by Clone / GetAsFrozen; the copy shares this ClassData.
    Noesis::Ptr<Noesis::Freezable> CreateInstanceCore() const override {
        if (mClassData) {
            Noesis::BaseComponent* bc = make_trampoline(mClassData);
            auto* fz = Noesis::DynamicCast<Noesis::Freezable*>(bc);
            // Adopt make_trampoline's +1.
            if (fz) return Noesis::Ptr<Noesis::Freezable>(*fz);
        }
        return Noesis::Ptr<Noesis::Freezable>(*new RustFreezable());
    }

private:
    typedef RustFreezable SelfClass;
    typedef Noesis::Freezable ParentClass;
    friend class Noesis::TypeClassCreator;
    static void StaticFillClassType(Noesis::TypeClassCreator&) {}
};
const Noesis::TypeClass* RustFreezable::StaticGetClassType(Noesis::TypeTag<RustFreezable>*) {
    static const Noesis::TypeClass* type;
    if (NS_UNLIKELY(type == 0)) {
        type = static_cast<const Noesis::TypeClass*>(Noesis::Reflection::RegisterType(
            "DmNoesis.RustFreezable", Noesis::TypeClassCreator::Create<RustFreezable>,
            Noesis::TypeClassCreator::Fill<RustFreezable, Noesis::Freezable>));
    }
    return type;
}

// Parent of the synthetic TypeClass.
const Noesis::TypeClass* base_static_type(noesis_class_base base) {
    using namespace Noesis;
    switch (base) {
        case NOESIS_BASE_CONTENT_CONTROL:
            return RustContentControl::StaticGetClassType((TypeTag<RustContentControl>*)nullptr);
        case NOESIS_BASE_CONTROL:
            return RustControl::StaticGetClassType((TypeTag<RustControl>*)nullptr);
        case NOESIS_BASE_FRAMEWORK_ELEMENT:
            return RustFrameworkElement::StaticGetClassType((TypeTag<RustFrameworkElement>*)nullptr);
        case NOESIS_BASE_USER_CONTROL:
            return RustUserControl::StaticGetClassType((TypeTag<RustUserControl>*)nullptr);
        case NOESIS_BASE_PANEL:
            return RustPanel::StaticGetClassType((TypeTag<RustPanel>*)nullptr);
        case NOESIS_BASE_DECORATOR:
            return RustDecorator::StaticGetClassType((TypeTag<RustDecorator>*)nullptr);
        case NOESIS_BASE_FREEZABLE:
            return RustFreezable::StaticGetClassType((TypeTag<RustFreezable>*)nullptr);
    }
    return nullptr;
}

bool base_supported(noesis_class_base base) {
    switch (base) {
        case NOESIS_BASE_CONTENT_CONTROL:
        case NOESIS_BASE_CONTROL:
        case NOESIS_BASE_FRAMEWORK_ELEMENT:
        case NOESIS_BASE_USER_CONTROL:
        case NOESIS_BASE_PANEL:
        case NOESIS_BASE_DECORATOR:
        case NOESIS_BASE_FREEZABLE:
            return true;
    }
    return false;
}

bool base_is_uielement(noesis_class_base base) {
    return base != NOESIS_BASE_FREEZABLE;
}

// Returns the canonical BaseComponent* carrying the caller's +1 (the initial
// refcount); adopt it, don't AddRef.
Noesis::BaseComponent* make_trampoline(ClassData* cd) {
    Noesis::BaseComponent* canonical = nullptr;
    Noesis::DependencyObject* dobj = nullptr;
    RustClassInstance* iface = nullptr;

    switch (cd->base) {
        case NOESIS_BASE_CONTENT_CONTROL: {
            auto* o = new RustContentControl(); canonical = o; dobj = o; iface = o; break;
        }
        case NOESIS_BASE_CONTROL: {
            auto* o = new RustControl(); canonical = o; dobj = o; iface = o; break;
        }
        case NOESIS_BASE_FRAMEWORK_ELEMENT: {
            auto* o = new RustFrameworkElement(); canonical = o; dobj = o; iface = o; break;
        }
        case NOESIS_BASE_USER_CONTROL: {
            auto* o = new RustUserControl(); canonical = o; dobj = o; iface = o; break;
        }
        case NOESIS_BASE_PANEL: {
            auto* o = new RustPanel(); canonical = o; dobj = o; iface = o; break;
        }
        case NOESIS_BASE_DECORATOR: {
            auto* o = new RustDecorator(); canonical = o; dobj = o; iface = o; break;
        }
        case NOESIS_BASE_FREEZABLE: {
            auto* o = new RustFreezable(); canonical = o; dobj = o; iface = o; break;
        }
        default:
            return nullptr;
    }

    iface->BindClassData(cd, canonical, dobj);
    return canonical;
}

// ── Factory creator ────────────────────────────────────────────────────────

Noesis::BaseComponent* class_creator(Noesis::Symbol name) {
    ClassData* cd = registry_find(name);
    if (!cd) return nullptr;
    return make_trampoline(cd);
}

// ── Coerce thunk pool ───────────────────────────────────────────────────────
//
// CoerceValueCallback gets no DP pointer, so each coerced DP gets a thunk that
// hard-codes its property index. Only the first kCoerceSlots properties can be
// coerced.

constexpr uint32_t kCoerceSlots = 32;

size_t coercible_size(noesis_prop_type t) {
    switch (t) {
        case NOESIS_PROP_INT32:
        case NOESIS_PROP_UINT32:
        case NOESIS_PROP_FLOAT:
        case NOESIS_PROP_ENUM:
            return 4;
        case NOESIS_PROP_DOUBLE:
            return 8;
        case NOESIS_PROP_BOOL:
            return sizeof(bool);
        case NOESIS_PROP_POINT:
        case NOESIS_PROP_SIZE:
        case NOESIS_PROP_VECTOR:
            return sizeof(float) * 2;
        case NOESIS_PROP_THICKNESS:
        case NOESIS_PROP_COLOR:
        case NOESIS_PROP_RECT:
            return sizeof(float) * 4;
        default:
            return 0;  // string / object tags are not coercible
    }
}

bool rust_coerce_dispatch(const Noesis::DependencyObject* d, uint32_t slot,
                          const void* baseValue, void* coercedValue) {
    // `coercedValue` arrives uninitialized and is used as the effective value
    // when we return true, so copy `baseValue` in first; a pass-through then
    // yields the unchanged value. Size 0 (string / object) is default-
    // constructed by Noesis and safe to leave alone.
    RustClassInstance* iface =
        instance_lookup((void*)static_cast<const Noesis::BaseComponent*>(d));
    ClassData* cd = iface ? iface->GetClassData() : nullptr;
    size_t sz = (cd && slot < cd->properties.size())
                    ? coercible_size(cd->properties[slot].type)
                    : 0;

    if (sz != 0 && baseValue && coercedValue) {
        std::memcpy(coercedValue, baseValue, sz);
        if (cd->coerce_cb) {
            cd->coerce_cb(cd->coerce_userdata,
                          (void*)static_cast<const Noesis::BaseComponent*>(d),
                          slot, baseValue, coercedValue);
        }
    }
    return true;
}

template<uint32_t Slot>
bool coerce_thunk(const Noesis::DependencyObject* d, const void* baseValue, void* coercedValue) {
    return rust_coerce_dispatch(d, Slot, baseValue, coercedValue);
}

template<uint32_t... I>
std::array<Noesis::CoerceValueCallback, sizeof...(I)>
make_coerce_table(std::integer_sequence<uint32_t, I...>) {
    return { Noesis::CoerceValueCallback(&coerce_thunk<I>)... };
}

Noesis::CoerceValueCallback coerce_callback_for(uint32_t slot) {
    static const std::array<Noesis::CoerceValueCallback, kCoerceSlots> table =
        make_coerce_table(std::make_integer_sequence<uint32_t, kCoerceSlots>{});
    if (slot >= kCoerceSlots) return Noesis::CoerceValueCallback();
    return table[slot];
}

// ── DP metadata + creation ──────────────────────────────────────────────────

template<class T>
Noesis::Ptr<Noesis::PropertyMetadata> make_md(
    const T& def, uint32_t options, const Noesis::CoerceValueCallback& coerce) {
    using namespace Noesis;
    if (options != 0) {
        if (coerce) return FrameworkPropertyMetadata::Create<T>(def, options, coerce);
        return FrameworkPropertyMetadata::Create<T>(def, options);
    }
    if (coerce) return PropertyMetadata::Create<T>(def, coerce);
    return PropertyMetadata::Create<T>(def);
}

template<class T>
Noesis::Ptr<Noesis::PropertyMetadata> make_md_obj(const T& def, uint32_t options) {
    using namespace Noesis;
    if (options != 0) return FrameworkPropertyMetadata::Create<T>(def, options);
    return PropertyMetadata::Create<T>(def);
}

// `coerce` is honored only for coercible tags.
Noesis::Ptr<Noesis::DependencyProperty> create_dp_ex(
    const char* name,
    const Noesis::TypeClass* owner,
    noesis_prop_type type,
    const void* default_ptr,
    uint32_t options,
    const Noesis::CoerceValueCallback& coerce,
    bool read_only) {
    using namespace Noesis;
    PropertyAccess access = read_only ? PropertyAccess_ReadOnly : PropertyAccess_ReadWrite;
    switch (type) {
        case NOESIS_PROP_INT32: {
            int32_t def = default_ptr ? *static_cast<const int32_t*>(default_ptr) : 0;
            return DependencyProperty::Create<int32_t>(
                name, owner, make_md<int32_t>(def, options, coerce).GetPtr(), nullptr, access);
        }
        case NOESIS_PROP_UINT32: {
            uint32_t def = default_ptr ? *static_cast<const uint32_t*>(default_ptr) : 0;
            return DependencyProperty::Create<uint32_t>(
                name, owner, make_md<uint32_t>(def, options, coerce).GetPtr(), nullptr, access);
        }
        case NOESIS_PROP_UINT64: {
            uint64_t def = default_ptr ? *static_cast<const uint64_t*>(default_ptr) : 0;
            return DependencyProperty::Create<uint64_t>(
                name, owner, make_md<uint64_t>(def, options, coerce).GetPtr(), nullptr, access);
        }
        case NOESIS_PROP_FLOAT: {
            float def = default_ptr ? *static_cast<const float*>(default_ptr) : 0.0f;
            return DependencyProperty::Create<float>(
                name, owner, make_md<float>(def, options, coerce).GetPtr(), nullptr, access);
        }
        case NOESIS_PROP_DOUBLE: {
            double def = default_ptr ? *static_cast<const double*>(default_ptr) : 0.0;
            return DependencyProperty::Create<double>(
                name, owner, make_md<double>(def, options, coerce).GetPtr(), nullptr, access);
        }
        case NOESIS_PROP_BOOL: {
            bool def = default_ptr ? *static_cast<const bool*>(default_ptr) : false;
            return DependencyProperty::Create<bool>(
                name, owner, make_md<bool>(def, options, coerce).GetPtr(), nullptr, access);
        }
        case NOESIS_PROP_STRING: {
            const char* def = default_ptr ? *static_cast<const char* const*>(default_ptr) : nullptr;
            String s = def ? String(def) : String();
            return DependencyProperty::Create<String>(
                name, owner, make_md<String>(s, options, coerce).GetPtr(), nullptr, access);
        }
        case NOESIS_PROP_THICKNESS: {
            Thickness def;
            if (default_ptr) {
                const auto* f = static_cast<const float*>(default_ptr);
                def = Thickness(f[0], f[1], f[2], f[3]);
            }
            return DependencyProperty::Create<Thickness>(
                name, owner, make_md<Thickness>(def, options, coerce).GetPtr(), nullptr, access);
        }
        case NOESIS_PROP_COLOR: {
            Color def;
            if (default_ptr) {
                const auto* f = static_cast<const float*>(default_ptr);
                def = Color(f[0], f[1], f[2], f[3]);
            }
            return DependencyProperty::Create<Color>(
                name, owner, make_md<Color>(def, options, coerce).GetPtr(), nullptr, access);
        }
        case NOESIS_PROP_RECT: {
            Rect def;
            if (default_ptr) {
                const auto* f = static_cast<const float*>(default_ptr);
                def = Rect(f[0], f[1], f[0] + f[2], f[1] + f[3]);
            }
            return DependencyProperty::Create<Rect>(
                name, owner, make_md<Rect>(def, options, coerce).GetPtr(), nullptr, access);
        }
        case NOESIS_PROP_POINT: {
            Point def;
            if (default_ptr) {
                const auto* f = static_cast<const float*>(default_ptr);
                def = Point(f[0], f[1]);
            }
            return DependencyProperty::Create<Point>(
                name, owner, make_md<Point>(def, options, coerce).GetPtr(), nullptr, access);
        }
        case NOESIS_PROP_SIZE: {
            Size def;
            if (default_ptr) {
                const auto* f = static_cast<const float*>(default_ptr);
                def = Size(f[0], f[1]);
            }
            return DependencyProperty::Create<Size>(
                name, owner, make_md<Size>(def, options, coerce).GetPtr(), nullptr, access);
        }
        case NOESIS_PROP_VECTOR: {
            Vector2 def;
            if (default_ptr) {
                const auto* f = static_cast<const float*>(default_ptr);
                def = Vector2(f[0], f[1]);
            }
            return DependencyProperty::Create<Vector2>(
                name, owner, make_md<Vector2>(def, options, coerce).GetPtr(), nullptr, access);
        }
        case NOESIS_PROP_ENUM:
            // Enums need a TypeEnum; see create_enum_dp.
            return nullptr;
        case NOESIS_PROP_IMAGE_SOURCE: {
            // Explicit null default: without one the typed Init path crashes
            // during a visual-tree walk.
            Ptr<BaseComponent> null_default;
            return DependencyProperty::Create<Ptr<BaseComponent>>(
                name, TypeOf<ImageSource>(), owner,
                make_md_obj<Ptr<BaseComponent>>(null_default, options).GetPtr(), nullptr, access);
        }
        case NOESIS_PROP_BASE_COMPONENT: {
            Ptr<BaseComponent> null_default;
            return DependencyProperty::Create<Ptr<BaseComponent>>(
                name, TypeOf<BaseComponent>(), owner,
                make_md_obj<Ptr<BaseComponent>>(null_default, options).GetPtr(), nullptr, access);
        }
    }
    return nullptr;
}

// int32 storage with the reflected Type set to the TypeEnum named
// `enum_type_name`. Null if that name is not a registered enum.
Noesis::Ptr<Noesis::DependencyProperty> create_enum_dp(
    const char* name,
    const Noesis::TypeClass* owner,
    const char* enum_type_name,
    int32_t default_value,
    uint32_t options,
    bool read_only) {
    using namespace Noesis;
    if (!enum_type_name) return nullptr;
    Symbol sym(enum_type_name, Symbol::NullIfNotFound());
    if (sym.IsNull()) return nullptr;
    const auto* enumType = DynamicCast<const TypeEnum*>(Reflection::GetType(sym));
    if (!enumType) return nullptr;

    PropertyAccess access = read_only ? PropertyAccess_ReadOnly : PropertyAccess_ReadWrite;
    auto md = make_md<int32_t>(default_value, options, CoerceValueCallback());
    return DependencyProperty::Create<int32_t>(
        name, enumType, owner, md.GetPtr(), nullptr, access);
}

}  // namespace

extern "C" void* noesis_class_register(
    const char* name,
    noesis_class_base base,
    noesis_prop_changed_fn cb,
    void* userdata,
    noesis_class_free_fn free_handler) {
    if (!name) return nullptr;
    if (!base_supported(base)) return nullptr;

    Noesis::Symbol sym = Noesis::Symbol(name);

    // A duplicate would leave the Factory creator resolving a stale ClassData.
    if (Noesis::Reflection::IsTypeRegistered(sym)) {
        return nullptr;
    }

    auto* cd = new ClassData();
    cd->name = name;
    cd->sym = sym;
    cd->base = base;
    cd->cb = cb;
    cd->userdata = userdata;
    cd->free_handler = free_handler;

    // Reflection owns the TypeClassBuilder once registered.
    cd->typeClass = new Noesis::TypeClassBuilder(sym, /*isInterface*/ false);
    cd->typeClass->AddBase(base_static_type(base));

    if (base_is_uielement(base)) {
        cd->uiData = Noesis::MakePtr<Noesis::UIElementData>(cd->typeClass);
    } else {
        cd->uiData = Noesis::MakePtr<Noesis::DependencyData>(cd->typeClass);
    }
    cd->typeClass->AddMeta(cd->uiData.GetPtr());

    Noesis::Reflection::RegisterType(cd->typeClass);
    Noesis::Factory::RegisterComponent(sym, Noesis::Symbol(""), class_creator);

    if (!registry_insert(sym, cd)) {
        // No instances exist yet, so freeing ClassData is safe here.
        Noesis::Factory::UnregisterComponent(sym);
        Noesis::Reflection::Unregister(cd->typeClass);
        cd->FreeDonated();
        delete cd;
        return nullptr;
    }

    track_class_data(cd);
    return cd;
}

// Returns +1, released via noesis_base_component_release.
extern "C" void* noesis_class_create_instance(void* class_token) {
    if (!class_token) return nullptr;
    auto* cd = static_cast<ClassData*>(class_token);
    Noesis::BaseComponent* instance = make_trampoline(cd);
    if (!instance) return nullptr;
    return instance;
}

extern "C" uint32_t noesis_class_register_property_ex(
    void* class_token,
    const char* prop_name,
    noesis_prop_type prop_type,
    const void* default_ptr,
    uint32_t fpm_options,
    bool read_only,
    bool coerce) {
    if (!class_token || !prop_name) return UINT32_MAX;
    auto* cd = static_cast<ClassData*>(class_token);

    uint32_t index = static_cast<uint32_t>(cd->properties.size());

    bool coerce_eff = coerce && coercible_size(prop_type) != 0;
    if (coerce_eff && index >= kCoerceSlots) return UINT32_MAX;
    Noesis::CoerceValueCallback cc =
        coerce_eff ? coerce_callback_for(index) : Noesis::CoerceValueCallback();

    auto dp = create_dp_ex(prop_name, cd->typeClass, prop_type, default_ptr,
                           fpm_options, cc, read_only);
    if (!dp) return UINT32_MAX;

    const Noesis::DependencyProperty* installed = cd->uiData->InsertProperty(dp.GetPtr());
    if (!installed) return UINT32_MAX;

    cd->properties.push_back({installed, prop_type, read_only});
    return index;
}

extern "C" uint32_t noesis_class_register_property(
    void* class_token,
    const char* prop_name,
    noesis_prop_type prop_type,
    const void* default_ptr) {
    return noesis_class_register_property_ex(
        class_token, prop_name, prop_type, default_ptr, /*options*/ 0,
        /*read_only*/ false, /*coerce*/ false);
}

extern "C" uint32_t noesis_class_register_enum_property(
    void* class_token,
    const char* prop_name,
    const char* enum_type_name,
    int32_t default_value,
    uint32_t fpm_options,
    bool read_only) {
    if (!class_token || !prop_name) return UINT32_MAX;
    auto* cd = static_cast<ClassData*>(class_token);

    uint32_t index = static_cast<uint32_t>(cd->properties.size());

    auto dp = create_enum_dp(prop_name, cd->typeClass, enum_type_name, default_value,
                             fpm_options, read_only);
    if (!dp) return UINT32_MAX;

    const Noesis::DependencyProperty* installed = cd->uiData->InsertProperty(dp.GetPtr());
    if (!installed) return UINT32_MAX;

    cd->properties.push_back({installed, NOESIS_PROP_ENUM, read_only});
    return index;
}

extern "C" void noesis_class_set_coerce(
    void* class_token,
    noesis_coerce_fn cb,
    void* userdata,
    noesis_class_free_fn free_handler) {
    if (!class_token) {
        if (free_handler && userdata) free_handler(userdata);
        return;
    }
    auto* cd = static_cast<ClassData*>(class_token);
    if (cd->coerce_free && cd->coerce_userdata) cd->coerce_free(cd->coerce_userdata);
    cd->coerce_cb = cb;
    cd->coerce_userdata = userdata;
    cd->coerce_free = free_handler;
    if (!cb && free_handler && userdata) {
        // Detaching: free the donated box now.
        free_handler(userdata);
        cd->coerce_userdata = nullptr;
        cd->coerce_free = nullptr;
    }
}

extern "C" void noesis_class_set_layout(
    void* class_token,
    const noesis_layout_vtable* vtable,
    void* userdata,
    noesis_layout_free_fn free_handler) {
    if (!class_token) {
        if (free_handler && userdata) free_handler(userdata);
        return;
    }
    auto* cd = static_cast<ClassData*>(class_token);
    if (cd->layout_free && cd->layout_userdata) cd->layout_free(cd->layout_userdata);
    cd->layout_userdata = nullptr;
    cd->layout_free = nullptr;
    cd->has_layout = false;
    if (vtable) {
        cd->layout = *vtable;
        cd->layout_userdata = userdata;
        cd->layout_free = free_handler;
        cd->has_layout = true;
    } else if (free_handler && userdata) {
        free_handler(userdata);
    }
}

extern "C" void noesis_class_set_render(
    void* class_token,
    noesis_render_fn cb,
    void* userdata,
    noesis_render_free_fn free_handler) {
    if (!class_token) {
        if (free_handler && userdata) free_handler(userdata);
        return;
    }
    auto* cd = static_cast<ClassData*>(class_token);
    if (cd->render_free && cd->render_userdata) cd->render_free(cd->render_userdata);
    cd->render_cb = cb;
    cd->render_userdata = userdata;
    cd->render_free = free_handler;
    if (!cb && free_handler && userdata) {
        // Detaching: free the donated box now.
        free_handler(userdata);
        cd->render_userdata = nullptr;
        cd->render_free = nullptr;
    }
}

extern "C" void noesis_class_unregister(void* class_token) {
    if (!class_token) return;
    auto* cd = static_cast<ClassData*>(class_token);

    // Stops new instances; live ones keep their ClassData reference.
    Noesis::Factory::UnregisterComponent(cd->sym);
    registry_erase(cd->sym);

    cd->Release();
}

// Called after Noesis::Shutdown. Frees donated boxes whose refcount never
// reached zero; ClassData itself stays leaked.
extern "C" void noesis_classes_force_free_at_shutdown(void) {
    std::vector<ClassData*> all;
    {
        std::lock_guard<std::mutex> lock(g_all_class_data_mutex);
        all = std::move(g_all_class_data);
    }
    for (ClassData* cd : all) {
        cd->FreeDonated();
    }
}

namespace {

// Null if `instance` is not a registered-class instance or the index is out
// of range.
const PropEntry* instance_prop(void* instance, uint32_t prop_index,
                               ClassData** out_cd, Noesis::DependencyObject** out_do) {
    RustClassInstance* iface = instance_lookup(instance);
    if (!iface) return nullptr;
    ClassData* cd = iface->GetClassData();
    if (!cd || prop_index >= cd->properties.size()) return nullptr;
    if (out_cd) *out_cd = cd;
    if (out_do) *out_do = iface->GetDO();
    return &cd->properties[prop_index];
}

enum class SetMode { Local, Current };

enum class GetMode { Effective, Base };

void apply_set(
    Noesis::DependencyObject* obj,
    const Noesis::DependencyProperty* dp,
    noesis_prop_type type,
    const void* value_ptr,
    SetMode mode = SetMode::Local) {
    using namespace Noesis;
    switch (type) {
        case NOESIS_PROP_INT32: {
            int32_t v = value_ptr ? *static_cast<const int32_t*>(value_ptr) : 0;
            if (mode == SetMode::Current) obj->SetCurrentValue<int32_t>(dp, v);
            else obj->SetValue<int32_t>(dp, v);
            return;
        }
        case NOESIS_PROP_UINT32: {
            uint32_t v = value_ptr ? *static_cast<const uint32_t*>(value_ptr) : 0;
            if (mode == SetMode::Current) obj->SetCurrentValue<uint32_t>(dp, v);
            else obj->SetValue<uint32_t>(dp, v);
            return;
        }
        case NOESIS_PROP_UINT64: {
            uint64_t v = value_ptr ? *static_cast<const uint64_t*>(value_ptr) : 0;
            if (mode == SetMode::Current) obj->SetCurrentValue<uint64_t>(dp, v);
            else obj->SetValue<uint64_t>(dp, v);
            return;
        }
        case NOESIS_PROP_FLOAT: {
            float v = value_ptr ? *static_cast<const float*>(value_ptr) : 0.0f;
            if (mode == SetMode::Current) obj->SetCurrentValue<float>(dp, v);
            else obj->SetValue<float>(dp, v);
            return;
        }
        case NOESIS_PROP_DOUBLE: {
            double v = value_ptr ? *static_cast<const double*>(value_ptr) : 0.0;
            if (mode == SetMode::Current) obj->SetCurrentValue<double>(dp, v);
            else obj->SetValue<double>(dp, v);
            return;
        }
        case NOESIS_PROP_BOOL: {
            bool v = value_ptr ? *static_cast<const bool*>(value_ptr) : false;
            if (mode == SetMode::Current) obj->SetCurrentValue<bool>(dp, v);
            else obj->SetValue<bool>(dp, v);
            return;
        }
        case NOESIS_PROP_STRING: {
            const char* s = value_ptr ? *static_cast<const char* const*>(value_ptr) : nullptr;
            const char* safe = s ? s : "";
            if (mode == SetMode::Current) obj->SetCurrentValue<String>(dp, safe);
            else obj->SetValue<String>(dp, safe);
            return;
        }
        case NOESIS_PROP_THICKNESS: {
            Thickness t;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                t = Thickness(f[0], f[1], f[2], f[3]);
            }
            if (mode == SetMode::Current) obj->SetCurrentValue<Thickness>(dp, t);
            else obj->SetValue<Thickness>(dp, t);
            return;
        }
        case NOESIS_PROP_COLOR: {
            Color c;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                c = Color(f[0], f[1], f[2], f[3]);
            }
            if (mode == SetMode::Current) obj->SetCurrentValue<Color>(dp, c);
            else obj->SetValue<Color>(dp, c);
            return;
        }
        case NOESIS_PROP_RECT: {
            Rect r;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                r = Rect(f[0], f[1], f[0] + f[2], f[1] + f[3]);
            }
            if (mode == SetMode::Current) obj->SetCurrentValue<Rect>(dp, r);
            else obj->SetValue<Rect>(dp, r);
            return;
        }
        case NOESIS_PROP_POINT: {
            Point p;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                p = Point(f[0], f[1]);
            }
            if (mode == SetMode::Current) obj->SetCurrentValue<Point>(dp, p);
            else obj->SetValue<Point>(dp, p);
            return;
        }
        case NOESIS_PROP_SIZE: {
            Size s;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                s = Size(f[0], f[1]);
            }
            if (mode == SetMode::Current) obj->SetCurrentValue<Size>(dp, s);
            else obj->SetValue<Size>(dp, s);
            return;
        }
        case NOESIS_PROP_VECTOR: {
            Vector2 v;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                v = Vector2(f[0], f[1]);
            }
            if (mode == SetMode::Current) obj->SetCurrentValue<Vector2>(dp, v);
            else obj->SetValue<Vector2>(dp, v);
            return;
        }
        case NOESIS_PROP_ENUM: {
            int32_t v = value_ptr ? *static_cast<const int32_t*>(value_ptr) : 0;
            if (mode == SetMode::Current) obj->SetCurrentValue<int32_t>(dp, v);
            else obj->SetValue<int32_t>(dp, v);
            return;
        }
        case NOESIS_PROP_IMAGE_SOURCE:
        case NOESIS_PROP_BASE_COMPONENT: {
            BaseComponent* b = value_ptr ? *static_cast<BaseComponent* const*>(value_ptr) : nullptr;
            if (mode == SetMode::Current) obj->SetCurrentValueObject(dp, b);
            else obj->SetValueObject(dp, b);
            return;
        }
    }
}

// String and object outputs are borrowed from the DP's storage.
bool apply_get(
    Noesis::DependencyObject* obj,
    const Noesis::DependencyProperty* dp,
    noesis_prop_type type,
    void* out_value,
    GetMode mode = GetMode::Effective) {
    using namespace Noesis;
    const bool base = mode == GetMode::Base;
    switch (type) {
        case NOESIS_PROP_INT32:
            *static_cast<int32_t*>(out_value) =
                base ? obj->GetBaseValue<int32_t>(dp) : obj->GetValue<int32_t>(dp);
            return true;
        case NOESIS_PROP_UINT32:
            *static_cast<uint32_t*>(out_value) =
                base ? obj->GetBaseValue<uint32_t>(dp) : obj->GetValue<uint32_t>(dp);
            return true;
        case NOESIS_PROP_UINT64:
            *static_cast<uint64_t*>(out_value) =
                base ? obj->GetBaseValue<uint64_t>(dp) : obj->GetValue<uint64_t>(dp);
            return true;
        case NOESIS_PROP_FLOAT:
            *static_cast<float*>(out_value) =
                base ? obj->GetBaseValue<float>(dp) : obj->GetValue<float>(dp);
            return true;
        case NOESIS_PROP_DOUBLE:
            *static_cast<double*>(out_value) =
                base ? obj->GetBaseValue<double>(dp) : obj->GetValue<double>(dp);
            return true;
        case NOESIS_PROP_BOOL:
            *static_cast<bool*>(out_value) =
                base ? obj->GetBaseValue<bool>(dp) : obj->GetValue<bool>(dp);
            return true;
        case NOESIS_PROP_STRING: {
            const String& s = base ? obj->GetBaseValue<String>(dp) : obj->GetValue<String>(dp);
            *static_cast<const char**>(out_value) = s.Str();
            return true;
        }
        case NOESIS_PROP_THICKNESS: {
            const Thickness& t =
                base ? obj->GetBaseValue<Thickness>(dp) : obj->GetValue<Thickness>(dp);
            auto* f = static_cast<float*>(out_value);
            f[0] = t.left; f[1] = t.top; f[2] = t.right; f[3] = t.bottom;
            return true;
        }
        case NOESIS_PROP_COLOR: {
            const Color& c = base ? obj->GetBaseValue<Color>(dp) : obj->GetValue<Color>(dp);
            auto* f = static_cast<float*>(out_value);
            f[0] = c.r; f[1] = c.g; f[2] = c.b; f[3] = c.a;
            return true;
        }
        case NOESIS_PROP_RECT: {
            const Rect& r = base ? obj->GetBaseValue<Rect>(dp) : obj->GetValue<Rect>(dp);
            auto* f = static_cast<float*>(out_value);
            f[0] = r.x; f[1] = r.y; f[2] = r.width; f[3] = r.height;
            return true;
        }
        case NOESIS_PROP_POINT: {
            const Point& p = base ? obj->GetBaseValue<Point>(dp) : obj->GetValue<Point>(dp);
            auto* f = static_cast<float*>(out_value);
            f[0] = p.x; f[1] = p.y;
            return true;
        }
        case NOESIS_PROP_SIZE: {
            const Size& s = base ? obj->GetBaseValue<Size>(dp) : obj->GetValue<Size>(dp);
            auto* f = static_cast<float*>(out_value);
            f[0] = s.width; f[1] = s.height;
            return true;
        }
        case NOESIS_PROP_VECTOR: {
            const Vector2& v = base ? obj->GetBaseValue<Vector2>(dp) : obj->GetValue<Vector2>(dp);
            auto* f = static_cast<float*>(out_value);
            f[0] = v.x; f[1] = v.y;
            return true;
        }
        case NOESIS_PROP_ENUM:
            *static_cast<int32_t*>(out_value) =
                base ? obj->GetBaseValue<int32_t>(dp) : obj->GetValue<int32_t>(dp);
            return true;
        case NOESIS_PROP_IMAGE_SOURCE:
        case NOESIS_PROP_BASE_COMPONENT: {
            if (base) return false;
            Ptr<BaseComponent> v = obj->GetValueObject(dp);
            *static_cast<BaseComponent**>(out_value) = v.GetPtr();
            return true;
        }
    }
    return false;
}

bool prop_type_matches(const Noesis::Type* t, noesis_prop_type tag) {
    using namespace Noesis;
    if (!t) return false;
    switch (tag) {
        case NOESIS_PROP_INT32:     return t == TypeOf<int32_t>();
        case NOESIS_PROP_UINT32:    return t == TypeOf<uint32_t>();
        case NOESIS_PROP_UINT64:    return t == TypeOf<uint64_t>();
        case NOESIS_PROP_FLOAT:     return t == TypeOf<float>();
        case NOESIS_PROP_DOUBLE:    return t == TypeOf<double>();
        case NOESIS_PROP_BOOL:      return t == TypeOf<bool>();
        case NOESIS_PROP_STRING:    return t == TypeOf<String>();
        case NOESIS_PROP_THICKNESS: return t == TypeOf<Thickness>();
        case NOESIS_PROP_COLOR:     return t == TypeOf<Color>();
        case NOESIS_PROP_RECT:      return t == TypeOf<Rect>();
        case NOESIS_PROP_POINT:     return t == TypeOf<Point>();
        case NOESIS_PROP_SIZE:      return t == TypeOf<Size>();
        case NOESIS_PROP_VECTOR:    return t == TypeOf<Vector2>();
        case NOESIS_PROP_ENUM:      return DynamicCast<const TypeEnum*>(t) != nullptr;
        case NOESIS_PROP_IMAGE_SOURCE:
            return TypeOf<ImageSource>()->IsAssignableFrom(t);
        case NOESIS_PROP_BASE_COMPONENT:
            return TypeOf<BaseComponent>()->IsAssignableFrom(t);
    }
    return false;
}

// -1 for types with no tag.
int32_t prop_type_to_tag(const Noesis::Type* t) {
    using namespace Noesis;
    if (!t) return -1;
    if (t == TypeOf<int32_t>())   return NOESIS_PROP_INT32;
    if (t == TypeOf<uint32_t>())  return NOESIS_PROP_UINT32;
    if (t == TypeOf<uint64_t>())  return NOESIS_PROP_UINT64;
    if (t == TypeOf<float>())     return NOESIS_PROP_FLOAT;
    if (t == TypeOf<double>())    return NOESIS_PROP_DOUBLE;
    if (t == TypeOf<bool>())      return NOESIS_PROP_BOOL;
    if (t == TypeOf<String>())    return NOESIS_PROP_STRING;
    if (t == TypeOf<Thickness>()) return NOESIS_PROP_THICKNESS;
    if (t == TypeOf<Color>())     return NOESIS_PROP_COLOR;
    if (t == TypeOf<Rect>())      return NOESIS_PROP_RECT;
    if (t == TypeOf<Point>())     return NOESIS_PROP_POINT;
    if (t == TypeOf<Size>())      return NOESIS_PROP_SIZE;
    if (t == TypeOf<Vector2>())   return NOESIS_PROP_VECTOR;
    if (Noesis::DynamicCast<const Noesis::TypeEnum*>(t)) return NOESIS_PROP_ENUM;
    if (TypeOf<ImageSource>()->IsAssignableFrom(t)) return NOESIS_PROP_IMAGE_SOURCE;
    if (TypeOf<BaseComponent>()->IsAssignableFrom(t)) return NOESIS_PROP_BASE_COMPONENT;
    return -1;
}

const Noesis::DependencyProperty* resolve_dp(
    void* obj, const char* name, Noesis::DependencyObject** out_d) {
    if (!obj || !name) return nullptr;
    auto* d = Noesis::DynamicCast<Noesis::DependencyObject*>(
        static_cast<Noesis::BaseComponent*>(obj));
    if (out_d) *out_d = d;
    if (!d) return nullptr;
    return Noesis::FindDependencyProperty(d->GetClassType(), Noesis::Symbol(name));
}

}  // namespace

extern "C" void noesis_instance_set_property(
    void* instance,
    uint32_t prop_index,
    const void* value_ptr) {
    Noesis::DependencyObject* obj = nullptr;
    const PropEntry* pe = instance_prop(instance, prop_index, nullptr, &obj);
    if (!pe || !obj) return;
    // Read-only DPs are written only by noesis_instance_set_readonly_property.
    if (pe->read_only) return;
    apply_set(obj, pe->dp, pe->type, value_ptr);
}

extern "C" bool noesis_instance_set_readonly_property(
    void* instance,
    uint32_t prop_index,
    const void* value_ptr) {
    RustClassInstance* iface = instance_lookup(instance);
    if (!iface) return false;
    ClassData* cd = iface->GetClassData();
    if (!cd || prop_index >= cd->properties.size()) return false;
    const PropEntry& pe = cd->properties[prop_index];
    iface->RustSetReadOnly(pe.dp, pe.type, value_ptr);
    return true;
}

extern "C" bool noesis_freezable_freeze(void* freezable) {
    if (!freezable) return false;
    auto* fz = Noesis::DynamicCast<Noesis::Freezable*>(
        static_cast<Noesis::BaseComponent*>(freezable));
    if (!fz || !fz->CanFreeze()) return false;
    fz->Freeze();
    return true;
}

extern "C" bool noesis_freezable_is_frozen(void* freezable) {
    if (!freezable) return false;
    auto* fz = Noesis::DynamicCast<Noesis::Freezable*>(
        static_cast<Noesis::BaseComponent*>(freezable));
    return fz && fz->IsFrozen();
}

extern "C" bool noesis_freezable_can_freeze(void* freezable) {
    if (!freezable) return false;
    auto* fz = Noesis::DynamicCast<Noesis::Freezable*>(
        static_cast<Noesis::BaseComponent*>(freezable));
    return fz && fz->CanFreeze();
}

extern "C" bool noesis_image_source_get_size(
    void* image_source,
    float* out_width,
    float* out_height) {
    if (!image_source || !out_width || !out_height) return false;
    auto* obj = static_cast<Noesis::BaseComponent*>(image_source);
    auto* img = Noesis::DynamicCast<Noesis::ImageSource*>(obj);
    if (!img) return false;
    *out_width = img->GetWidth();
    *out_height = img->GetHeight();
    return true;
}

extern "C" bool noesis_instance_get_property(
    void* instance,
    uint32_t prop_index,
    void* out_value) {
    Noesis::DependencyObject* obj = nullptr;
    const PropEntry* pe = instance_prop(instance, prop_index, nullptr, &obj);
    if (!pe || !obj || !out_value) return false;
    return apply_get(obj, pe->dp, pe->type, out_value);
}

// ── UIElement layout primitives ─────────────────────────────────────────────

extern "C" bool noesis_uielement_measure(void* element, float avail_w, float avail_h) {
    if (!element) return false;
    auto* e = Noesis::DynamicCast<Noesis::UIElement*>(static_cast<Noesis::BaseComponent*>(element));
    if (!e) return false;
    e->Measure(Noesis::Size(avail_w, avail_h));
    return true;
}

extern "C" bool noesis_uielement_arrange(void* element, float x, float y, float w, float h) {
    if (!element) return false;
    auto* e = Noesis::DynamicCast<Noesis::UIElement*>(static_cast<Noesis::BaseComponent*>(element));
    if (!e) return false;
    // Rect(left, top, right, bottom).
    e->Arrange(Noesis::Rect(x, y, x + w, y + h));
    return true;
}

extern "C" bool noesis_uielement_desired_size(void* element, float* out_w, float* out_h) {
    if (!element || !out_w || !out_h) return false;
    auto* e = Noesis::DynamicCast<Noesis::UIElement*>(static_cast<Noesis::BaseComponent*>(element));
    if (!e) return false;
    const Noesis::Size& d = e->GetDesiredSize();
    *out_w = d.width;
    *out_h = d.height;
    return true;
}

// ── Generic name-keyed DependencyProperty access ───────────────────────────
//
// The caller's tag is checked against the DP's reflected type before any
// cast; a mismatch returns false. Setters refuse read-only DPs.
//
// No VerifyAccess(): these must never throw across the C ABI.

extern "C" bool noesis_dependency_object_set_property(
    void* obj,
    const char* name,
    uint32_t prop_type,
    const void* value_ptr) {
    if (!obj || !name) return false;
    auto* base = static_cast<Noesis::BaseComponent*>(obj);
    auto* d = Noesis::DynamicCast<Noesis::DependencyObject*>(base);
    if (!d) return false;

    const Noesis::DependencyProperty* dp =
        Noesis::FindDependencyProperty(d->GetClassType(), Noesis::Symbol(name));
    if (!dp) return false;

    auto type = static_cast<noesis_prop_type>(prop_type);
    if (!prop_type_matches(dp->GetType(), type)) return false;
    if (dp->IsReadOnly()) return false;

    apply_set(d, dp, type, value_ptr);
    return true;
}

extern "C" bool noesis_dependency_object_get_property(
    void* obj,
    const char* name,
    uint32_t prop_type,
    void* out_value) {
    if (!obj || !name || !out_value) return false;
    auto* base = static_cast<Noesis::BaseComponent*>(obj);
    auto* d = Noesis::DynamicCast<Noesis::DependencyObject*>(base);
    if (!d) return false;

    const Noesis::DependencyProperty* dp =
        Noesis::FindDependencyProperty(d->GetClassType(), Noesis::Symbol(name));
    if (!dp) return false;

    auto type = static_cast<noesis_prop_type>(prop_type);
    if (!prop_type_matches(dp->GetType(), type)) return false;

    return apply_get(d, dp, type, out_value);
}

// ── Attached properties ─────────────────────────────────────────────────────
//
// The DP is looked up on `owner_type` (e.g. "Grid" / "Row") and applied to
// `obj`. Owners may be built-in types or classes from noesis_class_register.

extern "C" bool noesis_dependency_object_set_attached(
    void* obj,
    const char* owner_type,
    const char* prop_name,
    uint32_t prop_type,
    const void* value_ptr) {
    if (!obj || !owner_type || !prop_name) return false;
    auto* d = Noesis::DynamicCast<Noesis::DependencyObject*>(
        static_cast<Noesis::BaseComponent*>(obj));
    if (!d) return false;

    const Noesis::Type* t = Noesis::Reflection::GetType(Noesis::Symbol(owner_type));
    const auto* owner = Noesis::DynamicCast<const Noesis::TypeClass*>(t);
    if (!owner) return false;

    const Noesis::DependencyProperty* dp =
        Noesis::FindDependencyProperty(owner, Noesis::Symbol(prop_name));
    if (!dp) return false;

    auto type = static_cast<noesis_prop_type>(prop_type);
    if (!prop_type_matches(dp->GetType(), type)) return false;
    if (dp->IsReadOnly()) return false;

    apply_set(d, dp, type, value_ptr);
    return true;
}

extern "C" bool noesis_dependency_object_get_attached(
    void* obj,
    const char* owner_type,
    const char* prop_name,
    uint32_t prop_type,
    void* out_value) {
    if (!obj || !owner_type || !prop_name || !out_value) return false;
    auto* d = Noesis::DynamicCast<Noesis::DependencyObject*>(
        static_cast<Noesis::BaseComponent*>(obj));
    if (!d) return false;

    const Noesis::Type* t = Noesis::Reflection::GetType(Noesis::Symbol(owner_type));
    const auto* owner = Noesis::DynamicCast<const Noesis::TypeClass*>(t);
    if (!owner) return false;

    const Noesis::DependencyProperty* dp =
        Noesis::FindDependencyProperty(owner, Noesis::Symbol(prop_name));
    if (!dp) return false;

    auto type = static_cast<noesis_prop_type>(prop_type);
    if (!prop_type_matches(dp->GetType(), type)) return false;

    return apply_get(d, dp, type, out_value);
}

// ── ClearValue / SetCurrentValue / GetBaseValue ─────────────────────────────

extern "C" bool noesis_dependency_object_clear_value(void* obj, const char* name) {
    Noesis::DependencyObject* d = nullptr;
    const Noesis::DependencyProperty* dp = resolve_dp(obj, name, &d);
    if (!dp) return false;
    if (dp->IsReadOnly()) return false;
    d->ClearLocalValue(dp);
    return true;
}

extern "C" bool noesis_dependency_object_set_current_value(
    void* obj,
    const char* name,
    uint32_t prop_type,
    const void* value_ptr) {
    Noesis::DependencyObject* d = nullptr;
    const Noesis::DependencyProperty* dp = resolve_dp(obj, name, &d);
    if (!dp) return false;

    auto type = static_cast<noesis_prop_type>(prop_type);
    if (!prop_type_matches(dp->GetType(), type)) return false;
    if (dp->IsReadOnly()) return false;

    apply_set(d, dp, type, value_ptr, SetMode::Current);
    return true;
}

extern "C" bool noesis_dependency_object_get_base_value(
    void* obj,
    const char* name,
    uint32_t prop_type,
    void* out_value) {
    if (!out_value) return false;
    Noesis::DependencyObject* d = nullptr;
    const Noesis::DependencyProperty* dp = resolve_dp(obj, name, &d);
    if (!dp) return false;

    auto type = static_cast<noesis_prop_type>(prop_type);
    if (!prop_type_matches(dp->GetType(), type)) return false;

    // apply_get returns false for object tags.
    return apply_get(d, dp, type, out_value, GetMode::Base);
}

extern "C" int32_t noesis_dependency_object_property_tag(void* obj, const char* name) {
    const Noesis::DependencyProperty* dp = resolve_dp(obj, name, nullptr);
    if (!dp) return -1;
    return prop_type_to_tag(dp->GetType());
}
