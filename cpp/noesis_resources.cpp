// ResourceDictionary access, Style from code, and template assignment
// (TODO §7 / Phase B "resources-styles-templates").
//
// Three cooperating surfaces, all building on the object-creation /
// ownership idioms already established in noesis_binding.cpp and
// noesis_view.cpp:
//
//   * ResourceDictionary — create/own a dictionary from Rust, add a
//     key->component, look up by key (borrowed component out), wire merged
//     dictionaries, parse a <ResourceDictionary> from an in-memory XAML
//     string, and install one as the process-global application resources
//     (GUI::SetApplicationResources / GetApplicationResources). Per-element
//     Resources get/set and a non-throwing FindResource (logical-chain
//     lookup) live here too.
//
//   * Style — `new Style`, set the target type by name (resolved through
//     Noesis::Reflection like the RelativeSource FindAncestor path in
//     noesis_binding.cpp), append Setters (DP resolved on the target type by
//     name; value is a boxed BaseComponent*), set BasedOn, then assign via
//     FrameworkElement::SetStyle / read back GetStyle.
//
//   * Templates — parse a <ControlTemplate>/<DataTemplate> from a string
//     (GUI::ParseXaml + cast), assign a ControlTemplate via Control::SetTemplate
//     (DataTemplate is DP-settable via the existing set_component path), read
//     GetTemplate back, and a FrameworkTemplate::FindName accessor.
//
// OWNERSHIP: create/parse entrypoints return a freshly-created object at +1
// owned by the caller (released via the matching *_destroy or the generic
// noesis_base_component_release). Borrowed getters (GetResources,
// GetApplicationResources, GetStyle handed out below via AddRef so Rust can own
// it, Find/Get on a dictionary) follow the contract documented per-function.

#include "noesis_shim.h"

#include <NsCore/BaseComponent.h>
#include <NsCore/Boxing.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsCore/Reflection.h>
#include <NsCore/ReflectionImplement.h>
#include <NsCore/Symbol.h>
#include <NsGui/BaseBinding.h>
#include <NsGui/BaseTrigger.h>
#include <NsGui/Condition.h>
#include <NsGui/Control.h>
#include <NsGui/ControlTemplate.h>
#include <NsGui/DataTemplate.h>
#include <NsGui/DataTemplateSelector.h>
#include <NsGui/DataTrigger.h>
#include <NsGui/DependencyProperty.h>
#include <NsGui/EventTrigger.h>
#include <NsGui/FrameworkElement.h>
#include <NsGui/FrameworkTemplate.h>
#include <NsGui/IntegrationAPI.h>
#include <NsGui/IUITreeNode.h>
#include <NsGui/MultiDataTrigger.h>
#include <NsGui/MultiTrigger.h>
#include <NsGui/ResourceDictionary.h>
#include <NsGui/RoutedEvent.h>
#include <NsGui/Setter.h>
#include <NsGui/Style.h>
#include <NsGui/Trigger.h>
#include <NsGui/TriggerAction.h>
#include <NsGui/UICollection.h>
#include <NsGui/Uri.h>

namespace {

// Hand a freshly-created (or AddRef'd) BaseComponent out across the C ABI with
// exactly one reference owned by the caller. Mirrors handout() in
// noesis_binding.cpp / noesis_collections.cpp.
void* handout(Noesis::BaseComponent* c) {
    if (!c) return nullptr;
    c->AddReference();
    return c;
}

Noesis::ResourceDictionary* as_dict(void* p) {
    if (!p) return nullptr;
    return Noesis::DynamicCast<Noesis::ResourceDictionary*>(
        static_cast<Noesis::BaseComponent*>(p));
}

Noesis::Style* as_style(void* p) {
    if (!p) return nullptr;
    return Noesis::DynamicCast<Noesis::Style*>(static_cast<Noesis::BaseComponent*>(p));
}

Noesis::FrameworkElement* as_element(void* p) {
    if (!p) return nullptr;
    return Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(p));
}

// Resolve a DependencyProperty by name on a reflection-registered type. Mirrors
// the resolution noesis_style_add_setter does on the Style's TargetType, but
// generalised so trigger/condition/setter construction can target an explicit
// type (a property trigger's Property lives on the templated/styled type, which
// is not always the Style's own TargetType). Returns null on an unknown type or
// an unknown DP name on that type.
const Noesis::DependencyProperty* resolve_dp(const char* type_name, const char* dp_name) {
    if (!type_name || !dp_name) return nullptr;
    Noesis::Symbol tsym(type_name, Noesis::Symbol::NullIfNotFound());
    if (tsym.IsNull()) return nullptr;
    const Noesis::Type* type = Noesis::Reflection::GetType(tsym);
    const auto* tc = Noesis::DynamicCast<const Noesis::TypeClass*>(type);
    if (!tc) return nullptr;
    return Noesis::FindDependencyProperty(tc, Noesis::Symbol(dp_name));
}

