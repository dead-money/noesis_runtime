// Programmatic control access (TODO §8 / Phase B).
//
// Typed sugar + genuinely-new entrypoints over the standard Noesis controls,
// each guarded by a DynamicCast to the right control type so a type mismatch
// degrades gracefully (false / null / a sentinel) rather than crashing across
// the C ABI — mirroring the text_get/set + visual_state guards elsewhere.
//
// Covered families:
//   * Selector  — SelectedIndex / SelectedItem (ListBox/ComboBox/TabControl/…)
//   * ItemsControl.Items — direct collection mutation (Add/Insert/RemoveAt/Clear)
//   * RangeBase — Value/Minimum/Maximum (Slider/ProgressBar/ScrollBar)
//   * ToggleButton — IsChecked as a tri-state Nullable<bool> (CheckBox/RadioButton)
//   * Popup.IsOpen / Expander.IsExpanded
//   * ScrollViewer — read offsets/extents + ScrollTo* methods
//   * BaseTextBox/TextBox — selection + caret; PasswordBox password
//
// No VerifyAccess() — these must never throw across the C ABI. Single-thread
// (View) affinity is the caller's responsibility, like the other accessors.

#include "noesis_shim.h"

#include <NsCore/Noesis.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Nullable.h>
#include <NsGui/ContextMenu.h>
#include <NsGui/ContextMenuService.h>
#include <NsGui/DependencyObject.h>
#include <NsGui/Expander.h>
#include <NsGui/FrameworkElement.h>
#include <NsGui/FreezableCollection.h>
#include <NsGui/GridView.h>
#include <NsGui/GridViewColumn.h>
#include <NsGui/Image.h>
#include <NsGui/ImageSource.h>
#include <NsGui/ItemCollection.h>
#include <NsGui/ItemContainerGenerator.h>
#include <NsGui/ItemsControl.h>
#include <NsGui/ListView.h>
#include <NsGui/PasswordBox.h>
#include <NsGui/Popup.h>
#include <NsGui/RangeBase.h>
#include <NsGui/ScrollViewer.h>
#include <NsGui/Selector.h>
#include <NsGui/TextBox.h>
#include <NsGui/ToggleButton.h>
#include <NsGui/ToolTip.h>
#include <NsGui/ToolTipService.h>
#include <NsGui/TreeView.h>
#include <NsGui/TreeViewItem.h>

namespace {

template<class T>
T* as(void* element) {
    return Noesis::DynamicCast<T*>(static_cast<Noesis::BaseComponent*>(element));
}

} // namespace

// ── Selector (SelectedIndex / SelectedItem) ─────────────────────────────────

// Reads the selected index into *out (>= 0, or -1 when the selection is empty).
// Returns false (leaving *out untouched) if `element` is not a Selector.
extern "C" bool dm_noesis_selector_get_selected_index(void* element, int32_t* out) {
    if (!element || !out) return false;
    auto* s = as<Noesis::Selector>(element);
    if (!s) return false;
    *out = s->GetSelectedIndex();
    return true;
}

// Sets the selected index (-1 clears; an out-of-range index is coerced by
// Noesis to -1). Returns false if `element` is not a Selector.
extern "C" bool dm_noesis_selector_set_selected_index(void* element, int32_t index) {
    if (!element) return false;
    auto* s = as<Noesis::Selector>(element);
    if (!s) return false;
    s->SetSelectedIndex(index);
    return true;
}

// Borrowed (no +1) pointer to the selected item, or null if `element` is not a
// Selector or the selection is empty. For an ItemsSource-bound Selector this is
// the data item; for direct items it is the container.
extern "C" void* dm_noesis_selector_get_selected_item(void* element) {
    if (!element) return nullptr;
    auto* s = as<Noesis::Selector>(element);
    return s ? s->GetSelectedItem() : nullptr;
}

// Sets the selected item (borrowed; Noesis takes its own reference). Pass null
// to clear. Returns false if `element` is not a Selector.
extern "C" bool dm_noesis_selector_set_selected_item(void* element, void* item) {
    if (!element) return false;
    auto* s = as<Noesis::Selector>(element);
    if (!s) return false;
    s->SetSelectedItem(static_cast<Noesis::BaseComponent*>(item));
    return true;
}

