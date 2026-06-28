// Reflection meta registration: custom enums, routed events, factory metadata,
// and string->value conversion.
//
// Everything here registers / queries a runtime entity against Noesis's
// reflection database so the XAML parser / bindings resolve it the same way they
// resolve a compile-time NS_REGISTER_* declaration. Two SDK facts make this work
// from a C ABI:
//
//   * Reflection::RegisterType(Type*) takes ownership of a hand-built Type and
//     keeps it alive until shutdown — the same path noesis_classes.cpp uses for
//     its synthetic TypeClassBuilder. We reuse it for a runtime TypeEnum.
//   * TypeMeta::AddMeta / FindMeta let us attach (and recover) per-type metadata
//     — UIElementData (routed events) and ContentPropertyMetaData — on an
//     already-registered type, keyed only by its reflected name. So none of this
//     needs the opaque ClassData token from noesis_classes.cpp.
//
// NOTE: custom *reflection TypeConverter* registration is NOT exposed in
// 3.2.13. TypeConverter::Get resolves converters through an internal registry
// that TypeConverterMetaData + Factory::RegisterComponent do not drive at
// runtime (verified empirically: a synthetic converter type registers in the
// Factory yet Get still returns null). The consumption side
// (noesis_type_converter_from_string) works for any built-in / reflected
// type. See LIMITATIONS.md "Known SDK limitations".

#include "noesis_shim.h"

#include <NsCore/BaseComponent.h>
#include <NsCore/Boxing.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Factory.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsCore/Reflection.h>
#include <NsCore/Symbol.h>
#include <NsCore/TypeClass.h>
#include <NsCore/TypeConverter.h>
#include <NsCore/TypeEnum.h>
#include <NsCore/TypeMeta.h>
#include <NsGui/ContentPropertyMetaData.h>
#include <NsGui/DependsOnMetaData.h>
#include <NsGui/RoutedEvent.h>
#include <NsGui/UIElement.h>
#include <NsGui/UIElementData.h>

namespace {

// Hand a freshly-created / borrowed BaseComponent across the C ABI with exactly
// one reference owned by the caller (mirrors the helper in noesis_binding.cpp).
void* handout(Noesis::BaseComponent* c) {
    if (!c) return nullptr;
    c->AddReference();
    return c;
}

// Resolve a reflected type by name without interning a junk Symbol for a
// never-seen name.
const Noesis::Type* find_type(const char* name) {
    if (!name) return nullptr;
    Noesis::Symbol sym(name, Noesis::Symbol::NullIfNotFound());
    if (sym.IsNull()) return nullptr;
    return Noesis::Reflection::GetType(sym);
}

// ── (A) Custom enums ────────────────────────────────────────────────────────
//
// We cannot use Noesis::TypeEnumImpl<T> for a runtime enum: instantiating it
// forces TypeEnumImpl<T>::GetValueObject -> Boxing::Box<T> -> TypeOf<T>(), which
// requires a compile-time NS_DECLARE_REFLECTION_ENUM(T). Instead we subclass
// TypeEnum directly (its only pure virtual is GetValueObject) and box members as
// a plain int32 — enough for the reflection / EnumConverter lookup paths the
// XAML parser drives, with no compile-time type baggage. The (name, value)
// members and reflected name come entirely from runtime arguments, so one C++
// type services arbitrarily many distinct named enums.
class DmRuntimeTypeEnum final: public Noesis::TypeEnum {
public:
    explicit DmRuntimeTypeEnum(Noesis::Symbol name): TypeEnum(name) {}

    bool GetValueObject(Noesis::Symbol name, Noesis::Ptr<Noesis::BoxedValue>& value) const override {
        uint64_t v = 0;
        if (HasName(name, v)) {
            value = Noesis::Boxing::Box<int32_t>(static_cast<int32_t>(static_cast<uint32_t>(v)));
            return true;
        }
        return false;
    }
};

}  // namespace

extern "C" void* noesis_register_enum(
    const char* name, const noesis_enum_value* values, uint32_t count) {
    if (!name || name[0] == '\0') return nullptr;

    Noesis::Symbol sym(name);
    if (Noesis::Reflection::IsTypeRegistered(sym)) {
        return nullptr;
    }

    auto* te = new DmRuntimeTypeEnum(sym);
    for (uint32_t i = 0; i < count; ++i) {
        if (!values[i].name) continue;
        te->AddValue(Noesis::Symbol(values[i].name),
                     static_cast<uint64_t>(static_cast<uint32_t>(values[i].value)));
    }

    // RegisterType assumes ownership; the TypeEnum lives until Reflection::Shutdown.
    Noesis::Reflection::RegisterType(te);
    return static_cast<Noesis::Type*>(te);
}

extern "C" bool noesis_enum_value_from_name(
    const char* enum_type, const char* value_name, int32_t* out_value) {
    if (!value_name || !out_value) return false;
    const auto* te = Noesis::DynamicCast<const Noesis::TypeEnum*>(find_type(enum_type));
    if (!te) return false;
    Noesis::Symbol member(value_name, Noesis::Symbol::NullIfNotFound());
    if (member.IsNull()) return false;
    uint64_t v = 0;
    if (!te->HasName(member, v)) return false;
    *out_value = static_cast<int32_t>(static_cast<uint32_t>(v));
    return true;
}

extern "C" bool noesis_enum_name_from_value(
    const char* enum_type, int32_t value, const char** out_name) {
    if (!out_name) return false;
    const auto* te = Noesis::DynamicCast<const Noesis::TypeEnum*>(find_type(enum_type));
    if (!te) return false;
    Noesis::Symbol name;
    if (!te->HasValue(static_cast<uint64_t>(static_cast<uint32_t>(value)), name)) return false;
    *out_name = name.Str();
    return true;
}

