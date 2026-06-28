//! `Animation::begin_on` drives a dependency property directly, without a
//! `Storyboard`, using the element's view `TimeManager`.

use noesis_runtime::animation::{Animation, DoubleAnimation, HandoffBehavior, Timeline};
use noesis_runtime::view::{FrameworkElement, View};

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <Border x:Name="Box" Width="100" Height="100" Background="Purple" Opacity="0"/>
</Grid>"##;

#[test]
fn begin_on_drives_property_directly() {
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
        let box_el = content.find_name("Box").expect("Box");

        let mut anim = DoubleAnimation::new();
        assert!(anim.set_from(Some(0.0)), "setter should succeed");
        assert!(anim.set_to(Some(1.0)), "setter should succeed");
        anim.set_duration_secs(0.5);

        assert!(
            !anim.begin_on(
                &box_el,
                "NoSuchProperty",
                HandoffBehavior::SnapshotAndReplace
            ),
            "begin_on with an unknown property should fail"
        );

        assert!(
            anim.begin_on(&box_el, "Opacity", HandoffBehavior::SnapshotAndReplace),
            "begin_on on a connected element should succeed"
        );
        view.update(0.0);
        let start = box_el.get_f32("Opacity").expect("opacity");
        assert!(start < 0.05, "should start ~0, got {start}");

        view.update(0.25);
        let mid = box_el.get_f32("Opacity").expect("opacity");
        assert!(
            mid > 0.1 && mid < 0.95,
            "should be mid-animation, got {mid}"
        );

        view.update(0.6);
        let end = box_el.get_f32("Opacity").expect("opacity");
        assert!(end > 0.98, "should reach ~1, got {end}");

        drop(box_el);
        drop(content);
    }

    noesis_runtime::shutdown();
}