// ── ItemsControl.Items direct mutation ──────────────────────────────────────
//
// These mutate the control's own `Items` collection (NOT an external
// ItemsSource — when ItemsSource is set, Items is read-only and these no-op via
// Noesis's own guard). `item` is a borrowed BaseComponent* (typically a boxed
// value); the ItemCollection takes its own reference.

// Appends `item`; returns the new index, or -1 if `element` is not an
// ItemsControl (or the add was rejected, e.g. Items is read-only).
extern "C" int32_t dm_noesis_items_control_items_add(void* element, void* item) {
    if (!element) return -1;
    auto* ic = as<Noesis::ItemsControl>(element);
    if (!ic) return -1;
    Noesis::ItemCollection* items = ic->GetItems();
    if (!items) return -1;
    return items->Add(static_cast<Noesis::BaseComponent*>(item));
}

// Inserts `item` at `index` (allows index == Count). Returns false if `element`
// is not an ItemsControl or `index` is out of range.
extern "C" bool dm_noesis_items_control_items_insert(void* element, uint32_t index, void* item) {
    if (!element) return false;
    auto* ic = as<Noesis::ItemsControl>(element);
    if (!ic) return false;
    Noesis::ItemCollection* items = ic->GetItems();
    if (!items || index > (uint32_t)items->Count()) return false;
    items->Insert(index, static_cast<Noesis::BaseComponent*>(item));
    return true;
}

// Removes the item at `index`. Returns false if `element` is not an
// ItemsControl or `index` is out of range.
extern "C" bool dm_noesis_items_control_items_remove_at(void* element, uint32_t index) {
    if (!element) return false;
    auto* ic = as<Noesis::ItemsControl>(element);
    if (!ic) return false;
    Noesis::ItemCollection* items = ic->GetItems();
    if (!items || index >= (uint32_t)items->Count()) return false;
    items->RemoveAt(index);
    return true;
}

// Removes every item. Returns false if `element` is not an ItemsControl.
extern "C" bool dm_noesis_items_control_items_clear(void* element) {
    if (!element) return false;
    auto* ic = as<Noesis::ItemsControl>(element);
    if (!ic) return false;
    Noesis::ItemCollection* items = ic->GetItems();
    if (!items) return false;
    items->Clear();
    return true;
}

// ── RangeBase (Value / Minimum / Maximum) ───────────────────────────────────
//
// `which`: 0 = Value, 1 = Minimum, 2 = Maximum. Getter writes *out and returns
// false (leaving *out untouched) on a non-RangeBase or a bad selector. Setter
// goes through RangeBase::SetValue/SetMinimum/SetMaximum so Noesis's coercion
// (Value clamped to [Minimum, Maximum]) runs.

extern "C" bool dm_noesis_rangebase_get(void* element, int32_t which, float* out) {
    if (!element || !out) return false;
    auto* r = as<Noesis::RangeBase>(element);
    if (!r) return false;
    switch (which) {
        case 0: *out = r->GetValue(); return true;
        case 1: *out = r->GetMinimum(); return true;
        case 2: *out = r->GetMaximum(); return true;
        default: return false;
    }
}

extern "C" bool dm_noesis_rangebase_set(void* element, int32_t which, float value) {
    if (!element) return false;
    auto* r = as<Noesis::RangeBase>(element);
    if (!r) return false;
    switch (which) {
        case 0: r->SetValue(value); return true;
        case 1: r->SetMinimum(value); return true;
        case 2: r->SetMaximum(value); return true;
        default: return false;
    }
}

// ── ToggleButton.IsChecked (tri-state Nullable<bool>) ───────────────────────
//
// `state`: 0 = unchecked, 1 = checked, 2 = indeterminate (null). The getter
// writes *out_state and returns false (leaving it untouched) on a non-toggle.

extern "C" bool dm_noesis_toggle_get_is_checked(void* element, int8_t* out_state) {
    if (!element || !out_state) return false;
    auto* tb = as<Noesis::ToggleButton>(element);
    if (!tb) return false;
    const Noesis::Nullable<bool>& v = tb->GetIsChecked();
    *out_state = !v.HasValue() ? 2 : (v.GetValue() ? 1 : 0);
    return true;
}

