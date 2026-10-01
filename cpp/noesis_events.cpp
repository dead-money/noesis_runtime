// FrameworkElement lookup and helpers, and event subscription.
//
//   * find_name / register_name, visibility, margin, text get/set, focus,
//     Path polyline data and polygon clips.
//   * Typed subscriptions: Button Click, KeyDown, Selector SelectionChanged.
//   * Generic routed-event subscription by name, with typed arg accessors
//     keyed on an arg-kind discriminant.
//   * DragDrop source side, DataObject Copying / Pasting handlers.
//   * Non-routed lifecycle events (Initialized, LayoutUpdated, Is*Changed).
//
// Every subscription returns a heap handler as its token. The handler holds a
// +1 ref on the element, so the subscription stays valid after the caller
// drops its other handles, and owns the Rust userdata box (see
// RustSubscription).

#include "noesis_shim.h"

#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsCore/Delegate.h>
#include <NsCore/DynamicCast.h>
#include <NsGui/BaseButton.h>
#include <NsGui/DataObject.h>
#include <NsGui/DependencyObject.h>
#include <NsGui/DragDrop.h>
#include <NsGui/Enums.h>
#include <NsGui/Events.h>
#include <NsGui/FrameworkElement.h>
#include <NsGui/Path.h>
#include <NsGui/RoutedEvent.h>
#include <NsGui/Selector.h>
#include <NsGui/StreamGeometry.h>
#include <NsGui/StreamGeometryContext.h>
#include <NsGui/TextBlock.h>
#include <NsGui/TextBox.h>
#include <NsGui/UIElement.h>
#include <NsGui/UIElementEvents.h>
#include <NsCore/Symbol.h>
#include <NsDrawing/Point.h>
#include <NsDrawing/Size.h>
#include <NsDrawing/Thickness.h>

#include <string.h>

namespace {

// Shared teardown for every Rust-owned subscription in this file.
//
//   * Donated userdata. Rust hands over its handler box plus a free fn and
//     forgets it. The box is freed exactly once, in the destructor. Rust's Drop
//     only calls unsubscribe, so the box can't be freed while a callback still
//     borrows it.
//
//   * Unsubscribe from inside the handler's own callback. Deleting there would
//     leave the running callback's `this` dangling. Handlers bracket the
//     callback with enterDispatch() / leaveDispatch(); unsubscribe calls
//     deferDeleteIfDispatching(), and the outermost dispatch frame does
//     `delete this` as it unwinds. Detaching the Noesis delegate (`-=`) during
//     its own dispatch is already safe: MultiDelegate nulls the slot and defers
//     compaction until invoke nesting reaches zero.
//
// Dispatch depth is a counter, not a bool: a callback can re-raise its own
// event, and only the outermost frame may delete. Plain counter, no atomics:
// everything runs on the single view thread.
//
// Non-virtual destructor: every unsubscribe path deletes through the concrete
// handler type.
class RustSubscription {
public:
    // True: a callback is on the stack and deletion is deferred; the caller
    // must not delete. False: the caller deletes now.
    bool deferDeleteIfDispatching() {
        if (mDispatchDepth > 0) {
            mPendingDelete = true;
            return true;
        }
        return false;
    }

    // Hands the box back to Rust so the destructor won't free it. For a
    // subscribe that fails after construction and returns NULL.
    void abandonDonation() {
        mUserdata = nullptr;
        mFree = nullptr;
    }

protected:
    RustSubscription(void* userdata, noesis_subscription_free_fn free)
        : mUserdata(userdata), mFree(free) {}

    ~RustSubscription() {
        if (mFree && mUserdata) {
            mFree(mUserdata);
        }
    }

    void enterDispatch() { mDispatchDepth++; }

    // True when this is the outermost frame and an unsubscribe arrived during
    // dispatch; the caller must then `delete this`.
    bool leaveDispatch() { return (--mDispatchDepth == 0) && mPendingDelete; }

    void* mUserdata;
    noesis_subscription_free_fn mFree;
    uint32_t mDispatchDepth = 0;
    bool mPendingDelete = false;
};

// Click handler. Holds a +1 ref on the button; paired `Click() +=` on
// subscribe and `Click() -=` on unsubscribe.
class RustClickHandler final: public RustSubscription {
public:
    RustClickHandler(noesis_click_fn cb, void* userdata,
                     noesis_subscription_free_fn free, Noesis::BaseButton* button)
        : RustSubscription(userdata, free), mCb(cb), mButton(button)
    {
        if (mButton) {
            mButton->AddReference();
        }
    }

    ~RustClickHandler() {
        if (mButton) {
            mButton->Release();
        }
    }

    RustClickHandler(const RustClickHandler&) = delete;
    RustClickHandler& operator=(const RustClickHandler&) = delete;

    void OnClick(Noesis::BaseComponent* /*sender*/, const Noesis::RoutedEventArgs& /*args*/) {
        enterDispatch();
        if (mCb) {
            mCb(mUserdata);
        }
        if (leaveDispatch()) {
            delete this;  // deferred teardown from an unsubscribe during dispatch
        }
    }

    Noesis::BaseButton* button() const { return mButton; }

private:
    noesis_click_fn mCb;
    Noesis::BaseButton* mButton;
};

}  // namespace

extern "C" void* noesis_framework_element_find_name(void* element, const char* name) {
    if (!element || !name) return nullptr;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    Noesis::BaseComponent* found = fe->FindName(name);
    if (!found) return nullptr;
    // FindName's result is borrowed; hand out +1 for noesis_base_component_release
    auto* result = Noesis::DynamicCast<Noesis::FrameworkElement*>(found);
    if (!result) return nullptr;
    result->AddReference();
    return result;
}

// Register / unregister an x:Name in the namescope hosting `element`.
// `object` is borrowed; the scope takes its own reference. Returns false if
// `element` is not a FrameworkElement. The element must live within a namescope
// (the XAML root hosts one); registering a name already present updates it.
extern "C" bool noesis_framework_element_register_name(
    void* element, const char* name, void* object) {
    if (!element || !name) return false;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return false;
    fe->RegisterName(name, static_cast<Noesis::BaseComponent*>(object));
    return true;
}

extern "C" bool noesis_framework_element_unregister_name(void* element, const char* name) {
    if (!element || !name) return false;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return false;
    fe->UnregisterName(name);
    return true;
}

extern "C" const char* noesis_framework_element_get_name(void* element) {
    if (!element) return nullptr;
    return static_cast<Noesis::FrameworkElement*>(element)->GetName();
}

extern "C" void noesis_framework_element_set_visibility(void* element, bool visible) {
    if (!element) return;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    fe->SetVisibility(visible ? Noesis::Visibility_Visible : Noesis::Visibility_Collapsed);
}

extern "C" void noesis_framework_element_set_margin(
    void* element, float left, float top, float right, float bottom)
{
    if (!element) return;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    fe->SetMargin(Noesis::Thickness(left, top, right, bottom));
}

