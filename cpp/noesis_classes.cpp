// Custom XAML class registration FFI (Phase 5.C).
//
// Architecture mirrors what Noesis's C# / Unity binding does for managed
// code — we cannot conjure C++ types from C-FFI, but we *can* synthesize
// a `TypeClassBuilder` per consumer-named class and register a Factory
// creator that returns a fresh instance of a per-base trampoline subclass
// whose `GetClassType()` reports the synthetic class.
//
// Components:
//   * RustContentControl — trampoline subclass of Noesis::ContentControl.
//     Holds a back-pointer to its owning ClassData; overrides GetClassType()
//     and OnPropertyChanged() to (a) report the synthetic TypeClass for XAML
//     style/binding lookups and (b) forward DP changes to the Rust callback.
//   * ClassData — per-registered-class state: synthetic TypeClassBuilder,
//     UIElementData metadata, dense list of registered DPs (with their FFI
//     value-type tags so we know how to marshal across the boundary), and
//     the Rust callback fn pointer + userdata.
//   * g_class_registry — Symbol → ClassData* lookup so the Factory creator
//     (which only receives a Symbol) can recover the right ClassData.
//
// Adding a new base type (Control, UserControl, FrameworkElement, Panel)
// is a uniform addition: derive another trampoline class with the same
// override pattern, branch on the `base` enum in dm_noesis_class_register.

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
#include <NsCore/TypeOf.h>
#include <NsDrawing/Color.h>
#include <NsDrawing/Rect.h>
#include <NsDrawing/Thickness.h>
#include <NsGui/ContentControl.h>
#include <NsGui/DependencyObject.h>
#include <NsGui/DependencyProperty.h>
#include <NsGui/ImageSource.h>
#include <NsGui/PropertyMetadata.h>
#include <NsGui/UIElementData.h>

#include <atomic>
#include <mutex>
#include <unordered_map>
#include <vector>

namespace {

struct PropEntry {
    const Noesis::DependencyProperty* dp;
    dm_noesis_prop_type type;
};

// Intrusively refcounted per-registered-class state.
//
// Lifetime model:
//
//   * `ref_count` starts at 1 — that's the Rust caller's reference, held by
//     the `ClassRegistration` Rust struct.
//   * Each live instance (constructed by the Noesis Factory via
//     `class_creator` below) bumps the count via `RustContentControl::
//     BindClassData`; the instance's destructor releases its share.
//   * `dm_noesis_class_unregister` calls `Release()` on the Rust caller's
//     ref. When the count hits 0, the Rust handler box is freed
//     immediately via the `free_handler` trampoline; `ClassData` itself,
//     `typeClass`, and `uiData` persist until process exit. (The
//     destructor chain from a still-live-during-Release instance walks
//     `typeClass` after `Release` returns; tearing it down mid-chain
//     UAFs in libNoesis. See `Release` and `free_class_data_at_shutdown`.)
//   * On final free we invoke `free_handler(userdata)` — a Rust trampoline
//     that drops the boxed `dyn PropertyChangeHandler`. The Rust side
//     never frees its own box; ownership is donated to ClassData at
//     register time.
//
// This guarantees that any property-change callback fired during instance
// destruction (via `ForwardChange` -> `cd->cb(cd->userdata, ...)`) sees a
// live `userdata`, even if the Rust `ClassRegistration` was dropped first.
struct ClassData {
    Noesis::String                      name;
    Noesis::Symbol                      sym;
    // TypeClassBuilder isn't a BaseComponent — it inherits TypeMeta and is
    // owned by Noesis's Reflection registry (Reflection::RegisterType +
    // Reflection::Unregister handle the lifecycle).
    Noesis::TypeClassBuilder*           typeClass;
    Noesis::Ptr<Noesis::UIElementData>  uiData;
    std::vector<PropEntry>              properties;
    dm_noesis_prop_changed_fn           cb;
    void*                               userdata;
    dm_noesis_class_free_fn             free_handler;
    std::atomic<int>                    ref_count;

    ClassData(): ref_count(1) {}

    void AddRef() noexcept {
        ref_count.fetch_add(1, std::memory_order_relaxed);
    }

