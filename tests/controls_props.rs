// TODO §8 — RangeBase / ToggleButton / Expander / Popup / TextBox / PasswordBox
// programmatic property + method round-trips.
//
// One headless `#[test]`. The RangeBase/Toggle/Expander/Popup paths are pure
// property round-trips on parsed control roots (no render pass needed). The
// text controls are hosted in a live `View` (their selection/caret model is
// built during layout) and pumped before asserting:
//
//   * `Slider`: value round-trips, and a value beyond `Maximum`/`Minimum` is
//     COERCED by RangeBase (a strong non-vacuous assertion a DP-only stub that
//     skipped `SetValue` would fail).
//   * `CheckBox`: three-state `IsChecked` — `Some(true)` / `Some(false)` /
//     `None` (indeterminate) all round-trip; the null state is NOT collapsed.
//   * `Expander.IsExpanded` / `Popup.IsOpen` toggle + read back.
//   * `TextBox`: `Select(1,3)` ⇒ start==1, length==3, selected_text=="ell";
//     `set_caret_index` + `select_all` round-trip.
//   * `PasswordBox`: password get/set round-trip.
//   * Negatives: each typed accessor returns `None`/`false` on the wrong type.

use dm_noesis_runtime::view::{FrameworkElement, View};

const NS: &str = r#"xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation""#;

// The text-editing model (caret/selection) lives in the TextView created by the
// templated `PART_ContentHost`. Without a theme there is no default template, so
// we supply a minimal one with the required content-host part so `Select` works.
const TEXT_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="300" Height="200">
  <Grid.Resources>
    <ControlTemplate x:Key="TextHost" TargetType="TextBox">
      <ScrollViewer x:Name="PART_ContentHost"/>
    </ControlTemplate>
    <ControlTemplate x:Key="PasswordHost" TargetType="PasswordBox">
      <ScrollViewer x:Name="PART_ContentHost"/>
    </ControlTemplate>
  </Grid.Resources>
  <StackPanel>
    <TextBox x:Name="TB" Text="Hello World" Width="200" Height="30"
             Template="{StaticResource TextHost}"/>
    <PasswordBox x:Name="PB" Width="200" Height="30"
                 Template="{StaticResource PasswordHost}"/>
    <TextBlock x:Name="TBLOCK" Text="plain"/>
  </StackPanel>
</Grid>"##;

#[test]
fn control_property_round_trips() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        // -- RangeBase (Slider) with coercion -- no view needed --
        let mut slider = FrameworkElement::parse(&format!(
            r#"<Slider {NS} Minimum="0" Maximum="100" Value="10"/>"#
        ))
        .expect("parse Slider");
        assert_eq!(slider.range_minimum(), Some(0.0));
        assert_eq!(slider.range_maximum(), Some(100.0));
        assert_eq!(slider.range_value(), Some(10.0));
        assert!(slider.set_range_value(50.0));
        assert_eq!(slider.range_value(), Some(50.0));
        // Beyond the range -> coerced to the bounds.
        assert!(slider.set_range_value(999.0));
        assert_eq!(
            slider.range_value(),
            Some(100.0),
            "value coerced to Maximum"
        );
        assert!(slider.set_range_value(-5.0));
        assert_eq!(slider.range_value(), Some(0.0), "value coerced to Minimum");
        // Narrow the range, then set a value beyond the new Maximum: the clamp
        // tracks the live bounds (proves SetMaximum took effect + coercion runs).
        assert!(slider.set_range_maximum(40.0));
        assert_eq!(slider.range_maximum(), Some(40.0));
        assert!(slider.set_range_value(80.0));
        assert_eq!(
            slider.range_value(),
            Some(40.0),
            "value clamped to the lowered Maximum"
        );

        // -- ToggleButton three-state -- no view needed --
        let mut cb = FrameworkElement::parse(&format!(r#"<CheckBox {NS} IsThreeState="True"/>"#))
            .expect("parse CheckBox");
        assert!(cb.set_is_checked(Some(true)));
        assert_eq!(cb.is_checked(), Some(Some(true)));
        assert!(cb.set_is_checked(Some(false)));
        assert_eq!(cb.is_checked(), Some(Some(false)));
        assert!(cb.set_is_checked(None));
        assert_eq!(
            cb.is_checked(),
            Some(None),
            "indeterminate state must NOT collapse to Some(false)"
        );

        // -- Expander --
        let mut exp = FrameworkElement::parse(&format!(
            r#"<Expander {NS} IsExpanded="False"><TextBlock Text="body"/></Expander>"#
        ))
        .expect("parse Expander");
        assert_eq!(exp.is_expanded(), Some(false));
        assert!(exp.set_is_expanded(true));
        assert_eq!(exp.is_expanded(), Some(true));

        // -- Popup --
        let mut pop =
            FrameworkElement::parse(&format!(r#"<Popup {NS}><TextBlock Text="body"/></Popup>"#))
                .expect("parse Popup");
        assert_eq!(pop.is_open(), Some(false));
        assert!(pop.set_is_open(true));
        assert_eq!(pop.is_open(), Some(true));
        assert!(pop.set_is_open(false));
        assert_eq!(pop.is_open(), Some(false));

        // -- TextBox / PasswordBox -- hosted in a live View so the text model is
        // built (selection/caret are no-ops on a never-laid-out TextBox).
        let root = FrameworkElement::parse(TEXT_XAML).expect("parse text XAML");
        let mut view = View::create(root);
        view.set_size(300, 200);
        view.activate();
        for i in 1..=6 {
            view.update(f64::from(i) * 0.016);
        }
        let content = view.content().expect("view content");
        let mut tb = content.find_name("TB").expect("find TextBox");
        let mut pb = content.find_name("PB").expect("find PasswordBox");
        let mut tblock = content.find_name("TBLOCK").expect("find TextBlock");

        assert_eq!(tb.text().as_deref(), Some("Hello World"));
        assert!(tb.select(1, 3));
        assert_eq!(tb.selection_start(), Some(1));
        assert_eq!(tb.selection_length(), Some(3));
        assert_eq!(
            tb.selected_text().as_deref(),
            Some("ell"),
            "selected substring of 'Hello World' at [1,3)"
        );
        assert!(tb.set_caret_index(5));
        assert_eq!(tb.caret_index(), Some(5));
        assert!(tb.select_all());
        assert_eq!(
            tb.selection_length(),
            Some(11),
            "select_all spans the whole text"
        );

        assert_eq!(pb.password().as_deref(), Some(""), "empty initially");
        assert!(pb.set_password("s3cr3t!"));
        assert_eq!(pb.password().as_deref(), Some("s3cr3t!"));

        // -- Negatives: wrong control type --
        assert_eq!(cb.range_value(), None, "CheckBox is not a RangeBase");
        assert_eq!(slider.is_checked(), None, "Slider is not a ToggleButton");
        assert_eq!(slider.is_expanded(), None, "Slider is not an Expander");
        assert_eq!(slider.is_open(), None, "Slider is not a Popup");
        assert_eq!(tblock.selection_start(), None, "TextBlock is not a TextBox");
        assert_eq!(tblock.selected_text(), None, "TextBlock is not a TextBox");
        assert_eq!(tb.password(), None, "TextBox is not a PasswordBox");
        assert!(!tblock.set_range_value(1.0), "no-op on a TextBlock");

        drop(tblock);
        drop(pb);
        drop(tb);
        drop(content);
        drop(view);
        drop(pop);
        drop(exp);
        drop(cb);
        drop(slider);
    }

    dm_noesis_runtime::shutdown();
}
