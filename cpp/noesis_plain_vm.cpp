// Plain (non-DependencyObject) view models + MultiBinding.
//
// Three cooperating pieces:
//
//   * RustPlainVm — a plain `Noesis::BaseComponent` (NOT a DependencyObject)
//     that implements `INotifyPropertyChanged` and reports a per-registration
//     synthetic `TypeClass`. Its properties resolve through reflection to
//     per-instance boxed values that Rust pushes in. This is what makes
//     `{Binding Title}` work against a Rust view model used as a DataContext,
//     and — paired with PropertyChanged notifications — what makes a bound UI
//     target refresh when Rust mutates the model.
//
//   * RustPlainProperty : Noesis::TypeProperty — a custom reflected property
//     (the NsProp-equivalent) whose accessors (GetComponent / SetComponent)
//     read/write the owning instance's boxed value store instead of a C++
//     member offset. This is the "install property accessors that resolve into
//     Rust" requirement: the value lives in a Rust-controlled store, the
//     binding engine reaches it purely through reflection.
//
//   * RustMultiValueConverter : Noesis::BaseMultiValueConverter + MultiBinding
//     construction — combine N child Bindings through a Rust converter over an
//     array of boxed values.
//
// Lifetime mirrors the synthetic-class registry in noesis_classes.cpp (refcounted
// PlainClassData; donated Rust free handler runs once on last release; a shutdown
// sweep cleans up handler boxes whose instances bypassed teardown). The converter
// lifetime mirrors RustValueConverter in noesis_binding.cpp.

#include "noesis_shim.h"

#include <NsCore/ArrayRef.h>
#include <NsCore/BaseComponent.h>
#include <NsCore/Boxing.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsCore/Reflection.h>
#include <NsCore/ReflectionImplement.h>
#include <NsCore/String.h>
#include <NsCore/Symbol.h>
#include <NsCore/Type.h>
#include <NsCore/TypeClass.h>
#include <NsCore/TypeClassBuilder.h>
#include <NsCore/TypeClassCreator.h>
#include <NsCore/TypeOf.h>
#include <NsCore/TypeProperty.h>
#include <NsGui/BaseMultiValueConverter.h>
#include <NsGui/Binding.h>
#include <NsGui/BindingOperations.h>
#include <NsGui/DependencyObject.h>
#include <NsGui/DependencyProperty.h>
#include <NsGui/Enums.h>
#include <NsGui/IMultiValueConverter.h>
#include <NsGui/INotifyPropertyChanged.h>
#include <NsGui/MultiBinding.h>
#include <NsGui/UICollection.h>

#include <atomic>
#include <mutex>
#include <vector>

namespace {

class RustPlainVm;

struct PlainProp {
    Noesis::TypeProperty* prop;  // owned by the TypeClass once added
    noesis_plain_type  type;
};

// Per-registered-class state. Refcount + free-handler lifetime model copied
// verbatim from `ClassData` in noesis_classes.cpp — see that file for the full
// rationale (deferred free so a property write fired during instance teardown
// always sees a live `userdata`).
struct PlainClassData {
    Noesis::String              name;
    Noesis::Symbol              sym;
    Noesis::TypeClassBuilder*   typeClass;
    std::vector<PlainProp>      properties;
    noesis_plain_set_fn      on_set;
    void*                       userdata;
    noesis_plain_free_fn     free_handler;
    bool                        hasInstances;
    std::atomic<int>            ref_count;

    PlainClassData(): hasInstances(false), ref_count(1) {}

    void AddRef() noexcept { ref_count.fetch_add(1, std::memory_order_relaxed); }