extern "C" bool dm_noesis_toggle_set_is_checked(void* element, int8_t state) {
    if (!element) return false;
    auto* tb = as<Noesis::ToggleButton>(element);
    if (!tb) return false;
    if (state == 2) {
        tb->SetIsChecked(Noesis::Nullable<bool>());
    } else {
        tb->SetIsChecked(Noesis::Nullable<bool>(state != 0));
    }
    return true;
}

// ── Popup.IsOpen / Expander.IsExpanded ──────────────────────────────────────

extern "C" bool dm_noesis_popup_get_is_open(void* element, bool* out) {
    if (!element || !out) return false;
    auto* p = as<Noesis::Popup>(element);
    if (!p) return false;
    *out = p->GetIsOpen();
    return true;
}

extern "C" bool dm_noesis_popup_set_is_open(void* element, bool open) {
    if (!element) return false;
    auto* p = as<Noesis::Popup>(element);
    if (!p) return false;
    p->SetIsOpen(open);
    return true;
}

extern "C" bool dm_noesis_expander_get_is_expanded(void* element, bool* out) {
    if (!element || !out) return false;
    auto* e = as<Noesis::Expander>(element);
    if (!e) return false;
    *out = e->GetIsExpanded();
    return true;
}

extern "C" bool dm_noesis_expander_set_is_expanded(void* element, bool expanded) {
    if (!element) return false;
    auto* e = as<Noesis::Expander>(element);
    if (!e) return false;
    e->SetIsExpanded(expanded);
    return true;
}

// ── ScrollViewer ────────────────────────────────────────────────────────────
//
// `which`: 0 = HorizontalOffset, 1 = VerticalOffset, 2 = ScrollableWidth,
// 3 = ScrollableHeight, 4 = ExtentHeight, 5 = ViewportHeight. These are
// read-only computed metrics (no DP setters); scrolling goes through the
// ScrollTo* methods. Getter returns false (leaving *out untouched) on a
// non-ScrollViewer or bad selector.

extern "C" bool dm_noesis_scrollviewer_get(void* element, int32_t which, float* out) {
    if (!element || !out) return false;
    auto* sv = as<Noesis::ScrollViewer>(element);
    if (!sv) return false;
    switch (which) {
        case 0: *out = sv->GetHorizontalOffset(); return true;
        case 1: *out = sv->GetVerticalOffset(); return true;
        case 2: *out = sv->GetScrollableWidth(); return true;
        case 3: *out = sv->GetScrollableHeight(); return true;
        case 4: *out = sv->GetExtentHeight(); return true;
        case 5: *out = sv->GetViewportHeight(); return true;
        default: return false;
    }
}

extern "C" bool dm_noesis_scrollviewer_scroll_to_horizontal(void* element, float offset) {
    if (!element) return false;
    auto* sv = as<Noesis::ScrollViewer>(element);
    if (!sv) return false;
    sv->ScrollToHorizontalOffset(offset);
    return true;
}

extern "C" bool dm_noesis_scrollviewer_scroll_to_vertical(void* element, float offset) {
    if (!element) return false;
    auto* sv = as<Noesis::ScrollViewer>(element);
    if (!sv) return false;
    sv->ScrollToVerticalOffset(offset);
    return true;
}

// ScrollToHome scrolls to the top-left origin; ScrollToEnd scrolls to the
// bottom. Both are axis-agnostic ScrollViewer helpers.
extern "C" bool dm_noesis_scrollviewer_scroll_to_home(void* element) {
    if (!element) return false;
    auto* sv = as<Noesis::ScrollViewer>(element);
    if (!sv) return false;
    sv->ScrollToHome();
    return true;
}

extern "C" bool dm_noesis_scrollviewer_scroll_to_end(void* element) {
    if (!element) return false;
    auto* sv = as<Noesis::ScrollViewer>(element);
    if (!sv) return false;
    sv->ScrollToEnd();
    return true;
}

