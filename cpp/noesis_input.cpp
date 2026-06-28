// Input — finer control (TODO §16).
//
// Element-level mouse/touch capture, keyboard-state queries, focus-state DPs,
// focus engagement + traversal (MoveFocus / PredictFocus), the FocusManager and
// KeyboardNavigation static/attached-property surfaces, and input gestures +
// bindings (KeyGesture / MouseGesture / KeyBinding / MouseBinding /
// InputBinding) wired to the existing RustCommand ICommand bridge.
//
// Everything here narrows an opaque `Noesis::BaseComponent*` to the concrete
// type it needs via DynamicCast and null-checks first, returning false / null /
// a no-op on a type mismatch — never dereferencing blind. Borrowed pointers
// (GetCaptured, GetFocused, GetFocusedElement, GetFocusScope, PredictFocus) are
// returned WITHOUT an extra reference; the create entrypoints (gestures /
// bindings) return a fresh object at +1 that Rust releases on drop, mirroring
// the `new Noesis::RoutedCommand(...)` idiom in noesis_commands.cpp.

#include "noesis_shim.h"

#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsGui/DependencyObject.h>
#include <NsGui/Enums.h>
#include <NsGui/FocusManager.h>
#include <NsGui/ICommand.h>
#include <NsGui/InputBinding.h>
#include <NsGui/InputEnums.h>
#include <NsGui/Keyboard.h>
#include <NsGui/KeyBinding.h>
#include <NsGui/KeyGesture.h>
#include <NsGui/KeyboardNavigation.h>
#include <NsGui/Mouse.h>
#include <NsGui/MouseBinding.h>
#include <NsGui/MouseGesture.h>
#include <NsGui/UICollection.h>
#include <NsGui/UIElement.h>

// Lock the FFI enum ordinals against the Noesis headers at compile time, the
// same way noesis_view.cpp pins MouseButton / Key.
static_assert((int32_t)Noesis::ModifierKeys_None == 0, "ModifierKeys::None");
static_assert((int32_t)Noesis::ModifierKeys_Alt == 1, "ModifierKeys::Alt");
static_assert((int32_t)Noesis::ModifierKeys_Control == 2, "ModifierKeys::Control");
static_assert((int32_t)Noesis::ModifierKeys_Shift == 4, "ModifierKeys::Shift");
static_assert((int32_t)Noesis::ModifierKeys_Windows == 8, "ModifierKeys::Windows");

static_assert((int32_t)Noesis::KeyStates_None == 0, "KeyStates::None");
static_assert((int32_t)Noesis::KeyStates_Down == 1, "KeyStates::Down");
static_assert((int32_t)Noesis::KeyStates_Toggled == 2, "KeyStates::Toggled");

static_assert((int32_t)Noesis::CaptureMode_None == 0, "CaptureMode::None");
static_assert((int32_t)Noesis::CaptureMode_Element == 1, "CaptureMode::Element");
static_assert((int32_t)Noesis::CaptureMode_SubTree == 2, "CaptureMode::SubTree");

static_assert((int32_t)Noesis::MouseAction_None == 0, "MouseAction::None");
static_assert((int32_t)Noesis::MouseAction_LeftClick == 1, "MouseAction::LeftClick");
static_assert((int32_t)Noesis::MouseAction_RightClick == 2, "MouseAction::RightClick");
static_assert((int32_t)Noesis::MouseAction_MiddleClick == 3, "MouseAction::MiddleClick");
static_assert((int32_t)Noesis::MouseAction_WheelClick == 4, "MouseAction::WheelClick");
static_assert((int32_t)Noesis::MouseAction_LeftDoubleClick == 5, "MouseAction::LeftDoubleClick");
static_assert((int32_t)Noesis::MouseAction_RightDoubleClick == 6, "MouseAction::RightDoubleClick");
static_assert((int32_t)Noesis::MouseAction_MiddleDoubleClick == 7, "MouseAction::MiddleDoubleClick");