    void Release() {
        if (ref_count.fetch_sub(1, std::memory_order_acq_rel) == 1) {
            std::atomic_thread_fence(std::memory_order_acquire);
            void* ud = userdata;
            userdata = nullptr;
            if (free_handler && ud) {
                free_handler(ud);
            }
        }
    }
};

// Every successfully-registered PlainClassData, ever — for the shutdown sweep
// (see noesis_classes_force_free_at_shutdown for the rationale). Unlike the
// synthetic-control registry in noesis_classes.cpp, plain VMs are never created
// by name through the Factory (they're instantiated directly from the Rust
// token), so no Symbol→ClassData lookup map is needed.
std::mutex                                  g_all_mutex;
std::vector<PlainClassData*>                g_all;

void track(PlainClassData* cd) {
    std::lock_guard<std::mutex> lock(g_all_mutex);
    g_all.push_back(cd);
}

const Noesis::Type* plain_content_type(noesis_plain_type t) {
    using namespace Noesis;
    switch (t) {
        case NOESIS_PLAIN_INT32:          return TypeOf<int32_t>();
        case NOESIS_PLAIN_DOUBLE:         return TypeOf<double>();
        case NOESIS_PLAIN_BOOL:           return TypeOf<bool>();
        case NOESIS_PLAIN_STRING:         return TypeOf<String>();
        case NOESIS_PLAIN_BASE_COMPONENT: return TypeOf<BaseComponent>();
    }
    return nullptr;
}

// ── RustPlainVm ─────────────────────────────────────────────────────────────
//
// Hand-rolled reflection (like RustContentControl): a custom GetClassType so an
// instance reports its per-registration synthetic TypeClass, while the static
// "DmNoesis.RustPlainVm" base type carries the INotifyPropertyChanged interface
// registration (so `DynamicCast<INotifyPropertyChanged*>(vm)` resolves through
// the synthetic class's base chain — that's how the binding engine discovers the
// model is observable).

class RustPlainVm: public Noesis::BaseComponent, public Noesis::INotifyPropertyChanged {
public:
    RustPlainVm() = default;

    ~RustPlainVm() {
        if (mClassData) {
            mClassData->Release();
            mClassData = nullptr;
        }
    }

    void BindClassData(PlainClassData* cd) {
        if (mClassData) mClassData->Release();
        mClassData = cd;
        if (cd) {
            cd->AddRef();
            mValues.resize(cd->properties.size());
        }
    }

    PlainClassData* GetClassData() const { return mClassData; }

    // Reflection read: borrow-return the boxed value (the binding takes its own
    // reference). Null Ptr if unset / out of range.
    Noesis::Ptr<Noesis::BaseComponent> GetBoxed(uint32_t index) const {
        if (index >= mValues.size()) return nullptr;
        return mValues[index];
    }

    // Store a boxed value WITHOUT firing the Rust callback. Used by the Rust
    // push path (noesis_plain_vm_set_value) and internally by the binding
    // writeback path (after which we additionally notify Rust).
    bool StoreBoxed(uint32_t index, Noesis::BaseComponent* value) {
        if (index >= mValues.size()) return false;
        mValues[index] = Noesis::Ptr<Noesis::BaseComponent>(value);  // AddRefs
        return true;
    }

    // Reflection write (TwoWay binding pushed a value to the source): store it,
    // then forward to Rust so the model author observes the UI edit.
    void SetFromBinding(uint32_t index, Noesis::BaseComponent* value) {
        if (!StoreBoxed(index, value)) return;
        if (mClassData && mClassData->on_set) {
            mClassData->on_set(mClassData->userdata, this, index, value);
        }
    }

    void Raise(Noesis::Symbol name) {
        if (!mPropertyChanged.Empty()) {
            mPropertyChanged(this, Noesis::PropertyChangedEventArgs(name));
        }
    }

    // From INotifyPropertyChanged.
    Noesis::PropertyChangedEventHandler& PropertyChanged() override {
        return mPropertyChanged;
    }

    static const Noesis::TypeClass* StaticGetClassType(Noesis::TypeTag<RustPlainVm>*);
    const Noesis::TypeClass* GetClassType() const override;

    NS_IMPLEMENT_INTERFACE_FIXUP

private:
    Noesis::PropertyChangedEventHandler             mPropertyChanged;
    std::vector<Noesis::Ptr<Noesis::BaseComponent>> mValues;
    PlainClassData*                                 mClassData = nullptr;

