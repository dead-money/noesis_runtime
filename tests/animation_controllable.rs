//! TODO §6 — the controllable `Storyboard` actions (Pause / Resume / Stop) have
//! observable effects. We begin an `Opacity` 0->1 animation as controllable,
//! then: pause and confirm the value holds across further ticks; resume and
//! confirm it advances again; stop and confirm the value reverts to its base.
//! Each step reads `Opacity` back through Noesis, so a stubbed Pause/Stop (which
//! would let the value keep climbing to 1) fails the assertions.

use dm_noesis_runtime::animation::{Animation, DoubleAnimation, Storyboard, Timeline};
use dm_noesis_runtime::view::{FrameworkElement, View};

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <Border x:Name="Box" Width="100" Height="100" Background="Orange" Opacity="0"/>
</Grid>"##;

#[test]
fn controllable_storyboard_pause_resume_stop() {
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

        let mut anim = DoubleAnimation::new();
        anim.set_from(Some(0.0));
        anim.set_to(Some(1.0));
        anim.set_duration_secs(1.0);
        anim.set_target_name("Box");
        anim.set_target_property("Opacity");

        let mut sb = Storyboard::new();
        assert!(sb.add_child(&anim));
        // Must be controllable for Pause/Resume/Stop to do anything.
        assert!(sb.begin(&content, true));
        view.update(0.0);

        // Advance to ~0.3.
        view.update(0.3);
        let running = box_el.get_f32("Opacity").expect("opacity");
        assert!(running > 0.1, "should be running, got {running}");
        assert!(sb.is_playing(&content), "should report playing");
        assert!(!sb.is_paused(&content), "should not report paused yet");

        // Pause: the value must hold across further ticks.
        assert!(sb.pause(&content));
        assert!(sb.is_paused(&content), "should report paused");
        view.update(0.6);
        let held = box_el.get_f32("Opacity").expect("opacity");
        assert!(
            (held - running).abs() < 0.05,
            "paused value should hold ~{running}, got {held}"
        );

        // Resume: the value advances again past where it was held.
        assert!(sb.resume(&content));
        view.update(0.8);
        let resumed = box_el.get_f32("Opacity").expect("opacity");
        assert!(
            resumed > held + 0.05,
            "resumed value {resumed} should exceed held {held}"
        );

        // Stop: the animation is removed and the property reverts to its base.
        assert!(sb.stop(&content));
        assert!(
            !sb.is_playing(&content),
            "should not report playing after stop"
        );
        view.update(1.0);
        let stopped = box_el.get_f32("Opacity").expect("opacity");
        assert!(
            stopped < 0.05,
            "after Stop the value should revert to base ~0, got {stopped}"
        );

        drop(box_el);
        drop(content);
    }

    dm_noesis_runtime::shutdown();
}
