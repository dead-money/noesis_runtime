//! TODO §6 — the additional animation value types (`Rect` / `Size` / `Int16` /
//! `Int32` / `Int64` From-To + their `*UsingKeyFrames`, plus the discrete
//! `Object` / `Matrix` key-frame animations), `KeySpline` spline key frames, and
//! the `BeginStoryboard` trigger action.
//!
//! Each assertion is a read-back round-trip: a value is set on (or a key frame
//! added to) the live Noesis object, then re-read through a getter that queries
//! that same object. A stubbed / no-op / hardcoded implementation leaves the
//! getter returning the wrong value (or `None`) and fails the test.
//!
//! Single `#[test]` per file (Noesis can't be re-init'd in a process); all work
//! happens in an inner scope so every owning wrapper drops before `shutdown()`.

use noesis_runtime::animation::{
    Animation, AsComponent, BeginStoryboard, DoubleAnimation, HandoffBehavior, Int16Animation,
    Int16AnimationUsingKeyFrames, Int32Animation, Int32AnimationUsingKeyFrames, Int64Animation,
    Int64AnimationUsingKeyFrames, KeyFrameInterp, KeyFrameKind, KeySpline,
    MatrixAnimationUsingKeyFrames, ObjectAnimationUsingKeyFrames, RectAnimation,
    RectAnimationUsingKeyFrames, SizeAnimation, SizeAnimationUsingKeyFrames, Storyboard, Timeline,
};
use noesis_runtime::view::{FrameworkElement, View};

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <Border x:Name="Box" Width="100" Height="100" Background="Red" Opacity="0"/>
</Grid>"##;

