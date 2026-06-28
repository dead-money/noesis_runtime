// FrameworkElement traversal + event subscription FFI (Phase 5.B).
//
// Pieces:
//   * `dm_noesis_framework_element_find_name` — wraps Noesis's `FindName`.
//     Returns an owning (+1 ref) `FrameworkElement*` so the Rust side
//     manages lifetime via the same release path as `GUI::LoadXaml`.
//   * `dm_noesis_subscribe_click` — installs a Rust callback on the
//     `BaseButton::Click` routed event. `dm_noesis_unsubscribe_click`
//     removes it. The token returned to Rust is a heap-allocated
//     `RustClickHandler` whose lifetime is tied 1:1 to the subscription;
//     it owns a +1 ref on the button so the subscription stays valid
//     even if the only other reference is the parent FrameworkElement
//     that the Rust caller dropped.
//   * `dm_noesis_subscribe_keydown` / `_unsubscribe_keydown` — same shape
//     as click but for `UIElement::KeyDown`. The callback receives the
//     pressed Key as a raw int and a writable `out_handled` flag the
//     Rust side can set to `true` to suppress further routing (e.g.
//     swallow backtick so it doesn't get typed into the focused TextBox).
//   * `dm_noesis_text_get` / `_text_set` / `_text_caret_to_end` — read /
//     write `TextBox::Text` (and `TextBlock::Text` for read), plus a
//     caret-to-end helper for command-history navigation.
//   * `dm_noesis_focus_element` — `UIElement::Focus()` so Rust can move
//     keyboard focus to a named element programmatically (the input box
//     when the console opens, etc.).
//
// Why a separate translation unit (rather than appending to noesis_view.cpp):
// the headers we pull in here (`BaseButton.h`, `RoutedEvent.h`, `Delegate.h`)
// are heavy enough that we'd rather not pay for them in the input-pump file
// every other FFI surface depends on.

#include "noesis_shim.h"

#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsCore/Delegate.h>
#include <NsCore/DynamicCast.h>
#include <NsGui/BaseButton.h>
#include <NsGui/FrameworkElement.h>
#include <NsGui/Path.h>
#include <NsGui/RoutedEvent.h>
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

// Adapter between Noesis's `Delegate<void(BaseComponent*, const
// RoutedEventArgs&)>` and the C ABI callback. Stores the function pointer +
// userdata that the Rust trampoline registered, plus a +1 ref on the button
// so the subscription remains valid even if Rust drops every other handle to
// the element. A handler owns its subscription; pair construction with
// `Click() +=` and destruction with `Click() -=` so the reference symmetry
// between this object and the routed-event-handler list is exact.
class RustClickHandler {
public:
    RustClickHandler(dm_noesis_click_fn cb, void* userdata, Noesis::BaseButton* button)
        : mCb(cb), mUserdata(userdata), mButton(button)
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
        if (mCb) {
            mCb(mUserdata);
        }
    }

    Noesis::BaseButton* button() const { return mButton; }

private:
    dm_noesis_click_fn mCb;
    void* mUserdata;
    Noesis::BaseButton* mButton;  // raw + manual AddRef/Release — see ctor/dtor.
};

}  // namespace

extern "C" void* dm_noesis_framework_element_find_name(void* element, const char* name) {
    if (!element || !name) return nullptr;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    Noesis::BaseComponent* found = fe->FindName(name);
    if (!found) return nullptr;
    // FindName returns a non-owning raw pointer (the parent FE owns the named
    // child). We need an owning +1-ref `FrameworkElement*` so the Rust wrapper
    // can release it via `dm_noesis_base_component_release` like every other
    // FFI-provided component. Cast first; AddReference second.
    auto* result = Noesis::DynamicCast<Noesis::FrameworkElement*>(found);
    if (!result) return nullptr;
    result->AddReference();
    return result;
}