Noesis::BaseTrigger* as_trigger(void* p) {
    if (!p) return nullptr;
    return Noesis::DynamicCast<Noesis::BaseTrigger*>(static_cast<Noesis::BaseComponent*>(p));
}

// Append a `new Setter{ Property=resolve_dp(type,dp), Value=value }` to a setter
// collection. Shared by the property/data/multi trigger entrypoints. Returns
// false on a null collection, an unresolvable DP, or a null value.
bool add_setter_to(Noesis::BaseSetterCollection* setters, const char* type_name,
    const char* dp_name, void* value) {
    if (!setters || !value) return false;
    const Noesis::DependencyProperty* dp = resolve_dp(type_name, dp_name);
    if (!dp) return false;
    Noesis::Ptr<Noesis::Setter> setter = *new Noesis::Setter();
    setter->SetProperty(dp);
    setter->SetValue(static_cast<Noesis::BaseComponent*>(value));
    setters->Add(setter.GetPtr());
    return true;
}

// ── RustDataTemplateSelector ─────────────────────────────────────────────────
//
// A DataTemplateSelector subclass whose SelectTemplate() virtual trampolines
// into a Rust callback — the runtime-constructible "selector from Rust" path.
// Mirrors the RustValueConverter trampoline in noesis_binding.cpp (donated
// userdata box, freed once when the final reference drops). The callback returns
// a BORROWED DataTemplate* (the selector keeps its candidate templates alive);
// null selects no template.

struct noesis_template_selector_vtable {
    void* (*select)(void* userdata, void* item, void* container);
};

typedef void (*noesis_template_selector_free_fn)(void* userdata);

class RustDataTemplateSelector final: public Noesis::DataTemplateSelector {
public:
    RustDataTemplateSelector(const noesis_template_selector_vtable* vt, void* userdata,
        noesis_template_selector_free_fn free_handler)
        : mVtable(*vt), mUserdata(userdata), mFree(free_handler) {}

    ~RustDataTemplateSelector() {
        void* ud = mUserdata;
        mUserdata = nullptr;
        if (mFree && ud) {
            mFree(ud);
        }
    }

    Noesis::DataTemplate* SelectTemplate(Noesis::BaseComponent* item,
        Noesis::DependencyObject* container) override {
        if (!mVtable.select) return nullptr;
        void* result = mVtable.select(mUserdata, item, container);
        return Noesis::DynamicCast<Noesis::DataTemplate*>(
            static_cast<Noesis::BaseComponent*>(result));
    }

    NS_IMPLEMENT_INLINE_REFLECTION(RustDataTemplateSelector, Noesis::DataTemplateSelector,
                                   "DmNoesis.RustDataTemplateSelector") {}

private:
    noesis_template_selector_vtable  mVtable;
    void*                               mUserdata;
    noesis_template_selector_free_fn mFree;
};

}  // namespace

// ── Boxing: float ───────────────────────────────────────────────────────────
//
// Companion to the bool/int32/double boxers in noesis_binding.cpp. Needed
// because several common DPs are `float` (FontSize, Opacity, …): a Style Setter
// or ResourceDictionary entry whose value is a BoxedValue<double> will NOT apply
// to a float DP (no implicit unbox-coercion), so style setters on float
// properties must carry a BoxedValue<float>. +1 ref for the caller.
extern "C" void* noesis_box_float(float value) {
    Noesis::Ptr<Noesis::BoxedValue> boxed = Noesis::Boxing::Box<float>(value);
    return handout(boxed.GetPtr());
}

// ── ResourceDictionary ──────────────────────────────────────────────────────

extern "C" void* noesis_resource_dictionary_create(void) {
    // new ResourceDictionary starts at refcount 1 — the caller's +1, balanced
    // by noesis_resource_dictionary_destroy (or any AddRef a consumer takes,
    // e.g. SetApplicationResources / SetResources).
    auto* d = new Noesis::ResourceDictionary();
    return static_cast<Noesis::BaseComponent*>(d);
}

extern "C" void noesis_resource_dictionary_destroy(void* dict) {
    if (!dict) return;
    static_cast<Noesis::BaseComponent*>(dict)->Release();
}

// Parse a bare <ResourceDictionary> from an in-memory XAML string. Returns a
// +1-owned ResourceDictionary* (NULL if the XAML is malformed or its root is
// not a ResourceDictionary).
extern "C" void* noesis_resource_dictionary_parse(const char* xaml) {
    if (!xaml) return nullptr;
    Noesis::Ptr<Noesis::BaseComponent> root = Noesis::GUI::ParseXaml(xaml);
    if (!root) return nullptr;
    Noesis::Ptr<Noesis::ResourceDictionary> dict =
        Noesis::DynamicPtrCast<Noesis::ResourceDictionary>(root);
    if (!dict) return nullptr;
    return dict.GiveOwnership();
}

// Number of entries in the base dictionary (excluding merged dictionaries).
extern "C" uint32_t noesis_resource_dictionary_count(void* dict) {
    Noesis::ResourceDictionary* d = as_dict(dict);
    return d ? d->Count() : 0u;
}