    void Release() {
        // acq_rel on the decrement so the final store synchronizes with
        // any prior writes from other threads holding the last refs (we
        // don't currently use ClassData cross-thread, but the atomic
        // semantics keep the contract correct if we ever do).
        if (ref_count.fetch_sub(1, std::memory_order_acq_rel) == 1) {
            std::atomic_thread_fence(std::memory_order_acquire);
            // Run the Rust handler-box drop ONLY. Do NOT delete typeClass,
            // uiData, or `this` — `~RustContentControl` runs Release from
            // inside the C++ destructor chain, and the parent destructors
            // (`~ContentControl`, `~FrameworkElement`, `~DependencyObject`)
            // that run AFTER our body still walk `typeClass` for property-
            // metadata cleanup. Tearing down typeClass mid-chain UAFs in
            // libNoesis.
            //
            // The Noesis-side state (typeClass, uiData, this ClassData
            // itself) is freed at process shutdown by
            // `dm_noesis_classes_force_free_at_shutdown` after `Noesis::Shutdown` has torn
            // down every live instance. Bounded leak: one ClassData per
            // registered class.
            //
            // CAS the userdata ptr to null so a (currently-impossible but
            // future-defensive) double Release-at-zero can't double-free
            // the handler box.
            void* ud = userdata;
            userdata = nullptr;
            if (free_handler && ud) {
                free_handler(ud);
            }
        }
    }
};

// Symbol-keyed registry. Symbols are 32-bit interned IDs so we can use a
// plain unordered_map keyed by their underlying integer.
std::mutex                                       g_registry_mutex;
std::unordered_map<uint32_t, ClassData*>         g_class_registry;

// Every successfully-registered ClassData, ever. `Release` doesn't erase
// from this list when refcount hits zero — instead, the shutdown sweep
// walks the whole list and free's any handler box that's still set.
// Entries whose `userdata` is already null (the common case, after
// instances finished destructing normally) are no-ops in the sweep.
//
// We keep the "every allocation" semantic rather than maintaining a true
// pending list because erasing from a vector during instance destruction
// would need a dedicated re-entrancy-safe structure; the current cost is
// O(N_classes) iteration at shutdown over a tiny N. Separate from
// `g_class_registry` (which is erased at unregister time) because the
// sweep needs to find entries even after they've been unregistered.
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

// ── Trampoline subclass: ContentControl ────────────────────────────────────
//
// Hand-rolled reflection: we cannot use NS_DECLARE_REFLECTION /
// NS_IMPLEMENT_REFLECTION because they generate a `GetClassType()` body that
// always returns the static class type. We need a custom override so that
// instances created via the synthetic Factory creator report their per-name
// TypeClass instead — that's what makes XAML `Style TargetType="aor:Foo"`
// matching work. The hand-rolled version reuses Noesis's own
// `TypeClassCreator::Create` / `Fill` template machinery, so `TypeOf<>`,
// `RegisterType`, and `IsAssignableFrom` all behave normally.

class RustContentControl: public Noesis::ContentControl {
public:
    RustContentControl() = default;

    ~RustContentControl() {
        // Release this instance's share of the ClassData refcount. This
        // is the deferred-free path: if `dm_noesis_class_unregister` ran
        // before the View tore down (typical Bevy resource-drop order),
        // ClassData is still alive at this point and the last instance
        // releasing it triggers `free_handler` + `delete cd`.
        if (mClassData) {
            mClassData->Release();
            mClassData = nullptr;
        }
    }

    // BindClassData is called exactly once per instance (from `class_creator`)
    // before the visual tree sees this object. The +1 ref taken here is paired
    // with the dtor's Release.
    void BindClassData(ClassData* cd) {
        if (mClassData) mClassData->Release();
        mClassData = cd;
        if (cd) cd->AddRef();
    }
    ClassData* GetClassData() const { return mClassData; }

    // Custom reflection — see comment above.
    static const Noesis::TypeClass* StaticGetClassType(Noesis::TypeTag<RustContentControl>*);
    const Noesis::TypeClass* GetClassType() const override;

protected:
    bool OnPropertyChanged(const Noesis::DependencyPropertyChangedEventArgs& args) override {
        bool processed = ContentControl::OnPropertyChanged(args);
        ForwardChange(args);
        return processed;
    }

private:
    void ForwardChange(const Noesis::DependencyPropertyChangedEventArgs& args);

    ClassData* mClassData = nullptr;

