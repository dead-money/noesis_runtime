// VisualStateManager::GoToState, so Rust can drive a templated control's
// visual states (CommonStates, FocusStates, ...) from code.
//
// Returns false for an unknown state name or an element with no template state
// groups (a bare Grid or TextBlock), so there is no separate Control check.
// No VerifyAccess(): it would throw across the C ABI. The caller keeps the
// call on the view's thread.

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