// Add a borrowed value under `key`; the dictionary stores its own reference.
// Returns false on a NULL/non-dictionary handle or NULL key/value.
extern "C" bool noesis_resource_dictionary_add(void* dict, const char* key, void* value) {
    Noesis::ResourceDictionary* d = as_dict(dict);
    if (!d || !key || !value) return false;
    d->Add(key, static_cast<Noesis::BaseComponent*>(value));
    return true;
}

// Whether the base dictionary (or a merged one) contains `key`.
extern "C" bool noesis_resource_dictionary_contains(void* dict, const char* key) {
    Noesis::ResourceDictionary* d = as_dict(dict);
    if (!d || !key) return false;
    return d->Contains(key);
}

// Borrowed (no +1) lookup by key — valid while the dictionary keeps the entry.
// NULL if absent. Uses Find (the non-throwing variant) so a miss is a clean
// NULL rather than an error.
extern "C" void* noesis_resource_dictionary_find(void* dict, const char* key) {
    Noesis::ResourceDictionary* d = as_dict(dict);
    if (!d || !key) return nullptr;
    Noesis::Ptr<Noesis::BaseComponent> found;
    if (!d->Find(key, found)) return nullptr;
    // Borrowed: the dictionary still owns the entry; `found` releases its local
    // ref on scope exit, leaving the dictionary's own reference intact.
    return found.GetPtr();
}

// Add `merged` to `dict`'s MergedDictionaries collection. The collection takes
// its own reference. Returns false on a NULL/non-dictionary handle.
extern "C" bool noesis_resource_dictionary_add_merged(void* dict, void* merged) {
    Noesis::ResourceDictionary* d = as_dict(dict);
    Noesis::ResourceDictionary* m = as_dict(merged);
    if (!d || !m) return false;
    Noesis::ResourceDictionaryCollection* merged_dicts = d->GetMergedDictionaries();
    if (!merged_dicts) return false;
    merged_dicts->Add(m);
    return true;
}

// ── Application resources ────────────────────────────────────────────────────

// Install `dict` as the process-global application resources. Noesis takes its
// own reference; the caller keeps ownership of its handle.
extern "C" void noesis_gui_set_application_resources(void* dict) {
    Noesis::GUI::SetApplicationResources(as_dict(dict));
}

// Borrowed (no +1) application ResourceDictionary*, or NULL if none installed.
// Owned by the GUI subsystem — do NOT release.
extern "C" void* noesis_gui_get_application_resources(void) {
    return Noesis::GUI::GetApplicationResources();
}

// Register `uri`'s dictionary in the internal theme (default styles). Returns
// false on a NULL/empty uri.
extern "C" bool noesis_gui_register_default_styles(const char* uri) {
    if (!uri || !*uri) return false;
    Noesis::GUI::RegisterDefaultStyles(Noesis::Uri(uri));
    return true;
}

// ── Per-element resources ────────────────────────────────────────────────────

// +1-owned ResourceDictionary* for `element`'s local Resources (AddRef'd so Rust
// can own it), or NULL if the element has none / is not a FrameworkElement.
extern "C" void* noesis_framework_element_get_resources(void* element) {
    Noesis::FrameworkElement* fe = as_element(element);
    if (!fe) return nullptr;
    return handout(fe->GetResources());
}

// Replace `element`'s local Resources with `dict` (Noesis takes its own ref).
// Returns false if `element` is not a FrameworkElement or `dict` not a
// ResourceDictionary.
extern "C" bool noesis_framework_element_set_resources(void* element, void* dict) {
    Noesis::FrameworkElement* fe = as_element(element);
    Noesis::ResourceDictionary* d = as_dict(dict);
    if (!fe || !d) return false;
    fe->SetResources(d);
    return true;
}

// Non-throwing resource lookup walking the logical parent chain + application
// resources. Borrowed (no +1) — valid transiently. NULL if not found or
// `element` is not a FrameworkElement. This is the TryFindResource-style
// variant: FrameworkElement::FindResource returns NULL on a miss (it does not
// throw), so callers get an honest Option.
extern "C" void* noesis_framework_element_find_resource(void* element, const char* key) {
    Noesis::FrameworkElement* fe = as_element(element);
    if (!fe || !key) return nullptr;
    return fe->FindResource(key);
}

// ── Style ────────────────────────────────────────────────────────────────────

extern "C" void* noesis_style_create(void) {
    // new Style starts at refcount 1 — the caller's +1, balanced by
    // noesis_style_destroy (or an AddRef from SetStyle / a ResourceDictionary).
    auto* s = new Noesis::Style();
    return static_cast<Noesis::BaseComponent*>(s);
}

extern "C" void noesis_style_destroy(void* style) {
    if (!style) return;
    static_cast<Noesis::BaseComponent*>(style)->Release();
}