extern "C" bool noesis_type_converter_from_string(
    const char* type_name, const char* str, void** out_boxed) {
    if (!str || !out_boxed) return false;
    const Noesis::Type* t = find_type(type_name);
    if (!t) return false;
    Noesis::TypeConverter* conv = Noesis::TypeConverter::Get(t);
    if (!conv) return false;
    Noesis::Ptr<Noesis::BaseComponent> result;
    if (!conv->TryConvertFromString(str, result)) return false;
    *out_boxed = handout(result.GetPtr());
    return true;
}

// ── (B) Custom routed events ──────────────────────────────────────────────────

extern "C" bool noesis_register_routed_event(
    const char* type_name, const char* event_name, int32_t strategy) {
    if (!event_name) return false;
    const auto* tc = Noesis::DynamicCast<const Noesis::TypeClass*>(find_type(type_name));
    if (!tc) return false;

    auto* uiData = Noesis::FindMeta<Noesis::UIElementData>(tc);
    if (!uiData) return false;

    Noesis::Symbol evSym(event_name);
    if (uiData->FindEvent(evSym) != nullptr) {
        return false;  // already registered on this type
    }

    Noesis::RoutingStrategy rs;
    switch (strategy) {
        case 0: rs = Noesis::RoutingStrategy_Tunnel; break;
        case 2: rs = Noesis::RoutingStrategy_Direct; break;
        case 1:
        default: rs = Noesis::RoutingStrategy_Bubble; break;
    }

    // RegisterEvent writes the created RoutedEvent through the reference once;
    // it does not retain &slot, so a local is fine. The event itself is owned
    // by the UIElementData's event map.
    const Noesis::RoutedEvent* slot = nullptr;
    uiData->RegisterEvent(slot, event_name, rs);
    return slot != nullptr;
}

extern "C" bool noesis_raise_routed_event(void* element, const char* event_name) {
    if (!element || !event_name) return false;
    auto* uie = Noesis::DynamicCast<Noesis::UIElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!uie) return false;

    Noesis::Symbol evSym(event_name, Noesis::Symbol::NullIfNotFound());
    if (evSym.IsNull()) return false;
    const Noesis::RoutedEvent* ev = Noesis::FindRoutedEvent(uie->GetClassType(), evSym);
    if (!ev) return false;

    Noesis::RoutedEventArgs args(uie, ev);
    uie->RaiseEvent(args);
    return true;
}

// ── (C) Factory / component metadata ───────────────────────────────────────

extern "C" bool noesis_factory_is_registered(const char* name) {
    if (!name) return false;
    Noesis::Symbol sym(name, Noesis::Symbol::NullIfNotFound());
    if (sym.IsNull()) return false;
    return Noesis::Factory::IsComponentRegistered(sym);
}

extern "C" bool noesis_type_set_content_property(
    const char* type_name, const char* prop_name) {
    if (!prop_name) return false;
    const auto* tc = Noesis::DynamicCast<const Noesis::TypeClass*>(find_type(type_name));
    if (!tc) return false;

    // AddMeta is non-const; the synthetic types we attach to are mutable through
    // the registry, and TypeMeta::AddMeta only appends. The const_cast is sound
    // because we only ever pass our own registered types here.
    auto* meta = const_cast<Noesis::TypeClass*>(tc);
    Noesis::Ptr<Noesis::ContentPropertyMetaData> cp =
        Noesis::MakePtr<Noesis::ContentPropertyMetaData>(prop_name);
    meta->AddMeta(cp.GetPtr());
    return true;
}

extern "C" bool noesis_type_get_content_property(
    const char* type_name, const char** out_name) {
    if (!out_name) return false;
    const auto* tc = Noesis::DynamicCast<const Noesis::TypeClass*>(find_type(type_name));
    if (!tc) return false;

    // FindMeta is keyed by the metadata TypeClass, so this reads the
    // ContentPropertyMetaData record independently of any DependsOnMetaData
    // attached to the same type.
    const auto* cp = Noesis::FindMeta<Noesis::ContentPropertyMetaData>(tc);
    if (!cp) return false;
    *out_name = cp->GetContentProperty().Str();
    return true;
}

extern "C" bool noesis_type_add_depends_on(
    const char* type_name, const char* prop_name) {
    if (!prop_name) return false;
    const auto* tc = Noesis::DynamicCast<const Noesis::TypeClass*>(find_type(type_name));
    if (!tc) return false;

    // DependsOnMetaData is type-level metadata in Noesis (NsGui/DependsOnMetaData.h),
    // attached the same way as ContentPropertyMetaData. The const_cast is sound:
    // we only attach to our own runtime-registered types and AddMeta only appends.
    auto* meta = const_cast<Noesis::TypeClass*>(tc);
    Noesis::Ptr<Noesis::DependsOnMetaData> dep =
        Noesis::MakePtr<Noesis::DependsOnMetaData>(prop_name);
    meta->AddMeta(dep.GetPtr());
    return true;
}

extern "C" bool noesis_type_get_depends_on(
    const char* type_name, const char** out_name) {
    if (!out_name) return false;
    const auto* tc = Noesis::DynamicCast<const Noesis::TypeClass*>(find_type(type_name));
    if (!tc) return false;

    const auto* dep = Noesis::FindMeta<Noesis::DependsOnMetaData>(tc);
    if (!dep) return false;
    *out_name = dep->GetDependsOnProperty().Str();
    return true;
}