// Register / unregister an x:Name in the namescope hosting `element` (TODO §2.F).
// `object` is borrowed — the scope takes its own reference. Returns false if
// `element` is not a FrameworkElement. The element must live within a namescope
// (the XAML root hosts one); registering a name already present updates it.
extern "C" bool dm_noesis_framework_element_register_name(
    void* element, const char* name, void* object) {
    if (!element || !name) return false;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return false;
    fe->RegisterName(name, static_cast<Noesis::BaseComponent*>(object));
    return true;
}

extern "C" bool dm_noesis_framework_element_unregister_name(void* element, const char* name) {
    if (!element || !name) return false;
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(
        static_cast<Noesis::BaseComponent*>(element));
    if (!fe) return false;
    fe->UnregisterName(name);
    return true;
}

extern "C" const char* dm_noesis_framework_element_get_name(void* element) {
    if (!element) return nullptr;
    return static_cast<Noesis::FrameworkElement*>(element)->GetName();
}

extern "C" void dm_noesis_framework_element_set_visibility(void* element, bool visible) {
    if (!element) return;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    fe->SetVisibility(visible ? Noesis::Visibility_Visible : Noesis::Visibility_Collapsed);
}

extern "C" void dm_noesis_framework_element_set_margin(
    void* element, float left, float top, float right, float bottom)
{
    if (!element) return;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    fe->SetMargin(Noesis::Thickness(left, top, right, bottom));
}

extern "C" void* dm_noesis_subscribe_click(
    void* element, dm_noesis_click_fn cb, void* userdata)
{
    if (!element || !cb) return nullptr;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    auto* button = Noesis::DynamicCast<Noesis::BaseButton*>(fe);
    if (!button) return nullptr;

    auto* handler = new RustClickHandler(cb, userdata, button);
    button->Click() += Noesis::MakeDelegate(handler, &RustClickHandler::OnClick);
    return handler;
}

extern "C" void dm_noesis_unsubscribe_click(void* token) {
    if (!token) return;
    auto* handler = static_cast<RustClickHandler*>(token);
    if (auto* button = handler->button()) {
        button->Click() -= Noesis::MakeDelegate(handler, &RustClickHandler::OnClick);
    }
    delete handler;
}

// ── KeyDown subscription ───────────────────────────────────────────────────

namespace {

// Adapter between Noesis's `Delegate<void(BaseComponent*, const KeyEventArgs&)>`
// and the C ABI callback. Mirrors `RustClickHandler` — owns a +1 ref on the
// element so the subscription survives the caller dropping every other
// handle. Pair construction with `KeyDown() +=` and destruction with
// `KeyDown() -=`.
//
// `out_handled` lets the Rust side mark the event handled so further routing
// stops — important for swallowing the backtick keystroke that opens the
// console (otherwise it gets typed into the focused TextBox).
class RustKeyDownHandler {
public:
    RustKeyDownHandler(dm_noesis_keydown_fn cb, void* userdata, Noesis::UIElement* element)
        : mCb(cb), mUserdata(userdata), mElement(element)
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
        if (!mCb) return;
        bool handled = false;
        mCb(mUserdata, static_cast<int32_t>(args.key), &handled);
        // RoutedEventArgs::handled is `mutable` — writing through a const
        // reference is supported by design.
        if (handled) {
            args.handled = true;
        }
    }

    Noesis::UIElement* element() const { return mElement; }

private:
    dm_noesis_keydown_fn mCb;
    void* mUserdata;
    Noesis::UIElement* mElement;  // raw + manual AddRef/Release — see ctor/dtor.
};

}  // namespace

extern "C" void* dm_noesis_subscribe_keydown(
    void* element, dm_noesis_keydown_fn cb, void* userdata)
{
    if (!element || !cb) return nullptr;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    auto* uie = Noesis::DynamicCast<Noesis::UIElement*>(fe);
    if (!uie) return nullptr;

    auto* handler = new RustKeyDownHandler(cb, userdata, uie);
    uie->KeyDown() += Noesis::MakeDelegate(handler, &RustKeyDownHandler::OnKeyDown);
    return handler;
}