    typedef RustPlainVm SelfClass;
    typedef Noesis::BaseComponent ParentClass;
    friend class Noesis::TypeClassCreator;
    static void StaticFillClassType(Noesis::TypeClassCreator& helper) {
        // Register the INotifyPropertyChanged interface (with the correct
        // this-pointer offset) so reflection-driven DynamicCast finds it.
        helper.Impl<RustPlainVm, Noesis::INotifyPropertyChanged>();
    }
};

const Noesis::TypeClass*
RustPlainVm::StaticGetClassType(Noesis::TypeTag<RustPlainVm>*) {
    static const Noesis::TypeClass* type;
    if (NS_UNLIKELY(type == 0)) {
        type = static_cast<const Noesis::TypeClass*>(Noesis::Reflection::RegisterType(
            "DmNoesis.RustPlainVm",
            Noesis::TypeClassCreator::Create<RustPlainVm>,
            Noesis::TypeClassCreator::Fill<RustPlainVm, Noesis::BaseComponent>));
    }
    return type;
}

const Noesis::TypeClass* RustPlainVm::GetClassType() const {
    if (mClassData && mClassData->typeClass) {
        return static_cast<const Noesis::TypeClass*>(mClassData->typeClass);
    }
    return StaticGetClassType((Noesis::TypeTag<RustPlainVm>*)nullptr);
}

// ── RustPlainProperty ───────────────────────────────────────────────────────
//
// Custom TypeProperty whose accessors forward to the owning instance's boxed
// value store. The `ptr` reflection passes is the source object pointer; since
// RustPlainVm has BaseComponent as its first base (offset 0), it equals the
// RustPlainVm*. Only the boxed accessors (GetComponent / SetComponent) are
// exercised by the binding engine for a CLR-style source property; the raw
// Get/GetContent paths are not used here, so they degrade safely.

class RustPlainProperty final: public Noesis::TypeProperty {
public:
    RustPlainProperty(Noesis::Symbol name, const Noesis::Type* type, uint32_t index)
        : TypeProperty(name, type), mIndex(index) {}

    void* GetContent(const void* /*ptr*/) const override {
        // No stable address for a boxed-store value; the binding uses
        // GetComponent instead. Returning null keeps any unexpected caller
        // from reading a bogus offset.
        return nullptr;
    }

    bool IsReadOnly() const override { return false; }

    Noesis::Ptr<Noesis::BaseComponent> GetComponent(const void* ptr) const override {
        const auto* vm = static_cast<const RustPlainVm*>(
            static_cast<const Noesis::BaseComponent*>(ptr));
        return vm ? vm->GetBoxed(mIndex) : nullptr;
    }

    void SetComponent(void* ptr, Noesis::BaseComponent* value) const override {
        auto* vm = static_cast<RustPlainVm*>(static_cast<Noesis::BaseComponent*>(ptr));
        if (vm) vm->SetFromBinding(mIndex, value);
    }

private:
    uint32_t mIndex;
};

// ── RustMultiValueConverter ─────────────────────────────────────────────────

class RustMultiValueConverter final: public Noesis::BaseMultiValueConverter {
public:
    RustMultiValueConverter(const noesis_multi_value_converter_vtable* vt, void* userdata,
                            noesis_multi_value_converter_free_fn free_handler)
        : mVtable(*vt), mUserdata(userdata), mFree(free_handler) {}

    ~RustMultiValueConverter() {
        void* ud = mUserdata;
        mUserdata = nullptr;
        if (mFree && ud) {
            mFree(ud);
        }
    }

    bool TryConvert(Noesis::ArrayRef<Noesis::BaseComponent*> values,
                    const Noesis::Type* targetType, Noesis::BaseComponent* parameter,
                    Noesis::Ptr<Noesis::BaseComponent>& result) override {
        if (!mVtable.convert) return false;
        void* out = nullptr;
        bool ok = mVtable.convert(
            mUserdata,
            // ArrayRef<BaseComponent*>::Data() is `BaseComponent* const*`, which
            // is exactly the `void* const*` the Rust vtable expects.
            reinterpret_cast<void* const*>(values.Data()),
            values.Size(),
            static_cast<const void*>(targetType),
            parameter,
            &out);
        if (!ok) return false;
        if (out) {
            result = Noesis::Ptr<Noesis::BaseComponent>(*static_cast<Noesis::BaseComponent*>(out));
        } else {
            result.Reset();
        }
        return true;
    }