    // Required by TypeClassCreator::Fill<SelfClass, ParentClass>.
    typedef RustContentControl SelfClass;
    typedef Noesis::ContentControl ParentClass;
    friend class Noesis::TypeClassCreator;
    static void StaticFillClassType(Noesis::TypeClassCreator& /*helper*/) {
        // No statically-declared DPs — every consumer adds their own via
        // dm_noesis_class_register_property against a synthetic TypeClass.
    }
};

const Noesis::TypeClass*
RustContentControl::StaticGetClassType(Noesis::TypeTag<RustContentControl>*) {
    static const Noesis::TypeClass* type;
    if (NS_UNLIKELY(type == 0)) {
        type = static_cast<const Noesis::TypeClass*>(Noesis::Reflection::RegisterType(
            "DmNoesis.RustContentControl",
            Noesis::TypeClassCreator::Create<RustContentControl>,
            Noesis::TypeClassCreator::Fill<RustContentControl, Noesis::ContentControl>));
    }
    return type;
}

const Noesis::TypeClass* RustContentControl::GetClassType() const {
    if (mClassData && mClassData->typeClass) {
        return static_cast<const Noesis::TypeClass*>(mClassData->typeClass);
    }
    return StaticGetClassType((Noesis::TypeTag<RustContentControl>*)nullptr);
}

// ── Marshaling helpers ─────────────────────────────────────────────────────

void invoke_cb(ClassData* cd, void* instance, uint32_t idx,
               dm_noesis_prop_type ty, const void* raw) {
    if (!cd->cb) return;

    switch (ty) {
        case DM_NOESIS_PROP_INT32:
        case DM_NOESIS_PROP_UINT32:
        case DM_NOESIS_PROP_FLOAT:
        case DM_NOESIS_PROP_DOUBLE:
        case DM_NOESIS_PROP_BOOL:
        case DM_NOESIS_PROP_THICKNESS:
        case DM_NOESIS_PROP_COLOR:
        case DM_NOESIS_PROP_RECT:
            // Pass through directly — `raw` already points to the typed value.
            cd->cb(cd->userdata, instance, idx, raw);
            return;

        case DM_NOESIS_PROP_STRING: {
            // Noesis stores String values as Noesis::String (FixedString<24>);
            // expose as `const char*` to Rust via a borrowed pointer.
            const auto* s = static_cast<const Noesis::String*>(raw);
            const char* c = s ? s->Str() : nullptr;
            cd->cb(cd->userdata, instance, idx, &c);
            return;
        }

        case DM_NOESIS_PROP_IMAGE_SOURCE:
        case DM_NOESIS_PROP_BASE_COMPONENT: {
            // Noesis stores object values as Ptr<T> — the raw pointer is to a
            // Ptr<BaseComponent>. Unbox to a borrowed BaseComponent*.
            const auto* p = static_cast<const Noesis::Ptr<Noesis::BaseComponent>*>(raw);
            Noesis::BaseComponent* b = p ? p->GetPtr() : nullptr;
            cd->cb(cd->userdata, instance, idx, &b);
            return;
        }
    }
}

void RustContentControl::ForwardChange(
    const Noesis::DependencyPropertyChangedEventArgs& args) {
    if (!mClassData) return;
    for (uint32_t i = 0; i < mClassData->properties.size(); ++i) {
        const auto& pe = mClassData->properties[i];
        if (pe.dp == args.prop) {
            invoke_cb(mClassData, this, i, pe.type, args.newValue);
            return;
        }
    }
}

// ── Factory creator ────────────────────────────────────────────────────────

Noesis::BaseComponent* class_creator(Noesis::Symbol name) {
    ClassData* cd = registry_find(name);
    if (!cd) return nullptr;

    // Only ContentControl base for v1. Future bases dispatch on the base
    // tag we'd need to stash in ClassData.
    auto* instance = new RustContentControl();
    instance->BindClassData(cd);
    return instance;
}

// ── DP creation: dispatch by FFI type tag ──────────────────────────────────

Noesis::Ptr<Noesis::DependencyProperty> create_dp(
    const char* name,
    const Noesis::TypeClass* owner,
    dm_noesis_prop_type type,
    const void* default_ptr) {
    using namespace Noesis;
    switch (type) {
        case DM_NOESIS_PROP_INT32: {
            int32_t def = default_ptr ? *static_cast<const int32_t*>(default_ptr) : 0;
            return DependencyProperty::Create<int32_t>(
                name, owner, PropertyMetadata::Create(def), nullptr);
        }
        case DM_NOESIS_PROP_UINT32: {
            uint32_t def = default_ptr ? *static_cast<const uint32_t*>(default_ptr) : 0;
            return DependencyProperty::Create<uint32_t>(
                name, owner, PropertyMetadata::Create(def), nullptr);
        }
        case DM_NOESIS_PROP_FLOAT: {
            float def = default_ptr ? *static_cast<const float*>(default_ptr) : 0.0f;
            return DependencyProperty::Create<float>(
                name, owner, PropertyMetadata::Create(def), nullptr);
        }
        case DM_NOESIS_PROP_DOUBLE: {
            double def = default_ptr ? *static_cast<const double*>(default_ptr) : 0.0;
            return DependencyProperty::Create<double>(
                name, owner, PropertyMetadata::Create(def), nullptr);
        }
        case DM_NOESIS_PROP_BOOL: {
            bool def = default_ptr ? *static_cast<const bool*>(default_ptr) : false;
            return DependencyProperty::Create<bool>(
                name, owner, PropertyMetadata::Create(def), nullptr);
        }
        case DM_NOESIS_PROP_STRING: {
            const char* def = default_ptr ? *static_cast<const char* const*>(default_ptr)
                                          : nullptr;
            String s = def ? String(def) : String();
            return DependencyProperty::Create<String>(
                name, owner, PropertyMetadata::Create(s), nullptr);
        }
        case DM_NOESIS_PROP_THICKNESS: {
            Thickness def;
            if (default_ptr) {
                const auto* f = static_cast<const float*>(default_ptr);
                def = Thickness(f[0], f[1], f[2], f[3]);
            }
            return DependencyProperty::Create<Thickness>(
                name, owner, PropertyMetadata::Create(def), nullptr);
        }
        case DM_NOESIS_PROP_COLOR: {
            Color def;
            if (default_ptr) {
                const auto* f = static_cast<const float*>(default_ptr);
                def = Color(f[0], f[1], f[2], f[3]);
            }
            return DependencyProperty::Create<Color>(
                name, owner, PropertyMetadata::Create(def), nullptr);
        }
        case DM_NOESIS_PROP_RECT: {
            Rect def;
            if (default_ptr) {
                const auto* f = static_cast<const float*>(default_ptr);
                def = Rect(f[0], f[1], f[0] + f[2], f[1] + f[3]);
            }
            return DependencyProperty::Create<Rect>(
                name, owner, PropertyMetadata::Create(def), nullptr);
        }
        case DM_NOESIS_PROP_IMAGE_SOURCE: {
            // Always seed an explicit null `Ptr<BaseComponent>` default —
            // without one, `DependencyObject::Init` walks the property
            // metadata and asks `ValueStorageManagerImpl<Ptr<...>>::Box`
            // to box a missing source. With certain XAML structures
            // (e.g. our RustContentControl as a Grid child whose Source
            // isn't set in attribute syntax) the Box source pointer is
            // null and Noesis crashes inside the typed `Init` path. The
            // existing safety_smoke "Block 2" reproduces this once the
            // synthetic class participates in a real visual-tree walk.
            //
            // Overriding non-null defaults from FFI is a v2 feature; AoR's
            // slicers default Source to null anyway.
            Ptr<BaseComponent> null_default;
            return DependencyProperty::Create<Ptr<BaseComponent>>(
                name, TypeOf<ImageSource>(), owner,
                PropertyMetadata::Create<Ptr<BaseComponent>>(null_default), nullptr);
        }
        case DM_NOESIS_PROP_BASE_COMPONENT: {
            // Same Box(null) crash class as IMAGE_SOURCE above.
            Ptr<BaseComponent> null_default;
            return DependencyProperty::Create<Ptr<BaseComponent>>(
                name, TypeOf<BaseComponent>(), owner,
                PropertyMetadata::Create<Ptr<BaseComponent>>(null_default), nullptr);
        }
    }
    return nullptr;
}

}  // namespace