static_assert((int32_t)Noesis::FocusNavigationDirection_Next == 0, "FND::Next");
static_assert((int32_t)Noesis::FocusNavigationDirection_Previous == 1, "FND::Previous");
static_assert((int32_t)Noesis::FocusNavigationDirection_First == 2, "FND::First");
static_assert((int32_t)Noesis::FocusNavigationDirection_Last == 3, "FND::Last");
static_assert((int32_t)Noesis::FocusNavigationDirection_Left == 4, "FND::Left");
static_assert((int32_t)Noesis::FocusNavigationDirection_Right == 5, "FND::Right");
static_assert((int32_t)Noesis::FocusNavigationDirection_Up == 6, "FND::Up");
static_assert((int32_t)Noesis::FocusNavigationDirection_Down == 7, "FND::Down");

static_assert((int32_t)Noesis::KeyboardNavigationMode_Continue == 0, "KNM::Continue");
static_assert((int32_t)Noesis::KeyboardNavigationMode_Once == 1, "KNM::Once");
static_assert((int32_t)Noesis::KeyboardNavigationMode_Cycle == 2, "KNM::Cycle");
static_assert((int32_t)Noesis::KeyboardNavigationMode_None == 3, "KNM::None");
static_assert((int32_t)Noesis::KeyboardNavigationMode_Contained == 4, "KNM::Contained");
static_assert((int32_t)Noesis::KeyboardNavigationMode_Local == 5, "KNM::Local");

namespace {

inline Noesis::UIElement* as_ui(void* p) {
    return Noesis::DynamicCast<Noesis::UIElement*>(static_cast<Noesis::BaseComponent*>(p));
}

inline Noesis::DependencyObject* as_do(void* p) {
    return Noesis::DynamicCast<Noesis::DependencyObject*>(static_cast<Noesis::BaseComponent*>(p));
}

}  // namespace

// ── Mouse / touch capture (element-level) ────────────────────────────────────

extern "C" bool dm_noesis_ui_element_capture_mouse(void* element) {
    Noesis::UIElement* ui = as_ui(element);
    return ui ? ui->CaptureMouse() : false;
}

extern "C" void dm_noesis_ui_element_release_mouse_capture(void* element) {
    Noesis::UIElement* ui = as_ui(element);
    if (ui) {
        ui->ReleaseMouseCapture();
    }
}

extern "C" bool dm_noesis_ui_element_get_is_mouse_captured(void* element) {
    Noesis::UIElement* ui = as_ui(element);
    return ui ? ui->GetIsMouseCaptured() : false;
}

extern "C" bool dm_noesis_ui_element_capture_touch(void* element, uint64_t touch_device) {
    Noesis::UIElement* ui = as_ui(element);
    return ui ? ui->CaptureTouch(touch_device) : false;
}

// Capture `element` through its View's Mouse with the given CaptureMode (0 None,
// 1 Element, 2 SubTree). Returns false if not a UIElement or not yet attached to
// a View (GetMouse() is null until layout connects the element).
extern "C" bool dm_noesis_ui_element_capture_mouse_mode(void* element, int32_t mode) {
    Noesis::UIElement* ui = as_ui(element);
    if (!ui) return false;
    Noesis::Mouse* mouse = ui->GetMouse();
    return mouse ? mouse->Capture(ui, static_cast<Noesis::CaptureMode>(mode)) : false;
}

// Borrowed `UIElement*` currently holding mouse capture in `element`'s View, or
// null if nothing is captured / not a UIElement / no View.
extern "C" void* dm_noesis_ui_element_get_mouse_captured(void* element) {
    Noesis::UIElement* ui = as_ui(element);
    if (!ui) return nullptr;
    Noesis::Mouse* mouse = ui->GetMouse();
    return mouse ? static_cast<void*>(mouse->GetCaptured()) : nullptr;
}

// Pointer position relative to `element`, in element-local DIPs
// (Mouse::GetPosition(UIElement*)). Writes the position into *out_x/*out_y and
// returns true; false (outputs untouched) if `element` is not a UIElement. The
// value is the last position the View's Mouse recorded (set by MouseMove), so it
// is meaningful only once `element` is attached to a live, input-pumped View;
// before any pointer event it reads back as the origin.
extern "C" bool dm_noesis_ui_element_get_mouse_position(
    void* element, float* out_x, float* out_y) {
    Noesis::UIElement* ui = as_ui(element);
    if (!ui) return false;
    Noesis::Point p = Noesis::Mouse::GetPosition(ui);
    if (out_x) *out_x = p.x;
    if (out_y) *out_y = p.y;
    return true;
}