    NS_IMPLEMENT_INLINE_REFLECTION(RustMultiValueConverter, Noesis::BaseMultiValueConverter,
                                   "DmNoesis.RustMultiValueConverter") {}

private:
    noesis_multi_value_converter_vtable  mVtable;
    void*                                   mUserdata;
    noesis_multi_value_converter_free_fn mFree;
};

Noesis::MultiBinding* as_multi_binding(void* p) {
    if (!p) return nullptr;
    return Noesis::DynamicCast<Noesis::MultiBinding*>(static_cast<Noesis::BaseComponent*>(p));
}

}  // namespace

// ── C ABI: plain VM registration ────────────────────────────────────────────

extern "C" void* noesis_plain_vm_register(
    const char* name,
    noesis_plain_set_fn on_set,
    void* userdata,
    noesis_plain_free_fn free_handler) {
    if (!name) return nullptr;

    Noesis::Symbol sym = Noesis::Symbol(name);
    if (Noesis::Reflection::IsTypeRegistered(sym)) {
        return nullptr;
    }

    auto* cd = new PlainClassData();
    cd->name = name;
    cd->sym = sym;
    cd->on_set = on_set;
    cd->userdata = userdata;
    cd->free_handler = free_handler;

    cd->typeClass = new Noesis::TypeClassBuilder(sym, /*isInterface*/ false);
    cd->typeClass->AddBase(Noesis::TypeOf<RustPlainVm>());

    Noesis::Reflection::RegisterType(cd->typeClass);

    track(cd);
    return cd;
}

extern "C" uint32_t noesis_plain_vm_register_property(
    void* token, const char* prop_name, uint32_t content_type) {
    if (!token || !prop_name) return UINT32_MAX;
    auto* cd = static_cast<PlainClassData*>(token);
    // Properties must be fixed before instances exist (instances size their
    // value store from the property count at BindClassData time).
    if (cd->hasInstances) return UINT32_MAX;

    auto type = static_cast<noesis_plain_type>(content_type);
    const Noesis::Type* ct = plain_content_type(type);
    if (!ct) return UINT32_MAX;

    uint32_t index = static_cast<uint32_t>(cd->properties.size());
    auto* prop = new RustPlainProperty(Noesis::Symbol(prop_name), ct, index);
    cd->typeClass->AddProperty(prop);  // TypeClass takes ownership
    cd->properties.push_back({prop, type});
    return index;
}

extern "C" void* noesis_plain_vm_create_instance(void* token) {
    if (!token) return nullptr;
    auto* cd = static_cast<PlainClassData*>(token);
    cd->hasInstances = true;
    auto* instance = new RustPlainVm();
    instance->BindClassData(cd);  // +1 share of cd
    // `new` started the BaseComponent at refcount 1 — that IS the caller's +1.
    return static_cast<Noesis::BaseComponent*>(instance);
}

extern "C" bool noesis_plain_vm_set_value(
    void* instance, uint32_t prop_index, void* boxed_value) {
    if (!instance) return false;
    auto* vm = static_cast<RustPlainVm*>(static_cast<Noesis::BaseComponent*>(instance));
    return vm->StoreBoxed(prop_index, static_cast<Noesis::BaseComponent*>(boxed_value));
}

extern "C" void* noesis_plain_vm_get_value(void* instance, uint32_t prop_index) {
    if (!instance) return nullptr;
    auto* vm = static_cast<RustPlainVm*>(static_cast<Noesis::BaseComponent*>(instance));
    Noesis::Ptr<Noesis::BaseComponent> v = vm->GetBoxed(prop_index);
    if (!v) return nullptr;
    v->AddReference();  // +1 for the caller
    return v.GetPtr();
}

