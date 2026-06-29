//! `ObservableCollection` CRUD and `INotifyCollectionChanged` end-to-end: Rust
//! mutations propagate to a bound `ItemsControl`'s realized container count.

use std::collections::HashMap;

use noesis_runtime::binding::ObservableCollection;
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const LIST_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<ItemsControl xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
              xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
              x:Name="List" Width="200" Height="300">
  <ItemsControl.Template>
    <ControlTemplate TargetType="ItemsControl">
      <ItemsPresenter/>
    </ControlTemplate>
  </ItemsControl.Template>
  <ItemsControl.ItemTemplate>
    <DataTemplate>
      <TextBlock Text="{Binding}" Height="20"/>
    </DataTemplate>
  </ItemsControl.ItemTemplate>
</ItemsControl>"##;

struct InMem(HashMap<String, Vec<u8>>);
impl XamlProvider for InMem {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.0.get(uri).map(Vec::as_slice)
    }
}

// Single `#[test]` per file: Noesis's Init/Shutdown can't repeat within a
// process, so the CRUD checks and the bound-control checks share one lifecycle.
#[test]
fn observable_collection() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut coll = ObservableCollection::new();
        assert!(coll.is_empty());
        assert_eq!(coll.push_string("a"), Some(0));
        assert_eq!(coll.push_string("b"), Some(1));
        assert_eq!(coll.push_string("c"), Some(2));
        assert_eq!(coll.len(), 3);
        assert!(coll.get(2).is_some());
        assert!(coll.get(3).is_none());

        let mid = noesis_runtime::binding::box_string("z");
        // SAFETY: `mid` is a live boxed value for the duration of the call.
        assert!(unsafe { coll.insert_component(0, mid.raw()) });
        assert_eq!(coll.len(), 4);

        assert!(coll.remove_at(0));
        assert_eq!(coll.len(), 3);
        assert!(!coll.remove_at(99));

        coll.clear();
        assert!(coll.is_empty());

        // Typed items round-trip alongside strings (purely additive surface).
        assert_eq!(coll.push_string("s"), Some(0));
        assert_eq!(coll.push_bool(true), Some(1));
        assert_eq!(coll.push_i32(7), Some(2));
        assert_eq!(coll.push_f64(2.5), Some(3));
        assert_eq!(coll.len(), 4);

        coll.clear();
        assert!(coll.is_empty());
    }

    {
        let mut coll = ObservableCollection::new();
        coll.push_string("Alpha");
        coll.push_string("Beta");

        let mut bytes = HashMap::new();
        bytes.insert("list.xaml".to_string(), LIST_XAML.as_bytes().to_vec());
        let _guard = noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let element = FrameworkElement::load("list.xaml").expect("load_xaml returned None");
        let mut view = View::create(element);
        view.set_size(200, 300);
        view.activate();

        let mut content = view.content().expect("View::content returned None");

        // SAFETY: coll outlives this scope; raw() is a live ObservableCollection*.
        assert!(
            content.set_items_source(&coll),
            "set_items_source returned false (root not an ItemsControl?)"
        );

        assert!(view.update(0.0));
        assert_eq!(
            content.items_count(),
            Some(2),
            "ItemsControl did not see the 2 bound items"
        );
        assert_eq!(
            content.realized_item_count(),
            Some(2),
            "ItemsControl did not realize containers for the initial items"
        );

        // Mutate from Rust AFTER the first layout. Only INotifyCollectionChanged
        // can make the generator realize a new container here.
        coll.push_string("Gamma");
        assert!(view.update(0.0));
        assert_eq!(content.items_count(), Some(3));
        assert_eq!(
            content.realized_item_count(),
            Some(3),
            "adding an item from Rust did not regenerate a container : \
             INotifyCollectionChanged did not reach the control"
        );

        // Removal must likewise propagate.
        assert!(coll.remove_at(0));
        assert!(view.update(0.0));
        assert_eq!(content.items_count(), Some(2));
        assert_eq!(
            content.realized_item_count(),
            Some(2),
            "removing an item from Rust did not regenerate containers"
        );

        // Teardown: drop the view (releases the ItemsControl's ItemsSource ref)
        // before the collection handle. Clearing the source with the control
        // still alive is sound.
        content.clear_items_source();
        drop(content);
        view.deactivate();
        drop(view);
        drop(coll);
    }

    noesis_runtime::shutdown();
}
