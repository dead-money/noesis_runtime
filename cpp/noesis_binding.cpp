// Code-built bindings, Rust value converters, and primitive boxing.
//
// RustValueConverter forwards TryConvert / TryConvertBack into a Rust vtable.
// Values cross as boxed BaseComponent*; Rust unboxes with noesis_unbox_* and
// boxes results with noesis_box_* (noesis_box_string is in
// noesis_collections.cpp). The converter frees its Rust box when the last
// reference drops, which may be a Binding that outlives the Rust handle.

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

// Returns `c` with a +1 owned by the caller. The producing local Ptr releases
// its own reference.
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
        // Null first so a re-entrant teardown can't double-free.
        void* ud = mUserdata;
        mUserdata = nullptr;
        if (mFree && ud) {
            mFree(ud);
        }
    }

    // `value` / `parameter` are borrowed and may be null. The Rust callback
    // writes a +1 BaseComponent* into `out` and returns true; false means
    // UnsetValue (FallbackValue or default applies).
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
            // Ptr<T>(T&) adopts Rust's +1 without AddReference.
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

extern "C" void* noesis_box_u64(uint64_t value) {
    Noesis::Ptr<Noesis::BoxedValue> boxed = Noesis::Boxing::Box<uint64_t>(value);
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

extern "C" bool noesis_unbox_u64(void* boxed, uint64_t* out) {
    if (!boxed || !out) return false;
    auto* b = static_cast<Noesis::BaseComponent*>(boxed);
    if (!Noesis::Boxing::CanUnbox<uint64_t>(b)) return false;
    *out = Noesis::Boxing::Unbox<uint64_t>(b);
    return true;
}

// Borrowed, valid while `boxed` is alive. Null if `boxed` is not a boxed String.
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
    // Initial refcount of 1 is the caller's +1, balanced by
    // noesis_value_converter_destroy.
    auto* conv = new RustValueConverter(vt, userdata, free_handler);
    return static_cast<Noesis::BaseComponent*>(conv);
}

extern "C" void noesis_value_converter_destroy(void* converter) {
    if (!converter) return;
    static_cast<Noesis::BaseComponent*>(converter)->Release();
}

// ── Binding construction ────────────────────────────────────────────────────

extern "C" void* noesis_binding_create(const char* path) {
    // Initial refcount of 1 is the caller's +1, balanced by
    // noesis_binding_destroy.
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

// `type_name` must already be registered with reflection; an unknown name
// returns false. `level` is 1-based; 0 is treated as 1 (nearest match).
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
    Noesis::Ptr<Noesis::RelativeSource> rs = *new Noesis::RelativeSource(
        Noesis::RelativeSourceMode_FindAncestor, type, lvl);
    b->SetRelativeSource(rs.GetPtr());
    return true;
}

extern "C" void noesis_binding_set_relative_source_previous_data(void* binding) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetRelativeSource(Noesis::RelativeSource::GetPreviousData());
}

extern "C" void noesis_binding_set_relative_source_templated_parent(void* binding) {
    Noesis::Binding* b = as_binding(binding);
    if (b) b->SetRelativeSource(Noesis::RelativeSource::GetTemplatedParent());
}

// ── BindingExpression inspection ─────────────────────────────────────────────

// Borrowed; owned by the target and valid while the binding stays on that
// property. Returned as BaseBindingExpression* (the update entry points below
// cast back to that). Null if `element` is not a DependencyObject, the DP is
// unknown, or no binding is set.
extern "C" void* noesis_get_binding_expression(void* element, const char* dp_name) {
    if (!element || !dp_name) return nullptr;
    auto* d = Noesis::DynamicCast<Noesis::DependencyObject*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!d) return nullptr;

    const Noesis::DependencyProperty* dp =
        Noesis::FindDependencyProperty(d->GetClassType(), Noesis::Symbol(dp_name));
    if (!dp) return nullptr;

    Noesis::BaseBindingExpression* be = Noesis::BindingOperations::GetBindingExpression(d, dp);
    return be;
}

extern "C" void noesis_binding_expression_update_target(void* expr) {
    if (!expr) return;
    static_cast<Noesis::BaseBindingExpression*>(expr)->UpdateTarget();
}

// No-op unless Mode is TwoWay / OneWayToSource. Commits an Explicit
// UpdateSourceTrigger.
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

// True also when no binding was set; false only when the element or DP can't
// be resolved.
extern "C" bool noesis_clear_binding(void* element, const char* dp_name) {
    if (!element || !dp_name) return false;
    auto* d = Noesis::DynamicCast<Noesis::DependencyObject*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!d) return false;

    const Noesis::DependencyProperty* dp =
        Noesis::FindDependencyProperty(d->GetClassType(), Noesis::Symbol(dp_name));
    if (!dp) return false;

    Noesis::BindingOperations::ClearBinding(d, dp);
    return true;
}

// ── ResourceDictionary insertion ────────────────────────────────────────────
//
// Lets XAML {StaticResource} reach a Rust-created converter or value. Creates
// the element's dictionary if it has none.

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
    res->Add(key, static_cast<Noesis::BaseComponent*>(object));
    return true;
}