// Resolve `type_name` through the reflection registry and set it as the style's
// TargetType. The type must already be registered (referencing it from loaded
// XAML guarantees this; the built-in controls register on first use). Returns
// false on a NULL/non-Style handle or an unknown type name.
extern "C" bool noesis_style_set_target_type(void* style, const char* type_name) {
    Noesis::Style* s = as_style(style);
    if (!s || !type_name) return false;
    Noesis::Symbol sym(type_name, Noesis::Symbol::NullIfNotFound());
    if (sym.IsNull()) return false;
    const Noesis::Type* type = Noesis::Reflection::GetType(sym);
    if (!type) return false;
    s->SetTargetType(type);
    return true;
}

// Append a Setter to the style: resolve `dp_name` as a DependencyProperty on the
// style's TargetType, build `new Setter` with that property + boxed `value`, and
// add it to GetSetters(). The setter stores its own reference to `value`.
// Returns false if no TargetType is set, the DP name is unknown on that type,
// the value is NULL, or the handle is not a Style.
extern "C" bool noesis_style_add_setter(void* style, const char* dp_name, void* value) {
    Noesis::Style* s = as_style(style);
    if (!s || !dp_name || !value) return false;

    const Noesis::Type* target = s->GetTargetType();
    if (!target) return false;
    const auto* targetClass = Noesis::DynamicCast<const Noesis::TypeClass*>(target);
    if (!targetClass) return false;

    const Noesis::DependencyProperty* dp =
        Noesis::FindDependencyProperty(targetClass, Noesis::Symbol(dp_name));
    if (!dp) return false;

    Noesis::BaseSetterCollection* setters = s->GetSetters();
    if (!setters) return false;

    // new Setter starts at refcount 1; the local Ptr adopts that and releases on
    // scope exit — Add takes the collection's own reference.
    Noesis::Ptr<Noesis::Setter> setter = *new Noesis::Setter();
    setter->SetProperty(dp);
    setter->SetValue(static_cast<Noesis::BaseComponent*>(value));
    setters->Add(setter.GetPtr());
    return true;
}

// Set the BasedOn style (inheritance). Noesis takes its own reference. NULL
// `base` clears it. No-op on a NULL/non-Style handle.
extern "C" void noesis_style_set_based_on(void* style, void* base) {
    Noesis::Style* s = as_style(style);
    if (s) s->SetBasedOn(as_style(base));
}

// ── FrameworkElement style ───────────────────────────────────────────────────

extern "C" bool noesis_framework_element_set_style(void* element, void* style) {
    Noesis::FrameworkElement* fe = as_element(element);
    Noesis::Style* s = as_style(style);
    if (!fe || !s) return false;
    fe->SetStyle(s);
    return true;
}

// +1-owned Style* for `element`'s assigned Style (AddRef'd so Rust can own it),
// or NULL if none / not a FrameworkElement.
extern "C" void* noesis_framework_element_get_style(void* element) {
    Noesis::FrameworkElement* fe = as_element(element);
    if (!fe) return nullptr;
    return handout(fe->GetStyle());
}

// ── Templates ────────────────────────────────────────────────────────────────

// Parse a bare <ControlTemplate> from an in-memory XAML string. Returns a
// +1-owned ControlTemplate* (NULL if malformed or the root is not a
// ControlTemplate).
extern "C" void* noesis_control_template_parse(const char* xaml) {
    if (!xaml) return nullptr;
    Noesis::Ptr<Noesis::BaseComponent> root = Noesis::GUI::ParseXaml(xaml);
    if (!root) return nullptr;
    Noesis::Ptr<Noesis::ControlTemplate> tmpl =
        Noesis::DynamicPtrCast<Noesis::ControlTemplate>(root);
    if (!tmpl) return nullptr;
    return tmpl.GiveOwnership();
}

// Parse a bare <DataTemplate> from an in-memory XAML string. Returns a
// +1-owned DataTemplate* (NULL if malformed or the root is not a DataTemplate).
// Assign it via the existing set_component path on ContentTemplate / ItemTemplate.
extern "C" void* noesis_data_template_parse(const char* xaml) {
    if (!xaml) return nullptr;
    Noesis::Ptr<Noesis::BaseComponent> root = Noesis::GUI::ParseXaml(xaml);
    if (!root) return nullptr;
    Noesis::Ptr<Noesis::DataTemplate> tmpl =
        Noesis::DynamicPtrCast<Noesis::DataTemplate>(root);
    if (!tmpl) return nullptr;
    return tmpl.GiveOwnership();
}

// Assign `tmpl` (a ControlTemplate) to `control` via Control::SetTemplate.
// Noesis takes its own reference. Returns false if `control` is not a Control or
// `tmpl` is not a ControlTemplate.
extern "C" bool noesis_control_set_template(void* control, void* tmpl) {
    auto* c = Noesis::DynamicCast<Noesis::Control*>(static_cast<Noesis::BaseComponent*>(control));
    auto* t =
        Noesis::DynamicCast<Noesis::ControlTemplate*>(static_cast<Noesis::BaseComponent*>(tmpl));
    if (!c || !t) return false;
    c->SetTemplate(t);
    return true;
}