// ── C ABI surface ──────────────────────────────────────────────────────────

extern "C" void* dm_noesis_class_register(
    const char* name,
    dm_noesis_class_base base,
    dm_noesis_prop_changed_fn cb,
    void* userdata,
    dm_noesis_class_free_fn free_handler) {
    if (!name) return nullptr;
    if (base != DM_NOESIS_BASE_CONTENT_CONTROL) return nullptr;

    Noesis::Symbol sym = Noesis::Symbol(name);

    // Reject duplicate names so callers see the failure rather than silently
    // shadowing an earlier registration with a stale ClassData* dangling
    // inside the Factory creator path.
    if (Noesis::Reflection::IsTypeRegistered(sym)) {
        return nullptr;
    }

    auto* cd = new ClassData();
    cd->name = name;
    cd->sym = sym;
    cd->cb = cb;
    cd->userdata = userdata;
    cd->free_handler = free_handler;

    // Build the synthetic TypeClass. Reflection::RegisterType assumes
    // ownership and deletes it on Unregister / Shutdown.
    cd->typeClass = new Noesis::TypeClassBuilder(sym, /*isInterface*/ false);
    cd->typeClass->AddBase(Noesis::TypeOf<RustContentControl>());

    cd->uiData = Noesis::MakePtr<Noesis::UIElementData>(cd->typeClass);
    cd->typeClass->AddMeta(cd->uiData.GetPtr());

    Noesis::Reflection::RegisterType(cd->typeClass);
    Noesis::Factory::RegisterComponent(sym, Noesis::Symbol(""), class_creator);

    if (!registry_insert(sym, cd)) {
        // Symbol collision after the IsTypeRegistered check — extremely
        // unlikely, but unwind to keep the registry consistent. No
        // instances exist yet (Factory just registered, no XAML has
        // referenced this class), no destructor chain is in play, and
        // `cd` hasn't been added to the shutdown sweep list. Free
        // everything fully — including ClassData itself (and via its
        // member, the Ptr<UIElementData>).
        Noesis::Factory::UnregisterComponent(sym);
        Noesis::Reflection::Unregister(cd->typeClass);
        if (cd->free_handler && cd->userdata) {
            cd->free_handler(cd->userdata);
        }
        delete cd;
        return nullptr;
    }

    // Registration succeeded — only NOW add to the shutdown sweep list.
    // On the failure branch above we fully tore cd down, so it must
    // never appear in the list.
    track_class_data(cd);

    return cd;
}

// Instantiate a registered class directly from Rust (no XAML reference needed).
// Returns a BaseComponent* with +1 ref for the caller, released via
// dm_noesis_base_component_release. NULL on null token.
//
// The motivating use is data binding: a synthetic class is a DependencyObject
// with registered DPs, so an instance created here makes a perfectly good
// binding source / view model — set it as an element's DataContext and author
// `{Binding SomeDP}` in XAML. Writing a DP from Rust (dm_noesis_instance_set_*)
// raises the DependencyObject change notification the binding engine observes.
extern "C" void* dm_noesis_class_create_instance(void* class_token) {
    if (!class_token) return nullptr;
    auto* cd = static_cast<ClassData*>(class_token);
    auto* instance = new RustContentControl();
    instance->BindClassData(cd);
    instance->AddReference();  // +1 for the caller; paired with base_component_release
    return static_cast<Noesis::BaseComponent*>(instance);
}

