// Custom MarkupExtension registration FFI (Phase 5.D).
//
// Same architectural pattern as noesis_classes.cpp: a per-base C++
// trampoline subclass + synthetic per-name TypeClassBuilder + Factory
// creator + Symbol → ClassData side table. The trampoline's virtual
// override is `ProvideValue` (rather than `OnPropertyChanged`), and it
// dispatches to the Rust callback with the current `Key` value the XAML
// parser set via the ContentProperty mechanism.
//
// v1 scope: a single positional `Key` string argument. Returns either a
// borrowed C string (most common — wrapped into a BoxedValue<String>) or
// a borrowed BaseComponent* (for value types that can't be expressed as
// text, e.g. an existing resource lookup). Reactive bindings (locale
// switch updates UI in place) are deferred to a later PR — they need a
// LocalizationManager-style indexer + Binding, which is its own design.

#include "noesis_shim.h"

#include <NsCore/Boxing.h>
#include <NsCore/Factory.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsCore/Reflection.h>
#include <NsCore/ReflectionImplement.h>
#include <NsCore/String.h>
#include <NsCore/Symbol.h>
#include <NsCore/TypeClassBuilder.h>
#include <NsCore/TypeClassCreator.h>
#include <NsCore/TypeOf.h>
#include <NsGui/ContentPropertyMetaData.h>
#include <NsGui/MarkupExtension.h>
#include <NsGui/ValueTargetProvider.h>

#include <atomic>
#include <mutex>
#include <unordered_map>
#include <vector>

namespace {

// ── ClassData + registry ───────────────────────────────────────────────────

// Same intrusive-refcount model as ClassData in noesis_classes.cpp — see
// the comment there for the full lifetime contract. Short version: each
// live `RustMarkupExtension` instance bumps the count; the Rust caller's
// `MarkupExtensionRegistration` holds the +1 created at register time;
// final free runs the `free_handler` Rust trampoline.
struct MarkupClassData {
    Noesis::String                 name;
    Noesis::Symbol                 sym;
    Noesis::TypeClassBuilder*      typeClass; // owned by Reflection registry
    dm_noesis_markup_provide_fn    cb;
    void*                          userdata;
    dm_noesis_markup_free_fn       free_handler;
    std::atomic<int>               ref_count;

    MarkupClassData(): ref_count(1) {}

    void AddRef() noexcept {
        ref_count.fetch_add(1, std::memory_order_relaxed);
    }

    void Release() {
        if (ref_count.fetch_sub(1, std::memory_order_acq_rel) == 1) {
            std::atomic_thread_fence(std::memory_order_acquire);
            // See ClassData::Release in noesis_classes.cpp — same
            // rationale: never delete typeClass or `this` from inside
            // the destructor chain. Just free the Rust handler box;
            // the rest leaks until process shutdown sweeps it.
            void* ud = userdata;
            userdata = nullptr;
            if (free_handler && ud) {
                free_handler(ud);
            }
        }
    }
};

std::mutex                                              g_markup_registry_mutex;
std::unordered_map<uint32_t, MarkupClassData*>          g_markup_registry;

// Same shape as g_all_class_data in noesis_classes.cpp — see the comment
// there. Holds every successfully-registered MarkupClassData; the
// shutdown sweep iterates the whole list and frees any handler box
// whose `userdata` is still set (entries with userdata=null are
// no-ops, the common case after normal teardown).
std::mutex                                              g_all_markup_data_mutex;
std::vector<MarkupClassData*>                           g_all_markup_data;

void track_markup_data(MarkupClassData* cd) {
    std::lock_guard<std::mutex> lock(g_all_markup_data_mutex);
    g_all_markup_data.push_back(cd);
}

MarkupClassData* markup_registry_find(Noesis::Symbol sym) {
    std::lock_guard<std::mutex> lock(g_markup_registry_mutex);
    auto it = g_markup_registry.find((uint32_t)sym);
    return it == g_markup_registry.end() ? nullptr : it->second;
}

bool markup_registry_insert(Noesis::Symbol sym, MarkupClassData* cd) {
    std::lock_guard<std::mutex> lock(g_markup_registry_mutex);
    return g_markup_registry.emplace((uint32_t)sym, cd).second;
}

void markup_registry_erase(Noesis::Symbol sym) {
    std::lock_guard<std::mutex> lock(g_markup_registry_mutex);
    g_markup_registry.erase((uint32_t)sym);
}

// ── Trampoline subclass: MarkupExtension ───────────────────────────────────
//
// Same hand-rolled-reflection pattern as noesis_classes.cpp's
// RustContentControl: NS_DECLARE_REFLECTION's macros generate a
// GetClassType() that always returns the static type, but we need our
// override to report the synthetic per-name class so XAML's parser
// finds the right factory creator.

class RustMarkupExtension: public Noesis::MarkupExtension {
public:
    Noesis::String Key; // ContentProperty — populated by XAML parser