// +1-owned ControlTemplate* for `control`'s assigned Template (AddRef'd so Rust
// can own it), or NULL if none / not a Control.
extern "C" void* noesis_control_get_template(void* control) {
    auto* c = Noesis::DynamicCast<Noesis::Control*>(static_cast<Noesis::BaseComponent*>(control));
    if (!c) return nullptr;
    return handout(c->GetTemplate());
}

// FrameworkTemplate::FindName — find a named element within `tmpl` as applied to
// `templated_parent`. Borrowed (no +1); valid while the template stays applied.
// NULL if `tmpl` is not a FrameworkTemplate, `templated_parent` is not a
// FrameworkElement, or the name is not found in the applied template.
extern "C" void* noesis_framework_template_find_name(
    void* tmpl, const char* name, void* templated_parent) {
    auto* t = Noesis::DynamicCast<Noesis::FrameworkTemplate*>(
        static_cast<Noesis::BaseComponent*>(tmpl));
    Noesis::FrameworkElement* parent = as_element(templated_parent);
    if (!t || !name || !parent) return nullptr;
    return t->FindName(name, parent);
}

// ── Style triggers ───────────────────────────────────────────────────────────
//
// Construct Trigger / DataTrigger / MultiTrigger / EventTrigger from code and
// attach them to a Style's Triggers collection (Style::GetTriggers), then read
// the trigger surface back from the LIVE objects. A property/value/setter-count
// read on a trigger fetched back out of the collection proves the construction
// crossed the FFI rather than echoing a Rust cache.
//
// OWNERSHIP: *_create returns a +1-owned BaseTrigger* (released via the generic
// noesis_base_component_release). Adding it to a Style's Triggers takes the
// collection's own reference, so the create handle may be dropped afterwards.

// ── Property Trigger (Trigger) ───────────────────────────────────────────────

extern "C" void* noesis_templates_trigger_create(void) {
    auto* t = new Noesis::Trigger();
    return static_cast<Noesis::BaseComponent*>(t);
}

// Set the Trigger's Property by name, resolved on `type_name`. Returns false on
// a non-Trigger handle or an unresolvable DP.
extern "C" bool noesis_templates_trigger_set_property(
    void* trigger, const char* type_name, const char* dp_name) {
    auto* t = Noesis::DynamicCast<Noesis::Trigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return false;
    const Noesis::DependencyProperty* dp = resolve_dp(type_name, dp_name);
    if (!dp) return false;
    t->SetProperty(dp);
    return true;
}

// Borrowed name of the Trigger's Property (valid while the DP exists, which is
// process-lifetime), or null if unset / not a Trigger.
extern "C" const char* noesis_templates_trigger_get_property_name(void* trigger) {
    auto* t = Noesis::DynamicCast<Noesis::Trigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return nullptr;
    const Noesis::DependencyProperty* dp = t->GetProperty();
    return dp ? dp->GetName().Str() : nullptr;
}

extern "C" bool noesis_templates_trigger_set_value(void* trigger, void* value) {
    auto* t = Noesis::DynamicCast<Noesis::Trigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t || !value) return false;
    t->SetValue(static_cast<Noesis::BaseComponent*>(value));
    return true;
}

// +1-owned (AddRef'd) copy of the Trigger's Value, or null.
extern "C" void* noesis_templates_trigger_get_value(void* trigger) {
    auto* t = Noesis::DynamicCast<Noesis::Trigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return nullptr;
    return handout(t->GetValue());
}

extern "C" bool noesis_templates_trigger_add_setter(
    void* trigger, const char* type_name, const char* dp_name, void* value) {
    auto* t = Noesis::DynamicCast<Noesis::Trigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return false;
    return add_setter_to(t->GetSetters(), type_name, dp_name, value);
}

extern "C" int32_t noesis_templates_trigger_setter_count(void* trigger) {
    auto* t = Noesis::DynamicCast<Noesis::Trigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return -1;
    Noesis::BaseSetterCollection* s = t->GetSetters();
    return s ? s->Count() : 0;
}

// ── Data Trigger (DataTrigger) ───────────────────────────────────────────────

extern "C" void* noesis_templates_data_trigger_create(void) {
    auto* t = new Noesis::DataTrigger();
    return static_cast<Noesis::BaseComponent*>(t);
}

// Set the DataTrigger's Binding (any BaseBinding* — e.g. a Binding from
// noesis_binding.cpp). Returns false on a non-DataTrigger handle or a value that
// is not a BaseBinding.
extern "C" bool noesis_templates_data_trigger_set_binding(void* trigger, void* binding) {
    auto* t =
        Noesis::DynamicCast<Noesis::DataTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    auto* b = Noesis::DynamicCast<Noesis::BaseBinding*>(static_cast<Noesis::BaseComponent*>(binding));
    if (!t || !b) return false;
    t->SetBinding(b);
    return true;
}

