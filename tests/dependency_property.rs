//! Name-keyed `DependencyProperty` get/set round-trips on `FrameworkElement`:
//! float (Width, Opacity), string (Text), component read-back (Background),
//! and graceful failure on unknown name, type mismatch, and read-only property.

use std::collections::HashMap;

use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const SCENE_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="400" Height="200">
  <TextBlock x:Name="Label"
             Text="initial" Foreground="White"
             Width="120" Opacity="0.5"
             VerticalAlignment="Top" HorizontalAlignment="Left"/>
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
fn dependency_property_round_trip() {
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

        let content = view.content().expect("View::content returned None");
        let mut label = content
            .find_name("Label")
            .expect("Label not found in scene");

        assert_eq!(
            label.get_f32("Width"),
            Some(120.0),
            "initial Width mismatch",
        );
        assert!(label.set_f32("Width", 256.0), "set_f32(Width) failed");
        assert_eq!(label.get_f32("Width"), Some(256.0), "Width didn't update",);

        assert_eq!(
            label.get_f32("Opacity"),
            Some(0.5),
            "initial Opacity mismatch",
        );
        assert!(label.set_f32("Opacity", 1.0), "set_f32(Opacity) failed");
        assert_eq!(label.get_f32("Opacity"), Some(1.0), "Opacity didn't update");

        assert_eq!(
            label.get_string("Text").as_deref(),
            Some("initial"),
            "initial Text mismatch",
        );
        assert!(
            label.set_string("Text", "updated"),
            "set_string(Text) failed"
        );
        assert_eq!(
            label.get_string("Text").as_deref(),
            Some("updated"),
            "Text didn't update",
        );

        assert!(
            content.get_component("Background").is_some(),
            "Background should resolve to a non-null Brush",
        );

        assert_eq!(
            label.get_f32("NotARealProperty"),
            None,
            "unknown property get should be None",
        );
        assert!(
            !label.set_f32("NotARealProperty", 1.0),
            "unknown property set should be false",
        );

        assert_eq!(
            label.get_string("Width"),
            None,
            "type-mismatched get should be None",
        );
        assert!(
            !label.set_string("Width", "nope"),
            "type-mismatched set should be false",
        );

        assert!(
            !label.set_f32("ActualWidth", 99.0),
            "set on read-only ActualWidth should be false",
        );

        drop(label);
        drop(content);
        drop(view);
    }

    noesis_runtime::shutdown();
}
