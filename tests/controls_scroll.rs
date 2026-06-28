// TODO §8 — ScrollViewer read metrics + ScrollTo* methods.
//
// One headless `#[test]`. A `ScrollViewer` with content taller than its
// viewport is driven inside a live `View` so layout computes the scroll
// extent. Then:
//
//   * `scrollable_height()` is positive (~content - viewport) — proves the
//     scroll info is real, not a stubbed 0.
//   * `scroll_to_vertical_offset(50)` + pump ⇒ `vertical_offset()` reaches ~50.
//   * `scroll_to_end()` ⇒ offset reaches the bottom (== scrollable_height);
//     `scroll_to_home()` ⇒ offset returns to 0.
//   * Negative: a non-ScrollViewer reports `vertical_offset()==None`.

use dm_noesis_runtime::view::{FrameworkElement, View};

// Without a theme there is no default ScrollViewer template, so the
// `PART_ScrollContentPresenter` (which provides the IScrollInfo extent/viewport)
// is absent and scrolling is inert. Supply a minimal template with that part.
const SV: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<ScrollViewer xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
              xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
              Width="100" Height="100"
              VerticalScrollBarVisibility="Visible"
              HorizontalScrollBarVisibility="Visible">
  <ScrollViewer.Template>
    <ControlTemplate TargetType="ScrollViewer">
      <ScrollContentPresenter x:Name="PART_ScrollContentPresenter"
                              Content="{TemplateBinding Content}"
                              CanContentScroll="{TemplateBinding CanContentScroll}"/>
    </ControlTemplate>
  </ScrollViewer.Template>
  <Border Height="600" Width="600" Background="#FF00AA00"/>
</ScrollViewer>"##;

fn pump(view: &mut View, range: std::ops::RangeInclusive<u32>) {
    for i in range {
        view.update(f64::from(i) * 0.016);
    }
}

#[test]
fn scrollviewer_offsets_and_methods() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        let root = FrameworkElement::parse(SV).expect("parse ScrollViewer");
        let mut view = View::create(root);
        view.set_size(100, 100);
        view.activate();
        pump(&mut view, 1..=10);

        let mut sv = view.content().expect("view content");

        let scrollable = sv.scrollable_height().expect("scrollable_height");
        assert!(
            scrollable > 100.0,
            "content (600) taller than viewport (100) should be scrollable, got {scrollable}"
        );
        // Start at the top.
        assert_eq!(sv.vertical_offset(), Some(0.0));

        // Scroll to a mid offset; the command applies on the next layout pass.
        assert!(sv.scroll_to_vertical_offset(50.0));
        pump(&mut view, 11..=20);
        let off = sv.vertical_offset().expect("vertical_offset");
        assert!(
            (off - 50.0).abs() < 1.0,
            "vertical_offset should reach ~50, got {off}"
        );

        // Scroll to the very bottom.
        assert!(sv.scroll_to_end());
        pump(&mut view, 21..=30);
        let bottom = sv.vertical_offset().expect("vertical_offset");
        assert!(
            (bottom - scrollable).abs() < 1.0,
            "scroll_to_end should reach scrollable_height {scrollable}, got {bottom}"
        );

        // Back to the top.
        assert!(sv.scroll_to_home());
        pump(&mut view, 31..=40);
        assert!(
            sv.vertical_offset().expect("vertical_offset") < 1.0,
            "scroll_to_home returns to the top"
        );

        // -- Horizontal axis (content 600 wide vs 100 viewport) --
        let scrollable_w = sv.scrollable_width().expect("scrollable_width");
        assert!(
            scrollable_w > 100.0,
            "content (600) wider than viewport (100) should be horizontally scrollable, got {scrollable_w}"
        );
        assert_eq!(sv.horizontal_offset(), Some(0.0));
        assert!(sv.scroll_to_horizontal_offset(40.0));
        pump(&mut view, 41..=50);
        let hoff = sv.horizontal_offset().expect("horizontal_offset");
        assert!(
            (hoff - 40.0).abs() < 1.0,
            "horizontal_offset should reach ~40, got {hoff}"
        );

        // Negative: the root child Border is not a ScrollViewer.
        let child = sv.visual_child(0).expect("scrollviewer has a visual child");
        assert_eq!(
            child.vertical_offset(),
            None,
            "a non-ScrollViewer reports no offset"
        );
        assert_eq!(
            child.horizontal_offset(),
            None,
            "a non-ScrollViewer reports no horizontal offset"
        );

        drop(child);
        drop(sv);
        drop(view);
    }

    dm_noesis_runtime::shutdown();
}