// ── TextBox selection / caret ───────────────────────────────────────────────
//
// `which` for the int getters/setters: 0 = SelectionStart, 1 = SelectionLength,
// 2 = CaretIndex. Getter writes *out, returns false on a non-TextBox.

extern "C" bool dm_noesis_textbox_get_int(void* element, int32_t which, int32_t* out) {
    if (!element || !out) return false;
    auto* tb = as<Noesis::TextBox>(element);
    if (!tb) return false;
    switch (which) {
        case 0: *out = tb->GetSelectionStart(); return true;
        case 1: *out = tb->GetSelectionLength(); return true;
        case 2: *out = tb->GetCaretIndex(); return true;
        default: return false;
    }
}

extern "C" bool dm_noesis_textbox_set_int(void* element, int32_t which, int32_t value) {
    if (!element) return false;
    auto* tb = as<Noesis::TextBox>(element);
    if (!tb) return false;
    switch (which) {
        case 0: tb->SetSelectionStart(value); return true;
        case 1: tb->SetSelectionLength(value); return true;
        case 2: tb->SetCaretIndex(value); return true;
        default: return false;
    }
}

// Selects `length` characters starting at `start`. Returns false on a
// non-TextBox.
extern "C" bool dm_noesis_textbox_select(void* element, int32_t start, int32_t length) {
    if (!element) return false;
    auto* tb = as<Noesis::TextBox>(element);
    if (!tb) return false;
    tb->Select(start, length);
    return true;
}

extern "C" bool dm_noesis_textbox_select_all(void* element) {
    if (!element) return false;
    auto* tb = as<Noesis::TextBox>(element);
    if (!tb) return false;
    tb->SelectAll();
    return true;
}

// Borrowed (no copy on our side) pointer to the currently-selected text, or
// null if `element` is not a TextBox. Copy immediately on the Rust side.
extern "C" const char* dm_noesis_textbox_get_selected_text(void* element) {
    if (!element) return nullptr;
    auto* tb = as<Noesis::TextBox>(element);
    return tb ? tb->GetSelectedText() : nullptr;
}

// ── PasswordBox ─────────────────────────────────────────────────────────────

// Borrowed pointer to the password plaintext, or null if not a PasswordBox.
// Copy immediately on the Rust side.
extern "C" const char* dm_noesis_passwordbox_get_password(void* element) {
    if (!element) return nullptr;
    auto* pb = as<Noesis::PasswordBox>(element);
    return pb ? pb->GetPassword() : nullptr;
}

extern "C" bool dm_noesis_passwordbox_set_password(void* element, const char* password) {
    if (!element) return false;
    auto* pb = as<Noesis::PasswordBox>(element);
    if (!pb) return false;
    pb->SetPassword(password ? password : "");
    return true;
}

// ════════════════════════════════════════════════════════════════════════════
// §8 remainder (prefix dm_noesis_controls_): SelectedValue/Path, TreeView
// selection, ItemContainerGenerator mapping, GridView columns, ToolTip /
// ContextMenu, line/page scrolling + IScrollInfo, Image source.
// ════════════════════════════════════════════════════════════════════════════

// ── Selector.SelectedValue / SelectedValuePath ──────────────────────────────
//
// SelectedValue is the value of SelectedItem projected through SelectedValuePath
// (the whole item when the path is empty). Setting it selects the item whose
// projected value matches.

// Borrowed (no +1) canonical BaseComponent* of the current SelectedValue, or
// null when there is no selection or `element` is not a Selector.
extern "C" void* dm_noesis_controls_selector_get_selected_value(void* element) {
    if (!element) return nullptr;
    auto* s = as<Noesis::Selector>(element);
    return s ? static_cast<Noesis::BaseComponent*>(s->GetSelectedValue()) : nullptr;
}

// Selects the item whose projected value equals `value` (borrowed; Noesis takes
// its own reference). Pass null to clear. Returns false on a non-Selector.
extern "C" bool dm_noesis_controls_selector_set_selected_value(void* element, void* value) {
    if (!element) return false;
    auto* s = as<Noesis::Selector>(element);
    if (!s) return false;
    s->SetSelectedValue(static_cast<Noesis::BaseComponent*>(value));
    return true;
}

