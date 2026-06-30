//! Spike-Mount: can ONE live `View` host two copies of the SAME sub-XAML, each
//! with its own `DataContext` and its own namescope, mounted MID-FRAME into a
//! shared named panel, without rebuilding the View?
//!
//! This gates the "one panel View, many sub-trees" composition the ECS-UI
//! design wants (a HUD View whose children are independently-bound fragments).
//! The two risks it isolates:
//!
//!   1. `DataContext` isolation: copy A renders its `Title` and copy B renders
//!      its OWN distinct `Title`, even though both were `load`ed from identical
//!      markup.
//!   2. Namescope isolation: both fragments define `x:Name="Leaf"`. Resolving
//!      "Leaf" from fragment A's root must find A's `TextBlock`, and a
//!      `set_and_notify` on A's view model must NOT bleed into B.
//!
//! Both fragments are inserted via `panel_children.add` AFTER the View is built
//! and pumped (mid-frame), so we also prove the host panel realizes injected
//! children without a View rebuild.

use std::collections::HashMap;

use noesis_runtime::element_tree::panel_children;
use noesis_runtime::plain_vm::{PlainType, PlainValue, PlainVmBuilder};
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const HOST_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="200" Height="200">
  <StackPanel x:Name="Host"/>
</Grid>"##;

// Loaded TWICE; each copy gets its own namescope + DataContext.
const LEAF_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Border xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
        xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
        Height="40">
  <TextBlock x:Name="Leaf" Text="{Binding Title}"/>
</Border>"##;

struct InMem(HashMap<String, Vec<u8>>);
impl XamlProvider for InMem {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.0.get(uri).map(Vec::as_slice)
    }
}

#[test]
fn spike_mount_two_copies_isolated() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut bytes = HashMap::new();
        bytes.insert("host.xaml".to_string(), HOST_XAML.as_bytes().to_vec());
        bytes.insert("leaf.xaml".to_string(), LEAF_XAML.as_bytes().to_vec());
        let _guard = noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        // Two plain view models with distinct Title values.
        let mut vm_builder = PlainVmBuilder::new("DmSpike.LeafVm");
        let title = vm_builder.add_property("Title", PlainType::String);
        let vm_class = vm_builder.register().expect("register LeafVm");

        let vm_a = vm_class.create_instance().expect("vm A");
        let vm_b = vm_class.create_instance().expect("vm B");
        assert!(vm_a.set(title, PlainValue::String("AAA".into())));
        assert!(vm_b.set(title, PlainValue::String("BBB".into())));

        // Build + pump the host View FIRST; the scene exists before we mount.
        let host_root = FrameworkElement::load("host.xaml").expect("load host.xaml");
        let mut view = View::create(host_root);
        view.set_size(200, 200);
        view.activate();
        for i in 1..=4 {
            view.update(f64::from(i) * 0.016);
        }

        let content = view.content().expect("view content");
        let host = content.find_name("Host").expect("find Host panel");

        // Two independent copies of the SAME sub-XAML, each its own namescope.
        let mut leaf_a = FrameworkElement::load("leaf.xaml").expect("load leaf A");
        let mut leaf_b = FrameworkElement::load("leaf.xaml").expect("load leaf B");
        assert!(vm_a.set_data_context(&mut leaf_a), "set DC on A");
        assert!(vm_b.set_data_context(&mut leaf_b), "set DC on B");

        // Mount MID-FRAME into the shared panel (no View rebuild).
        let mut children = panel_children(&host).expect("Host is a Panel");
        let ia = children.add(&leaf_a).expect("add A");
        let ib = children.add(&leaf_b).expect("add B");
        assert_eq!((ia, ib), (0, 1), "both fragments mounted into one panel");
        for i in 5..=10 {
            view.update(f64::from(i) * 0.016);
        }

        // Resolve "Leaf" from EACH fragment root: namescope isolation means
        // each resolves to its own TextBlock.
        let leaf_a_tb = leaf_a.find_name("Leaf").expect("A/Leaf");
        let leaf_b_tb = leaf_b.find_name("Leaf").expect("B/Leaf");

        let a_initial = leaf_a_tb.text();
        let b_initial = leaf_b_tb.text();
        eprintln!("SPIKE-MOUNT: initial A={a_initial:?} B={b_initial:?}");
        let distinct_render =
            a_initial.as_deref() == Some("AAA") && b_initial.as_deref() == Some("BBB");

        // Mutate ONLY A and notify. B must be untouched.
        assert!(vm_a.set_and_notify(title, "Title", PlainValue::String("ZZZ".into())));
        for i in 11..=14 {
            view.update(f64::from(i) * 0.016);
        }
        let a_after = leaf_a_tb.text();
        let b_after = leaf_b_tb.text();
        eprintln!("SPIKE-MOUNT: after A.notify(ZZZ) A={a_after:?} B={b_after:?}");

        let a_updated = a_after.as_deref() == Some("ZZZ");
        let b_isolated = b_after.as_deref() == Some("BBB");
        let mount_isolation_works = distinct_render && a_updated && b_isolated;

        eprintln!(
            "SPIKE-MOUNT: distinct_render={distinct_render} a_updated={a_updated} \
             b_isolated={b_isolated}; mountIsolationWorks={mount_isolation_works}"
        );

        assert!(
            mount_isolation_works,
            "mount isolation failed: distinct={distinct_render} a_updated={a_updated} \
             b_isolated={b_isolated}"
        );

        // Teardown: clear the panel (releases child refs) before fragments /
        // view models drop, then tear down the view.
        assert!(children.clear());
        drop(children);
        drop(leaf_a_tb);
        drop(leaf_b_tb);
        drop(host);
        drop(leaf_a);
        drop(leaf_b);
        drop(content);
        view.deactivate();
        drop(view);
        drop(vm_a);
        drop(vm_b);
        drop(vm_class);
    }

    noesis_runtime::shutdown();
}
