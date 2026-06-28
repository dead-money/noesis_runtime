// VisualStateManager FFI.
//
// A single thin entrypoint over `VisualStateManager::GoToState` so Rust can
// drive control state transitions (the CommonStates / CheckStates / FocusStates
// groups a ControlTemplate declares) from code instead of only through triggers.
//
// The SDK signature is:
//
//     static bool VisualStateManager::GoToState(
//         FrameworkElement* control, Symbol stateName, bool useTransitions);
//
// Despite the parameter being a `FrameworkElement*`, GoToState only does useful
// work for a templated control — it walks the element's ControlTemplate looking
// for the VisualStateGroup that owns `stateName`. For a plain element with no
// template (a bare Grid, a TextBlock) there are no state groups to find, so the
// call simply returns false. That means we don't need to gate on "is a Control"
// ourselves: an unknown state name and a non-templated element both fall out as
// `false` naturally, which is exactly the contract we want. (We expose GoToState
// rather than GoToElementState because control state transitions are the common
// case; GoToElementState is for app-defined groups on an arbitrary root and can
// be added separately if a need shows up.)
//
// No VerifyAccess() — this must never throw across the C ABI (mirrors the
// text_get/set and dependency_object_* accessors). Single-thread (View) affinity
// is the caller's responsibility.

#include "noesis_shim.h"

#include <NsCore/Noesis.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Symbol.h>
#include <NsGui/FrameworkElement.h>
#include <NsGui/VisualStateManager.h>

extern "C" bool noesis_visual_state_go_to_state(
    void* element,
    const char* state,
    bool use_transitions) {
    if (!element || !state) return false;
    auto* base = static_cast<Noesis::BaseComponent*>(element);
    auto* fe = Noesis::DynamicCast<Noesis::FrameworkElement*>(base);
    if (!fe) return false;
    return Noesis::VisualStateManager::GoToState(fe, Noesis::Symbol(state), use_transitions);
}