extern "C" void* noesis_subscribe_click(
    void* element, noesis_click_fn cb, void* userdata, noesis_subscription_free_fn free_handler)
{
    if (!element || !cb) return nullptr;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    auto* button = Noesis::DynamicCast<Noesis::BaseButton*>(fe);
    if (!button) return nullptr;

    auto* handler = new RustClickHandler(cb, userdata, free_handler, button);
    button->Click() += Noesis::MakeDelegate(handler, &RustClickHandler::OnClick);
    return handler;
}

extern "C" void noesis_unsubscribe_click(void* token) {
    if (!token) return;
    auto* handler = static_cast<RustClickHandler*>(token);
    if (auto* button = handler->button()) {
        button->Click() -= Noesis::MakeDelegate(handler, &RustClickHandler::OnClick);
    }
    if (handler->deferDeleteIfDispatching()) return;
    delete handler;
}

// ── KeyDown subscription ───────────────────────────────────────────────────

namespace {

// KeyDown handler. Holds a +1 ref on the element. The callback's `handled`
// out-param stops further routing, e.g. so a hotkey isn't also typed into the
// focused TextBox.
class RustKeyDownHandler final: public RustSubscription {
public:
    RustKeyDownHandler(noesis_keydown_fn cb, void* userdata,
                       noesis_subscription_free_fn free, Noesis::UIElement* element)
        : RustSubscription(userdata, free), mCb(cb), mElement(element)
    {
        if (mElement) {
            mElement->AddReference();
        }
    }

    ~RustKeyDownHandler() {
        if (mElement) {
            mElement->Release();
        }
    }

    RustKeyDownHandler(const RustKeyDownHandler&) = delete;
    RustKeyDownHandler& operator=(const RustKeyDownHandler&) = delete;

    void OnKeyDown(Noesis::BaseComponent* /*sender*/, const Noesis::KeyEventArgs& args) {
        enterDispatch();
        if (mCb) {
            bool handled = false;
            mCb(mUserdata, static_cast<int32_t>(args.key), &handled);
            // RoutedEventArgs::handled is `mutable`
            if (handled) {
                args.handled = true;
            }
        }
        if (leaveDispatch()) {
            delete this;  // deferred teardown from an unsubscribe during dispatch
        }
    }

    Noesis::UIElement* element() const { return mElement; }

private:
    noesis_keydown_fn mCb;
    Noesis::UIElement* mElement;
};

}  // namespace

extern "C" void* noesis_subscribe_keydown(
    void* element, noesis_keydown_fn cb, void* userdata, noesis_subscription_free_fn free_handler)
{
    if (!element || !cb) return nullptr;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    auto* uie = Noesis::DynamicCast<Noesis::UIElement*>(fe);
    if (!uie) return nullptr;

    auto* handler = new RustKeyDownHandler(cb, userdata, free_handler, uie);
    uie->KeyDown() += Noesis::MakeDelegate(handler, &RustKeyDownHandler::OnKeyDown);
    return handler;
}

extern "C" void noesis_unsubscribe_keydown(void* token) {
    if (!token) return;
    auto* handler = static_cast<RustKeyDownHandler*>(token);
    if (auto* uie = handler->element()) {
        uie->KeyDown() -= Noesis::MakeDelegate(handler, &RustKeyDownHandler::OnKeyDown);
    }
    if (handler->deferDeleteIfDispatching()) return;
    delete handler;
}

// ── Selector.SelectionChanged subscription ─────────────────────────────────

namespace {

// SelectionChanged handler. Holds a +1 ref on the Selector.
class RustSelectionChangedHandler final: public RustSubscription {
public:
    RustSelectionChangedHandler(noesis_selection_changed_fn cb, void* userdata,
                                noesis_subscription_free_fn free, Noesis::Selector* selector)
        : RustSubscription(userdata, free), mCb(cb), mSelector(selector)
    {
        if (mSelector) {
            mSelector->AddReference();
        }
    }

    ~RustSelectionChangedHandler() {
        if (mSelector) {
            mSelector->Release();
        }
    }

    RustSelectionChangedHandler(const RustSelectionChangedHandler&) = delete;
    RustSelectionChangedHandler& operator=(const RustSelectionChangedHandler&) = delete;

    void OnSelectionChanged(Noesis::BaseComponent* /*sender*/,
                            const Noesis::SelectionChangedEventArgs& /*args*/) {
        enterDispatch();
        if (mCb) {
            mCb(mUserdata);
        }
        if (leaveDispatch()) {
            delete this;  // deferred teardown from an unsubscribe during dispatch
        }
    }

    Noesis::Selector* selector() const { return mSelector; }

private:
    noesis_selection_changed_fn mCb;
    Noesis::Selector* mSelector;
};

}  // namespace

// Subscribe `cb` to `Selector::SelectionChanged` on `element` (a Selector:
// ListBox / ListView / ComboBox / TabControl / ...). Returns an opaque token
// (release via noesis_unsubscribe_selection_changed) or NULL when `element` is
// not a Selector / `cb` is null.
extern "C" void* noesis_subscribe_selection_changed(
    void* element, noesis_selection_changed_fn cb, void* userdata,
    noesis_subscription_free_fn free_handler)
{
    if (!element || !cb) return nullptr;
    auto* selector = Noesis::DynamicCast<Noesis::Selector*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!selector) return nullptr;

    auto* handler = new RustSelectionChangedHandler(cb, userdata, free_handler, selector);
    selector->SelectionChanged() +=
        Noesis::MakeDelegate(handler, &RustSelectionChangedHandler::OnSelectionChanged);
    return handler;
}

extern "C" void noesis_unsubscribe_selection_changed(void* token) {
    if (!token) return;
    auto* handler = static_cast<RustSelectionChangedHandler*>(token);
    if (auto* selector = handler->selector()) {
        selector->SelectionChanged() -=
            Noesis::MakeDelegate(handler, &RustSelectionChangedHandler::OnSelectionChanged);
    }
    if (handler->deferDeleteIfDispatching()) return;
    delete handler;
}

// ── Text get/set + caret + focus ───────────────────────────────────────────

extern "C" const char* noesis_text_get(void* element) {
    if (!element) return nullptr;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    if (auto* tb = Noesis::DynamicCast<Noesis::TextBox*>(fe)) {
        return tb->GetText();
    }
    if (auto* tbk = Noesis::DynamicCast<Noesis::TextBlock*>(fe)) {
        return tbk->GetText();
    }
    return nullptr;
}

extern "C" bool noesis_text_set(void* element, const char* text) {
    if (!element) return false;
    const char* safe = text ? text : "";
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    if (auto* tb = Noesis::DynamicCast<Noesis::TextBox*>(fe)) {
        tb->SetText(safe);
        return true;
    }
    if (auto* tbk = Noesis::DynamicCast<Noesis::TextBlock*>(fe)) {
        tbk->SetText(safe);
        return true;
    }
    return false;
}

extern "C" bool noesis_text_caret_to_end(void* element) {
    if (!element) return false;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    auto* tb = Noesis::DynamicCast<Noesis::TextBox*>(fe);
    if (!tb) return false;
    const char* current = tb->GetText();
    const int32_t len = current ? static_cast<int32_t>(strlen(current)) : 0;
    tb->SetCaretIndex(len);
    return true;
}

