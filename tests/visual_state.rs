//! Integration test for `FrameworkElement::go_to_state`.
//!
//! The template is declared inline rather than relying on a loaded theme so
//! the valid-state assertions hold regardless of whether a default Noesis
//! style dictionary is available.

use std::collections::HashMap;

use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const SCENE_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="400" Height="200">
  <Button x:Name="Btn" Width="120" Height="40">
    <Button.Template>
      <ControlTemplate TargetType="{x:Type Button}">
        <Border x:Name="Root" Background="Gray">
          <VisualStateManager.VisualStateGroups>
            <VisualStateGroup x:Name="CommonStates">
              <VisualState x:Name="Normal"/>
              <VisualState x:Name="MouseOver"/>
              <VisualState x:Name="Pressed"/>
              <VisualState x:Name="Disabled"/>
            </VisualStateGroup>
          </VisualStateManager.VisualStateGroups>
          <ContentPresenter/>
        </Border>
      </ControlTemplate>
    </Button.Template>
  </Button>
  <TextBlock x:Name="Label" Text="hi" VerticalAlignment="Bottom"/>
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

#[test]
fn visual_state_go_to_state() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), SCENE_XAML.as_bytes().to_vec());
        let provider = InMem { bytes };
        let _registered = noesis_runtime::xaml_provider::set_xaml_provider(provider);

        let element =
            FrameworkElement::load("scene.xaml").expect("load_xaml returned None for scene.xaml");
        let mut view = View::create(element);
        view.set_size(400, 200);
        // Layout pass required before VisualStateGroups become resolvable by GoToState.
        view.update(0.0);

        let content = view.content().expect("View::content returned None");
        let button = content.find_name("Btn").expect("Btn not found in scene");
        let label = content
            .find_name("Label")
            .expect("Label not found in scene");

        assert!(
            button.go_to_state("Normal", false),
            "GoToState(Normal) should succeed on the templated Button",
        );
        assert!(
            button.go_to_state("Pressed", true),
            "GoToState(Pressed, useTransitions) should succeed",
        );
        assert!(
            button.go_to_state("MouseOver", false),
            "GoToState(MouseOver) should succeed",
        );

        assert!(
            !button.go_to_state("NotARealState", false),
            "GoToState with an unknown state name should return false",
        );

        assert!(
            !label.go_to_state("Normal", false),
            "GoToState on a TextBlock (no template/states) should return false",
        );

        assert!(
            !content.go_to_state("Normal", false),
            "GoToState on the root Grid should return false",
        );

        drop(label);
        drop(button);
        drop(content);
        drop(view);
    }

    noesis_runtime::shutdown();
}
