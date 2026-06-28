//! Round-trips for `Selector`/`ItemContainerGenerator`, `TreeView`, `GridView`
//! columns, `ToolTip`/`ContextMenu`, `ScrollViewer` scrolling, and `Image` source.

use std::collections::HashMap;

use noesis_runtime::binding::box_string;
use noesis_runtime::imaging::BitmapImage;
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const SCENE_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid x:Name="Root"
      xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="600" Height="400">
  <ListBox x:Name="List" Width="200" Height="300"
           HorizontalAlignment="Left" VerticalAlignment="Top">
    <ListBoxItem Content="Alpha" Tag="a"/>
    <ListBoxItem Content="Beta"  Tag="b"/>
    <ListBoxItem Content="Gamma" Tag="c"/>
  </ListBox>

  <TreeView x:Name="Tree" Width="160" Height="300"
            HorizontalAlignment="Left" VerticalAlignment="Top" Margin="210,0,0,0">
    <TreeViewItem x:Name="TI0" Header="Root0">
      <TreeViewItem x:Name="TI0a" Header="Child0a"/>
    </TreeViewItem>
    <TreeViewItem x:Name="TI1" Header="Root1"/>
  </TreeView>

  <ListView x:Name="LV" Width="200" Height="120"
            HorizontalAlignment="Right" VerticalAlignment="Top">
    <ListView.View>
      <GridView>
        <GridViewColumn Header="Name" Width="120"/>
        <GridViewColumn Header="Age" Width="50"/>
      </GridView>
    </ListView.View>
  </ListView>

  <!-- Without a theme there is no default ScrollViewer template, so the
       PART_ScrollContentPresenter (which provides the IScrollInfo extent /
       viewport) is absent and scrolling is inert. Supply a minimal one. -->
  <ScrollViewer x:Name="Scroll" Width="200" Height="200"
                HorizontalAlignment="Right" VerticalAlignment="Bottom"
                VerticalScrollBarVisibility="Visible"
                HorizontalScrollBarVisibility="Disabled">
    <ScrollViewer.Template>
      <ControlTemplate TargetType="ScrollViewer">
        <ScrollContentPresenter x:Name="PART_ScrollContentPresenter"
                                Content="{TemplateBinding Content}"
                                CanContentScroll="{TemplateBinding CanContentScroll}"/>
      </ControlTemplate>
    </ScrollViewer.Template>
    <StackPanel>
      <Border Height="100" Background="#FF400000"/>
      <Border Height="100" Background="#FF004000"/>
      <Border Height="100" Background="#FF000040"/>
      <Border Height="100" Background="#FF404000"/>
      <Border Height="100" Background="#FF400040"/>
      <Border Height="100" Background="#FF004040"/>
    </StackPanel>
  </ScrollViewer>

  <Image x:Name="Img" Width="64" Height="64"
         HorizontalAlignment="Center" VerticalAlignment="Center"/>

  <Border x:Name="TipTarget" Width="40" Height="40"
          HorizontalAlignment="Center" VerticalAlignment="Bottom"
          Background="#FF202020"/>
</Grid>"##;

struct InMem {
    bytes: HashMap<String, Vec<u8>>,
}

impl XamlProvider for InMem {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.bytes.get(uri).map(Vec::as_slice)
    }
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}

// Scroll commands apply on subsequent layout passes; pump a few frames.
fn pump(view: &mut View) {
    for i in 0..8 {
        view.update(f64::from(i) * 0.016);
    }
}

