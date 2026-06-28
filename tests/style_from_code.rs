//! Build a `Style` from code, assign it to one element, and verify the setter applied by reading the property back through Noesis; also exercises `BasedOn`.

use std::collections::HashMap;

use noesis_runtime::binding::box_f32;
use noesis_runtime::styles::Style;
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const SCENE: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="200" Height="200">
  <StackPanel>
    <TextBlock x:Name="Styled" Text="A"/>
    <TextBlock x:Name="Plain" Text="B"/>
    <TextBlock x:Name="BasedStyled" Text="C"/>
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

const STYLED_SIZE: f32 = 33.0;
const BASED_SIZE: f32 = 41.0;

#[test]
fn style_setter_applies_to_assigned_element() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), SCENE.as_bytes().to_vec());
        let provider = InMem { bytes };
        let _registered = noesis_runtime::xaml_provider::set_xaml_provider(provider);

        let root = FrameworkElement::load("scene.xaml").expect("scene load");
        let mut view = View::create(root);
        view.set_size(200, 200);
        view.activate();
        view.update(0.0);

        let mut styled = view
            .content()
            .and_then(|c| c.find_name("Styled"))
            .expect("find Styled");
        let plain = view
            .content()
            .and_then(|c| c.find_name("Plain"))
            .expect("find Plain");
        let mut based = view
            .content()
            .and_then(|c| c.find_name("BasedStyled"))
            .expect("find BasedStyled");

        let default_size = plain.get_f32("FontSize").expect("Plain FontSize");
        assert!(
            (styled.get_f32("FontSize").unwrap() - default_size).abs() < 0.001,
            "all blocks start at the default FontSize"
        );
        assert!(
            (default_size - STYLED_SIZE).abs() > 0.001,
            "default must differ from the setter value or the test proves nothing"
        );

        assert!(styled.style().is_none(), "no style before assignment");

        let mut style = Style::new();
        assert!(
            style.set_target_type("TextBlock"),
            "TextBlock must resolve through reflection"
        );
        assert!(
            style.add_setter("FontSize", &box_f32(STYLED_SIZE)),
            "FontSize setter must resolve on TextBlock"
        );
        // Negative: an unknown DP name on the target type fails.
        assert!(
            !style.add_setter("NoSuchProperty", &box_f32(1.0)),
            "unknown DP name must not add a setter"
        );

        assert!(styled.set_style(&style), "set_style on a TextBlock");
        view.update(0.016);

        assert!(
            styled.style().is_some(),
            "GetStyle returns the assigned style"
        );
        assert!(
            (styled.get_f32("FontSize").unwrap() - STYLED_SIZE).abs() < 0.001,
            "styled block FontSize must equal the setter value ({STYLED_SIZE})"
        );
        assert!(
            (plain.get_f32("FontSize").unwrap() - default_size).abs() < 0.001,
            "unstyled sibling keeps the default FontSize"
        );

        let mut derived = Style::new();
        assert!(derived.set_target_type("TextBlock"));
        derived.set_based_on(&style);
        assert!(derived.add_setter("FontSize", &box_f32(BASED_SIZE)));
        assert!(based.set_style(&derived), "set derived style");
        view.update(0.032);
        assert!(
            (based.get_f32("FontSize").unwrap() - BASED_SIZE).abs() < 0.001,
            "derived style's own setter wins over the BasedOn value"
        );
    }

    noesis_runtime::shutdown();
}