// ── Keyboard state / modifiers (via UIElement::GetKeyboard) ──────────────────

// `ModifierKeys` bitmask currently held down, into *out. False if not a
// UIElement or no Keyboard (no View).
extern "C" bool dm_noesis_ui_element_get_modifiers(void* element, int32_t* out) {
    Noesis::UIElement* ui = as_ui(element);
    if (!ui || !out) return false;
    Noesis::Keyboard* kb = ui->GetKeyboard();
    if (!kb) return false;
    *out = static_cast<int32_t>(kb->GetModifiers());
    return true;
}

// `KeyStates` bitmask for `key`, into *out. False if not a UIElement / no
// Keyboard.
extern "C" bool dm_noesis_ui_element_get_key_states(void* element, int32_t key, int32_t* out) {
    Noesis::UIElement* ui = as_ui(element);
    if (!ui || !out) return false;
    Noesis::Keyboard* kb = ui->GetKeyboard();
    if (!kb) return false;
    *out = static_cast<int32_t>(kb->GetKeyStates(static_cast<Noesis::Key>(key)));
    return true;
}

extern "C" bool dm_noesis_ui_element_is_key_down(void* element, int32_t key) {
    Noesis::UIElement* ui = as_ui(element);
    if (!ui) return false;
    Noesis::Keyboard* kb = ui->GetKeyboard();
    return kb ? kb->IsKeyDown(static_cast<Noesis::Key>(key)) : false;
}

extern "C" bool dm_noesis_ui_element_is_key_up(void* element, int32_t key) {
    Noesis::UIElement* ui = as_ui(element);
    if (!ui) return true;  // an un-attached element has no key held down
    Noesis::Keyboard* kb = ui->GetKeyboard();
    return kb ? kb->IsKeyUp(static_cast<Noesis::Key>(key)) : true;
}

extern "C" bool dm_noesis_ui_element_is_key_toggled(void* element, int32_t key) {
    Noesis::UIElement* ui = as_ui(element);
    if (!ui) return false;
    Noesis::Keyboard* kb = ui->GetKeyboard();
    return kb ? kb->IsKeyToggled(static_cast<Noesis::Key>(key)) : false;
}

// Borrowed `UIElement*` with keyboard focus in `element`'s View, or null.
extern "C" void* dm_noesis_ui_element_get_keyboard_focused(void* element) {
    Noesis::UIElement* ui = as_ui(element);
    if (!ui) return nullptr;
    Noesis::Keyboard* kb = ui->GetKeyboard();
    return kb ? static_cast<void*>(kb->GetFocused()) : nullptr;
}

// ── Focus-state DPs ──────────────────────────────────────────────────────────

extern "C" bool dm_noesis_ui_element_get_is_focused(void* element) {
    Noesis::UIElement* ui = as_ui(element);
    return ui ? ui->GetIsFocused() : false;
}

extern "C" bool dm_noesis_ui_element_get_is_keyboard_focused(void* element) {
    Noesis::UIElement* ui = as_ui(element);
    return ui ? ui->GetIsKeyboardFocused() : false;
}

extern "C" bool dm_noesis_ui_element_get_is_keyboard_focus_within(void* element) {
    Noesis::UIElement* ui = as_ui(element);
    return ui ? ui->GetIsKeyboardFocusWithin() : false;
}

// ── Focus engagement + traversal ─────────────────────────────────────────────

extern "C" bool dm_noesis_ui_element_focus_engage(void* element, bool engage) {
    Noesis::UIElement* ui = as_ui(element);
    return ui ? ui->Focus(engage) : false;
}

extern "C" bool dm_noesis_ui_element_move_focus(void* element, int32_t direction, bool wrapped) {
    Noesis::UIElement* ui = as_ui(element);
    if (!ui) return false;
    Noesis::TraversalRequest request;
    request.focusNavigationDirection = static_cast<Noesis::FocusNavigationDirection>(direction);
    request.wrapped = wrapped;
    return ui->MoveFocus(request);
}

// Borrowed `DependencyObject*` Noesis predicts focus would land on, or null.
extern "C" void* dm_noesis_ui_element_predict_focus(void* element, int32_t direction) {
    Noesis::UIElement* ui = as_ui(element);
    if (!ui) return nullptr;
    return static_cast<void*>(
        ui->PredictFocus(static_cast<Noesis::FocusNavigationDirection>(direction)));
}