// +1-owned (AddRef'd) copy of the DataTrigger's Binding, or null.
extern "C" void* noesis_templates_data_trigger_get_binding(void* trigger) {
    auto* t =
        Noesis::DynamicCast<Noesis::DataTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return nullptr;
    return handout(t->GetBinding());
}

extern "C" bool noesis_templates_data_trigger_set_value(void* trigger, void* value) {
    auto* t =
        Noesis::DynamicCast<Noesis::DataTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t || !value) return false;
    t->SetValue(static_cast<Noesis::BaseComponent*>(value));
    return true;
}

extern "C" void* noesis_templates_data_trigger_get_value(void* trigger) {
    auto* t =
        Noesis::DynamicCast<Noesis::DataTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return nullptr;
    return handout(t->GetValue());
}

extern "C" bool noesis_templates_data_trigger_add_setter(
    void* trigger, const char* type_name, const char* dp_name, void* value) {
    auto* t =
        Noesis::DynamicCast<Noesis::DataTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return false;
    return add_setter_to(t->GetSetters(), type_name, dp_name, value);
}

extern "C" int32_t noesis_templates_data_trigger_setter_count(void* trigger) {
    auto* t =
        Noesis::DynamicCast<Noesis::DataTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return -1;
    Noesis::BaseSetterCollection* s = t->GetSetters();
    return s ? s->Count() : 0;
}

// ── Multi Trigger (MultiTrigger) ─────────────────────────────────────────────

extern "C" void* noesis_templates_multi_trigger_create(void) {
    auto* t = new Noesis::MultiTrigger();
    return static_cast<Noesis::BaseComponent*>(t);
}

// Append a Condition{ Property=resolve_dp(type,dp), Value=value } to the
// MultiTrigger's Conditions. Returns false on a non-MultiTrigger handle, an
// unresolvable DP, or a null value.
extern "C" bool noesis_templates_multi_trigger_add_condition(
    void* trigger, const char* type_name, const char* dp_name, void* value) {
    auto* t =
        Noesis::DynamicCast<Noesis::MultiTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t || !value) return false;
    const Noesis::DependencyProperty* dp = resolve_dp(type_name, dp_name);
    if (!dp) return false;
    Noesis::ConditionCollection* conditions = t->GetConditions();
    if (!conditions) return false;
    Noesis::Ptr<Noesis::Condition> condition = *new Noesis::Condition();
    condition->SetProperty(dp);
    condition->SetValue(static_cast<Noesis::BaseComponent*>(value));
    conditions->Add(condition.GetPtr());
    return true;
}

extern "C" int32_t noesis_templates_multi_trigger_condition_count(void* trigger) {
    auto* t =
        Noesis::DynamicCast<Noesis::MultiTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return -1;
    Noesis::ConditionCollection* c = t->GetConditions();
    return c ? c->Count() : 0;
}

// Borrowed Property name of the condition at `index`, or null.
extern "C" const char* noesis_templates_multi_trigger_get_condition_property_name(
    void* trigger, uint32_t index) {
    auto* t =
        Noesis::DynamicCast<Noesis::MultiTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return nullptr;
    Noesis::ConditionCollection* c = t->GetConditions();
    if (!c || index >= static_cast<uint32_t>(c->Count())) return nullptr;
    Noesis::Condition* cond = c->Get(index);
    if (!cond) return nullptr;
    const Noesis::DependencyProperty* dp = cond->GetProperty();
    return dp ? dp->GetName().Str() : nullptr;
}

// +1-owned (AddRef'd) Value of the condition at `index`, or null.
extern "C" void* noesis_templates_multi_trigger_get_condition_value(
    void* trigger, uint32_t index) {
    auto* t =
        Noesis::DynamicCast<Noesis::MultiTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return nullptr;
    Noesis::ConditionCollection* c = t->GetConditions();
    if (!c || index >= static_cast<uint32_t>(c->Count())) return nullptr;
    Noesis::Condition* cond = c->Get(index);
    return cond ? handout(cond->GetValue()) : nullptr;
}

extern "C" bool noesis_templates_multi_trigger_add_setter(
    void* trigger, const char* type_name, const char* dp_name, void* value) {
    auto* t =
        Noesis::DynamicCast<Noesis::MultiTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return false;
    return add_setter_to(t->GetSetters(), type_name, dp_name, value);
}

extern "C" int32_t noesis_templates_multi_trigger_setter_count(void* trigger) {
    auto* t =
        Noesis::DynamicCast<Noesis::MultiTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return -1;
    Noesis::BaseSetterCollection* s = t->GetSetters();
    return s ? s->Count() : 0;
}

// ── Event Trigger (EventTrigger) ─────────────────────────────────────────────

extern "C" void* noesis_templates_event_trigger_create(void) {
    auto* t = new Noesis::EventTrigger();
    return static_cast<Noesis::BaseComponent*>(t);
}

