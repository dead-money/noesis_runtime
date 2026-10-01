// ResourceDictionary, Style, templates, style triggers, and a Rust-backed
// DataTemplateSelector.
//
// Ownership: *_create / *_parse return a +1 object (release with the matching
// *_destroy or noesis_base_component_release). Whether a getter hands out +1 or
// a borrowed pointer is documented per function, in noesis_shim.h or below.

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

// Adds a +1 for the caller; null passes through.
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

// Takes an explicit type because a trigger's Property can live on a type other
// than the Style's TargetType. Null on an unknown type or DP name.
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

// False on a null collection, an unresolvable DP, or a null value.
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

// SelectTemplate forwards to Rust. The callback returns a borrowed
// DataTemplate* (null selects none), so the Rust side must keep it alive. The
// donated userdata is freed once, when the last reference drops.

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

// Float DPs (FontSize, Opacity, ...) need a BoxedValue<float>: a Setter or
// resource holding a BoxedValue<double> is not coerced and does not apply.
extern "C" void* noesis_box_float(float value) {
    Noesis::Ptr<Noesis::BoxedValue> boxed = Noesis::Boxing::Box<float>(value);
    return handout(boxed.GetPtr());
}

extern "C" void* noesis_resource_dictionary_create(void) {
    // `new` starts at refcount 1: that is the caller's +1.
    auto* d = new Noesis::ResourceDictionary();
    return static_cast<Noesis::BaseComponent*>(d);
}

extern "C" void noesis_resource_dictionary_destroy(void* dict) {
    if (!dict) return;
    static_cast<Noesis::BaseComponent*>(dict)->Release();
}

extern "C" void* noesis_resource_dictionary_parse(const char* xaml) {
    if (!xaml) return nullptr;
    Noesis::Ptr<Noesis::BaseComponent> root = Noesis::GUI::ParseXaml(xaml);
    if (!root) return nullptr;
    Noesis::Ptr<Noesis::ResourceDictionary> dict =
        Noesis::DynamicPtrCast<Noesis::ResourceDictionary>(root);
    if (!dict) return nullptr;
    return dict.GiveOwnership();
}

extern "C" uint32_t noesis_resource_dictionary_count(void* dict) {
    Noesis::ResourceDictionary* d = as_dict(dict);
    return d ? d->Count() : 0u;
}

extern "C" bool noesis_resource_dictionary_add(void* dict, const char* key, void* value) {
    Noesis::ResourceDictionary* d = as_dict(dict);
    if (!d || !key || !value) return false;
    d->Add(key, static_cast<Noesis::BaseComponent*>(value));
    return true;
}

extern "C" bool noesis_resource_dictionary_contains(void* dict, const char* key) {
    Noesis::ResourceDictionary* d = as_dict(dict);
    if (!d || !key) return false;
    return d->Contains(key);
}

extern "C" void* noesis_resource_dictionary_find(void* dict, const char* key) {
    Noesis::ResourceDictionary* d = as_dict(dict);
    if (!d || !key) return nullptr;
    Noesis::Ptr<Noesis::BaseComponent> found;
    if (!d->Find(key, found)) return nullptr;
    // Borrowed: `found` drops its ref on return; the dictionary keeps its own.
    return found.GetPtr();
}

extern "C" bool noesis_resource_dictionary_add_merged(void* dict, void* merged) {
    Noesis::ResourceDictionary* d = as_dict(dict);
    Noesis::ResourceDictionary* m = as_dict(merged);
    if (!d || !m) return false;
    Noesis::ResourceDictionaryCollection* merged_dicts = d->GetMergedDictionaries();
    if (!merged_dicts) return false;
    merged_dicts->Add(m);
    return true;
}

extern "C" bool noesis_resource_dictionary_set_source(void* dict, const char* uri) {
    Noesis::ResourceDictionary* d = as_dict(dict);
    if (!d || !uri) return false;
    d->SetSource(Noesis::Uri(uri));
    return true;
}

// A non-dictionary pointer is ignored rather than clearing the resources.
extern "C" void noesis_gui_set_application_resources(void* dict) {
    if (!dict) {
        Noesis::GUI::SetApplicationResources(nullptr);
        return;
    }
    Noesis::ResourceDictionary* d = as_dict(dict);
    if (!d) return;
    Noesis::GUI::SetApplicationResources(d);
}

extern "C" void* noesis_gui_get_application_resources(void) {
    return Noesis::GUI::GetApplicationResources();
}

