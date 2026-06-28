//! Layout participation for a Rust-backed `Panel`.
//!
//! Registers a `Panel` whose `MeasureOverride` returns a fixed size (150×80)
//! and whose `ArrangeOverride` stacks children vertically. After a real layout
//! pass it asserts `ActualWidth`/`ActualHeight` equal the Rust measure result
//! (stubbing the trampoline would yield 0), children were arranged by Rust, and
//! both callbacks fired.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use noesis_runtime::classes::{
    ClassBuilder, Instance, LayoutHandler, PropertyChangeHandler, PropertyValue, Size,
};
use noesis_runtime::ffi::ClassBase;
use noesis_runtime::view::{FrameworkElement, View};

struct NoopChange;
impl PropertyChangeHandler for NoopChange {
    fn on_changed(&self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

#[derive(Clone, Default)]
struct Counters {
    measures: Arc<AtomicU32>,
    arranges: Arc<AtomicU32>,
}

struct StackLayout {
    counters: Counters,
}
impl LayoutHandler for StackLayout {
    fn measure(&self, instance: Instance, _available: Size) -> Size {
        self.counters.measures.fetch_add(1, Ordering::SeqCst);
        // Measure every child with unbounded space so DesiredSize is known.
        let n = instance.layout_child_count();
        for i in 0..n {
            if let Some(child) = instance.layout_child(i) {
                child.measure(Size::new(f32::INFINITY, f32::INFINITY));
            }
        }
        // Deliberately fixed — discriminates a stubbed trampoline.
        Size::new(150.0, 80.0)
    }

    fn arrange(&self, instance: Instance, final_size: Size) -> Size {
        self.counters.arranges.fetch_add(1, Ordering::SeqCst);
        let mut y = 0.0f32;
        let n = instance.layout_child_count();
        for i in 0..n {
            if let Some(child) = instance.layout_child(i) {
                let d = child.desired_size().unwrap_or(Size::ZERO);
                child.arrange(0.0, y, d.width, d.height);
                y += d.height;
            }
        }
        final_size
    }
}

const XAML: &str = r##"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:l="clr-namespace:Lay" Width="400" Height="300">
  <l:MyStack x:Name="P" HorizontalAlignment="Left" VerticalAlignment="Top">
    <Rectangle x:Name="R1" Width="20" Height="20"/>
    <Rectangle x:Name="R2" Width="30" Height="40"/>
  </l:MyStack>
</Grid>"##;

#[test]
fn custom_panel_layout() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let counters = Counters::default();

        let mut b = ClassBuilder::new("Lay.MyStack", ClassBase::Panel, NoopChange);
        b.set_layout(StackLayout {
            counters: counters.clone(),
        });
        let reg = b.register().expect("MyStack registration failed");

        let root = FrameworkElement::parse(XAML).expect("parse XAML");
        let mut view = View::create(root);
        view.set_size(400, 300);
        view.activate();
        assert!(view.update(0.0));

        let content = view.content().expect("content");
        let panel = content.find_name("P").expect("find P");

        assert_eq!(
            panel.actual_width(),
            Some(150.0),
            "panel width != Rust measure result"
        );
        assert_eq!(
            panel.actual_height(),
            Some(80.0),
            "panel height != Rust measure result"
        );

        let r1 = content.find_name("R1").expect("find R1");
        let r2 = content.find_name("R2").expect("find R2");
        assert_eq!(r1.actual_width(), Some(20.0), "R1 not arranged");
        assert_eq!(r2.actual_width(), Some(30.0), "R2 not arranged");

        assert!(
            counters.measures.load(Ordering::SeqCst) > 0,
            "MeasureOverride trampoline never fired"
        );
        assert!(
            counters.arranges.load(Ordering::SeqCst) > 0,
            "ArrangeOverride trampoline never fired"
        );

        drop(r1);
        drop(r2);
        drop(panel);
        drop(content);
        view.deactivate();
        drop(view);
        drop(reg);
    }

    noesis_runtime::shutdown();
}