// Borrowed pointer to the SelectedValuePath string (never null for a Selector —
// the default is ""). Null only when `element` is not a Selector. Copy now.
extern "C" const char* dm_noesis_controls_selector_get_selected_value_path(void* element) {
    if (!element) return nullptr;
    auto* s = as<Noesis::Selector>(element);
    return s ? s->GetSelectedValuePath() : nullptr;
}

extern "C" bool dm_noesis_controls_selector_set_selected_value_path(void* element,
                                                                    const char* path) {
    if (!element) return false;
    auto* s = as<Noesis::Selector>(element);
    if (!s) return false;
    s->SetSelectedValuePath(path ? path : "");
    return true;
}

// ── TreeView selection ──────────────────────────────────────────────────────

// Borrowed canonical BaseComponent* of the TreeView's selected item (the data
// item, or the TreeViewItem container for direct items), or null when nothing
// is selected / `element` is not a TreeView.
extern "C" void* dm_noesis_controls_treeview_get_selected_item(void* element) {
    if (!element) return nullptr;
    auto* tv = as<Noesis::TreeView>(element);
    return tv ? static_cast<Noesis::BaseComponent*>(tv->GetSelectedItem()) : nullptr;
}

// TreeViewItem.IsSelected / IsExpanded — selection is driven per-item in a
// TreeView (there is no public TreeView::SetSelectedItem).
extern "C" bool dm_noesis_controls_treeviewitem_get_is_selected(void* element, bool* out) {
    if (!element || !out) return false;
    auto* item = as<Noesis::TreeViewItem>(element);
    if (!item) return false;
    *out = item->GetIsSelected();
    return true;
}

extern "C" bool dm_noesis_controls_treeviewitem_set_is_selected(void* element, bool selected) {
    if (!element) return false;
    auto* item = as<Noesis::TreeViewItem>(element);
    if (!item) return false;
    item->SetIsSelected(selected);
    return true;
}

extern "C" bool dm_noesis_controls_treeviewitem_get_is_expanded(void* element, bool* out) {
    if (!element || !out) return false;
    auto* item = as<Noesis::TreeViewItem>(element);
    if (!item) return false;
    *out = item->GetIsExpanded();
    return true;
}

extern "C" bool dm_noesis_controls_treeviewitem_set_is_expanded(void* element, bool expanded) {
    if (!element) return false;
    auto* item = as<Noesis::TreeViewItem>(element);
    if (!item) return false;
    item->SetIsExpanded(expanded);
    return true;
}

// ── ItemContainerGenerator (container ⇄ item ⇄ index) ───────────────────────
//
// Each takes the host ItemsControl and routes through its
// GetItemContainerGenerator(). Containers are only realized once the control has
// been laid out in a live View. Returned containers/items are borrowed canonical
// BaseComponent* (a DependencyObject is-a BaseComponent).

extern "C" void* dm_noesis_controls_generator_container_from_index(void* element, int32_t index) {
    if (!element) return nullptr;
    auto* ic = as<Noesis::ItemsControl>(element);
    if (!ic) return nullptr;
    Noesis::ItemContainerGenerator* g = ic->GetItemContainerGenerator();
    if (!g) return nullptr;
    return static_cast<Noesis::BaseComponent*>(g->ContainerFromIndex(index));
}

extern "C" void* dm_noesis_controls_generator_container_from_item(void* element, void* item) {
    if (!element) return nullptr;
    auto* ic = as<Noesis::ItemsControl>(element);
    if (!ic) return nullptr;
    Noesis::ItemContainerGenerator* g = ic->GetItemContainerGenerator();
    if (!g) return nullptr;
    return static_cast<Noesis::BaseComponent*>(
        g->ContainerFromItem(static_cast<Noesis::BaseComponent*>(item)));
}

// Index of `container` in the items, or -1 when it is not a realized container /
// `element` is not an ItemsControl.
extern "C" int32_t dm_noesis_controls_generator_index_from_container(void* element,
                                                                     void* container) {
    if (!element || !container) return -1;
    auto* ic = as<Noesis::ItemsControl>(element);
    if (!ic) return -1;
    Noesis::ItemContainerGenerator* g = ic->GetItemContainerGenerator();
    if (!g) return -1;
    auto* dobj = Noesis::DynamicCast<Noesis::DependencyObject*>(
        static_cast<Noesis::BaseComponent*>(container));
    if (!dobj) return -1;
    return g->IndexFromContainer(dobj);
}