    RustMarkupExtension() = default;

    ~RustMarkupExtension() {
        if (mClassData) {
            mClassData->Release();
            mClassData = nullptr;
        }
    }

    void BindClassData(MarkupClassData* cd) {
        if (mClassData) mClassData->Release();
        mClassData = cd;
        if (cd) cd->AddRef();
    }
    MarkupClassData* GetClassData() const { return mClassData; }

    Noesis::Ptr<Noesis::BaseComponent>
    ProvideValue(const Noesis::ValueTargetProvider* /*provider*/) override;

    // Hand-rolled reflection — see noesis_classes.cpp::RustContentControl
    // for the rationale.
    static const Noesis::TypeClass*
    StaticGetClassType(Noesis::TypeTag<RustMarkupExtension>*);
    const Noesis::TypeClass* GetClassType() const override;

private:
    MarkupClassData* mClassData = nullptr;

    typedef RustMarkupExtension SelfClass;
    typedef Noesis::MarkupExtension ParentClass;
    friend class Noesis::TypeClassCreator;

    static void StaticFillClassType(Noesis::TypeClassCreator& helper) {
        // Register `Key` as a reflection property so XAML's parser can
        // populate it from `{aor:Localize SOME_KEY}`. Marking it as the
        // ContentProperty makes the positional argument syntax work
        // without callers having to write `Key=...` explicitly.
        helper.Prop("Key", &RustMarkupExtension::Key);
        helper.Meta<Noesis::ContentPropertyMetaData>("Key");
    }
};

const Noesis::TypeClass*
RustMarkupExtension::StaticGetClassType(Noesis::TypeTag<RustMarkupExtension>*) {
    static const Noesis::TypeClass* type;
    if (NS_UNLIKELY(type == 0)) {
        type = static_cast<const Noesis::TypeClass*>(Noesis::Reflection::RegisterType(
            "DmNoesis.RustMarkupExtension",
            Noesis::TypeClassCreator::Create<RustMarkupExtension>,
            Noesis::TypeClassCreator::Fill<RustMarkupExtension, Noesis::MarkupExtension>));
    }
    return type;
}

const Noesis::TypeClass* RustMarkupExtension::GetClassType() const {
    if (mClassData && mClassData->typeClass) {
        return static_cast<const Noesis::TypeClass*>(mClassData->typeClass);
    }
    return StaticGetClassType((Noesis::TypeTag<RustMarkupExtension>*)nullptr);
}

Noesis::Ptr<Noesis::BaseComponent>
RustMarkupExtension::ProvideValue(const Noesis::ValueTargetProvider* /*provider*/) {
    if (!mClassData || !mClassData->cb) {
        return nullptr;
    }

    const char* out_string = nullptr;
    void* out_component = nullptr;
    bool produced = mClassData->cb(
        mClassData->userdata, Key.Str(), &out_string, &out_component);

    if (!produced) {
        // Returning a null Ptr signals UnsetValue to Noesis's parser.
        return nullptr;
    }

    if (out_string) {
        // Box the C string into a BoxedValue<String>. Boxing copies the
        // bytes; the caller's pointer can go away after this call.
        return Noesis::Boxing::Box(out_string);
    }
    if (out_component) {
        // Borrowed BaseComponent*; increment the ref count for the
        // returned Ptr (Noesis::Ptr's adopt-from-raw form would consume
        // the caller's ref, which contract-wise we don't have).
        auto* obj = static_cast<Noesis::BaseComponent*>(out_component);
        return Noesis::Ptr<Noesis::BaseComponent>(obj);
    }
    return nullptr;
}

// ── Factory creator ────────────────────────────────────────────────────────

Noesis::BaseComponent* markup_creator(Noesis::Symbol name) {
    MarkupClassData* cd = markup_registry_find(name);
    if (!cd) return nullptr;
    auto* ext = new RustMarkupExtension();
    ext->BindClassData(cd);
    return ext;
}

}  // namespace