extern "C" uint32_t dm_noesis_class_register_property(
    void* class_token,
    const char* prop_name,
    dm_noesis_prop_type prop_type,
    const void* default_ptr) {
    if (!class_token || !prop_name) return UINT32_MAX;
    auto* cd = static_cast<ClassData*>(class_token);

    auto dp = create_dp(prop_name, cd->typeClass, prop_type, default_ptr);
    if (!dp) return UINT32_MAX;

    const Noesis::DependencyProperty* installed = cd->uiData->InsertProperty(dp.GetPtr());
    if (!installed) return UINT32_MAX;

    cd->properties.push_back({installed, prop_type});
    return static_cast<uint32_t>(cd->properties.size() - 1);
}

extern "C" void dm_noesis_class_unregister(void* class_token) {
    if (!class_token) return;
    auto* cd = static_cast<ClassData*>(class_token);

    // Stop new instances from being created. Existing instances keep
    // their ClassData reference; the typeClass / uiData / ClassData
    // allocations stay alive because the parent destructor chain
    // (`~ContentControl` → `~FrameworkElement` → `~DependencyObject`)
    // still walks the type metadata after `~RustContentControl`.
    Noesis::Factory::UnregisterComponent(cd->sym);
    registry_erase(cd->sym);

    // Release the Rust caller's ref. If no instances are alive, the
    // Rust handler box is freed here (refcount → 0 → free_handler).
    // Otherwise the freeing is deferred to the last instance dying.
    cd->Release();
}

// Called from `dm_noesis_shutdown` AFTER `Noesis::Shutdown` has destroyed
// every live DependencyObject — defensively releases any handler boxes
// whose owning instances never fired the refcount-driven cleanup. (In
// practice the per-instance Release calls already nulled `userdata` on
// every entry; this loop is a belt-and-suspenders safeguard for paths
// that bypass normal teardown — e.g. orphaned Views never `drop`-ed.)
//
// Does NOT delete the ClassData / typeClass / uiData. Their lifetimes
// are entangled with Noesis's internal Reflection registry, and the
// safe cross-FFI ordering for tearing them down is "after Noesis is
// fully shut down" — at which point the OS reaps the process anyway.
// One ClassData per registered class is a bounded leak; gain is that
// no destructor walks dangling Noesis state.
extern "C" void dm_noesis_classes_force_free_at_shutdown(void) {
    std::vector<ClassData*> all;
    {
        std::lock_guard<std::mutex> lock(g_all_class_data_mutex);
        all = std::move(g_all_class_data);
    }
    for (ClassData* cd : all) {
        // Most entries already have userdata=null (their refcount-driven
        // free already ran during normal instance teardown); the null
        // check makes this iteration a no-op for them. Non-null entries
        // belong to classes whose instances bypassed normal destruction.
        void* ud = cd->userdata;
        cd->userdata = nullptr;
        if (cd->free_handler && ud) {
            cd->free_handler(ud);
        }
    }
}

