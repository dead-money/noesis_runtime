//! Style triggers, `DataTemplateSelector`, and `TemplateSelector` constructed from code
//! and round-tripped through live Noesis objects.

use std::collections::HashMap;

use noesis_runtime::binding::{Binding, box_bool, box_f32, box_string};
use noesis_runtime::styles::DataTemplate;
use noesis_runtime::styles::{
    DataTrigger, EventTrigger, MultiTrigger, Style, TemplateSelector, Trigger,
};
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

// A scene that references every control type the trigger tests resolve by name,
// so Noesis's reflection registry knows them (the built-ins register on use).
const SCENE: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="200" Height="200">
  <StackPanel>
    <TextBlock Text="A"/>
    <ToggleButton Content="T"/>
    <Button Content="B"/>
  </StackPanel>
</Grid>"##;

struct InMem {
    bytes: HashMap<String, Vec<u8>>,
}

impl XamlProvider for InMem {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.bytes.get(uri).map(Vec::as_slice)
    }
}

fn boot() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();
}

// Force the built-in controls to register so type names resolve.
fn register_types() -> View {
    let mut bytes = HashMap::new();
    bytes.insert("scene.xaml".to_string(), SCENE.as_bytes().to_vec());
    let _registered = noesis_runtime::xaml_provider::set_xaml_provider(InMem { bytes });
    let root = FrameworkElement::load("scene.xaml").expect("scene load");
    let mut view = View::create(root);
    view.set_size(200, 200);
    view.activate();
    view.update(0.0);
    view
}

#[test]
fn templates_and_triggers_roundtrip() {
    boot();
    {
        let _view = register_types();

        let mut trigger = Trigger::new();
        assert!(
            trigger.set_property("ToggleButton", "IsChecked"),
            "IsChecked must resolve on ToggleButton"
        );
        assert!(
            !trigger.set_property("ToggleButton", "NoSuchProperty"),
            "unknown DP must not set the trigger property"
        );
        assert!(
            !trigger.set_property("NoSuchType", "IsChecked"),
            "unknown type must not set the trigger property"
        );
        assert!(trigger.set_value(&box_bool(true)));
        assert!(
            trigger.add_setter("ToggleButton", "Opacity", &box_f32(0.5)),
            "Opacity setter must resolve on ToggleButton"
        );
        assert!(
            !trigger.add_setter("ToggleButton", "Nope", &box_f32(1.0)),
            "unknown DP must not add a setter"
        );
        assert_eq!(trigger.setter_count(), 1);

        let mut style = Style::new();
        assert!(style.set_target_type("ToggleButton"));
        assert!(
            style.add_trigger(&trigger),
            "attach trigger to Style.Triggers"
        );
        assert_eq!(style.trigger_count(), 1, "Style.Triggers holds the trigger");

        let rb = style.get_trigger(0).expect("trigger 0");
        assert_eq!(
            rb.property_name().as_deref(),
            Some("IsChecked"),
            "trigger Property survived the round-trip"
        );
        assert_eq!(
            rb.value().and_then(|v| v.as_bool()),
            Some(true),
            "trigger Value survived the round-trip"
        );
        assert_eq!(rb.setter_count(), 1, "trigger Setter count survived");
        assert!(style.get_trigger(1).is_none(), "only one trigger");

        let mut data_trigger = DataTrigger::new();
        assert!(data_trigger.set_binding(&Binding::new("IsEnabled")));
        assert!(
            data_trigger.has_binding(),
            "Binding read back from the object"
        );
        assert!(data_trigger.set_value(&box_bool(false)));
        assert_eq!(
            data_trigger.value().and_then(|v| v.as_bool()),
            Some(false),
            "DataTrigger Value round-trip"
        );
        assert!(data_trigger.add_setter("TextBlock", "FontSize", &box_f32(20.0)));
        assert_eq!(data_trigger.setter_count(), 1);
        let mut tb_style = Style::new();
        assert!(tb_style.set_target_type("TextBlock"));
        assert!(tb_style.add_trigger(&data_trigger));
        assert_eq!(tb_style.trigger_count(), 1);

        let mut multi = MultiTrigger::new();
        assert!(multi.add_condition("ToggleButton", "IsChecked", &box_bool(true)));
        assert!(multi.add_condition("ToggleButton", "IsEnabled", &box_bool(false)));
        assert_eq!(multi.condition_count(), 2);
        assert_eq!(
            multi.condition_property_name(0).as_deref(),
            Some("IsChecked"),
            "condition[0] Property round-trip"
        );
        assert_eq!(
            multi.condition_value(1).and_then(|v| v.as_bool()),
            Some(false),
            "condition[1] Value round-trip"
        );
        assert!(multi.add_setter("ToggleButton", "Opacity", &box_f32(0.25)));
        assert_eq!(multi.setter_count(), 1);
        assert!(!multi.add_condition("ToggleButton", "Nope", &box_bool(true)));

        let mut multi_style = Style::new();
        assert!(multi_style.set_target_type("ToggleButton"));
        assert!(multi_style.add_trigger(&multi));
        assert_eq!(multi_style.get_trigger(0).expect("mt").condition_count(), 2);

        let mut event_trigger = EventTrigger::new();
        assert!(
            event_trigger.set_routed_event("Button", "Click"),
            "Click must resolve on Button"
        );
        assert!(
            !event_trigger.set_routed_event("Button", "NoSuchEvent"),
            "unknown event must not set"
        );
        assert_eq!(
            event_trigger.routed_event_name().as_deref(),
            Some("Click"),
            "RoutedEvent name round-trip"
        );
        assert!(event_trigger.set_source_name("PART_Toggle"));
        assert_eq!(
            event_trigger.source_name().as_deref(),
            Some("PART_Toggle"),
            "SourceName round-trip"
        );
        assert_eq!(event_trigger.action_count(), 0, "no actions added");
        let mut ev_style = Style::new();
        assert!(ev_style.set_target_type("Button"));
        assert!(ev_style.add_trigger(&event_trigger));
        assert_eq!(
            ev_style
                .get_trigger(0)
                .expect("et")
                .routed_event_name()
                .as_deref(),
            Some("Click")
        );

        let template = DataTemplate::parse(
            r##"<DataTemplate xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"><TextBlock Text="X"/></DataTemplate>"##,
        )
        .expect("parse DataTemplate");
        let template_ptr = template.raw() as usize;
        let selector = TemplateSelector::new(move |item: *mut std::ffi::c_void, _container| {
            if item.is_null() {
                None
            } else {
                Some(template_ptr as *mut std::ffi::c_void)
            }
        });
        let item = box_string("data");
        // SAFETY: item.raw() is a live boxed BaseComponent*; template stays alive.
        let chosen = unsafe { selector.select(item.raw(), std::ptr::null_mut()) }
            .expect("selector chose a template");
        assert_eq!(
            chosen.as_ptr() as usize,
            template_ptr,
            "the Rust callback's DataTemplate came back through SelectTemplate"
        );
        // SAFETY: both arguments are null.
        assert!(
            unsafe { selector.select(std::ptr::null_mut(), std::ptr::null_mut()) }.is_none(),
            "null item selects no template (callback round-trip both ways)"
        );
    }
    noesis_runtime::shutdown();
}