// ── C ABI surface ──────────────────────────────────────────────────────────

extern "C" void* dm_noesis_markup_extension_register(
    const char* name,
    dm_noesis_markup_provide_fn cb,
    void* userdata,
    dm_noesis_markup_free_fn free_handler) {
    if (!name || !cb) return nullptr;

    Noesis::Symbol sym = Noesis::Symbol(name);
    if (Noesis::Reflection::IsTypeRegistered(sym)) {
        return nullptr;
    }

    auto* cd = new MarkupClassData();
    cd->name = name;
    cd->sym = sym;
    cd->cb = cb;
    cd->userdata = userdata;
    cd->free_handler = free_handler;

    cd->typeClass = new Noesis::TypeClassBuilder(sym, /*isInterface*/ false);
    cd->typeClass->AddBase(Noesis::TypeOf<RustMarkupExtension>());

    Noesis::Reflection::RegisterType(cd->typeClass);
    Noesis::Factory::RegisterComponent(sym, Noesis::Symbol(""), markup_creator);

    if (!markup_registry_insert(sym, cd)) {
        // Same fully-torn-down failure path as in noesis_classes.cpp:
        // no instances exist, no destructor chain in play, `cd` not yet
        // in the shutdown sweep list — so free everything including
        // MarkupClassData itself.
        Noesis::Factory::UnregisterComponent(sym);
        Noesis::Reflection::Unregister(cd->typeClass);
        if (cd->free_handler && cd->userdata) {
            cd->free_handler(cd->userdata);
        }
        delete cd;
        return nullptr;
    }

    track_markup_data(cd);

    return cd;
}

extern "C" void dm_noesis_markup_extension_unregister(void* token) {
    if (!token) return;
    auto* cd = static_cast<MarkupClassData*>(token);

    // Stop new instances; existing live instances retain their own refs.
    // Reflection::Unregister is deliberately NOT called — Noesis::Shutdown
    // tears down the registry on its own and walking it manually mid-
    // process trips on instance destructor chains. See noesis_classes.cpp.
    Noesis::Factory::UnregisterComponent(cd->sym);
    markup_registry_erase(cd->sym);

    // Drop the Rust caller's ref. The Rust handler box is freed here if
    // no extension instances are alive, or deferred to the last instance
    // dying (RustMarkupExtension's destructor).
    cd->Release();
}

// Process-shutdown sweep — see noesis_classes.cpp's
// `dm_noesis_classes_force_free_at_shutdown` for the rationale.
extern "C" void dm_noesis_markup_extensions_force_free_at_shutdown(void) {
    std::vector<MarkupClassData*> all;
    {
        std::lock_guard<std::mutex> lock(g_all_markup_data_mutex);
        all = std::move(g_all_markup_data);
    }
    for (MarkupClassData* cd : all) {
        void* ud = cd->userdata;
        cd->userdata = nullptr;
        if (cd->free_handler && ud) {
            cd->free_handler(ud);
        }
    }
}