extern "C" void dm_noesis_unsubscribe_keydown(void* token) {
    if (!token) return;
    auto* handler = static_cast<RustKeyDownHandler*>(token);
    if (auto* uie = handler->element()) {
        uie->KeyDown() -= Noesis::MakeDelegate(handler, &RustKeyDownHandler::OnKeyDown);
    }
    delete handler;
}

// ── Text get/set + caret + focus ───────────────────────────────────────────

extern "C" const char* dm_noesis_text_get(void* element) {
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

extern "C" bool dm_noesis_text_set(void* element, const char* text) {
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

extern "C" bool dm_noesis_text_caret_to_end(void* element) {
    if (!element) return false;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    auto* tb = Noesis::DynamicCast<Noesis::TextBox*>(fe);
    if (!tb) return false;
    const char* current = tb->GetText();
    const int32_t len = current ? static_cast<int32_t>(strlen(current)) : 0;
    tb->SetCaretIndex(len);
    return true;
}

extern "C" bool dm_noesis_focus_element(void* element) {
    if (!element) return false;
    auto* fe = static_cast<Noesis::FrameworkElement*>(element);
    auto* uie = Noesis::DynamicCast<Noesis::UIElement*>(fe);
    if (!uie) return false;
    return uie->Focus();
}

// Build a single open polyline figure from `count` (x, y) pairs in `xy` (so
// `xy` has `2*count` floats, in the Path's local coordinate space) and assign it
// as a named `Path`'s `Data`. This is the geometry affordance behind the live
// oscilloscope trace — a real vector polyline fed from Rust each frame, in place
// of a rasterised text canvas. Returns false if the element is missing, not a
// `Path`, or there are fewer than two points (no segment to draw).
extern "C" bool dm_noesis_path_set_points(void* element, const float* xy, uint32_t count) {
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

// ── Generic routed-event subscription (TODO §5) ─────────────────────────────
//
// One mechanism replaces the bespoke Click/KeyDown wrappers for the whole
// routed-event surface. Two facts about this SDK make it work:
//
//   1. `UIElement::AddHandler(const RoutedEvent*, const RoutedEventHandler&)`
//      takes a *generic* `RoutedEventHandler` =
//      `Delegate<void(BaseComponent*, const RoutedEventArgs&)>`. Every typed
//      `RoutedEvent_<T>` wrapper in UIElement.h reinterpret_casts its handler
//      to exactly that delegate before calling AddHandler — so a single
//      delegate signature is ABI-correct for *every* routed event. We register
//      one `OnEvent(BaseComponent*, const RoutedEventArgs&)` for all of them.
//
//   2. The event arg structs (`MouseEventArgs`, `KeyEventArgs`, ...) are plain
//      structs WITHOUT reflection, so `DynamicCast` cannot downcast them. We
//      instead classify the arg shape up-front from the event being subscribed
//      to (see kEvents table) and carry an integer `kind` discriminant in the
//      `DmEventArgs` wrapper handed to the callback. The typed accessors below
//      `static_cast` on that `kind` (single, non-virtual inheritance → zero
//      pointer offset, so the downcast is sound for the known type) and return
//      a sentinel when the kind doesn't match.
//
// `handledEventsToo`: this SDK's `AddHandler` has NO third bool parameter —
// already-handled events are not re-delivered to a registered handler across
// the bubble/tunnel route. The `handled_too` flag is still honoured *within*
// the per-element multicast delegate: when false, the user callback is skipped
// if a prior handler on the same element already set `handled`. See
// `RustRoutedHandler::OnEvent`.

namespace {

// Arg-shape discriminant. Mirrored by `events::ArgKind` in src/events.rs and
// the accessor sentinels there. Keep the two in sync.
enum DmArgKind : int32_t {
    DM_ARG_ROUTED       = 0,  // RoutedEventArgs (source + handled only)
    DM_ARG_MOUSE        = 1,  // MouseEventArgs (position)
    DM_ARG_MOUSE_BUTTON = 2,  // MouseButtonEventArgs (position + button)
    DM_ARG_MOUSE_WHEEL  = 3,  // MouseWheelEventArgs (position + wheel delta)
    DM_ARG_KEY          = 4,  // KeyEventArgs (key)
    DM_ARG_SIZE_CHANGED = 5,  // SizeChangedEventArgs (new/previous size)
    DM_ARG_TEXT_INPUT   = 6,  // TextCompositionEventArgs (ch)
};

// The opaque payload the callback receives. `args` always points at the live
// `RoutedEventArgs` (or a derived struct, selected by `kind`) on the C++
// stack; it is valid only for the duration of the callback.
struct DmEventArgs {
    int32_t kind;
    const Noesis::RoutedEventArgs* args;
};

// Name → (static RoutedEvent* slot, arg kind). The slots are the
// `UIElement::*Event` / `FrameworkElement::*Event` statics; we store their
// addresses and dereference at lookup time (the pointers are populated during
// Noesis registration, after init). Typed kinds get rich accessors; everything
// else is exposed with `DM_ARG_ROUTED` (source + handled), which is enough to
// observe the event and read its originating element.
struct EventEntry {
    const char* name;
    const Noesis::RoutedEvent* const* slot;
    int32_t kind;
};

const EventEntry kEvents[] = {
    // Mouse (position only)
    {"MouseEnter",        &Noesis::UIElement::MouseEnterEvent,                 DM_ARG_MOUSE},
    {"MouseLeave",        &Noesis::UIElement::MouseLeaveEvent,                 DM_ARG_MOUSE},
    {"MouseMove",         &Noesis::UIElement::MouseMoveEvent,                  DM_ARG_MOUSE},
    {"PreviewMouseMove",  &Noesis::UIElement::PreviewMouseMoveEvent,           DM_ARG_MOUSE},
    {"GotMouseCapture",   &Noesis::UIElement::GotMouseCaptureEvent,            DM_ARG_MOUSE},
    {"LostMouseCapture",  &Noesis::UIElement::LostMouseCaptureEvent,           DM_ARG_MOUSE},
    // Mouse buttons (position + changedButton)
    {"MouseDown",                  &Noesis::UIElement::MouseDownEvent,                 DM_ARG_MOUSE_BUTTON},
    {"MouseUp",                    &Noesis::UIElement::MouseUpEvent,                   DM_ARG_MOUSE_BUTTON},
    {"MouseLeftButtonDown",        &Noesis::UIElement::MouseLeftButtonDownEvent,       DM_ARG_MOUSE_BUTTON},
    {"MouseLeftButtonUp",          &Noesis::UIElement::MouseLeftButtonUpEvent,         DM_ARG_MOUSE_BUTTON},
    {"MouseRightButtonDown",       &Noesis::UIElement::MouseRightButtonDownEvent,      DM_ARG_MOUSE_BUTTON},
    {"MouseRightButtonUp",         &Noesis::UIElement::MouseRightButtonUpEvent,        DM_ARG_MOUSE_BUTTON},
    {"PreviewMouseDown",           &Noesis::UIElement::PreviewMouseDownEvent,          DM_ARG_MOUSE_BUTTON},
    {"PreviewMouseUp",             &Noesis::UIElement::PreviewMouseUpEvent,            DM_ARG_MOUSE_BUTTON},
    {"PreviewMouseLeftButtonDown", &Noesis::UIElement::PreviewMouseLeftButtonDownEvent, DM_ARG_MOUSE_BUTTON},
    {"PreviewMouseLeftButtonUp",   &Noesis::UIElement::PreviewMouseLeftButtonUpEvent,  DM_ARG_MOUSE_BUTTON},
    {"PreviewMouseRightButtonDown",&Noesis::UIElement::PreviewMouseRightButtonDownEvent,DM_ARG_MOUSE_BUTTON},
    {"PreviewMouseRightButtonUp",  &Noesis::UIElement::PreviewMouseRightButtonUpEvent, DM_ARG_MOUSE_BUTTON},
    // Mouse wheel (position + wheelRotation)
    {"MouseWheel",        &Noesis::UIElement::MouseWheelEvent,                 DM_ARG_MOUSE_WHEEL},
    {"PreviewMouseWheel", &Noesis::UIElement::PreviewMouseWheelEvent,          DM_ARG_MOUSE_WHEEL},
    // Keyboard (key)
    {"KeyDown",        &Noesis::UIElement::KeyDownEvent,            DM_ARG_KEY},
    {"KeyUp",          &Noesis::UIElement::KeyUpEvent,              DM_ARG_KEY},
    {"PreviewKeyDown", &Noesis::UIElement::PreviewKeyDownEvent,     DM_ARG_KEY},
    {"PreviewKeyUp",   &Noesis::UIElement::PreviewKeyUpEvent,       DM_ARG_KEY},
    // Text input (ch)
    {"TextInput",        &Noesis::UIElement::TextInputEvent,        DM_ARG_TEXT_INPUT},
    {"PreviewTextInput", &Noesis::UIElement::PreviewTextInputEvent, DM_ARG_TEXT_INPUT},
    // Focus (base routed args; KeyboardFocus variants carry old/new focus we
    // don't surface yet — source + handled still apply)
    {"GotFocus",                 &Noesis::UIElement::GotFocusEvent,                 DM_ARG_ROUTED},
    {"LostFocus",                &Noesis::UIElement::LostFocusEvent,                DM_ARG_ROUTED},
    {"GotKeyboardFocus",         &Noesis::UIElement::GotKeyboardFocusEvent,         DM_ARG_ROUTED},
    {"LostKeyboardFocus",        &Noesis::UIElement::LostKeyboardFocusEvent,        DM_ARG_ROUTED},
    {"PreviewGotKeyboardFocus",  &Noesis::UIElement::PreviewGotKeyboardFocusEvent,  DM_ARG_ROUTED},
    {"PreviewLostKeyboardFocus", &Noesis::UIElement::PreviewLostKeyboardFocusEvent, DM_ARG_ROUTED},
    // Lifecycle
    {"Loaded",      &Noesis::FrameworkElement::LoadedEvent,      DM_ARG_ROUTED},
    {"Unloaded",    &Noesis::FrameworkElement::UnloadedEvent,    DM_ARG_ROUTED},
    {"SizeChanged", &Noesis::FrameworkElement::SizeChangedEvent, DM_ARG_SIZE_CHANGED},
    // Touch / manipulation (base routed args)
    {"TouchDown",             &Noesis::UIElement::TouchDownEvent,             DM_ARG_ROUTED},
    {"TouchMove",             &Noesis::UIElement::TouchMoveEvent,             DM_ARG_ROUTED},
    {"TouchUp",               &Noesis::UIElement::TouchUpEvent,               DM_ARG_ROUTED},
    {"TouchEnter",            &Noesis::UIElement::TouchEnterEvent,            DM_ARG_ROUTED},
    {"TouchLeave",            &Noesis::UIElement::TouchLeaveEvent,            DM_ARG_ROUTED},
    {"Tapped",                &Noesis::UIElement::TappedEvent,                DM_ARG_ROUTED},
    {"DoubleTapped",          &Noesis::UIElement::DoubleTappedEvent,          DM_ARG_ROUTED},
    {"Holding",               &Noesis::UIElement::HoldingEvent,               DM_ARG_ROUTED},
    {"RightTapped",           &Noesis::UIElement::RightTappedEvent,           DM_ARG_ROUTED},
    {"ManipulationStarting",  &Noesis::UIElement::ManipulationStartingEvent,  DM_ARG_ROUTED},
    {"ManipulationStarted",   &Noesis::UIElement::ManipulationStartedEvent,   DM_ARG_ROUTED},
    {"ManipulationDelta",     &Noesis::UIElement::ManipulationDeltaEvent,     DM_ARG_ROUTED},
    {"ManipulationCompleted", &Noesis::UIElement::ManipulationCompletedEvent, DM_ARG_ROUTED},
    // Drag/drop (base routed args)
    {"DragEnter",        &Noesis::UIElement::DragEnterEvent,        DM_ARG_ROUTED},
    {"DragOver",         &Noesis::UIElement::DragOverEvent,         DM_ARG_ROUTED},
    {"DragLeave",        &Noesis::UIElement::DragLeaveEvent,        DM_ARG_ROUTED},
    {"Drop",             &Noesis::UIElement::DropEvent,             DM_ARG_ROUTED},
    {"PreviewDragEnter", &Noesis::UIElement::PreviewDragEnterEvent, DM_ARG_ROUTED},
    {"PreviewDragOver",  &Noesis::UIElement::PreviewDragOverEvent,  DM_ARG_ROUTED},
    {"PreviewDragLeave", &Noesis::UIElement::PreviewDragLeaveEvent, DM_ARG_ROUTED},
    {"PreviewDrop",      &Noesis::UIElement::PreviewDropEvent,      DM_ARG_ROUTED},
};

// Resolve `name` to a RoutedEvent + arg kind. Tries the curated table first
// (gives the precise arg kind), then falls back to the SDK's generic
// `FindRoutedEvent` over the element's class hierarchy (arg kind reported as
// `DM_ARG_ROUTED`, i.e. base accessors only). Returns nullptr if neither path
// resolves the name.
const Noesis::RoutedEvent* LookupEvent(
    Noesis::UIElement* element, const char* name, int32_t& outKind)
{
    for (const EventEntry& e : kEvents) {
        if (strcmp(e.name, name) == 0) {
            outKind = e.kind;
            return *e.slot;
        }
    }
    // Generic fallback: a name we didn't curate but the reflection system knows.
    Noesis::Symbol sym(name, Noesis::Symbol::NullIfNotFound());
    if (sym.IsNull()) return nullptr;
    const Noesis::RoutedEvent* ev = Noesis::FindRoutedEvent(element->GetClassType(), sym);
    if (ev) {
        outKind = DM_ARG_ROUTED;
    }
    return ev;
}

// Adapter between a generic `RoutedEventHandler` and the C ABI callback.
// Mirrors `RustClickHandler` / `RustKeyDownHandler`: owns a +1 ref on the
// element (so the subscription survives the caller dropping every other handle)
// and stores the RoutedEvent it registered against so `-=` is exact on
// teardown. Pair construction with `AddHandler` and destruction with
// `RemoveHandler`.
class RustRoutedHandler {
public:
    RustRoutedHandler(dm_noesis_routed_event_fn cb, void* userdata, Noesis::UIElement* element,
        const Noesis::RoutedEvent* ev, int32_t kind, bool handledToo)
        : mCb(cb), mUserdata(userdata), mElement(element), mEvent(ev), mKind(kind),
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
        if (!mCb) return;
        // handledEventsToo semantics: when false, respect a prior handler on
        // this element that already marked the event handled.
        if (!mHandledToo && args.handled) return;

        DmEventArgs wrap{mKind, &args};
        bool handled = args.handled;
        mCb(mUserdata, &wrap, &handled);
        // RoutedEventArgs::handled is `mutable` — writing through the const
        // reference is supported by design.
        if (handled) {
            args.handled = true;
        }
    }

    Noesis::UIElement* element() const { return mElement; }
    const Noesis::RoutedEvent* event() const { return mEvent; }

private:
    dm_noesis_routed_event_fn mCb;
    void* mUserdata;
    Noesis::UIElement* mElement;  // raw + manual AddRef/Release — see ctor/dtor.
    const Noesis::RoutedEvent* mEvent;
    int32_t mKind;
    bool mHandledToo;
};

}  // namespace

extern "C" void* dm_noesis_subscribe_event(
    void* element, const char* event_name, bool handled_too, dm_noesis_routed_event_fn cb,
    void* userdata)
{
    if (!element || !event_name || !cb) return nullptr;
    auto* uie = Noesis::DynamicCast<Noesis::UIElement*>(static_cast<Noesis::BaseComponent*>(element));
    if (!uie) return nullptr;

    int32_t kind = DM_ARG_ROUTED;
    const Noesis::RoutedEvent* ev = LookupEvent(uie, event_name, kind);
    if (!ev) return nullptr;

    auto* handler = new RustRoutedHandler(cb, userdata, uie, ev, kind, handled_too);
    uie->AddHandler(ev, Noesis::MakeDelegate(handler, &RustRoutedHandler::OnEvent));
    return handler;
}

extern "C" void dm_noesis_unsubscribe_event(void* token) {
    if (!token) return;
    auto* handler = static_cast<RustRoutedHandler*>(token);
    if (auto* uie = handler->element()) {
        uie->RemoveHandler(handler->event(),
            Noesis::MakeDelegate(handler, &RustRoutedHandler::OnEvent));
    }
    delete handler;
}

// ── Event-arg accessors ─────────────────────────────────────────────────────
//
// Each takes the opaque `args` the callback received (a `DmEventArgs*`) and
// introspects via the carried `kind`. Returning a sentinel (false / -1 / 0)
// when the kind doesn't match lets one generic callback safely probe whatever
// arrived without knowing the concrete type up front.

extern "C" bool dm_noesis_mouse_args_position(const void* args, float* x, float* y) {
    if (!args) return false;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != DM_ARG_MOUSE && w->kind != DM_ARG_MOUSE_BUTTON &&
        w->kind != DM_ARG_MOUSE_WHEEL) {
        return false;
    }
    auto* m = static_cast<const Noesis::MouseEventArgs*>(w->args);
    if (x) *x = m->position.x;
    if (y) *y = m->position.y;
    return true;
}

extern "C" int32_t dm_noesis_mouse_button_args_button(const void* args) {
    if (!args) return -1;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != DM_ARG_MOUSE_BUTTON) return -1;
    auto* m = static_cast<const Noesis::MouseButtonEventArgs*>(w->args);
    return static_cast<int32_t>(m->changedButton);
}