extern "C" bool noesis_plain_vm_notify(void* instance, const char* prop_name) {
    if (!instance || !prop_name) return false;
    auto* vm = static_cast<RustPlainVm*>(static_cast<Noesis::BaseComponent*>(instance));
    vm->Raise(Noesis::Symbol(prop_name));
    return true;
}

extern "C" void noesis_plain_vm_unregister(void* token) {
    if (!token) return;
    auto* cd = static_cast<PlainClassData*>(token);
    cd->Release();
}

extern "C" void noesis_plain_vm_force_free_at_shutdown(void) {
    std::vector<PlainClassData*> all;
    {
        std::lock_guard<std::mutex> lock(g_all_mutex);
        all = std::move(g_all);
    }
    for (PlainClassData* cd : all) {
        void* ud = cd->userdata;
        cd->userdata = nullptr;
        if (cd->free_handler && ud) {
            cd->free_handler(ud);
        }
    }
}

// ── C ABI: IMultiValueConverter + MultiBinding ──────────────────────────────

extern "C" void* noesis_multi_value_converter_create(
    const noesis_multi_value_converter_vtable* vt,
    void* userdata,
    noesis_multi_value_converter_free_fn free_handler) {
    if (!vt) return nullptr;
    auto* conv = new RustMultiValueConverter(vt, userdata, free_handler);
    return static_cast<Noesis::BaseComponent*>(conv);
}

extern "C" void noesis_multi_value_converter_destroy(void* converter) {
    if (!converter) return;
    static_cast<Noesis::BaseComponent*>(converter)->Release();
}

extern "C" void* noesis_multi_binding_create(void) {
    auto* mb = new Noesis::MultiBinding();
    return static_cast<Noesis::BaseComponent*>(mb);
}

extern "C" void noesis_multi_binding_destroy(void* multi_binding) {
    if (!multi_binding) return;
    static_cast<Noesis::BaseComponent*>(multi_binding)->Release();
}

extern "C" bool noesis_multi_binding_add_binding(void* multi_binding, void* binding) {
    Noesis::MultiBinding* mb = as_multi_binding(multi_binding);
    if (!mb || !binding) return false;
    auto* child = Noesis::DynamicCast<Noesis::BaseBinding*>(
        static_cast<Noesis::BaseComponent*>(binding));
    if (!child) return false;
    Noesis::BindingCollection* bindings = mb->GetBindings();
    if (!bindings) return false;
    bindings->Add(child);  // takes its own reference
    return true;
}

extern "C" void noesis_multi_binding_set_converter(void* multi_binding, void* converter) {
    Noesis::MultiBinding* mb = as_multi_binding(multi_binding);
    if (!mb) return;
    auto* conv = converter
        ? Noesis::DynamicCast<Noesis::IMultiValueConverter*>(
              static_cast<Noesis::BaseComponent*>(converter))
        : nullptr;
    mb->SetConverter(conv);
}

extern "C" void noesis_multi_binding_set_converter_parameter(
    void* multi_binding, void* parameter) {
    Noesis::MultiBinding* mb = as_multi_binding(multi_binding);
    if (mb) mb->SetConverterParameter(static_cast<Noesis::BaseComponent*>(parameter));
}

extern "C" void noesis_multi_binding_set_mode(void* multi_binding, int32_t mode) {
    Noesis::MultiBinding* mb = as_multi_binding(multi_binding);
    if (mb) mb->SetMode(static_cast<Noesis::BindingMode>(mode));
}

extern "C" bool noesis_set_multi_binding(
    void* element, const char* dp_name, void* multi_binding) {
    if (!element || !dp_name || !multi_binding) return false;
    auto* d = Noesis::DynamicCast<Noesis::DependencyObject*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!d) return false;
    Noesis::MultiBinding* mb = as_multi_binding(multi_binding);
    if (!mb) return false;

    const Noesis::DependencyProperty* dp =
        Noesis::FindDependencyProperty(d->GetClassType(), Noesis::Symbol(dp_name));
    if (!dp) return false;

    Noesis::BindingOperations::SetBinding(d, dp, mb);
    return true;
}