// ── FocusManager statics ─────────────────────────────────────────────────────

extern "C" void* dm_noesis_focus_manager_get_focused_element(void* scope) {
    Noesis::DependencyObject* d = as_do(scope);
    return d ? static_cast<void*>(Noesis::FocusManager::GetFocusedElement(d)) : nullptr;
}

extern "C" bool dm_noesis_focus_manager_set_focused_element(void* scope, void* element) {
    Noesis::DependencyObject* d = as_do(scope);
    if (!d) return false;
    // `element` may be null (clear). A non-null value must be a UIElement.
    Noesis::UIElement* ui = element ? as_ui(element) : nullptr;
    if (element && !ui) return false;
    Noesis::FocusManager::SetFocusedElement(d, ui);
    return true;
}

extern "C" bool dm_noesis_focus_manager_get_is_focus_scope(void* element) {
    Noesis::DependencyObject* d = as_do(element);
    return d ? Noesis::FocusManager::GetIsFocusScope(d) : false;
}

extern "C" bool dm_noesis_focus_manager_set_is_focus_scope(void* element, bool value) {
    Noesis::DependencyObject* d = as_do(element);
    if (!d) return false;
    Noesis::FocusManager::SetIsFocusScope(d, value);
    return true;
}

extern "C" void* dm_noesis_focus_manager_get_focus_scope(void* element) {
    Noesis::DependencyObject* d = as_do(element);
    return d ? static_cast<void*>(Noesis::FocusManager::GetFocusScope(d)) : nullptr;
}

// ── KeyboardNavigation attached properties ───────────────────────────────────

extern "C" bool dm_noesis_keyboard_navigation_get_tab_index(void* element, int32_t* out) {
    Noesis::DependencyObject* d = as_do(element);
    if (!d || !out) return false;
    *out = Noesis::KeyboardNavigation::GetTabIndex(d);
    return true;
}

extern "C" bool dm_noesis_keyboard_navigation_set_tab_index(void* element, int32_t value) {
    Noesis::DependencyObject* d = as_do(element);
    if (!d) return false;
    Noesis::KeyboardNavigation::SetTabIndex(d, value);
    return true;
}

extern "C" bool dm_noesis_keyboard_navigation_get_is_tab_stop(void* element, bool* out) {
    Noesis::DependencyObject* d = as_do(element);
    if (!d || !out) return false;
    *out = Noesis::KeyboardNavigation::GetIsTabStop(d);
    return true;
}

extern "C" bool dm_noesis_keyboard_navigation_set_is_tab_stop(void* element, bool value) {
    Noesis::DependencyObject* d = as_do(element);
    if (!d) return false;
    Noesis::KeyboardNavigation::SetIsTabStop(d, value);
    return true;
}

extern "C" bool dm_noesis_keyboard_navigation_get_tab_navigation(void* element, int32_t* out) {
    Noesis::DependencyObject* d = as_do(element);
    if (!d || !out) return false;
    *out = static_cast<int32_t>(Noesis::KeyboardNavigation::GetTabNavigation(d));
    return true;
}

extern "C" bool dm_noesis_keyboard_navigation_set_tab_navigation(void* element, int32_t mode) {
    Noesis::DependencyObject* d = as_do(element);
    if (!d) return false;
    Noesis::KeyboardNavigation::SetTabNavigation(
        d, static_cast<Noesis::KeyboardNavigationMode>(mode));
    return true;
}

extern "C" bool dm_noesis_keyboard_navigation_get_control_tab_navigation(void* element,
                                                                         int32_t* out) {
    Noesis::DependencyObject* d = as_do(element);
    if (!d || !out) return false;
    *out = static_cast<int32_t>(Noesis::KeyboardNavigation::GetControlTabNavigation(d));
    return true;
}

extern "C" bool dm_noesis_keyboard_navigation_set_control_tab_navigation(void* element,
                                                                         int32_t mode) {
    Noesis::DependencyObject* d = as_do(element);
    if (!d) return false;
    Noesis::KeyboardNavigation::SetControlTabNavigation(
        d, static_cast<Noesis::KeyboardNavigationMode>(mode));
    return true;
}