extern "C" int32_t dm_noesis_mouse_wheel_args_delta(const void* args) {
    if (!args) return 0;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != DM_ARG_MOUSE_WHEEL) return 0;
    auto* m = static_cast<const Noesis::MouseWheelEventArgs*>(w->args);
    return static_cast<int32_t>(m->wheelRotation);
}

extern "C" int32_t dm_noesis_key_args_key(const void* args) {
    if (!args) return -1;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != DM_ARG_KEY) return -1;
    auto* k = static_cast<const Noesis::KeyEventArgs*>(w->args);
    return static_cast<int32_t>(k->key);
}

extern "C" int32_t dm_noesis_text_args_ch(const void* args) {
    if (!args) return -1;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != DM_ARG_TEXT_INPUT) return -1;
    auto* t = static_cast<const Noesis::TextCompositionEventArgs*>(w->args);
    return static_cast<int32_t>(t->ch);
}

extern "C" bool dm_noesis_size_changed_args_new_size(const void* args, float* width, float* height) {
    if (!args) return false;
    auto* w = static_cast<const DmEventArgs*>(args);
    if (w->kind != DM_ARG_SIZE_CHANGED) return false;
    auto* s = static_cast<const Noesis::SizeChangedEventArgs*>(w->args);
    if (width) *width = s->newSize.width;
    if (height) *height = s->newSize.height;
    return true;
}

// Borrowed pointer to the event's originating element (`RoutedEventArgs::source`).
// Returns NULL if `args` is null or the source is null. Not ref-counted — do
// not release; valid only for the callback's duration.
extern "C" void* dm_noesis_routed_args_source(const void* args) {
    if (!args) return nullptr;
    auto* w = static_cast<const DmEventArgs*>(args);
    return w->args ? w->args->source : nullptr;
}