extern "C" bool noesis_focus_element(void* element) {
    if (!element) return false;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    auto* uie = Noesis::DynamicCast<Noesis::UIElement*>(fe);
    if (!uie) return false;
    return uie->Focus();
}

// Sets a Path's Data to one open, unfilled polyline through `count` (x, y)
// pairs (`xy` holds `2*count` floats, in the Path's local space). Builds a new
// StreamGeometry per call. Returns false if `element` is not a Path, `xy` is
// null, or `count < 2`.
extern "C" bool noesis_path_set_points(void* element, const float* xy, uint32_t count) {
    if (!element || !xy || count < 2) return false;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    auto* path = Noesis::DynamicCast<Noesis::Path*>(fe);
    if (!path) return false;

    Noesis::Ptr<Noesis::StreamGeometry> geometry = Noesis::MakePtr<Noesis::StreamGeometry>();
    {
        Noesis::StreamGeometryContext ctx = geometry->Open();
        ctx.BeginFigure(Noesis::Point(xy[0], xy[1]), false /* open, not filled */);
        for (uint32_t i = 1; i < count; ++i) {
            ctx.LineTo(Noesis::Point(xy[2 * i], xy[2 * i + 1]));
        }
        ctx.Close();
    }
    path->SetData(geometry);
    return true;
}

// Sets an element's Clip to a closed polygon through `count` (x, y) pairs
// (`2*count` floats, in the element's own space). `count == 0` clears the
// clip. Returns false on a null element, a null `xy`, or a `count` of 1 or 2.
extern "C" bool noesis_element_set_clip_points(void* element, const float* xy, uint32_t count) {
    if (!element) return false;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    if (count == 0) {
        fe->SetClip(nullptr);
        return true;
    }
    if (!xy || count < 3) return false;

    Noesis::Ptr<Noesis::StreamGeometry> geometry = Noesis::MakePtr<Noesis::StreamGeometry>();
    {
        Noesis::StreamGeometryContext ctx = geometry->Open();
        ctx.BeginFigure(Noesis::Point(xy[0], xy[1]), true /* closed, filled region */);
        for (uint32_t i = 1; i < count; ++i) {
            ctx.LineTo(Noesis::Point(xy[2 * i], xy[2 * i + 1]));
        }
        ctx.Close();
    }
    fe->SetClip(geometry);
    return true;
}

// ── Generic routed-event subscription ───────────────────────────────────────
//
// Covers any routed event by name. Two SDK facts make one handler work:
//
//   1. UIElement::AddHandler takes the generic RoutedEventHandler
//      (`Delegate<void(BaseComponent*, const RoutedEventArgs&)>`), and every
//      typed RoutedEvent_<T> in UIElement.h reinterpret_casts its handler to
//      that delegate. One OnEvent signature is ABI-correct for every event.
//
//   2. The arg structs (MouseEventArgs, KeyEventArgs, ...) have no reflection,
//      so DynamicCast can't downcast them. The arg shape is classified from the
//      subscribed event (kEvents) and carried as `kind` in DmEventArgs. The
//      accessors static_cast on that kind (single non-virtual inheritance, zero
//      offset) and return a sentinel on a mismatch.
//
// handledEventsToo: this SDK's AddHandler has no third parameter, so
// already-handled events are never re-delivered along the route. `handled_too`
// only applies within one element's multicast delegate: when false, the
// callback is skipped if an earlier handler on the same element set `handled`.

