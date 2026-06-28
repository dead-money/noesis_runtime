//! TODO §6 — easing functions change the interpolation curve. Two `Border`s
//! animate `Width` 0 -> 100 over the same 1s span via one `Storyboard`; one
//! linear, one with a `QuadraticEase` in `EaseIn` mode. At the midpoint the
//! eased value must trail the linear one (quadratic-in at t=0.5 yields ~25, vs
//! linear ~50), and both must reach ~100 at the end. A stubbed easing (treated
//! as linear) would make the two equal at the midpoint, so this discriminates.

use noesis_runtime::animation::{
    Animation, DoubleAnimation, EasingFunction, EasingKind, EasingMode, Storyboard, Timeline,
};
use noesis_runtime::view::{FrameworkElement, View};

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<StackPanel xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="400" Height="200">
  <Border x:Name="Lin" Height="50" Background="Red" Width="0"/>
  <Border x:Name="Eased" Height="50" Background="Blue" Width="0"/>
</StackPanel>"##;

#[test]
fn easing_changes_interpolation_curve() {
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
        view.set_size(400, 200);
        view.activate();
        view.update(0.0);

        let content = view.content().expect("content");
        let lin = content.find_name("Lin").expect("Lin");
        let eased = content.find_name("Eased").expect("Eased");

        let mut a_lin = DoubleAnimation::new();
        assert!(a_lin.set_from(Some(0.0)), "setter should succeed");
        assert!(a_lin.set_to(Some(100.0)), "setter should succeed");
        a_lin.set_duration_secs(1.0);
        a_lin.set_target_name("Lin");
        a_lin.set_target_property("Width");

        let mut a_eased = DoubleAnimation::new();
        assert!(a_eased.set_from(Some(0.0)), "setter should succeed");
        assert!(a_eased.set_to(Some(100.0)), "setter should succeed");
        a_eased.set_duration_secs(1.0);
        a_eased.set_target_name("Eased");
        a_eased.set_target_property("Width");
        let ease = EasingFunction::new(EasingKind::Quadratic, EasingMode::EaseIn);
        assert!(a_eased.set_easing(&ease));

        let mut sb = Storyboard::new();
        assert!(sb.add_child(&a_lin));
        assert!(sb.add_child(&a_eased));
        assert_eq!(sb.child_count(), Some(2));

        assert!(sb.begin(&content, false));
        view.update(0.0);

        // Midpoint: eased (quadratic-in) lags linear.
        view.update(0.5);
        let lw = lin.get_f32("Width").expect("lin width");
        let ew = eased.get_f32("Width").expect("eased width");
        assert!(
            lw > 30.0,
            "linear midpoint should be well advanced, got {lw}"
        );
        assert!(
            ew + 10.0 < lw,
            "eased (quadratic-in) midpoint {ew} should clearly trail linear {lw}"
        );

        // End: both reach ~100.
        view.update(1.1);
        let lw_end = lin.get_f32("Width").expect("lin width");
        let ew_end = eased.get_f32("Width").expect("eased width");
        assert!(lw_end > 99.0, "linear end ~100, got {lw_end}");
        assert!(ew_end > 99.0, "eased end ~100, got {ew_end}");

        drop(ease);
        drop(lin);
        drop(eased);
        drop(content);
    }

    noesis_runtime::shutdown();
}