extern "C" void* dm_noesis_controls_generator_item_from_container(void* element, void* container) {
    if (!element || !container) return nullptr;
    auto* ic = as<Noesis::ItemsControl>(element);
    if (!ic) return nullptr;
    Noesis::ItemContainerGenerator* g = ic->GetItemContainerGenerator();
    if (!g) return nullptr;
    auto* dobj = Noesis::DynamicCast<Noesis::DependencyObject*>(
        static_cast<Noesis::BaseComponent*>(container));
    if (!dobj) return nullptr;
    return static_cast<Noesis::BaseComponent*>(g->ItemFromContainer(dobj));
}

// ── ListView / GridView columns ─────────────────────────────────────────────

// Borrowed canonical pointer to the GridView set as a ListView's View, or null
// when `element` is not a ListView or its View is not a GridView.
extern "C" void* dm_noesis_controls_listview_get_view(void* element) {
    if (!element) return nullptr;
    auto* lv = as<Noesis::ListView>(element);
    if (!lv) return nullptr;
    auto* gv = Noesis::DynamicCast<Noesis::GridView*>(lv->GetView());
    return gv ? static_cast<Noesis::BaseComponent*>(gv) : nullptr;
}

// Number of columns, or -1 when `gridview` is not a GridView.
extern "C" int32_t dm_noesis_controls_gridview_column_count(void* gridview) {
    if (!gridview) return -1;
    auto* gv = as<Noesis::GridView>(gridview);
    if (!gv) return -1;
    Noesis::GridViewColumnCollection* cols = gv->GetColumns();
    return cols ? cols->Count() : -1;
}

namespace {
Noesis::GridViewColumn* column_at(void* gridview, uint32_t index) {
    auto* gv = Noesis::DynamicCast<Noesis::GridView*>(
        static_cast<Noesis::BaseComponent*>(gridview));
    if (!gv) return nullptr;
    Noesis::GridViewColumnCollection* cols = gv->GetColumns();
    if (!cols || index >= (uint32_t)cols->Count()) return nullptr;
    return cols->Get(index);
}
} // namespace

extern "C" bool dm_noesis_controls_gridview_column_get_width(void* gridview, uint32_t index,
                                                             float* out) {
    if (!gridview || !out) return false;
    Noesis::GridViewColumn* col = column_at(gridview, index);
    if (!col) return false;
    *out = col->GetWidth();
    return true;
}

extern "C" bool dm_noesis_controls_gridview_column_set_width(void* gridview, uint32_t index,
                                                             float width) {
    if (!gridview) return false;
    Noesis::GridViewColumn* col = column_at(gridview, index);
    if (!col) return false;
    col->SetWidth(width);
    return true;
}

extern "C" bool dm_noesis_controls_gridview_column_get_actual_width(void* gridview, uint32_t index,
                                                                    float* out) {
    if (!gridview || !out) return false;
    Noesis::GridViewColumn* col = column_at(gridview, index);
    if (!col) return false;
    *out = col->GetActualWidth();
    return true;
}

// Borrowed canonical pointer to a column's Header (typically a boxed string),
// or null on a bad index / non-GridView / null header.
extern "C" void* dm_noesis_controls_gridview_column_get_header(void* gridview, uint32_t index) {
    if (!gridview) return nullptr;
    Noesis::GridViewColumn* col = column_at(gridview, index);
    if (!col) return nullptr;
    return static_cast<Noesis::BaseComponent*>(col->GetHeader());
}

// ── ToolTip / ToolTipService ────────────────────────────────────────────────

// FrameworkElement.ToolTip (the inline DP). Borrowed canonical content pointer.
extern "C" void* dm_noesis_controls_fe_get_tooltip(void* element) {
    if (!element) return nullptr;
    auto* fe = as<Noesis::FrameworkElement>(element);
    return fe ? static_cast<Noesis::BaseComponent*>(fe->GetToolTip()) : nullptr;
}