namespace {

// Arg-shape discriminant. Must match `arg_kind` in src/events.rs.
enum DmArgKind : int32_t {
    ARG_ROUTED       = 0,  // RoutedEventArgs (source + handled only)
    ARG_MOUSE        = 1,  // MouseEventArgs (position)
    ARG_MOUSE_BUTTON = 2,  // MouseButtonEventArgs (position + button)
    ARG_MOUSE_WHEEL  = 3,  // MouseWheelEventArgs (position + wheel delta)
    ARG_KEY          = 4,  // KeyEventArgs (key)
    ARG_SIZE_CHANGED = 5,  // SizeChangedEventArgs (new/previous size)
    ARG_TEXT_INPUT   = 6,  // TextCompositionEventArgs (ch)
    ARG_FOCUS_CHANGED = 7, // KeyboardFocusChangedEventArgs (old/new focus)
    ARG_DRAG          = 8, // DragEventArgs (effects/allowed/keyStates/data/position)
    ARG_MANIP_STARTED   = 9,  // ManipulationStartedEventArgs (origin)
    ARG_MANIP_DELTA     = 10, // ManipulationDeltaEventArgs (delta/cumulative/velocities)
    ARG_MANIP_COMPLETED = 11, // ManipulationCompletedEventArgs (total/finalVelocities)
    ARG_MANIP_INERTIA   = 12, // ManipulationInertiaStartingEventArgs (initialVelocities)
};

// The opaque payload the callback receives. `args` always points at the live
// `RoutedEventArgs` (or a derived struct, selected by `kind`) on the C++
// stack; it is valid only for the duration of the callback.
struct DmEventArgs {
    int32_t kind;
    const Noesis::RoutedEventArgs* args;
};

// Name -> (RoutedEvent* static, arg kind). Stores the statics' addresses and
// dereferences at lookup time: Noesis fills them during init. Events not
// listed here get ARG_ROUTED (source + handled only).
struct EventEntry {
    const char* name;
    const Noesis::RoutedEvent* const* slot;
    int32_t kind;
};

const EventEntry kEvents[] = {
    // Mouse (position only)
    {"MouseEnter",        &Noesis::UIElement::MouseEnterEvent,                 ARG_MOUSE},
    {"MouseLeave",        &Noesis::UIElement::MouseLeaveEvent,                 ARG_MOUSE},
    {"MouseMove",         &Noesis::UIElement::MouseMoveEvent,                  ARG_MOUSE},
    {"PreviewMouseMove",  &Noesis::UIElement::PreviewMouseMoveEvent,           ARG_MOUSE},
    {"GotMouseCapture",   &Noesis::UIElement::GotMouseCaptureEvent,            ARG_MOUSE},
    {"LostMouseCapture",  &Noesis::UIElement::LostMouseCaptureEvent,           ARG_MOUSE},
    // Mouse buttons (position + changedButton)
    {"MouseDown",                  &Noesis::UIElement::MouseDownEvent,                 ARG_MOUSE_BUTTON},
    {"MouseUp",                    &Noesis::UIElement::MouseUpEvent,                   ARG_MOUSE_BUTTON},
    {"MouseLeftButtonDown",        &Noesis::UIElement::MouseLeftButtonDownEvent,       ARG_MOUSE_BUTTON},
    {"MouseLeftButtonUp",          &Noesis::UIElement::MouseLeftButtonUpEvent,         ARG_MOUSE_BUTTON},
    {"MouseRightButtonDown",       &Noesis::UIElement::MouseRightButtonDownEvent,      ARG_MOUSE_BUTTON},
    {"MouseRightButtonUp",         &Noesis::UIElement::MouseRightButtonUpEvent,        ARG_MOUSE_BUTTON},
    {"PreviewMouseDown",           &Noesis::UIElement::PreviewMouseDownEvent,          ARG_MOUSE_BUTTON},
    {"PreviewMouseUp",             &Noesis::UIElement::PreviewMouseUpEvent,            ARG_MOUSE_BUTTON},
    {"PreviewMouseLeftButtonDown", &Noesis::UIElement::PreviewMouseLeftButtonDownEvent, ARG_MOUSE_BUTTON},
    {"PreviewMouseLeftButtonUp",   &Noesis::UIElement::PreviewMouseLeftButtonUpEvent,  ARG_MOUSE_BUTTON},
    {"PreviewMouseRightButtonDown",&Noesis::UIElement::PreviewMouseRightButtonDownEvent,ARG_MOUSE_BUTTON},
    {"PreviewMouseRightButtonUp",  &Noesis::UIElement::PreviewMouseRightButtonUpEvent, ARG_MOUSE_BUTTON},
    // Mouse wheel (position + wheelRotation)
    {"MouseWheel",        &Noesis::UIElement::MouseWheelEvent,                 ARG_MOUSE_WHEEL},
    {"PreviewMouseWheel", &Noesis::UIElement::PreviewMouseWheelEvent,          ARG_MOUSE_WHEEL},
    // Keyboard (key)
    {"KeyDown",        &Noesis::UIElement::KeyDownEvent,            ARG_KEY},
    {"KeyUp",          &Noesis::UIElement::KeyUpEvent,              ARG_KEY},
    {"PreviewKeyDown", &Noesis::UIElement::PreviewKeyDownEvent,     ARG_KEY},
    {"PreviewKeyUp",   &Noesis::UIElement::PreviewKeyUpEvent,       ARG_KEY},
    // Text input (ch)
    {"TextInput",        &Noesis::UIElement::TextInputEvent,        ARG_TEXT_INPUT},
    {"PreviewTextInput", &Noesis::UIElement::PreviewTextInputEvent, ARG_TEXT_INPUT},
    // Focus: GotFocus/LostFocus carry base routed args; the keyboard-focus
    // variants downcast to KeyboardFocusChangedEventArgs (old/new focus).
    {"GotFocus",                 &Noesis::UIElement::GotFocusEvent,                 ARG_ROUTED},
    {"LostFocus",                &Noesis::UIElement::LostFocusEvent,                ARG_ROUTED},
    {"GotKeyboardFocus",         &Noesis::UIElement::GotKeyboardFocusEvent,         ARG_FOCUS_CHANGED},
    {"LostKeyboardFocus",        &Noesis::UIElement::LostKeyboardFocusEvent,        ARG_FOCUS_CHANGED},
    {"PreviewGotKeyboardFocus",  &Noesis::UIElement::PreviewGotKeyboardFocusEvent,  ARG_FOCUS_CHANGED},
    {"PreviewLostKeyboardFocus", &Noesis::UIElement::PreviewLostKeyboardFocusEvent, ARG_FOCUS_CHANGED},
    // Lifecycle
    {"Loaded",      &Noesis::FrameworkElement::LoadedEvent,      ARG_ROUTED},
    {"Unloaded",    &Noesis::FrameworkElement::UnloadedEvent,    ARG_ROUTED},
    {"SizeChanged", &Noesis::FrameworkElement::SizeChangedEvent, ARG_SIZE_CHANGED},
    // Touch (base routed args)
    {"TouchDown",             &Noesis::UIElement::TouchDownEvent,             ARG_ROUTED},
    {"TouchMove",             &Noesis::UIElement::TouchMoveEvent,             ARG_ROUTED},
    {"TouchUp",               &Noesis::UIElement::TouchUpEvent,               ARG_ROUTED},
    {"TouchEnter",            &Noesis::UIElement::TouchEnterEvent,            ARG_ROUTED},
    {"TouchLeave",            &Noesis::UIElement::TouchLeaveEvent,            ARG_ROUTED},
    {"Tapped",                &Noesis::UIElement::TappedEvent,                ARG_ROUTED},
    {"DoubleTapped",          &Noesis::UIElement::DoubleTappedEvent,          ARG_ROUTED},
    {"Holding",               &Noesis::UIElement::HoldingEvent,               ARG_ROUTED},
    {"RightTapped",           &Noesis::UIElement::RightTappedEvent,           ARG_ROUTED},
    // Manipulation: Starting carries only mode/container (base args); the rest
    // downcast to their typed args (origin / delta / velocities / inertia).
    {"ManipulationStarting",         &Noesis::UIElement::ManipulationStartingEvent,         ARG_ROUTED},
    {"ManipulationStarted",          &Noesis::UIElement::ManipulationStartedEvent,          ARG_MANIP_STARTED},
    {"ManipulationDelta",            &Noesis::UIElement::ManipulationDeltaEvent,            ARG_MANIP_DELTA},
    {"ManipulationInertiaStarting",  &Noesis::UIElement::ManipulationInertiaStartingEvent,  ARG_MANIP_INERTIA},
    {"ManipulationCompleted",        &Noesis::UIElement::ManipulationCompletedEvent,        ARG_MANIP_COMPLETED},
    // Drag/drop: DragEventArgs. QueryContinueDrag and GiveFeedback carry other
    // arg types and resolve through the generic fallback as ARG_ROUTED.
    {"DragEnter",        &Noesis::UIElement::DragEnterEvent,        ARG_DRAG},
    {"DragOver",         &Noesis::UIElement::DragOverEvent,         ARG_DRAG},
    {"DragLeave",        &Noesis::UIElement::DragLeaveEvent,        ARG_DRAG},
    {"Drop",             &Noesis::UIElement::DropEvent,             ARG_DRAG},
    {"PreviewDragEnter", &Noesis::UIElement::PreviewDragEnterEvent, ARG_DRAG},
    {"PreviewDragOver",  &Noesis::UIElement::PreviewDragOverEvent,  ARG_DRAG},
    {"PreviewDragLeave", &Noesis::UIElement::PreviewDragLeaveEvent, ARG_DRAG},
    {"PreviewDrop",      &Noesis::UIElement::PreviewDropEvent,      ARG_DRAG},
};

// Resolves `name` through kEvents, then through FindRoutedEvent on the
// element's class hierarchy (reported as ARG_ROUTED). nullptr if neither
// knows the name.
const Noesis::RoutedEvent* LookupEvent(
    Noesis::UIElement* element, const char* name, int32_t& outKind)
{
    for (const EventEntry& e : kEvents) {
        if (strcmp(e.name, name) == 0) {
            outKind = e.kind;
            return *e.slot;
        }
    }
    Noesis::Symbol sym(name, Noesis::Symbol::NullIfNotFound());
    if (sym.IsNull()) return nullptr;
    const Noesis::RoutedEvent* ev = Noesis::FindRoutedEvent(element->GetClassType(), sym);
    if (ev) {
        outKind = ARG_ROUTED;
    }
    return ev;
}

// Generic routed-event handler. Holds a +1 ref on the element and the
// RoutedEvent it registered against, for the matching RemoveHandler.
class RustRoutedHandler final: public RustSubscription {
public:
    RustRoutedHandler(noesis_routed_event_fn cb, void* userdata,
        noesis_subscription_free_fn free, Noesis::UIElement* element,
        const Noesis::RoutedEvent* ev, int32_t kind, bool handledToo)
        : RustSubscription(userdata, free), mCb(cb), mElement(element), mEvent(ev), mKind(kind),
          mHandledToo(handledToo)
    {
        if (mElement) {
            mElement->AddReference();
        }
    }

