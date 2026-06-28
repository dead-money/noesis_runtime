//! `BindingExpression::update_source` commits a `TwoWay`/`Explicit` binding;
//! a `OneWay` mirror distinguishes a real explicit commit from an implicit one.

use std::collections::HashMap;

use noesis_runtime::binding::{Binding, BindingMode, UpdateSourceTrigger, set_binding};
use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::ffi::{ClassBase, PropType};
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="240" Height="120">
  <StackPanel>
    <TextBox x:Name="Editor"/>
    <TextBox x:Name="Mirror"/>
  </StackPanel>
</Grid>"##;

struct InMem(HashMap<String, Vec<u8>>);
impl XamlProvider for InMem {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.0.get(uri).map(Vec::as_slice)
    }
}

struct NoopHandler;
impl PropertyChangeHandler for NoopHandler {
    fn on_changed(&self, _i: Instance, _idx: u32, _v: PropertyValue<'_>) {}
}

#[test]
fn binding_expression_explicit_update_source() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut builder =
            ClassBuilder::new("Sample.ExprVM", ClassBase::ContentControl, NoopHandler);
        let text_idx = builder.add_property("Text", PropType::String);
        let registration = builder.register().expect("VM class registration failed");
        let vm = registration
            .create_instance()
            .expect("create_instance returned None");
        vm.handle().set_string(text_idx, "init");

        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), XAML.as_bytes().to_vec());
        let _guard = noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let element = FrameworkElement::load("scene.xaml").expect("load_xaml returned None");
        let mut view = View::create(element);
        view.set_size(240, 120);
        view.activate();

        let mut content = view.content().expect("View::content returned None");
        // SAFETY: vm outlives this scope; Noesis stores its own reference.
        assert!(
            content.set_data_context(&vm),
            "set_data_context returned false"
        );

        let mut editor = content.find_name("Editor").expect("find Editor");
        let mirror = content.find_name("Mirror").expect("find Mirror");

        assert!(
            editor.binding_expression("Text").is_none(),
            "unbound property should have no BindingExpression"
        );

        // Editor: TwoWay + Explicit. Mirror: OneWay (tracks the source).
        let edit_b = Binding::new("Text")
            .mode(BindingMode::TwoWay)
            .update_source_trigger(UpdateSourceTrigger::Explicit);
        assert!(
            set_binding(&editor, "Text", &edit_b),
            "set_binding Editor.Text failed"
        );
        let mirror_b = Binding::new("Text").mode(BindingMode::OneWay);
        assert!(
            set_binding(&mirror, "Text", &mirror_b),
            "set_binding Mirror.Text failed"
        );

        assert!(view.update(0.0), "first Update should report change");
        let _ = view.update(0.016);

        assert_eq!(editor.get_string("Text").as_deref(), Some("init"));
        assert_eq!(mirror.get_string("Text").as_deref(), Some("init"));

        assert!(
            editor.binding_expression("NoSuchProperty").is_none(),
            "unknown DP name should have no BindingExpression"
        );

        // Edit the target. With UpdateSourceTrigger=Explicit, the source must
        // NOT change yet, so the OneWay Mirror still shows "init".
        assert!(
            editor.set_string("Text", "changed"),
            "set Editor.Text failed"
        );
        let _ = view.update(0.032);
        assert_eq!(
            editor.get_string("Text").as_deref(),
            Some("changed"),
            "editor target should hold the edited value"
        );
        assert_eq!(
            mirror.get_string("Text").as_deref(),
            Some("init"),
            "explicit trigger: source (and OneWay mirror) must NOT change before update_source"
        );

        let expr = editor
            .binding_expression("Text")
            .expect("Editor.Text should have a live BindingExpression");
        expr.update_source();
        let _ = view.update(0.048);
        let _ = view.update(0.064);
        assert_eq!(
            mirror.get_string("Text").as_deref(),
            Some("changed"),
            "after update_source the source should propagate to the OneWay mirror"
        );

        drop(editor);
        drop(mirror);
        drop(content);
        view.deactivate();
        drop(view);
        drop(_guard);
    }

    noesis_runtime::shutdown();
}
