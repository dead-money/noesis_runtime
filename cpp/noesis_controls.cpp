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
#include <NsGui/Expander.h>
#include <NsGui/ItemCollection.h>
#include <NsGui/ItemsControl.h>
#include <NsGui/PasswordBox.h>
#include <NsGui/Popup.h>
#include <NsGui/RangeBase.h>
#include <NsGui/ScrollViewer.h>
#include <NsGui/Selector.h>
#include <NsGui/TextBox.h>
#include <NsGui/ToggleButton.h>

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
