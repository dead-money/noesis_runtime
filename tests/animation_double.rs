//! TODO §6 — a `DoubleAnimation` driven by a `Storyboard` animates a named
//! element's `Opacity` from 0 to 1 over a short duration. We pump the view
//! clock across the duration and read `Opacity` back through Noesis: a stubbed
//! `Begin` / no-op animation leaves it unchanged, so the moving value and the
//! endpoints discriminate a real implementation.
//!
//! Single `#[test]` per file (Noesis can't be re-init'd in a process); all work
//! happens in an inner scope so every owning wrapper drops before `shutdown()`.

use dm_noesis_runtime::animation::{Animation, DoubleAnimation, Storyboard, Timeline};
use dm_noesis_runtime::view::{FrameworkElement, View};

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <Border x:Name="Box" Width="100" Height="100" Background="Red" Opacity="0"/>
</Grid>"##;

#[test]
fn double_animation_drives_opacity() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        let element = FrameworkElement::parse(XAML).expect("parse returned None");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        // Establish the layout + the time-manager baseline at t=0.
        view.update(0.0);

        let content = view.content().expect("content");
        let box_el = content.find_name("Box").expect("find Box");
        let base = box_el.get_f32("Opacity").expect("Opacity is a float DP");
        assert!(
            base.abs() < 1e-3,
            "base opacity should start at 0, got {base}"
        );

        // Opacity 0 -> 1 over 0.5s, linear.
        let mut anim = DoubleAnimation::new();
        anim.set_from(Some(0.0));
        anim.set_to(Some(1.0));
        anim.set_duration_secs(0.5);
        assert_eq!(anim.duration_secs(), Some(0.5));
        assert!(anim.set_target_name("Box"));
        assert!(anim.set_target_property("Opacity"));

        let mut sb = Storyboard::new();
        assert!(sb.add_child(&anim));
        assert_eq!(sb.child_count(), Some(1));
        // The builder handles may be dropped after wiring — Noesis holds its own
        // references through the storyboard's children collection.
        drop(anim);

        assert!(sb.begin(&content, false), "begin failed");

        // Tick the clock to its anchor; value should still be ~From (0).
        view.update(0.0);
        let start = box_el.get_f32("Opacity").expect("opacity");
        assert!(start < 0.05, "opacity near start should be ~0, got {start}");

        // Midway through the 0.5s span, the value should be partway (linear
        // ~0.5, but we only require strictly between the endpoints).
        view.update(0.25);
        let mid = box_el.get_f32("Opacity").expect("opacity");
        assert!(
            mid > start + 0.1 && mid < 0.95,
            "opacity mid should be between endpoints, got {mid}"
        );

        // Past the end: the value should have reached ~To (1.0).
        view.update(0.6);
        let end = box_el.get_f32("Opacity").expect("opacity");
        assert!(end > 0.98, "opacity at end should be ~1, got {end}");

        drop(box_el);
        drop(content);
    }

    dm_noesis_runtime::shutdown();
}
