// Code-built bindings + Rust value converters (TODO §3).
//
// Two cooperating pieces that close the gap between "bindings authored in
// XAML" and "bindings + conversion logic driven from Rust":
//
//   * RustValueConverter : Noesis::BaseValueConverter — a trampoline whose
//     TryConvert / TryConvertBack forward into a Rust vtable. Binding values
//     cross the FFI as boxed `BaseComponent*` (the same boxing the rest of the
//     data-binding bridge uses); the Rust side unboxes the input with the
//     dm_noesis_unbox_* helpers below and boxes its result with dm_noesis_box_*.
//     Lifetime is modelled on RustCommand (noesis_commands.cpp): the converter
//     is an ordinary BaseComponent, so Noesis's intrusive refcount runs the
//     destructor — and the donated Rust free handler — exactly once after the
//     last reference drops (which may be a Binding holding the converter alive
//     well past the Rust handle being dropped).
//
//   * Binding construction + BindingOperations::SetBinding — `new Binding(path)`
//     plus setters for the common knobs (Source, ElementName, Mode, Converter,
//     ConverterParameter, StringFormat, FallbackValue, UpdateSourceTrigger,
//     RelativeSource Self) and a wiring entrypoint that resolves the target DP
//     by name and calls BindingOperations::SetBinding. This is the code path
//     that mirrors what XAML `{Binding ...}` authoring does.
//
// Plus value boxing/unboxing helpers (bool/int32/double) so Rust can move
// primitive values across as BaseComponent* — the currency every binding /
// converter speaks. `dm_noesis_box_string` already lives in
// noesis_collections.cpp; the string *unbox* helper is here next to its peers.

#include "noesis_shim.h"

#include <NsCore/BaseComponent.h>
#include <NsCore/Boxing.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsCore/Reflection.h>
#include <NsCore/ReflectionImplement.h>
#include <NsCore/String.h>
#include <NsCore/Symbol.h>
#include <NsGui/BaseValueConverter.h>
#include <NsGui/Binding.h>
#include <NsGui/BindingOperations.h>
#include <NsGui/Enums.h>  // BindingMode
#include <NsGui/DependencyObject.h>
#include <NsGui/DependencyProperty.h>
#include <NsGui/FrameworkElement.h>
#include <NsGui/IValueConverter.h>
#include <NsGui/RelativeSource.h>
#include <NsGui/ResourceDictionary.h>
#include <NsGui/UpdateSourceTrigger.h>

namespace {

// Hand a freshly-created (or borrowed) BaseComponent out across the C ABI with
// exactly one reference owned by the caller. Mirrors the helper in
// noesis_collections.cpp. Safe on a refcount-1 `new`'d object (bumps to 2,
// balanced when the local Ptr that produced it releases) or a borrowed object.
void* handout(Noesis::BaseComponent* c) {
    if (!c) return nullptr;
    c->AddReference();
    return c;
}

// ── RustValueConverter ──────────────────────────────────────────────────────

class RustValueConverter final: public Noesis::BaseValueConverter {
public:
    RustValueConverter(const dm_noesis_value_converter_vtable* vt, void* userdata,
                       dm_noesis_value_converter_free_fn free_handler)
        : mVtable(*vt), mUserdata(userdata), mFree(free_handler) {}

    ~RustValueConverter() {
        // Donated ownership: drop the Rust handler box exactly once, when the
        // final BaseComponent reference goes away. Null first so a (currently
        // impossible) re-entrant teardown can't double-free.
        void* ud = mUserdata;
        mUserdata = nullptr;
        if (mFree && ud) {
            mFree(ud);
        }
    }

    // From IValueConverter (via BaseValueConverter). `value` / `parameter` are
    // borrowed boxed BaseComponent* (may be null). The Rust callback writes a
    // +1-owned BaseComponent* into `out` (ownership transfers to us) and returns
    // true; returning false signals UnsetValue (use FallbackValue / default).
    bool TryConvert(Noesis::BaseComponent* value, const Noesis::Type* targetType,
                    Noesis::BaseComponent* parameter,
                    Noesis::Ptr<Noesis::BaseComponent>& result) override {
        return Forward(mVtable.convert, value, targetType, parameter, result);
    }