extern "C" bool noesis_gui_register_default_styles(const char* uri) {
    if (!uri || !*uri) return false;
    Noesis::GUI::RegisterDefaultStyles(Noesis::Uri(uri));
    return true;
}

extern "C" void* noesis_framework_element_get_resources(void* element) {
    Noesis::FrameworkElement* fe = as_element(element);
    if (!fe) return nullptr;
    return handout(fe->GetResources());
}

extern "C" bool noesis_framework_element_set_resources(void* element, void* dict) {
    Noesis::FrameworkElement* fe = as_element(element);
    Noesis::ResourceDictionary* d = as_dict(dict);
    if (!fe || !d) return false;
    fe->SetResources(d);
    return true;
}

extern "C" void* noesis_framework_element_find_resource(void* element, const char* key) {
    Noesis::FrameworkElement* fe = as_element(element);
    if (!fe || !key) return nullptr;
    return fe->FindResource(key);
}

extern "C" void* noesis_style_create(void) {
    // `new` starts at refcount 1: that is the caller's +1.
    auto* s = new Noesis::Style();
    return static_cast<Noesis::BaseComponent*>(s);
}

extern "C" void noesis_style_destroy(void* style) {
    if (!style) return;
    static_cast<Noesis::BaseComponent*>(style)->Release();
}

// The type must already be registered with Reflection; built-in controls
// register on first use.
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

    Noesis::Ptr<Noesis::Setter> setter = *new Noesis::Setter();
    setter->SetProperty(dp);
    setter->SetValue(static_cast<Noesis::BaseComponent*>(value));
    setters->Add(setter.GetPtr());
    return true;
}

extern "C" void noesis_style_set_based_on(void* style, void* base) {
    Noesis::Style* s = as_style(style);
    if (s) s->SetBasedOn(as_style(base));
}

extern "C" bool noesis_framework_element_set_style(void* element, void* style) {
    Noesis::FrameworkElement* fe = as_element(element);
    Noesis::Style* s = as_style(style);
    if (!fe || !s) return false;
    fe->SetStyle(s);
    return true;
}

extern "C" void* noesis_framework_element_get_style(void* element) {
    Noesis::FrameworkElement* fe = as_element(element);
    if (!fe) return nullptr;
    return handout(fe->GetStyle());
}

extern "C" void* noesis_control_template_parse(const char* xaml) {
    if (!xaml) return nullptr;
    Noesis::Ptr<Noesis::BaseComponent> root = Noesis::GUI::ParseXaml(xaml);
    if (!root) return nullptr;
    Noesis::Ptr<Noesis::ControlTemplate> tmpl =
        Noesis::DynamicPtrCast<Noesis::ControlTemplate>(root);
    if (!tmpl) return nullptr;
    return tmpl.GiveOwnership();
}

extern "C" void* noesis_data_template_parse(const char* xaml) {
    if (!xaml) return nullptr;
    Noesis::Ptr<Noesis::BaseComponent> root = Noesis::GUI::ParseXaml(xaml);
    if (!root) return nullptr;
    Noesis::Ptr<Noesis::DataTemplate> tmpl =
        Noesis::DynamicPtrCast<Noesis::DataTemplate>(root);
    if (!tmpl) return nullptr;
    return tmpl.GiveOwnership();
}

extern "C" bool noesis_control_set_template(void* control, void* tmpl) {
    auto* c = Noesis::DynamicCast<Noesis::Control*>(static_cast<Noesis::BaseComponent*>(control));
    auto* t =
        Noesis::DynamicCast<Noesis::ControlTemplate*>(static_cast<Noesis::BaseComponent*>(tmpl));
    if (!c || !t) return false;
    c->SetTemplate(t);
    return true;
}

extern "C" void* noesis_control_get_template(void* control) {
    auto* c = Noesis::DynamicCast<Noesis::Control*>(static_cast<Noesis::BaseComponent*>(control));
    if (!c) return nullptr;
    return handout(c->GetTemplate());
}

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
// Trigger / DataTrigger / MultiTrigger / MultiDataTrigger / EventTrigger built
// from code. Not declared in noesis_shim.h; the Rust externs live in src/ffi.rs.
// *_create returns a +1 BaseTrigger* (release with
// noesis_base_component_release); adding it to a Style's Triggers takes the
// collection's own reference. *_setter_count / *_condition_count /
// *_action_count return -1 on a wrong-type handle.

extern "C" void* noesis_templates_trigger_create(void) {
    auto* t = new Noesis::Trigger();
    return static_cast<Noesis::BaseComponent*>(t);
}