// Resolve a RoutedEvent named `event_name` registered on `owner_type` and set it
// as the EventTrigger's RoutedEvent. Returns false on a non-EventTrigger handle,
// an unknown owner type, or an unknown routed event on that type.
extern "C" bool noesis_templates_event_trigger_set_routed_event(
    void* trigger, const char* owner_type, const char* event_name) {
    auto* t =
        Noesis::DynamicCast<Noesis::EventTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t || !owner_type || !event_name) return false;
    Noesis::Symbol tsym(owner_type, Noesis::Symbol::NullIfNotFound());
    if (tsym.IsNull()) return false;
    const auto* tc = Noesis::DynamicCast<const Noesis::TypeClass*>(Noesis::Reflection::GetType(tsym));
    if (!tc) return false;
    const Noesis::RoutedEvent* ev = Noesis::FindRoutedEvent(tc, Noesis::Symbol(event_name));
    if (!ev) return false;
    t->SetRoutedEvent(ev);
    return true;
}

// Borrowed name of the EventTrigger's RoutedEvent, or null if unset.
extern "C" const char* noesis_templates_event_trigger_get_routed_event_name(void* trigger) {
    auto* t =
        Noesis::DynamicCast<Noesis::EventTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return nullptr;
    const Noesis::RoutedEvent* ev = t->GetRoutedEvent();
    return ev ? ev->GetName().Str() : nullptr;
}

extern "C" bool noesis_templates_event_trigger_set_source_name(
    void* trigger, const char* name) {
    auto* t =
        Noesis::DynamicCast<Noesis::EventTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t || !name) return false;
    t->SetSourceName(name);
    return true;
}

// Borrowed SourceName of the EventTrigger (empty string if unset), or null on a
// non-EventTrigger handle.
extern "C" const char* noesis_templates_event_trigger_get_source_name(void* trigger) {
    auto* t =
        Noesis::DynamicCast<Noesis::EventTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    return t ? t->GetSourceName() : nullptr;
}

// Number of TriggerAction objects in the EventTrigger's Actions collection.
extern "C" int32_t noesis_templates_event_trigger_action_count(void* trigger) {
    auto* t =
        Noesis::DynamicCast<Noesis::EventTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return -1;
    Noesis::TriggerActionCollection* a = t->GetActions();
    return a ? a->Count() : 0;
}

// Append `action` (any TriggerAction*, e.g. a BeginStoryboard from
// noesis_animation.cpp) to the EventTrigger's Actions collection (which takes
// its own reference). Returns false on a non-EventTrigger handle or a value that
// is not a TriggerAction. Read back via noesis_templates_event_trigger_action_count.
extern "C" bool noesis_templates_event_trigger_add_action(void* trigger, void* action) {
    auto* t =
        Noesis::DynamicCast<Noesis::EventTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    auto* a =
        Noesis::DynamicCast<Noesis::TriggerAction*>(static_cast<Noesis::BaseComponent*>(action));
    if (!t || !a) return false;
    Noesis::TriggerActionCollection* actions = t->GetActions();
    if (!actions) return false;
    actions->Add(a);
    return true;
}

// ── Multi Data Trigger (MultiDataTrigger) ────────────────────────────────────
//
// Binding-condition sibling of the MultiTrigger above: each Condition matches a
// bound data value (Condition::SetBinding) against a Value, rather than a
// dependency property. Setters apply when every condition is met.

extern "C" void* noesis_templates_multi_data_trigger_create(void) {
    auto* t = new Noesis::MultiDataTrigger();
    return static_cast<Noesis::BaseComponent*>(t);
}

// Append a Condition{ Binding=binding, Value=value } to the MultiDataTrigger's
// Conditions. Returns false on a non-MultiDataTrigger handle, a value that is
// not a BaseBinding, or a null value.
extern "C" bool noesis_templates_multi_data_trigger_add_condition(
    void* trigger, void* binding, void* value) {
    auto* t = Noesis::DynamicCast<Noesis::MultiDataTrigger*>(
        static_cast<Noesis::BaseComponent*>(trigger));
    auto* b =
        Noesis::DynamicCast<Noesis::BaseBinding*>(static_cast<Noesis::BaseComponent*>(binding));
    if (!t || !b || !value) return false;
    Noesis::ConditionCollection* conditions = t->GetConditions();
    if (!conditions) return false;
    Noesis::Ptr<Noesis::Condition> condition = *new Noesis::Condition();
    condition->SetBinding(b);
    condition->SetValue(static_cast<Noesis::BaseComponent*>(value));
    conditions->Add(condition.GetPtr());
    return true;
}