extern "C" bool dm_noesis_keyboard_navigation_get_directional_navigation(void* element,
                                                                         int32_t* out) {
    Noesis::DependencyObject* d = as_do(element);
    if (!d || !out) return false;
    *out = static_cast<int32_t>(Noesis::KeyboardNavigation::GetDirectionalNavigation(d));
    return true;
}

extern "C" bool dm_noesis_keyboard_navigation_set_directional_navigation(void* element,
                                                                         int32_t mode) {
    Noesis::DependencyObject* d = as_do(element);
    if (!d) return false;
    Noesis::KeyboardNavigation::SetDirectionalNavigation(
        d, static_cast<Noesis::KeyboardNavigationMode>(mode));
    return true;
}

extern "C" bool dm_noesis_keyboard_navigation_get_accepts_return(void* element, bool* out) {
    Noesis::DependencyObject* d = as_do(element);
    if (!d || !out) return false;
    *out = Noesis::KeyboardNavigation::GetAcceptsReturn(d);
    return true;
}

extern "C" bool dm_noesis_keyboard_navigation_set_accepts_return(void* element, bool value) {
    Noesis::DependencyObject* d = as_do(element);
    if (!d) return false;
    Noesis::KeyboardNavigation::SetAcceptsReturn(d, value);
    return true;
}

// ── Input gestures + bindings ────────────────────────────────────────────────
//
// Each create returns a fresh object at +1 (Noesis `new` yields refcount 1,
// like `new RoutedCommand` in noesis_commands.cpp); Rust releases it on drop.
// `add_input_binding` hands the binding to the element's InputBindingCollection,
// which adds its own reference.

extern "C" void* dm_noesis_key_gesture_create(int32_t key, int32_t modifiers) {
    auto* g = new Noesis::KeyGesture(static_cast<Noesis::Key>(key),
                                     static_cast<Noesis::ModifierKeys>(modifiers));
    return static_cast<void*>(g);
}

extern "C" void* dm_noesis_mouse_gesture_create(int32_t action, int32_t modifiers) {
    auto* g = new Noesis::MouseGesture(static_cast<Noesis::MouseAction>(action),
                                       static_cast<Noesis::ModifierKeys>(modifiers));
    return static_cast<void*>(g);
}

extern "C" void* dm_noesis_key_binding_create(void* command, int32_t key, int32_t modifiers) {
    auto* cmd = Noesis::DynamicCast<Noesis::ICommand*>(
        static_cast<Noesis::BaseComponent*>(command));
    if (!cmd) return nullptr;
    auto* b = new Noesis::KeyBinding(cmd, static_cast<Noesis::Key>(key),
                                     static_cast<Noesis::ModifierKeys>(modifiers));
    return static_cast<void*>(static_cast<Noesis::InputBinding*>(b));
}

extern "C" void* dm_noesis_mouse_binding_create(void* command, int32_t action, int32_t modifiers) {
    auto* cmd = Noesis::DynamicCast<Noesis::ICommand*>(
        static_cast<Noesis::BaseComponent*>(command));
    if (!cmd) return nullptr;
    auto* b = new Noesis::MouseBinding(cmd, static_cast<Noesis::MouseAction>(action),
                                       static_cast<Noesis::ModifierKeys>(modifiers));
    return static_cast<void*>(static_cast<Noesis::InputBinding*>(b));
}

extern "C" void* dm_noesis_input_binding_create(void* command, void* gesture) {
    auto* cmd = Noesis::DynamicCast<Noesis::ICommand*>(
        static_cast<Noesis::BaseComponent*>(command));
    auto* g = Noesis::DynamicCast<Noesis::InputGesture*>(
        static_cast<Noesis::BaseComponent*>(gesture));
    if (!cmd || !g) return nullptr;
    auto* b = new Noesis::InputBinding(cmd, g);
    return static_cast<void*>(b);
}

extern "C" bool dm_noesis_ui_element_add_input_binding(void* element, void* binding) {
    Noesis::UIElement* ui = as_ui(element);
    auto* b = Noesis::DynamicCast<Noesis::InputBinding*>(
        static_cast<Noesis::BaseComponent*>(binding));
    if (!ui || !b) return false;
    ui->GetInputBindings()->Add(b);
    return true;
}
