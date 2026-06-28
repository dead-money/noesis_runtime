//! Key-frame animation round-trips for Point, Thickness, Boolean, String, Color,
//! and Double (all interpolation kinds), plus `ParallelTimeline` grouping.

use noesis_runtime::animation::{
    Animation, BooleanAnimationUsingKeyFrames, ColorAnimationUsingKeyFrames, DoubleAnimation,
    DoubleAnimationUsingKeyFrames, EasingFunction, EasingKind, EasingMode, KeyFrameInterp,
    KeyFrameKind, KeySpline, ParallelTimeline, PointAnimationUsingKeyFrames, Storyboard,
    StringAnimationUsingKeyFrames, ThicknessAnimationUsingKeyFrames, Timeline,
};
use noesis_runtime::view::{FrameworkElement, View};

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <Border x:Name="Box" Height="50" Background="Green" Width="0"/>
</Grid>"##;

#[test]
fn keyframe_types_round_trip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let spline = KeySpline::new((0.25, 0.1), (0.25, 1.0));
        let easing = EasingFunction::new(EasingKind::Cubic, EasingMode::EaseInOut);

        let mut pointkf = PointAnimationUsingKeyFrames::new();
        assert!(pointkf.add_key_frame(
            KeyFrameKind::Discrete,
            0.0,
            (1.0, 2.0),
            KeyFrameInterp::None
        ));
        assert!(pointkf.add_key_frame(KeyFrameKind::Linear, 0.5, (3.0, 4.0), KeyFrameInterp::None));
        assert!(pointkf.add_key_frame(
            KeyFrameKind::Easing,
            0.75,
            (5.0, 6.0),
            KeyFrameInterp::Easing(&easing),
        ));
        assert!(pointkf.add_key_frame(
            KeyFrameKind::Spline,
            1.0,
            (7.0, 8.0),
            KeyFrameInterp::Spline(&spline),
        ));
        assert_eq!(pointkf.key_frame_count(), Some(4));
        assert_eq!(pointkf.key_frame_value(1), Some((3.0, 4.0)));
        assert_eq!(pointkf.key_frame_value(3), Some((7.0, 8.0)));
        assert_eq!(pointkf.key_frame_time(2), Some(0.75));
        assert_eq!(pointkf.key_frame_value(4), None);

        let mut thickkf = ThicknessAnimationUsingKeyFrames::new();
        assert!(thickkf.add_key_frame(
            KeyFrameKind::Linear,
            0.0,
            [1.0, 2.0, 3.0, 4.0],
            KeyFrameInterp::None,
        ));
        assert!(thickkf.add_key_frame(
            KeyFrameKind::Spline,
            1.0,
            [5.0, 6.0, 7.0, 8.0],
            KeyFrameInterp::Spline(&spline),
        ));
        assert_eq!(thickkf.key_frame_count(), Some(2));
        assert_eq!(thickkf.key_frame_value(0), Some([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(thickkf.key_frame_value(1), Some([5.0, 6.0, 7.0, 8.0]));
        assert_eq!(thickkf.key_frame_time(1), Some(1.0));
        assert_eq!(thickkf.key_frame_value(2), None);

        let mut boolkf = BooleanAnimationUsingKeyFrames::new();
        assert!(boolkf.add_key_frame(0.0, false));
        assert!(boolkf.add_key_frame(1.0, true));
        assert_eq!(boolkf.key_frame_count(), Some(2));
        assert_eq!(boolkf.key_frame_value(0), Some(false));
        assert_eq!(boolkf.key_frame_value(1), Some(true));
        assert_eq!(boolkf.key_frame_time(1), Some(1.0));
        assert_eq!(boolkf.key_frame_value(2), None);

        let mut strkf = StringAnimationUsingKeyFrames::new();
        assert!(strkf.add_key_frame(0.0, "hello"));
        assert!(strkf.add_key_frame(2.0, "world"));
        assert_eq!(strkf.key_frame_count(), Some(2));
        assert_eq!(strkf.key_frame_value(0), Some("hello".to_string()));
        assert_eq!(strkf.key_frame_value(1), Some("world".to_string()));
        assert_eq!(strkf.key_frame_time(1), Some(2.0));
        assert_eq!(strkf.key_frame_value(2), None);

        let mut colorkf = ColorAnimationUsingKeyFrames::new();
        assert!(
            colorkf.add_key_frame(
                KeyFrameKind::Spline,
                1.0,
                [1.0, 0.0, 0.0, 1.0],
                KeyFrameInterp::Spline(&spline),
            ),
            "Color spline key frame should be supported"
        );

        let mut inner = ParallelTimeline::new();
        let inner_child = DoubleAnimation::new();
        assert!(inner.add_child(&inner_child));
        assert_eq!(inner.child_count(), Some(1));

        let mut group = ParallelTimeline::new();
        assert_eq!(group.child_count(), Some(0));
        let group_child = DoubleAnimation::new();
        assert!(group.add_child(&group_child));
        assert!(group.add_child(&inner)); // nest a timeline group inside another
        assert_eq!(group.child_count(), Some(2));
        assert!(group.set_duration_secs(3.0));
        assert_eq!(group.duration_secs(), Some(3.0));

        let mut dspline = DoubleAnimationUsingKeyFrames::new();
        assert!(dspline.add_key_frame(KeyFrameKind::Linear, 0.0, 0.0, KeyFrameInterp::None));
        assert!(dspline.add_key_frame(
            KeyFrameKind::Spline,
            1.0,
            100.0,
            KeyFrameInterp::Spline(&spline),
        ));
        dspline.set_target_name("Box");
        dspline.set_target_property("Width");

        let element = FrameworkElement::parse(XAML).expect("parse");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        view.update(0.0);

        let content = view.content().expect("content");
        let box_el = content.find_name("Box").expect("Box");

        let mut sb = Storyboard::new();
        assert!(sb.add_child(&dspline));
        assert!(sb.begin(&content, false));
        view.update(0.0);

        let start = box_el.get_f32("Width").expect("width");
        assert!(start < 1.0, "spline width should start at 0, got {start}");

        view.update(0.5);
        let mid = box_el.get_f32("Width").expect("width");
        assert!(
            mid > 0.0 && mid < 100.0,
            "spline midpoint should be strictly between the frames, got {mid}"
        );

        // Past the final (1.0s) key frame: holds the 100 end value.
        view.update(1.5);
        let end = box_el.get_f32("Width").expect("width");
        assert!(
            end > 99.0,
            "spline should reach the 100 end value, got {end}"
        );

        drop(box_el);
        drop(content);
    }

    noesis_runtime::shutdown();
}