extern "C" bool dm_noesis_controls_fe_set_tooltip(void* element, void* tooltip) {
    if (!element) return false;
    auto* fe = as<Noesis::FrameworkElement>(element);
    if (!fe) return false;
    fe->SetToolTip(static_cast<Noesis::BaseComponent*>(tooltip));
    return true;
}

extern "C" bool dm_noesis_controls_fe_set_tooltip_string(void* element, const char* text) {
    if (!element) return false;
    auto* fe = as<Noesis::FrameworkElement>(element);
    if (!fe) return false;
    fe->SetToolTip(text ? text : "");
    return true;
}

// ToolTipService attached ToolTip — readable on ANY DependencyObject (it is what
// FrameworkElement.ToolTip ultimately writes; exposing the service lets a caller
// read/write it on non-FrameworkElement targets too).
extern "C" void* dm_noesis_controls_tooltipservice_get_tooltip(void* obj) {
    if (!obj) return nullptr;
    auto* d = as<Noesis::DependencyObject>(obj);
    return d ? static_cast<Noesis::BaseComponent*>(Noesis::ToolTipService::GetToolTip(d)) : nullptr;
}

extern "C" bool dm_noesis_controls_tooltipservice_set_tooltip(void* obj, void* tooltip) {
    if (!obj) return false;
    auto* d = as<Noesis::DependencyObject>(obj);
    if (!d) return false;
    Noesis::ToolTipService::SetToolTip(d, static_cast<Noesis::BaseComponent*>(tooltip));
    return true;
}

// ToolTip control IsOpen.
extern "C" bool dm_noesis_controls_tooltip_get_is_open(void* element, bool* out) {
    if (!element || !out) return false;
    auto* t = as<Noesis::ToolTip>(element);
    if (!t) return false;
    *out = t->GetIsOpen();
    return true;
}

extern "C" bool dm_noesis_controls_tooltip_set_is_open(void* element, bool open) {
    if (!element) return false;
    auto* t = as<Noesis::ToolTip>(element);
    if (!t) return false;
    t->SetIsOpen(open);
    return true;
}

// ── ContextMenu / ContextMenuService ────────────────────────────────────────

extern "C" void* dm_noesis_controls_fe_get_context_menu(void* element) {
    if (!element) return nullptr;
    auto* fe = as<Noesis::FrameworkElement>(element);
    return fe ? static_cast<Noesis::BaseComponent*>(fe->GetContextMenu()) : nullptr;
}

// `menu` must be a ContextMenu* (borrowed; Noesis takes its own reference) or
// null to clear. Returns false on a non-FrameworkElement or a non-ContextMenu
// `menu`.
extern "C" bool dm_noesis_controls_fe_set_context_menu(void* element, void* menu) {
    if (!element) return false;
    auto* fe = as<Noesis::FrameworkElement>(element);
    if (!fe) return false;
    Noesis::ContextMenu* cm = menu ? as<Noesis::ContextMenu>(menu) : nullptr;
    if (menu && !cm) return false;
    fe->SetContextMenu(cm);
    return true;
}

// ContextMenuService attached ContextMenu — readable/writable on any
// DependencyObject.
extern "C" void* dm_noesis_controls_contextmenuservice_get_context_menu(void* obj) {
    if (!obj) return nullptr;
    auto* d = as<Noesis::DependencyObject>(obj);
    return d ? static_cast<Noesis::BaseComponent*>(Noesis::ContextMenuService::GetContextMenu(d))
             : nullptr;
}

extern "C" bool dm_noesis_controls_contextmenuservice_set_context_menu(void* obj, void* menu) {
    if (!obj) return false;
    auto* d = as<Noesis::DependencyObject>(obj);
    if (!d) return false;
    Noesis::ContextMenu* cm = menu ? as<Noesis::ContextMenu>(menu) : nullptr;
    if (menu && !cm) return false;
    Noesis::ContextMenuService::SetContextMenu(d, cm);
    return true;
}