    ~RustRoutedHandler() {
        if (mElement) {
            mElement->Release();
        }
    }

    RustRoutedHandler(const RustRoutedHandler&) = delete;
    RustRoutedHandler& operator=(const RustRoutedHandler&) = delete;

    void OnEvent(Noesis::BaseComponent* /*sender*/, const Noesis::RoutedEventArgs& args) {
        // returns before enterDispatch: no callback, so no re-entrant unsubscribe
        if (!mCb || (!mHandledToo && args.handled)) return;

        enterDispatch();
        DmEventArgs wrap{mKind, &args};
        bool handled = args.handled;
        mCb(mUserdata, &wrap, &handled);
        // RoutedEventArgs::handled is `mutable`
        if (handled) {
            args.handled = true;
        }
        if (leaveDispatch()) {
            delete this;  // deferred teardown from an unsubscribe during dispatch
        }
    }

    Noesis::UIElement* element() const { return mElement; }
    const Noesis::RoutedEvent* event() const { return mEvent; }

private:
    noesis_routed_event_fn mCb;
    Noesis::UIElement* mElement;
    const Noesis::RoutedEvent* mEvent;
    int32_t mKind;
    bool mHandledToo;
};

}  // namespace

extern "C" void* noesis_subscribe_event(
    void* element, const char* event_name, bool handled_too, noesis_routed_event_fn cb,
    void* userdata, noesis_subscription_free_fn free_handler)
{
    if (!element || !event_name || !cb) return nullptr;
    auto* uie = Noesis::DynamicCast<Noesis::UIElement*>(static_cast<Noesis::BaseComponent*>(element));
    if (!uie) return nullptr;

    int32_t kind = ARG_ROUTED;
    const Noesis::RoutedEvent* ev = LookupEvent(uie, event_name, kind);
    if (!ev) return nullptr;

    auto* handler = new RustRoutedHandler(cb, userdata, free_handler, uie, ev, kind, handled_too);
    uie->AddHandler(ev, Noesis::MakeDelegate(handler, &RustRoutedHandler::OnEvent));
    return handler;
}

extern "C" void noesis_unsubscribe_event(void* token) {
    if (!token) return;
    auto* handler = static_cast<RustRoutedHandler*>(token);
    if (auto* uie = handler->element()) {
        uie->RemoveHandler(handler->event(),
            Noesis::MakeDelegate(handler, &RustRoutedHandler::OnEvent));
    }
    if (handler->deferDeleteIfDispatching()) return;
    delete handler;
}

// ── Event-arg accessors ─────────────────────────────────────────────────────
//
// Each takes the opaque `args` the callback received (a DmEventArgs*) and
// returns a sentinel (false / -1 / 0) when its `kind` doesn't match. Valid only
// during the callback.

extern "C" bool noesis_mouse_args_position(const void* args, float* x, float* y) {
    if (!args) return false;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != ARG_MOUSE && w->kind != ARG_MOUSE_BUTTON &&
        w->kind != ARG_MOUSE_WHEEL) {
        return false;
    }
    auto* m = static_cast<const Noesis::MouseEventArgs*>(w->args);
    if (x) *x = m->position.x;
    if (y) *y = m->position.y;
    return true;
}

extern "C" int32_t noesis_mouse_button_args_button(const void* args) {
    if (!args) return -1;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != ARG_MOUSE_BUTTON) return -1;
    auto* m = static_cast<const Noesis::MouseButtonEventArgs*>(w->args);
    return static_cast<int32_t>(m->changedButton);
}

extern "C" int32_t noesis_mouse_wheel_args_delta(const void* args) {
    if (!args) return 0;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != ARG_MOUSE_WHEEL) return 0;
    auto* m = static_cast<const Noesis::MouseWheelEventArgs*>(w->args);
    return static_cast<int32_t>(m->wheelRotation);
}

extern "C" int32_t noesis_key_args_key(const void* args) {
    if (!args) return -1;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != ARG_KEY) return -1;
    auto* k = static_cast<const Noesis::KeyEventArgs*>(w->args);
    return static_cast<int32_t>(k->key);
}

extern "C" int32_t noesis_text_args_ch(const void* args) {
    if (!args) return -1;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != ARG_TEXT_INPUT) return -1;
    auto* t = static_cast<const Noesis::TextCompositionEventArgs*>(w->args);
    return static_cast<int32_t>(t->ch);
}

extern "C" bool noesis_size_changed_args_new_size(const void* args, float* width, float* height) {
    if (!args) return false;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != ARG_SIZE_CHANGED) return false;
    auto* s = static_cast<const Noesis::SizeChangedEventArgs*>(w->args);
    if (width) *width = s->newSize.width;
    if (height) *height = s->newSize.height;
    return true;
}

// Borrowed pointer to the event's originating element (`RoutedEventArgs::source`).
// Returns NULL if `args` is null or the source is null. Not ref-counted; do
// not release; valid only for the callback's duration.
extern "C" void* noesis_routed_args_source(const void* args) {
    if (!args) return nullptr;
    auto* w = static_cast<const DmEventArgs*>(args);
    return w->args ? w->args->source : nullptr;
}

// The DmArgKind carried by `args`, or -1 if `args` is null. Classify on this,
// not on which accessor succeeds: the accessors share sentinels (a MouseMove
// and a zero-delta MouseWheel read the same).
extern "C" int32_t noesis_event_args_kind(const void* args) {
    if (!args) return -1;
    return static_cast<const DmEventArgs*>(args)->kind;
}

// ── Typed arg accessors: focus / drag / manipulation ────────────────────────
//
// Same contract as above. Returned element pointers are borrowed, not
// ref-counted, and valid only during the callback.

// KeyboardFocusChangedEventArgs::oldFocus: element that previously had focus.
// Borrowed UIElement* (may be null even on a real focus event). Returns null
// when the args are not a keyboard-focus event.
extern "C" void* noesis_routed_events_focus_old(const void* args) {
    if (!args) return nullptr;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != ARG_FOCUS_CHANGED) return nullptr;
    auto* f = static_cast<const Noesis::KeyboardFocusChangedEventArgs*>(w->args);
    return f->oldFocus;
}

// KeyboardFocusChangedEventArgs::newFocus: element focus moved to. Borrowed
// UIElement*. Returns null when the args are not a keyboard-focus event.
extern "C" void* noesis_routed_events_focus_new(const void* args) {
    if (!args) return nullptr;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != ARG_FOCUS_CHANGED) return nullptr;
    auto* f = static_cast<const Noesis::KeyboardFocusChangedEventArgs*>(w->args);
    return f->newFocus;
}

