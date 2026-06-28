//! TODO §6 — a `DoubleAnimationUsingKeyFrames` walks a `Width` through linear
//! key frames (0 @ 0s, 100 @ 1s) plus a held discrete tail. We assert the value
//! interpolates between frames (linear midpoint ~50) and reaches the final frame
//! value. A stubbed key-frame animation leaves the value at its base.

use dm_noesis_runtime::animation::{
    Animation, DoubleAnimationUsingKeyFrames, KeyFrameInterp, KeyFrameKind, Storyboard,
};
use dm_noesis_runtime::view::{FrameworkElement, View};

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <Border x:Name="Box" Height="50" Background="Green" Width="0"/>
</Grid>"##;

#[test]
fn keyframe_animation_interpolates_and_reaches_end() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        let element = FrameworkElement::parse(XAML).expect("parse");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        view.update(0.0);

        let content = view.content().expect("content");
        let box_el = content.find_name("Box").expect("Box");

        let mut anim = DoubleAnimationUsingKeyFrames::new();
        // Linear ramp 0 -> 100 across the first second...
        assert!(anim.add_key_frame(KeyFrameKind::Linear, 0.0, 0.0, KeyFrameInterp::None));
        assert!(anim.add_key_frame(KeyFrameKind::Linear, 1.0, 100.0, KeyFrameInterp::None));
        // ...then a discrete frame holds 200 from 1.5s.
        assert!(anim.add_key_frame(KeyFrameKind::Discrete, 1.5, 200.0, KeyFrameInterp::None));
        anim.set_target_name("Box");
        anim.set_target_property("Width");

        let mut sb = Storyboard::new();
        assert!(sb.add_child(&anim));
        assert!(sb.begin(&content, false));
        view.update(0.0);

        let start = box_el.get_f32("Width").expect("width");
        assert!(start < 1.0, "width should start at 0, got {start}");

        // Halfway along the first linear segment: ~50.
        view.update(0.5);
        let mid = box_el.get_f32("Width").expect("width");
        assert!(
            mid > 35.0 && mid < 65.0,
            "linear-keyframe midpoint should be ~50, got {mid}"
        );

        // Just before the discrete frame: at the 100 key value.
        view.update(1.2);
        let at_linear_end = box_el.get_f32("Width").expect("width");
        assert!(
            at_linear_end > 99.0 && at_linear_end < 101.0,
            "should hold the 100 key value before the discrete frame, got {at_linear_end}"
        );

        // After the discrete frame jumps to 200.
        view.update(1.6);
        let end = box_el.get_f32("Width").expect("width");
        assert!(end > 199.0, "discrete frame should reach 200, got {end}");

        drop(box_el);
        drop(content);
    }

    dm_noesis_runtime::shutdown();
}