    bool TryConvertBack(Noesis::BaseComponent* value, const Noesis::Type* targetType,
                        Noesis::BaseComponent* parameter,
                        Noesis::Ptr<Noesis::BaseComponent>& result) override {
        return Forward(mVtable.convert_back, value, targetType, parameter, result);
    }

    NS_IMPLEMENT_INLINE_REFLECTION(RustValueConverter, Noesis::BaseValueConverter,
                                   "DmNoesis.RustValueConverter") {}

private:
    bool Forward(
        bool (*fn)(void*, void*, const void*, void*, void**),
        Noesis::BaseComponent* value, const Noesis::Type* targetType,
        Noesis::BaseComponent* parameter, Noesis::Ptr<Noesis::BaseComponent>& result) {
        if (!fn) return false;
        void* out = nullptr;
        bool ok = fn(mUserdata, value, static_cast<const void*>(targetType), parameter, &out);
        if (!ok) return false;
        if (out) {
            // Adopt the +1 reference transferred from Rust (the `Ptr<T>(T&)`
            // constructor takes ownership without an extra AddReference — the
            // same adopt idiom used for `*new T` elsewhere in this shim).
            result = Noesis::Ptr<Noesis::BaseComponent>(*static_cast<Noesis::BaseComponent*>(out));
        } else {
            result.Reset();
        }
        return true;
    }

    dm_noesis_value_converter_vtable  mVtable;
    void*                             mUserdata;
    dm_noesis_value_converter_free_fn mFree;
};

Noesis::Binding* as_binding(void* p) {
    if (!p) return nullptr;
    return Noesis::DynamicCast<Noesis::Binding*>(static_cast<Noesis::BaseComponent*>(p));
}

}  // namespace

// ── Boxing / unboxing primitives ────────────────────────────────────────────

extern "C" void* dm_noesis_box_bool(bool value) {
    Noesis::Ptr<Noesis::BoxedValue> boxed = Noesis::Boxing::Box<bool>(value);
    return handout(boxed.GetPtr());
}

extern "C" void* dm_noesis_box_int32(int32_t value) {
    Noesis::Ptr<Noesis::BoxedValue> boxed = Noesis::Boxing::Box<int32_t>(value);
    return handout(boxed.GetPtr());
}

extern "C" void* dm_noesis_box_double(double value) {
    Noesis::Ptr<Noesis::BoxedValue> boxed = Noesis::Boxing::Box<double>(value);
    return handout(boxed.GetPtr());
}

extern "C" bool dm_noesis_unbox_bool(void* boxed, bool* out) {
    if (!boxed || !out) return false;
    auto* b = static_cast<Noesis::BaseComponent*>(boxed);
    if (!Noesis::Boxing::CanUnbox<bool>(b)) return false;
    *out = Noesis::Boxing::Unbox<bool>(b);
    return true;
}

extern "C" bool dm_noesis_unbox_int32(void* boxed, int32_t* out) {
    if (!boxed || !out) return false;
    auto* b = static_cast<Noesis::BaseComponent*>(boxed);
    if (!Noesis::Boxing::CanUnbox<int32_t>(b)) return false;
    *out = Noesis::Boxing::Unbox<int32_t>(b);
    return true;
}

extern "C" bool dm_noesis_unbox_double(void* boxed, double* out) {
    if (!boxed || !out) return false;
    auto* b = static_cast<Noesis::BaseComponent*>(boxed);
    if (!Noesis::Boxing::CanUnbox<double>(b)) return false;
    *out = Noesis::Boxing::Unbox<double>(b);
    return true;
}

// Borrowed (no +1) view of a boxed string's bytes, valid while `boxed` is alive.
// NULL if `boxed` is not a BoxedValue<String>.
extern "C" const char* dm_noesis_unbox_string(void* boxed) {
    if (!boxed) return nullptr;
    auto* b = static_cast<Noesis::BaseComponent*>(boxed);
    if (!Noesis::Boxing::CanUnbox<Noesis::String>(b)) return nullptr;
    const Noesis::String& s = Noesis::Boxing::Unbox<Noesis::String>(b);
    return s.Str();
}

// ── Value converter ─────────────────────────────────────────────────────────

