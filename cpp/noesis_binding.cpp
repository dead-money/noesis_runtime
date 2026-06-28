// Code-built bindings + Rust value converters (TODO §3).
//
// Two cooperating pieces that close the gap between "bindings authored in
// XAML" and "bindings + conversion logic driven from Rust":
//
//   * RustValueConverter : Noesis::BaseValueConverter — a trampoline whose
//     TryConvert / TryConvertBack forward into a Rust vtable. Binding values
//     cross the FFI as boxed `BaseComponent*` (the same boxing the rest of the
//     data-binding bridge uses); the Rust side unboxes the input with the
//     noesis_unbox_* helpers below and boxes its result with noesis_box_*.
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
// converter speaks. `noesis_box_string` already lives in
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
#include <NsGui/BaseBindingExpression.h>
#include <NsGui/BaseValueConverter.h>
#include <NsGui/Binding.h>
#include <NsGui/BindingExpression.h>
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
    RustValueConverter(const noesis_value_converter_vtable* vt, void* userdata,
                       noesis_value_converter_free_fn free_handler)
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

    noesis_value_converter_vtable  mVtable;
    void*                             mUserdata;
    noesis_value_converter_free_fn mFree;
};

Noesis::Binding* as_binding(void* p) {
    if (!p) return nullptr;
    return Noesis::DynamicCast<Noesis::Binding*>(static_cast<Noesis::BaseComponent*>(p));
}

}  // namespace

// ── Boxing / unboxing primitives ────────────────────────────────────────────

extern "C" void* noesis_box_bool(bool value) {
    Noesis::Ptr<Noesis::BoxedValue> boxed = Noesis::Boxing::Box<bool>(value);
    return handout(boxed.GetPtr());
}

extern "C" void* noesis_box_int32(int32_t value) {
    Noesis::Ptr<Noesis::BoxedValue> boxed = Noesis::Boxing::Box<int32_t>(value);
    return handout(boxed.GetPtr());
}

extern "C" void* noesis_box_double(double value) {
    Noesis::Ptr<Noesis::BoxedValue> boxed = Noesis::Boxing::Box<double>(value);
    return handout(boxed.GetPtr());
}

extern "C" bool noesis_unbox_bool(void* boxed, bool* out) {
    if (!boxed || !out) return false;
    auto* b = static_cast<Noesis::BaseComponent*>(boxed);
    if (!Noesis::Boxing::CanUnbox<bool>(b)) return false;
    *out = Noesis::Boxing::Unbox<bool>(b);
    return true;
}

extern "C" bool noesis_unbox_int32(void* boxed, int32_t* out) {
    if (!boxed || !out) return false;
    auto* b = static_cast<Noesis::BaseComponent*>(boxed);
    if (!Noesis::Boxing::CanUnbox<int32_t>(b)) return false;
    *out = Noesis::Boxing::Unbox<int32_t>(b);
    return true;
}

extern "C" bool noesis_unbox_double(void* boxed, double* out) {
    if (!boxed || !out) return false;
    auto* b = static_cast<Noesis::BaseComponent*>(boxed);
    if (!Noesis::Boxing::CanUnbox<double>(b)) return false;
    *out = Noesis::Boxing::Unbox<double>(b);
    return true;
}

// Borrowed (no +1) view of a boxed string's bytes, valid while `boxed` is alive.
// NULL if `boxed` is not a BoxedValue<String>.
extern "C" const char* noesis_unbox_string(void* boxed) {
    if (!boxed) return nullptr;
    auto* b = static_cast<Noesis::BaseComponent*>(boxed);
    if (!Noesis::Boxing::CanUnbox<Noesis::String>(b)) return nullptr;
    const Noesis::String& s = Noesis::Boxing::Unbox<Noesis::String>(b);
    return s.Str();
}

// ── Value converter ─────────────────────────────────────────────────────────

extern "C" void* noesis_value_converter_create(
    const noesis_value_converter_vtable* vt,
    void* userdata,
    noesis_value_converter_free_fn free_handler) {
    if (!vt) return nullptr;
    // BaseComponent starts at refcount 1 — that initial reference IS the
    // caller's +1, balanced by noesis_value_converter_destroy. A Binding
    // that later stores the converter (SetConverter) takes its own ref, so the
    // handler box outlives our destroy until that ref also drops.
    auto* conv = new RustValueConverter(vt, userdata, free_handler);
    return static_cast<Noesis::BaseComponent*>(conv);
}

extern "C" void noesis_value_converter_destroy(void* converter) {
    if (!converter) return;
    static_cast<Noesis::BaseComponent*>(converter)->Release();
}

// ── Binding construction ────────────────────────────────────────────────────

extern "C" void* noesis_binding_create(const char* path) {
    // new Binding starts at refcount 1 (the caller's +1), balanced by
    // noesis_binding_destroy. SetBinding takes its own reference.
    auto* b = path ? new Noesis::Binding(path) : new Noesis::Binding();
    return static_cast<Noesis::BaseComponent*>(b);
}

extern "C" void noesis_binding_destroy(void* binding) {
    if (!binding) return;
    static_cast<Noesis::BaseComponent*>(binding)->Release();
}

extern "C" void noesis_binding_set_source(void* binding, void* source) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetSource(static_cast<Noesis::BaseComponent*>(source));
}

extern "C" void noesis_binding_set_element_name(void* binding, const char* name) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetElementName(name ? name : "");
}