// DragEventArgs effects/allowedEffects/keyStates bitmasks (DragDropEffects /
// DragDropKeyStates). Writes only the non-null out params. Returns false when
// the args are not a drag event.
extern "C" bool noesis_routed_events_drag_effects(
    const void* args, uint32_t* effects, uint32_t* allowed, uint32_t* key_states)
{
    if (!args) return false;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != ARG_DRAG) return false;
    auto* d = static_cast<const Noesis::DragEventArgs*>(w->args);
    if (effects) *effects = d->effects;
    if (allowed) *allowed = d->allowedEffects;
    if (key_states) *key_states = d->keyStates;
    return true;
}

// Set DragEventArgs::effects: the chosen drop effect a Drop/DragOver handler
// reports back to the drag source (`effects` is `mutable`). Returns false when
// the args are not a drag event.
extern "C" bool noesis_routed_events_drag_set_effects(const void* args, uint32_t effects) {
    if (!args) return false;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != ARG_DRAG) return false;
    auto* d = static_cast<const Noesis::DragEventArgs*>(w->args);
    d->effects = effects;
    return true;
}

// Borrowed pointer to the dragged data object (DragEventArgs::data,
// BaseComponent*). Not ref-counted. Returns null when the args are not a drag
// event or carry no data.
extern "C" void* noesis_routed_events_drag_data(const void* args) {
    if (!args) return nullptr;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != ARG_DRAG) return nullptr;
    auto* d = static_cast<const Noesis::DragEventArgs*>(w->args);
    return d->data;
}

// DragEventArgs::GetPosition(relativeTo): drop point in `relative_to`'s
// coordinate space. `relative_to` must be a live UIElement*. Returns false when
// the args are not a drag event or `relative_to` is null.
extern "C" bool noesis_routed_events_drag_position(
    const void* args, void* relative_to, float* x, float* y)
{
    if (!args || !relative_to) return false;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != ARG_DRAG) return false;
    auto* d = static_cast<const Noesis::DragEventArgs*>(w->args);
    Noesis::Point p = d->GetPosition(static_cast<Noesis::UIElement*>(relative_to));
    if (x) *x = p.x;
    if (y) *y = p.y;
    return true;
}

// Manipulation origin (manipulationOrigin): present on Started/Delta/
// Completed/InertiaStarting. Returns false for other kinds.
extern "C" bool noesis_routed_events_manip_origin(const void* args, float* x, float* y) {
    if (!args) return false;
    auto* w = static_cast<const DmEventArgs*>(args);
    Noesis::Point origin;
    switch (w->kind) {
        case ARG_MANIP_STARTED:
            origin = static_cast<const Noesis::ManipulationStartedEventArgs*>(w->args)->manipulationOrigin;
            break;
        case ARG_MANIP_DELTA:
            origin = static_cast<const Noesis::ManipulationDeltaEventArgs*>(w->args)->manipulationOrigin;
            break;
        case ARG_MANIP_COMPLETED:
            origin = static_cast<const Noesis::ManipulationCompletedEventArgs*>(w->args)->manipulationOrigin;
            break;
        case ARG_MANIP_INERTIA:
            origin = static_cast<const Noesis::ManipulationInertiaStartingEventArgs*>(w->args)->manipulationOrigin;
            break;
        default:
            return false;
    }
    if (x) *x = origin.x;
    if (y) *y = origin.y;
    return true;
}

// The primary ManipulationDelta transform: `deltaManipulation` for a Delta
// event, `totalManipulation` for a Completed event. translation (tx,ty), scale,
// rotation (degrees), expansion (ex,ey). Returns false for other kinds.
extern "C" bool noesis_routed_events_manip_delta(
    const void* args, float* tx, float* ty, float* scale, float* rotation, float* ex, float* ey)
{
    if (!args) return false;
    auto* w = static_cast<const DmEventArgs*>(args);
    const Noesis::ManipulationDelta* m = nullptr;
    if (w->kind == ARG_MANIP_DELTA) {
        m = &static_cast<const Noesis::ManipulationDeltaEventArgs*>(w->args)->deltaManipulation;
    } else if (w->kind == ARG_MANIP_COMPLETED) {
        m = &static_cast<const Noesis::ManipulationCompletedEventArgs*>(w->args)->totalManipulation;
    } else {
        return false;
    }
    if (tx) *tx = m->translation.x;
    if (ty) *ty = m->translation.y;
    if (scale) *scale = m->scale;
    if (rotation) *rotation = m->rotation;
    if (ex) *ex = m->expansion.x;
    if (ey) *ey = m->expansion.y;
    return true;
}

// The cumulative ManipulationDelta transform for a Delta event
// (`cumulativeManipulation`). Returns false for other kinds.
extern "C" bool noesis_routed_events_manip_cumulative(
    const void* args, float* tx, float* ty, float* scale, float* rotation, float* ex, float* ey)
{
    if (!args) return false;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != ARG_MANIP_DELTA) return false;
    const Noesis::ManipulationDelta& m =
        static_cast<const Noesis::ManipulationDeltaEventArgs*>(w->args)->cumulativeManipulation;
    if (tx) *tx = m.translation.x;
    if (ty) *ty = m.translation.y;
    if (scale) *scale = m.scale;
    if (rotation) *rotation = m.rotation;
    if (ex) *ex = m.expansion.x;
    if (ey) *ey = m.expansion.y;
    return true;
}

// ManipulationVelocities: `velocities` (Delta), `finalVelocities` (Completed)
// or `initialVelocities` (InertiaStarting). angular (deg/ms), linear (lx,ly)
// and expansion (ex,ey) in px/ms. Returns false for other kinds.
extern "C" bool noesis_routed_events_manip_velocities(
    const void* args, float* angular, float* lx, float* ly, float* ex, float* ey)
{
    if (!args) return false;
    auto* w = static_cast<const DmEventArgs*>(args);
    const Noesis::ManipulationVelocities* v = nullptr;
    switch (w->kind) {
        case ARG_MANIP_DELTA:
            v = &static_cast<const Noesis::ManipulationDeltaEventArgs*>(w->args)->velocities;
            break;
        case ARG_MANIP_COMPLETED:
            v = &static_cast<const Noesis::ManipulationCompletedEventArgs*>(w->args)->finalVelocities;
            break;
        case ARG_MANIP_INERTIA:
            v = &static_cast<const Noesis::ManipulationInertiaStartingEventArgs*>(w->args)->initialVelocities;
            break;
        default:
            return false;
    }
    if (angular) *angular = v->angularVelocity;
    if (lx) *lx = v->linearVelocity.x;
    if (ly) *ly = v->linearVelocity.y;
    if (ex) *ex = v->expansionVelocity.x;
    if (ey) *ey = v->expansionVelocity.y;
    return true;
}

// isInertial flag: 1 (inertia phase), 0, or -1 when not a Delta/Completed
// manipulation event.
extern "C" int32_t noesis_routed_events_manip_is_inertial(const void* args) {
    if (!args) return -1;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind == ARG_MANIP_DELTA) {
        return static_cast<const Noesis::ManipulationDeltaEventArgs*>(w->args)->isInertial ? 1 : 0;
    }
    if (w->kind == ARG_MANIP_COMPLETED) {
        return static_cast<const Noesis::ManipulationCompletedEventArgs*>(w->args)->isInertial ? 1 : 0;
    }
    return -1;
}