extern "C" void* dm_noesis_value_converter_create(
    const dm_noesis_value_converter_vtable* vt,
    void* userdata,
    dm_noesis_value_converter_free_fn free_handler) {
    if (!vt) return nullptr;
    // BaseComponent starts at refcount 1 — that initial reference IS the
    // caller's +1, balanced by dm_noesis_value_converter_destroy. A Binding
    // that later stores the converter (SetConverter) takes its own ref, so the
    // handler box outlives our destroy until that ref also drops.
    auto* conv = new RustValueConverter(vt, userdata, free_handler);
    return static_cast<Noesis::BaseComponent*>(conv);
}

extern "C" void dm_noesis_value_converter_destroy(void* converter) {
    if (!converter) return;
    static_cast<Noesis::BaseComponent*>(converter)->Release();
}

// ── Binding construction ────────────────────────────────────────────────────

extern "C" void* dm_noesis_binding_create(const char* path) {
    // new Binding starts at refcount 1 (the caller's +1), balanced by
    // dm_noesis_binding_destroy. SetBinding takes its own reference.
    auto* b = path ? new Noesis::Binding(path) : new Noesis::Binding();
    return static_cast<Noesis::BaseComponent*>(b);
}

extern "C" void dm_noesis_binding_destroy(void* binding) {
    if (!binding) return;
    static_cast<Noesis::BaseComponent*>(binding)->Release();
}

extern "C" void dm_noesis_binding_set_source(void* binding, void* source) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetSource(static_cast<Noesis::BaseComponent*>(source));
}

extern "C" void dm_noesis_binding_set_element_name(void* binding, const char* name) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetElementName(name ? name : "");
}

extern "C" void dm_noesis_binding_set_mode(void* binding, int32_t mode) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetMode(static_cast<Noesis::BindingMode>(mode));
}

extern "C" void dm_noesis_binding_set_converter(void* binding, void* converter) {
    Noesis::Binding* b = as_binding(binding);
    if (!b) return;
    auto* conv = converter
        ? Noesis::DynamicCast<Noesis::IValueConverter*>(
              static_cast<Noesis::BaseComponent*>(converter))
        : nullptr;
    b->SetConverter(conv);
}

extern "C" void dm_noesis_binding_set_converter_parameter(void* binding, void* parameter) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetConverterParameter(static_cast<Noesis::BaseComponent*>(parameter));
}

extern "C" void dm_noesis_binding_set_string_format(void* binding, const char* format) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetStringFormat(format ? format : "");
}

extern "C" void dm_noesis_binding_set_fallback_value(void* binding, void* value) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetFallbackValue(static_cast<Noesis::BaseComponent*>(value));
}

extern "C" void dm_noesis_binding_set_update_source_trigger(void* binding, int32_t trigger) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetUpdateSourceTrigger(static_cast<Noesis::UpdateSourceTrigger>(trigger));
}

extern "C" void dm_noesis_binding_set_relative_source_self(void* binding) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetRelativeSource(Noesis::RelativeSource::GetSelf());
}

extern "C" bool dm_noesis_set_binding(void* element, const char* dp_name, void* binding) {
    if (!element || !dp_name || !binding) return false;
    auto* d = Noesis::DynamicCast<Noesis::DependencyObject*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!d) return false;
    Noesis::Binding* b = as_binding(binding);
    if (!b) return false;

    const Noesis::DependencyProperty* dp =
        Noesis::FindDependencyProperty(d->GetClassType(), Noesis::Symbol(dp_name));
    if (!dp) return false;

    Noesis::BindingOperations::SetBinding(d, dp, b);
    return true;
}

// ── ResourceDictionary insertion (so XAML {StaticResource} can reach a Rust
//    converter / value) ─────────────────────────────────────────────────────

extern "C" bool dm_noesis_framework_element_add_resource(
    void* element, const char* key, void* object) {
    if (!element || !key || !object) return false;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return false;

    Noesis::ResourceDictionary* res = fe->GetResources();
    if (!res) {
        Noesis::Ptr<Noesis::ResourceDictionary> created = *new Noesis::ResourceDictionary();
        fe->SetResources(created.GetPtr());
        res = created.GetPtr();
    }
    // Add takes a borrowed value and stores its own reference.
    res->Add(key, static_cast<Noesis::BaseComponent*>(object));
    return true;
}