// ContextMenu control IsOpen.
extern "C" bool dm_noesis_controls_contextmenu_get_is_open(void* element, bool* out) {
    if (!element || !out) return false;
    auto* cm = as<Noesis::ContextMenu>(element);
    if (!cm) return false;
    *out = cm->GetIsOpen();
    return true;
}

extern "C" bool dm_noesis_controls_contextmenu_set_is_open(void* element, bool open) {
    if (!element) return false;
    auto* cm = as<Noesis::ContextMenu>(element);
    if (!cm) return false;
    cm->SetIsOpen(open);
    return true;
}

// ── ScrollViewer line / page / edge scrolling + IScrollInfo ─────────────────
//
// `which` for line: 0=LineUp 1=LineDown 2=LineLeft 3=LineRight
//        for page: 0=PageUp 1=PageDown 2=PageLeft 3=PageRight
//        for edge: 0=Top 1=Bottom 2=LeftEnd 3=RightEnd
// All are deferred by Noesis to the next layout pass (like ScrollToOffset).

extern "C" bool dm_noesis_controls_scrollviewer_line(void* element, int32_t which) {
    if (!element) return false;
    auto* sv = as<Noesis::ScrollViewer>(element);
    if (!sv) return false;
    switch (which) {
        case 0: sv->LineUp(); return true;
        case 1: sv->LineDown(); return true;
        case 2: sv->LineLeft(); return true;
        case 3: sv->LineRight(); return true;
        default: return false;
    }
}

extern "C" bool dm_noesis_controls_scrollviewer_page(void* element, int32_t which) {
    if (!element) return false;
    auto* sv = as<Noesis::ScrollViewer>(element);
    if (!sv) return false;
    switch (which) {
        case 0: sv->PageUp(); return true;
        case 1: sv->PageDown(); return true;
        case 2: sv->PageLeft(); return true;
        case 3: sv->PageRight(); return true;
        default: return false;
    }
}

extern "C" bool dm_noesis_controls_scrollviewer_edge(void* element, int32_t which) {
    if (!element) return false;
    auto* sv = as<Noesis::ScrollViewer>(element);
    if (!sv) return false;
    switch (which) {
        case 0: sv->ScrollToTop(); return true;
        case 1: sv->ScrollToBottom(); return true;
        case 2: sv->ScrollToLeftEnd(); return true;
        case 3: sv->ScrollToRightEnd(); return true;
        default: return false;
    }
}

// Extra ScrollViewer width metrics (the existing dm_noesis_scrollviewer_get
// exposes 0..5; 6=ExtentWidth, 7=ViewportWidth). Noesis 3.2.13 keeps
// ScrollViewer::GetScrollInfo() protected, so the raw IScrollInfo backend is
// not publicly reachable; the public line/page/edge methods above are the
// IScrollInfo surface as exposed by ScrollViewer.
extern "C" bool dm_noesis_controls_scrollviewer_metric(void* element, int32_t which, float* out) {
    if (!element || !out) return false;
    auto* sv = as<Noesis::ScrollViewer>(element);
    if (!sv) return false;
    switch (which) {
        case 6: *out = sv->GetExtentWidth(); return true;
        case 7: *out = sv->GetViewportWidth(); return true;
        default: return false;
    }
}

// ── Image source ────────────────────────────────────────────────────────────

// Borrowed canonical pointer to the Image's Source (an ImageSource), or null
// when unset / `element` is not an Image.
extern "C" void* dm_noesis_controls_image_get_source(void* element) {
    if (!element) return nullptr;
    auto* img = as<Noesis::Image>(element);
    return img ? static_cast<Noesis::BaseComponent*>(img->GetSource()) : nullptr;
}

// `source` must be an ImageSource* (e.g. a BitmapImage / TextureSource handle's
// raw pointer; borrowed, Noesis takes its own reference) or null to clear.
// Returns false on a non-Image or a `source` that is not an ImageSource.
extern "C" bool dm_noesis_controls_image_set_source(void* element, void* source) {
    if (!element) return false;
    auto* img = as<Noesis::Image>(element);
    if (!img) return false;
    Noesis::ImageSource* src = source ? as<Noesis::ImageSource>(source) : nullptr;
    if (source && !src) return false;
    img->SetSource(src);
    return true;
}