namespace {

// Helper: locate the prop entry on an instance.
const PropEntry* instance_prop(void* instance, uint32_t prop_index, ClassData** out_cd) {
    if (!instance) return nullptr;
    auto* tramp = static_cast<RustContentControl*>(instance);
    ClassData* cd = tramp->GetClassData();
    if (!cd || prop_index >= cd->properties.size()) return nullptr;
    if (out_cd) *out_cd = cd;
    return &cd->properties[prop_index];
}

// Which setter the per-tag write switch dispatches to.
//
//   * Local   → `SetValue<T>` / `SetValueObject`  (the normal local value)
//   * Current → `SetCurrentValue<T>` / `SetCurrentValueObject` (sets the
//     coerce field without overwriting the source / local value — see
//     `dm_noesis_dependency_object_set_current_value`).
enum class SetMode { Local, Current };

// Which getter the per-tag read switch dispatches to.
//
//   * Effective → `GetValue<T>` / `GetValueObject`  (the resolved value)
//   * Base      → `GetBaseValue<T>` (value before animation / coerce; there is
//     no `GetBaseValueObject` form, so object tags are unsupported in this
//     mode — see `apply_get`).
enum class GetMode { Effective, Base };

// Shared per-`dm_noesis_prop_type` boxing switch for writes. Reused by the
// instance path (`dm_noesis_instance_set_property`), the generic
// DependencyObject path (`dm_noesis_dependency_object_set_property`), the
// attached-property path, and the current-value path (`mode`). The caller is
// responsible for any type validation; this just marshals the FFI buffer into a
// typed `SetValue` / `SetCurrentValue`, matching the value-buffer layouts
// documented in noesis_shim.h verbatim (note the Rect (x,y,w,h)->(x,y,x+w,y+h)
// convention).
void apply_set(
    Noesis::DependencyObject* obj,
    const Noesis::DependencyProperty* dp,
    dm_noesis_prop_type type,
    const void* value_ptr,
    SetMode mode = SetMode::Local) {
    using namespace Noesis;
    switch (type) {
        case DM_NOESIS_PROP_INT32: {
            int32_t v = value_ptr ? *static_cast<const int32_t*>(value_ptr) : 0;
            if (mode == SetMode::Current) obj->SetCurrentValue<int32_t>(dp, v);
            else obj->SetValue<int32_t>(dp, v);
            return;
        }
        case DM_NOESIS_PROP_UINT32: {
            uint32_t v = value_ptr ? *static_cast<const uint32_t*>(value_ptr) : 0;
            if (mode == SetMode::Current) obj->SetCurrentValue<uint32_t>(dp, v);
            else obj->SetValue<uint32_t>(dp, v);
            return;
        }
        case DM_NOESIS_PROP_FLOAT: {
            float v = value_ptr ? *static_cast<const float*>(value_ptr) : 0.0f;
            if (mode == SetMode::Current) obj->SetCurrentValue<float>(dp, v);
            else obj->SetValue<float>(dp, v);
            return;
        }
        case DM_NOESIS_PROP_DOUBLE: {
            double v = value_ptr ? *static_cast<const double*>(value_ptr) : 0.0;
            if (mode == SetMode::Current) obj->SetCurrentValue<double>(dp, v);
            else obj->SetValue<double>(dp, v);
            return;
        }
        case DM_NOESIS_PROP_BOOL: {
            bool v = value_ptr ? *static_cast<const bool*>(value_ptr) : false;
            if (mode == SetMode::Current) obj->SetCurrentValue<bool>(dp, v);
            else obj->SetValue<bool>(dp, v);
            return;
        }
        case DM_NOESIS_PROP_STRING: {
            // SetValueType<String>::Type is `const char*` — pass the C string
            // directly. Noesis copies into its own String storage.
            const char* s = value_ptr ? *static_cast<const char* const*>(value_ptr)
                                      : nullptr;
            const char* safe = s ? s : "";
            if (mode == SetMode::Current) obj->SetCurrentValue<String>(dp, safe);
            else obj->SetValue<String>(dp, safe);
            return;
        }
        case DM_NOESIS_PROP_THICKNESS: {
            Thickness t;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                t = Thickness(f[0], f[1], f[2], f[3]);
            }
            if (mode == SetMode::Current) obj->SetCurrentValue<Thickness>(dp, t);
            else obj->SetValue<Thickness>(dp, t);
            return;
        }
        case DM_NOESIS_PROP_COLOR: {
            Color c;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                c = Color(f[0], f[1], f[2], f[3]);
            }
            if (mode == SetMode::Current) obj->SetCurrentValue<Color>(dp, c);
            else obj->SetValue<Color>(dp, c);
            return;
        }
        case DM_NOESIS_PROP_RECT: {
            Rect r;
            if (value_ptr) {
                const auto* f = static_cast<const float*>(value_ptr);
                r = Rect(f[0], f[1], f[0] + f[2], f[1] + f[3]);
            }
            if (mode == SetMode::Current) obj->SetCurrentValue<Rect>(dp, r);
            else obj->SetValue<Rect>(dp, r);
            return;
        }
        case DM_NOESIS_PROP_IMAGE_SOURCE:
        case DM_NOESIS_PROP_BASE_COMPONENT: {
            BaseComponent* b = value_ptr ? *static_cast<BaseComponent* const*>(value_ptr)
                                         : nullptr;
            if (mode == SetMode::Current) obj->SetCurrentValueObject(dp, b);
            else obj->SetValueObject(dp, b);
            return;
        }
    }
}

// Shared per-`dm_noesis_prop_type` unboxing switch for reads. Mirror of
// `apply_set`, reused by the instance path, the generic DependencyObject path,
// the attached-property path, and the base-value path (`mode`). `out_value`
// must already be non-null. Reference / string returns borrow Noesis-owned
// storage (no +1 ref / no copy) — the caller must copy immediately, per the
// noesis_shim.h ownership contract.
//
// `GetMode::Base` reads `GetBaseValue<T>` (pre-animation / pre-coerce). Noesis
// exposes no boxed `GetBaseValueObject`, so object tags (IMAGE_SOURCE /
// BASE_COMPONENT) are unsupported in Base mode and return false.
bool apply_get(
    Noesis::DependencyObject* obj,
    const Noesis::DependencyProperty* dp,
    dm_noesis_prop_type type,
    void* out_value,
    GetMode mode = GetMode::Effective) {
    using namespace Noesis;
    const bool base = mode == GetMode::Base;
    switch (type) {
        case DM_NOESIS_PROP_INT32:
            *static_cast<int32_t*>(out_value) =
                base ? obj->GetBaseValue<int32_t>(dp) : obj->GetValue<int32_t>(dp);
            return true;
        case DM_NOESIS_PROP_UINT32:
            *static_cast<uint32_t*>(out_value) =
                base ? obj->GetBaseValue<uint32_t>(dp) : obj->GetValue<uint32_t>(dp);
            return true;
        case DM_NOESIS_PROP_FLOAT:
            *static_cast<float*>(out_value) =
                base ? obj->GetBaseValue<float>(dp) : obj->GetValue<float>(dp);
            return true;
        case DM_NOESIS_PROP_DOUBLE:
            *static_cast<double*>(out_value) =
                base ? obj->GetBaseValue<double>(dp) : obj->GetValue<double>(dp);
            return true;
        case DM_NOESIS_PROP_BOOL:
            *static_cast<bool*>(out_value) =
                base ? obj->GetBaseValue<bool>(dp) : obj->GetValue<bool>(dp);
            return true;
        case DM_NOESIS_PROP_STRING: {
            const String& s = base ? obj->GetBaseValue<String>(dp) : obj->GetValue<String>(dp);
            *static_cast<const char**>(out_value) = s.Str();
            return true;
        }
        case DM_NOESIS_PROP_THICKNESS: {
            const Thickness& t =
                base ? obj->GetBaseValue<Thickness>(dp) : obj->GetValue<Thickness>(dp);
            auto* f = static_cast<float*>(out_value);
            f[0] = t.left; f[1] = t.top; f[2] = t.right; f[3] = t.bottom;
            return true;
        }
        case DM_NOESIS_PROP_COLOR: {
            const Color& c = base ? obj->GetBaseValue<Color>(dp) : obj->GetValue<Color>(dp);
            auto* f = static_cast<float*>(out_value);
            f[0] = c.r; f[1] = c.g; f[2] = c.b; f[3] = c.a;
            return true;
        }
        case DM_NOESIS_PROP_RECT: {
            const Rect& r = base ? obj->GetBaseValue<Rect>(dp) : obj->GetValue<Rect>(dp);
            auto* f = static_cast<float*>(out_value);
            f[0] = r.x; f[1] = r.y; f[2] = r.width; f[3] = r.height;
            return true;
        }
        case DM_NOESIS_PROP_IMAGE_SOURCE:
        case DM_NOESIS_PROP_BASE_COMPONENT: {
            // No boxed base-value accessor exists — object tags only resolve
            // in Effective mode.
            if (base) return false;
            Ptr<BaseComponent> v = obj->GetValueObject(dp);
            *static_cast<BaseComponent**>(out_value) = v.GetPtr();
            return true;
        }
    }
    return false;
}