// ── DragDrop source side + DataObject copy/paste handlers ────────────────────

// Starts a drag from `source` carrying `data`, advertising `allowed_effects`
// (DragDropEffects bitmask). The host's pointer input drives the drag from
// there; nothing completes it headlessly. Returns false if `source` or `data`
// is null or `source` is not a DependencyObject.
extern "C" bool noesis_routed_events_do_drag_drop(
    void* source, void* data, uint32_t allowed_effects)
{
    if (!source || !data) return false;
    auto* dep = Noesis::DynamicCast<Noesis::DependencyObject*>(
        static_cast<Noesis::BaseComponent*>(source));
    if (!dep) return false;
    Noesis::DragDrop::DoDragDrop(dep, static_cast<Noesis::BaseComponent*>(data), allowed_effects);
    return true;
}

namespace {

// DataObject.Copying / Pasting handler. Holds a +1 ref on the element and
// remembers which event it attached to. The callback gets the borrowed data
// object, isDragDrop, and a writable cancel flag.
class RustDataObjectHandler final: public RustSubscription {
public:
    enum Kind { Copying, Pasting };

    RustDataObjectHandler(noesis_data_object_fn cb, void* userdata,
        noesis_subscription_free_fn free, Noesis::UIElement* element, Kind kind)
        : RustSubscription(userdata, free), mCb(cb), mElement(element), mKind(kind)
    {
        if (mElement) {
            mElement->AddReference();
        }
    }

    ~RustDataObjectHandler() {
        if (mElement) {
            mElement->Release();
        }
    }

    RustDataObjectHandler(const RustDataObjectHandler&) = delete;
    RustDataObjectHandler& operator=(const RustDataObjectHandler&) = delete;

    void OnCopying(Noesis::BaseComponent* /*sender*/,
        const Noesis::DataObjectCopyingEventArgs& e) {
        enterDispatch();
        if (mCb) {
            bool cancel = e.commandCancelled;
            mCb(mUserdata, e.dataObject.GetPtr(), e.isDragDrop, &cancel);
            e.commandCancelled = cancel;
        }
        if (leaveDispatch()) {
            delete this;  // deferred teardown from an unsubscribe during dispatch
        }
    }

    void OnPasting(Noesis::BaseComponent* /*sender*/,
        const Noesis::DataObjectPastingEventArgs& e) {
        enterDispatch();
        if (mCb) {
            bool cancel = e.commandCancelled;
            mCb(mUserdata, e.dataObject.GetPtr(), e.isDragDrop, &cancel);
            e.commandCancelled = cancel;
        }
        if (leaveDispatch()) {
            delete this;  // deferred teardown from an unsubscribe during dispatch
        }
    }

    Noesis::UIElement* element() const { return mElement; }
    Kind kind() const { return mKind; }

private:
    noesis_data_object_fn mCb;
    Noesis::UIElement* mElement;
    Kind mKind;
};

}  // namespace

extern "C" void* noesis_routed_events_add_copying_handler(
    void* element, noesis_data_object_fn cb, void* userdata,
    noesis_subscription_free_fn free_handler)
{
    if (!element || !cb) return nullptr;
    auto* uie = Noesis::DynamicCast<Noesis::UIElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!uie) return nullptr;
    auto* handler = new RustDataObjectHandler(
        cb, userdata, free_handler, uie, RustDataObjectHandler::Copying);
    Noesis::DataObject::AddCopyingHandler(
        uie, Noesis::MakeDelegate(handler, &RustDataObjectHandler::OnCopying));
    return handler;
}

extern "C" void* noesis_routed_events_add_pasting_handler(
    void* element, noesis_data_object_fn cb, void* userdata,
    noesis_subscription_free_fn free_handler)
{
    if (!element || !cb) return nullptr;
    auto* uie = Noesis::DynamicCast<Noesis::UIElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!uie) return nullptr;
    auto* handler = new RustDataObjectHandler(
        cb, userdata, free_handler, uie, RustDataObjectHandler::Pasting);
    Noesis::DataObject::AddPastingHandler(
        uie, Noesis::MakeDelegate(handler, &RustDataObjectHandler::OnPasting));
    return handler;
}

extern "C" void noesis_routed_events_remove_data_object_handler(void* token) {
    if (!token) return;
    auto* handler = static_cast<RustDataObjectHandler*>(token);
    if (auto* uie = handler->element()) {
        if (handler->kind() == RustDataObjectHandler::Copying) {
            Noesis::DataObject::RemoveCopyingHandler(
                uie, Noesis::MakeDelegate(handler, &RustDataObjectHandler::OnCopying));
        } else {
            Noesis::DataObject::RemovePastingHandler(
                uie, Noesis::MakeDelegate(handler, &RustDataObjectHandler::OnPasting));
        }
    }
    if (handler->deferDeleteIfDispatching()) return;
    delete handler;
}

// ── Non-routed lifecycle events ─────────────────────────────────────────────
//
// Initialized, LayoutUpdated, DataContextChanged and the Is*Changed
// notifications are not routed events; they use Event_<T> (AddEventHandler
// with an internal Symbol key). Each is driven through its public accessor's
// `+=` / `-=`, which supplies the right key. No args are surfaced; the callback
// is `void(userdata)`.
//
// The handler holds a +1 ref on the element and copies the event name for the
// matching `-=`.

namespace {

class RustLifecycleHandler final: public RustSubscription {
public:
    RustLifecycleHandler(noesis_lifecycle_fn cb, void* userdata,
        noesis_subscription_free_fn free, Noesis::FrameworkElement* element, const char* name)
        : RustSubscription(userdata, free), mCb(cb), mElement(element)
    {
        if (mElement) {
            mElement->AddReference();
        }
        const size_t n = strlen(name);
        mName = new char[n + 1];
        memcpy(mName, name, n + 1);
    }

    ~RustLifecycleHandler() {
        if (mElement) {
            mElement->Release();
        }
        delete[] mName;
    }

    RustLifecycleHandler(const RustLifecycleHandler&) = delete;
    RustLifecycleHandler& operator=(const RustLifecycleHandler&) = delete;

    // EventHandler signature (Initialized / LayoutUpdated).
    void OnEvent(Noesis::BaseComponent* /*sender*/, const Noesis::EventArgs& /*args*/) {
        dispatch();
    }

    // DependencyPropertyChangedEventHandler signature (everything else).
    void OnDpEvent(Noesis::BaseComponent* /*sender*/,
        const Noesis::DependencyPropertyChangedEventArgs& /*args*/) {
        dispatch();
    }

    Noesis::FrameworkElement* element() const { return mElement; }
    const char* name() const { return mName; }

private:
    void dispatch() {
        enterDispatch();
        if (mCb) mCb(mUserdata);
        if (leaveDispatch()) {
            delete this;  // deferred teardown from an unsubscribe during dispatch
        }
    }