#[test]
fn animation_value_types_round_trip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        // ── Rect From/To/By ──────────────────────────────────────────────────
        let mut rect = RectAnimation::new();
        assert!(rect.set_from(Some([1.0, 2.0, 3.0, 4.0])));
        assert!(rect.set_to(Some([5.0, 6.0, 7.0, 8.0])));
        assert!(rect.set_by(Some([0.5, 0.5, 0.5, 0.5])));
        assert_eq!(rect.from(), Some([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(rect.to(), Some([5.0, 6.0, 7.0, 8.0]));
        assert_eq!(rect.by(), Some([0.5, 0.5, 0.5, 0.5]));
        assert!(rect.set_from(None));
        assert_eq!(rect.from(), None);

        // ── Size From/To/By ──────────────────────────────────────────────────
        let mut size = SizeAnimation::new();
        assert!(size.set_from(Some([10.0, 20.0])));
        assert!(size.set_to(Some([30.0, 40.0])));
        assert_eq!(size.from(), Some([10.0, 20.0]));
        assert_eq!(size.to(), Some([30.0, 40.0]));
        assert_eq!(size.by(), None);

        // ── Int16 / Int32 / Int64 From/To ────────────────────────────────────
        let mut i16a = Int16Animation::new();
        assert!(i16a.set_from(Some(-7)));
        assert!(i16a.set_to(Some(123)));
        assert_eq!(i16a.from(), Some(-7));
        assert_eq!(i16a.to(), Some(123));

        let mut i32a = Int32Animation::new();
        assert!(i32a.set_from(Some(-100_000)));
        assert!(i32a.set_to(Some(250_000)));
        assert!(i32a.set_by(Some(5)));
        assert_eq!(i32a.from(), Some(-100_000));
        assert_eq!(i32a.to(), Some(250_000));
        assert_eq!(i32a.by(), Some(5));

        let mut i64a = Int64Animation::new();
        assert!(i64a.set_from(Some(-5_000_000_000)));
        assert!(i64a.set_to(Some(9_000_000_000)));
        assert_eq!(i64a.from(), Some(-5_000_000_000));
        assert_eq!(i64a.to(), Some(9_000_000_000));

        // ── KeySpline (used below for spline key frames) ─────────────────────
        let mut spline = KeySpline::new((0.25, 0.1), (0.25, 1.0));
        assert_eq!(spline.control_point1(), Some((0.25, 0.1)));
        assert_eq!(spline.control_point2(), Some((0.25, 1.0)));
        assert!(spline.set_control_point1(0.4, 0.2));
        assert_eq!(spline.control_point1(), Some((0.4, 0.2)));
        assert!(spline.set_control_point2(0.6, 0.8));
        assert_eq!(spline.control_point2(), Some((0.6, 0.8)));

        let easing = noesis_runtime::animation::EasingFunction::new(
            noesis_runtime::animation::EasingKind::Cubic,
            noesis_runtime::animation::EasingMode::EaseInOut,
        );

        // ── Rect key frames (Discrete / Linear / Easing / Spline) ────────────
        let mut rectkf = RectAnimationUsingKeyFrames::new();
        assert!(rectkf.add_key_frame(
            KeyFrameKind::Discrete,
            0.0,
            [0.0, 0.0, 1.0, 1.0],
            KeyFrameInterp::None,
        ));
        assert!(rectkf.add_key_frame(
            KeyFrameKind::Linear,
            0.5,
            [1.0, 1.0, 2.0, 2.0],
            KeyFrameInterp::None,
        ));
        assert!(rectkf.add_key_frame(
            KeyFrameKind::Easing,
            0.75,
            [2.0, 2.0, 3.0, 3.0],
            KeyFrameInterp::Easing(&easing),
        ));
        assert!(rectkf.add_key_frame(
            KeyFrameKind::Spline,
            1.0,
            [3.0, 3.0, 4.0, 4.0],
            KeyFrameInterp::Spline(&spline),
        ));
        assert_eq!(rectkf.key_frame_count(), Some(4));
        assert_eq!(rectkf.key_frame_value(1), Some([1.0, 1.0, 2.0, 2.0]));
        assert_eq!(rectkf.key_frame_value(3), Some([3.0, 3.0, 4.0, 4.0]));
        assert_eq!(rectkf.key_frame_time(2), Some(0.75));
        assert_eq!(rectkf.key_frame_value(4), None);

        // ── Size key frames ──────────────────────────────────────────────────
        let mut sizekf = SizeAnimationUsingKeyFrames::new();
        assert!(sizekf.add_key_frame(
            KeyFrameKind::Discrete,
            0.0,
            [1.0, 1.0],
            KeyFrameInterp::None
        ));
        assert!(sizekf.add_key_frame(
            KeyFrameKind::Spline,
            1.0,
            [5.0, 6.0],
            KeyFrameInterp::Spline(&spline),
        ));
        assert_eq!(sizekf.key_frame_count(), Some(2));
        assert_eq!(sizekf.key_frame_value(1), Some([5.0, 6.0]));
        assert_eq!(sizekf.key_frame_time(1), Some(1.0));

        // ── Int key frames ───────────────────────────────────────────────────
        let mut i16kf = Int16AnimationUsingKeyFrames::new();
        assert!(i16kf.add_key_frame(KeyFrameKind::Discrete, 0.0, -3, KeyFrameInterp::None));
        assert!(i16kf.add_key_frame(KeyFrameKind::Linear, 1.0, 42, KeyFrameInterp::None));
        assert_eq!(i16kf.key_frame_count(), Some(2));
        assert_eq!(i16kf.key_frame_value(1), Some(42));
        assert_eq!(i16kf.key_frame_time(1), Some(1.0));

        let mut i32kf = Int32AnimationUsingKeyFrames::new();
        assert!(i32kf.add_key_frame(
            KeyFrameKind::Easing,
            0.5,
            777,
            KeyFrameInterp::Easing(&easing),
        ));
        assert_eq!(i32kf.key_frame_count(), Some(1));
        assert_eq!(i32kf.key_frame_value(0), Some(777));
        assert_eq!(i32kf.key_frame_time(0), Some(0.5));

        let mut i64kf = Int64AnimationUsingKeyFrames::new();
        assert!(i64kf.add_key_frame(
            KeyFrameKind::Spline,
            2.0,
            8_000_000_000,
            KeyFrameInterp::Spline(&spline),
        ));
        assert_eq!(i64kf.key_frame_value(0), Some(8_000_000_000));
        assert_eq!(i64kf.key_frame_time(0), Some(2.0));

        // ── Object key frames (discrete only; value is a real component) ─────
        let value_obj = DoubleAnimation::new();
        let mut objkf = ObjectAnimationUsingKeyFrames::new();
        assert!(objkf.add_key_frame(0.25, &value_obj));
        assert_eq!(objkf.key_frame_count(), Some(1));
        let read = objkf.key_frame_value(0).expect("object key frame value");
        assert_eq!(
            read.component_raw(),
            value_obj.raw(),
            "object key frame value should be the same component that crossed"
        );
        assert_eq!(objkf.key_frame_time(0), Some(0.25));

        // ── Matrix key frames (discrete only) ────────────────────────────────
        let mut matkf = MatrixAnimationUsingKeyFrames::new();
        assert!(matkf.add_key_frame(0.0, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]));
        assert!(matkf.add_key_frame(1.0, [2.0, 0.0, 0.0, 2.0, 5.0, 7.0]));
        assert_eq!(matkf.key_frame_count(), Some(2));
        assert_eq!(
            matkf.key_frame_value(1),
            Some([2.0, 0.0, 0.0, 2.0, 5.0, 7.0])
        );
        assert_eq!(matkf.key_frame_time(1), Some(1.0));

        // ── BeginStoryboard trigger action ───────────────────────────────────
        let mut begin = BeginStoryboard::new();
        assert!(!begin.has_storyboard());
        assert!(begin.set_handoff(HandoffBehavior::Compose));
        assert_eq!(begin.handoff(), Some(HandoffBehavior::Compose));
        assert!(begin.set_handoff(HandoffBehavior::SnapshotAndReplace));
        assert_eq!(begin.handoff(), Some(HandoffBehavior::SnapshotAndReplace));
        assert!(begin.set_name("MyBeginSb"));
        assert_eq!(begin.name(), Some("MyBeginSb".to_string()));

        let sb = Storyboard::new();
        assert!(begin.set_storyboard(&sb));
        assert!(begin.has_storyboard());

        // ── Storyboard.Begin with an explicit HandoffBehavior, driven live ───
        // Proves the new HandoffBehavior begin path actually starts the clocks:
        // a stubbed begin would leave Opacity at 0.
        let element = FrameworkElement::parse(XAML).expect("parse returned None");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        view.update(0.0);

        let content = view.content().expect("content");
        let box_el = content.find_name("Box").expect("find Box");
        assert!(box_el.get_f32("Opacity").expect("Opacity").abs() < 1e-3);

        let mut anim = DoubleAnimation::new();
        assert!(anim.set_from(Some(0.0)), "setter should succeed");
        assert!(anim.set_to(Some(1.0)), "setter should succeed");
        anim.set_duration_secs(0.5);
        assert!(anim.set_target_name("Box"));
        assert!(anim.set_target_property("Opacity"));

        let mut handoff_sb = Storyboard::new();
        assert!(handoff_sb.add_child(&anim));
        drop(anim);
        assert!(handoff_sb.begin_with_handoff(&box_el, HandoffBehavior::Compose, true));

        view.update(0.25);
        let mid = box_el.get_f32("Opacity").expect("Opacity");
        assert!(
            mid > 0.1,
            "opacity should have advanced under begin_with_handoff, got {mid}"
        );
    }

    noesis_runtime::shutdown();
}