// Validate that a caller-supplied `dm_noesis_prop_type` tag matches the real
// `Type*` of a resolved DependencyProperty. The generic name-keyed path must
// not trust the caller's tag blindly — a wrong tag would drive a wrong cast
// (UB). Value / struct types compare against the exact reflected `Type*`;
// reference types use `IsAssignableFrom` so a base tag accepts any subclass.
bool prop_type_matches(const Noesis::Type* t, dm_noesis_prop_type tag) {
    using namespace Noesis;
    if (!t) return false;
    switch (tag) {
        case DM_NOESIS_PROP_INT32:     return t == TypeOf<int32_t>();
        case DM_NOESIS_PROP_UINT32:    return t == TypeOf<uint32_t>();
        case DM_NOESIS_PROP_FLOAT:     return t == TypeOf<float>();
        case DM_NOESIS_PROP_DOUBLE:    return t == TypeOf<double>();
        case DM_NOESIS_PROP_BOOL:      return t == TypeOf<bool>();
        case DM_NOESIS_PROP_STRING:    return t == TypeOf<String>();
        case DM_NOESIS_PROP_THICKNESS: return t == TypeOf<Thickness>();
        case DM_NOESIS_PROP_COLOR:     return t == TypeOf<Color>();
        case DM_NOESIS_PROP_RECT:      return t == TypeOf<Rect>();
        case DM_NOESIS_PROP_IMAGE_SOURCE:
            return TypeOf<ImageSource>()->IsAssignableFrom(t);
        case DM_NOESIS_PROP_BASE_COMPONENT:
            return TypeOf<BaseComponent>()->IsAssignableFrom(t);
    }
    return false;
}

// Inverse of `prop_type_matches`: map a resolved DependencyProperty `Type*` to
// its `dm_noesis_prop_type` tag, or -1 when the type corresponds to no tag.
// Value / struct types compare by exact `Type*`; reference types fall through
// to the assignable-from checks (ImageSource before the broader BaseComponent).
int32_t prop_type_to_tag(const Noesis::Type* t) {
    using namespace Noesis;
    if (!t) return -1;
    if (t == TypeOf<int32_t>())   return DM_NOESIS_PROP_INT32;
    if (t == TypeOf<uint32_t>())  return DM_NOESIS_PROP_UINT32;
    if (t == TypeOf<float>())     return DM_NOESIS_PROP_FLOAT;
    if (t == TypeOf<double>())    return DM_NOESIS_PROP_DOUBLE;
    if (t == TypeOf<bool>())      return DM_NOESIS_PROP_BOOL;
    if (t == TypeOf<String>())    return DM_NOESIS_PROP_STRING;
    if (t == TypeOf<Thickness>()) return DM_NOESIS_PROP_THICKNESS;
    if (t == TypeOf<Color>())     return DM_NOESIS_PROP_COLOR;
    if (t == TypeOf<Rect>())      return DM_NOESIS_PROP_RECT;
    if (TypeOf<ImageSource>()->IsAssignableFrom(t)) return DM_NOESIS_PROP_IMAGE_SOURCE;
    if (TypeOf<BaseComponent>()->IsAssignableFrom(t)) return DM_NOESIS_PROP_BASE_COMPONENT;
    return -1;
}

// Resolve `obj` to a DependencyObject and `name` to one of its dependency
// properties (searching the inherited class hierarchy). Returns the property,
// or null (with `*out_d` set to the cast object when non-null) on any failure.
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