// Resolves `dp_name` on `type_name`. False on a non-Trigger or unknown DP.
extern "C" bool noesis_templates_trigger_set_property(
    void* trigger, const char* type_name, const char* dp_name) {
    auto* t = Noesis::DynamicCast<Noesis::Trigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return false;
    const Noesis::DependencyProperty* dp = resolve_dp(type_name, dp_name);
    if (!dp) return false;
    t->SetProperty(dp);
    return true;
}

// Borrowed DP name (process lifetime), or null if unset / not a Trigger.
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

// +1 reference to the Trigger's Value, or null.
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

extern "C" void* noesis_templates_data_trigger_create(void) {
    auto* t = new Noesis::DataTrigger();
    return static_cast<Noesis::BaseComponent*>(t);
}

// `binding` is any BaseBinding*. False on a wrong-type trigger or binding.
extern "C" bool noesis_templates_data_trigger_set_binding(void* trigger, void* binding) {
    auto* t =
        Noesis::DynamicCast<Noesis::DataTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    auto* b = Noesis::DynamicCast<Noesis::BaseBinding*>(static_cast<Noesis::BaseComponent*>(binding));
    if (!t || !b) return false;
    t->SetBinding(b);
    return true;
}

// +1 reference to the DataTrigger's Binding, or null.
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

extern "C" void* noesis_templates_multi_trigger_create(void) {
    auto* t = new Noesis::MultiTrigger();
    return static_cast<Noesis::BaseComponent*>(t);
}

// Appends Condition{Property = `type_name`.`dp_name`, Value = `value`}. False
// on a non-MultiTrigger, unknown DP, or null value.
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

// +1 reference to the condition's Value, or null.
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

extern "C" void* noesis_templates_event_trigger_create(void) {
    auto* t = new Noesis::EventTrigger();
    return static_cast<Noesis::BaseComponent*>(t);
}

// False on a non-EventTrigger, unknown owner type, or unknown event name.
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

extern "C" int32_t noesis_templates_event_trigger_action_count(void* trigger) {
    auto* t =
        Noesis::DynamicCast<Noesis::EventTrigger*>(static_cast<Noesis::BaseComponent*>(trigger));
    if (!t) return -1;
    Noesis::TriggerActionCollection* a = t->GetActions();
    return a ? a->Count() : 0;
}

// `action` is any TriggerAction*, e.g. a BeginStoryboard. The collection takes
// its own reference.
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

// MultiDataTrigger conditions match a Binding's value instead of a DP.

extern "C" void* noesis_templates_multi_data_trigger_create(void) {
    auto* t = new Noesis::MultiDataTrigger();
    return static_cast<Noesis::BaseComponent*>(t);
}

// Appends Condition{Binding = `binding`, Value = `value`}. False on a
// non-MultiDataTrigger, non-BaseBinding, or null value.
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

// 1 / 0 for whether the condition has a Binding; -1 on a wrong-type handle or
// out-of-range index.
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

// +1 reference to the condition's Value, or null.
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

// The Triggers collection takes its own reference. False on a non-Style or
// non-trigger.
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

// +1 reference to the trigger at `index`, or null.
extern "C" void* noesis_templates_style_get_trigger(void* style, uint32_t index) {
    Noesis::Style* s = as_style(style);
    if (!s) return nullptr;
    Noesis::TriggerCollection* triggers = s->GetTriggers();
    if (!triggers || index >= static_cast<uint32_t>(triggers->Count())) return nullptr;
    return handout(triggers->Get(index));
}

// ── DataTemplateSelector from Rust ───────────────────────────────────────────

// +1 selector. `userdata` is donated; `free_handler` runs once when the last
// reference drops. A null `vt` returns null without freeing `userdata`.
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

// Works on any DataTemplateSelector, not only Rust-backed ones. Returns the
// chosen template borrowed, or null. `item` / `container` may be null.
extern "C" void* noesis_templates_selector_select(
    void* selector, void* item, void* container) {
    auto* sel = Noesis::DynamicCast<Noesis::DataTemplateSelector*>(
        static_cast<Noesis::BaseComponent*>(selector));
    if (!sel) return nullptr;
    auto* obj = Noesis::DynamicCast<Noesis::DependencyObject*>(
        static_cast<Noesis::BaseComponent*>(container));
    return sel->SelectTemplate(static_cast<Noesis::BaseComponent*>(item), obj);
}
