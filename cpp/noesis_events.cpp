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
#include <NsDrawing/Point.h>
#include <NsDrawing/Thickness.h>

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