#[test]
fn controls_s8_remainder_round_trips() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), SCENE_XAML.as_bytes().to_vec());
        let _registered = noesis_runtime::xaml_provider::set_xaml_provider(InMem { bytes });

        let element = FrameworkElement::load("scene.xaml").expect("load scene.xaml");
        let mut view = View::create(element);
        view.set_size(600, 400);
        view.activate();
        assert!(view.update(0.0), "initial layout");

        let mut root = view.content().expect("content");

        let mut list = root.find_name("List").expect("find List");
        let c0 = list
            .container_from_index(0)
            .expect("container_from_index(0) should be realized after layout");
        let idx0 = unsafe { list.index_from_container(c0.as_ptr()) };
        assert_eq!(idx0, Some(0), "index_from_container(container0)");
        // A directly-authored ListBoxItem is its own container: item == container.
        let item0 = unsafe { list.item_from_container(c0.as_ptr()) }.expect("item_from_container");
        assert_eq!(item0, c0, "ListBoxItem is its own container");
        let c0b = unsafe { list.container_from_item(item0.as_ptr()) }.expect("container_from_item");
        assert_eq!(
            c0b, c0,
            "container_from_item round-trips to the same container"
        );
        assert_eq!(
            unsafe { list.index_from_container(root.raw()) },
            None,
            "Root grid is not a container of List",
        );

        assert!(list.set_selected_index(2), "select index 2");
        let sel_item = list.selected_item().expect("selected_item");
        let sel_val = list
            .selected_value()
            .expect("selected_value (default path)");
        assert_eq!(
            sel_val, sel_item,
            "with empty SelectedValuePath, SelectedValue == SelectedItem",
        );

        // SetSelectedValue with the default path selects the matching item
        // (here the container itself), proven via SelectedIndex moving to 0.
        assert!(
            unsafe { list.set_selected_value(c0.as_ptr()) },
            "set_selected_value(container0)",
        );
        assert_eq!(
            list.selected_index(),
            Some(0),
            "SetSelectedValue moved selection to index 0",
        );

        assert!(
            list.selected_value_path().is_some(),
            "SelectedValuePath is Some for a Selector",
        );
        assert!(list.set_selected_value_path("Tag"), "set SelectedValuePath");
        assert_eq!(
            list.selected_value_path().as_deref(),
            Some("Tag"),
            "SelectedValuePath read-back",
        );
        // Type-guard: a non-Selector has no SelectedValuePath.
        assert_eq!(
            root.selected_value_path(),
            None,
            "Grid is not a Selector (no SelectedValuePath)",
        );

        let mut ti0 = root.find_name("TI0").expect("find TI0");
        let mut ti1 = root.find_name("TI1").expect("find TI1");
        let tree = root.find_name("Tree").expect("find Tree");
        assert_eq!(
            tree.tree_selected_item(),
            None,
            "nothing selected initially"
        );
        assert!(ti0.set_tree_item_is_selected(true), "select TI0");
        assert_eq!(ti0.tree_item_is_selected(), Some(true), "TI0 IsSelected");
        let selected = tree.tree_selected_item().expect("TreeView SelectedItem");
        assert_eq!(
            selected.as_ptr(),
            ti0.raw(),
            "TreeView.SelectedItem is the selected TreeViewItem",
        );
        // Move selection: the live SelectedItem pointer must follow.
        assert!(ti1.set_tree_item_is_selected(true), "select TI1");
        let selected2 = tree.tree_selected_item().expect("SelectedItem after move");
        assert_eq!(
            selected2.as_ptr(),
            ti1.raw(),
            "SelectedItem followed the selection to TI1",
        );
        assert_eq!(ti0.tree_item_is_expanded(), Some(false), "TI0 collapsed");
        assert!(ti0.set_tree_item_is_expanded(true), "expand TI0");
        assert_eq!(ti0.tree_item_is_expanded(), Some(true), "TI0 expanded");
        assert_eq!(
            root.tree_item_is_selected(),
            None,
            "Grid is not a TreeViewItem"
        );

        let lv = root.find_name("LV").expect("find LV");
        let gv = lv.listview_gridview().expect("ListView.View GridView");
        assert_eq!(
            unsafe { FrameworkElement::gridview_column_count(gv) },
            Some(2),
            "two GridView columns",
        );
        assert_eq!(
            unsafe { FrameworkElement::gridview_column_width(gv, 0) },
            Some(120.0),
            "authored column 0 width",
        );
        assert!(
            unsafe { FrameworkElement::set_gridview_column_width(gv, 0, 200.0) },
            "set column 0 width",
        );
        assert_eq!(
            unsafe { FrameworkElement::gridview_column_width(gv, 0) },
            Some(200.0),
            "column 0 width read-back",
        );
        assert!(
            unsafe { FrameworkElement::gridview_column_header(gv, 0) }.is_some(),
            "column 0 Header present",
        );
        assert_eq!(
            unsafe { FrameworkElement::gridview_column_width(gv, 9) },
            None,
            "out-of-range column is None",
        );
        // Non-ListView has no GridView.
        assert!(root.listview_gridview().is_none(), "Grid is not a ListView");

        let mut tip_target = root.find_name("TipTarget").expect("find TipTarget");
        let tip = box_string("hover me");
        assert!(
            unsafe { tip_target.set_tooltip(tip.raw()) },
            "set ToolTip content",
        );
        assert_eq!(
            tip_target.tooltip().map(|p| p.as_ptr()),
            Some(tip.raw()),
            "ToolTip content round-trips",
        );
        // FrameworkElement.ToolTip writes the ToolTipService attached value.
        assert_eq!(
            tip_target.tooltip_service_tooltip().map(|p| p.as_ptr()),
            Some(tip.raw()),
            "ToolTipService.ToolTip mirrors the inline ToolTip DP",
        );
        assert!(
            tip_target.set_tooltip_string("plain text"),
            "set ToolTip from string",
        );
        assert!(
            tip_target.tooltip().is_some(),
            "string ToolTip stored as a component",
        );
        // ToolTip control IsOpen type-guard (opening a popup needs a hosted
        // placement target, so only the type discrimination is asserted).
        let mut tt = FrameworkElement::parse(
            r##"<ToolTip xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"/>"##,
        )
        .expect("parse ToolTip");
        assert_eq!(tt.tooltip_is_open(), Some(false), "ToolTip default IsOpen");
        assert!(tt.set_tooltip_is_open(false), "ToolTip IsOpen setter typed");
        assert_eq!(
            tip_target.tooltip_is_open(),
            None,
            "a Border is not a ToolTip control",
        );

        let mut cm = FrameworkElement::parse(
            r##"<ContextMenu xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"><MenuItem Header="Cut"/></ContextMenu>"##,
        )
        .expect("parse ContextMenu");
        assert!(
            unsafe { tip_target.set_context_menu(cm.raw()) },
            "set ContextMenu",
        );
        assert_eq!(
            tip_target.context_menu().map(|p| p.as_ptr()),
            Some(cm.raw()),
            "ContextMenu round-trips",
        );
        assert_eq!(
            tip_target.context_menu_service_menu().map(|p| p.as_ptr()),
            Some(cm.raw()),
            "ContextMenuService.ContextMenu mirrors the inline DP",
        );
        // A non-ContextMenu rejected by the set type-guard.
        assert!(
            !unsafe { tip_target.set_context_menu(root.raw()) },
            "a Grid is not a ContextMenu",
        );
        assert_eq!(
            cm.context_menu_is_open(),
            Some(false),
            "ContextMenu default IsOpen",
        );
        assert!(
            cm.set_context_menu_is_open(false),
            "ContextMenu IsOpen typed"
        );
        assert_eq!(
            tip_target.context_menu_is_open(),
            None,
            "a Border is not a ContextMenu control",
        );

        let mut scroll = root.find_name("Scroll").expect("find Scroll");
        pump(&mut view);
        assert_eq!(scroll.vertical_offset(), Some(0.0), "starts at top");
        let scrollable = scroll.scrollable_height().expect("scrollable_height");
        assert!(
            scrollable > 0.0,
            "content overflows the viewport (={scrollable})"
        );
        assert!(scroll.extent_width().is_some(), "ExtentWidth present");
        assert!(scroll.viewport_width().is_some(), "ViewportWidth present");

        assert!(scroll.page_down(), "PageDown");
        pump(&mut view);
        let after_page = scroll.vertical_offset().expect("offset after PageDown");
        assert!(
            after_page > 0.0,
            "PageDown moved the offset down (={after_page})"
        );

        assert!(scroll.line_down(), "LineDown");
        pump(&mut view);
        let after_line = scroll.vertical_offset().expect("offset after LineDown");
        assert!(
            after_line >= after_page,
            "LineDown advanced further ({after_line} >= {after_page})",
        );

        assert!(scroll.scroll_to_bottom(), "ScrollToBottom");
        pump(&mut view);
        assert!(
            approx(scroll.vertical_offset().unwrap(), scrollable),
            "ScrollToBottom lands at the scrollable extent",
        );

        assert!(scroll.scroll_to_top(), "ScrollToTop");
        pump(&mut view);
        assert!(
            approx(scroll.vertical_offset().unwrap(), 0.0),
            "ScrollToTop lands back at 0",
        );
        // Type-guard: a Grid is not a ScrollViewer.
        assert!(!root.page_down(), "Grid has no PageDown");

        let mut img = root.find_name("Img").expect("find Img");
        assert_eq!(img.image_source(), None, "Image starts with no Source");
        let bitmap = BitmapImage::new();
        assert!(
            unsafe { img.set_image_source(bitmap.raw()) },
            "set Image.Source",
        );
        assert_eq!(
            img.image_source().map(|p| p.as_ptr()),
            Some(bitmap.raw()),
            "Image.Source round-trips the BitmapImage handle",
        );
        assert!(
            !unsafe { root.set_image_source(bitmap.raw()) },
            "a Grid is not an Image",
        );

        // Keep handles alive across all assertions (Noesis holds its own refs,
        // but the wrappers must outlive the borrowed-pointer comparisons).
        drop(tip);
        drop(cm);
        drop(tt);
        drop(bitmap);
    }
}