extern "C" void dm_noesis_instance_set_property(
    void* instance,
    uint32_t prop_index,
    const void* value_ptr) {
    const PropEntry* pe = instance_prop(instance, prop_index, nullptr);
    if (!pe) return;
    auto* obj = static_cast<Noesis::DependencyObject*>(static_cast<RustContentControl*>(instance));
    apply_set(obj, pe->dp, pe->type, value_ptr);
}

extern "C" bool dm_noesis_image_source_get_size(
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

extern "C" bool dm_noesis_instance_get_property(
    void* instance,
    uint32_t prop_index,
    void* out_value) {
    const PropEntry* pe = instance_prop(instance, prop_index, nullptr);
    if (!pe || !out_value) return false;
    auto* obj = static_cast<Noesis::DependencyObject*>(static_cast<RustContentControl*>(instance));
    return apply_get(obj, pe->dp, pe->type, out_value);
}

// ── Generic name-keyed DependencyProperty access ───────────────────────────
//
// Unlike the instance path above (which trusts a dense index into a
// Rust-registered class), these resolve a DependencyProperty by *name* on an
// arbitrary DependencyObject, then marshal through the same per-type switch.
// Because the caller supplies the type tag, we validate it against the
// property's real reflected type before casting — a mismatch returns false
// rather than risking a bad cast.
//
// No VerifyAccess() — these must never throw across the C ABI (mirrors the
// text_get/set accessors). Single-thread (View) affinity is the caller's
// responsibility.

extern "C" bool dm_noesis_dependency_object_set_property(
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

    auto type = static_cast<dm_noesis_prop_type>(prop_type);
    if (!prop_type_matches(dp->GetType(), type)) return false;
    if (dp->IsReadOnly()) return false;

    apply_set(d, dp, type, value_ptr);
    return true;
}

extern "C" bool dm_noesis_dependency_object_get_property(
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

    auto type = static_cast<dm_noesis_prop_type>(prop_type);
    if (!prop_type_matches(dp->GetType(), type)) return false;

    return apply_get(d, dp, type, out_value);
}

// ── Attached properties (TODO §2.B) ─────────────────────────────────────────
//
// Resolve a DependencyProperty registered on `owner_type` (e.g. owner="Grid",
// prop="Row"; owner="Canvas", prop="Left"), then set / get it on `obj`. Same
// prop_type tag layout + validation as the generic name-keyed path. The owner
// type is resolved through Reflection by name and must already be registered
// (referencing it from XAML, or any prior use, forces registration).

extern "C" bool dm_noesis_dependency_object_set_attached(
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

    auto type = static_cast<dm_noesis_prop_type>(prop_type);
    if (!prop_type_matches(dp->GetType(), type)) return false;
    if (dp->IsReadOnly()) return false;

    apply_set(d, dp, type, value_ptr);
    return true;
}

extern "C" bool dm_noesis_dependency_object_get_attached(
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

    auto type = static_cast<dm_noesis_prop_type>(prop_type);
    if (!prop_type_matches(dp->GetType(), type)) return false;

    return apply_get(d, dp, type, out_value);
}

// ── ClearValue / SetCurrentValue / GetBaseValue (TODO §2.C) ─────────────────

extern "C" bool dm_noesis_dependency_object_clear_value(void* obj, const char* name) {
    Noesis::DependencyObject* d = nullptr;
    const Noesis::DependencyProperty* dp = resolve_dp(obj, name, &d);
    if (!dp) return false;
    if (dp->IsReadOnly()) return false;
    d->ClearLocalValue(dp);
    return true;
}

extern "C" bool dm_noesis_dependency_object_set_current_value(
    void* obj,
    const char* name,
    uint32_t prop_type,
    const void* value_ptr) {
    Noesis::DependencyObject* d = nullptr;
    const Noesis::DependencyProperty* dp = resolve_dp(obj, name, &d);
    if (!dp) return false;

    auto type = static_cast<dm_noesis_prop_type>(prop_type);
    if (!prop_type_matches(dp->GetType(), type)) return false;
    if (dp->IsReadOnly()) return false;

    apply_set(d, dp, type, value_ptr, SetMode::Current);
    return true;
}

extern "C" bool dm_noesis_dependency_object_get_base_value(
    void* obj,
    const char* name,
    uint32_t prop_type,
    void* out_value) {
    if (!out_value) return false;
    Noesis::DependencyObject* d = nullptr;
    const Noesis::DependencyProperty* dp = resolve_dp(obj, name, &d);
    if (!dp) return false;

    auto type = static_cast<dm_noesis_prop_type>(prop_type);
    if (!prop_type_matches(dp->GetType(), type)) return false;

    // Object tags have no boxed base-value accessor — apply_get returns false.
    return apply_get(d, dp, type, out_value, GetMode::Base);
}

// ── Dynamic tag inference (TODO §2.D) ───────────────────────────────────────

extern "C" int32_t dm_noesis_dependency_object_property_tag(void* obj, const char* name) {
    const Noesis::DependencyProperty* dp = resolve_dp(obj, name, nullptr);
    if (!dp) return -1;
    return prop_type_to_tag(dp->GetType());
}
