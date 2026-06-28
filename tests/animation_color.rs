//! `ColorAnimation` in a `Storyboard` animates a code-built `SolidColorBrush`;
//! reading back through the retained brush handle observes the live animated color.

use noesis_runtime::animation::{Animation, ColorAnimation, Storyboard, Timeline};
use noesis_runtime::brushes::SolidColorBrush;
use noesis_runtime::view::{FrameworkElement, View};

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <Border x:Name="Box" Width="100" Height="100"/>
</Grid>"##;

#[test]
fn color_animation_drives_brush_color() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let element = FrameworkElement::parse(XAML).expect("parse");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        view.update(0.0);

        let content = view.content().expect("content");
        let mut box_el = content.find_name("Box").expect("Box");

        // Retained so we can read the live animated color back through Noesis.
        let brush = SolidColorBrush::new([1.0, 0.0, 0.0, 1.0]);
        assert!(box_el.set_background(&brush));
        assert_eq!(brush.color(), [1.0, 0.0, 0.0, 1.0]);

        let mut anim = ColorAnimation::new();
        assert!(
            anim.set_from(Some([1.0, 0.0, 0.0, 1.0])),
            "setter should succeed"
        );
        assert!(
            anim.set_to(Some([0.0, 0.0, 1.0, 1.0])),
            "setter should succeed"
        );
        anim.set_duration_secs(0.5);
        anim.set_target_name("Box");
        anim.set_target_property("(Border.Background).(SolidColorBrush.Color)");

        let mut sb = Storyboard::new();
        assert!(sb.add_child(&anim));
        assert!(sb.begin(&content, false));
        view.update(0.0);

        let start = brush.color();
        assert!(
            start[0] > 0.9 && start[2] < 0.1,
            "color should start red, got {start:?}"
        );

        view.update(0.25);
        let mid = brush.color();
        assert!(
            mid[2] > 0.1 && mid[2] < 0.9 && mid[0] < start[0],
            "color should be partway red->blue, got {mid:?}"
        );

        view.update(0.6);
        let end = brush.color();
        assert!(
            end[2] > 0.95 && end[0] < 0.05,
            "color should end blue, got {end:?}"
        );

        drop(brush);
        drop(box_el);
        drop(content);
    }

    noesis_runtime::shutdown();
}