extern "C" void noesis_binding_set_mode(void* binding, int32_t mode) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetMode(static_cast<Noesis::BindingMode>(mode));
}

extern "C" void noesis_binding_set_converter(void* binding, void* converter) {
    Noesis::Binding* b = as_binding(binding);
    if (!b) return;
    auto* conv = converter
        ? Noesis::DynamicCast<Noesis::IValueConverter*>(
              static_cast<Noesis::BaseComponent*>(converter))
        : nullptr;
    b->SetConverter(conv);
}

extern "C" void noesis_binding_set_converter_parameter(void* binding, void* parameter) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetConverterParameter(static_cast<Noesis::BaseComponent*>(parameter));
}

extern "C" void noesis_binding_set_string_format(void* binding, const char* format) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetStringFormat(format ? format : "");
}

extern "C" void noesis_binding_set_fallback_value(void* binding, void* value) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetFallbackValue(static_cast<Noesis::BaseComponent*>(value));
}

extern "C" void noesis_binding_set_update_source_trigger(void* binding, int32_t trigger) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetUpdateSourceTrigger(static_cast<Noesis::UpdateSourceTrigger>(trigger));
}

extern "C" void noesis_binding_set_relative_source_self(void* binding) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetRelativeSource(Noesis::RelativeSource::GetSelf());
}

// FindAncestor: resolve `type_name` through the reflection registry, then build
// a `RelativeSource(FindAncestor, type, level)`. The ancestor type must already
// be registered with Reflection (referencing it from XAML forces registration);
// an unknown name fails gracefully (no-op, returns false) rather than crashing.
// `level` is the 1-based ancestor index (0 is coerced to 1, the nearest match).
extern "C" bool noesis_binding_set_relative_source_find_ancestor(
    void* binding, const char* type_name, uint32_t level) {
    Noesis::Binding* b = as_binding(binding);
    if (!b || !type_name) return false;

    // NullIfNotFound avoids interning a junk symbol for a never-seen name.
    Noesis::Symbol sym(type_name, Noesis::Symbol::NullIfNotFound());
    if (sym.IsNull()) return false;
    const Noesis::Type* type = Noesis::Reflection::GetType(sym);
    if (!type) return false;

    const int lvl = level == 0 ? 1 : static_cast<int>(level);
    // new RelativeSource starts at refcount 1; the local Ptr adopts that and
    // releases on scope exit — SetRelativeSource takes its own reference.
    Noesis::Ptr<Noesis::RelativeSource> rs = *new Noesis::RelativeSource(
        Noesis::RelativeSourceMode_FindAncestor, type, lvl);
    b->SetRelativeSource(rs.GetPtr());
    return true;
}

// PreviousData: bind to the previous item in a data-bound collection (the
// idiom behind delta columns). Uses the shared static RelativeSource singleton.
extern "C" void noesis_binding_set_relative_source_previous_data(void* binding) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetRelativeSource(Noesis::RelativeSource::GetPreviousData());
}

// TemplatedParent: bind to the control a ControlTemplate is applied to (the
// code-built equivalent of `{Binding RelativeSource={RelativeSource
// TemplatedParent}}`). Shared static singleton.
extern "C" void noesis_binding_set_relative_source_templated_parent(void* binding) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetRelativeSource(Noesis::RelativeSource::GetTemplatedParent());
}

// ── BindingExpression inspection (TODO §3) ───────────────────────────────────

// Borrowed BindingExpression* for the binding on `element`'s `dp_name` property,
// via BindingOperations::GetBindingExpression. The expression is OWNED by the
// target object — the caller must NOT release it, and it is valid only while the
// binding stays live on that property. Returns NULL if `element` is not a
// DependencyObject, the DP name is unknown, or no binding is set on it. The
// pointer is returned as the BaseBindingExpression base (upcast) so the
// update entrypoints below can call the virtuals uniformly.
extern "C" void* noesis_get_binding_expression(void* element, const char* dp_name) {
    if (!element || !dp_name) return nullptr;
    auto* d = Noesis::DynamicCast<Noesis::DependencyObject*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!d) return nullptr;

    const Noesis::DependencyProperty* dp =
        Noesis::FindDependencyProperty(d->GetClassType(), Noesis::Symbol(dp_name));
    if (!dp) return nullptr;

    // Implicit upcast BindingExpression* -> BaseBindingExpression* (adjusts the
    // pointer for the compiler); borrowed, no AddReference.
    Noesis::BaseBindingExpression* be = Noesis::BindingOperations::GetBindingExpression(d, dp);
    return be;
}

// Force a source -> target data transfer (re-pull the source value).
extern "C" void noesis_binding_expression_update_target(void* expr) {
    if (!expr) return;
    static_cast<Noesis::BaseBindingExpression*>(expr)->UpdateTarget();
}

// Push the current target value back to the source. No-op (per Noesis) unless
// the binding's Mode is TwoWay / OneWayToSource — this is what commits a binding
// whose UpdateSourceTrigger is Explicit.
extern "C" void noesis_binding_expression_update_source(void* expr) {
    if (!expr) return;
    static_cast<Noesis::BaseBindingExpression*>(expr)->UpdateSource();
}

extern "C" bool noesis_set_binding(void* element, const char* dp_name, void* binding) {
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

extern "C" bool noesis_framework_element_add_resource(
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
