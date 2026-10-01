// Rust-backed custom MarkupExtensions, registered by XAML type name.
//
// Built like noesis_classes.cpp: a C++ trampoline subclass, a synthetic
// per-name TypeClassBuilder, a Factory creator, and a Symbol -> class-data
// table. ProvideValue calls the Rust callback with the extension's `Key`, a
// single positional string argument (`{my:Localize SOME_KEY}`).
//
// The callback returns either a borrowed C string, boxed as a string, or a
// borrowed BaseComponent* for values that aren't text (e.g. a resource
// lookup).

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

// Intrusive refcount, as ClassData in noesis_classes.cpp: each live
// RustMarkupExtension holds one, the Rust MarkupExtensionRegistration holds the
// initial one, and the last release frees the Rust handler box.
struct MarkupClassData {
    Noesis::String                 name;
    Noesis::Symbol                 sym;
    Noesis::TypeClassBuilder*      typeClass; // owned by Reflection registry
    noesis_markup_provide_fn    cb;
    void*                          userdata;
    noesis_markup_free_fn       free_handler;
    std::atomic<int>               ref_count;

    MarkupClassData(): ref_count(1) {}

    void AddRef() noexcept {
        ref_count.fetch_add(1, std::memory_order_relaxed);
    }

    void Release() {
        if (ref_count.fetch_sub(1, std::memory_order_acq_rel) == 1) {
            std::atomic_thread_fence(std::memory_order_acquire);
            // Free only the handler box: this runs inside an instance's
            // destructor chain, and typeClass / `this` must stay valid
            // (see ClassData::Release in noesis_classes.cpp). Bounded leak.
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

// Every registered MarkupClassData, never erased. The shutdown sweep frees any
// handler box whose `userdata` is still set.
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
// Hand-rolled reflection: NS_DECLARE_REFLECTION's GetClassType() always
// returns the static type, but instances must report their synthetic per-name
// class so the XAML parser finds the right factory creator.

class RustMarkupExtension: public Noesis::MarkupExtension {
public:
    Noesis::String Key; // ContentProperty, populated by XAML parser

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

    static const Noesis::TypeClass*
    StaticGetClassType(Noesis::TypeTag<RustMarkupExtension>*);
    const Noesis::TypeClass* GetClassType() const override;

private:
    MarkupClassData* mClassData = nullptr;

    typedef RustMarkupExtension SelfClass;
    typedef Noesis::MarkupExtension ParentClass;
    friend class Noesis::TypeClassCreator;

    static void StaticFillClassType(Noesis::TypeClassCreator& helper) {
        // ContentProperty enables the positional form `{my:Localize SOME_KEY}`
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
        // null Ptr reads as UnsetValue to the parser
        return nullptr;
    }

    if (out_string) {
        // Box copies the bytes
        return Noesis::Boxing::Box(out_string);
    }
    if (out_component) {
        // borrowed: Ptr(T*) adds a reference rather than adopting one
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

extern "C" void* noesis_markup_extension_register(
    const char* name,
    noesis_markup_provide_fn cb,
    void* userdata,
    noesis_markup_free_fn free_handler) {
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
        // No instances yet, so a full teardown is safe. Leave `userdata`: on a
        // null return the Rust caller still owns and frees it.
        Noesis::Factory::UnregisterComponent(sym);
        Noesis::Reflection::Unregister(cd->typeClass);
        delete cd;
        return nullptr;
    }

    track_markup_data(cd);

    return cd;
}

extern "C" void noesis_markup_extension_unregister(void* token) {
    if (!token) return;
    auto* cd = static_cast<MarkupClassData*>(token);

    // Stops new instances; live ones keep their refs. No
    // Reflection::Unregister: live instances' destructor chains still walk the
    // type, and Noesis::Shutdown tears the registry down.
    Noesis::Factory::UnregisterComponent(cd->sym);
    markup_registry_erase(cd->sym);

    // frees the handler box now, or when the last live instance dies
    cd->Release();
}

// Runs after Noesis::Shutdown: frees handler boxes whose instances never
// released them. Like noesis_classes_force_free_at_shutdown.
extern "C" void noesis_markup_extensions_force_free_at_shutdown(void) {
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