    noesis_lifecycle_fn mCb;
    Noesis::FrameworkElement* mElement;
    char* mName;
};

// Adds or removes the handler for the named lifecycle event. One table for
// both directions keeps them symmetric. Returns false for an unknown name.
bool ApplyLifecycle(
    Noesis::FrameworkElement* fe, const char* name, RustLifecycleHandler* h, bool add)
{
#define LC_PLAIN(N, ACC)                                                       \
    if (strcmp(name, N) == 0) {                                                    \
        if (add) fe->ACC() += Noesis::MakeDelegate(h, &RustLifecycleHandler::OnEvent); \
        else fe->ACC() -= Noesis::MakeDelegate(h, &RustLifecycleHandler::OnEvent);     \
        return true;                                                               \
    }
#define LC_DP(N, ACC)                                                          \
    if (strcmp(name, N) == 0) {                                                    \
        if (add) fe->ACC() += Noesis::MakeDelegate(h, &RustLifecycleHandler::OnDpEvent); \
        else fe->ACC() -= Noesis::MakeDelegate(h, &RustLifecycleHandler::OnDpEvent);     \
        return true;                                                               \
    }
    LC_PLAIN("Initialized", Initialized)
    LC_PLAIN("LayoutUpdated", LayoutUpdated)
    LC_DP("IsEnabledChanged", IsEnabledChanged)
    LC_DP("IsVisibleChanged", IsVisibleChanged)
    LC_DP("IsHitTestVisibleChanged", IsHitTestVisibleChanged)
    LC_DP("IsKeyboardFocusedChanged", IsKeyboardFocusedChanged)
    LC_DP("IsKeyboardFocusWithinChanged", IsKeyboardFocusWithinChanged)
    LC_DP("IsMouseCapturedChanged", IsMouseCapturedChanged)
    LC_DP("IsMouseCaptureWithinChanged", IsMouseCaptureWithinChanged)
    LC_DP("IsMouseDirectlyOverChanged", IsMouseDirectlyOverChanged)
    LC_DP("FocusableChanged", FocusableChanged)
    LC_DP("DataContextChanged", DataContextChanged)
#undef LC_PLAIN
#undef LC_DP
    return false;
}

}  // namespace

extern "C" void* noesis_subscribe_lifecycle(
    void* element, const char* event_name, noesis_lifecycle_fn cb, void* userdata,
    noesis_subscription_free_fn free_handler)
{
    if (!element || !event_name || !cb) return nullptr;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return nullptr;

    auto* handler = new RustLifecycleHandler(cb, userdata, free_handler, fe, event_name);
    if (!ApplyLifecycle(fe, handler->name(), handler, true)) {
        // ownership never transferred; Rust reclaims the box on NULL
        handler->abandonDonation();
        delete handler;
        return nullptr;
    }
    return handler;
}

extern "C" void noesis_unsubscribe_lifecycle(void* token) {
    if (!token) return;
    auto* handler = static_cast<RustLifecycleHandler*>(token);
    if (auto* fe = handler->element()) {
        ApplyLifecycle(fe, handler->name(), handler, false);
    }
    if (handler->deferDeleteIfDispatching()) return;
    delete handler;
}

// ── Test-only entry points (`test-utils` feature, NOESIS_TEST_UTILS) ────────
//
// Drag and manipulation events can't be synthesized headlessly: drags need an
// OS drag loop, manipulations a multi-frame touch stream. These build the real
// arg structs with known values, wrap them in DmEventArgs as OnEvent does, and
// invoke the callback so tests can read them back through the accessors.

#ifdef NOESIS_TEST_UTILS

// Build a DragEventArgs with deterministic fields and dispatch it. `element`
// (a live UIElement*) is used as source / data / target so GetPosition has a
// valid `relativeTo` to resolve against. effects=Copy, allowedEffects=All,
// keyStates=Control, dropPoint=(12, 34).
extern "C" void noesis_routed_events_test_raise_drag(
    void* element, noesis_routed_event_fn cb, void* userdata)
{
    if (!element || !cb) return;
    auto* uie = static_cast<Noesis::UIElement*>(element);
    Noesis::DragEventArgs args(uie, Noesis::UIElement::DropEvent, uie,
        Noesis::DragDropKeyStates_ControlKey,
        Noesis::DragDropEffects_All, uie, Noesis::Point(12.0f, 34.0f));
    args.effects = Noesis::DragDropEffects_Copy;
    DmEventArgs wrap{ARG_DRAG, &args};
    bool handled = false;
    cb(userdata, &wrap, &handled);
}

// Build a ManipulationDeltaEventArgs with deterministic fields and dispatch it.
// origin=(100,200); delta translation=(5,7) scale=2 rotation=15 expansion=(3,4);
// cumulative translation=(50,70) scale=4 rotation=30 expansion=(6,8);
// velocities angular=1.5 linear=(0.5,0.6) expansion=(0.1,0.2); isInertial=true.
extern "C" void noesis_routed_events_test_raise_manip_delta(
    void* element, noesis_routed_event_fn cb, void* userdata)
{
    if (!element || !cb) return;
    auto* uie = static_cast<Noesis::UIElement*>(element);
    Noesis::ManipulationDelta delta{Noesis::Point(3.0f, 4.0f), 15.0f, 2.0f, Noesis::Point(5.0f, 7.0f)};
    Noesis::ManipulationDelta cumulative{Noesis::Point(6.0f, 8.0f), 30.0f, 4.0f, Noesis::Point(50.0f, 70.0f)};
    Noesis::ManipulationVelocities velocities{1.5f, Noesis::Point(0.1f, 0.2f), Noesis::Point(0.5f, 0.6f)};
    Noesis::ManipulationDeltaEventArgs args(uie, Noesis::UIElement::ManipulationDeltaEvent,
        nullptr, Noesis::Point(100.0f, 200.0f), delta, cumulative, velocities,
        true, Noesis::ArrayRef<Noesis::Manipulator>());
    DmEventArgs wrap{ARG_MANIP_DELTA, &args};
    bool handled = false;
    cb(userdata, &wrap, &handled);
}

// Build a ManipulationCompletedEventArgs with deterministic fields and dispatch
// it. origin=(100,200); total translation=(11,13) scale=3 rotation=45
// expansion=(1,2); finalVelocities angular=2.5 linear=(1.5,1.6)
// expansion=(1.1,1.2); isInertial=false.
extern "C" void noesis_routed_events_test_raise_manip_completed(
    void* element, noesis_routed_event_fn cb, void* userdata)
{
    if (!element || !cb) return;
    auto* uie = static_cast<Noesis::UIElement*>(element);
    Noesis::ManipulationDelta total{Noesis::Point(1.0f, 2.0f), 45.0f, 3.0f, Noesis::Point(11.0f, 13.0f)};
    Noesis::ManipulationVelocities velocities{2.5f, Noesis::Point(1.1f, 1.2f), Noesis::Point(1.5f, 1.6f)};
    Noesis::ManipulationCompletedEventArgs args(uie, Noesis::UIElement::ManipulationCompletedEvent,
        nullptr, Noesis::Point(100.0f, 200.0f), velocities, total, false,
        Noesis::ArrayRef<Noesis::Manipulator>());
    DmEventArgs wrap{ARG_MANIP_COMPLETED, &args};
    bool handled = false;
    cb(userdata, &wrap, &handled);
}

#endif  // NOESIS_TEST_UTILS