extern "C" int32_t noesis_templates_multi_data_trigger_condition_count(void* trigger) {
    auto* t = Noesis::DynamicCast<Noesis::MultiDataTrigger*>(
        static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return -1;
    Noesis::ConditionCollection* c = t->GetConditions();
    return c ? c->Count() : 0;
}

// Whether the condition at `index` has a Binding set (read back from the live
// object). -1 on a non-MultiDataTrigger handle or out-of-range index; 0/1 else.
extern "C" int32_t noesis_templates_multi_data_trigger_condition_has_binding(
    void* trigger, uint32_t index) {
    auto* t = Noesis::DynamicCast<Noesis::MultiDataTrigger*>(
        static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return -1;
    Noesis::ConditionCollection* c = t->GetConditions();
    if (!c || index >= static_cast<uint32_t>(c->Count())) return -1;
    Noesis::Condition* cond = c->Get(index);
    if (!cond) return -1;
    return cond->GetBinding() != nullptr ? 1 : 0;
}

// +1-owned (AddRef'd) Value of the condition at `index`, or null.
extern "C" void* noesis_templates_multi_data_trigger_get_condition_value(
    void* trigger, uint32_t index) {
    auto* t = Noesis::DynamicCast<Noesis::MultiDataTrigger*>(
        static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return nullptr;
    Noesis::ConditionCollection* c = t->GetConditions();
    if (!c || index >= static_cast<uint32_t>(c->Count())) return nullptr;
    Noesis::Condition* cond = c->Get(index);
    return cond ? handout(cond->GetValue()) : nullptr;
}

extern "C" bool noesis_templates_multi_data_trigger_add_setter(
    void* trigger, const char* type_name, const char* dp_name, void* value) {
    auto* t = Noesis::DynamicCast<Noesis::MultiDataTrigger*>(
        static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return false;
    return add_setter_to(t->GetSetters(), type_name, dp_name, value);
}

extern "C" int32_t noesis_templates_multi_data_trigger_setter_count(void* trigger) {
    auto* t = Noesis::DynamicCast<Noesis::MultiDataTrigger*>(
        static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return -1;
    Noesis::BaseSetterCollection* s = t->GetSetters();
    return s ? s->Count() : 0;
}

// ── Style ⇄ Triggers ─────────────────────────────────────────────────────────

// Append a trigger to a Style's Triggers collection. The collection takes its
// own reference. Returns false on a non-Style handle or a non-trigger value.
extern "C" bool noesis_templates_style_add_trigger(void* style, void* trigger) {
    Noesis::Style* s = as_style(style);
    Noesis::BaseTrigger* t = as_trigger(trigger);
    if (!s || !t) return false;
    Noesis::TriggerCollection* triggers = s->GetTriggers();
    if (!triggers) return false;
    triggers->Add(t);
    return true;
}

extern "C" int32_t noesis_templates_style_trigger_count(void* style) {
    Noesis::Style* s = as_style(style);
    if (!s) return -1;
    Noesis::TriggerCollection* triggers = s->GetTriggers();
    return triggers ? triggers->Count() : 0;
}

// +1-owned (AddRef'd) trigger at `index` in the Style's Triggers, so Rust can
// re-read its property/value/setter surface from the live object. Null on a
// non-Style handle or out-of-range index.
extern "C" void* noesis_templates_style_get_trigger(void* style, uint32_t index) {
    Noesis::Style* s = as_style(style);
    if (!s) return nullptr;
    Noesis::TriggerCollection* triggers = s->GetTriggers();
    if (!triggers || index >= static_cast<uint32_t>(triggers->Count())) return nullptr;
    return handout(triggers->Get(index));
}

// ── DataTemplateSelector from Rust ───────────────────────────────────────────

// Create a DataTemplateSelector whose SelectTemplate() trampolines into Rust.
// Returns a +1-owned selector (released via noesis_templates_selector_destroy
// or the generic release). The userdata box is donated and freed once.
extern "C" void* noesis_templates_selector_create(
    const noesis_template_selector_vtable* vt, void* userdata,
    noesis_template_selector_free_fn free_handler) {
    if (!vt) return nullptr;
    auto* sel = new RustDataTemplateSelector(vt, userdata, free_handler);
    return static_cast<Noesis::BaseComponent*>(sel);
}

extern "C" void noesis_templates_selector_destroy(void* selector) {
    if (!selector) return;
    static_cast<Noesis::BaseComponent*>(selector)->Release();
}

// Drive SelectTemplate(item, container) through the C++ virtual (which dispatches
// to the Rust callback for a RustDataTemplateSelector, or runs native logic for
// any other selector). Returns the borrowed DataTemplate* the selector chose, or
// null. `item` / `container` may be null.
extern "C" void* noesis_templates_selector_select(
    void* selector, void* item, void* container) {
    auto* sel = Noesis::DynamicCast<Noesis::DataTemplateSelector*>(
        static_cast<Noesis::BaseComponent*>(selector));
    if (!sel) return nullptr;
    auto* obj = Noesis::DynamicCast<Noesis::DependencyObject*>(
        static_cast<Noesis::BaseComponent*>(container));
    return sel->SelectTemplate(static_cast<Noesis::BaseComponent*>(item), obj);
}
